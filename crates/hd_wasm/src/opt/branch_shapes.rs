//! Batch rule 5's branch-only arms and rule 6's value arms. Removing a
//! conditional never deletes a target of the kept arm: collect target use
//! once, then remap relative depths with a prefix of removed labels. Nested
//! rewrites share the same compaction.

use super::{Func, I, Ins, Remap};
use crate::module::WasmVal as _;
use hd_base::SliceSort as _;
use wasm_encoder::BlockType;

/// Whether a branch targets the label each instruction opens, by the
/// instruction's index: one pass over the body with a stack of open labels.
fn targeted(f: &Func) -> Vec<bool> {
    let mut referenced = vec![false; f.body.len()];
    let mut labels = Vec::new();
    for (at, ins) in f.body.iter().enumerate() {
        crate::work::control(1);
        // Only these scalar forms open/close a label or carry relative
        // targets; symbolic Ins variants carry no structured label operands.
        if matches!(ins, Ins::Op(I::Block(_) | I::Loop(_) | I::If(_))) {
            labels.push(at);
        }
        if matches!(ins, Ins::Op(I::End)) {
            labels.pop();
        }
        let mut mark = |depth: u32| {
            crate::work::control(1);
            if let Some(&target) = labels
                .len()
                .checked_sub(depth as usize + 1)
                .and_then(|n| labels.get(n))
            {
                referenced[target] = true;
            }
        };
        if let Ins::Op(I::Br(d) | I::BrIf(d)) = ins {
            mark(*d);
        }
        if let Ins::Op(I::BrTable(table, default)) = ins {
            for &d in f.label_list(*table).iter().chain([default]) {
                mark(d);
            }
        }
    }
    referenced
}

/// Rule 6 at every site of `sites` in one rebuild: `if; A; local.set x;
/// else; B; local.set x; end; local.get x`, where `x` is a local past the
/// parameters read once and written twice and no branch targets the `if`,
/// becomes `if (result t); A; else; B; end`. A rewrite keeps every label,
/// so no branch depth moves, and changes only `x`'s counts, which belong to
/// its site alone: the sites that hold in this body hold together, and
/// their edits never share an instruction.
///
/// Cost: one pass for the local counts, one for the branch targets (only
/// when a site's instructions match), one test per site and one rebuild,
/// linear in the body. Rewriting one site per pass over the body was
/// quadratic in the number of value `if`s, nested (#1905, #1913) or not.
pub(super) fn value_arms(f: &mut Func, np: u32, sites: &[(usize, Option<usize>, usize)]) -> bool {
    let (reads, writes) = super::counts(f);
    let mut referenced = None;
    // Each rewrite's `if` and its result type, and the three instructions it drops.
    let mut edits = Vec::new();
    for &(start, otherwise, end) in sites {
        crate::work::control(1);
        let b = &f.body;
        let (Ins::Op(I::If(BlockType::Empty)), Some(otherwise)) = (&b[start], otherwise) else {
            continue;
        };
        if let (
            Some(Ins::Op(I::LocalSet(x))),
            Some(Ins::Op(I::LocalSet(y))),
            Some(Ins::Op(I::LocalGet(z))),
        ) = (b.get(otherwise - 1), b.get(end - 1), b.get(end + 1))
            && x == y
            && y == z
            && *x >= np
            && reads[*x as usize] == 1
            && writes[*x as usize] == 2
            && !referenced.get_or_insert_with(|| targeted(f))[start]
        {
            let t = f.locals[(*x - np) as usize].wasm();
            edits.push((start, Some((t, otherwise, end))));
            edits.extend([otherwise - 1, end - 1, end + 1].map(|at| (at, None)));
        }
    }
    if edits.is_empty() {
        return false;
    }
    edits.sort_stable_by_key(|&(at, _)| at);
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len() - edits.len() * 3 / 4);
    let mut origins = Remap::new(f.origins.as_deref());
    let mut next = edits.into_iter().peekable();
    for (at, ins) in body.into_iter().enumerate() {
        crate::work::control(1);
        match next.next_if(|&(edit, _)| edit == at) {
            Some((_, Some((t, otherwise, end)))) => {
                let origin = origins
                    .first(at..at + 1)
                    .or_else(|| origins.first(otherwise - 1..otherwise))
                    .or_else(|| origins.first(end - 1..end))
                    .or_else(|| origins.first(end + 1..end + 2));
                out.push(Ins::Op(I::If(BlockType::Result(t))));
                origins.repeat(origin, 1);
            }
            Some((_, None)) => {}
            None => {
                out.push(ins);
                origins.copy(at..at + 1);
            }
        }
    }
    f.body = out;
    origins.finish(f);
    true
}

pub fn run(f: &mut Func, sites: &[(usize, Option<usize>, usize)]) -> bool {
    let plans: Vec<_> = sites
        .iter()
        .filter_map(|&(start, otherwise, end)| {
            crate::work::control(1);
            if !matches!(f.body[start], Ins::Op(I::If(BlockType::Empty))) {
                return None;
            }
            let then = start + 1..otherwise.unwrap_or(end);
            let other = otherwise.map_or(end..end, |e| e + 1..end);
            [(other, true), (then, false)]
                .into_iter()
                .find_map(|(exit, negate)| {
                    if let [Ins::Op(I::Br(depth))] = &f.body[exit.clone()]
                        && *depth >= 1
                    {
                        Some((start, otherwise, end, exit.start, *depth, negate))
                    } else {
                        None
                    }
                })
        })
        .collect();
    if plans.is_empty() {
        return false;
    }
    let referenced = targeted(f);
    let mut replace = vec![None; f.body.len()];
    let mut gone = vec![false; f.body.len()];
    let mut changed = false;
    for (start, otherwise, end, exit, depth, negate) in plans {
        crate::work::control(1);
        if referenced[start] {
            continue;
        }
        replace[start] = Some((depth, negate, exit));
        gone[start] = true;
        gone[exit] = true;
        gone[end] = true;
        if let Some(otherwise) = otherwise {
            gone[otherwise] = true;
        }
        changed = true;
    }
    if !changed {
        return false;
    }
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len());
    let mut origins = Remap::new(f.origins.as_deref());
    // Prefix entry 0 is outside every explicit label, the function target.
    let mut removed = vec![0u32];
    for (at, ins) in body.into_iter().enumerate() {
        crate::work::control(1);
        if matches!(ins, Ins::Op(I::Block(_) | I::Loop(_) | I::If(_))) {
            removed.push(removed[removed.len() - 1] + u32::from(gone[at]));
        }
        let depth = |d: u32| {
            let open = removed.len() - 1;
            d - (removed[open] - removed[open.saturating_sub(d as usize)])
        };
        if let Some((d, negate, exit)) = replace[at] {
            let origin = origins
                .first(at..at + 1)
                .or_else(|| origins.first(exit..exit + 1));
            if negate {
                out.push(Ins::Op(I::I32Eqz));
            }
            out.push(Ins::Op(I::BrIf(depth(d))));
            origins.repeat(origin, usize::from(negate) + 1);
        } else if !gone[at] {
            out.push(match ins {
                Ins::Op(I::Br(d)) => Ins::Op(I::Br(depth(d))),
                Ins::Op(I::BrIf(d)) => Ins::Op(I::BrIf(depth(d))),
                Ins::Op(I::BrTable(table, default)) => {
                    let targets: Vec<_> = f.label_list(table).iter().map(|&d| depth(d)).collect();
                    let table = if targets == f.label_list(table) {
                        table
                    } else {
                        f.intern_labels(targets)
                    };
                    Ins::Op(I::BrTable(table, depth(default)))
                }
                other => other,
            });
            origins.copy(at..at + 1);
        }
        if matches!(ins, Ins::Op(I::End)) {
            removed.pop();
        }
    }
    f.body = out;
    origins.finish(f);
    true
}
