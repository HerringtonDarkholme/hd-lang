//! Shapes of a body that emission reads: its block tree (`Em::topo`), and
//! what a store, a read or a reference names (a local, a list, a
//! projection, a read through a reference). Nothing here decides a count:
//! the ownership passes did (`docs/compiler/tir-v2.md`, "Stage 2").

use hd_tir::ir::{Body, Nest, PlaceRoot, Proj, Ref, Tag};

use super::Em;

impl Em<'_, '_, '_> {
    pub(super) fn block_path(&self, x: u32) -> Option<Vec<(u32, usize)>> {
        Nest::block_path(&self.topo, self.b, x)
    }

    /// [`Nest::dominates`] (a resumed suspending body is the exception, `susp.rs`).
    pub(super) fn dominates(&self, d: u32, j: u32) -> bool {
        self.tree().dominates(&self.topo, d, j)
    }

    /// The position of the statement of block `blk` that holds
    /// instruction `j`, when `blk` is around `j`.
    pub(super) fn position_in(&self, blk: u32, j: u32) -> Option<usize> {
        let k = self.tree().child_above(blk, j)?;
        Some(self.topo.pos[k as usize] as usize)
    }

    /// The body's [`Nest`], built on first use.
    pub(super) fn tree(&self) -> &Nest {
        self.tree.get_or_init(|| {
            crate::work::loop_scans(self.b.len());
            Nest::new(self.b, &self.topo)
        })
    }

    pub(super) fn in_loop(&self, i: u32, loop_inst: u32) -> bool {
        self.tree().in_loop(i, loop_inst)
    }

    pub(super) fn loop_body(&self, loop_inst: u32) -> Vec<u32> {
        let out = self.tree().loop_body(loop_inst);
        crate::work::loop_scans(out.len());
        out
    }

    pub(super) fn statements(&self, blk: u32, from: usize, to: usize) -> Vec<u32> {
        let out = self.tree().statements(self.b, blk, from, to);
        crate::work::loop_scans(out.len());
        out
    }
}

pub(super) fn u32_of(i: usize) -> u32 {
    // ruled out: TIR names instructions by `u32` words, so a body's
    // instruction count fits.
    u32::try_from(i).expect("instructions fit u32")
}

/// The local that value `r` reads when it is a plain local read,
/// `Read [$l]`; `None` for a constant or any other instruction.
pub(super) fn read_local(b: &Body, r: u32) -> Option<usize> {
    let i = Ref(r).as_inst()?.0 as usize;
    (i < b.len())
        .then(|| b.local_read(i))
        .flatten()
        .map(hd_base::LocalId::idx)
}

/// A `Store` `i` into a whole local: the local and the value it takes.
pub(super) fn local_store(b: &Body, i: usize) -> Option<(usize, u32)> {
    match b.stored_local(i)? {
        (l, true) => Some((l.idx(), b.data[i][1])),
        (_, false) => None,
    }
}

/// The list value whose element `Store` `i` writes (`[%xs [%i] ..]`), as
/// an element store intrinsic names its list.
pub(super) fn elem_store_list(b: &Body, i: usize) -> Option<u32> {
    if b.tags[i] != Tag::Store {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match (p.root, p.proj.first()) {
        (PlaceRoot::Value(r), Some(Proj::Elem(_))) => Some(r.0),
        _ => None,
    }
}

/// The local whose own place reference `i` names, `&x` or `&x.f`: a
/// `Ref` rooted at a local with only field projections.
pub(super) fn ref_local(b: &Body, i: usize) -> Option<usize> {
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

/// What a read through a reference reads through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Through {
    /// The reference a local holds.
    Local(usize),
    /// A reference value.
    Value(u32),
}

/// What a `Read` `i` whose place starts with a dereference reads
/// through, and whether it reads the whole referenced value. A data
/// object's field ([`Projected`]) is no read through a reference.
pub(super) fn read_through(b: &Body, i: usize, projected: Projected) -> Option<(Through, bool)> {
    match b.tags[i] {
        Tag::Read if projection(b, i, projected).is_none() => {
            let p = b.decode_place(b.data[i][0])?;
            if p.proj.first() != Some(&Proj::Deref) {
                return None;
            }
            let through = match p.root {
                PlaceRoot::Local(l) => Through::Local(l.idx()),
                PlaceRoot::Value(r) => Through::Value(r.0),
                _ => return None,
            };
            Some((through, p.proj.len() == 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body of one local: a store into it, a reference to it, a read
    /// and a write through that reference, then a read of the local.
    fn reference_body() -> Body {
        use hd_tir::ir::Place;
        fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
            let i = u32::try_from(b.tags.len()).unwrap();
            b.tags.push(tag);
            b.data.push([a, w]);
            b.ty.push(hd_types::Ty::VOID);
            b.syn.push(hd_base::NodeIdx::NONE);
            i
        }
        fn place(b: &mut Body, p: &Place) -> u32 {
            match (p.words(), p.root) {
                (Some(words), _) => b.push_record(&words),
                (None, PlaceRoot::Local(l)) => l.raw() | hd_tir::ir::place::BARE_LOCAL,
                (None, _) => unreachable!("only a bare local has no record"),
            }
        }
        let mut b = Body::new(hd_base::DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
        b.local_ty.push(hd_types::Ty::VOID);
        b.local_name.push(hd_base::Symbol::from_raw(0));
        b.local_syn.push(hd_base::NodeIdx::NONE);
        b.local_flags.push(hd_tir::ir::local_flags::ASSIGNED);
        let local = Place::local(hd_base::LocalId::from_raw(0));
        let none = b.push_record(&[]);
        let value = inst(&mut b, Tag::NewTuple, hd_tir::ir::NONE, none);
        let mut list = vec![value];
        let at = place(&mut b, &local);
        list.push(inst(&mut b, Tag::Store, at, value));
        let r = inst(&mut b, Tag::Ref, at, 1);
        let through = Place {
            root: PlaceRoot::Value(Ref(r)),
            proj: vec![Proj::Deref],
        };
        let at = place(&mut b, &through);
        let read = inst(&mut b, Tag::Read, at, 0);
        list.extend([r, read, inst(&mut b, Tag::Store, at, value)]);
        list.push(inst(&mut b, Tag::Read, hd_tir::ir::place::BARE_LOCAL, 0));
        let list = b.push_record(&list);
        let root = inst(&mut b, Tag::Block, list, hd_tir::ir::NONE);
        b.sub_root.push(root);
        b.sub_parent.push(hd_base::SubId::NONE);
        b.sub_flags.push(0);
        b.sub_params.push(hd_base::Range32::default());
        b
    }

    /// The place helpers name the locals of `Read`, `Store` and `Ref`: a
    /// store into the whole local, the reference's root, and a read and
    /// a write through the reference, which reach no local.
    #[test]
    fn places_name_their_locals() {
        let new = reference_body();
        assert_eq!(local_store(&new, 1), Some((0, 0)));
        assert_eq!(ref_local(&new, 2), Some(0));
        assert_eq!(
            read_through(&new, 3, Projected::None),
            Some((Through::Value(2), true))
        );
        assert_eq!(local_store(&new, 4), None);
    }
}
