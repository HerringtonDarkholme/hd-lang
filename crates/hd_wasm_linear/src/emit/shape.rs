//! Shapes of a body that emission reads: its block tree ([`Tree`]), and
//! what a store, a read or a reference names (a local, a list, a
//! projection, a read through a reference). Nothing here decides a count:
//! the ownership passes did (`docs/compiler/tir-v2.md`, "Stage 2").

use hd_tir::ir::{Body, NONE, Op, PlaceRoot, Proj, Ref, Tag};

/// The block structure of a body: each instruction's block and position,
/// and each block's owning instruction.
pub(super) struct Tree {
    /// The block whose list holds each instruction, and its position.
    parent: Vec<(u32, usize)>,
    /// The instruction that runs each block (an `If`, a `Loop`, ...).
    owner: Vec<u32>,
}

impl Tree {
    pub(super) fn new(b: &Body) -> Tree {
        let n = b.len();
        let mut parent = vec![(NONE, 0); n];
        let mut owner = vec![NONE; n];
        for i in 0..n {
            let [a, w] = b.data[i];
            let tag = b.tags[i];
            if tag == Tag::Block {
                for (p, &c) in b.record(a).iter().enumerate() {
                    if let Some(x) = parent.get_mut(c as usize) {
                        *x = (u32::try_from(i).expect("insts"), p);
                    }
                }
                continue;
            }
            let mut blocks = Vec::new();
            for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
                match op {
                    Op::Block => blocks.push(word),
                    Op::Blocks if word != NONE => blocks.extend_from_slice(b.record(word)),
                    _ => {}
                }
            }
            if matches!(
                tag,
                Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
            ) {
                blocks.extend(b.record(w).iter().skip(1).take(2));
            }
            for blk in blocks {
                if b.tags.get(blk as usize) == Some(&Tag::Block) {
                    owner[blk as usize] = u32::try_from(i).expect("insts");
                }
            }
        }
        Tree { parent, owner }
    }

    /// The blocks around instruction `x`, innermost first, each with the
    /// position of the statement in it that holds `x`; `None` when the
    /// chain does not reach a sub-body's root (a closure's own body).
    pub(super) fn path(&self, b: &Body, mut x: u32) -> Option<Vec<(u32, usize)>> {
        let mut out = Vec::new();
        loop {
            if b.tags[x as usize] == Tag::Block {
                if b.sub_root.contains(&x) {
                    return Some(out);
                }
                x = self.owner[x as usize];
                if x == NONE {
                    return None;
                }
                continue;
            }
            let (blk, p) = self.parent[x as usize];
            if blk == NONE {
                return None;
            }
            out.push((blk, p));
            x = blk;
        }
    }

    /// Whether instruction `d` runs before instruction `j` on every path
    /// of their sub-body that reaches `j`: `d` is a statement of a block
    /// around `j`, before the statement that holds `j`. Blocks are entered
    /// only at their start (a resumed suspending body is the exception,
    /// `susp.rs`), so reaching that statement ran `d`.
    pub(super) fn dominates(&self, b: &Body, d: u32, j: u32) -> bool {
        let (blk, p) = self.parent[d as usize];
        blk != NONE
            && self
                .path(b, j)
                .is_some_and(|path| path.iter().any(|&(k, q)| k == blk && q > p))
    }

    /// Whether `i` belongs to this loop, including its nested control
    /// blocks but excluding separately invoked closure bodies.
    pub(super) fn in_loop(&self, b: &Body, i: u32, loop_inst: u32) -> bool {
        self.path(b, i).is_some_and(|path| {
            path.iter()
                .any(|(blk, _)| self.owner[*blk as usize] == loop_inst)
        })
    }

    /// A write guaranteed on an entered iteration: no intervening
    /// conditional or inner loop may skip it. `guard` is the loop test.
    pub(super) fn iteration_write(&self, b: &Body, i: u32, loop_inst: u32, guard: u32) -> bool {
        self.path(b, i).is_some_and(|path| {
            for (blk, _) in path {
                let owner = self.owner[blk as usize];
                if owner == loop_inst {
                    return true;
                }
                if owner != NONE && owner != guard && b.tags[owner as usize] != Tag::Scope {
                    return false;
                }
            }
            false
        })
    }
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

/// A `Store` `i` into a local or a struct field of it (`[$l]`,
/// `[$l .x .y]`): the local and the value stored. A field store replaces
/// part of the local, as a whole store replaces all of it.
pub(super) fn local_part_store(b: &Body, i: usize) -> Option<(usize, u32)> {
    b.stored_local(i).map(|(l, _)| (l.idx(), b.data[i][1]))
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
/// of that value (`Em::projected`): one fact per instruction, in a table
/// indexed by instruction. A read through a reference is `None`, though
/// `Read [*%r .k]` has the shape of a data object's field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Projected {
    /// No projection read.
    #[default]
    None,
    /// A field of a struct, newtype or data value (`[%v .k]`,
    /// `[*%o .k]`).
    Field,
    /// An element of a tuple (`[%v .k]`).
    Element,
    /// A field of an enum value's payload (`[%v as .V .k]`).
    Payload,
}

/// The value projection read `i` reads from, and what it reads
/// ([`Projected`]); `None` for any other instruction.
pub(super) fn projection(b: &Body, i: usize, projs: &[Projected]) -> Option<(u32, Projected)> {
    match projs.get(i).copied().unwrap_or_default() {
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
pub(super) fn read_through(b: &Body, i: usize, projs: &[Projected]) -> Option<(Through, bool)> {
    match b.tags[i] {
        Tag::Read if projection(b, i, projs).is_none() => {
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

/// The instruction values instruction `i` reads: TIR's own list where it
/// has one, else every operand word that names an instruction, an
/// over-approximation, which only ever makes a list's alias check refuse.
pub(super) fn uses(b: &Body, i: usize) -> Vec<u32> {
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    // A block's record is its statements; only its tail is a use.
    if tag == Tag::Block {
        return if w != NONE && Ref(w).as_inst().is_some() {
            vec![w]
        } else {
            vec![]
        };
    }
    let mut out = if let Some(mut v) = b.value_operands(i) {
        // A call's last three words are its providers: a range of key and
        // value pairs (1), or a context value (2).
        if tag == Tag::Call {
            let r = b.record(w);
            if let [.., kind, x, n] = *r {
                match kind {
                    1 => v.extend(b.extra.iter().skip(x as usize).take(n as usize)),
                    2 => v.push(x),
                    _ => {}
                }
            }
        }
        v
    } else if matches!(tag, Tag::With | Tag::ContextNew) {
        // Keys and values in pairs.
        b.record(a).iter().skip(1).step_by(2).copied().collect()
    } else {
        let mut out = Vec::new();
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Value => out.push(word),
                Op::Values if word != NONE => out.extend_from_slice(b.record(word)),
                // Only these records hold values; a closure's holds its
                // captures' range, a switch's its case and arms.
                Op::Record
                    if word != NONE && matches!(tag, Tag::CallDyn | Tag::Await | Tag::NewList) =>
                {
                    out.extend_from_slice(b.record(word));
                }
                _ => {}
            }
        }
        out
    };
    out.retain(|&r| r != NONE && Ref(r).as_inst().is_some() && (r as usize) < b.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body of one local: a store into it, a reference to it, a read
    /// and a write through that reference, then a read of the local.
    fn reference_body() -> Body {
        use hd_tir::ir::Place;
        fn record(b: &mut Body, words: &[u32]) -> u32 {
            let at = u32::try_from(b.extra.len()).unwrap();
            b.extra.push(u32::try_from(words.len()).unwrap());
            b.extra.extend_from_slice(words);
            at
        }
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
                (Some(words), _) => record(b, &words),
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
        let none = record(&mut b, &[]);
        let value = inst(&mut b, Tag::NewTuple, NONE, none);
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
        let list = record(&mut b, &list);
        let root = inst(&mut b, Tag::Block, list, NONE);
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
        assert_eq!(read_through(&new, 3, &[]), Some((Through::Value(2), true)));
        assert_eq!(local_part_store(&new, 4), None);
    }
}
