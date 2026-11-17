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
//! - a generic local function's sub-body (`sub_flags::GENERIC`), unless
//!   the caller passes [`LocalGenerics`]: until the driver instantiates
//!   them (gap: #2104), their own parameters, numbered after the item's
//!   (`hd_check/src/local.rs`), are allowed and no other.
//!
//! Not yet checked (gap: #2103): a trait call left chosen through a bound.
//! Today `hd_tir::inline::rechoose` writes only impl choices, so a call a
//! builtin impl answers, and one at a trait-value receiver, stay bound
//! calls the emitter chooses; `hd_mono::instantiate` is to choose them.
//!
//! Cost: one pass over the columns and the records. A type's flags answer
//! "names a parameter" and "names a projection" in O(1); only a body with
//! a generic local function walks a parameter-holding type's parts, once
//! per distinct type (a memo), so the check is linear in the body's size.

use hd_base::{DefId, HashMap};
use hd_types::{InternPool, Ty, TyData, TyList};

use super::{Body, Phase, TypeWord, VerifyError, record_type_words, sub_flags, u32_of};

/// The parameters of a body's generic local functions: those of `owner`
/// (the item's generics owner) numbered from `first`, past the owner's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalGenerics {
    pub owner: DefId,
    pub first: u16,
}

/// Verifies instance body `b` (invariant 24, [the module](self)).
#[must_use]
pub fn verify_instance(
    b: &Body,
    pool: &InternPool,
    local: Option<LocalGenerics>,
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
    let generic_sub = b.sub_flags.iter().position(|f| f & sub_flags::GENERIC != 0);
    let has_local = generic_sub.is_some();
    if let Some(sub) = generic_sub
        && local.is_none()
    {
        let root = b.sub_root[sub] as usize;
        err(root, format!("sub-body {sub} is a generic local function"));
    }
    let mut open = Open {
        pool,
        name,
        local: local.filter(|_| has_local),
        memo: HashMap::default(),
    };
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

/// Whether a type is still generic, memoized per distinct type.
struct Open<'p> {
    pool: &'p InternPool,
    name: &'p dyn Fn(Ty) -> String,
    local: Option<LocalGenerics>,
    memo: HashMap<Ty, bool>,
}

impl Open<'_> {
    /// Why `t` is not an instance's type, or `None`.
    fn why(&mut self, t: Ty) -> Option<String> {
        let types = self.pool.types();
        if types.has_assoc(t) {
            return Some(format!("`{}` names a projection", (self.name)(t)));
        }
        if types.has_param(t) && !self.local_only(t) {
            return Some(format!("`{}` names a type parameter", (self.name)(t)));
        }
        None
    }

    /// Whether every parameter `t` names is a generic local function's:
    /// its parameter-holding parts in post-order on an explicit stack,
    /// each distinct type once (the memo). A row's parameters (`$R`) are
    /// numbered as type parameters are (`hd_check/src/local.rs`), so a
    /// row a part holds answers by the same rule.
    fn local_only(&mut self, t: Ty) -> bool {
        let Some(local) = self.local else {
            return false;
        };
        let types = self.pool.types();
        let mut stack = vec![(t, false)];
        let mut parts = Vec::new();
        while let Some((u, expanded)) = stack.pop() {
            if self.memo.contains_key(&u) {
                continue;
            }
            parts.clear();
            types.children(u, &mut parts);
            parts.retain(|&p| types.has_param(p));
            let ok = match self.pool.get(u) {
                TyData::Param(p) => p.owner == local.owner && p.index >= local.first,
                _ if !expanded => {
                    stack.push((u, true));
                    stack.extend(parts.iter().map(|&p| (p, false)));
                    continue;
                }
                data => {
                    let row = match data {
                        TyData::Fn { row, .. } | TyData::Row(row) | TyData::Context(row) => {
                            self.pool.row_data(row).params
                        }
                        _ => Vec::new(),
                    };
                    let rows_ok = row
                        .iter()
                        .all(|p| p.owner == local.owner && p.index >= local.first);
                    (!parts.is_empty() || !row.is_empty())
                        && rows_ok
                        && parts.iter().all(|p| self.memo.get(p) == Some(&true))
                }
            };
            self.memo.insert(u, ok);
        }
        self.memo.get(&t) == Some(&true)
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalGenerics, verify_instance};
    use crate::ir::{
        Body, BodyKind, Callee, Phase, Providers, Tag, TirBuilder, TirSink as _, local_flags,
        sub_flags,
    };
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{InternPool, ParamRef, RowData, RowParamRef, Ty, TyData};

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

    fn found(b: &Body, pool: &InternPool, local: Option<LocalGenerics>) -> Vec<String> {
        verify_instance(b, pool, local, &|t| format!("t{}", t.0))
            .into_iter()
            .map(|e| {
                assert_eq!(e.invariant, 24);
                e.what
            })
            .collect()
    }

    #[test]
    fn a_concrete_instance_body_passes() {
        let pool = InternPool::new();
        assert!(found(&body(&pool, Ty::I32), &pool, None).is_empty());
    }

    #[test]
    fn a_checked_body_is_not_an_instance() {
        let pool = InternPool::new();
        let mut b = body(&pool, Ty::I32);
        b.phase = Phase::Checked;
        assert_eq!(
            found(&b, &pool, None),
            ["a Checked body checked as an instance"]
        );
    }

    /// The parameter is reported in each column it reaches: the
    /// instruction and local columns and the callee's type arguments.
    #[test]
    fn a_type_parameter_is_rejected_in_every_column() {
        let pool = InternPool::new();
        let t = param(&pool, 0);
        let mut b = body(&pool, t);
        b.consts.push((t, 0));
        let what = found(&b, &pool, None);
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
        let what = found(&body(&pool, t), &pool, None);
        assert!(
            what.contains(&format!("its type `t{}` names a projection", t.0)),
            "{what:?}"
        );
    }

    /// Until #2104, a generic local function's own parameters, numbered
    /// from `first`, are allowed where the body has one; the item's own,
    /// below `first`, never are.
    #[test]
    fn only_a_generic_local_functions_parameters_are_allowed() {
        let pool = InternPool::new();
        let local = Some(LocalGenerics { owner: F, first: 1 });
        let own = param(&pool, 1);
        let pair = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[own, Ty::I32, own]),
            rest: None,
        });
        let mut b = body(&pool, pair);
        b.sub_flags[0] |= sub_flags::GENERIC;
        assert!(found(&b, &pool, local).is_empty());
        assert_eq!(
            found(&b, &pool, None)[0],
            "sub-body 0 is a generic local function"
        );
        let outer = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[own, param(&pool, 0)]),
            rest: None,
        });
        let mut b = body(&pool, outer);
        b.sub_flags[0] |= sub_flags::GENERIC;
        let what = found(&b, &pool, local);
        assert!(
            what.contains(&format!("its type `t{}` names a type parameter", outer.0)),
            "{what:?}"
        );
        // A row parameter (`$R`) answers by the same numbering.
        let row = |index| {
            pool.intern_ty(&TyData::Row(pool.row(&RowData {
                keys: vec![],
                params: vec![RowParamRef { owner: F, index }],
            })))
        };
        for (index, passes) in [(1, true), (0, false)] {
            let mut b = body(&pool, row(index));
            b.sub_flags[0] |= sub_flags::GENERIC;
            assert_eq!(
                found(&b, &pool, local).is_empty(),
                passes,
                "row index {index}"
            );
        }
        // No generic local function: no parameter is allowed.
        let b = body(&pool, pair);
        assert!(!found(&b, &pool, local).is_empty());
    }
}
