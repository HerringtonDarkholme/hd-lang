//! Late operand reads, before insertion: a call's or a construction's
//! operand that reads a local's place, or takes a reference to it, moves to
//! just before its consumer when a later operand reads the same local and
//! nothing in between can change it.
//!
//! `m.at(m.infos[i].position)` reads `m` for the receiver first, then
//! `m.infos` for the argument. Left in that order, the receiver read is
//! not the local's last access, so it copies `m` (a count of each of its
//! collections) or takes a counted reference, where
//! `p := m.infos[i].position; m.at(p)` moves it or takes the local's own
//! count. A read of a place has no effect, so reading it after the other
//! operands gives the same value when nothing between them stores to the
//! local, references it, or can reach it through memory, a call or a
//! suspension. The moved read keeps its place word, type and syntax node;
//! the body is rebuilt once ([`splice`]) and insertion then sees the
//! operands in that order.
//!
//! The read's place is the local or a field path of it: an element step's
//! index check could panic, and a dereference reads memory a call may
//! change, so both stay where they are.

use super::{Loc, Operand, OwnCx, Role, operands, splice, tracked};
use crate::ir::{Body, CaptureMode, IntrinsicOp, NONE, ReadMode, Tag, Topology, place, u32_of};

/// The pass's tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    ops: Vec<Operand>,
    /// How many operands name each instruction's value, counted once a
    /// body has a candidate.
    uses: Vec<u32>,
    /// (read, consumer, operand position) of each read that moves.
    found: Vec<(u32, u32, Loc)>,
    /// Per local, counted once a candidate needs it: how many `Ref`s are
    /// rooted at it, plus one for a shared capture.
    reach: Vec<u32>,
}

/// Moves each operand read that qualifies to just before its consumer.
/// Builds and rebuilds nothing for a body with none.
pub(crate) fn run(
    b: &mut Body,
    topo: &mut Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    t.uses.clear();
    t.found.clear();
    t.reach.clear();
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..b.len() {
        if !matches!(
            b.tags[i],
            Tag::Call
                | Tag::CallValue
                | Tag::CallHost
                | Tag::Intrinsic
                | Tag::NewStruct
                | Tag::NewVariant
                | Tag::NewTuple
                | Tag::NewList
                | Tag::NewMap
        ) {
            continue;
        }
        operands(b, i, &mut ops);
        for &op in &ops {
            let v = op.value as usize;
            if !matches!(op.role, Role::Arg(_) | Role::Consume) || v >= i || !tracked(b, cx, v) {
                continue;
            }
            let Some(l) = field_path_local(b, v) else {
                continue;
            };
            if !clear_between(b, l, v, i, &mut t.reach) {
                continue;
            }
            if t.uses.is_empty() {
                count_uses(b, &mut t.ops, &mut t.uses);
            }
            if t.uses[v] == 1 {
                t.found.push((u32_of(v), u32_of(i), op.at));
            }
        }
    }
    t.ops = ops;
    if t.found.is_empty() {
        return Ok(());
    }
    topo.build(b);
    edits.clear();
    for &(v, i, at) in &t.found {
        let (x, c) = (v as usize, i as usize);
        if topo.blk[x] == NONE || topo.blk[x] != topo.blk[c] {
            continue;
        }
        let e = edits.insert(
            topo.blk[c],
            topo.pos[c],
            b.tags[x],
            b.data[x],
            b.ty[x],
            b.syn[x],
        );
        edits.rewrite(i, at, e);
        edits.remove(v);
    }
    splice::apply(b, topo, edits)?;
    edits.clear();
    Ok(())
}

/// The local whose place `Read use` or `Ref` `v` names, when the place is
/// the local or a field path of it.
fn field_path_local(b: &Body, v: usize) -> Option<u32> {
    match b.tags[v] {
        Tag::Read if b.data[v][1] == ReadMode::Use as u32 => {}
        Tag::Ref => {}
        _ => return None,
    }
    let view = b.view(b.data[v][0]);
    let l = view.local()?;
    b.extra[view.from as usize..view.to as usize]
        .iter()
        .all(|&w| w & place::PROJ_KIND == place::PROJ_FIELD)
        .then_some(l)
}

/// How many operands name each instruction's value.
fn count_uses(b: &Body, ops: &mut Vec<Operand>, uses: &mut Vec<u32>) {
    uses.clear();
    uses.resize(b.len(), 0);
    for i in 0..b.len() {
        operands(b, i, ops);
        for op in ops.iter() {
            uses[op.value as usize] += 1;
        }
    }
}

/// Whether the instructions strictly between `v` and its consumer `i`
/// read local `l` where `v`'s place overlaps, so `v` is not its last
/// access (a disjoint field is pair cancellation's, `super::pairs`), and
/// none of them can change `l`: no store or reference rooted at `l` (through a chain of
/// reads too), no closure that captures it other than by copy, no
/// suspension, and no collection change. When a reference or a shared
/// capture elsewhere in the body may reach `l`, no call or store at all.
fn clear_between(b: &Body, l: u32, v: usize, i: usize, reach: &mut Vec<u32>) -> bool {
    let mut reads = false;
    let mut writes = false;
    for x in v + 1..i {
        match b.tags[x] {
            Tag::Read => reads |= overlaps(b, b.data[v][0], b.data[x][0], l),
            Tag::Store | Tag::Ref => {
                if rooted_at(b, b.data[x][0], l) {
                    return false;
                }
                writes |= b.tags[x] == Tag::Store;
            }
            Tag::Closure => {
                let rec = b.record(b.data[x][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    if b.cap_local[c as usize].raw() == l {
                        if b.cap_mode.get(c as usize) != Some(&CaptureMode::Copy) {
                            return false;
                        }
                        reads = true;
                    }
                }
            }
            Tag::Intrinsic => match IntrinsicOp::from_u32(b.data[x][0]) {
                Some(
                    IntrinsicOp::ListLen
                    | IntrinsicOp::MapLen
                    | IntrinsicOp::MapGet
                    | IntrinsicOp::StrIndex
                    | IntrinsicOp::StrConcat
                    | IntrinsicOp::StrEq
                    | IntrinsicOp::MapEntry,
                ) => {}
                _ => return false,
            },
            Tag::Call | Tag::CallValue | Tag::CallHost => writes = true,
            t if crate::ir::is_suspension(t) => return false,
            _ => {}
        }
    }
    if !reads {
        return false;
    }
    if writes && reach.is_empty() {
        count_reach(b, reach);
    }
    !writes || reach[l as usize] == u32::from(b.tags[v] == Tag::Ref)
}

/// Whether place word `w` reads part of the place `p` of local `l` names
/// or contains it: one's field path is a prefix of the other's. A place
/// rooted at `l` through a chain of reads counts as overlapping.
fn overlaps(b: &Body, p: u32, w: u32, l: u32) -> bool {
    let (pv, wv) = (b.view(p), b.view(w));
    if wv.local() != Some(l) {
        return rooted_at(b, w, l);
    }
    let a = &b.extra[pv.from as usize..pv.to as usize];
    let c = &b.extra[wv.from as usize..wv.to as usize];
    let k = a.len().min(c.len());
    a[..k] == c[..k]
}

/// Whether place word `w` is rooted at local `l` ([`root_local`]).
fn rooted_at(b: &Body, w: u32, l: u32) -> bool {
    root_local(b, w) == Some(l)
}

/// The local place word `w` is rooted at, directly or through a chain of
/// reads (`[%r .a]` after `%r = Read [$l]`).
fn root_local(b: &Body, mut w: u32) -> Option<u32> {
    for _ in 0..64 {
        let view = b.view(w);
        if let Some(l) = view.local() {
            return Some(l);
        }
        let r = view.value()? as usize;
        if b.tags[r] != Tag::Read {
            return None;
        }
        w = b.data[r][0];
    }
    None
}

/// Counts, per local, the `Ref`s rooted at it and its shared captures:
/// whatever may reach it through memory.
fn count_reach(b: &Body, reach: &mut Vec<u32>) {
    reach.clear();
    reach.resize(b.local_ty.len(), 0);
    for x in 0..b.len() {
        match b.tags[x] {
            Tag::Ref => {
                if let Some(l) = root_local(b, b.data[x][0]) {
                    reach[l as usize] += 1;
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[x][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    if b.cap_mode.get(c as usize) == Some(&CaptureMode::Shared) {
                        reach[b.cap_local[c as usize].raw() as usize] += 1;
                    }
                }
            }
            _ => {}
        }
    }
}
