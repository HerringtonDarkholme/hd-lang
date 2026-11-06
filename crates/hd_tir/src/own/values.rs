//! Value locals (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`, "Values"):
//! which locals of a body name one value at a time, so a later pass can
//! read them as values, and which stay places.
//!
//! A local is a **place** when its storage is reached other than by
//! reading or storing it whole or by a field path: a `Ref` rooted at it,
//! directly or through a chain of reads (`%r = Read [$l]`, then
//! `Ref [%r .a]`), with no dereference on the way; a closure that shares
//! it through a cell; a cleanup suite that reads or writes it, which runs
//! at its scope's exits; or a store into an element or a variant payload
//! of it, which changes its buffer or its tag in place. A `Ref` or a store
//! through a dereference reaches what the local refers to, not the local.
//! A local defined in more than one block (stored in match arms or a loop
//! body as well as before them) is a place too: a read after such stores
//! would join values, and TIR has no join form. Every other local is a
//! **value**. Its definitions are its stores, whole or through fields,
//! and its entry value when it is a parameter, all in one block. A read of
//! a value local is the value its last definition before it made.
//!
//! The ownership statements (`Drop`, `IsUnique`, `FlagSet`, `FlagClear`)
//! are not counted: the passes write them, and the classification is of
//! what the checker and the inliner leave. It reads a body before the
//! passes or after them alike.

use super::splice;
use crate::ir::{Body, NONE, Proj, Ref, Role, Root, Tag, Topology, inside, u32_of};
use hd_base::SliceSort as _;

/// What one local of a body is ([`classify`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Local {
    /// Its storage is reached other than whole or by a field path.
    Place,
    /// It names one value at a time, made by this many definitions: its
    /// stores, plus one for a parameter's entry value. Zero for a local
    /// the body never stores or reads.
    Value(u32),
}

/// The class of each local of `b`, in `out` (one per local).
pub fn classify(b: &Body, out: &mut Vec<Local>) {
    let n = b.local_ty.len();
    out.clear();
    out.extend((0..n).map(|l| {
        let param = b.local_flags[l] & crate::ir::local_flags::PARAM != 0;
        Local::Value(u32::from(param))
    }));
    let place = |out: &mut Vec<Local>, l: u32| {
        if let Some(c) = out.get_mut(l as usize) {
            *c = Local::Place;
        }
    };
    for i in 0..b.len() {
        match b.tags[i] {
            Tag::Ref => {
                if let Some(l) = storage_local(b, b.data[i][0]) {
                    place(out, l);
                }
            }
            Tag::Store => {
                let p = b.view(b.data[i][0]);
                let Some(l) = p.local() else {
                    continue;
                };
                if p.steps(b).all(|s| matches!(s, Proj::Field(_))) {
                    if let Some(Local::Value(d)) = out.get_mut(l as usize) {
                        *d += 1;
                    }
                } else if storage_local(b, b.data[i][0]).is_some() {
                    place(out, l);
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    if b.cap_mode.get(c as usize).is_some_and(|m| m.shares()) {
                        place(out, b.cap_local[c as usize].raw());
                    }
                }
            }
            Tag::Scope if b.data[i][1] != crate::ir::NONE => {
                suite_places(b, b.record(b.data[i][1]), out);
            }
            Tag::Defer => suite_places(b, std::slice::from_ref(&b.data[i][0]), out),
            _ => {}
        }
    }
    joined_places(b, out);
}

/// Marks as places the value locals defined in more than one block: a
/// local stored on more than one path (match arms, loop iterations) has
/// reads that join values, which TIR has no form for. A parameter's
/// entry value is defined in its sub-body's root block.
fn joined_places(b: &Body, out: &mut [Local]) {
    if !out.iter().any(|c| matches!(c, Local::Value(d) if *d > 1)) {
        return;
    }
    let mut home = vec![NONE; out.len()];
    let root = b.sub_root.first().copied().unwrap_or(NONE);
    for (l, h) in home.iter_mut().enumerate() {
        if b.local_flags[l] & crate::ir::local_flags::PARAM != 0 {
            *h = root;
        }
    }
    for (s, r) in b.sub_params.iter().enumerate().skip(1) {
        if r.len == 0 {
            continue;
        }
        for &l in b.record(r.start) {
            if let Some(h) = home.get_mut(l as usize) {
                *h = b.sub_root[s];
            }
        }
    }
    for i in 0..b.len() {
        if b.tags[i] != Tag::Block {
            continue;
        }
        for &m in b.record(b.data[i][0]) {
            let m = m as usize;
            if m >= b.len() || b.tags[m] != Tag::Store {
                continue;
            }
            let p = b.view(b.data[m][0]);
            let Some(l) = p.local().map(|l| l as usize) else {
                continue;
            };
            if !matches!(out[l], Local::Value(d) if d > 1)
                || !p.steps(b).all(|s| matches!(s, Proj::Field(_)))
            {
                continue;
            }
            if home[l] == NONE {
                home[l] = u32_of(i);
            } else if home[l] != u32_of(i) {
                out[l] = Local::Place;
            }
        }
    }
}

/// Marks as places the locals cleanup suites `roots` touch.
fn suite_places(b: &Body, roots: &[u32], out: &mut [Local]) {
    let mut stack = Vec::new();
    inside(b, roots, &mut stack, |i| {
        match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref => {
                if let Some(l) = root_local(b, b.data[i][0])
                    && let Some(c) = out.get_mut(l as usize)
                {
                    *c = Local::Place;
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    if let Some(c) = out.get_mut(b.cap_local[c as usize].raw() as usize) {
                        *c = Local::Place;
                    }
                }
            }
            _ => {}
        }
        true
    });
}

/// The local place word `w` is rooted at, directly or through a chain of
/// reads (`[%r .a]` after `%r = Read [$l]`).
fn root_local(b: &Body, mut w: u32) -> Option<u32> {
    for _ in 0..64 {
        let view = b.view(w);
        match view.root {
            Root::Local(l) => return Some(l),
            Root::Value(r) if b.tags[r as usize] == Tag::Read => w = b.data[r as usize][0],
            _ => return None,
        }
    }
    None
}

/// The local whose own storage place word `w` names: [`root_local`] when
/// no step on the way is a dereference, which leaves the local for what
/// its reference refers to (`[*$self .infos]`).
fn storage_local(b: &Body, mut w: u32) -> Option<u32> {
    for _ in 0..64 {
        let view = b.view(w);
        if view.steps(b).any(|s| matches!(s, Proj::Deref)) {
            return None;
        }
        match view.root {
            Root::Local(l) => return Some(l),
            Root::Value(r) if b.tags[r as usize] == Tag::Read => w = b.data[r as usize][0],
            _ => return None,
        }
    }
    None
}

/// The `own` view's word for class `c`: `value`, `value ×n` for a local
/// with `n` definitions, or `place`.
#[must_use]
pub fn label(c: Local) -> String {
    match c {
        Local::Place => "place".to_owned(),
        Local::Value(d) if d <= 1 => "value".to_owned(),
        Local::Value(d) => format!("value ×{d}"),
    }
}

/// How one read of a value local uses the value it reads (step 8a): it
/// consumes it when one of the read's own uses takes the read's count
/// ([`super::consumes`]: a store, a construction part, a return, an owned
/// argument) and it reads the value or a field or payload of it, and
/// lends it otherwise: a read through an element or a dereference lends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Lend,
    Consume,
}

/// One use of a value local ([`uses`]): a read of it, a store or a
/// reference through it, or a closure's capture of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueUse {
    pub local: u32,
    /// The definition whose value it reads: a `Store`, or `NONE` for a
    /// parameter's entry value.
    pub def: u32,
    /// The using instruction.
    pub at: u32,
    pub how: Use,
    /// Whether it is its value's last use on its path ([`mark_last`]).
    pub last: bool,
}

/// Each use of a value local of `b` (`classes`, from [`classify`]), in
/// `out` by local, then definition, then instruction: the value it uses
/// and whether it consumes or lends it. A value local's definitions sit
/// in one block, so a read's value is the last definition before the
/// read's statement in that block; a read after that block reads its
/// last definition, and a read before every store a parameter's entry.
pub fn uses(
    b: &Body,
    cx: &dyn super::OwnCx,
    topo: &Topology,
    classes: &[Local],
    out: &mut Vec<ValueUse>,
) {
    out.clear();
    let value = |l: u32| matches!(classes.get(l as usize), Some(Local::Value(_)));
    let local_read = |v: usize| {
        if v >= b.len() || b.tags[v] != Tag::Read {
            return None;
        }
        match b.view(b.data[v][0]).root {
            Root::Local(l) if value(l) => Some(l),
            _ => None,
        }
    };
    // Each value local's definitions, in the order their block runs them.
    let mut defs: Vec<(u32, u32)> = Vec::new();
    let mut consumed = Vec::new();
    let mut ops = Vec::new();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Store {
            let p = b.view(b.data[i][0]);
            if let Some(l) = p.local()
                && value(l)
                && p.steps(b).all(|s| matches!(s, Proj::Field(_)))
            {
                defs.push((l, u32_of(i)));
            }
        }
        super::operands(b, i, &mut ops);
        for op in &ops {
            if local_read(op.value as usize).is_none() {
                continue;
            }
            let owner = if op.role == Role::Tail && !topo.is_root[i] {
                topo.owner[i]
            } else {
                NONE
            };
            if super::consumes(b, cx, i, op, owner) {
                consumed.push(op.value);
            }
        }
    }
    defs.sort_stable_by_key(|&(l, s)| (l, topo.pos[s as usize]));
    consumed.sort_stable();
    // The definition use `r` of local `l` reads.
    let def_of = |l: u32, r: usize| {
        let lo = defs.partition_point(|&(x, _)| x < l);
        let hi = defs.partition_point(|&(x, _)| x <= l);
        let mine = &defs[lo..hi];
        let &(_, s) = mine.first()?;
        let home = topo.blk[s as usize];
        // The use's statement in the definitions' block, or `NONE` when
        // the use is after that block or in a closure's body.
        let mut x = u32_of(r);
        while x != NONE && topo.blk[x as usize] != home {
            x = if topo.is_root[x as usize] {
                NONE
            } else {
                topo.blk[x as usize]
            };
        }
        let before =
            |&&(_, s): &&(u32, u32)| x == NONE || topo.pos[s as usize] < topo.pos[x as usize];
        mine.iter().rev().find(before).map(|&(_, s)| s)
    };
    let mut push = |l: u32, r: usize, how: Use| {
        let def = def_of(l, r).unwrap_or(NONE);
        out.push(ValueUse {
            local: l,
            def,
            at: u32_of(r),
            how,
            last: false,
        });
    };
    for r in 0..b.len() {
        match b.tags[r] {
            Tag::Read => {
                let Some(l) = local_read(r) else {
                    continue;
                };
                // A read through an element or a dereference copies what
                // the value refers to, and only lends the value.
                let part = b
                    .view(b.data[r][0])
                    .steps(b)
                    .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)));
                let how = if part && consumed.binary_search(&u32_of(r)).is_ok() {
                    Use::Consume
                } else {
                    Use::Lend
                };
                push(l, r, how);
            }
            // A store or a reference through the value's dereference
            // lends the reference the value is.
            Tag::Store | Tag::Ref => {
                let p = b.view(b.data[r][0]);
                if let Root::Local(l) = p.root
                    && value(l)
                    && p.steps(b).any(|s| matches!(s, Proj::Deref))
                {
                    push(l, r, Use::Lend);
                }
            }
            // A closure that copies or moves the value in consumes it.
            Tag::Closure => {
                let rec = b.record(b.data[r][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    let l = b.cap_local[c as usize].raw();
                    if !b.cap_mode.get(c as usize).is_some_and(|m| m.shares()) && value(l) {
                        push(l, r, Use::Consume);
                    }
                }
            }
            _ => {}
        }
    }
    out.sort_stable_by_key(|u| (u.local, u.def.wrapping_add(1), u.at));
}

/// Marks each use in `uses` (one body's, from [`uses`]) that is its
/// value's last use on its path (step 8b): no other use of the same value
/// may run after it. A use in a loop that its value's definition is
/// outside of is never last, since the next round may use the value
/// again; its value ends where the loop exits. Of two uses in different
/// arms of one `If` or `Match`, neither runs after the other. A use in a
/// closure's body, which may run any number of times, is never last. A
/// use followed by a `Return`, an `Unreachable`, or a `Break` or
/// `Continue` that leaves past a later use, is last on its path. A call
/// that never returns is not seen as an exit.
pub fn mark_last(b: &Body, topo: &Topology, uses: &mut [ValueUse]) {
    let chain = |i: u32, out: &mut Vec<Level>| statement_chain(topo, i, out);
    let root_of = |c: &[Level]| c.last().map_or(NONE, |&(blk, _, _)| blk);
    // Whether loop `l` holds instruction `i`.
    let holds = |l: u32, i: u32| {
        let mut m = topo.loop_of[i as usize];
        while m != NONE {
            if m == l {
                return true;
            }
            m = topo.loop_of[m as usize];
        }
        false
    };
    let (mut cu, mut cw) = (Vec::new(), Vec::new());
    let mut lo = 0;
    while lo < uses.len() {
        let key = (uses[lo].local, uses[lo].def);
        let hi = lo
            + uses[lo..]
                .iter()
                .take_while(|u| (u.local, u.def) == key)
                .count();
        for k in lo..hi {
            let u = uses[k].at;
            chain(u, &mut cu);
            let def = uses[k].def;
            // In a closure's body, or in a loop without the definition.
            let def_root = if def == NONE {
                param_root(b, uses[k].local)
            } else {
                chain(def, &mut cw);
                root_of(&cw)
            };
            let elsewhere = def_root != root_of(&cu);
            let inner = topo.loop_of[u as usize];
            let looped = inner != NONE && (def == NONE || !holds(inner, def));
            let later = (lo..hi).any(|j| {
                if j == k {
                    return false;
                }
                chain(uses[j].at, &mut cw);
                after(b, &cu, &cw)
            });
            uses[k].last = !elsewhere && !looped && !later;
        }
        lo = hi;
    }
}

/// One level of a use's statement chain ([`mark_last`]): the block, the
/// position in it, and the statement there.
pub(crate) type Level = (u32, u32, u32);

/// The statements around instruction `i`, innermost first, as (block,
/// position, statement), to its sub-body's root ([`after`]).
pub(crate) fn statement_chain(topo: &Topology, i: u32, out: &mut Vec<Level>) {
    out.clear();
    let mut x = i;
    loop {
        let blk = topo.blk[x as usize];
        if blk == NONE {
            return;
        }
        out.push((blk, topo.pos[x as usize], x));
        if topo.is_root[blk as usize] || topo.owner[blk as usize] == NONE {
            return;
        }
        x = topo.owner[blk as usize];
    }
}

/// Whether the use with statement chain `w` may run after the one with
/// chain `u` ([`mark_last`]): at the innermost block both are in, `w`'s
/// statement comes later, and no exit runs between them. In one
/// statement's different blocks (the arms of an `If` or a `Match`)
/// neither runs after the other. An exit is a `Return` or an
/// `Unreachable` after `u` in a block around it, below `w`'s statement;
/// or a `Break` or `Continue` there whose target holds both uses' block,
/// which it leaves.
pub(crate) fn after(b: &Body, u: &[Level], w: &[Level]) -> bool {
    let Some((at, pw)) = u.iter().enumerate().find_map(|(j, &(blk, _, _))| {
        w.iter()
            .find(|&&(x, _, _)| x == blk)
            .map(|&(_, pw, _)| (j, pw))
    }) else {
        return false;
    };
    if pw <= u[at].1 {
        return false;
    }
    let exits = |m: u32| match b.tags[m as usize] {
        Tag::Return | Tag::Unreachable => true,
        Tag::Break | Tag::Continue => {
            let t = b
                .label_inst
                .get(b.data[m as usize][0] as usize)
                .copied()
                .unwrap_or(NONE);
            u[at..].iter().any(|&(blk, _, _)| blk == t)
                || u[at + 1..].iter().any(|&(_, _, x)| x == t)
        }
        _ => false,
    };
    for (j, &(blk, p, _)) in u[..=at].iter().enumerate() {
        let list = b.record(b.data[blk as usize][0]);
        let end = if j == at { pw as usize } else { list.len() };
        let from = (p as usize + 1).min(end);
        if list[from..end].iter().any(|&m| exits(m)) {
            return false;
        }
    }
    true
}

/// The root block of the sub-body local `l` is a parameter of.
fn param_root(b: &Body, l: u32) -> u32 {
    for (s, r) in b.sub_params.iter().enumerate().skip(1) {
        if r.len > 0 && b.record(r.start).contains(&l) {
            return b.sub_root[s];
        }
    }
    b.sub_root.first().copied().unwrap_or(NONE)
}

/// The `own` view's line for the uses of value local `l` (its entries
/// of [`uses`], by definition): `uses $l: %d -> %u lend, %w consume; ...`,
/// where `%d` is the definition's `Store`, or `entry` for a parameter's
/// entry value, and a last use ends in `last`; `uses $l: none` for a
/// local nothing uses.
#[must_use]
pub fn uses_line(l: u32, uses: &[ValueUse]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("uses ${l}:");
    let mine = uses.iter().filter(|u| u.local == l);
    let mut def = None;
    for u in mine {
        if def == Some(u.def) {
            out.push(',');
        } else {
            if def.is_some() {
                out.push(';');
            }
            def = Some(u.def);
            if u.def == NONE {
                out.push_str(" entry ->");
            } else {
                let _ = write!(out, " %{} ->", u.def);
            }
        }
        let how = match u.how {
            Use::Lend => "lend",
            Use::Consume => "consume",
        };
        let _ = write!(out, " %{} {how}", u.at);
        if u.last {
            out.push_str(" last");
        }
    }
    if def.is_none() {
        out.push_str(" none");
    }
    out
}

/// A closure's `Copy` capture of all of a value local the body owns, at the
/// value's last use, becomes a `Move` when the local's `Drop` is the next
/// thing to touch it in the closure's block: the environment takes the
/// local's count, and the drop goes. Not a generic local function's, whose
/// instantiations share their captures, nor a local that holds a borrow,
/// which has no count to give.
pub(crate) fn capture_moves(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn super::OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    use crate::ir::{CaptureMode, sub_flags};
    if !b.cap_mode.contains(&CaptureMode::Copy) {
        return;
    }
    classify(b, &mut t.classes);
    let mut found = Vec::new();
    uses(b, cx, topo, &t.classes, &mut found);
    mark_last(b, topo, &mut found);
    for u in found {
        let i = u.at as usize;
        if !u.last || b.tags[i] != Tag::Closure || topo.blk[i] == NONE {
            continue;
        }
        let [sub, w] = b.data[i];
        let generic = b
            .sub_flags
            .get(sub as usize)
            .is_some_and(|f| f & sub_flags::GENERIC != 0)
            || b.is_instantiation(i);
        let ty = b.local_ty[u.local as usize];
        let holds_value = cx.counted(ty) && b.exit_drops.iter().any(|e| e.1.raw() == u.local)
            || !cx.counted(ty) && cx.copies(ty) != super::Tri::Yes;
        if generic || !holds_value || holds_borrow(b, u.local) {
            continue;
        }
        let [start, len] = b.record(w)[..2] else {
            continue;
        };
        let capture = (start..start + len).find(|&c| {
            let c = c as usize;
            b.cap_local[c].raw() == u.local && b.cap_mode[c] == CaptureMode::Copy
        });
        let drop = next_touch_in_block(b, topo, u32_of(i), u.local);
        if let (Some(c), Some(d)) = (capture, drop) {
            b.cap_mode[c as usize] = CaptureMode::Move;
            edits.remove(d);
        }
    }
}

/// Whether a borrow is stored into all of local `l`.
fn holds_borrow(b: &Body, l: u32) -> bool {
    (0..b.len()).any(|i| {
        b.tags[i] == Tag::Store && {
            let p = b.view(b.data[i][0]);
            p.local() == Some(l) && p.whole() && b.loan_read(b.data[i][1]).is_some()
        }
    })
}

/// The whole `Drop` of local `l` that follows statement `x` in its block,
/// when nothing between them touches `l` or a value read from it.
fn next_touch_in_block(b: &Body, topo: &Topology, x: u32, l: u32) -> Option<u32> {
    let blk = topo.blk[x as usize];
    let list = b.record(b.data[blk as usize][0]);
    let from_l = |v: u32| {
        (v as usize) < b.len()
            && b.tags[v as usize] == Tag::Read
            && b.view(b.data[v as usize][0]).local() == Some(l)
    };
    let mut ops = Vec::new();
    for &m in &list[topo.pos[x as usize] as usize + 1..] {
        for j in topo.first[m as usize]..=m {
            let k = j as usize;
            let tag = b.tags[k];
            let named = matches!(
                tag,
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique
            );
            let p = named.then(|| b.view(b.data[k][0]));
            if j == m && tag == Tag::Drop && p.is_some_and(|p| p.whole() && p.local() == Some(l)) {
                return Some(m);
            }
            let captures = tag == Tag::Closure && {
                let rec = b.record(b.data[k][1]);
                b.cap_local[rec[0] as usize..(rec[0] + rec[1]) as usize]
                    .iter()
                    .any(|c| c.raw() == l)
            };
            super::operands(b, k, &mut ops);
            if captures
                || p.is_some_and(|p| p.local() == Some(l) || p.value().is_some_and(from_l))
                || ops.iter().any(|o| from_l(o.value))
            {
                return None;
            }
        }
    }
    None
}

/// The pass's tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    classes: Vec<Local>,
    /// Per local: `NONE` while it may be forwarded, `MANY` once not.
    state: Vec<u32>,
    /// Each whole store and read of a local that may be forwarded, as
    /// (local, instruction), then in the order its block runs them.
    names: Vec<(u32, u32)>,
    /// Per instruction: the value a forwarded read stands for, or `NONE`.
    to: Vec<u32>,
    /// Each store whose value a read forwards, with that read.
    pairs: Vec<(u32, u32)>,
    /// Statements to delete: each forwarded store and its read.
    dead: Vec<u32>,
}

const MANY: u32 = NONE - 1;

/// Whether value `v` may be forwarded as far as its own kind goes: any
/// instruction's value but a read of memory a reference or a list's
/// buffer holds. Stored into a binding local, such a read borrows where a
/// call lends the local (`borrow`, rule 3); read where the call takes it,
/// it would copy.
fn local_read(b: &Body, v: usize) -> bool {
    if b.tags[v] != Tag::Read {
        return true;
    }
    let p = b.view(b.data[v][0]);
    matches!(p.root, Root::Local(_))
        && p.steps(b)
            .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
}

/// Whether a statement between a forwarded local's store and its read
/// leaves every fact about the value as it was: it calls, stores, suspends
/// and runs no block, so the value's uses see what the local's read saw.
fn plain(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::Read
            | Tag::Prim
            | Tag::Coerce
            | Tag::NewStruct
            | Tag::NewTuple
            | Tag::NewVariant
            | Tag::Is
            | Tag::CopyStruct
    )
}

/// Value locals that only rename another (`b := a`): each read of `b` reads
/// `a`, and the store and its read go. Both name one value for all of `b`'s
/// life, since neither is stored again or changed in place, and insertion
/// then keeps `a` until `b`'s last use. `b` has one whole store, of a whole
/// read of `a`, and every use of it follows that store in the store's block.
/// It is no parameter and no closure captures it. A local that holds a
/// reference keeps its name, as in [`forward`], and a body that suspends
/// keeps its locals.
pub(crate) fn aliases(
    b: &mut Body,
    cx: &dyn super::OwnCx,
    topo: &mut Topology,
    t: &mut Tables,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    let stored_read = |b: &Body, i: usize| {
        if b.tags[i] != Tag::Store {
            return None;
        }
        let p = b.view(b.data[i][0]);
        let to = p.local().filter(|_| p.whole())?;
        let r = Ref(b.data[i][1]).as_inst()?.0 as usize;
        let q = (b.tags[r] == Tag::Read).then(|| b.view(b.data[r][0]))?;
        let from = q.local().filter(|_| q.whole())?;
        Some((to, from, r))
    };
    if b.tags.iter().any(|&tag| crate::ir::is_suspension(tag))
        || cx.suspending(b.item)
        || !(0..b.len()).any(|i| stored_read(b, i).is_some())
    {
        return Ok(());
    }
    classify(b, &mut t.classes);
    topo.build(b);
    let n = b.local_ty.len();
    // Locals a change in place reaches, whose copies differ afterwards, and
    // locals a closure captures.
    let mut fixed = vec![true; n];
    let mut unshared = vec![true; n];
    for c in &b.cap_local {
        unshared[c.idx()] = false;
    }
    for i in 0..b.len() {
        for (_, v) in super::changed(b, i) {
            if (v as usize) < b.len()
                && b.tags[v as usize] == Tag::Read
                && let Some(l) = b.view(b.data[v as usize][0]).local()
            {
                fixed[l as usize] = false;
            }
        }
    }
    let mut named = vec![0u32; b.len()];
    for i in 0..b.len() {
        let member = b.tags[i] == Tag::Block;
        crate::ir::slots(b, i, |_, value, slot| {
            let statement = member && matches!(slot, crate::ir::Word::Inst);
            if !statement && let Some(n) = named.get_mut(value as usize) {
                *n += 1;
            }
        });
    }
    let once = |c: &[Local], l: u32| c[l as usize] == Local::Value(1);
    let mut to: Vec<u32> = vec![NONE; n];
    let mut stores = Vec::new();
    for i in 0..b.len() {
        let Some((x, from, r)) = stored_read(b, i) else {
            continue;
        };
        let (xi, fi) = (x as usize, from as usize);
        if x != from
            && named[r] == 1
            && once(&t.classes, x)
            && once(&t.classes, from)
            && b.local_flags[xi] & crate::ir::local_flags::PARAM == 0
            && unshared[xi]
            && fixed[xi]
            && fixed[fi]
            && b.local_ty[xi] == b.local_ty[fi]
            && cx.counted(b.local_ty[xi])
            && !cx.holds_ref(b.local_ty[xi])
            && uses_follow(b, topo, x, i)
        {
            to[xi] = from;
            stores.push((u32_of(i), u32_of(r)));
        }
    }
    if stores.is_empty() {
        return Ok(());
    }
    let resolve = |mut l: u32| {
        while to[l as usize] != NONE {
            l = to[l as usize];
        }
        l
    };
    for i in 0..b.len() {
        if !matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref) {
            continue;
        }
        let word = b.data[i][0];
        if let Some(l) = b.view(word).local().filter(|&l| to[l as usize] != NONE) {
            let new = resolve(l);
            if word & crate::ir::place::BARE_LOCAL != 0 {
                b.data[i][0] = new | crate::ir::place::BARE_LOCAL;
            } else {
                b.extra[word as usize + 2] = new;
            }
        }
    }
    edits.clear();
    for (s, r) in stores {
        edits.remove(s);
        edits.remove(r);
    }
    splice::apply(b, topo, edits)?;
    edits.clear();
    Ok(())
}

/// Whether every read and every reference or store through local `l` comes
/// after store `s` in `s`'s block.
fn uses_follow(b: &Body, topo: &Topology, l: u32, s: usize) -> bool {
    let home = topo.blk[s];
    (0..b.len()).all(|i| {
        if i == s || !matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref) {
            return true;
        }
        if b.view(b.data[i][0]).local() != Some(l) {
            return true;
        }
        let mut x = u32_of(i);
        while x != NONE && topo.blk[x as usize] != home {
            x = if topo.is_root[x as usize] {
                NONE
            } else {
                topo.blk[x as usize]
            };
        }
        x != NONE && topo.pos[x as usize] > topo.pos[s]
    })
}

/// Forwards the values a value local carries from a store to one read
/// (steps 6 and 7a of the migration). The local is a [`Local::Value`],
/// not a parameter, whose every naming is a whole `Store [$l]` or a whole
/// `Read [$l]`, all in one block, with nothing else naming it (no
/// capture). Each store starts a new value: when the next naming after
/// it is its value's one read, the store's value is an instruction's
/// value of the local's type, and only [`plain`] statements are between
/// them, the read's consumers take the stored `%v` itself, and the store
/// and the read go. So `%v` has the uses the local's read had: consumed
/// in its own block where the read moved, lent where it lent. Other
/// stores of the local stay, as do reads of them. A counted local stored
/// with a value made from its own read (`s += "x"`) is not forwarded: the
/// backend appends to its buffer in place. An inlined copy's
/// parameter binding (`self := %v`) and a re-store read right after
/// (`s = b; s.len()`) are the usual such values. A body that suspends
/// keeps its locals. Rebuilds nothing for a body with no such value.
pub(crate) fn forward(
    b: &mut Body,
    cx: &dyn super::OwnCx,
    topo: &mut Topology,
    t: &mut Tables,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    if b.tags.iter().any(|&tag| crate::ir::is_suspension(tag)) {
        return Ok(());
    }
    classify(b, &mut t.classes);
    t.state.clear();
    let mut any = false;
    for (l, c) in t.classes.iter().enumerate() {
        // A local that holds a reference keeps its name: the backend's
        // loan elision tells two references apart by the local each comes
        // from (`alias := node` beside `node`).
        let candidate = matches!(*c, Local::Value(d) if d > 0)
            && b.local_flags[l] & crate::ir::local_flags::PARAM == 0
            && !cx.holds_ref(b.local_ty[l]);
        any |= candidate;
        t.state.push(if candidate { NONE } else { MANY });
    }
    if !any {
        return Ok(());
    }
    let named = |b: &Body, i: usize| match b.tags[i] {
        Tag::Read
        | Tag::Store
        | Tag::Ref
        | Tag::Drop
        | Tag::IsUnique
        | Tag::FlagSet
        | Tag::FlagClear => Some(b.view(b.data[i][0])),
        _ => None,
    };
    t.names.clear();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure {
            let rec = b.record(b.data[i][1]);
            for c in rec[0]..rec[0] + rec[1] {
                t.state[b.cap_local[c as usize].raw() as usize] = MANY;
            }
            continue;
        }
        let Some(p) = named(b, i) else {
            continue;
        };
        let Some(l) = p.local() else {
            continue;
        };
        if t.state[l as usize] == MANY {
            continue;
        }
        if p.whole() && matches!(b.tags[i], Tag::Store | Tag::Read) {
            t.names.push((l, u32_of(i)));
        } else {
            t.state[l as usize] = MANY;
        }
    }
    // A counted local stored with a value made from its own read
    // (`s += "x"`, `s = s + t`) keeps its local: the backend changes such
    // a value in place, appending to the local's unique buffer.
    for &(l, s) in &t.names {
        let s = s as usize;
        if b.tags[s] != Tag::Store || !cx.counted(b.local_ty[l as usize]) {
            continue;
        }
        let Some(v) = Ref(b.data[s][1]).as_inst().map(|v| v.0 as usize) else {
            continue;
        };
        if v >= b.len() {
            continue;
        }
        let mut own_read = false;
        crate::ir::slots(b, v, |_, x, slot| {
            own_read |= matches!(slot, crate::ir::Word::Value(_))
                && (x as usize) < b.len()
                && b.tags[x as usize] == Tag::Read
                && b.view(b.data[x as usize][0]).local() == Some(l);
        });
        if own_read {
            t.state[l as usize] = MANY;
        }
    }
    let state = &t.state;
    t.names.retain(|&(l, _)| state[l as usize] != MANY);
    // Whether store `s` of local `l` stores an instruction's value of the
    // local's type that may be forwarded.
    let forwardable = |b: &Body, s: usize, l: u32| {
        let v = b.data[s][1];
        Ref(v).as_inst().is_some_and(|v| {
            (v.0 as usize) < b.len() && b.ty[v.0 as usize] == b.local_ty[l as usize]
        }) && local_read(b, v as usize)
    };
    if !t
        .names
        .iter()
        .any(|&(l, i)| b.tags[i as usize] == Tag::Store && forwardable(b, i as usize, l))
    {
        return Ok(());
    }
    topo.build(b);
    t.names
        .sort_stable_by_key(|&(l, i)| (l, topo.blk[i as usize], topo.pos[i as usize]));
    t.to.clear();
    t.to.resize(b.len(), NONE);
    t.pairs.clear();
    for group in t.names.chunk_by(|x, y| x.0 == y.0) {
        let (l, blk) = (group[0].0, topo.blk[group[0].1 as usize]);
        if blk == NONE || group.iter().any(|&(_, i)| topo.blk[i as usize] != blk) {
            continue;
        }
        let list = b.record(b.data[blk as usize][0]);
        for (k, w) in group.windows(2).enumerate() {
            let (s, r) = (w[0].1 as usize, w[1].1 as usize);
            let read_again = group
                .get(k + 2)
                .is_some_and(|&(_, x)| b.tags[x as usize] == Tag::Read);
            if b.tags[s] != Tag::Store
                || b.tags[r] != Tag::Read
                || read_again
                || !forwardable(b, s, l)
            {
                continue;
            }
            let between = &list[topo.pos[s] as usize + 1..topo.pos[r] as usize];
            if between.iter().all(|&x| plain(b.tags[x as usize])) {
                t.to[r] = b.data[s][1];
                t.pairs.push((u32_of(s), u32_of(r)));
            }
        }
    }
    if t.pairs.is_empty() {
        return Ok(());
    }
    // A read whose value roots another place keeps its local: the borrow
    // pass follows a chain of reads to a local, where a value root is
    // memory it knows nothing of (`xs := ys; xs[i]`).
    for i in 0..b.len() {
        let p = match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique => b.view(b.data[i][0]),
            _ => continue,
        };
        if let Root::Value(r) | Root::Ptr(r) = p.root
            && let Some(w) = t.to.get_mut(r as usize)
        {
            *w = NONE;
        }
    }
    // A read some instruction names other than as a value, a provider or
    // a block's statement keeps its local (a `Hook` observes it).
    for i in 0..b.len() {
        let (to, member) = (&mut t.to, b.tags[i] == Tag::Block);
        crate::ir::slots(b, i, |_, value, slot| {
            let named = match slot {
                crate::ir::Word::Value(_) | crate::ir::Word::Provider => false,
                crate::ir::Word::Inst => !member,
                crate::ir::Word::Block => true,
            };
            if named && let Some(w) = to.get_mut(value as usize) {
                *w = NONE;
            }
        });
    }
    t.dead.clear();
    for &(s, r) in &t.pairs {
        if t.to[r as usize] != NONE {
            t.dead.extend([s, r]);
        }
    }
    if t.dead.is_empty() {
        return Ok(());
    }
    // A forwarded read's value may itself be a forwarded read's.
    let resolve = |to: &[u32], mut v: u32| {
        while let Some(&w) = to.get(v as usize).filter(|&&w| w != NONE) {
            v = w;
        }
        v
    };
    let mut writes = Vec::new();
    for i in 0..b.len() {
        crate::ir::slots(b, i, |at, value, slot| {
            if matches!(slot, crate::ir::Word::Value(_) | crate::ir::Word::Provider)
                && t.to.get(value as usize).is_some_and(|&w| w != NONE)
            {
                writes.push((i, at, resolve(&t.to, value)));
            }
        });
    }
    for (i, at, v) in writes {
        match at {
            crate::ir::Loc::Data(k) => b.data[i][usize::from(k)] = v,
            crate::ir::Loc::Extra(x) => b.extra[x as usize] = v,
        }
    }
    edits.clear();
    for &d in &t.dead {
        edits.remove(d);
    }
    splice::apply(b, topo, edits)?;
    edits.clear();
    Ok(())
}
