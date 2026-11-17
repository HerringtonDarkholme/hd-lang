//! Invariant 24 (`docs/future-work/INSTANCE_TIR.md`, "Verifier"): an
//! instance body holds no generic form. The driver marks each body it
//! builds for an instance [`Phase::Instance`] and runs this check before
//! the ownership passes, as Swift's SIL verifier rejects an archetype left
//! in a function `GenericSpecializer` produced.
//!
//! Rejected:
//! - a type that names a type parameter or an unnormalized projection, in
//!   the instruction, local or constant columns or in a record's type
//!   words ([`record_type_words`]);
//! - a generic local function's sub-body (`sub_flags::GENERIC`):
//!   `hd_mono::instantiate_locals` makes one plain sub-body per
//!   instantiation;
//! - a trait call still chosen through a bound (`ChoiceKind::Bound`) at a
//!   receiver that is not a trait value: `hd_mono::instantiate` chooses
//!   each one at a concrete receiver as an impl or builtin call
//!   (`crate::inline::rechoose`). A trait value's call dispatches through
//!   its table, and a receiver naming a parameter is the type check's
//!   (reported above).
//!
//! Cost: one pass over the columns, the calls and the records. A type's
//! flags answer "names a parameter" and "names a projection" in O(1).

use hd_types::{InternPool, Ty, TyData, TyList};

use super::{
    Body, Callee, ChoiceKind, Phase, Tag, TypeWord, VerifyError, record_type_words, sub_flags,
    u32_of,
};

/// Whether `t`, or what reference `t` refers to, is a trait value: its
/// trait calls dispatch through the value's table.
fn is_trait_value(pool: &InternPool, t: Ty) -> bool {
    let t = match pool.get(t) {
        TyData::Ref { inner, .. } => inner,
        _ => t,
    };
    matches!(pool.get(t), TyData::TraitValue { .. })
}

/// Verifies instance body `b` (invariant 24, [the module](self)).
#[must_use]
pub fn verify_instance(
    b: &Body,
    pool: &InternPool,
    name: &dyn Fn(Ty) -> String,
) -> Vec<VerifyError> {
    let mut errs = Vec::new();
    let mut err = |inst: usize, what: String| {
        errs.push(VerifyError {
            invariant: 24,
            inst: u32_of(inst),
            what,
        });
    };
    if b.phase != Phase::Instance {
        err(0, format!("a {:?} body checked as an instance", b.phase));
    }
    if let Some(sub) = b.sub_flags.iter().position(|f| f & sub_flags::GENERIC != 0) {
        let root = b.sub_root[sub] as usize;
        err(root, format!("sub-body {sub} is a generic local function"));
    }
    let open = Open { pool, name };
    for (i, &t) in b.ty.iter().enumerate() {
        if let Some(what) = open.why(t) {
            err(i, format!("its type {what}"));
        }
    }
    for (l, &t) in b.local_ty.iter().enumerate() {
        if let Some(what) = open.why(t) {
            err(0, format!("local {l}'s type {what}"));
        }
    }
    for (k, &(t, _)) in b.consts.iter().enumerate() {
        if let Some(what) = open.why(t) {
            err(0, format!("constant {k}'s type {what}"));
        }
    }
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Call) {
        if let Some(Callee::TraitMethod {
            self_ty,
            choice: (ChoiceKind::Bound, _),
            ..
        }) = Callee::from_words(b.record(b.data[i][0]))
            && !pool.has_param(self_ty)
            && !is_trait_value(pool, self_ty)
        {
            err(
                i,
                format!(
                    "a trait call chosen through a bound at `{}`",
                    (open.name)(self_ty)
                ),
            );
        }
    }
    let mut words = Vec::new();
    record_type_words(b, &mut |at, word| words.push((at, word)));
    for (at, word) in words {
        let w = b.extra[at];
        let what = match word {
            TypeWord::Ty => open.why(Ty(w)),
            TypeWord::List => pool.list_items(TyList(w)).iter().find_map(|&t| open.why(t)),
        };
        if let Some(what) = what {
            err(0, format!("record word {at}: a type {what}"));
        }
    }
    errs
}

/// Whether a type is still generic.
struct Open<'p> {
    pool: &'p InternPool,
    name: &'p dyn Fn(Ty) -> String,
}

impl Open<'_> {
    /// Why `t` is not an instance's type, or `None`.
    fn why(&self, t: Ty) -> Option<String> {
        let types = self.pool.types();
        if types.has_assoc(t) {
            return Some(format!("`{}` names a projection", (self.name)(t)));
        }
        if types.has_param(t) {
            return Some(format!("`{}` names a type parameter", (self.name)(t)));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::verify_instance;
    use crate::ir::{
        Body, BodyKind, Callee, ChoiceKind, Phase, Providers, Tag, TirBuilder, TirSink as _,
        local_flags, sub_flags,
    };
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{InternPool, ParamRef, Ty, TyData};

    const F: DefId = DefId::from_raw(1);

    fn param(pool: &InternPool, index: u16) -> Ty {
        pool.intern_ty(&TyData::Param(ParamRef { owner: F, index }))
    }

    /// `fn f(x: t) -> t: g::[t](x)`, marked an instance.
    fn body(pool: &InternPool, t: Ty) -> Body {
        let n = NodeIdx::from_raw(0);
        let mut b = TirBuilder::new(F, BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(t, Symbol::from_raw(0), local_flags::PARAM, n);
        let get = b.get(x, t, n);
        let callee = Callee::Item {
            def: DefId::from_raw(2),
            targs: pool.list(&[t]),
        };
        let call = b.call(&callee, &[get], Providers::None, t, n);
        b.emit(Tag::Return, call.0, crate::ir::NONE, Ty::NEVER, n);
        let root = b.close_block(blk, None, Ty::NEVER, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        body.phase = Phase::Instance;
        body
    }

    fn found(b: &Body, pool: &InternPool) -> Vec<String> {
        verify_instance(b, pool, &|t| format!("t{}", t.0))
            .into_iter()
            .map(|e| {
                assert_eq!(e.invariant, 24);
                e.what
            })
            .collect()
    }

    /// `fn f(x: t): x.m()`, a trait call chosen `choice` at `t`.
    fn trait_call(pool: &InternPool, t: Ty, choice: (ChoiceKind, u32)) -> Body {
        let n = NodeIdx::from_raw(0);
        let mut b = TirBuilder::new(F, BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(t, Symbol::from_raw(0), local_flags::PARAM, n);
        let get = b.get(x, t, n);
        let callee = Callee::TraitMethod {
            trait_: DefId::from_raw(5),
            method: DefId::from_raw(6),
            self_ty: t,
            targs: pool.list(&[]),
            choice,
        };
        b.call(&callee, &[get], Providers::None, Ty::VOID, n);
        let root = b.close_block(blk, None, Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        body.phase = Phase::Instance;
        body
    }

    /// A bound's call left at a concrete receiver is rejected; one chosen
    /// as an impl or builtin passes, as does a bound's call at a trait
    /// value or a reference to one, which dispatches through its table.
    #[test]
    fn a_bound_call_passes_only_at_a_trait_value() {
        let pool = InternPool::new();
        let bound = (ChoiceKind::Bound, 0);
        assert_eq!(
            found(&trait_call(&pool, Ty::I32, bound), &pool),
            [format!(
                "a trait call chosen through a bound at `t{}`",
                Ty::I32.0
            )]
        );
        for choice in [(ChoiceKind::Impl, 9), (ChoiceKind::Builtin, 1)] {
            assert!(found(&trait_call(&pool, Ty::I32, choice), &pool).is_empty());
        }
        let dyn_ = pool.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(5),
            args: pool.list(&[]),
            bindings: Vec::new(),
        });
        let by_ref = pool.intern_ty(&TyData::Ref {
            inner: dyn_,
            mutable: false,
            counted: false,
        });
        for t in [dyn_, by_ref] {
            assert!(found(&trait_call(&pool, t, bound), &pool).is_empty());
        }
        // A receiver naming a parameter is the type check's finding only.
        let what = found(&trait_call(&pool, param(&pool, 0), bound), &pool);
        assert!(
            what.iter().all(|w| !w.starts_with("a trait call")),
            "{what:?}"
        );
    }

    #[test]
    fn a_concrete_instance_body_passes() {
        let pool = InternPool::new();
        assert!(found(&body(&pool, Ty::I32), &pool).is_empty());
    }

    #[test]
    fn a_checked_body_is_not_an_instance() {
        let pool = InternPool::new();
        let mut b = body(&pool, Ty::I32);
        b.phase = Phase::Checked;
        assert_eq!(found(&b, &pool), ["a Checked body checked as an instance"]);
    }

    /// The parameter is reported in each column it reaches: the
    /// instruction and local columns and the callee's type arguments.
    #[test]
    fn a_type_parameter_is_rejected_in_every_column() {
        let pool = InternPool::new();
        let t = param(&pool, 0);
        let mut b = body(&pool, t);
        b.consts.push((t, 0));
        let what = found(&b, &pool);
        let name = format!("`t{}` names a type parameter", t.0);
        assert!(
            what.iter().any(|w| w == &format!("its type {name}")),
            "{what:?}"
        );
        assert!(
            what.iter().any(|w| w == &format!("local 0's type {name}")),
            "{what:?}"
        );
        assert!(
            what.iter()
                .any(|w| w == &format!("constant 0's type {name}")),
            "{what:?}"
        );
        assert!(
            what.iter()
                .any(|w| w.starts_with("record word") && w.ends_with(&name)),
            "{what:?}"
        );
    }

    #[test]
    fn an_unnormalized_projection_is_rejected() {
        let pool = InternPool::new();
        let t = pool.intern_ty(&TyData::Assoc {
            assoc: DefId::from_raw(5),
            trait_: DefId::from_raw(6),
            self_ty: Ty::I32,
            args: pool.list(&[]),
        });
        let what = found(&body(&pool, t), &pool);
        assert!(
            what.contains(&format!("its type `t{}` names a projection", t.0)),
            "{what:?}"
        );
    }

    /// A generic local function's sub-body is rejected, and its own
    /// parameters with it, as any other parameter.
    #[test]
    fn a_generic_local_function_is_rejected() {
        let pool = InternPool::new();
        let own = param(&pool, 1);
        let mut b = body(&pool, own);
        b.sub_flags[0] |= sub_flags::GENERIC;
        let what = found(&b, &pool);
        assert_eq!(what[0], "sub-body 0 is a generic local function");
        assert!(
            what.contains(&format!("its type `t{}` names a type parameter", own.0)),
            "{what:?}"
        );
    }
}
