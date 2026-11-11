//! A cold frame's local-reference owners, including conditional payloads.
//!
//! The ownership walk includes thin local references; the physical count
//! walk excludes them. Subtract their leaves under the same discriminant
//! path. Guard prefixes are interned, so a depth-D guard protecting R
//! references occupies D + R records, rather than D * R copied predicates.

use hd_base::{HashMap, HashSet};

use crate::layout::{Discr, Walk};

pub(crate) fn for_type(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    ty: hd_types::Ty,
) -> Plan {
    plan(
        crate::layout::walk(pool, env, ty),
        crate::layout::count_walk(pool, env, ty),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Guard {
    pub parent: Option<usize>,
    pub discr: Discr,
    pub variant: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Referent {
    pub guard: Option<usize>,
    pub value: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Parents precede children, allowing one forward guard evaluation.
    pub guards: Vec<Guard>,
    pub refs: Vec<Referent>,
}

/// O(A + C) expected work and space for ownership/count walk nodes and
/// leaves. Both owned trees are consumed with a worklist, including their
/// destruction; neither traversal nor cleanup recurses with input depth.
pub(crate) fn plan(all: Option<Walk>, counted: Option<Walk>) -> Plan {
    plan_with(all, counted, &mut || {})
}

fn plan_with(all: Option<Walk>, counted: Option<Walk>, visit: &mut impl FnMut()) -> Plan {
    let mut guards = Vec::new();
    let mut index = HashMap::default();
    let all = leaves(all, &mut guards, &mut index, visit);
    let counted: HashSet<_> = leaves(counted, &mut guards, &mut index, visit)
        .into_iter()
        .collect();
    let refs: Vec<_> = all
        .into_iter()
        .filter(|leaf| {
            visit();
            !counted.contains(leaf)
        })
        .collect();
    if refs.is_empty() {
        return Plan::default();
    }

    // Keep only predicates that guard a retained leaf. Walking the guard
    // arena backwards propagates need to ancestors once, not per leaf.
    let mut needed = vec![false; guards.len()];
    for leaf in &refs {
        visit();
        if let Some(guard) = leaf.guard {
            needed[guard] = true;
        }
    }
    for (id, guard) in guards.iter().enumerate().rev() {
        visit();
        if needed[id]
            && let Some(parent) = guard.parent
        {
            needed[parent] = true;
        }
    }
    let mut remap = vec![0; guards.len()];
    let mut out = Plan::default();
    for (id, guard) in guards.into_iter().enumerate() {
        visit();
        if needed[id] {
            remap[id] = out.guards.len();
            out.guards.push(Guard {
                // Every kept parent precedes its child and was marked
                // needed by the backwards pass, so its new ID is set.
                parent: guard.parent.map(|parent| remap[parent]),
                ..guard
            });
        }
    }
    out.refs = refs
        .into_iter()
        .map(|leaf| Referent {
            guard: leaf.guard.map(|guard| remap[guard]),
            ..leaf
        })
        .collect();
    out
}

fn leaves(
    root: Option<Walk>,
    guards: &mut Vec<Guard>,
    index: &mut HashMap<Guard, usize>,
    visit: &mut impl FnMut(),
) -> Vec<Referent> {
    let mut out = Vec::new();
    let mut work: Vec<_> = root.into_iter().map(|walk| (walk, None)).collect();
    while let Some((walk, parent)) = work.pop() {
        visit();
        match walk {
            Walk::Ref(value) | Walk::Known(value) => out.push(Referent {
                guard: parent,
                value,
            }),
            Walk::Glue => {}
            Walk::Seq(children) => {
                work.extend(children.into_iter().rev().map(|child| (child, parent)));
            }
            Walk::Switch(discr, arms) => {
                for (variant, arm) in arms.into_iter().enumerate().rev() {
                    visit();
                    if let Some(arm) = arm {
                        let guard = Guard {
                            parent,
                            discr: *discr,
                            variant,
                        };
                        let id = *index.entry(guard).or_insert_with(|| {
                            let id = guards.len();
                            guards.push(guard);
                            id
                        });
                        work.push((arm, Some(id)));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Enc, Leaf};
    use crate::module::Val;

    fn discr(value: u32) -> Discr {
        Discr {
            leaf: Leaf {
                offset: value * 4,
                val: Val::I32,
                bytes: 4,
                signed: false,
            },
            value,
            enc: Enc::Direct,
        }
    }

    #[test]
    fn overlapping_payload_slots_are_subtracted_only_under_the_same_guard() {
        let all = Walk::Switch(
            Box::new(discr(0)),
            vec![Some(Walk::Ref(1)), Some(Walk::Known(1))],
        );
        let counted = Walk::Switch(Box::new(discr(0)), vec![None, Some(Walk::Known(1))]);
        let p = plan(Some(all), Some(counted));
        assert_eq!(
            p.refs,
            [Referent {
                guard: Some(0),
                value: 1
            }]
        );
        assert_eq!(
            p.guards,
            [Guard {
                parent: None,
                discr: discr(0),
                variant: 0
            }]
        );
    }

    #[test]
    fn unconditional_and_nested_reference_owners_keep_shared_guard_prefixes() {
        let all = Walk::Seq(vec![
            Walk::Ref(0),
            Walk::Ref(1),
            Walk::Switch(
                Box::new(discr(2)),
                vec![
                    None,
                    Some(Walk::Switch(
                        Box::new(discr(3)),
                        vec![Some(Walk::Seq(vec![Walk::Ref(4), Walk::Ref(5)])), None],
                    )),
                ],
            ),
        ]);
        let p = plan(Some(all), Some(Walk::Ref(1)));
        assert_eq!(p.guards.len(), 2);
        assert_eq!(p.guards[1].parent, Some(0));
        assert_eq!(
            p.refs,
            [
                Referent {
                    guard: None,
                    value: 0
                },
                Referent {
                    guard: Some(1),
                    value: 4
                },
                Referent {
                    guard: Some(1),
                    value: 5
                },
            ]
        );
    }

    #[test]
    fn no_physical_local_references_need_no_frame_slots_or_guards() {
        let walks = || Walk::Switch(Box::new(discr(0)), vec![Some(Walk::Ref(1)), None]);
        assert_eq!(plan(Some(walks()), Some(walks())), Plan::default());
        assert_eq!(plan(None, None), Plan::default());
    }

    #[test]
    fn wide_and_deep_payloads_count_linear_work_without_copying_paths() {
        for (width, depth) in [(100, 20), (1_000, 20), (20, 1_000)] {
            let mut walk = Walk::Seq((0..width).map(Walk::Ref).collect());
            for at in 0..depth {
                walk = Walk::Switch(Box::new(discr(at)), vec![Some(walk), None]);
            }
            let mut work = 0;
            let p = plan_with(Some(walk), None, &mut || work += 1);
            assert_eq!(p.refs.len(), width as usize);
            assert_eq!(p.guards.len(), depth as usize);
            assert!(work <= 5 * (width + depth + 1), "{width} {depth}: {work}");
        }
    }
}
