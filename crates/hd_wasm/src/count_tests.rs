//! The test before a count call. Most retains and releases at run time find
//! no count to change: a null, static data, a frame slot. A call that only
//! finds that out costs more than the test, so each count call first asks
//! what its call would answer:
//!
//! - a known payload start ([`Helper::RetainKnown`], [`Helper::ReleaseKnown`]):
//!   the address is not null and its header's count is not 0. A release also
//!   calls while the collector traces, where a gray object's count may have
//!   reached 0 and `visit` must still see the reference. An acyclic module
//!   has no collector;
//! - any other address: it is not null. Finding the block needs the page
//!   table, so a frame slot or static data still reaches the call.
//!
//! The runtime's own helpers keep their plain calls. A test pays only where it
//! skips, and a helper walks the fields of an object it holds, whose addresses
//! are real counts: `binary_trees` skips none of its 30M releases, and wrapping
//! the helpers made it 10% slower with nothing to skip. So the test goes where
//! a value's origin is unknown, in compiled bodies.
//!
//! A checked build runs the same test and tells its runtime when one skips a
//! call, so the skipped work stays on the count table (`hd.skipped`).
//!
//! A test changes the size of the body it stands in, and on V8 that alone can
//! move another body's speed by several percent (#1434). `progs/list-sort`
//! runs 15 count operations, and its `sort`, called once, runs its whole loop
//! in Liftoff code, since V8 has no on-stack replacement for Wasm. The test in
//! `main` makes `main`'s Liftoff code 56 bytes longer, which moves where
//! `sort`'s byte-identical code lands. The result's sign follows the layout,
//! not the test: with the test, 384 against 403 ms at the default tier, 388
//! against 355 with `--liftoff-only`, 454 against 456 with `--no-liftoff`.

use crate::module::{Func, Global, HEAP, Ins, Sym, Val, mem};
use crate::op::Op as I;
use crate::rt::Helper;
use hd_base::Hash128;
use wasm_encoder::BlockType;

/// What the link knows about a module's count calls.
#[derive(Clone, Copy)]
pub(crate) struct Counts {
    /// The runtime's `retain`, called directly.
    pub retain: Hash128,
    /// No object takes part in cycle collection (§23.28).
    pub acyclic: bool,
    /// `inline_counts` has already lowered the known forms.
    pub known_inlined: bool,
    /// A checked build's `skipped_test`.
    pub skipped: Option<Hash128>,
}

#[derive(Clone, Copy)]
enum Kind {
    KnownRetain,
    KnownRelease,
    Other,
}

fn kind(f: &Func, ins: &Ins, c: Counts) -> Option<Kind> {
    let Ins::Call(id) = ins else {
        return None;
    };
    match &*f.symbol(*id) {
        Sym::Helper(Helper::RetainKnown) if !c.known_inlined => Some(Kind::KnownRetain),
        Sym::Helper(Helper::ReleaseKnown) if !c.known_inlined => Some(Kind::KnownRelease),
        Sym::Helper(Helper::ReleaseRef) => Some(Kind::Other),
        Sym::Inst(i) if *i == c.retain => Some(Kind::Other),
        _ => None,
    }
}

/// Puts each count call of `f` behind its test. Gives whether any changed.
pub(crate) fn lower(f: &mut Func, c: Counts) -> bool {
    lower_with_work(f, c, &mut || {})
}

// At most two visits per input instruction; guards add a fixed number of
// instructions. State is one producer local, independent of control depth.
fn lower_with_work(f: &mut Func, c: Counts, visit: &mut impl FnMut()) -> bool {
    if !f.body.iter().any(|i| {
        visit();
        kind(f, i, c).is_some()
    }) {
        return false;
    }
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    let close = |f: &mut Func| {
        if let Some(skipped) = c.skipped {
            f.op(I::Else);
            f.call(Sym::Inst(skipped));
        }
        f.op(I::End);
    };
    let mut operand_local = None;
    for (position, ins) in body.into_iter().enumerate() {
        visit();
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        let Some(form) = kind(f, &ins, c) else {
            match &ins {
                Ins::Op(I::LocalGet(local) | I::LocalTee(local)) => operand_local = Some(*local),
                // These markers emit no instruction and cannot change a local
                // or the top stack value. Keep their source/dependency records.
                Ins::Anchor(_) | Ins::DrainGlue(_) => {}
                _ => operand_local = None,
            }
            f.push(ins);
            continue;
        };
        // A Wasm local cannot be changed by the calls inside the guard.
        // A computed stack value instead gets its own single-definition
        // temporary, which forwarding/coalescing can reuse independently.
        let t = operand_local.take().unwrap_or_else(|| {
            let local = f.local(Val::I32);
            f.op(I::LocalTee(local));
            local
        });
        f.op(I::If(BlockType::Empty));
        let known = !matches!(form, Kind::Other);
        if known {
            f.get(t);
            f.i32(8);
            f.op(I::I32Sub);
            f.op(I::I32Load(mem(HEAP, 0, 2)));
            if matches!(form, Kind::KnownRelease) && !c.acyclic {
                f.global_get(Global::Tracing);
                f.op(I::I32Or);
            }
            f.op(I::If(BlockType::Empty));
        }
        f.get(t);
        f.push(ins);
        if known {
            close(f);
        }
        close(f);
    }
    f.origin_at = outer;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;

    fn config() -> Counts {
        Counts {
            retain: Hash128(7),
            acyclic: false,
            known_inlined: false,
            skipped: Some(Hash128(8)),
        }
    }

    fn function() -> Func {
        Module::default().func("count operands", vec![Val::I32], vec![])
    }

    fn guards(f: &Func) -> usize {
        f.body
            .iter()
            .filter(|i| matches!(i, Ins::Op(I::If(_))))
            .count()
    }

    #[test]
    fn existing_local_producers_and_zero_width_markers_need_no_temporary() {
        for tee in [false, true] {
            let mut f = function();
            let source = if tee { f.local(Val::I32) } else { 0 };
            f.get(0);
            if tee {
                f.op(I::LocalTee(source));
            }
            f.push(Ins::Anchor(17));
            let dependency = f.intern(Sym::Helper(Helper::ReleaseRef));
            f.push(Ins::DrainGlue(dependency));
            f.call(Sym::Helper(Helper::ReleaseKnown));
            f.origins = Some(
                (0..f.body.len())
                    .map(|i| Some(u32::try_from(i).unwrap()))
                    .collect(),
            );
            f.origin_at = Some(99);
            let original_locals = f.locals.len();
            let call_origin = f.origins.as_ref().unwrap().last().copied().unwrap();
            assert!(lower(&mut f, config()));
            assert_eq!(f.locals.len(), original_locals);
            assert_eq!(guards(&f), 2);
            assert_eq!(
                f.body
                    .iter()
                    .filter(|i| matches!(i, Ins::Op(I::LocalTee(_))))
                    .count(),
                usize::from(tee)
            );
            assert!(f.body.iter().any(|i| matches!(i, Ins::Anchor(17))));
            assert!(
                f.body
                    .iter()
                    .any(|i| matches!(i, Ins::DrainGlue(s) if *s == dependency))
            );
            assert!(
                f.body
                    .iter()
                    .filter_map(|i| if let Ins::Op(I::LocalGet(l)) = i {
                        Some(l)
                    } else {
                        None
                    })
                    .all(|l| *l == 0 || *l == source)
            );
            assert_eq!(f.origin_at, Some(99));
            let origins = f.origins.as_ref().unwrap();
            assert_eq!(origins.len(), f.body.len());
            let first_guard = f
                .body
                .iter()
                .position(|i| matches!(i, Ins::Op(I::If(_))))
                .unwrap();
            assert!(origins[first_guard..].iter().all(|o| *o == call_origin));
        }
    }

    #[test]
    fn computed_operands_have_independent_single_definition_temporaries() {
        let mut f = function();
        for offset in [8, 16] {
            f.get(0);
            f.i32(offset);
            f.op(I::I32Add);
            f.call(Sym::Helper(Helper::ReleaseRef));
        }
        assert!(lower(&mut f, config()));
        let temps: Vec<_> = f
            .body
            .iter()
            .filter_map(|i| match i {
                Ins::Op(I::LocalTee(l)) => Some(*l),
                _ => None,
            })
            .collect();
        assert_eq!(temps, [1, 2]);
        assert_eq!(f.locals, [Val::I32, Val::I32]);
    }

    #[test]
    fn modes_keep_known_inlining_tracing_and_skipped_count_behavior() {
        for acyclic in [false, true] {
            for known_inlined in [false, true] {
                for checked in [false, true] {
                    let mut f = function();
                    for symbol in [
                        Sym::Helper(Helper::RetainKnown),
                        Sym::Helper(Helper::ReleaseKnown),
                        Sym::Helper(Helper::ReleaseRef),
                        Sym::Inst(Hash128(7)),
                    ] {
                        f.get(0);
                        f.call(symbol);
                    }
                    let c = Counts {
                        acyclic,
                        known_inlined,
                        skipped: checked.then_some(Hash128(8)),
                        ..config()
                    };
                    assert!(lower(&mut f, c));
                    assert!(f.locals.is_empty());
                    let known = usize::from(!known_inlined);
                    assert_eq!(guards(&f), 2 + 4 * known);
                    assert_eq!(
                        f.body
                            .iter()
                            .filter(|i| matches!(i, Ins::GlobalGet(_)))
                            .count(),
                        usize::from(!acyclic && !known_inlined)
                    );
                    assert_eq!(
                        f.body
                            .iter()
                            .filter(|i| matches!(i, Ins::Op(I::Else)))
                            .count(),
                        usize::from(checked) * (2 + 4 * known)
                    );
                }
            }
        }
        let mut f = function();
        f.get(0);
        f.call(Sym::Inst(Hash128(99)));
        assert!(!lower(&mut f, config()));
        assert!(f.locals.is_empty());
    }

    #[test]
    fn generated_width_and_depth_have_linear_visits_and_fixed_guard_expansion() {
        for width in [1000, 10000] {
            for computed in [false, true] {
                let mut f = function();
                for _ in 0..20 {
                    f.op(I::Block(BlockType::Empty));
                }
                for _ in 0..width {
                    f.get(0);
                    if computed {
                        f.i32(8);
                        f.op(I::I32Add);
                    }
                    f.call(Sym::Helper(Helper::ReleaseKnown));
                }
                for _ in 0..20 {
                    f.op(I::End);
                }
                let input = f.body.len();
                let mut visits = 0;
                assert!(lower_with_work(&mut f, config(), &mut || visits += 1));
                assert!(visits <= 2 * input, "{width}/{computed}: {visits}/{input}");
                assert!(f.body.len() <= input + 18 * width);
                assert_eq!(f.locals.len(), if computed { width } else { 0 });
                assert_eq!(guards(&f), 2 * width);
            }
        }
    }
}
