use hd_base::{DefId, NodeIdx, Symbol};
use hd_tir::ir::{
    Body, BodyKind, NONE, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, TirBuilder, TirSink,
    local_flags,
};
use hd_types::{InternPool, Ty};

use super::{WORK, run};

const N: NodeIdx = NodeIdx::from_raw(1);

/// `k` copies of an inlined `next()` matched at once: a block that breaks
/// with `.None` when `$i >= 10` and else with `.Some($i)`, under a
/// `Scope`, then `match` binding `.Some`'s payload to `$y`.
fn matches(pool: &InternPool, k: usize) -> Body {
    // The enum's type is opaque to the pass: any type not `void` stands in.
    let _ = pool;
    let opt = Ty::BOOL;
    let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
    let root = b.open_block();
    let flags = local_flags::READ | local_flags::ASSIGNED;
    for _ in 0..k {
        let i = b.local(Ty::I32, Symbol::from_raw(0), flags, N);
        let y = b.local(Ty::I32, Symbol::from_raw(1), flags, N);
        let zero = b.const_value(Ty::I32, 0);
        b.set(i, zero, N);
        // The producer: its label is the next row, written once it closes.
        let label = u32::try_from(b.body().label_inst.len()).unwrap();
        b.body_mut().label_inst.push(NONE);
        let blk = b.open_block();
        let at = b.get(i, Ty::I32, N);
        let ten = b.const_value(Ty::I32, 10);
        let done = b.prim(hd_tir::ir::PrimOp::Ge as u32, &[at, ten], Ty::BOOL, N);
        let taken = b.open_block();
        let none = b.refs_record(&[]);
        let nv0 = b.emit(Tag::NewVariant, 0, none, opt, N);
        b.emit(Tag::Break, label, nv0.0, Ty::NEVER, N);
        let taken = b.close_block(taken, None, Ty::NEVER, N);
        let arms = b.refs_record(&[taken, Ref(NONE)]);
        b.emit(Tag::If, done.0, arms, Ty::VOID, N);
        let v = b.get(i, Ty::I32, N);
        let some = b.refs_record(&[v]);
        let nv1 = b.emit(Tag::NewVariant, 1, some, opt, N);
        b.emit(Tag::Break, label, nv1.0, Ty::NEVER, N);
        let blk = b.close_block(blk, None, opt, N);
        b.body_mut().label_inst[label as usize] = blk.0;
        let sm = b.open_scope();
        let s = b.close_scope(sm, blk, opt, N);
        // Arm 0 (`.None`) and arm 1 (`.Some(y)`) do nothing.
        let a0 = b.open_block();
        let a0 = b.close_block(a0, None, Ty::VOID, N);
        let a1 = b.open_block();
        let a1 = b.close_block(a1, None, Ty::VOID, N);
        let dec = b.open_block();
        let c0 = b.open_block();
        b.emit(Tag::ToArm, 0, NONE, Ty::NEVER, N);
        let c0 = b.close_block(c0, None, Ty::NEVER, N);
        let c1 = b.open_block();
        let payload = b.place(&Place {
            root: PlaceRoot::Value(s),
            proj: vec![Proj::Variant(1), Proj::Field(0)],
        });
        let r = b.emit(Tag::Read, payload, ReadMode::Use as u32, Ty::I32, N);
        b.set(y, r, N);
        b.emit(Tag::ToArm, 1, NONE, Ty::NEVER, N);
        let c1 = b.close_block(c1, None, Ty::NEVER, N);
        let cases = b.refs_record(&[Ref(0), c0, Ref(1), c1, Ref(NONE)]);
        b.emit(Tag::SwitchTag, s.0, cases, Ty::NEVER, N);
        let dec = b.close_block(dec, None, Ty::NEVER, N);
        let blocks = b.refs_record(&[dec, a0, a1]);
        b.emit(Tag::Match, s.0, blocks, Ty::VOID, N);
    }
    let root = b.close_block(root, None, Ty::VOID, N);
    // ruled out: every local is stored before it is read, in one block.
    b.finish(root, &[]).expect("the matches build")
}

fn work_of(k: usize) -> (usize, Body) {
    let pool = InternPool::default();
    let mut body = matches(&pool, k);
    WORK.with(|w| w.set(0));
    assert_eq!(run(&mut body, &|_| true), Ok(true));
    (WORK.with(std::cell::Cell::get), body)
}

#[test]
fn known_constructors_jump_to_their_arms_and_the_work_is_linear() {
    let (small, body) = work_of(100);
    let (large, _) = work_of(1000);
    // No match, switch or constructor is left: each exit stores `$y` and
    // jumps to its arm's landing.
    for tag in [Tag::Match, Tag::SwitchTag, Tag::NewVariant, Tag::ToArm] {
        assert_eq!(
            body.tags.iter().filter(|&&t| t == tag).count(),
            0,
            "{tag:?}"
        );
    }
    // Every label names a block, and every `Break` a label.
    assert!(body.label_inst.iter().all(|&l| (l as usize) < body.len()));
    // Ten times the body, ten times the work, within rounding.
    assert!(
        small > 0 && large <= 11 * small,
        "work {small} then {large}"
    );
}

#[test]
fn a_scrutinee_used_after_the_match_stays_built() {
    let pool = InternPool::default();
    let mut body = matches(&pool, 1);
    // A second use of the scrutinee: a `Store` of it into a fresh local.
    let s = (0..body.len())
        .find(|&i| body.tags[i] == Tag::Match)
        .map(|m| body.data[m][0])
        .unwrap();
    body.local_ty.push(body.ty[s as usize]);
    body.local_name.push(Symbol::from_raw(2));
    body.local_syn.push(N);
    body.local_flags
        .push(local_flags::READ | local_flags::ASSIGNED);
    let keep = hd_tir::ir::place::BARE_LOCAL | u32::try_from(body.local_ty.len() - 1).unwrap();
    let root = body.sub_root[0] as usize;
    let store = u32::try_from(body.len()).unwrap();
    body.tags.push(Tag::Store);
    body.data.push([keep, s]);
    body.ty.push(Ty::VOID);
    body.syn.push(N);
    let mut members = body.record(body.data[root][0]).to_vec();
    members.insert(members.len(), store);
    body.data[root][0] = body.push_record(&members);
    // The store comes after the root block in the columns; the pass only
    // counts uses, so it sees two uses and keeps the match.
    assert_eq!(run(&mut body, &|_| true), Ok(false));
    assert_eq!(body.tags.iter().filter(|&&t| t == Tag::Match).count(), 1);
}
