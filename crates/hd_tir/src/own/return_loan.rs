//! Order-independent facts for the borrowed result convention. Only a
//! pass-through parameter qualifies: every use of its value forwards it
//! to the root function's result, never to another owner or address.

use super::{Role, operands, params};
use crate::ir::{Body, BodyKind, NONE, Ref, Root, Tag, Topology};

/// The parameter every return forwards, with no other use of its count.
/// This reads the checked body, not another body's ownership summary.
/// The driver caches the answer per declaration; instance layout decides
/// whether the parameter can use the borrowed convention.
#[must_use]
pub fn parameter(b: &Body) -> Option<u8> {
    parameter_with(b, || {})
}

fn parameter_with(b: &Body, mut visit: impl FnMut()) -> Option<u8> {
    if !matches!(b.kind, BodyKind::Fn | BodyKind::DeriveInstance) {
        return None;
    }
    let root = *b.sub_root.first()?;
    let tail = b.data[root as usize][1];
    if tail != NONE {
        let i = Ref(tail).as_inst()?.0 as usize;
        if !matches!(b.tags.get(i), Some(Tag::Read | Tag::Coerce)) {
            return None;
        }
    }
    let mut allowed = vec![0u32; b.len()];
    let mut forwards = vec![false; b.len()];
    let mut source = None;
    // Each coercion has one source edge, even if several returns forward
    // its result. The structural budget bounds every return's chain.
    let mut accept = |mut value: u32| -> Option<()> {
        allowed[Ref(value).as_inst()?.0 as usize] += 1;
        for _ in 0..16 {
            visit();
            let i = Ref(value).as_inst()?.0 as usize;
            match *b.tags.get(i)? {
                Tag::Read => {
                    let p = b.view(b.data[i][0]);
                    let local = p.local().filter(|_| p.whole())?;
                    if source.is_some_and(|old| old != local) {
                        return None;
                    }
                    source = Some(local);
                    forwards[i] = true;
                    return Some(());
                }
                Tag::Coerce if b.same_value_coercion(i) => {
                    let next = b.data[i][0];
                    if !std::mem::replace(&mut forwards[i], true) {
                        allowed[Ref(next).as_inst()?.0 as usize] += 1;
                    }
                    value = next;
                }
                _ => return None,
            }
        }
        None
    };
    if tail != NONE {
        accept(tail)?;
    } else if b.ty[root as usize] != hd_types::Ty::NEVER {
        return None;
    }
    let topo = Topology::new(b);
    for i in 0..b.len() {
        if b.tags[i] == Tag::Return && topo.sub[i] == 0 {
            accept(b.data[i][0])?;
        }
    }
    let source = source?;
    if b.cap_local.iter().any(|l| l.raw() == source) {
        return None;
    }
    let k = params(b).iter().position(|&l| l == source)?;
    let mut actual = vec![0u32; b.len()];
    let mut ops = Vec::new();
    for (i, &forward) in forwards.iter().enumerate() {
        visit();
        if b.tags[i].operands().0 == crate::ir::Op::Place {
            let p = b.view(b.data[i][0]);
            if p.root == Root::Local(source) && !(b.tags[i] == Tag::Read && forward) {
                return None;
            }
        }
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped && Ref(op.value).as_inst().is_some() {
                actual[op.value as usize] += 1;
            }
        }
    }
    for (i, &forward) in forwards.iter().enumerate() {
        if forward && actual[i] != allowed[i] {
            return None;
        }
    }
    u8::try_from(k).ok()
}

/// Forward address uses of a returned-argument call to that argument, while
/// retaining the call and its effects (LLVM `InstCombine`'s `returned` rule).
/// One SSA pass canonicalizes call chains; each shared place is copied once.
/// Cleanup still names the call result, whose loan lifetime is independent.
pub(super) fn forward_places(b: &mut Body, cx: &dyn super::OwnCx) {
    forward_places_with(b, |callee| cx.returned_loan(callee), |_| {});
}

fn forward_places_with(
    b: &mut Body,
    mut returned: impl FnMut(&crate::ir::Callee) -> Option<u8>,
    mut visit: impl FnMut(usize),
) {
    if !b.tags.contains(&Tag::Call) {
        return;
    }
    let mut sources: Vec<u32> = (0..b.len()).map(crate::ir::u32_of).collect();
    let mut found = false;
    for i in 0..b.len() {
        visit(1);
        if b.tags[i] != Tag::Call {
            continue;
        }
        let Some(callee) = crate::ir::Callee::from_words(b.record(b.data[i][0])) else {
            continue;
        };
        let Some(k) = returned(&callee) else {
            continue;
        };
        let Some(arg) = b.record(b.data[i][1]).get(usize::from(k)).copied() else {
            continue;
        };
        let Some(arg) = Ref(arg).as_inst().map(|v| v.0 as usize).filter(|&v| v < i) else {
            continue;
        };
        // Exact types preserve the interpretation of every field projection.
        // A representation-changing return needs its original result type.
        if b.ty[arg] == b.ty[i] {
            sources[i] = sources[arg];
            found = true;
        }
    }
    if !found {
        return;
    }
    let mut places = hd_base::HashMap::default();
    for i in 0..b.len() {
        visit(1);
        if !matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::Store | Tag::IsUnique) {
            continue;
        }
        let old = b.data[i][0];
        let Root::Value(value) = b.view(old).root else {
            continue;
        };
        let source = sources[value as usize];
        if source == value {
            continue;
        }
        let new = *places.entry(old).or_insert_with(|| {
            let mut words = b.record(old).to_vec();
            visit(words.len());
            words[1] = source;
            let at = crate::ir::u32_of(b.extra.len());
            b.extra.push(crate::ir::u32_of(words.len()));
            b.extra.extend(words);
            at
        });
        b.data[i][0] = new;
    }
}

/// Apply a proven root result convention before ownership insertion. The
/// driver first checks the declaration fact, concrete layout and call ABI.
/// Nested closures retain their independent result convention.
pub fn prepare(b: &mut Body) {
    if let Some(flags) = b.sub_flags.first_mut() {
        *flags |= crate::ir::sub_flags::BORROWED_RESULT;
    }
    let topo = Topology::new(b);
    for i in 0..b.len() {
        if b.tags[i] == Tag::Return && topo.sub[i] == 0 {
            b.data[i][1] = crate::ir::return_word::BORROWED;
        }
    }
}

/// Check the producer side independently of ABI planning. A borrowed
/// result must forward a Copy parameter whose storage and lifetime remain
/// valid for the call, and only its root returns carry the marker.
pub(crate) fn verify(b: &Body, cx: &dyn super::OwnCx, mut error: impl FnMut(u32, String)) {
    let lent = b
        .sub_flags
        .first()
        .is_some_and(|f| f & crate::ir::sub_flags::BORROWED_RESULT != 0);
    if lent {
        let valid = parameter(b)
            .and_then(|p| params(b).get(usize::from(p)).copied())
            .is_some_and(|p| {
                let ty = b.local_ty[p as usize];
                cx.counted(ty)
                    && cx.copies(ty) == super::Tri::Yes
                    && cx.holds_drop(ty) == super::Tri::No
            });
        if !valid {
            error(
                b.sub_root.first().copied().unwrap_or(NONE),
                "a borrowed result does not forward a count-bearing Copy parameter without Drop"
                    .into(),
            );
        }
    }
    let topo = Topology::new(b);
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::Return {
            continue;
        }
        let mode = b.data[i][1];
        let expected = if lent && topo.sub[i] == 0 {
            crate::ir::return_word::BORROWED
        } else {
            NONE
        };
        if mode != expected {
            error(
                crate::ir::u32_of(i),
                "a return's ownership mode differs from its function result".into(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Callee, Providers, TirBuilder, TirSink, local_flags, u32_of};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    fn body(extra_use: bool) -> Result<Body, String> {
        body_mode(extra_use, false)
    }

    fn body_mode(extra_use: bool, explicit: bool) -> Result<Body, String> {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let node = NodeIdx::from_raw(1);
        let block = b.open_block();
        let p = b.local(Ty::STRING, Symbol::from_raw(1), local_flags::PARAM, node);
        if extra_use {
            let value = b.get(p, Ty::STRING, node);
            b.call(
                &Callee::Item {
                    def: DefId::from_raw(8),
                    targs: TyList::EMPTY,
                },
                &[value],
                Providers::None,
                Ty::VOID,
                node,
            );
        }
        let value = b.get(p, Ty::STRING, node);
        let root = if explicit {
            b.emit(Tag::Return, value.0, NONE, Ty::NEVER, node);
            b.close_block(block, None, Ty::NEVER, node)
        } else {
            b.close_block(block, Some(value), Ty::STRING, node)
        };
        b.finish(root, &[]).map_err(|e| format!("{e:?}"))
    }

    struct Facts;
    crate::own::test_cx::own_cx!(Facts, [temporary_owner_name, parts], {
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
        }
        fn holds_drop(&self, _: Ty) -> super::super::Tri {
            super::super::Tri::No
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> super::super::Pass {
            super::super::Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> super::super::Pass {
            super::super::Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }

        fn received(&self, _: DefId, _: usize, _: usize) -> super::super::Pass {
            super::super::Pass::Lent
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    });

    #[test]
    fn a_borrowed_root_tail_and_explicit_return_keep_no_owned_count() -> Result<(), String> {
        for explicit in [false, true] {
            let mut b = body_mode(false, explicit)?;
            prepare(&mut b);
            super::super::run(&mut b, &Facts, &mut super::super::OwnScratch::default())
                .map_err(|e| format!("explicit={explicit}: {e}"))?;
            assert!(
                b.tags
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| **t == Tag::Read)
                    .all(|(i, _)| b.is_borrow(crate::ir::u32_of(i)))
            );
            assert!(b.exit_drops.is_empty());
            let errors = crate::ir::verify_owned(&b, &Facts);
            assert!(errors.is_empty(), "{errors:?}");
        }
        Ok(())
    }

    #[test]
    fn the_verifier_rejects_a_forged_borrowed_result() -> Result<(), String> {
        let mut b = body(true)?;
        prepare(&mut b);
        let mut errors = Vec::new();
        verify(&b, &Facts, |_, e| errors.push(e));
        assert_eq!(errors.len(), 1);
        Ok(())
    }

    #[test]
    fn a_pass_through_does_not_also_give_its_count_to_another_use() -> Result<(), String> {
        assert_eq!(parameter(&body(false)?), Some(0));
        assert_eq!(parameter(&body(true)?), None);
        Ok(())
    }

    #[test]
    fn pass_through_work_is_bounded_at_width_1000_depth_20() {
        for width in [1, 1000] {
            for depth in [1, 20] {
                let mut b = Body::new(DefId::from_raw(4), BodyKind::Fn);
                b.local_ty.push(Ty::STRING);
                b.local_flags.push(local_flags::PARAM);
                b.local_name.push(Symbol::from_raw(1));
                b.local_syn.push(NodeIdx::from_raw(1));
                // Several returns may share one coercion chain. There is
                // one source edge per coercion, not one per return path.
                b.tags.push(Tag::Read);
                b.data.push([crate::ir::place::BARE_LOCAL, 0]);
                b.ty.push(Ty::STRING);
                let mut value = 0;
                for _ in 0..depth {
                    let record = u32_of(b.extra.len());
                    b.extra
                        .extend([2, crate::ir::Coercion::Identity as u32, NONE]);
                    let next = u32_of(b.len());
                    b.tags.push(Tag::Coerce);
                    b.data.push([value, record]);
                    b.ty.push(Ty::STRING);
                    value = next;
                }
                let mut members = Vec::new();
                for _ in 0..width {
                    members.push(u32_of(b.len()));
                    b.tags.push(Tag::Return);
                    b.data.push([value, NONE]);
                    b.ty.push(Ty::NEVER);
                }
                let record = u32_of(b.extra.len());
                b.extra.push(u32_of(members.len()));
                b.extra.extend(members);
                b.sub_root.push(u32_of(b.len()));
                b.sub_params.push(hd_base::Range32::new(0, 0));
                b.sub_parent.push(hd_base::SubId::from_raw(NONE));
                b.sub_flags.push(0);
                b.tags.push(Tag::Block);
                b.data.push([record, NONE]);
                b.ty.push(Ty::NEVER);
                b.syn.resize(b.len(), NodeIdx::from_raw(1));
                let mut work = 0;
                let answer = parameter_with(&b, || work += 1);
                if depth < 16 {
                    assert_eq!(answer, Some(0));
                    assert_eq!(work, b.len() + width * (depth + 1));
                } else {
                    assert_eq!(answer, None);
                    assert_eq!(work, 16);
                }
            }
        }
    }
}

#[cfg(test)]
mod forwarded_place_tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, Place, PlaceRoot, Proj, Providers, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    const POINTER: Ty = Ty(1_000_000);

    fn generated(width: usize, depth: usize) -> (Body, Vec<(u32, u32, u32)>) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let node = NodeIdx::from_raw(0);
        let block = b.open_block();
        let param = b.local(POINTER, Symbol::NONE, crate::ir::local_flags::PARAM, node);
        let callee = Callee::Item {
            def: DefId::from_raw(8),
            targs: TyList::EMPTY,
        };
        let mut expected = vec![];
        for _ in 0..width {
            let source = b.get(param, POINTER, node);
            let mut value = source;
            for _ in 0..depth {
                value = b.call(&callee, &[value], Providers::None, POINTER, node);
            }
            let place = b.place(&Place {
                root: PlaceRoot::Value(value),
                proj: vec![Proj::Deref, Proj::Field(0)],
            });
            let read = b.emit(
                Tag::Read,
                place,
                crate::ir::ReadMode::Use as u32,
                Ty::STRING,
                node,
            );
            expected.push((read.0, source.0, value.0));
        }
        let root = b.close_block(block, None, Ty::VOID, node);
        (
            b.finish(root, &[]).expect("valid returned-place graph"),
            expected,
        )
    }

    #[test]
    fn returned_places_forward_in_linear_work_at_width_and_depth() {
        for width in [1_000, 10_000] {
            for depth in [1, 20] {
                let (mut b, expected) = generated(width, depth);
                let n = b.len();
                let mut work = 0;
                forward_places_with(&mut b, |_| Some(0), |n| work += n);
                assert_eq!(work, 2 * n + 4 * width);
                assert_eq!(
                    b.tags.iter().filter(|&&t| t == Tag::Call).count(),
                    width * depth
                );
                for (read, source, _) in expected {
                    let place = b.view(b.data[read as usize][0]);
                    assert_eq!(place.root, Root::Value(source));
                    assert_eq!(
                        place.steps(&b).collect::<Vec<_>>(),
                        [Proj::Deref, Proj::Field(0)]
                    );
                }
            }
        }
    }

    #[test]
    fn returned_places_preserve_calls_cleanup_and_unproved_results() {
        let (original, expected) = generated(1, 2);
        let (read, _source, call) = expected[0];
        let mut unknown = original.clone();
        forward_places_with(&mut unknown, |_| None, |_| {});
        assert_eq!(unknown, original);
        let mut different = original.clone();
        different.ty[call as usize] = Ty::I32;
        let before = different.clone();
        forward_places_with(&mut different, |_| Some(0), |_| {});
        assert_eq!(different.data[read as usize], before.data[read as usize]);
        let mut cleanup = original.clone();
        cleanup.tags[read as usize] = Tag::LoanEnd;
        let old = cleanup.data[read as usize];
        forward_places_with(&mut cleanup, |_| Some(0), |_| {});
        assert_eq!(cleanup.data[read as usize], old);
        assert_eq!(cleanup.tags[call as usize], Tag::Call);

        let (mut b, expected) = generated(2, 2);
        let (read, source, call) = expected[0];
        let other = expected[1].0 as usize;
        let old = b.data[read as usize][0];
        b.data[other][0] = old;
        let old_len = b.extra.len();
        forward_places_with(&mut b, |_| Some(0), |_| {});
        assert_eq!(b.extra.len(), old_len + 5, "shared place copied once");
        assert_eq!(b.data[read as usize][0], b.data[other][0]);
        assert_eq!(b.view(b.data[read as usize][0]).root, Root::Value(source));
        assert_eq!(b.tags[call as usize], Tag::Call);
        assert_eq!(b.record(old)[1], call, "old record remains immutable");
    }
}
