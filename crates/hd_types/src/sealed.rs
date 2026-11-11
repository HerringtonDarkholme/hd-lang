//! Compiler-supplied impls (`crates/hd_check/DESIGN.md` (trait solver) §3.9): one function answers
//! each sealed trait the compiler implements from the type's form, as the
//! rows of one `match`. The rows read no impl table, so their answers
//! depend on no impl universe; they read only the declarations a
//! [`Declarations`] gives, which are the same in every context of a run.
//!
//! `Num`, `Integer` and `Float` are not rows here: std writes their one
//! impl per primitive (`lib/std/num.hd`), with the bodies of `zero`, `one`
//! and `from_i64`, so the impl tables answer them like any trait.

use hd_base::{DefId, HashSet, InferVar};

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
    /// `Fn[I, O, $R]`, implemented by every plain function type with
    /// matching inputs, output and row.
    pub fn_trait: DefId,
    /// `FnMut[I, O, $R]`, `Fn`'s supertrait, implemented by the same types.
    pub fn_mut_trait: DefId,
    pub structure: DefId,
    pub union_type: DefId,
    /// The `'static` bound (`ref.generic.static`), which a type meets
    /// when it holds no local reference.
    pub static_bound: DefId,
    /// The `Copy` marker (`ref.copy.marker`), which a type meets when no
    /// part of it held by value is a `&mut`, a type declared `!Copy`, or a
    /// type parameter without the bound.
    pub copy: DefId,
    /// `List` and `Map`, values with copy-on-write (`types.coll.value`),
    /// so `AnyVal` types.
    pub list: DefId,
    pub map: DefId,
    /// A suspension has identity regardless of its compiler-seeded declaration.
    pub suspend: DefId,
}

impl Default for SealedTraits {
    fn default() -> Self {
        SealedTraits {
            any: DefId::NONE,
            any_val: DefId::NONE,
            any_ref: DefId::NONE,
            inspectable: DefId::NONE,
            tuple: DefId::NONE,
            fn_trait: DefId::NONE,
            fn_mut_trait: DefId::NONE,
            structure: DefId::NONE,
            union_type: DefId::NONE,
            static_bound: DefId::NONE,
            copy: DefId::NONE,
            list: DefId::NONE,
            map: DefId::NONE,
            suspend: DefId::NONE,
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
                self.fn_trait,
                self.fn_mut_trait,
                self.structure,
                self.union_type,
                self.static_bound,
                self.copy,
            ]
            .contains(&trait_)
    }
}

/// What a nominal type's declaration is, as the rows read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeDecl {
    /// A std type the compiler declares (`List`, `Map`, `Suspend`), or no
    /// type declaration.
    Builtin,
    /// A `struct`: a value type (`types.value.struct`).
    Struct,
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
    /// (`trait.dyn.member.unavailable`, `trait.dyn.bound.reference`).
    fn has_unavailable_member(&self, trait_: DefId) -> bool;
    /// Member availability through a trait value reference.
    fn has_unavailable_reference_member(&self, trait_: DefId, _mutable: bool) -> bool {
        self.has_unavailable_member(trait_)
    }
    /// Whether a declaration is local to a block suite, which is never
    /// inspectable (`trait.inspectable.not.local`).
    fn is_local(&self, def: DefId) -> bool;
    /// Whether argument `index` of the declaration `def` fills a const
    /// parameter (`types.generic.const.parameter`): a value, not a type.
    fn is_const_slot(&self, _def: DefId, _index: usize) -> bool {
        false
    }
    /// What the `Copy` row reads of a nominal type (`ref.copy.structure`).
    fn copy_decl(&self, def: DefId) -> CopyDecl;
    /// Whether trait `trait_`'s `Self` is `Copy`: it is not declared
    /// `< ?Copy` (`ref.copy.trait.default`).
    fn trait_requires_copy(&self, trait_: DefId) -> bool;
}

/// A nominal type's part in the `Copy` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyDecl {
    /// No members the row reads: a std type with no declaration.
    Copies,
    /// Declared `impl !Copy` (`ref.copy.opt-out`).
    NotCopy,
    /// The types of its fields and payloads, over its own parameters.
    Members(Vec<Ty>),
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
        TyData::Prim(_) | TyData::Tuple { .. } | TyData::Fn { .. } => Category::Val,
        // A reference has identity: what it refers to (ref.type.any-ref).
        TyData::TraitValue { .. } | TyData::Ref { .. } => Category::Ref,
        // A collection is a value (`types.coll.value`).
        TyData::Adt { def, .. }
            if def != DefId::NONE && (def == decls.sealed().list || def == decls.sealed().map) =>
        {
            Category::Val
        }
        TyData::Adt { def, .. } if def != DefId::NONE && def == decls.sealed().suspend => {
            Category::Ref
        }
        TyData::Adt { def, args } => match decls.type_decl(def) {
            TypeDecl::Enum | TypeDecl::Struct => Category::Val,
            TypeDecl::Builtin => Category::Ref,
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
/// the argument itself, except that a trait value type counts as an
/// argument only (`trait.inspectable.counted-reference-type`) and adds just its
/// local reference arguments, its lifetimes among them, which fail
/// (`trait.inspectable.not.local-reference`).
fn argument(pool: Types<'_>, t: Ty, out: &mut Vec<Ty>) {
    let inner = t;
    if let TyData::TraitValue { args, bindings, .. } = pool.get(inner) {
        let local = |a: &Ty| matches!(pool.get(*a), TyData::Ref { counted: false, .. });
        out.extend(pool.list_items(args).iter().copied().filter(local));
        out.extend(bindings.iter().map(|&(_, b)| b).filter(local));
    } else {
        out.push(t);
    }
}

/// The row of `self_ty: trait_` (§3.9), or `None` when no row answers
/// `trait_`. `self_ty` is not poison, and an environment clause did not answer it: a parameter
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
    if let TyData::Infer(v) = t {
        return Some(Row::Stalled(v));
    }
    if trait_ == s.static_bound {
        return Some(static_row(pool, self_ty));
    }
    if trait_ == s.copy {
        // An environment clause answers a parameter before this row; a
        // parameter it does not answer was declared `?Copy`.
        if matches!(t, TyData::Param(_)) {
            return Some(Row::Fails(FailReason::NoImpl));
        }
        let mut subs = Vec::new();
        return Some(
            if copy_parts(pool, decls, self_ty, &mut subs, &mut Walk::default()) {
                Row::Holds(BuiltinImpl::Copy, subs)
            } else {
                Row::Fails(FailReason::NoImpl)
            },
        );
    }
    // A variable stalled above, before the `'static` and `Copy` rows.
    if matches!(
        t,
        TyData::Param(_) | TyData::Opaque { .. } | TyData::Assoc { .. }
    ) {
        return Some(Row::Fails(FailReason::NoImpl));
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
    if trait_ == s.union_type {
        return Some(if matches!(t, TyData::Union(_)) {
            Row::Holds(BuiltinImpl::Union, Vec::new())
        } else {
            Row::Fails(FailReason::NoImpl)
        });
    }
    if trait_ == s.inspectable {
        let mut subs = Vec::new();
        let ok = match t {
            TyData::Prim(_) => true,
            // A counted reference; a local one, as a struct or enum's
            // lifetime argument is, holds a local reference and fails, so
            // an argument holding one at any depth fails as its own goal
            // (`trait.inspectable.not.local-reference`).
            TyData::Ref {
                inner,
                counted: true,
                ..
            } => {
                argument(pool, inner, &mut subs);
                true
            }
            // Elements are values, not arguments (`trait.inspectable.collections`);
            // a rest element is its list's argument (`annot.tuple.rest`).
            TyData::Tuple { elems, rest } => {
                subs.extend(pool.list_items(elems).iter().copied());
                if let Some(r) = rest {
                    argument(pool, r, &mut subs);
                }
                true
            }
            TyData::Union(members) => {
                subs.extend(pool.list_items(members).iter().copied());
                true
            }
            // A declaration applied to inspectable arguments, `Option`
            // included (`trait.inspectable.declared`, `.option`), unless it
            // is local to a block suite (`trait.inspectable.not.local`).
            TyData::Adt { def, .. } if decls.is_local(def) => false,
            // Only type arguments count: a const argument is part of the
            // type's identity, and a row argument (`req.row.param.data-owner`)
            // is a requirement row, not a value type.
            TyData::Adt { def, args } => {
                for (i, a) in pool.list_items(args).iter().enumerate() {
                    if !decls.is_const_slot(def, i) && !matches!(pool.get(*a), TyData::Row(_)) {
                        argument(pool, *a, &mut subs);
                    }
                }
                true
            }
            // `never`, local references, function types and trait values
            // (whose own row is §9.3's) are not inspectable as values.
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
    // its methods per tuple type (`crates/hd_mono/DESIGN.md` §13.6).
    Some(if matches!(t, TyData::Tuple { .. } | TyData::Union(_)) {
        Row::Holds(BuiltinImpl::Structure, Vec::new())
    } else {
        Row::Fails(FailReason::NoImpl)
    })
}

/// `ref.generic.static`: a type meets `'static` when it holds no local
/// reference, so each part a value holds must meet it too. A counted
/// reference meets it, since `box` made its object from a value that met
/// it; a function value holds no local reference, since a closure never
/// captures one. A parameter meets it
/// only by its own bound, which the environment answers before this row.
/// Whether `t` may copy (`ref.copy.structure`): no part held by value is
/// a `&mut`, a type declared `!Copy`, or a type parameter whose bound
/// fails. The parameters and inference variables it reaches are left in
/// `subs`, for the environment to answer. A projection and an opaque type
/// copy, as today.
fn copy_parts(
    pool: Types<'_>,
    decls: &dyn Declarations,
    t: Ty,
    subs: &mut Vec<Ty>,
    seen: &mut Walk,
) -> bool {
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => !mutable || counted,
        TyData::Tuple { elems, rest } => {
            pool.list_items(elems)
                .iter()
                .all(|&e| copy_parts(pool, decls, e, subs, seen))
                && rest.is_none_or(|r| copy_parts(pool, decls, r, subs, seen))
        }
        TyData::Union(ms) => pool
            .list_items(ms)
            .iter()
            .all(|&m| copy_parts(pool, decls, m, subs, seen)),
        TyData::Param(_) | TyData::Infer(_) => {
            if !subs.contains(&t) {
                subs.push(t);
            }
            true
        }
        TyData::Adt { def, args } => {
            if seen.copies.contains(&t) || seen.open.contains(&t) || seen.open.len() > 32 {
                return true;
            }
            match decls.copy_decl(def) {
                CopyDecl::Copies => true,
                CopyDecl::NotCopy => false,
                CopyDecl::Members(ms) => {
                    let argv = pool.list_items(args);
                    seen.open.push(t);
                    let ok = ms.iter().all(|&m| {
                        let m = pool.subst(m, &|p: ParamRef| {
                            (p.owner == def)
                                .then(|| argv.get(usize::from(p.index)).copied())
                                .flatten()
                        });
                        copy_parts(pool, decls, m, subs, seen)
                    });
                    seen.open.pop();
                    if ok {
                        seen.copies.insert(t);
                    }
                    ok
                }
            }
        }
        _ => true,
    }
}

/// The nominal types `copy_parts` is inside, and those it already found to
/// copy, so a type argument shared by many fields (`Pair[X, X]`) is walked
/// once rather than once per path to it.
#[derive(Default)]
struct Walk {
    open: Vec<Ty>,
    copies: HashSet<Ty>,
}

fn static_row(pool: Types<'_>, self_ty: Ty) -> Row {
    let mut subs = Vec::new();
    match pool.get(self_ty) {
        TyData::Ref { counted: false, .. } => return Row::Fails(FailReason::NoImpl),
        // `box` made the object from a value that met `'static`. A trait
        // value is only ever a reference's referent, so this answers it.
        TyData::Ref { .. } => {}
        TyData::Tuple { elems, rest } => {
            subs.extend_from_slice(pool.list_items(elems));
            subs.extend(rest);
        }
        TyData::Adt { args, .. } => subs.extend_from_slice(pool.list_items(args)),
        // A projection meets it when its parts do, as Rust's projection
        // rule has it (RFC 1214). An opaque type's hidden type is unknown
        // here, so it meets it only by an environment clause.
        TyData::Assoc { self_ty, args, .. } => {
            subs.push(self_ty);
            subs.extend_from_slice(pool.list_items(args));
        }
        // An environment clause answers a parameter before this row.
        TyData::Param(_) | TyData::Opaque { .. } => return Row::Fails(FailReason::NoImpl),
        // No part to check: a value with no reference inside, a goal
        // already in error, or one the solver stalls on before this row.
        TyData::Prim(_)
        | TyData::Never
        | TyData::Poison
        | TyData::Union(_)
        | TyData::Fn { .. }
        | TyData::TraitValue { .. }
        | TyData::Const(_)
        | TyData::Infer(_)
        | TyData::Canon(_)
        | TyData::Row(_)
        | TyData::Context(_) => {}
    }
    subs.retain(|&t| {
        pool.has_ref(t) || pool.has_param(t) || pool.has_infer(t) || pool.has_assoc(t)
    });
    Row::Holds(BuiltinImpl::Static, subs)
}
