use super::verify_owned;
use crate::ir::{
    Body, BodyKind, Callee, ChoiceKind, Phase, Providers, Tag, TirBuilder, TirSink as _,
    local_flags, sub_flags,
};
use hd_base::{DefId, NodeIdx, Symbol};
use hd_types::{InternPool, ParamRef, Ty, TyData};

use crate::own::Pass;

/// The passes' facts with a pool, so the instance check runs.
struct Pooled<'a>(&'a InternPool);

crate::own::test_cx::own_cx!(Pooled<'_>, [temporary_owner_name, may_write, parts], {
    fn counted(&self, _: Ty) -> bool {
        false
    }
    fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
        Pass::Lent
    }
    fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
        Pass::Lent
    }

    fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
        Pass::Lent
    }
    fn holds_coll(&self, _: Ty) -> bool {
        false
    }
    fn pool(&self) -> Option<&InternPool> {
        Some(self.0)
    }
    fn shown(&self, t: Ty) -> String {
        format!("t{}", t.0)
    }
});

const F: DefId = DefId::from_raw(1);

fn param(pool: &InternPool, index: u16) -> Ty {
    pool.intern_ty(&TyData::Param(ParamRef { owner: F, index }))
}

/// `fn f(x: t) -> t: g::[t](x)`, marked an instance.
fn body(pool: &InternPool, t: Ty) -> Body {
    let n = NodeIdx::from_raw(0);
    let mut b = TirBuilder::new(F, BodyKind::Fn);
    let blk = b.open_block();
    let x = b.local(t, Symbol::from_raw(0), local_flags::PARAM, n);
    let get = b.get(x, t, n);
    let callee = Callee::Item {
        def: DefId::from_raw(2),
        targs: pool.list(&[t]),
    };
    let call = b.call(&callee, &[get], Providers::None, t, n);
    b.emit(Tag::Return, call.0, crate::ir::NONE, Ty::NEVER, n);
    let root = b.close_block(blk, None, Ty::NEVER, n);
    let mut body = b.finish(root, &[]).expect("verifies");
    body.phase = Phase::Instance;
    body
}

/// The lowered-TIR contract (invariant 25) holds of an elaborated body
/// and refuses a cleanup scope and a flag no drop reads, which only a
/// backend could still read.
#[test]
fn an_elaborated_body_keeps_no_cleanup_scope_or_unread_flag() {
    let pool = InternPool::new();
    let mut b = body(&pool, Ty::I32);
    b.phase = Phase::Elaborated;
    let lowered = |b: &Body| -> Vec<String> {
        let errs = verify_owned(b, &Pooled(&pool)).into_iter();
        errs.filter(|e| e.invariant == 25).map(|e| e.what).collect()
    };
    assert!(lowered(&b).is_empty(), "{:?}", lowered(&b));
    let root = b.sub_root[0] as usize;
    let flag = crate::ir::u32_of(b.local_ty.len());
    b.local_ty.push(Ty::BOOL);
    b.local_name.push(Symbol::from_raw(0));
    b.local_syn.push(NodeIdx::from_raw(0));
    b.local_flags.push(0);
    for (tag, data) in [
        (Tag::FlagSet, [crate::ir::place::BARE_LOCAL, flag]),
        (Tag::Defer, [b.sub_root[0], 0]),
    ] {
        let mut list = b.record(b.data[root][0]).to_vec();
        list.insert(0, crate::ir::u32_of(b.len()));
        b.tags.push(tag);
        b.data.push(data);
        b.ty.push(Ty::VOID);
        b.syn.push(NodeIdx::from_raw(0));
        b.data[root][0] = b.push_record(&list);
    }
    let found = lowered(&b);
    assert!(
        found.iter().any(|w| w.contains("no elaborated drop reads")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|w| w.contains("cleanup scope")),
        "{found:?}"
    );
}

/// The body's invariant 24 findings.
fn found(b: &Body, pool: &InternPool) -> Vec<String> {
    verify_owned(b, &Pooled(pool))
        .into_iter()
        .filter(|e| e.invariant == 24)
        .map(|e| e.what)
        .collect()
}

/// `fn f(x: t): x.m()`, a trait call chosen `choice` at `t`.
fn trait_call(pool: &InternPool, t: Ty, choice: (ChoiceKind, u32)) -> Body {
    let n = NodeIdx::from_raw(0);
    let mut b = TirBuilder::new(F, BodyKind::Fn);
    let blk = b.open_block();
    let x = b.local(t, Symbol::from_raw(0), local_flags::PARAM, n);
    let get = b.get(x, t, n);
    let callee = Callee::TraitMethod {
        trait_: DefId::from_raw(5),
        method: DefId::from_raw(6),
        self_ty: t,
        targs: pool.list(&[]),
        choice,
    };
    b.call(&callee, &[get], Providers::None, Ty::VOID, n);
    let root = b.close_block(blk, None, Ty::VOID, n);
    let mut body = b.finish(root, &[]).expect("verifies");
    body.phase = Phase::Instance;
    body
}

/// A bound's call left at a concrete receiver is rejected; one chosen
/// as an impl or builtin passes, as does a bound's call at a trait
/// value or a reference to one, which dispatches through its table.
#[test]
fn a_bound_call_passes_only_at_a_trait_value() {
    let pool = InternPool::new();
    let bound = (ChoiceKind::Bound, 0);
    assert_eq!(
        found(&trait_call(&pool, Ty::I32, bound), &pool),
        [format!(
            "a trait call chosen through a bound at `t{}`",
            Ty::I32.0
        )]
    );
    for choice in [(ChoiceKind::Impl, 9), (ChoiceKind::Builtin, 1)] {
        assert!(found(&trait_call(&pool, Ty::I32, choice), &pool).is_empty());
    }
    let dyn_ = pool.intern_ty(&TyData::TraitValue {
        def: DefId::from_raw(5),
        args: pool.list(&[]),
        bindings: Vec::new(),
    });
    let by_ref = pool.intern_ty(&TyData::Ref {
        inner: dyn_,
        mutable: false,
        counted: false,
    });
    for t in [dyn_, by_ref] {
        assert!(found(&trait_call(&pool, t, bound), &pool).is_empty());
    }
    // A receiver naming a parameter is the type check's finding only.
    let what = found(&trait_call(&pool, param(&pool, 0), bound), &pool);
    assert!(
        what.iter().all(|w| !w.starts_with("a trait call")),
        "{what:?}"
    );
}

#[test]
fn a_concrete_instance_body_passes() {
    let pool = InternPool::new();
    assert!(found(&body(&pool, Ty::I32), &pool).is_empty());
}

/// A checked body may name its item's parameters, and a context
/// without a pool checks no types.
#[test]
fn only_an_instance_body_with_a_pool_is_checked() {
    let pool = InternPool::new();
    let mut b = body(&pool, param(&pool, 0));
    assert!(!found(&b, &pool).is_empty());
    assert!(
        verify_owned(&b, &crate::own::tests::Cx)
            .iter()
            .all(|e| e.invariant != 24)
    );
    b.phase = Phase::Checked;
    assert!(found(&b, &pool).is_empty());
}

/// The parameter is reported in each column it reaches: the
/// instruction and local columns and the callee's type arguments.
#[test]
fn a_type_parameter_is_rejected_in_every_column() {
    let pool = InternPool::new();
    let t = param(&pool, 0);
    let mut b = body(&pool, t);
    b.consts.push((t, 0));
    let what = found(&b, &pool);
    let name = format!("`t{}` is generic", t.0);
    assert!(what.iter().any(|w| w == &name), "{what:?}");
    let other = format!("a local or constant: {name}");
    assert_eq!(what.iter().filter(|&w| w == &other).count(), 2, "{what:?}");
    assert!(
        what.iter()
            .any(|w| w.starts_with("record word") && w.ends_with(&name)),
        "{what:?}"
    );
}

#[test]
fn an_unnormalized_projection_is_rejected() {
    let pool = InternPool::new();
    let t = pool.intern_ty(&TyData::Assoc {
        assoc: DefId::from_raw(5),
        trait_: DefId::from_raw(6),
        self_ty: Ty::I32,
        args: pool.list(&[]),
    });
    let what = found(&body(&pool, t), &pool);
    assert!(what.contains(&format!("`t{}` is generic", t.0)), "{what:?}");
}

#[test]
fn an_unopened_opaque_result_is_rejected() {
    let pool = InternPool::new();
    let t = pool.intern_ty(&TyData::Opaque {
        owner: DefId::from_raw(5),
        site: 0,
        borrows: false,
        args: pool.list(&[]),
        bound: Ty::I32,
    });
    let what = found(&body(&pool, t), &pool);
    assert!(what.contains(&format!("`t{}` is generic", t.0)), "{what:?}");
}

/// A generic local function's sub-body is rejected, and its own
/// parameters with it, as any other parameter.
#[test]
fn a_generic_local_function_is_rejected() {
    let pool = InternPool::new();
    let own = param(&pool, 1);
    let mut b = body(&pool, own);
    b.sub_flags[0] |= sub_flags::GENERIC;
    let what = found(&b, &pool);
    assert_eq!(what[0], "sub-body 0 is a generic local function");
    assert!(
        what.contains(&format!("`t{}` is generic", own.0)),
        "{what:?}"
    );
}

#[test]
fn a_move_through_a_reference_is_not_a_local_emptying() {
    use crate::ir::{Place, PlaceRoot, Proj, ReadMode, Topology};
    let pool = InternPool::new();
    let reference = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        mutable: true,
        counted: false,
    });
    let mut b = TirBuilder::new(F, BodyKind::Fn);
    let root = b.open_block();
    let slot = b.local(reference, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
    let place = b.place(&Place {
        root: PlaceRoot::Local(slot),
        proj: vec![Proj::Deref],
    });
    let moved = b.emit(
        Tag::Read,
        place,
        ReadMode::Use as u32,
        Ty::I32,
        NodeIdx::NONE,
    );
    let root = b.close_block(root, Some(moved), Ty::I32, NodeIdx::NONE);
    let mut body = b.finish(root, &[]).expect("reference-read shape");
    body.phase = Phase::Elaborated;
    body.data[moved.0 as usize][1] = ReadMode::Move as u32;
    assert!(super::emptied(
        &body,
        &Topology::new(&body),
        moved.0 as usize
    ));
}
