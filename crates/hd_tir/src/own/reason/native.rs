//! Native checked builds label and verify every body. Batched next-use
//! queries and a write sweep replace the labels-only walk's per-copy
//! scans. The browser keeps the original walk: its counts-view path
//! otherwise links this index even though it never runs the verifier.

use super::{Reason, future, held_source, loan_holds, moved_consumers, read_start, source};
use crate::ir::{Body, NONE, Proj, ReadMode, Root, Tag, Topology, ref_word, u32_of};
use crate::own::{OwnCx, Role, consumes, operands};

/// The reason of each `copy` read of `b`, a body after the passes, in
/// `out` (`None` for any other instruction): `Stored` when a use of the
/// read's value consumes it while its source keeps its own count (the
/// source owns none to give: a lent parameter, a borrowed temporary, a
/// place behind a dereference, in a buffer or in a top-level binding;
/// or the source is used again after the read), where a store of the
/// read alone into a value local whose every use lends binds the value
/// and consumes nothing; `SecondOwner` when the
/// value only lends, and a write may change its source while it lives (a
/// store into or change of the same local, a mutable `Ref` of it,
/// or, for a place behind a dereference or in a buffer, any write through
/// one and any call that may write; a drop or a move of the source is
/// no write: where the copy borrowed, the passes would place them after
/// it); otherwise `Conservative("copy
/// read")`, a copy no fact here explains. Labels only: the emitter's
/// counts view reads them.
pub fn copy_reasons(b: &Body, topo: &Topology, cx: &dyn OwnCx, out: &mut Vec<Option<Reason>>) {
    let n = b.len();
    out.clear();
    out.resize(n, None);
    let copy = |i: usize| b.tags[i] == Tag::Read && b.data[i][1] == ReadMode::Copy as u32;
    if !(0..n).any(copy) {
        return;
    }
    // Per value: whether a use consumes it, its last use by index, and
    // how many uses it has besides drops.
    let (mut consumed, mut last, mut count) = (vec![false; n], vec![NONE; n], vec![0u32; n]);
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            let v = op.value as usize;
            if op.role == Role::Dropped || v >= n {
                continue;
            }
            last[v] = if last[v] == NONE {
                u32_of(i)
            } else {
                last[v].max(u32_of(i))
            };
            count[v] += 1;
            let owner = if op.role == Role::Tail && !topo.is_root[i] {
                topo.owner[i]
            } else {
                NONE
            };
            consumed[v] |= consumes(b, cx, i, op, owner);
        }
    }
    // A store into all of a value local whose every use lends binds the
    // value, as a borrow's binding does: it consumes nothing, and the
    // value lives as long as the local is named.
    let mut classes = Vec::new();
    crate::own::values::classify(b, &mut classes);
    let mut lends_only: Vec<bool> = classes
        .iter()
        .map(|c| matches!(c, crate::own::values::Local::Value(_)))
        .collect();
    let mut local_last = vec![NONE; b.local_ty.len()];
    for (i, &takes) in consumed.iter().enumerate() {
        match b.tags[i] {
            Tag::Read | Tag::Drop | Tag::Ref => {
                let p = b.view(b.data[i][0]);
                if let Some(l) = p.local() {
                    local_last[l as usize] = u32_of(i);
                }
                if b.tags[i] == Tag::Read
                    && takes
                    && p.steps(b)
                        .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
                    && let Root::Local(l) = p.root
                {
                    lends_only[l as usize] = false;
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    if !b.cap_mode.get(c as usize).is_some_and(|m| m.shares()) {
                        lends_only[b.cap_local[c as usize].idx()] = false;
                    }
                }
            }
            _ => {}
        }
    }
    for i in 0..n {
        if b.tags[i] != Tag::Store {
            continue;
        }
        let p = b.view(b.data[i][0]);
        let v = b.data[i][1] as usize;
        if let Some(l) = p.local()
            && v < n
            && p.whole()
            && lends_only[l as usize]
            && count[v] == 1
        {
            consumed[v] = false;
            if local_last[l as usize] != NONE {
                last[v] = last[v].max(local_last[l as usize]);
            }
        }
    }
    let mut owned = vec![false; b.local_ty.len()];
    for &(_, l) in &b.exit_drops {
        owned[l.idx()] = true;
    }
    moved_consumers(b, &mut consumed);
    let used = future::uses(b, topo);
    let changed = changes(b, topo, cx, &last);
    for r in 0..n {
        if !copy(r) {
            continue;
        }
        let src = source(b, b.data[r][0]);
        out[r] = Some(if consumed[r] {
            let gives = match src.root {
                Some(Root::Local(l)) => owned[l as usize] && !src.behind,
                Some(Root::Value(v)) => !src.behind && !b.is_borrow(v),
                _ => false,
            };
            if !gives || used[r] {
                Reason::Stored
            } else {
                Reason::Conservative("copy read")
            }
        } else if changed[r] {
            Reason::SecondOwner
        } else {
            Reason::Conservative("copy read")
        });
    }
}

/// Answer all copy lifetimes in one sweep. A query ends at its value's
/// last use; the latest local/indirect write there answers its range.
pub(super) fn changes(b: &Body, topo: &Topology, cx: &dyn OwnCx, last: &[u32]) -> Vec<bool> {
    let n = b.len();
    let mut head = vec![NONE; n];
    let mut next = vec![NONE; n];
    for r in 0..n {
        if b.tags[r] == Tag::Read && b.data[r][1] == ReadMode::Copy as u32 && last[r] != NONE {
            let end = last[r] as usize;
            next[r] = head[end];
            head[end] = u32_of(r);
        }
    }
    let loans = loan_holds(b);
    let mut written = vec![NONE; b.local_ty.len()];
    let mut indirect = NONE;
    let mut out = vec![false; n];
    for (i, &first_query) in head.iter().enumerate() {
        match b.tags[i] {
            Tag::Store | Tag::IsUnique | Tag::Ref => {
                let p = b.view(b.data[i][0]);
                let changes = b.tags[i] != Tag::Ref || b.data[i][1] & ref_word::MUT != 0;
                if changes && let Root::Local(l) = p.root {
                    written[l as usize] = u32_of(i);
                }
                if b.tags[i] != Tag::Ref
                    && p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
                {
                    indirect = u32_of(i);
                }
            }
            Tag::Call => {
                if crate::ir::Callee::from_words(b.record(b.data[i][0]))
                    .is_none_or(|c| cx.may_write(&c))
                {
                    indirect = u32_of(i);
                }
            }
            Tag::CallValue | Tag::CallDyn => indirect = u32_of(i),
            _ => {}
        }
        let mut r = first_query;
        while r != NONE {
            let read = r as usize;
            let src = held_source(b, &loans, b.data[read][0]);
            let start = read_start(b, topo, read);
            let in_range = |at: u32| at != NONE && at >= start;
            out[read] = matches!(src.root, Some(Root::Local(l)) if in_range(written[l as usize]))
                || (src.behind && in_range(indirect));
            r = next[read];
        }
    }
    out
}

/// Step 30a's independent verifier. `stored` has a consuming use;
/// `second owner` has none except a binding's whole store, and a possible
/// write at or after its statement. This checks the labels, not whether
/// the old ownership passes could have removed the copy. Native checked
/// builds call it even when no counts view is requested. No web code.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn check_copy_reasons(
    b: &Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    reasons: &[Option<Reason>],
) -> Vec<String> {
    let n = b.len();
    let mut takers = vec![Vec::new(); n];
    let mut moved = vec![false; n];
    let mut ops = Vec::new();
    let mut writes_after = vec![false; n + 1];
    for i in (0..n).rev() {
        writes_after[i] = writes_after[i + 1]
            || match b.tags[i] {
                Tag::Store | Tag::IsUnique | Tag::Call | Tag::CallValue | Tag::CallDyn => true,
                Tag::Ref => b.data[i][1] & ref_word::MUT != 0,
                _ => false,
            };
        operands(b, i, &mut ops);
        for op in &ops {
            let value = op.value as usize;
            if value >= n || op.role == Role::Dropped {
                continue;
            }
            let owner = if op.role == Role::Tail && !topo.is_root[i] {
                topo.owner[i]
            } else {
                NONE
            };
            // A `move` read hands the value on to what consumes it.
            moved[value] |= b.tags[i] == Tag::Read
                && b.data[i][1] == ReadMode::Move as u32
                && b.view(b.data[i][0]).root == Root::Value(op.value);
            if consumes(b, cx, i, op, owner) {
                takers[value].push(i);
            }
        }
    }
    let mut bad = Vec::new();
    for (read, reason) in reasons.iter().enumerate().take(n) {
        let valid = match reason {
            Some(Reason::Stored) => moved[read] || !takers[read].is_empty(),
            Some(Reason::SecondOwner) => {
                takers[read].iter().all(|&at| {
                    b.tags[at] == Tag::Store && b.data[at][1] as usize == read && {
                        let place = b.view(b.data[at][0]);
                        place.whole() && place.local().is_some()
                    }
                }) && writes_after[read_start(b, topo, read) as usize]
            }
            Some(Reason::Conservative(_)) | None => true,
            Some(_) => false,
        };
        if !valid {
            bad.push(format!(
                "%{read}: a copy read whose reason `{}` its facts do not show",
                reason.map_or_else(String::new, |reason| reason.to_string())
            ));
        }
    }
    bad
}
