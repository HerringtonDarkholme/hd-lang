//! Compiler-supplied impls (trait-solver.md §3.9): one function answers
//! each sealed trait the compiler implements from the type's form, as the
//! rows of one `match`. The rows read no impl table, so their answers
//! depend on no impl universe; they read only the declarations a
//! [`Declarations`] gives, which are the same in every context of a run.
//!
//! `Num`, `Integer` and `Float` are not rows here: std writes their one
//! impl per primitive (`lib/std/num.hd`), with the bodies of `zero`, `one`
//! and `from_i64`, so the impl tables answer them like any trait.

use hd_base::{DefId, InferVar};

use crate::pool::{ParamRef, Ty, TyData, Types};
use crate::solver::{BuiltinImpl, FailReason};

/// The sealed traits the rows answer, by `DefId` (`DefId::NONE` for one
/// the run's std does not declare).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealedTraits {
    pub any: DefId,
    pub any_val: DefId,
    pub any_ref: DefId,
    pub inspectable: DefId,
    pub tuple: DefId,
    pub structure: DefId,
}

impl Default for SealedTraits {
    fn default() -> Self {
        SealedTraits {
            any: DefId::NONE,
            any_val: DefId::NONE,
            any_ref: DefId::NONE,
            inspectable: DefId::NONE,
            tuple: DefId::NONE,
            structure: DefId::NONE,
        }
    }
}

impl SealedTraits {
    /// Whether a row of this module answers goals on `trait_`.
    #[must_use]
    pub fn supplies(&self, trait_: DefId) -> bool {
        trait_ != DefId::NONE
            && [
                self.any,
                self.any_val,
                self.any_ref,
                self.inspectable,
                self.tuple,
                self.structure,
            ]
            .contains(&trait_)
    }
}

/// What a nominal type's declaration is, as the rows read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeDecl {
    /// A data type, or a std type with no declaration (`List`, `Map`).
    Data,
    Enum,
    /// A newtype and its base type, over the newtype's own parameters.
    Newtype(Ty),
}

/// The declarations the rows and the trait-value rows (§9.3) read.
pub trait Declarations {
    fn sealed(&self) -> &SealedTraits;
    fn type_decl(&self, def: DefId) -> TypeDecl;
    /// A trait's direct supertraits, as trait value types over the trait's
    /// parameters (`Self` is parameter 0).
    fn supertraits(&self, trait_: DefId) -> &[Ty];
    /// Whether the trait itself declares a member that cannot work through
    /// a `dyn` value, so a `dyn` type does not satisfy a bound on it
    /// (`trait.dyn.member.unavailable`, `trait.dyn.bound.available`).
    fn has_unavailable_member(&self, trait_: DefId) -> bool;
    /// Whether a declaration is local to a block suite, which is never
    /// inspectable (`trait.inspectable.not.local`).
    fn is_local(&self, def: DefId) -> bool;
}

/// A row's answer: it holds when every subgoal does (each on the same
/// trait, one level deeper), or it fails, or it waits for inference.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Row {
    Holds(BuiltinImpl, Vec<Ty>),
    Fails(FailReason),
    Stalled(InferVar),
}

/// The sealed category of a type's form (`types.sealed.exactly-one`).
enum Category {
    Val,
    Ref,
    /// A newtype has its base type's category (`types.sealed.newtype`).
    Base(Ty),
    None,
}

fn category(pool: Types<'_>, decls: &dyn Declarations, t: Ty) -> Category {
    match pool.get(t) {
        TyData::Prim(_) | TyData::Tuple { .. } | TyData::Option(_) | TyData::Fn { .. } => {
            Category::Val
        }
        TyData::TraitValue { .. } => Category::Ref,
        TyData::Adt { def, args } => match decls.type_decl(def) {
            TypeDecl::Enum => Category::Val,
            TypeDecl::Data => Category::Ref,
            TypeDecl::Newtype(base) => {
                let argv = pool.list_items(args);
                Category::Base(pool.subst(base, &|p: ParamRef| {
                    (p.owner == def)
                        .then(|| argv.get(usize::from(p.index)).copied())
                        .flatten()
                }))
            }
        },
        _ => Category::None,
    }
}

/// The subgoal an inspectable type's argument adds (`trait.inspectable.*`):
/// none for a trait value type or `Any`, which count as arguments only
/// (`trait.inspectable.argument-only`), else the argument itself.
fn argument(pool: Types<'_>, t: Ty, out: &mut Vec<Ty>) {
    let inner = match pool.get(t) {
        TyData::Mut(i) => i,
        _ => t,
    };
    if !matches!(pool.get(inner), TyData::TraitValue { .. }) {
        out.push(t);
    }
}

/// The row of `self_ty: trait_` (§3.9), or `None` when no row answers
/// `trait_`. `self_ty` has no outer `mut` (`types.sealed.permission`), is
/// not poison, and an environment clause did not answer it: a parameter
/// or a projection gets nothing more here (`types.sealed.type-parameter`).
pub(crate) fn row(
    pool: Types<'_>,
    decls: &dyn Declarations,
    trait_: DefId,
    self_ty: Ty,
) -> Option<Row> {
    let s = *decls.sealed();
    if !s.supplies(trait_) {
        return None;
    }
    let t = pool.get(self_ty);
    let holds = |b| Some(Row::Holds(b, Vec::new()));
    // `Any`: every value type, `never`, optionals (`types.any.all`).
    if trait_ == s.any {
        return holds(BuiltinImpl::Any);
    }
    match t {
        TyData::Infer(v) => return Some(Row::Stalled(v)),
        TyData::Param(_) | TyData::Assoc { .. } => return Some(Row::Fails(FailReason::NoImpl)),
        _ => {}
    }
    if trait_ == s.any_val || trait_ == s.any_ref {
        let (b, want_ref) = if trait_ == s.any_ref {
            (BuiltinImpl::AnyRef, true)
        } else {
            (BuiltinImpl::AnyVal, false)
        };
        return Some(match category(pool, decls, self_ty) {
            Category::Val if !want_ref => Row::Holds(b, Vec::new()),
            Category::Ref if want_ref => Row::Holds(b, Vec::new()),
            Category::Base(base) => Row::Holds(b, vec![base]),
            // `never` implements neither (`types.sealed.never`).
            _ => Row::Fails(FailReason::WrongCategory),
        });
    }
    if trait_ == s.tuple {
        return Some(if matches!(t, TyData::Tuple { .. }) {
            Row::Holds(BuiltinImpl::Tuple, Vec::new())
        } else {
            Row::Fails(FailReason::NoImpl)
        });
    }
    if trait_ == s.inspectable {
        let mut subs = Vec::new();
        let ok = match t {
            TyData::Prim(_) => true,
            // Elements are values, not arguments (`trait.inspectable.collections`);
            // a rest element is its list's argument (`annot.tuple.rest`).
            TyData::Tuple { elems, rest } => {
                subs.extend(pool.list_items(elems).iter().copied());
                if let Some(r) = rest {
                    argument(pool, r, &mut subs);
                }
                true
            }
            // A declaration applied to inspectable arguments, `Option`
            // included (`trait.inspectable.declared`, `.option`), unless it
            // is local to a block suite (`trait.inspectable.not.local`).
            TyData::Option(i) => {
                argument(pool, i, &mut subs);
                true
            }
            TyData::Adt { def, .. } if decls.is_local(def) => false,
            TyData::Adt { args, .. } => {
                for a in pool.list_items(args) {
                    argument(pool, *a, &mut subs);
                }
                true
            }
            // `never`, function types and trait values (whose own row is
            // §9.3's) are not inspectable as values.
            _ => false,
        };
        return Some(if ok {
            Row::Holds(BuiltinImpl::Inspectable, subs)
        } else {
            Row::Fails(FailReason::NotInspectable { arg: self_ty })
        });
    }
    // `Structure`: a declared target has it only through a template
    // instance's environment (`annot.structure.generated`). A tuple type
    // has the compiler's (`annot.tuple.structure`): checking never asks it
    // at a concrete tuple, since `Structure` is named only inside a
    // template (`annot.structure.named-positions`), where a tuple
    // template's `T` is a parameter that the environment answers; codegen
    // asks it once a tuple template's instance is collected, and lowers
    // its methods per tuple type (codegen.md §13.6).
    Some(if matches!(t, TyData::Tuple { .. }) {
        Row::Holds(BuiltinImpl::Structure, Vec::new())
    } else {
        Row::Fails(FailReason::NoImpl)
    })
}
