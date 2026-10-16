//! Which locals of a suspending body need a frame field
//! (`crates/hd_wasm/DESIGN.md` §14.2, item 1): one backward liveness pass
//! over the body's Wasm, before any save or reload is written.
//!
//! A resume enters the body at its start, skips code under `pc` tests and
//! goes on from its state. That is one path through the body's control
//! flow from its entry. So a local that every path from the entry writes
//! before it reads needs no field: a resume writes it before reading it
//! too. The others, the locals live at the entry, are saved.

use wasmparser::{BinaryReader, FunctionBody, Operator};

/// One instruction's effect on the analysis.
enum Step {
    Read(u32),
    Write(u32),
    /// `block`, or `if` (with its `else`, or its `end` when it has none).
    Open {
        looped: bool,
        alt: Option<usize>,
    },
    Else,
    End,
    /// `br` and friends: unconditional or not, to each relative depth.
    Branch {
        depths: Vec<u32>,
        falls: bool,
    },
    /// `return`, `unreachable`, `throw`: no successor in the body.
    Stop,
    Other,
}

/// The locals live at the start of the function `body` (its encoding, local
/// declarations first), as one flag per parameter and local; `None` when
/// the body does not parse.
#[must_use]
pub fn live_at_entry(body: &[u8], nparams: u32) -> Option<Vec<bool>> {
    let f = FunctionBody::new(BinaryReader::new(body, 0));
    let mut nlocals = nparams;
    for l in f.get_locals_reader().ok()? {
        nlocals += l.ok()?.0;
    }
    let mut steps = Vec::new();
    let mut ops = f.get_operators_reader().ok()?;
    while !ops.eof() {
        steps.push(step(&ops.read().ok()?));
    }
    // Each construct's start and end, and where an `if` goes when false.
    let mut ends = vec![usize::MAX; steps.len()];
    let mut elses = vec![usize::MAX; steps.len()];
    let mut open = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        match s {
            Step::Open { .. } => open.push(i),
            Step::Else => elses[*open.last()?] = i,
            Step::End => {
                // The function's own `end` closes no construct.
                if let Some(o) = open.pop() {
                    ends[o] = i;
                }
            }
            _ => {}
        }
    }
    // Successors: a branch to a block or `if` goes to its `end`, to a
    // loop to its start; past the outermost construct it leaves the body.
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); steps.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        let target = |stack: &[usize], d: u32| -> Option<usize> {
            let k = stack.len().checked_sub(1 + d as usize)?;
            let o = stack[k];
            match steps[o] {
                Step::Open { looped: true, .. } => Some(o),
                _ => Some(ends[o]),
            }
        };
        let next = (i + 1 < steps.len()).then_some(i + 1);
        match s {
            Step::Open { looped, alt } => {
                stack.push(i);
                succ[i].extend(next);
                if alt.is_some() && !looped {
                    // An `if`: false goes past its `else`, or to its `end`.
                    let e = elses[i];
                    succ[i].push(if e == usize::MAX { ends[i] } else { e + 1 });
                }
            }
            Step::Else => {
                // The `then` arm falls through to the `end`.
                let o = *stack.last()?;
                succ[i].push(ends[o]);
            }
            Step::End => {
                stack.pop();
                succ[i].extend(next);
            }
            Step::Branch { depths, falls } => {
                for d in depths {
                    succ[i].extend(target(&stack, *d));
                }
                if *falls {
                    succ[i].extend(next);
                }
            }
            Step::Stop => {}
            Step::Read(_) | Step::Write(_) | Step::Other => succ[i].extend(next),
        }
    }
    // Backward to a fixed point: live before = (live after - written) + read.
    let words = (nlocals as usize).div_ceil(64);
    let mut live = vec![vec![0u64; words]; steps.len() + 1];
    let mut changed = true;
    while changed {
        changed = false;
        for i in (0..steps.len()).rev() {
            let mut out = vec![0u64; words];
            for &n in &succ[i] {
                for (o, x) in out.iter_mut().zip(&live[n]) {
                    *o |= x;
                }
            }
            match steps[i] {
                Step::Read(l) => out[l as usize / 64] |= 1 << (l % 64),
                Step::Write(l) => out[l as usize / 64] &= !(1 << (l % 64)),
                _ => {}
            }
            if out != live[i] {
                live[i] = out;
                changed = true;
            }
        }
    }
    let entry = live.first()?;
    Some(
        (0..nlocals)
            .map(|l| entry[l as usize / 64] & (1 << (l % 64)) != 0)
            .collect(),
    )
}

fn step(op: &Operator<'_>) -> Step {
    match op {
        Operator::LocalGet { local_index } => Step::Read(*local_index),
        // A tee writes, then pushes what it wrote.
        Operator::LocalSet { local_index } | Operator::LocalTee { local_index } => {
            Step::Write(*local_index)
        }
        Operator::Block { .. } | Operator::TryTable { .. } => Step::Open {
            looped: false,
            alt: None,
        },
        Operator::Loop { .. } => Step::Open {
            looped: true,
            alt: None,
        },
        Operator::If { .. } => Step::Open {
            looped: false,
            alt: Some(0),
        },
        Operator::Else => Step::Else,
        Operator::End => Step::End,
        Operator::Br { relative_depth } => Step::Branch {
            depths: vec![*relative_depth],
            falls: false,
        },
        Operator::BrIf { relative_depth }
        | Operator::BrOnNull { relative_depth }
        | Operator::BrOnNonNull { relative_depth }
        | Operator::BrOnCast { relative_depth, .. }
        | Operator::BrOnCastFail { relative_depth, .. } => Step::Branch {
            depths: vec![*relative_depth],
            falls: true,
        },
        Operator::BrTable { targets } => {
            let mut depths: Vec<u32> = targets.targets().filter_map(Result::ok).collect();
            depths.push(targets.default());
            Step::Branch {
                depths,
                falls: false,
            }
        }
        Operator::Return
        | Operator::Unreachable
        | Operator::Throw { .. }
        | Operator::ThrowRef
        | Operator::ReturnCall { .. }
        | Operator::ReturnCallRef { .. }
        | Operator::ReturnCallIndirect { .. } => Step::Stop,
        _ => Step::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::live_at_entry;
    use crate::VT;
    use crate::asm::Asm;

    #[test]
    fn a_local_written_before_every_read_is_not_live_at_entry() {
        // (p0) -> i32: `t` is written then read; `u` is read on the path
        // that skips its write.
        let mut a = Asm::new_dflt(vec![VT::I32]);
        let (t, u) = (a.local(VT::I32), a.local(VT::I32));
        a.i32(1);
        a.set(t);
        a.get(0);
        a.if_();
        a.i32(2);
        a.set(u);
        a.end();
        a.get(t);
        a.get(u);
        a.s().i32_add();
        let c = a.finish(vec![VT::I32]);
        let live = live_at_entry(&c.body, 1).expect("parses");
        assert_eq!(live, vec![true, false, true]);
    }

    #[test]
    fn a_loop_carries_a_read_back_to_its_start() {
        // `t` is read at the loop's top and written at its bottom: on the
        // first iteration the read comes first.
        let mut a = Asm::new_dflt(vec![]);
        let t = a.local(VT::I32);
        a.block();
        a.loop_();
        a.get(t);
        a.s().i32_eqz();
        a.br_if(1);
        a.i32(0);
        a.set(t);
        a.br(0);
        a.end();
        a.end();
        let c = a.finish(vec![]);
        let live = live_at_entry(&c.body, 0).expect("parses");
        assert_eq!(live, vec![true]);
    }
}
