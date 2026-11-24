//! Shapes of a body that emission reads: its block tree (`Em::topo`), and
//! what a store, a read or a reference names (a local, a list, a
//! projection, a read through a reference). Nothing here decides a count:
//! the ownership passes did (`docs/compiler/tir-v2.md`, "The ownership
//! passes").

use hd_tir::ir::{Body, PlaceRoot, Proj, Tag};

pub(super) fn u32_of(i: usize) -> u32 {
    // ruled out: TIR names instructions by `u32` words, so a body's
    // instruction count fits.
    u32::try_from(i).expect("instructions fit u32")
}

pub(super) use hd_opt::prove::read_local;

/// A `Store` `i` into a whole local: the local and the value it takes.
#[must_use]
pub fn local_store(b: &Body, i: usize) -> Option<(usize, u32)> {
    match b.stored_local(i)? {
        (l, true) => Some((l.idx(), b.data[i][1])),
        (_, false) => None,
    }
}

/// The local whose own place reference `i` names, `&x` or `&x.f`: a
/// `Ref` rooted at a local with only field projections.
#[must_use]
pub fn ref_local(b: &Body, i: usize) -> Option<usize> {
    if b.tags[i] != Tag::Ref {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match p.root {
        PlaceRoot::Local(l) if p.proj.iter().all(|s| matches!(s, Proj::Field(_))) => Some(l.idx()),
        _ => None,
    }
}

/// What a projection read of one step from a value reads, by the type
/// of that value (`Em::projected`), queried from its instantiated type.
/// A read through a reference is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Projected {
    /// No projection read.
    #[default]
    None,
    /// A field of a struct or newtype value (`[%v .k]`).
    Field,
    /// An element of a tuple (`[%v .k]`).
    Element,
    /// A field of an enum value's payload (`[%v as .V .k]`).
    Payload,
}

/// The value projection read `i` reads from, and what it reads
/// ([`Projected`]); `None` for any other instruction.
pub(super) fn projection(b: &Body, i: usize, projected: Projected) -> Option<(u32, Projected)> {
    match projected {
        Projected::None => None,
        p => b.read_step(i).map(|(v, _)| (v, p)),
    }
}
