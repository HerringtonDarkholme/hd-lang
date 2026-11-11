//! An ABI-owned argument needs a copy when the caller keeps its value.
//! State that copy separately from the logical argument pass: a summary
//! may lend the value even when the selected ABI receives it owned.
//! The instance's parameter type and lends mask condition the lowering.
//! An owned cold parameter also needs a copy when the caller's value
//! has another use, such as evaluation of a default argument.

use super::OwnCx;
use crate::ir::{
    Body, Callee, Ref, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx, topo: &Topology) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    let mut consumers = None::<Vec<u32>>;
    for (i, &tag) in b.tags.iter().enumerate() {
        if !matches!(tag, Tag::Call | Tag::Await) {
            continue;
        }
        let Some(callee) = Callee::from_words(b.record(b.data[i][0])) else {
            continue;
        };
        let record = b.record(b.data[i][1]);
        let args = &record[..record.len().saturating_sub(3)];
        for (k, &arg) in args.iter().enumerate() {
            let Some(value) = Ref(arg).as_inst() else {
                continue;
            };
            if !cx.counted(b.ty[value.0 as usize]) {
                continue;
            }
            let cold = cx.cold_argument_transfers(&callee, k, args.len());
            if !(cold || tag == Tag::Call && cx.call_argument_may_own(&callee, k, args.len())) {
                continue;
            }
            let consumers = consumers.get_or_insert_with(|| {
                let mut rows = Vec::new();
                super::consumers(b, topo, cx, &mut rows);
                rows
            });
            if consumers[value.0 as usize] != u32_of(i) {
                out.push((
                    Tag::Retain,
                    u32_of(i),
                    Info::new(At::Operation, Part::Operand(u32_of(k)), 0),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Providers, ReadMode, TirBuilder, TirSink, Topology, count, local_flags,
        verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    struct Facts;
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == Ty(43)
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent // The logical summary does not decide the ABI copy.
        }
        fn call_argument_may_own(&self, c: &Callee, k: usize, n: usize) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7)) && n == 3 && k < 2
        }
        fn call_has_requirement_context(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7))
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    }

    fn body(mode: ReadMode) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let mut args = Vec::new();
        for ty in [Ty::STRING, Ty(43), Ty::I32] {
            let local = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
            args.push(b.get(local, ty, NodeIdx::NONE));
        }
        for def in [7, 8] {
            b.call(
                &Callee::Item {
                    def: DefId::from_raw(def),
                    targs: TyList::EMPTY,
                },
                &args,
                Providers::None,
                Ty::VOID,
                NodeIdx::NONE,
            );
        }
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for i in 0..b.len() {
            if b.tags[i] == Tag::Read {
                b.data[i][1] = mode as u32;
            }
        }
        b
    }

    fn state(b: &mut Body) {
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn caller_owned_counted_arguments_to_possibly_owned_parameters_state_copies() {
        for mode in [ReadMode::Borrow, ReadMode::Copy, ReadMode::Move] {
            let b = body(mode);
            let copies = counts(&b, &Facts, &Topology::new(&b));
            assert_eq!(copies.len(), 2);
            for (k, (tag, target, info)) in copies.iter().enumerate() {
                assert_eq!(*tag, Tag::Retain);
                assert_eq!(b.tags[*target as usize], Tag::Call);
                assert_eq!(*info, Info::new(At::Operation, Part::Operand(u32_of(k)), 0));
            }
        }
    }

    #[test]
    fn argument_and_row_packets_have_independent_readers_and_no_consumer() {
        let mut b = body(ReadMode::Borrow);
        b.anchors = vec![37; b.len()];
        for _ in 0..2 {
            state(&mut b);
            let target = u32_of(b.tags.iter().position(|&t| t == Tag::Call).unwrap());
            let topo = Topology::new(&b);
            for part in [Part::Operand(0), Part::Operand(1), Part::Row] {
                assert_eq!(
                    count::operation(&b, &topo, target, Tag::Retain, part),
                    Some(Info::new(At::Operation, part, 0))
                );
            }
            assert_eq!(
                count::operation(&b, &topo, target, Tag::Release, Part::Row),
                Some(Info::new(At::Operation, Part::Row, 2))
            );
            for i in 0..b.len() {
                if Info::read(&b, i).is_some_and(|c| c.at == At::Operation) {
                    assert_eq!(b.anchors[i], b.anchors[b.data[i][0] as usize]);
                }
            }
            let mut consumers = Vec::new();
            crate::own::consumers(&b, &topo, &Facts, &mut consumers);
            assert!(consumers.iter().all(|&c| c == crate::ir::NONE));
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn a_copy_requires_its_action_phase_argument_reason_target_and_position() {
        let mut b = body(ReadMode::Borrow);
        state(&mut b);
        let i = (0..b.len())
            .find(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Operand(0)))
            .unwrap();
        for info in [
            Info::new(At::Exit, Part::Operand(0), 0),
            Info::new(At::Operation, Part::Operand(2), 0),
            Info::new(At::Operation, Part::Operand(0), 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[i] = Tag::Release;
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        let mut wrong = b.clone();
        wrong.data[i][0] = u32_of(b.tags.iter().rposition(|&t| t == Tag::Call).unwrap());
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        for mode in 0..3 {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let target = wrong.data[i][0] as usize;
            let mut edits = splice::Tables::default();
            if mode != 1 {
                edits.remove(u32_of(i));
            }
            if mode != 0 {
                edits.insert(
                    topo.blk[target],
                    if mode == 2 { 0 } else { topo.pos[target] + 1 },
                    wrong.tags[i],
                    wrong.data[i],
                    Ty::VOID,
                    wrong.syn[i],
                );
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &Facts).is_empty(), "mode {mode}");
        }
    }
}
