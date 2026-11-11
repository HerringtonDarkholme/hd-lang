//! Lent cold frames (#1355): a suspending call made cold whose frame only
//! awaits in the caller's root, so that the caller's locals it refers to
//! outlive it. Its frame needs no count of their objects
//! (`ref_word::LENT`, the backend's `Part::Lent` constructor).
//!
//! A `Call` of a suspending item in the root is lent when
//!
//! 1. each argument that may hold a local reference a frame would count
//!    ([`OwnCx::may_frame_referent`]) is a `Ref` of the storage of a local
//!    of the root (`ref_word::LOCAL`, no count of its own, no frame slot)
//!    that only this call uses, and at least one is; and
//! 2. its frame value is used, `Drop`s and `LoanEnd`s aside, only by an
//!    `AwaitAll` or `AwaitValue` in the root, or by one `Store` into a bare
//!    local of the root that no closure captures, no `Ref` addresses, and
//!    whose every read is a whole read used only by such an await.
//!
//! A frame in a list (`race!`, `all!` of a list), a returned or passed-on
//! frame, and one a closure awaits keep their counts.
//!
//! Each awaiting instruction is then an access of every local the call's
//! `Ref`s address, so insertion drops those locals after it (rule 3): the
//! frame never outlives them. An await that the caller's cancellation
//! interrupts cancels and drops the frame in its unwind, before the exit
//! drops the locals. A frame dropped unawaited never ran, so its glue reads
//! no referent.
//!
//! Cost: one scan of the body, plus one scan of the uses of each candidate
//! call's frame value and of a local it is stored in; none in a root with
//! no `Call` of a suspending item.

use super::{OwnCx, operands};
use crate::ir::{Body, Callee, Role, Tag, Topology, local_flags, place, ref_word, u32_of};

/// Marks each lent call's `Ref` arguments with `ref_word::LENT`, and calls
/// `access(await, local)` for each await of its frame and each local its
/// `Ref`s address.
pub(super) fn run(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, mut access: impl FnMut(u32, u32)) {
    let n = b.len();
    let calls: Vec<usize> = (0..n)
        .filter(|&i| b.tags[i] == Tag::Call && topo.sub[i] == 0 && suspends(b, cx, i))
        .collect();
    if calls.is_empty() {
        return;
    }
    // Each value's users, by one scan: (user, role), `Drop`s and
    // `LoanEnd`s aside. Each bare local's reads, and whether a `Ref` or a
    // non-bare place touches it.
    let mut users: Vec<Vec<(u32, Role)>> = vec![Vec::new(); n];
    let mut reads: Vec<Vec<u32>> = vec![Vec::new(); b.local_ty.len()];
    let mut stores = vec![0u32; b.local_ty.len()];
    let mut opaque = vec![false; b.local_ty.len()];
    for c in &b.cap_local {
        opaque[c.idx()] = true;
    }
    let mut ops = Vec::new();
    for i in 0..n {
        let tag = b.tags[i];
        if matches!(tag, Tag::Read | Tag::Store | Tag::Ref)
            && let Some(l) = b.view(b.data[i][0]).local()
        {
            let bare = b.data[i][0] & place::BARE_LOCAL != 0;
            match tag {
                Tag::Read if bare && topo.sub[i] == 0 => reads[l as usize].push(u32_of(i)),
                Tag::Store if bare && topo.sub[i] == 0 => stores[l as usize] += 1,
                _ => opaque[l as usize] = true,
            }
        }
        if matches!(tag, Tag::Drop | Tag::LoanEnd) {
            continue;
        }
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped {
                users[op.value as usize].push((u32_of(i), op.role));
            }
        }
    }
    let mut refs = Vec::new();
    let mut points = Vec::new();
    for c in calls {
        refs.clear();
        points.clear();
        operands(b, c, &mut ops);
        let lendable = ops.iter().all(|op| {
            let v = op.value as usize;
            if !matches!(op.role, Role::Arg(_)) || !cx.may_frame_referent(b.ty[v]) {
                return true;
            }
            let w = b.data[v][1];
            let ok = b.tags[v] == Tag::Ref
                && topo.sub[v] == 0
                && w & ref_word::LOCAL != 0
                && w & (ref_word::TAKES | ref_word::FRAME) == 0
                && b.local_in_place(b.data[v][0]).is_some()
                && users[v].iter().all(|&(u, _)| u as usize == c);
            if ok {
                refs.push(v);
            }
            ok
        });
        if !lendable || refs.is_empty() {
            continue;
        }
        let frame_awaited = users[c].iter().all(|&(u, _)| {
            let u = u as usize;
            match b.tags[u] {
                Tag::AwaitAll | Tag::AwaitValue => {
                    awaited_only(b, topo, &[(u32_of(u), Role::Lend)], &mut points)
                }
                Tag::Store if b.data[u][1] == u32_of(c) && topo.sub[u] == 0 => {
                    let word = b.data[u][0];
                    let l = (word & !place::BARE_LOCAL) as usize;
                    word & place::BARE_LOCAL != 0
                        && b.local_flags[l] & local_flags::PARAM == 0
                        && !opaque[l]
                        && stores[l] == 1
                        && reads[l]
                            .iter()
                            .all(|&r| awaited_only(b, topo, &users[r as usize], &mut points))
                }
                _ => false,
            }
        });
        if !frame_awaited || points.is_empty() {
            continue;
        }
        for &v in &refs {
            b.data[v][1] |= ref_word::LENT;
            if let Some(l) = b.local_in_place(b.data[v][0]) {
                for &p in &points {
                    access(p, l.raw());
                }
            }
        }
    }
}

/// Whether a value with users `users` is used only by awaits in the root,
/// each of which goes to `points`.
fn awaited_only(b: &Body, topo: &Topology, users: &[(u32, Role)], points: &mut Vec<u32>) -> bool {
    users.iter().all(|&(u, _)| {
        let ok = matches!(b.tags[u as usize], Tag::AwaitAll | Tag::AwaitValue)
            && topo.sub[u as usize] == 0;
        if ok {
            points.push(u);
        }
        ok
    })
}

/// Whether `Call` `i` calls a suspending item, so that its value is the
/// callee's cold frame.
fn suspends(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    matches!(
        Callee::from_words(b.record(b.data[i][0])),
        Some(Callee::Item { def, .. }) if cx.suspending(def)
    )
}
