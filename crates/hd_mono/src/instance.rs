//! The one builder of a generic instance's body
//! (`docs/future-work/INSTANCE_TIR.md`, "One builder"): its item's body
//! cloned once, every type it records substituted at the instance's type
//! arguments, and each trait call chosen through a bound chosen again at
//! its now concrete receiver, as an impl or builtin call. Swift's
//! `GenericSpecializer` does the same per substitution map: one clone of
//! the generic SIL function, its types mapped through the map, its
//! witness-method calls devirtualized.
//!
//! Cost: one clone of the body, one pass over its type columns and
//! records, and [`subst`] once per distinct type and each distinct type
//! list's items once ([`substitute_body`]); one pass over the calls to
//! rechoose. So an instance costs its body's size plus its distinct
//! types, never the program's.

use std::cell::RefCell;

use hd_base::{DefId, HashMap};
use hd_tir::inline::{Chosen, rechoose};
use hd_tir::ir::Body;
use hd_types::{InternPool, Ty, TyList};

use crate::{ProgramEnv, subst};

/// Instance `(item, args)`'s body from `generic`, its item's body (as
/// checked, or folded and inlined once per generic body in dev): types
/// substituted ([`subst`]) and bound calls chosen by `pick`, which answers
/// as collection selects. `args` is not empty: a non-generic item's own
/// body is its instance's, with no copy.
#[must_use]
pub fn instantiate(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    item: DefId,
    args: TyList,
    generic: &Body,
    pick: &dyn Fn(DefId, Ty, TyList) -> Option<Chosen>,
) -> Body {
    let mut body = generic.clone();
    substitute_body(pool, &mut body, &mut |t| subst(pool, env, item, args, t));
    rechoose(&mut body, pick);
    body
}

/// Maps every type `body` records through `ty` (`hd_tir::ir::map_body_types`),
/// calling `ty` once per distinct type, and rebuilding each distinct type
/// list once.
pub fn substitute_body(pool: &InternPool, body: &mut Body, ty: &mut dyn FnMut(Ty) -> Ty) {
    let ty = RefCell::new(ty);
    let types: RefCell<HashMap<Ty, Ty>> = RefCell::default();
    let lists: RefCell<HashMap<TyList, TyList>> = RefCell::default();
    let one = |t: Ty| {
        if let Some(&u) = types.borrow().get(&t) {
            return u;
        }
        let u = (ty.borrow_mut())(t);
        types.borrow_mut().insert(t, u);
        u
    };
    let list = |l: TyList| {
        if let Some(&m) = lists.borrow().get(&l) {
            return m;
        }
        let items: Vec<Ty> = pool.list_items(l).iter().map(|&t| one(t)).collect();
        let m = pool.list(&items);
        lists.borrow_mut().insert(l, m);
        m
    };
    hd_tir::ir::map_body_types(body, &one, &list);
}

#[cfg(test)]
mod tests {
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{
        Body, BodyKind, Callee, Providers, Tag, TirBuilder, TirSink as _, local_flags,
    };
    use hd_types::{InternPool, ParamRef, Ty, TyData};

    use super::substitute_body;

    const F: DefId = DefId::from_raw(1);

    /// `fn f(x: T)` calling `g::[T](x)` `n` times: `n` instructions and
    /// `n` callee records of the one type `T`.
    fn calls(pool: &InternPool, t: Ty, n: usize) -> Body {
        let at = NodeIdx::from_raw(0);
        let mut b = TirBuilder::new(F, BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(t, Symbol::from_raw(0), local_flags::PARAM, at);
        let callee = Callee::Item {
            def: DefId::from_raw(2),
            targs: pool.list(&[t]),
        };
        for _ in 0..n {
            let get = b.get(x, t, at);
            b.call(&callee, &[get], Providers::None, t, at);
        }
        let root = b.close_block(blk, None, Ty::VOID, at);
        b.finish(root, &[]).expect("verifies")
    }

    /// Substitution asks for each distinct type once, at n and 10n
    /// instructions of one type: the body's columns and records are
    /// mapped, and the work per instance does not grow with repeats.
    #[test]
    fn each_distinct_type_is_substituted_once() {
        let pool = InternPool::new();
        let t = pool.intern_ty(&TyData::Param(ParamRef { owner: F, index: 0 }));
        for n in [10, 100] {
            let mut body = calls(&pool, t, n);
            let mut asked = Vec::new();
            substitute_body(&pool, &mut body, &mut |u| {
                asked.push(u);
                if u == t { Ty::I32 } else { u }
            });
            let distinct: hd_base::HashSet<Ty> = asked.iter().copied().collect();
            // `T` and the blocks' `void`, each once, however many calls.
            assert_eq!(asked.len(), distinct.len(), "n = {n}: {asked:?}");
            assert!(asked.len() <= 3, "n = {n}: {asked:?}");
            assert!(!body.ty.contains(&t) && !body.local_ty.contains(&t));
            let calls = (0..body.len())
                .filter(|&i| body.tags[i] == Tag::Call)
                .count();
            assert_eq!(calls, n);
            for i in (0..body.len()).filter(|&i| body.tags[i] == Tag::Call) {
                let Some(Callee::Item { targs, .. }) =
                    Callee::from_words(body.record(body.data[i][0]))
                else {
                    panic!("call {i}");
                };
                assert_eq!(pool.list_items(targs), [Ty::I32]);
            }
        }
    }
}
