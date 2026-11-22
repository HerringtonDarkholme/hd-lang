//! Inline-cost contracts for source bindings that lower to scalar SSA values.
use hd_base::{DefId, NodeIdx, Symbol};
use hd_tir::inline::{inlinable, inlinable_scalar_bindings};
use hd_tir::ir::{Body, BodyKind, Place, Tag, TirBuilder, TirSink, local_flags};
use hd_types::{InternPool, Prim, Ty, TyData};

fn bindings(ty: Ty, addressed: Option<Ty>) -> Body {
    let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
    let block = builder.open_block();
    let parameter = builder.local(ty, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
    let mut value = builder.get(parameter, ty, NodeIdx::NONE);
    for _ in 0..8 {
        let local = builder.local(ty, Symbol::NONE, local_flags::SHORT, NodeIdx::NONE);
        builder.set(local, value, NodeIdx::NONE);
        value = builder.get(local, ty, NodeIdx::NONE);
    }
    if let Some(reference) = addressed {
        let place = builder.place(&Place::local(parameter));
        builder.emit(Tag::Ref, place, 0, reference, NodeIdx::NONE);
    }
    let root = builder.close_block(block, Some(value), ty, NodeIdx::NONE);
    builder.finish(root, &[]).expect("binding chain verifies")
}

#[test]
fn scalar_bindings_cost_no_operations_but_keep_the_structural_budget_separate() {
    for primitive in Prim::ALL.into_iter().filter(|&p| p != Prim::String) {
        let body = bindings(Ty::prim(primitive), None);
        assert!(!inlinable(&body, 1), "the ordinary structural budget stays");
        assert!(inlinable_scalar_bindings(&body, 1), "{primitive:?}");
        assert!(!inlinable_scalar_bindings(&body, 0), "the root costs one");
    }
}

#[test]
fn counted_unknown_captured_and_addressed_storage_keeps_its_cost() {
    let pool = InternPool::new();
    let aggregate = pool.intern_ty(&TyData::Tuple {
        elems: pool.list(&[Ty::I32]),
        rest: None,
    });
    let reference = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        mutable: false,
        counted: false,
    });
    for ty in [Ty::STRING, aggregate, reference] {
        assert!(!inlinable_scalar_bindings(&bindings(ty, None), 1));
    }
    assert!(!inlinable_scalar_bindings(
        &bindings(Ty::I32, Some(reference)),
        2
    ));
    for flag in [local_flags::CAPTURED, local_flags::CAPTURED_ASSIGNED] {
        let mut body = bindings(Ty::I32, None);
        body.local_flags[0] |= flag;
        assert!(!inlinable_scalar_bindings(&body, 1));
    }
}
