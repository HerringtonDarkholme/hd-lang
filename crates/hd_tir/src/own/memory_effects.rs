//! Complete call write effects and their monotone propagation. Parameter
//! masks follow ownership's existing 64-parameter convention. A write through
//! a parameter beyond that width conservatively becomes an external write.

use std::collections::VecDeque;

/// Memory a call may change: external state and/or its reference arguments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Writes {
    pub external: bool,
    pub parameters: u64,
    /// Unknown writes discharged only by proved generated-handle arguments.
    pub handles: u64,
}

impl Writes {
    pub const EXTERNAL: Self = Self {
        external: true,
        parameters: 0,
        handles: 0,
    };

    #[must_use]
    pub fn parameter(index: u32) -> Self {
        1u64.checked_shl(index)
            .map_or(Self::EXTERNAL, |parameters| Self {
                external: false,
                parameters,
                handles: 0,
            })
    }

    #[must_use]
    pub fn handle(index: u32) -> Self {
        1u64.checked_shl(index)
            .map_or(Self::EXTERNAL, |handles| Self {
                external: false,
                parameters: 0,
                handles,
            })
    }

    #[must_use]
    pub fn any(self) -> bool {
        self.external || self.parameters != 0 || self.handles != 0
    }

    /// External writes are top: further parameter bits convey no extra fact.
    pub fn join(&mut self, other: Self) -> bool {
        if self.external {
            return false;
        }
        if other.external {
            *self = Self::EXTERNAL;
            return true;
        }
        let before = (self.parameters, self.handles);
        self.parameters |= other.parameters;
        self.handles |= other.handles;
        before != (self.parameters, self.handles)
    }

    fn delta(self, sent: Self) -> Self {
        if sent.external {
            Self::default()
        } else if self.external {
            Self::EXTERNAL
        } else {
            Self {
                external: false,
                parameters: self.parameters & !sent.parameters,
                handles: self.handles & !sent.handles,
            }
        }
    }
}

/// Exact value identity in one immutable body. Unknown/nontransparent
/// producers remain their own instruction, for the client to classify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSource {
    Producer,
    Parameter(u32),
    Value(u32),
}

/// Only an exact reference to private storage can discharge a parameter write.
/// Further dereferences and reference-bearing aggregate fields stay external.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    Private,
    Parameter(u32),
    #[default]
    External,
}

impl Origin {
    #[must_use]
    pub fn writes(self) -> Writes {
        match self {
            Self::Private => Writes::default(),
            Self::Parameter(index) => Writes::parameter(index),
            Self::External => Writes::EXTERNAL,
        }
    }
}

/// A capability proved from a generated factory or forwarded unchanged.
/// Nominal handle types alone never establish this fact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HandleOrigin {
    Trusted,
    Parameter(u32),
    #[default]
    Unknown,
}

/// A complete callee effect mapped through this call's actual argument roots.
#[must_use]
pub fn through(writes: Writes, arguments: &[Origin]) -> Writes {
    through_with(writes, arguments, &[], &mut || {})
}

fn through_with(
    writes: Writes,
    arguments: &[Origin],
    handles: &[HandleOrigin],
    work: &mut impl FnMut(),
) -> Writes {
    work();
    if writes.external {
        return Writes::EXTERNAL;
    }
    let mut out = Writes::default();
    let mut parameters = writes.parameters;
    while parameters != 0 {
        work();
        let index = parameters.trailing_zeros() as usize;
        parameters &= parameters - 1;
        out.join(arguments.get(index).copied().unwrap_or_default().writes());
        if out.external {
            break;
        }
    }
    let mut requirements = writes.handles;
    while requirements != 0 && !out.external {
        work();
        let index = requirements.trailing_zeros() as usize;
        requirements &= requirements - 1;
        out.join(match handles.get(index).copied().unwrap_or_default() {
            HandleOrigin::Trusted => Writes::default(),
            HandleOrigin::Parameter(index) => Writes::handle(index),
            HandleOrigin::Unknown => Writes::EXTERNAL,
        });
    }
    out
}

/// A call's contribution, including a conditional immutable-element loan.
/// The latter becomes a write if the consumer ceases to be read-only.
pub enum Transfer {
    Arguments(Vec<Origin>),
    WithHandles {
        references: Vec<Origin>,
        handles: Vec<HandleOrigin>,
    },
    Any(Writes),
}

impl Transfer {
    #[must_use]
    pub fn apply(&self, effects: Writes) -> Writes {
        self.apply_with(effects, &mut || {})
    }

    fn apply_with(&self, effects: Writes, work: &mut impl FnMut()) -> Writes {
        match self {
            Self::Arguments(arguments) => through_with(effects, arguments, &[], work),
            Self::WithHandles {
                references,
                handles,
            } => through_with(effects, references, handles, work),
            Self::Any(writes) => {
                work();
                if effects.any() {
                    *writes
                } else {
                    Writes::default()
                }
            }
        }
    }
}

/// One internal call edge in a component. Arguments need at most 64 roots;
/// missing roots conservatively map to external storage.
pub struct Call {
    pub callee: usize,
    pub transfer: Transfer,
}

/// One call equation before the driver assigns dense component node IDs.
pub struct CallEffect {
    pub instruction: u32,
    pub callee: crate::ir::Callee,
    pub transfer: Transfer,
}

pub struct BodyEffects {
    pub direct: Writes,
    pub calls: Vec<CallEffect>,
}

impl BodyEffects {
    #[must_use]
    pub fn evaluate(&self, mut policy: impl FnMut(&crate::ir::Callee) -> Writes) -> Writes {
        let mut out = self.direct;
        for call in &self.calls {
            if out.external {
                break;
            }
            out.join(call.transfer.apply(policy(&call.callee)));
        }
        out
    }
}

/// Direct effects already include calls to completed/outside components.
pub struct Equation {
    pub direct: Writes,
    pub calls: Vec<Call>,
}

/// Each node gains at most 64 parameter and 64 handle bits then external/top. Each edge
/// transfers each gained bit once. Work is O(V + E + 129E), space O(V + E+A),
/// for nodes V, calls E and stored argument roots A. No native recursion.
#[must_use]
pub fn solve(equations: &[Equation]) -> Vec<Writes> {
    solve_with(equations, &mut || {})
}

fn solve_with(equations: &[Equation], work: &mut impl FnMut()) -> Vec<Writes> {
    let mut reverse = vec![Vec::new(); equations.len()];
    let mut values = Vec::with_capacity(equations.len());
    let mut sent = vec![Writes::default(); equations.len()];
    let mut queued = vec![false; equations.len()];
    let mut todo = VecDeque::new();
    for (caller, equation) in equations.iter().enumerate() {
        work();
        values.push(equation.direct);
        if equation.direct.any() {
            queued[caller] = true;
            todo.push_back(caller);
        }
        for call in &equation.calls {
            work();
            // ruled out: component construction assigns every internal callee
            // one dense node index before it builds the equations.
            assert!(
                call.callee < equations.len(),
                "effect equation names a component node"
            );
            reverse[call.callee].push((caller, &call.transfer));
        }
    }
    while let Some(callee) = todo.pop_front() {
        work();
        queued[callee] = false;
        let delta = values[callee].delta(sent[callee]);
        sent[callee] = values[callee];
        for &(caller, transfer) in &reverse[callee] {
            let changed = transfer.apply_with(delta, work);
            if values[caller].join(changed) && !queued[caller] {
                queued[caller] = true;
                todo.push_back(caller);
            }
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_arguments_discharge_only_complete_parameter_effects() {
        let call = |callee, origin| Call {
            callee,
            transfer: Transfer::Arguments(vec![origin]),
        };
        let equations = [
            Equation {
                direct: Writes::parameter(0),
                calls: vec![],
            },
            Equation {
                direct: Writes::default(),
                calls: vec![call(0, Origin::Private)],
            },
            Equation {
                direct: Writes::default(),
                calls: vec![call(0, Origin::Parameter(1))],
            },
            Equation {
                direct: Writes::default(),
                calls: vec![call(0, Origin::External)],
            },
            Equation {
                direct: Writes::default(),
                calls: vec![call(3, Origin::Private)],
            },
        ];
        assert_eq!(
            solve(&equations),
            [
                Writes::parameter(0),
                Writes::default(),
                Writes::parameter(1),
                Writes::EXTERNAL,
                Writes::EXTERNAL
            ]
        );
        assert_eq!(Writes::parameter(64), Writes::EXTERNAL);
        assert_eq!(through(Writes::parameter(1), &[]), Writes::EXTERNAL);
    }

    #[test]
    fn all_parameter_bits_propagate_with_bounded_per_call_transfer_work() {
        let width = 128;
        let equations: Vec<_> = (0..width)
            .map(|node| Equation {
                direct: Writes::parameter(crate::ir::u32_of(node % 64)),
                calls: vec![Call {
                    callee: (node + 1) % width,
                    transfer: Transfer::Arguments((0..64).map(Origin::Parameter).collect()),
                }],
            })
            .collect();
        let mut work = 0;
        let out = solve_with(&equations, &mut || work += 1);
        assert!(out.iter().all(|effect| *effect
            == Writes {
                external: false,
                parameters: u64::MAX,
                handles: 0,
            }));
        assert!(work <= 260 * width, "64 bits over {width} calls: {work}");
    }

    #[test]
    fn recursive_call_transfers_count_linear_width_and_depth_work() {
        let run = |width: usize, depth: usize| {
            let mut equations = Vec::with_capacity(width * depth);
            for layer in 0..depth {
                for column in 0..width {
                    let mut calls = vec![Call {
                        callee: layer * width + (column + 1) % width,
                        transfer: Transfer::Arguments(vec![Origin::Parameter(0)]),
                    }];
                    if layer != 0 {
                        calls.push(Call {
                            callee: (layer - 1) * width + column,
                            transfer: Transfer::Arguments(vec![Origin::Parameter(0)]),
                        });
                    }
                    equations.push(Equation {
                        direct: if layer == 0 && column == 0 {
                            Writes::parameter(0)
                        } else {
                            Writes::default()
                        },
                        calls,
                    });
                }
            }
            let mut work = 0;
            let out = solve_with(&equations, &mut || work += 1);
            assert!(out.iter().all(|&effect| effect == Writes::parameter(0)));
            assert!(work <= 12 * width * depth, "{width} x {depth}: {work}");
            work
        };
        let small = run(100, 20);
        let large = run(1_000, 20);
        assert!(large <= 12 * small, "100: {small}; 1000: {large}");
        run(20, 1_000);
    }
    #[test]
    fn only_proved_or_forwarded_handle_capabilities_discharge_writes() {
        let map = |origin| Transfer::WithHandles {
            references: vec![],
            handles: vec![origin],
        };
        assert_eq!(
            map(HandleOrigin::Trusted).apply(Writes::handle(0)),
            Writes::default()
        );
        assert_eq!(
            map(HandleOrigin::Parameter(3)).apply(Writes::handle(0)),
            Writes::handle(3)
        );
        assert_eq!(
            map(HandleOrigin::Unknown).apply(Writes::handle(0)),
            Writes::EXTERNAL
        );
        assert_eq!(
            map(HandleOrigin::Trusted).apply(Writes::EXTERNAL),
            Writes::EXTERNAL
        );
        assert_eq!(Writes::handle(64), Writes::EXTERNAL);
    }

    #[test]
    fn conditional_handle_dependencies_count_linear_width_and_depth_work() {
        let run = |width: usize, depth: usize| {
            let mut equations = Vec::new();
            for layer in 0..depth {
                for column in 0..width {
                    let mut calls = vec![Call {
                        callee: layer * width + (column + 1) % width,
                        transfer: Transfer::WithHandles {
                            references: vec![],
                            handles: vec![HandleOrigin::Parameter(0)],
                        },
                    }];
                    if layer != 0 {
                        calls.push(Call {
                            callee: (layer - 1) * width + column,
                            transfer: Transfer::WithHandles {
                                references: vec![],
                                handles: vec![HandleOrigin::Parameter(0)],
                            },
                        });
                    }
                    equations.push(Equation {
                        direct: if layer == 0 && column == 0 {
                            Writes::handle(0)
                        } else {
                            Writes::default()
                        },
                        calls,
                    });
                }
            }
            let mut work = 0;
            let out = solve_with(&equations, &mut || work += 1);
            assert!(out.iter().all(|effect| *effect == Writes::handle(0)));
            assert!(work <= 12 * width * depth, "{width}x{depth}: {work}");
            work
        };
        let small = run(100, 20);
        let large = run(1000, 20);
        assert!(large <= 12 * small);
        run(20, 1000);
    }
}
