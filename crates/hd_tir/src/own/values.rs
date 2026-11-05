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
//! through a dereference reaches what the local refers to, not the local. Every other local is a **value**. Its
//! definitions are its stores, whole or through fields, and its entry
//! value when it is a parameter. A read of a value local is the value its
//! last definition on that path made.
//!
//! The ownership statements (`Drop`, `IsUnique`, `FlagSet`, `FlagClear`)
//! are not counted: the passes write them, and the classification is of
//! what the checker and the inliner leave. It reads a body before the
//! passes or after them alike.

use super::splice;
use crate::ir::{Body, CaptureMode, NONE, Proj, Ref, Root, Tag, Topology, inside, u32_of};

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
                    if b.cap_mode.get(c as usize) == Some(&CaptureMode::Shared) {
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

/// The pass's tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    classes: Vec<Local>,
    /// Per local: its one whole store, or `NONE` while it has none;
    /// `MANY` once it has another, or anything but one read names it.
    store: Vec<u32>,
    /// Per local: its one read, or `NONE`; `MANY` once it has another.
    read: Vec<u32>,
    /// Per instruction: the value a forwarded read stands for, or `NONE`.
    to: Vec<u32>,
    /// Statements to delete: each forwarded local's store and read.
    dead: Vec<u32>,
    /// Per instruction: the local a candidate's read reads, or `NONE`.
    reads_of: Vec<(u32, u32)>,
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

/// Forwards each single-definition value local's value (step 6 of the
/// migration) where the local only carries it from its store to its one
/// read: a [`Local::Value`] with one definition, a whole
/// `Store [$l] %v` of an instruction's value of the local's type, and one
/// whole `Read [$l]` later in the store's own block, with only
/// [`plain`] statements between them and nothing else naming the local
/// (no capture). The read's consumers take `%v` itself,
/// and the store and the read go, so `%v` has the uses the local's read
/// had: consumed in its own block where the read moved, lent where it
/// lent. An inlined copy's parameter binding (`self := %v`) is the usual
/// such local. A body that suspends keeps its locals. Rebuilds nothing
/// for a body with no such local.
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
    let n = b.local_ty.len();
    t.store.clear();
    t.read.clear();
    let mut any = false;
    for (l, c) in t.classes.iter().enumerate() {
        // A local that holds a reference keeps its name: the backend's
        // loan elision tells two references apart by the local each comes
        // from (`alias := node` beside `node`).
        let candidate = *c == Local::Value(1)
            && b.local_flags[l] & crate::ir::local_flags::PARAM == 0
            && !cx.holds_ref(b.local_ty[l]);
        any |= candidate;
        t.store.push(if candidate { NONE } else { MANY });
        t.read.push(NONE);
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
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure {
            let rec = b.record(b.data[i][1]);
            for c in rec[0]..rec[0] + rec[1] {
                t.store[b.cap_local[c as usize].raw() as usize] = MANY;
            }
            continue;
        }
        let Some(p) = named(b, i) else {
            continue;
        };
        let Some(l) = p.local() else {
            continue;
        };
        let l = l as usize;
        if t.store[l] == MANY {
            continue;
        }
        match b.tags[i] {
            Tag::Store if p.whole() && t.store[l] == NONE => t.store[l] = u32_of(i),
            Tag::Read if p.whole() && t.read[l] == NONE => t.read[l] = u32_of(i),
            _ => t.store[l] = MANY,
        }
    }
    t.reads_of.clear();
    t.reads_of.resize(b.len(), (NONE, NONE));
    for (l, &r) in t.read.iter().enumerate() {
        if r != NONE && r != MANY {
            t.reads_of[r as usize] = (u32_of(l), r);
        }
    }
    let mut found = false;
    for l in 0..n {
        let (s, r) = (t.store[l], t.read[l]);
        if s == MANY || s == NONE || r == NONE || r == MANY || r < s {
            t.store[l] = MANY;
            continue;
        }
        let v = b.data[s as usize][1];
        let typed = Ref(v)
            .as_inst()
            .is_some_and(|v| (v.0 as usize) < b.len() && b.ty[v.0 as usize] == b.local_ty[l]);
        if !typed || !local_read(b, v as usize) {
            t.store[l] = MANY;
            continue;
        }
        found = true;
    }
    if !found {
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
            && let Some(&(l, _)) = t.reads_of.get(r as usize)
            && l != NONE
        {
            t.store[l as usize] = MANY;
        }
    }
    topo.build(b);
    t.to.clear();
    t.to.resize(b.len(), NONE);
    t.dead.clear();
    for l in 0..n {
        let (s, r) = (t.store[l], t.read[l]);
        if s == MANY {
            continue;
        }
        let (si, ri) = (s as usize, r as usize);
        let blk = topo.blk[si];
        if blk == NONE || blk != topo.blk[ri] || topo.pos[ri] <= topo.pos[si] {
            continue;
        }
        let (from, to) = (topo.pos[si] as usize + 1, topo.pos[ri] as usize);
        let between = &b.record(b.data[blk as usize][0])[from..to];
        if !between.iter().all(|&x| plain(b.tags[x as usize])) {
            continue;
        }
        t.to[ri] = b.data[si][1];
        t.dead.push(s);
    }
    if t.dead.is_empty() {
        return Ok(());
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
    t.dead.retain(|&s| {
        let l = b.view(b.data[s as usize][0]).local().unwrap_or(NONE) as usize;
        t.read.get(l).is_some_and(|&r| t.to[r as usize] != NONE)
    });
    for &s in &t.dead.clone() {
        let l = b.view(b.data[s as usize][0]).local().unwrap_or(NONE) as usize;
        t.dead.push(t.read[l]);
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
