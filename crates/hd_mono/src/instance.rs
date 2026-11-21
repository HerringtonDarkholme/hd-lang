//! The one builder of a generic instance's body
//! (`docs/future-work/INSTANCE_TIR.md` at 421bcec83, "One builder"): its item's body
//! cloned once, every type it records substituted at the instance's type
//! arguments, and each trait call chosen through a bound chosen again at
//! its now concrete receiver, as an impl or builtin call. Swift's
//! `GenericSpecializer` does the same per substitution map: one clone of
//! the generic SIL function, its types mapped through the map, its
//! witness-method calls devirtualized.
//!
//! Cost: at most one clone of the body, one pass over its type columns and
//! records, and [`subst`] once per distinct type and each distinct type
//! list's items once ([`substitute_body`]); one pass over the calls to
//! rechoose. So an instance costs its body's size plus its distinct
//! types, never the program's.

use std::borrow::Cow;
use std::cell::RefCell;

use hd_base::{DefId, HashMap};
use hd_tir::inline::{Chosen, rechoose};
use hd_tir::ir::Body;
use hd_types::{InternPool, Ty, TyList};

use crate::{ProgramEnv, subst};

/// Instance `(item, args)`'s body from `generic`, its item's body as
/// checked: types
/// substituted ([`subst`]) and bound calls chosen by `pick`, which answers
/// as collection selects. A non-generic callee copy passes empty `args`;
/// a non-generic item's own instance body needs no copy. A borrowed template is
/// cloned once; an already fresh body is consumed without another clone.
#[must_use]
pub fn instantiate(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    item: DefId,
    args: TyList,
    generic: Cow<'_, Body>,
    pick: &dyn Fn(DefId, Ty, TyList) -> Option<Chosen>,
) -> Body {
    // A body with generic local functions instantiates its own part: each
    // function is an instance of its own (`hd_tir::ir::lift`). A malformed
    // body is left as it is, and the owned verifier rejects it.
    let mut body = match hd_tir::ir::item_template(&generic) {
        Ok(Some(own)) => own,
        _ => generic.into_owned(),
    };
    substitute_body(pool, &mut body, &mut |t| subst(pool, env, item, args, t));
    rechoose(&mut body, pick);
    body
}

/// Whether every type an instance body records is settled enough to
/// copy or specialize: no projection left open and no poison (an opaque
/// result `subst` could not open), constants included; and unless `open`,
/// no type parameter or opaque type either. One pass over its columns.
#[must_use]
pub fn settled(pool: &InternPool, body: &Body, open: bool) -> bool {
    let types = || body.ty.iter().chain(&body.local_ty);
    let closed = |t: Ty| open || !(pool.has_param(t) || pool.has_opaque(t));
    types()
        .chain(body.consts.iter().map(|c| &c.0))
        .all(|&t| t != Ty::POISON)
        && types().all(|&t| !pool.has_assoc(t) && closed(t))
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

/// Closes a body that names no type parameter, about to become an
/// instance body (a non-generic item's own body, a generated one, or a
/// body the inliner copied callee code into): every opaque result it
/// records opened and every projection normalized, as [`subst`] closes
/// a generic instance's types, so no pass after it substitutes
/// (`docs/future-work/INSTANCE_TIR.md` at 421bcec83, "Passes and emission"). Returns
/// whether anything was open.
///
/// Cost: one pass over the type columns and record type words, each an
/// O(1) flag test; only a body with an open type is mapped
/// ([`substitute_body`]: one [`subst`] per distinct type).
pub fn close_body(pool: &InternPool, env: &dyn ProgramEnv, body: &mut Body) -> bool {
    let open = |t: Ty| pool.has_opaque(t) || pool.has_assoc(t);
    let mut any = body.ty.iter().chain(&body.local_ty).any(|&t| open(t))
        || body.consts.iter().any(|&(t, _)| open(t));
    if !any {
        let extra = &body.extra;
        hd_tir::ir::record_type_words(body, &mut |at, word| {
            any = any
                || match word {
                    hd_tir::ir::TypeWord::Ty => open(Ty(extra[at])),
                    hd_tir::ir::TypeWord::List => {
                        pool.list_items(TyList(extra[at])).iter().any(|&t| open(t))
                    }
                };
        });
    }
    if any {
        substitute_body(pool, body, &mut |t| crate::close(pool, env, t));
    }
    any
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
