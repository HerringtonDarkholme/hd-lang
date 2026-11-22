//! Ownership guards and equivalent IR representations from real checked programs.
use std::cell::Cell;

use hd_base::BuildProfile;
use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_tir::ir::{
    Body, Callee, Coercion, IntrinsicOp, NONE, ReadMode, Root, Storage, Tag, Topology, count,
    local_flags, place, ref_word, return_word, u32_of, unique_word,
};
use hd_tir::own::{BatchInsert, insert_batch};

use super::owned_cache_tests::{VERIFIED, build_at};

thread_local! {
    static CORRUPTED: Cell<Option<[usize; 16]>> = const { Cell::new(None) };
    static CHECKED_VARIANTS: Cell<Option<[usize; 3]>> = const { Cell::new(None) };
}

pub(super) fn active() -> bool {
    CORRUPTED.get().is_some()
}

pub(super) fn verify_checked_variants(env: &dyn hd_tir::own::OwnCx, body: &Body) {
    let Some(mut counts) = CHECKED_VARIANTS.take() else {
        return;
    };

    if counts[0] == 0 {
        for (i, &tag) in body.tags.iter().enumerate() {
            if tag != Tag::Intrinsic
                || IntrinsicOp::from_u32(body.data[i][0]) != Some(IntrinsicOp::ListPush)
            {
                continue;
            }
            let Some(&arg) = body.record(body.data[i][1]).first() else {
                continue;
            };
            let Some(arg_ref) = hd_tir::ir::Ref(arg).as_inst() else {
                continue;
            };
            if body.tags[arg_ref.0 as usize] != Tag::Read {
                continue;
            }
            let mut variant = body.clone();
            let topo = Topology::new(body);
            let block = topo.blk[i];
            let pos = body
                .record(body.data[block as usize][0])
                .iter()
                .position(|&v| v == u32_of(i))
                .map(u32_of)
                .expect("push in block");
            let identity = u32_of(variant.extra.len());
            variant
                .extra
                .extend_from_slice(&[2, Coercion::Identity as u32, NONE]);
            variant.extra[body.data[i][1] as usize + 1] = u32_of(body.len());
            let coerce = BatchInsert {
                block,
                pos,
                tag: Tag::Coerce,
                data: [arg, identity],
                ty: body.ty[arg_ref.0 as usize],
                syn: body.syn[i],
            };
            insert_batch(&mut variant, &topo, &[coerce]).expect("identity collection view");
            let errors = hd_tir::ir::verify_checked(&variant);
            assert!(
                errors.iter().all(|e| e.invariant == 22),
                "identity checked body: {errors:?}"
            );
            let mut scratch = hd_tir::own::OwnScratch::default();
            hd_tir::own::run(&mut variant, env, &mut scratch)
                .expect("ownership through identity view");
            // An SSA value view is not a writable collection holder.
            // Following its provenance must not hide the missing owner.
            let errors = hd_tir::ir::verify_owned(&variant, env);
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("leaves its buffer in no place")),
                "collection mutation through a value view: {errors:?}"
            );
            counts[0] += 1;
            break;
        }
    }
    if counts[1] == 0 && body.tags.contains(&Tag::Loop) {
        let topo = Topology::new(body);
        for (i, &tag) in body.tags.iter().enumerate() {
            if tag != Tag::Match || topo.loop_of[i] == NONE {
                continue;
            }
            let [decision, _] = *body.record(body.data[i][1]) else {
                continue;
            };
            let Some(&last) = body.record(body.data[decision as usize][0]).last() else {
                continue;
            };
            if body.tags[last as usize] != Tag::ToArm {
                continue;
            }
            let mut trapping = body.clone();
            // A guard that never completes has no unconditional arm,
            // even when the match has only one syntactic arm.
            trapping.tags[last as usize] = Tag::Unreachable;
            trapping.data[last as usize] = [NONE, NONE];
            let errors = hd_tir::ir::verify_checked(&trapping);
            assert!(
                errors.iter().all(|e| e.invariant == 22),
                "trapping decision: {errors:?}"
            );
            let mut scratch = hd_tir::own::OwnScratch::default();
            hd_tir::own::run(&mut trapping, env, &mut scratch).expect("trapping match ownership");
            let errors = hd_tir::ir::verify_owned(&trapping, env);
            assert!(errors.is_empty(), "trapping match: {errors:?}");
            counts[1] += 1;
            break;
        }
    }
    if counts[2] == 0 && env.summarizes() && !env.suspending(body.item) {
        for (local, &ty) in body.local_ty.iter().enumerate() {
            if !env.plain_reference(ty)
                || env.reference_target(ty).is_none()
                || !body.tags.iter().enumerate().any(|(i, &tag)| {
                    if tag != Tag::Store
                        || body.view(body.data[i][0]).local() != Some(u32_of(local))
                    {
                        return false;
                    }
                    let Some(value) = hd_tir::ir::Ref(body.data[i][1]).as_inst() else {
                        return false;
                    };
                    let value = value.0 as usize;
                    if body.tags[value] != Tag::Ref {
                        return false;
                    }
                    let Some(base) = body.view(body.data[value][0]).local() else {
                        return false;
                    };
                    body.local_flags[base as usize] & hd_tir::ir::local_flags::PARAM == 0
                        && !env.plain_reference(body.local_ty[base as usize])
                })
            {
                continue;
            }
            let root = body.sub_root[0];
            let pos = u32_of(body.record(body.data[root as usize][0]).len());
            let topo = Topology::new(body);
            let mut loan = body.clone();
            let end = BatchInsert {
                block: root,
                pos,
                tag: Tag::LoanEnd,
                data: [hd_tir::ir::place::BARE_LOCAL | u32_of(local), NONE],
                ty: hd_types::Ty::VOID,
                syn: body.syn[root as usize],
            };
            insert_batch(&mut loan, &topo, &[end]).expect("explicit local loan end");
            hd_tir::own::run(&mut loan, env, &mut hd_tir::own::OwnScratch::default())
                .expect("alias forwarding keeps an explicit loan end valid");
            let errors = hd_tir::ir::verify_owned(&loan, env);
            assert!(errors.is_empty(), "forwarded loan end: {errors:?}");
            let mut invalid = body.clone();
            let unexpected = BatchInsert {
                tag: Tag::IsUnique,
                ..end
            };
            insert_batch(&mut invalid, &topo, &[unexpected]).expect("unexpected ownership place");
            let result =
                hd_tir::own::run(&mut invalid, env, &mut hd_tir::own::OwnScratch::default());
            assert!(
                matches!(result, Err(ref e) if e.contains("an ownership statement")),
                "{result:?}"
            );
            counts[2] += 1;
            break;
        }
    }
    CHECKED_VARIANTS.set(Some(counts));
}

pub(super) fn verify_fresh(env: &dyn hd_tir::own::OwnCx, body: &Body) {
    if let Some(mut counts) = CORRUPTED.take() {
        if counts.iter().all(|&count| count != 0) {
            CORRUPTED.set(Some(counts));
            return;
        }

        if counts[0] == 0
            && let Some(ret) = body.tags.iter().position(|t| *t == Tag::Return)
        {
            let mut bad = body.clone();
            bad.data[ret][1] = if bad.data[ret][1] == return_word::BORROWED {
                hd_tir::ir::NONE
            } else {
                return_word::BORROWED
            };
            let errors = hd_tir::ir::verify_owned(&bad, env);
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("a return's ownership mode"))
            );
            counts[0] += 1;
        }
        // Re-running insertion is meaningful only for a scalar body
        // with no ownership operations already inserted. Its actual
        // callee/signature environment is the build's, not a mock.
        let scalar = body
            .ty
            .iter()
            .chain(&body.local_ty)
            .all(|&ty| !hd_tir::own::OwnCx::counted(env, ty))
            && !body.tags.iter().any(|t| {
                matches!(
                    t,
                    Tag::Drop
                        | Tag::LoanEnd
                        | Tag::Retain
                        | Tag::Release
                        | Tag::MakeUnique
                        | Tag::IsUnique
                        | Tag::FlagSet
                        | Tag::FlagClear
                )
            });
        if scalar {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::Call {
                    continue;
                }
                if counts[1] == 0 && body.call_args(i).is_empty() {
                    let mut bad = body.clone();
                    let record = bad.data[i][1] as usize;
                    bad.extra[record] = 0;
                    let mut scratch = hd_tir::own::OwnScratch::default();
                    let result = hd_tir::own::run(&mut bad, env, &mut scratch);
                    assert!(
                        matches!(result, Err(ref e) if e.contains("missing call provider footer")),
                        "{result:?}"
                    );
                    counts[1] += 1;
                }
                if counts[2] == 0 {
                    let mut bad = body.clone();
                    let record = bad.data[i][1] as usize;
                    let footer = record + bad.extra[record] as usize - 2;
                    bad.extra[footer] = 3;
                    let mut scratch = hd_tir::own::OwnScratch::default();
                    let result = hd_tir::own::run(&mut bad, env, &mut scratch);
                    assert!(
                        matches!(result, Err(ref e) if e.contains("malformed call provider footer")),
                        "{result:?}"
                    );
                    counts[2] += 1;
                }
                if counts[3] == 0 {
                    let mut bad = body.clone();
                    let callee = bad.data[i][0] as usize;
                    bad.extra[callee] = 0;
                    // An unknown callee must conservatively invalidate
                    // element loans and retain an external write.
                    assert!(hd_tir::own::WriteEffects::new(&bad).writes(env));
                    counts[3] += 1;
                }
                if counts[1..4].iter().all(|&count| count > 0) {
                    break;
                }
            }
        }
        if counts[4] == 0 {
            let facts = hd_tir::own::ElementChecks::new(body);
            for i in 0..body.len() {
                if !facts.repeats_bounds(i) {
                    continue;
                }
                let index = body
                    .view(body.data[i][0])
                    .steps(body)
                    .find_map(|step| {
                        if let hd_tir::ir::Proj::Elem(index) = step {
                            Some(index)
                        } else {
                            None
                        }
                    })
                    .expect("repeated bound has an element index");
                let mut bad = body.clone();
                if let Some(inst) = index.as_inst() {
                    bad.ty[inst.0 as usize] = hd_types::Ty::I32;
                } else {
                    bad.consts[(index.0 & !hd_tir::ir::Ref::CONST_BIT) as usize].0 =
                        hd_types::Ty::I32;
                }
                assert!(!hd_tir::own::ElementChecks::new(&bad).repeats_bounds(i));
                counts[4] += 1;
                break;
            }
        }
        if counts[5] == 0 {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::CallValue
                    || !body.call_args(i).iter().any(|&arg| {
                        let value = hd_tir::ir::Ref(arg);
                        let ty = value.as_inst().map_or_else(
                            || body.consts[(arg & !hd_tir::ir::Ref::CONST_BIT) as usize].0,
                            |inst| body.ty[inst.0 as usize],
                        );
                        env.counted(ty) && env.copies(ty) == hd_tir::own::Tri::No
                    })
                {
                    continue;
                }
                let callee = hd_tir::ir::Ref(body.data[i][0])
                    .as_inst()
                    .expect("source function read");
                let mut constant = body.clone();
                let at = u32::try_from(constant.consts.len()).expect("constant table");
                constant.consts.push((body.ty[callee.0 as usize], 0));
                constant.data[i][0] = hd_tir::ir::Ref::CONST_BIT | at;
                let errors = hd_tir::ir::verify_owned(&constant, env);
                assert!(errors.is_empty(), "constant callable: {errors:?}");
                counts[5] += 1;
                break;
            }
        }
        if counts[6] == 0 {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::Intrinsic
                    || !matches!(
                        IntrinsicOp::from_u32(body.data[i][0]),
                        Some(
                            IntrinsicOp::ListPush
                                | IntrinsicOp::ListReserve
                                | IntrinsicOp::MapSet
                                | IntrinsicOp::MapRemove
                                | IntrinsicOp::MapSlotEntryMut
                        )
                    )
                {
                    continue;
                }
                let mut invalid = body.clone();
                let first = invalid.data[i][1] as usize + 1;
                invalid.extra[first] = u32::try_from(i).expect("invalid intrinsic operand");
                assert!(hd_tir::own::WriteEffects::new(&invalid).writes(env));
                counts[6] += 1;
                break;
            }
        }
        if counts[7] == 0
            && scalar
            && let Some(read) = body.tags.iter().position(|&tag| tag == Tag::Read)
        {
            let mut cyclic = body.clone();
            let place = u32::try_from(cyclic.extra.len()).expect("cycle place");
            cyclic.extra.extend_from_slice(&[
                2,
                hd_tir::ir::place::ROOT_VALUE,
                u32::try_from(read).expect("cycle read"),
            ]);
            cyclic.data[read] = [place, hd_tir::ir::ReadMode::Borrow as u32];
            let errors = hd_tir::ir::verify_owned(&cyclic, env);
            assert!(!errors.is_empty(), "a self-rooted read is not valid SSA");
            counts[7] += 1;
        }
        if counts[8] == 0 {
            let mut read_only = vec![false; body.len()];
            let mut element = false;
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::Call {
                    continue;
                }
                let Some(callee) = hd_tir::ir::Callee::from_words(body.record(body.data[i][0]))
                else {
                    continue;
                };
                if env.may_write(&callee) {
                    continue;
                }
                read_only[i] = true;
                element |= body.call_args(i).iter().any(|&arg| {
                    let Some(arg) = hd_tir::ir::Ref(arg).as_inst() else {
                        return false;
                    };
                    let j = arg.0 as usize;
                    body.tags[j] == Tag::Ref
                        && body.data[j][1] & hd_tir::ir::ref_word::MUT == 0
                        && body
                            .view(body.data[j][0])
                            .steps(body)
                            .any(|s| matches!(s, hd_tir::ir::Proj::Elem(_)))
                });
            }
            if element {
                // A sparse proof of already read-only consumers must agree
                // with resolving the same call dependencies normally.
                let effects = hd_tir::own::WriteEffects::new(body);
                let full = effects.equation(env).evaluate(|c| env.call_writes(c));
                let sparse = effects
                    .equation_in(env, &[], &read_only)
                    .evaluate(|c| env.call_writes(c));
                assert_eq!(sparse, full, "read-only element consumers");
                counts[8] += 1;
            }
        }
        if counts[9] == 0 {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::IsUnique {
                    continue;
                }
                let Some(local) = body.view(body.data[i][0]).local() else {
                    continue;
                };
                let mut invalid = body.clone();
                let topo = Topology::new(&invalid);
                let block = topo.blk[i];
                let pos = invalid
                    .record(invalid.data[block as usize][0])
                    .iter()
                    .position(|&v| v == u32_of(i))
                    .map(u32_of)
                    .expect("uniqueness check in block");
                let n = u32_of(invalid.len());
                let coerce = u32_of(invalid.extra.len());
                invalid
                    .extra
                    .extend_from_slice(&[2, Coercion::Identity as u32, NONE]);
                let target = u32_of(invalid.extra.len());
                invalid
                    .extra
                    .extend_from_slice(&[2, place::ROOT_VALUE, n + 1]);
                let ty = invalid.local_ty[local as usize];
                let syn = invalid.syn[i];
                let edits = [
                    BatchInsert {
                        block,
                        pos,
                        tag: Tag::Read,
                        data: [place::BARE_LOCAL | local, ReadMode::Borrow as u32],
                        ty,
                        syn,
                    },
                    BatchInsert {
                        block,
                        pos,
                        tag: Tag::Coerce,
                        data: [n, coerce],
                        ty,
                        syn,
                    },
                    BatchInsert {
                        block,
                        pos,
                        tag: Tag::IsUnique,
                        data: [target, invalid.data[i][1]],
                        ty: hd_types::Ty::VOID,
                        syn,
                    },
                ];
                insert_batch(&mut invalid, &topo, &edits).expect("coerced uniqueness target");
                let errors = hd_tir::ir::verify_owned(&invalid, env);
                assert!(
                    errors
                        .iter()
                        .any(|e| e.what.contains("leaves its buffer in no place")),
                    "a coerced read cannot be the writable owner: {errors:?}"
                );
                counts[9] += 1;
                break;
            }
        }
        if counts[10] == 0 {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::LoanEnd {
                    continue;
                }
                let place = body.view(body.data[i][0]);
                let Root::Value(call) = place.root else {
                    continue;
                };
                if !place.whole() || body.tags[call as usize] != Tag::Call {
                    continue;
                }
                let Some(callee) = Callee::from_words(body.record(body.data[call as usize][0]))
                else {
                    continue;
                };
                if env.returned_loan(&callee).is_none() {
                    continue;
                }
                let mut invalid = body.clone();
                let topo = Topology::new(&invalid);
                let block = topo.blk[i];
                let pos = body
                    .record(body.data[block as usize][0])
                    .iter()
                    .position(|&v| v == u32_of(i))
                    .map(u32_of)
                    .expect("loan end in block");
                let duplicate = BatchInsert {
                    block,
                    pos,
                    tag,
                    data: body.data[i],
                    ty: body.ty[i],
                    syn: body.syn[i],
                };
                insert_batch(&mut invalid, &topo, &[duplicate]).expect("duplicate loan end");
                let errors = hd_tir::ir::verify_owned(&invalid, env);
                assert!(!errors.is_empty(), "one borrowed result cannot end twice");
                counts[10] += 1;
                break;
            }
        }
        if counts[11] == 0 && hd_tir::own::writes_in(body, 0..body.len(), &|c| env.may_write(c)) {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::Ref
                    || body.data[i][1] & (ref_word::LOCAL | ref_word::FRAME | ref_word::MUT)
                        != ref_word::LOCAL | ref_word::FRAME
                {
                    continue;
                }
                let Some(local) = body.view(body.data[i][0]).local() else {
                    continue;
                };
                if body.local_flags[local as usize] & local_flags::PARAM == 0
                    || !env.counted(body.local_ty[local as usize])
                    || hd_tir::ir::Topology::new(body).sub[i] != 0
                    || !body.storage.iter().any(|&(sub, l, storage)| {
                        sub.raw() == 0 && l.raw() == local && storage == Storage::Slot
                    })
                    || !body
                        .exit_drops
                        .iter()
                        .any(|&(sub, l)| sub.raw() == 0 && l.raw() == local)
                {
                    continue;
                }
                let pure = (0..body.len()).any(|call| {
                    body.tags[call] == Tag::Call
                        && body.call_args(call).contains(&(u32_of(i)))
                        && Callee::from_words(body.record(body.data[call][0]))
                            .is_some_and(|callee| !env.may_write(&callee))
                });
                if !pure {
                    continue;
                }
                let mut lent = body.clone();
                lent.exit_drops
                    .retain(|&(sub, l)| sub.raw() != 0 || l.raw() != local);
                for j in 0..lent.len() {
                    if lent.tags[j] == Tag::Drop {
                        let place = lent.view(lent.data[j][0]);
                        if place.whole() && place.root == Root::Local(local) {
                            lent.tags[j] = Tag::LoanEnd;
                        }
                    }
                }
                // Borrowing the descriptor preserves its pure frame-local
                // address even when this function writes through another
                // parameter. Its old ABI packets must still be rejected.
                let errors = hd_tir::ir::verify_owned(&lent, env);
                assert!(
                    errors
                        .iter()
                        .any(|e| e.what == "a boundary count without its count fact"),
                    "stale owned ABI packets: {errors:?}"
                );
                assert!(
                    !errors.iter().any(|e| e.what.contains("a reference to")),
                    "{errors:?}"
                );
                counts[11] += 1;
                break;
            }
        }
        if counts[12] == 0 {
            let topo = Topology::new(body);
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::Call {
                    continue;
                }
                let Some(callee) = Callee::from_words(body.record(body.data[i][0])) else {
                    continue;
                };
                let Some(k) = env.returned_loan(&callee) else {
                    continue;
                };
                let Some(&arg) = body.call_args(i).get(k as usize) else {
                    continue;
                };
                let Some(arg_ref) = hd_tir::ir::Ref(arg).as_inst() else {
                    continue;
                };
                let r = arg_ref.0 as usize;
                if body.tags[r] != Tag::Read || body.data[r][1] != ReadMode::Move as u32 {
                    continue;
                }
                let source = body.view(body.data[r][0]);
                let Some(local) = source.local().filter(|_| source.whole()) else {
                    continue;
                };
                if body.local_flags[local as usize] & local_flags::PARAM != 0 {
                    continue;
                }
                let block = topo.blk[i];
                if block == hd_tir::ir::NONE {
                    continue;
                }
                let Some(next) = body
                    .record(body.data[block as usize][0])
                    .get(topo.pos[i] as usize + 1..)
                    .and_then(|members| {
                        members
                            .iter()
                            .copied()
                            .find(|&v| !count::boundary(body, v as usize))
                    })
                else {
                    continue;
                };
                if body.tags[next as usize] != Tag::LoanEnd {
                    continue;
                }
                let end = body.view(body.data[next as usize][0]);
                if !end.whole() || end.root != Root::Value(arg) {
                    continue;
                }
                let mut handoff = body.clone();
                handoff.data[r][1] = ReadMode::Borrow as u32;
                handoff.data[next as usize][0] = place::BARE_LOCAL | local;
                let errors = hd_tir::ir::verify_owned(&handoff, env);
                assert!(errors.is_empty(), "holder-count handoff: {errors:?}");
                let mut wrong_holder = handoff.clone();
                let other = u32_of(wrong_holder.local_ty.len());
                wrong_holder.local_ty.push(body.local_ty[local as usize]);
                wrong_holder.local_name.push(hd_base::Symbol::NONE);
                wrong_holder.local_syn.push(body.syn[r]);
                wrong_holder.local_flags.push(0);
                wrong_holder.data[next as usize][0] = place::BARE_LOCAL | other;
                assert!(
                    !hd_tir::ir::verify_owned(&wrong_holder, env).is_empty(),
                    "an unrelated empty holder cannot donate this count"
                );
                if let Some(stored) = (0..body.len()).find(|&j| {
                    body.tags[j] == Tag::Store
                        && body.view(body.data[j][0]).local() == Some(local)
                        && body.view(body.data[j][0]).whole()
                }) {
                    // A stale SSA alias after transfer into the holder
                    // must not acquire another count from that old value.
                    let mut alias = handoff.clone();
                    let root = u32_of(alias.extra.len());
                    alias
                        .extra
                        .extend_from_slice(&[2, place::ROOT_VALUE, body.data[stored][1]]);
                    alias.data[r][0] = root;
                    alias.data[next as usize][0] = root;
                    let errors = hd_tir::ir::verify_owned(&alias, env);
                    assert!(
                        !errors.is_empty(),
                        "a moved SSA value cannot be borrowed again"
                    );
                } else {
                    panic!("the named donor must have its initializing store");
                }
                counts[12] += 1;
                break;
            }
        }
        if counts[13] == 0 {
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::Store || !body.view(body.data[i][0]).whole() {
                    continue;
                }
                let call = body.data[i][1] as usize;
                if call >= body.len() || body.tags[call] != Tag::Call {
                    continue;
                }
                let Some(callee) = Callee::from_words(body.record(body.data[call][0])) else {
                    continue;
                };
                let Some(k) = env.returned_loan(&callee) else {
                    continue;
                };
                let Some(&arg) = body.call_args(call).get(k as usize) else {
                    continue;
                };
                let Some(arg) = hd_tir::ir::Ref(arg).as_inst() else {
                    continue;
                };
                let r = arg.0 as usize;
                if body.tags[r] != Tag::Read || !body.is_borrow(arg.0) {
                    continue;
                }
                let source = body.view(body.data[r][0]);
                let Root::Local(local) = source.root else {
                    continue;
                };
                if !source.whole()
                    || body.local_flags[local as usize] & local_flags::PARAM == 0
                    || body
                        .exit_drops
                        .iter()
                        .any(|&(sub, l)| sub.raw() == 0 && l.raw() == local)
                {
                    continue;
                }
                let mut invalid = body.clone();
                let topo = Topology::new(body);
                let block = topo.blk[i];
                let root = u32_of(invalid.extra.len());
                invalid
                    .extra
                    .extend_from_slice(&[2, place::ROOT_VALUE, u32_of(call)]);
                let pos = topo.pos[i] + 1;
                let end = BatchInsert {
                    block,
                    pos,
                    tag: Tag::LoanEnd,
                    data: [root, NONE],
                    ty: hd_types::Ty::VOID,
                    syn: body.syn[i],
                };
                insert_batch(&mut invalid, &topo, &[end]).expect("end of stored borrowed result");
                let errors = hd_tir::ir::verify_owned(&invalid, env);
                assert!(
                    !errors.is_empty(),
                    "a stored alias cannot end the same borrowed result again"
                );
                counts[13] += 1;
                break;
            }
        }
        if counts[14] == 0 {
            let topo = Topology::new(body);
            for (i, &tag) in body.tags.iter().enumerate() {
                if tag != Tag::IsUnique || body.data[i][1] != unique_word::DEFERRED_INIT {
                    continue;
                }
                let place = body.view(body.data[i][0]);
                let hd_tir::ir::Root::Local(local) = place.root else {
                    continue;
                };
                if !place.whole() || body.cap_local.iter().any(|l| l.raw() == local) {
                    continue;
                }
                let Some(loop_) = (i + 1..body.len())
                    .find(|&j| body.tags[j] == Tag::Loop && topo.blk[j] == topo.blk[i])
                else {
                    continue;
                };
                let Some(push) = (0..loop_).find(|&j| {
                    topo.loop_of[j] == u32_of(loop_)
                        && body.tags[j] == Tag::Intrinsic
                        && IntrinsicOp::from_u32(body.data[j][0]) == Some(IntrinsicOp::ListPush)
                }) else {
                    continue;
                };
                let mut invalid = body.clone();
                invalid.data[i][1] = unique_word::DEFERRED_CACHE;
                invalid.local_flags[local as usize] &= !(hd_tir::ir::local_flags::CAPTURED
                    | hd_tir::ir::local_flags::CAPTURED_ASSIGNED);
                // The cached-header verifier must not decode an arbitrary
                // SSA operand as a place read, even on malformed input.
                invalid.extra[body.data[push][1] as usize + 1] = u32_of(push);
                let errors = hd_tir::ir::verify_owned(&invalid, env);
                // This ownership-only query assumes typed, structural
                // validity. A non-read operand supplies no place identity
                // and must not be decoded as a clobber of this local.
                assert!(
                    !errors
                        .iter()
                        .any(|e| e.what.contains("replaces its deferred cached header")),
                    "a non-read operand is not a decoded cached place: {errors:?}"
                );
                counts[14] += 1;
                break;
            }
        }
        if counts[15] == 0 {
            for i in 0..body.len() {
                if body.tags[i] != Tag::Read {
                    continue;
                }
                let Root::Local(local) = body.view(body.data[i][0]).root else {
                    continue;
                };
                let ty = body.local_ty[local as usize];
                if env.holds_drop(ty) == hd_tir::own::Tri::No
                    || env.own_drop(ty) != hd_tir::own::Tri::No
                {
                    continue;
                }
                let mut paths = body.clone();
                let global = u32_of(paths.extra.len());
                paths
                    .extra
                    .extend_from_slice(&[3, place::ROOT_GLOBAL, body.item.raw(), ty.0]);
                let dereference = u32_of(paths.extra.len());
                paths
                    .extra
                    .extend_from_slice(&[3, place::ROOT_LOCAL, local, place::PROJ_DEREF]);
                for word in [global, dereference, place::BARE_LOCAL | local] {
                    assert!(!hd_tir::own::part_hook_guard(&paths, env, word));
                }
                counts[15] += 1;
                break;
            }
        }
        CORRUPTED.set(Some(counts));
    }
}

#[test]
fn corrupt_ownership_metadata_from_a_real_program_is_rejected() {
    let source = include_str!("../../tests/cases/counts/local-reference-alias-adaptations.hd");
    let store = MemoryStore::default();
    build_at(
        &store,
        source,
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    let warm = MemoryStore::default();
    for (kind, key) in store.drain_new() {
        if kind == EntryKind::Owned {
            warm.put(
                kind,
                &key,
                &store.get(kind, &key).expect("stored owned body"),
            );
        }
    }
    VERIFIED.set(Some(0));
    CORRUPTED.set(Some([0; 16]));
    CHECKED_VARIANTS.set(Some([0; 3]));
    build_at(
        &warm,
        source,
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    build_at(
        &warm,
        include_str!("../../tests/cases/counts/returned-call-owner-handoffs.hd"),
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    build_at(
        &warm,
        include_str!("../../tests/cases/own/conditional-match-uniqueness.hd"),
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    build_at(
        &warm,
        include_str!("../../tests/cases/own/deferred-uniqueness-sequences.hd"),
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    assert_eq!(CHECKED_VARIANTS.take(), Some([1; 3]));
    VERIFIED.take().expect("active verification");
    let corrupted = CORRUPTED.take().expect("active corruption checks");
    assert_eq!(corrupted, [1; 16]);
}
