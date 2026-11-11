//! A call's temporary context owns its row's provider data.
//! The copy packet and the temporary context's last use are separate
//! statements. An empty concrete row or a forwarded open context lowers
//! neither; a cold suspension constructor keeps its own row count.
//! A final scoped provider may supply an entry's existing owner. Its
//! conditional Release/Operand permission leaves the ordinary Drop intact
//! unless lowering selects that exact provider for that exact final call.

use super::OwnCx;
use crate::ir::{
    Body, Callee, NONE, Ref, Root, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};
use hd_base::DefId;
use hd_types::TyList;

pub(super) fn counts(b: &Body, cx: &dyn OwnCx, topo: &Topology) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    let mut row_calls = vec![false; b.len()];
    for (i, &tag) in b.tags.iter().enumerate() {
        let needed = match tag {
            Tag::CallValue => {
                let callee = Ref(b.data[i][0]);
                let ty = match callee.as_inst() {
                    Some(v) => b.ty[v.0 as usize],
                    None => b.consts[(callee.0 & !Ref::CONST_BIT) as usize].0,
                };
                cx.function_has_requirement_row(ty)
            }
            Tag::Call | Tag::Await => Callee::from_words(b.record(b.data[i][0]))
                .is_some_and(|c| cx.call_has_requirement_context(&c)),
            Tag::DefaultCall => match b.record(b.data[i][0]) {
                &[def, args, ..] => cx.call_has_requirement_context(&Callee::Item {
                    def: DefId::from_raw(def),
                    targs: TyList(args),
                }),
                _ => false,
            },
            _ => false,
        };
        row_calls[i] = needed;
        if !needed {
            continue;
        }
        for (tag, reason) in [(Tag::Retain, 0), (Tag::Release, 2)] {
            out.push((tag, u32_of(i), Info::new(At::Operation, Part::Row, reason)));
        }
    }
    let candidates = transfers(b, topo);
    let mut cold = vec![false; b.sub_root.len()];
    if !candidates.is_empty() {
        for sub in super::cold::sub_bodies(b, cx) {
            cold[sub as usize] = true;
        }
    }
    for (scope, operand, call) in candidates {
        // Suspended scopes retain their ordinary owners for unwind ladders.
        if cold[topo.sub[scope as usize] as usize] {
            continue;
        }
        if !row_calls[call as usize] {
            continue;
        }
        let pairs = b.record(b.data[scope as usize][0]);
        if !cx.trait_value(hd_types::Ty(pairs[2 * operand as usize])) {
            continue;
        }
        out.push((
            Tag::Release,
            scope,
            Info::new(At::Operation, Part::Operand(operand), 2),
        ));
    }
    out
}

/// Final scoped owners that a tail function-value context may take.
/// One operand census and a bottom-up tail summary are linear in body size.
/// No path joins, retry or input-depth native recursion are needed.
pub(super) fn transfers(b: &Body, topo: &Topology) -> Vec<(u32, u32, u32)> {
    transfer_facts(b, topo, || {})
}

fn transfer_facts(b: &Body, topo: &Topology, mut visit: impl FnMut()) -> Vec<(u32, u32, u32)> {
    if !b.tags.contains(&Tag::With) {
        return Vec::new();
    }
    let mut preceding = vec![NONE; b.len()];
    for (i, tag) in b.tags.iter().enumerate() {
        visit();
        if *tag != Tag::Block {
            continue;
        }
        let mut last = NONE;
        for &member in b.record(b.data[i][0]) {
            visit();
            preceding[member as usize] = last;
            if !matches!(b.tags[member as usize], Tag::Drop | Tag::LoanEnd)
                && !crate::ir::count::boundary(b, member as usize)
            {
                last = member;
            }
        }
    }
    let mut uses = vec![0usize; b.len()];
    let mut drops = vec![NONE; b.len()];
    let mut ops = Vec::new();
    let mut tails = vec![NONE; b.len()];
    for i in 0..b.len() {
        visit();
        if b.tags[i] == Tag::Drop {
            let place = b.view(b.data[i][0]);
            if place.whole()
                && let Root::Value(value) = place.root
            {
                let slot = &mut drops[value as usize];
                // Multiple cleanup paths cannot share a conditional transfer.
                if *slot == NONE {
                    *slot = u32_of(i);
                } else {
                    uses[value as usize] += 2;
                }
            } else {
                super::operands(b, i, &mut ops);
                for op in &ops {
                    visit();
                    uses[op.value as usize] += 1;
                }
            }
        } else {
            super::operands(b, i, &mut ops);
            for op in &ops {
                visit();
                uses[op.value as usize] += 1;
            }
        }
        tails[i] = match b.tags[i] {
            Tag::CallValue => u32_of(i),
            Tag::Block => {
                let tail = b.data[i][1];
                if let Some(tail) = Ref(tail).as_inst() {
                    let tail = tail.0 as usize;
                    let last = b
                        .record(b.data[i][0])
                        .iter()
                        .rev()
                        .inspect(|_| visit())
                        .find(|&&v| !crate::ir::count::boundary(b, v as usize));
                    if last == Some(&u32_of(tail)) {
                        tails[tail]
                    } else {
                        NONE
                    }
                } else {
                    NONE
                }
            }
            Tag::Scope if b.record(b.data[i][1]).is_empty() => tails[b.data[i][0] as usize],
            Tag::With => tails[b.data[i][1] as usize],
            _ => NONE,
        };
    }
    let mut out = Vec::new();
    for (i, &tail) in tails.iter().enumerate() {
        visit();
        if b.tags[i] != Tag::With || tail == NONE {
            continue;
        }
        for (operand, pair) in b.record(b.data[i][0]).chunks_exact(2).enumerate() {
            visit();
            let Some(value) = Ref(pair[1]).as_inst() else {
                continue;
            };
            let value = value.0 as usize;
            let drop = drops[value];
            if uses[value] != 1
                || drop == NONE
                || topo.blk[drop as usize] != topo.blk[i]
                || topo.pos[drop as usize] <= topo.pos[i]
            {
                continue;
            }
            if preceding[drop as usize] == u32_of(i) {
                out.push((u32_of(i), u32_of(operand), tail));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, ChoiceKind, Providers, ReadMode, TirBuilder, TirSink, Topology, count,
        local_flags, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts {
        cold: bool,
    }
    const FACTS: Facts = Facts { cold: false };
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn suspending(&self, _: DefId) -> bool {
            self.cold
        }
        fn trait_value(&self, ty: Ty) -> bool {
            ty == Ty(43)
        }
        fn function_has_requirement_row(&self, ty: Ty) -> bool {
            ty == Ty(43) || ty == Ty(45)
        }
        fn call_has_requirement_context(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7))
                || matches!(c, Callee::TraitMethod { method, .. } if *method == DefId::from_raw(7))
        }
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty)
        }
        fn pass(&self, c: &Callee, k: usize, n: usize) -> Pass {
            Cx.pass(c, k, n)
        }
        fn pass_value(&self, ty: Ty, k: usize, n: usize) -> Pass {
            Cx.pass_value(ty, k, n)
        }
        fn may_write(&self, c: &Callee) -> bool {
            Cx.may_write(c)
        }
        fn parts(&self, ty: Ty, v: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(ty, v, out)
        }
        fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
            Cx.received(item, k, n)
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    fn body(ty: Ty, constant: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let callee = if constant {
            b.const_value(ty, 0)
        } else {
            let l = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
            b.get(l, ty, NodeIdx::NONE)
        };
        let args = b.refs_record(&[]);
        b.emit(Tag::CallValue, callee.0, args, Ty::I32, NodeIdx::NONE);
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for (i, &tag) in b.tags.iter().enumerate() {
            if tag == Tag::Read {
                b.data[i][1] = ReadMode::Borrow as u32;
            }
        }
        b
    }

    fn state(b: &mut Body) {
        statements::run(b, &FACTS, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &FACTS);
        assert!(errors.is_empty(), "{errors:?}");
    }

    fn scoped(width: usize, depth: usize, shared: bool, later: bool, cleanup: bool) -> Body {
        let n = NodeIdx::NONE;
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        let mut values = Vec::new();
        for _ in 0..if shared { 1 } else { width } {
            let empty = b.words_record(&[]);
            values.push(b.emit(Tag::NewStruct, NONE, empty, Ty(43), n));
        }
        let pairs = b.words_record(
            &(0..width)
                .flat_map(|k| [Ty(43).0, values[if shared { 0 } else { k }].0])
                .collect::<Vec<_>>(),
        );
        let inner = b.open_block();
        let mut layers = Vec::new();
        for _ in 0..depth {
            layers.push(b.open_block());
        }
        let callback = b.const_value(Ty(45), 0);
        let args = b.words_record(&[]);
        let mut tail = b.emit(Tag::CallValue, callback.0, args, Ty::I32, n);
        while let Some(layer) = layers.pop() {
            let block = b.close_block(layer, Some(tail), Ty::I32, n);
            let suites = if cleanup {
                let suite = b.open_block();
                let suite = b.close_block(suite, None, Ty::VOID, n);
                b.words_record(&[suite.0])
            } else {
                b.words_record(&[])
            };
            tail = b.emit(Tag::Scope, block.0, suites, Ty::I32, n);
        }
        let block = b.close_block(inner, Some(tail), Ty::I32, n);
        let scope = b.emit(Tag::With, pairs, block.0, Ty::I32, n);
        if later {
            let args = b.words_record(&[]);
            b.emit(Tag::CallValue, callback.0, args, Ty::I32, n);
        }
        for value in values {
            let place = b.place(&crate::ir::Place {
                root: crate::ir::PlaceRoot::Value(value),
                proj: Vec::new(),
            });
            b.emit(Tag::Drop, place, NONE, Ty::VOID, n);
        }
        let root = b.close_block(outer, Some(scope), Ty::I32, n);
        b.finish_tolerating(root, &[], &[22]).unwrap()
    }

    #[test]
    fn suspended_scopes_keep_their_owner_for_unwind_and_cancellation() {
        let b = scoped(1, 0, false, false, false);
        let packets = counts(&b, &Facts { cold: true }, &Topology::new(&b));
        assert!(
            packets
                .iter()
                .all(|&(_, target, _)| b.tags[target as usize] != Tag::With)
        );
    }

    #[test]
    fn scoped_transfer_packet_is_recomputed_and_rejects_forged_permission() {
        let mut b = scoped(1, 0, false, false, false);
        let initial_topo = Topology::new(&b);
        statements::run(
            &mut b,
            &FACTS,
            &initial_topo,
            &mut splice::Tables::default(),
        )
        .unwrap();
        let topo = Topology::new(&b);
        let mut errors = Vec::new();
        statements::verify(&b, &FACTS, &topo, |i, error| errors.push((i, error)));
        assert_eq!(errors, []);
        let packet = b
            .tags
            .iter()
            .enumerate()
            .find_map(|(i, &tag)| {
                (tag == Tag::Release && b.tags[b.data[i][0] as usize] == Tag::With).then_some(i)
            })
            .expect("scope transfer permission");
        let mut info = Info::read(&b, packet).unwrap();
        info.reason = 0;
        b.data[packet][1] = info.write(&mut b);
        statements::verify(&b, &FACTS, &Topology::new(&b), |i, error| {
            errors.push((i, error));
        });
        assert!(!errors.is_empty());
    }

    #[test]
    fn scoped_owner_proof_counts_linear_work_at_width_1000_and_depth_20() {
        for (width, depth) in [(1, 0), (1000, 0), (1, 20), (1000, 20)] {
            let b = scoped(width, depth, false, false, false);
            let mut work = 0;
            let facts = transfer_facts(&b, &Topology::new(&b), || work += 1);
            assert_eq!(facts.len(), width);
            assert!(
                work <= 12 * b.len(),
                "{width} x {depth}: {work} visits for {} rows",
                b.len()
            );
        }
    }

    #[test]
    fn shared_provider_values_later_work_and_cleanup_keep_their_old_owner() {
        for b in [
            scoped(2, 0, true, false, false),
            scoped(1, 0, false, true, false),
            scoped(1, 1, false, false, true),
        ] {
            assert_eq!(transfers(&b, &Topology::new(&b)), []);
        }
    }

    #[test]
    fn a_direct_default_or_awaited_call_uses_its_selected_signature_fact() {
        for tag in [Tag::Call, Tag::Await, Tag::DefaultCall] {
            for needed in [false, true] {
                for dynamic in [false, true] {
                    if dynamic && tag == Tag::DefaultCall {
                        continue;
                    }
                    let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
                    let block = builder.open_block();
                    let def = DefId::from_raw(if needed { 7 } else { 8 });
                    if tag == Tag::DefaultCall {
                        let call = builder.words_record(&[def.raw(), TyList::EMPTY.0]);
                        let args = builder.refs_record(&[]);
                        builder.emit(tag, call, args, Ty::I32, NodeIdx::NONE);
                    } else {
                        let callee = if dynamic {
                            Callee::TraitMethod {
                                trait_: DefId::from_raw(9),
                                method: def,
                                self_ty: Ty(46),
                                targs: TyList::EMPTY,
                                choice: (ChoiceKind::TraitValue, 0),
                            }
                        } else {
                            Callee::Item {
                                def,
                                targs: TyList::EMPTY,
                            }
                        };
                        builder.call(&callee, &[], Providers::None, Ty::I32, NodeIdx::NONE);
                    }
                    let root = builder.close_block(block, None, Ty::VOID, NodeIdx::NONE);
                    let mut b = builder.finish(root, &[]).unwrap();
                    b.tags[0] = tag;
                    b.anchors = vec![41; b.len()];
                    for _ in 0..2 {
                        state(&mut b);
                        let ds: Vec<_> = (0..b.len())
                            .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Row))
                            .collect();
                        assert_eq!(ds.len(), usize::from(needed) * 2);
                        for i in ds {
                            let target = b.data[i][0];
                            assert_eq!(b.tags[target as usize], tag);
                            assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
                            assert_eq!(b.anchors[i], b.anchors[target as usize]);
                            assert_eq!(
                                count::operation(
                                    &b,
                                    &Topology::new(&b),
                                    target,
                                    b.tags[i],
                                    Part::Row
                                ),
                                Info::read(&b, i),
                            );
                            if tag == Tag::Await {
                                assert!(
                                    count::operation(
                                        &b,
                                        &Topology::new(&b),
                                        target,
                                        Tag::Release,
                                        Part::Child,
                                    )
                                    .is_some()
                                );
                            }
                        }
                    }
                    let text = crate::ir::print(&b);
                    assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
                }
            }
        }
    }

    #[test]
    fn a_call_states_both_row_counts_once_and_an_empty_row_needs_neither() {
        for ty in [Ty(43), Ty(44), Ty(45)] {
            for constant in [false, true] {
                let mut b = body(ty, constant);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let ds: Vec<_> = (0..b.len())
                        .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Row))
                        .collect();
                    assert_eq!(ds.len(), if ty == Ty(44) { 0 } else { 2 });
                    for i in ds {
                        let point = b.data[i][0];
                        assert_eq!(b.tags[point as usize], Tag::CallValue);
                        let reason = if b.tags[i] == Tag::Retain { 0 } else { 2 };
                        assert_eq!(
                            Info::read(&b, i),
                            Some(Info::new(At::Operation, Part::Row, reason))
                        );
                        assert!(count::adjacent(&b, &Topology::new(&b), point, u32_of(i)));
                        assert_eq!(b.anchors[i], b.anchors[point as usize]);
                    }
                }
                let text = crate::ir::print(&b);
                assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
            }
        }
    }

    #[test]
    fn the_call_needs_each_actions_phase_row_reason_and_target() {
        let mut b = body(Ty(43), false);
        state(&mut b);
        for action in [Tag::Retain, Tag::Release] {
            let i = b.tags.iter().position(|&t| t == action).unwrap();
            let reason = if action == Tag::Retain { 0 } else { 2 };
            for info in [
                Info::new(At::Cold, Part::Row, reason),
                Info::new(At::Operation, Part::Captures, reason),
                Info::new(At::Operation, Part::Row, if reason == 0 { 2 } else { 0 }),
            ] {
                let mut wrong = b.clone();
                wrong.data[i][1] = info.write(&mut wrong);
                assert!(!verify_owned(&wrong, &FACTS).is_empty());
            }
            let mut wrong = b.clone();
            wrong.tags[i] = if action == Tag::Retain {
                Tag::Release
            } else {
                Tag::Retain
            };
            assert!(!verify_owned(&wrong, &FACTS).is_empty());
            let mut wrong = b.clone();
            wrong.data[i][0] = wrong.sub_root[0];
            assert!(!verify_owned(&wrong, &FACTS).is_empty());
            for mode in 0..3 {
                let mut wrong = b.clone();
                let topo = Topology::new(&wrong);
                let point = wrong.data[i][0] as usize;
                let mut edits = splice::Tables::default();
                if mode != 1 {
                    edits.remove(u32_of(i));
                }
                if mode != 0 {
                    edits.insert(
                        topo.blk[point],
                        if mode == 1 {
                            topo.pos[i] + 1
                        } else {
                            topo.pos[point]
                        },
                        action,
                        wrong.data[i],
                        Ty::VOID,
                        NodeIdx::NONE,
                    );
                }
                splice::apply(&mut wrong, &topo, &mut edits).unwrap();
                assert!(!verify_owned(&wrong, &FACTS).is_empty());
            }
        }
    }
}
