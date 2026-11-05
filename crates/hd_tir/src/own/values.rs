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

use crate::ir::{Body, CaptureMode, Proj, Root, Tag, inside};

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
