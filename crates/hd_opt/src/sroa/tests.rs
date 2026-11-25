use hd_base::{DefId, NodeIdx, Symbol};
use hd_tir::ir::{
    Body, BodyKind, Place, PlaceRoot, Proj, ReadMode, Tag, TirBuilder, TirSink, local_flags,
    ref_word, verify,
};
use hd_types::{InternPool, Ty, TyData};

use super::{WORK, run};

const N: NodeIdx = NodeIdx::from_raw(1);

/// `k` copies of an inlined writer: `$x = 0; $p = &mut $x; *$p = *$p + 1`,
/// each over its own local, then the sum of the locals.
fn writers(pool: &InternPool, k: usize) -> Body {
    let reference = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        mutable: true,
        counted: false,
    });
    let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
    let block = b.open_block();
    let mut sum = None;
    for _ in 0..k {
        let x = b.local(
            Ty::I32,
            Symbol::from_raw(0),
            local_flags::READ | local_flags::ASSIGNED | local_flags::MUTATED,
            N,
        );
        let p = b.local(reference, Symbol::from_raw(1), local_flags::READ, N);
        let zero = b.const_value(Ty::I32, 0);
        b.set(x, zero, N);
        let at = b.place(&Place::local(x));
        let r = b.emit(Tag::Ref, at, ref_word::MUT, reference, N);
        b.set(p, r, N);
        let through = b.place(&Place {
            root: PlaceRoot::Local(p),
            proj: vec![Proj::Deref],
        });
        let old = b.emit(Tag::Read, through, ReadMode::Use as u32, Ty::I32, N);
        let one = b.const_value(Ty::I32, 1);
        let new = b.prim(hd_tir::ir::PrimOp::Add as u32, &[old, one], Ty::I32, N);
        b.emit(Tag::Store, through, new.0, Ty::VOID, N);
        let value = b.get(x, Ty::I32, N);
        sum = Some(match sum {
            None => value,
            Some(s) => b.prim(hd_tir::ir::PrimOp::Add as u32, &[s, value], Ty::I32, N),
        });
    }
    let root = b.close_block(block, sum, Ty::I32, N);
    // ruled out: every local is stored before it is read, in one block.
    b.finish(root, &[]).expect("the writers verify")
}

fn work_of(k: usize) -> (usize, Body) {
    let pool = InternPool::default();
    let mut body = writers(&pool, k);
    WORK.with(|w| w.set(0));
    assert_eq!(run(&mut body, &pool, &|_| true), Ok(true));
    (WORK.with(std::cell::Cell::get), body)
}

#[test]
fn forwarded_writers_leave_no_address_and_the_work_is_linear() {
    let (small, body) = work_of(100);
    let (large, _) = work_of(1000);
    // No `Ref` and no binding is left: each `$x` lives in Wasm locals.
    assert_eq!(body.tags.iter().filter(|&&t| t == Tag::Ref).count(), 0);
    let through = (0..body.len())
        .filter(|&i| matches!(body.tags[i], Tag::Read | Tag::Store))
        .filter_map(|i| body.decode_place(body.data[i][0]))
        .filter(|p| p.proj.contains(&Proj::Deref))
        .count();
    assert_eq!(through, 0);
    assert_eq!(verify(&body), []);
    // Ten times the body, ten times the work, within rounding.
    assert!(
        small > 0 && large <= 11 * small,
        "work {small} then {large}"
    );
    assert!(large >= 9 * small, "work {small} then {large}");
}
