use std::cell::Cell;

use super::*;
use crate::ir::{BodyKind, Place, Providers, TirBuilder, TirSink, ref_word};
use crate::own::Pass;
use hd_base::{DefId, NodeIdx, Symbol};
use hd_types::{Ty, TyList};

const COUNTED: Ty = Ty(100);
const LOCAL: Ty = Ty(101);
const MIXED: Ty = Ty(102);

#[derive(Default)]
struct Cx {
    classifications: Cell<usize>,
    transfers: Cell<usize>,
}

impl OwnCx for Cx {
    fn temporary_owner_name(&self) -> Symbol {
        Symbol::NONE
    }
    fn counted(&self, ty: Ty) -> bool {
        ty == Ty::STRING || ty == COUNTED || ty == MIXED
    }
    fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
        Pass::Lent
    }
    fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
        Pass::Lent
    }
    fn may_write(&self, _: &Callee) -> bool {
        false
    }
    fn keeps(&self, _: &Callee, _: usize, _: usize) -> bool {
        false
    }
    fn suspending(&self, _: DefId) -> bool {
        false
    }
    fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
        false
    }
    fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
        Pass::Owned
    }
    fn holds_coll(&self, _: Ty) -> bool {
        false
    }
    fn holds_ref(&self, ty: Ty) -> bool {
        self.transfers.set(self.transfers.get() + 1);
        [COUNTED, LOCAL, MIXED].contains(&ty)
    }
    fn counted_refs_only(&self, ty: Ty) -> bool {
        self.classifications.set(self.classifications.get() + 1);
        ty == COUNTED
    }
}

fn call(b: &mut TirBuilder, args: &[Ref], ty: Ty) -> Ref {
    b.call(
        &Callee::Item {
            def: DefId::from_raw(8),
            targs: TyList::EMPTY,
        },
        args,
        Providers::None,
        ty,
        NodeIdx::NONE,
    )
}

fn body(width: usize, depth: usize, extra: Option<Ty>) -> (Body, u32, u32) {
    let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
    let root = b.open_block();
    let mut references = Vec::new();
    let mut last = 0;
    for _ in 0..width {
        let owner = b.local(Ty::STRING, Symbol::NONE, 0, NodeIdx::NONE);
        last = owner.raw();
        let initial = call(&mut b, &[], Ty::STRING);
        b.set(owner, initial, NodeIdx::NONE);
        let place = b.place(&Place::local(owner));
        let mut reference = b.emit(Tag::Ref, place, 0, COUNTED, NodeIdx::NONE);
        for _ in 0..depth {
            reference = call(&mut b, &[reference], COUNTED);
        }
        references.push(reference);
    }
    if let Some(ty) = extra {
        let place = b.place(&Place::local(hd_base::LocalId::from_raw(last)));
        references.push(b.emit(Tag::Ref, place, ref_word::LOCAL, ty, NodeIdx::NONE));
    }
    let used = call(&mut b, &references, Ty::VOID).0;
    let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
    (
        // ruled out: the builder above closes every block it opens and
        // types each value, so `finish` has no invalid body to reject.
        b.finish(root, &[]).expect("valid cell-liveness input"),
        used,
        last,
    )
}

#[test]
fn entirely_counted_references_create_no_cell_liveness_batches() {
    for (width, depth) in [(250, 20), (1000, 20), (20, 1000)] {
        let (body, _, _) = body(width, depth, None);
        let cx = Cx::default();
        let mut uses = Vec::new();
        extra_cell_uses(&body, &cx, &Tables::default(), |_| true, &mut |i, l, _| {
            uses.push((i, l));
        });
        assert!(uses.is_empty());
        assert_eq!(
            cx.classifications.get(),
            0,
            "the ownership flag needs no type query"
        );
        assert_eq!(
            cx.transfers.get(),
            0,
            "owned references start no dataflow solve"
        );
    }
}

#[test]
fn an_owner_with_a_local_or_mixed_reference_still_lives_until_its_use() {
    for ty in [LOCAL, MIXED] {
        let (body, consumer, owner) = body(1000, 0, Some(ty));
        let cx = Cx::default();
        let mut uses = Vec::new();
        extra_cell_uses(&body, &cx, &Tables::default(), |_| true, &mut |i, l, _| {
            uses.push((i, l));
        });
        assert!(
            uses.contains(&(consumer, owner)),
            "the uncounted view keeps its owner alive"
        );
        assert!(uses.iter().all(|&(_, l)| l == owner));
        assert!(
            cx.transfers.get() < 16 * body.len(),
            "one owner solve, not one per counted reference"
        );
    }
}

fn local_chains(width: usize, depth: usize) -> (Body, Vec<(u32, u32)>) {
    let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
    let root = b.open_block();
    let mut consumers = Vec::new();
    for _ in 0..width {
        let owner = b.local(Ty::STRING, Symbol::NONE, 0, NodeIdx::NONE);
        let initial = call(&mut b, &[], Ty::STRING);
        b.set(owner, initial, NodeIdx::NONE);
        let place = b.place(&Place::local(owner));
        let mut reference = b.emit(Tag::Ref, place, ref_word::LOCAL, LOCAL, NodeIdx::NONE);
        for _ in 0..depth {
            reference = call(&mut b, &[reference], LOCAL);
        }
        let used = call(&mut b, &[reference], Ty::VOID).0;
        consumers.push((used, owner.raw()));
    }
    let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
    // ruled out: all builder blocks are closed and all values have types.
    (
        b.finish(root, &[]).expect("valid local-reference chains"),
        consumers,
    )
}

#[test]
fn all_cell_owners_are_solved_together_at_large_width_and_depth() {
    for (width, depth) in [(1000, 20), (20, 1000)] {
        let (body, consumers) = local_chains(width, depth);
        let cx = Cx::default();
        let mut uses = hd_base::HashSet::default();
        extra_cell_uses(&body, &cx, &Tables::default(), |_| true, &mut |i, l, _| {
            uses.insert((i, l));
        });
        for pair in consumers {
            assert!(uses.contains(&pair), "lost cell use {pair:?}");
        }
        assert!(
            cx.transfers.get() < 16 * body.len(),
            "transfers must be linear in instructions at width {width}, depth {depth}: {} for {} instructions",
            cx.transfers.get(),
            body.len()
        );
    }
}

// Reverse stores force the flow-insensitive solve to discover one new
// local at a time. The initial external references have no local origin;
// a late assignment of the local reference seeds the complete chain.
fn reverse_stores(width: usize, depth: usize) -> (Body, Vec<(u32, u32)>) {
    let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
    let root = b.open_block();
    let mut consumers = Vec::new();
    for _ in 0..width {
        let owner = b.local(Ty::STRING, Symbol::NONE, 0, NodeIdx::NONE);
        let initial = call(&mut b, &[], Ty::STRING);
        b.set(owner, initial, NodeIdx::NONE);
        let mut locals = Vec::new();
        for _ in 0..=depth {
            let l = b.local(LOCAL, Symbol::NONE, 0, NodeIdx::NONE);
            let value = call(&mut b, &[], LOCAL);
            b.set(l, value, NodeIdx::NONE);
            locals.push(l);
        }
        for i in (1..=depth).rev() {
            let p = b.place(&Place::local(locals[i - 1]));
            let value = b.emit(Tag::Read, p, 0, LOCAL, NodeIdx::NONE);
            b.set(locals[i], value, NodeIdx::NONE);
        }
        let place = b.place(&Place::local(owner));
        let reference = b.emit(Tag::Ref, place, ref_word::LOCAL, LOCAL, NodeIdx::NONE);
        b.set(locals[0], reference, NodeIdx::NONE);
        let p = b.place(&Place::local(locals[depth]));
        let value = b.emit(Tag::Read, p, 0, LOCAL, NodeIdx::NONE);
        consumers.push((call(&mut b, &[value], Ty::VOID).0, owner.raw()));
    }
    let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
    // ruled out: all blocks are closed and each local is initialized before use.
    (
        b.finish(root, &[]).expect("valid reverse local flows"),
        consumers,
    )
}

#[test]
fn cell_origin_worklist_does_not_rescan_the_body_per_alias_depth() {
    for (width, depth) in [(1000, 20), (20, 1000)] {
        let (body, consumers) = reverse_stores(width, depth);
        let cx = Cx::default();
        let mut uses = hd_base::HashSet::default();
        extra_cell_uses(&body, &cx, &Tables::default(), |_| true, &mut |i, l, _| {
            uses.insert((i, l));
        });
        for pair in consumers {
            assert!(uses.contains(&pair), "lost transitive cell use {pair:?}");
        }
        assert!(
            cx.transfers.get() < 32 * body.len(),
            "independent of width {width} and reverse-flow depth {depth}: {} queries for {} instructions",
            cx.transfers.get(),
            body.len()
        );
    }
}

#[test]
fn immutable_frame_queries_match_marking_at_width_and_depth() {
    let mut work = Vec::new();
    for (width, depth) in [(100, 20), (1000, 20), (20, 1000)] {
        let (mut body, _) = local_chains(width, depth);
        let topo = Topology::new(&body);
        let before = (body.data.clone(), body.cap_mode.clone());
        let cx = Cx::default();
        let mut observed = Vec::new();
        uses(&body, &topo, &cx, &mut Tables::default(), |i, l, f| {
            observed.push((i, l, f));
        });
        assert_eq!((&body.data, &body.cap_mode), (&before.0, &before.1));
        let marking = Cx::default();
        let mut marked = Vec::new();
        run(
            &mut body,
            &topo,
            &marking,
            &mut Tables::default(),
            |i, l, f| marked.push((i, l, f)),
        );
        assert_eq!(observed, marked);
        assert_eq!(cx.transfers.get(), marking.transfers.get());
        assert!(
            body.tags
                .iter()
                .enumerate()
                .any(|(i, &tag)| tag == Tag::Ref && body.data[i][1] & ref_word::FRAME != 0)
        );
        work.push(cx.transfers.get());
    }
    assert!(work[1] <= 11 * work[0], "tenfold width: {work:?}");
}
