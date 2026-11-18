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
/// body is its instance's, with no copy; its generic local functions
/// are instantiated too ([`instantiate_locals`]).
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
    // Its generic local functions are instantiated at the now concrete
    // arguments, so their bound calls are chosen with the rest.
    // A malformed body is left as it is, and the instance verifier
    // rejects its generic local function (`hd_tir::ir::verify_instance`).
    let _ = instantiate_locals(pool, env, item, &mut body);
    rechoose(&mut body, pick);
    body
}

/// Instantiates `body`'s generic local functions in place
/// (`hd_tir::ir::instantiate_local_functions`): one sub-body per distinct
/// `(function, type arguments)` in its context, each type substituted
/// through [`subst`] once per distinct type per context. Returns the
/// number of sub-bodies made, 0 when it has none.
///
/// # Errors
/// A malformed body, whose closure names a word outside its own context.
pub fn instantiate_locals(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    item: DefId,
    body: &mut Body,
) -> Result<usize, String> {
    let owner = env.generics_owner(item);
    let mut types = Locals {
        pool,
        env,
        owner,
        memo: HashMap::default(),
    };
    let Some((out, made)) = hd_tir::ir::instantiate_local_functions(body, pool, &mut types)? else {
        return Ok(0);
    };
    *body = out;
    Ok(made)
}

/// [`subst`] for a context of generic local functions: their own
/// parameters, which follow the item's (`hd_check/src/local.rs`), bound;
/// the item's own left as they are.
struct Locals<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    owner: DefId,
    memo: HashMap<(u32, Ty), Ty>,
}

impl hd_tir::ir::LocalTypes for Locals<'_> {
    fn ty(&mut self, ctx: u32, bind: &[(u32, Ty)], t: Ty) -> Ty {
        if !self.pool.has_param(t) {
            return t;
        }
        if let Some(&u) = self.memo.get(&(ctx, t)) {
            return u;
        }
        let owner = self.owner;
        let s = self.pool.subst(t, &|p: hd_types::ParamRef| {
            if p.owner != owner {
                return None;
            }
            bind.iter().find(|b| b.0 == u32::from(p.index)).map(|b| b.1)
        });
        // In a generic item's own body (dev folds it once) the item's
        // parameters stay; its instances close the type.
        let u = if self.pool.has_param(s) {
            s
        } else {
            crate::close(self.pool, self.env, s)
        };
        self.memo.insert((ctx, t), u);
        u
    }
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
/// (`docs/future-work/INSTANCE_TIR.md`, "Passes and emission"). Returns
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
