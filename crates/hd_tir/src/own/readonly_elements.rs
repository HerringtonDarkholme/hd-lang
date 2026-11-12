//! A local immutable element address used by one read-only call needs no
//! copy-on-write buffer split when nothing can change between it and the
//! call. This proof does not remove pointer access checks or call loans.

use super::{OwnCx, Role, Tri, derive, operands};
use crate::ir::{Body, Callee, NONE, Proj, ReadMode, Root, Tag, ref_word};

/// Immutable def-use census, shared across effect-summary queries.
pub(super) struct Calls {
    counts: Vec<u32>,
    last: Vec<u32>,
}

impl Calls {
    pub(super) fn new(b: &Body) -> Self {
        let mut counts = vec![0; b.len()];
        let mut last = vec![NONE; b.len()];
        let mut ops = Vec::new();
        for i in 0..b.len() {
            #[cfg(test)]
            super::effect_places::work(1);
            operands(b, i, &mut ops);
            for op in &ops {
                #[cfg(test)]
                super::effect_places::work(1);
                if op.role != Role::Dropped {
                    counts[op.value as usize] += 1;
                    last[op.value as usize] = crate::ir::u32_of(i);
                }
            }
        }
        Self { counts, last }
    }

    /// O(instructions + operands + block members), independently of the
    /// number of addresses or coercion depth. Call policy is deliberately
    /// queried afresh: recursive-component summaries can change.
    pub(super) fn elements(&self, b: &Body, cx: &dyn OwnCx) -> Vec<bool> {
        self.lending(b, cx)
            .into_iter()
            .map(|call| {
                #[cfg(test)]
                super::effect_places::work(1);
                call != NONE
                    && Callee::from_words(b.record(b.data[call as usize][0]))
                        .is_some_and(|callee| !cx.may_write(&callee))
            })
            .collect()
    }

    /// Structural loans with their policy-dependent consumer call. The effect
    /// equations retain this dependency instead of freezing an SCC's current
    /// optimistic call policy into the structural index.
    pub(super) fn lending(&self, b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
        let calls = derive::local_lending_calls(b, cx, &self.counts, &self.last);
        let mut out = vec![NONE; b.len()];
        for block in 0..b.len() {
            #[cfg(test)]
            super::effect_places::work(1);
            if b.tags[block] != Tag::Block {
                continue;
            }
            let mut endpoint = NONE;
            for &word in b.record(b.data[block][0]).iter().rev() {
                #[cfg(test)]
                super::effect_places::work(1);
                let i = word as usize;
                match b.tags[i] {
                    Tag::Call => {
                        endpoint = if Callee::from_words(b.record(b.data[i][0])).is_some() {
                            word
                        } else {
                            NONE
                        };
                    }
                    Tag::Ref
                        if endpoint != NONE
                            && calls[i] == endpoint
                            && b.data[i][1] & ref_word::MUT == 0 =>
                    {
                        out[i] = endpoint;
                    }
                    // A read, scalar operation or same-value coercion cannot
                    // change the buffer. Pointer reads may move their target.
                    Tag::Read
                        if !cx.counted(b.ty[i])
                            && cx.holds_drop(b.ty[i]) == Tri::No
                            && b.data[i][1] != ReadMode::Move as u32
                            && !matches!(b.view(b.data[i][0]).root, Root::Ptr(_))
                            && !b
                                .view(b.data[i][0])
                                .steps(b)
                                .any(|step| matches!(step, Proj::Deref)) => {}
                    Tag::Prim => {}
                    Tag::Coerce if b.same_value_coercion(i) => {}
                    _ => endpoint = NONE,
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Coercion, Place, PlaceRoot, Proj, Providers, TirBuilder, TirSink, local_flags,
    };
    use crate::own::{Pass, WriteEffects};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};
    use std::cell::Cell;

    const LOCAL_REF: Ty = Ty(1_000_004);
    struct Cx(Cell<bool>);
    impl OwnCx for Cx {
        fn temporary_owner_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
        }
        fn holds_drop(&self, ty: Ty) -> Tri {
            if ty == Ty::I32 { Tri::No } else { Tri::Maybe }
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, c: &Callee) -> bool {
            self.0.get() || matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(9))
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
        fn holds_ref(&self, ty: Ty) -> bool {
            ty == LOCAL_REF
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
    }
    fn call(b: &mut TirBuilder, def: u32, arg: &[crate::ir::Ref], ty: Ty) {
        b.call(
            &Callee::Item {
                def: DefId::from_raw(def),
                targs: TyList::EMPTY,
            },
            arg,
            Providers::None,
            ty,
            NodeIdx::from_raw(1),
        );
    }
    fn body(width: usize, depth: usize, control: usize) -> Result<(Body, Vec<usize>), String> {
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let block = b.open_block();
        let local = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
        let index = b.const_value(Ty::I32, 0);
        let mut refs = Vec::new();
        for _ in 0..width {
            let place = b.place(&Place {
                root: PlaceRoot::Local(local),
                proj: vec![Proj::Elem(index)],
            });
            let mut r = b.emit(
                Tag::Ref,
                place,
                ref_word::LOCAL | if control == 1 { ref_word::MUT } else { 0 },
                LOCAL_REF,
                n,
            );
            refs.push(r.0 as usize);
            for _ in 0..depth {
                r = b.coerce(Coercion::Identity, NONE, r, LOCAL_REF, n);
            }
            if control == 4 {
                let changed = b.const_str("changed");
                b.set(local, changed, n);
            }
            if control == 7 {
                b.get(local, Ty::STRING, n);
            }
            if control == 6 {
                call(&mut b, 8, &[], Ty::VOID);
            }
            call(
                &mut b,
                if control == 5 { 9 } else { 8 },
                &[r],
                if control == 3 { LOCAL_REF } else { Ty::VOID },
            );
            if control == 2 {
                call(&mut b, 8, &[r], Ty::VOID);
            }
        }
        let root = b.close_block(block, None, Ty::VOID, n);
        Ok((b.finish(root, &[]).map_err(|e| format!("{e:?}"))?, refs))
    }
    #[test]
    fn only_one_readonly_nonretaining_call_without_intervening_effects_is_admitted()
    -> Result<(), String> {
        for control in 0..8 {
            let (body, refs) = body(1, 3, control)?;
            let cx = Cx(Cell::new(false));
            let elements = Calls::new(&body).elements(&body, &cx);
            assert_eq!(elements[refs[0]], control == 0, "control {control}");
        }
        Ok(())
    }
    #[test]
    fn integrated_effect_queries_count_linear_work_across_width_and_coercion_depth()
    -> Result<(), String> {
        for (width, depth) in [(1, 1), (1000, 1), (1, 20), (1000, 20)] {
            let (body, _) = body(width, depth, 0)?;
            let cx = Cx(Cell::new(false));
            super::super::effect_places::take_work();
            let effects = WriteEffects::new(&body);
            assert!(!effects.writes(&cx));
            let work = super::super::effect_places::take_work();
            assert!(
                work <= 16 * body.len() + body.local_ty.len(),
                "{width}x{depth}: {work}"
            );
            cx.0.set(true);
            assert!(
                effects.writes(&cx),
                "cached index must not freeze callee effects"
            );
        }
        Ok(())
    }
}
