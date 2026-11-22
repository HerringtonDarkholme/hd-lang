//! Compiler-supplied impls (`crates/hd_check/DESIGN.md` (trait solver) §3.9): one function answers
//! each sealed trait the compiler implements from the type's form, as the
//! rows of one `match`. The rows read no impl table, so their answers
//! depend on no impl universe; they read only the declarations a
//! [`Declarations`] gives, which are the same in every context of a run.
//!
//! `Num`, `Integer` and `Float` are not rows here: std writes their one
//! impl per primitive (`lib/std/num.hd`), with the bodies of `zero`, `one`
//! and `from_i64`, so the impl tables answer them like any trait.

use crate::ANON_SUM;
use hd_base::{DefId, HashMap, InferVar};

use crate::pool::{ParamRef, Ty, TyData, TyList, Types};
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
    /// `Marker` (`module.marker.definition`): a trait that names it as a
    /// supertrait is a marker, which the structural walk answers.
    pub marker: DefId,
    /// `Send` and `Wire` (`module.marker.std`), answered from the type's
    /// parts (`module.marker.structural`).
    pub send: DefId,
    pub wire: DefId,
    /// `Remote` (`module.remote.std`): a counted trait value of a trait
    /// that extends it is `Wire` (`module.marker.table.remote`).
    pub remote: DefId,
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
            marker: DefId::NONE,
            send: DefId::NONE,
            wire: DefId::NONE,
            remote: DefId::NONE,
        }
    }
}

impl SealedTraits {
    /// Whether a row of this module answers goals on `trait_`.
    #[must_use]
    pub fn supplies(&self, trait_: DefId) -> bool {
        trait_ != DefId::NONE && self.supplied().contains(&trait_)
    }

    /// The traits a row of this module answers goals on
    /// ([`Self::supplies`]); `DefId::NONE` for one std does not declare.
    #[must_use]
    pub fn supplied(&self) -> [DefId; 15] {
        [
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
            self.send,
            self.wire,
            self.marker,
            self.remote,
        ]
    }

    /// Whether `trait_` is a marker: it names `Marker` among its direct
    /// supertraits (`module.marker.definition`).
    #[must_use]
    pub fn is_marker(&self, decls: &dyn Declarations, pool: Types<'_>, trait_: DefId) -> bool {
        self.marker != DefId::NONE
            && decls.supertraits(trait_).iter().any(
                |&t| matches!(pool.get(t), TyData::TraitValue { def, .. } if def == self.marker),
            )
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
    /// The checker's answers [`CopyDecl::Copies`] for a declaration the
    /// value rule rejects (`hd_check::value::SolverDecls`): an instance's
    /// members may grow without end (#2013), and the declaration is
    /// reported. Codegen never meets one.
    fn copy_decl(&self, def: DefId) -> CopyDecl;
    /// Whether trait `trait_`'s `Self` is `Copy`: it is not declared
    /// `< ?Copy` (`ref.copy.trait.default`).
    fn trait_requires_copy(&self, trait_: DefId) -> bool;
}

/// A nominal type's part in the `Copy` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyDecl {
    /// No members the row reads: a std type with no declaration, or a
    /// declaration that holds itself, which the checker reports.
    Copies,
    /// Declared `impl !Copy` (`ref.copy.opt-out`), with its members, which
    /// a marker still reads (`module.marker.not-copy`).
    NotCopy(Vec<Ty>),
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
        TyData::Prim(_)
        | TyData::Tuple { .. }
        | TyData::Fn { .. }
        | TyData::Adt { def: ANON_SUM, .. } => Category::Val,
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
        TyData::Adt { def, args } if def.get().is_some() => match decls.type_decl(def) {
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
    copies: &mut CopyMemo,
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
    // `Marker`: every type, so it constrains only the traits that extend
    // it (`trait.sealed.marker`).
    // `Remote` too (`trait.sealed.remote`).
    if trait_ == s.marker || trait_ == s.remote {
        return holds(BuiltinImpl::Marker);
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
            if copy_parts(pool, decls, Walk::Copy, self_ty, &mut subs, copies, &|_| {
                false
            }) {
                Row::Holds(BuiltinImpl::Copy, subs)
            } else {
                Row::Fails(FailReason::NoImpl)
            },
        );
    }
    if trait_ == s.send || trait_ == s.wire {
        if matches!(t, TyData::Param(_)) {
            return Some(Row::Fails(FailReason::NoImpl));
        }
        let walk = if trait_ == s.send {
            Walk::Send
        } else {
            Walk::Wire
        };
        return Some(marker_row(pool, decls, walk, self_ty, copies, &|_| false));
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
        return Some(if matches!(t, TyData::Adt { def: ANON_SUM, .. }) {
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
            // A declaration applied to inspectable arguments, `Option`
            // included (`trait.inspectable.declared`, `.option`), unless it
            // is local to a block suite (`trait.inspectable.not.local`).
            TyData::Adt { def, .. } if def != ANON_SUM && decls.is_local(def) => false,
            // Only type arguments count: a const argument is part of the
            // type's identity, and a row argument (`req.row.param.data-owner`)
            // is a requirement row, not a value type.
            TyData::Adt { def, args } => {
                for (i, a) in pool.list_items(args).iter().enumerate() {
                    if def == ANON_SUM {
                        subs.push(*a);
                    } else if !decls.is_const_slot(def, i)
                        && !matches!(pool.get(*a), TyData::Row(_))
                    {
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
    Some(match t {
        TyData::Tuple { .. } | TyData::Adt { def: ANON_SUM, .. } => {
            Row::Holds(BuiltinImpl::Structure, Vec::new())
        }
        _ => Row::Fails(FailReason::NoImpl),
    })
}

/// The answers of earlier parts walks, by walk and type id: `None` for a
/// type that fails, else the parameters and variables left for the
/// environment, in walk order. A type's answer is a function of its
/// interned content and the run's declarations alone, so an entry stays
/// valid for as long as the type's pool does (a body's, `BodyMemo`). A
/// library marker's answer also reads which heads have an explicit impl,
/// which are the same in every context of a run.
pub(crate) type CopyMemo = HashMap<(Walk, Ty), Option<TyList>>;

/// Which trait a parts walk answers: `Copy`, a std marker, or a library
/// marker (`module.marker.structural`). One walk serves them all; they
/// differ only in what a leaf may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Walk {
    Copy,
    Send,
    Wire,
    Library(DefId),
}

/// A marker's row: the parts walk, which leaves parameters, variables and
/// (for a library marker) the parts an explicit impl answers as subgoals
/// on the same trait.
pub(crate) fn marker_row(
    pool: Types<'_>,
    decls: &dyn Declarations,
    walk: Walk,
    self_ty: Ty,
    memo: &mut CopyMemo,
    explicit: &dyn Fn(Ty) -> bool,
) -> Row {
    let mut subs = Vec::new();
    if copy_parts(pool, decls, walk, self_ty, &mut subs, memo, explicit) {
        Row::Holds(BuiltinImpl::Marker, subs)
    } else {
        Row::Fails(FailReason::NoImpl)
    }
}

/// How a marker walk judges a node that is not a composite: `None` to
/// keep walking (its parts, if any, are pushed), `Some(false)` to fail.
/// `Copy` judges in [`copy_parts`] itself.
fn marker_leaf(
    pool: Types<'_>,
    decls: &dyn Declarations,
    walk: Walk,
    data: &TyData,
    stack: &mut Vec<Ty>,
) -> bool {
    let s = decls.sealed();
    match *data {
        TyData::Prim(_) | TyData::Never | TyData::Poison => !matches!(walk, Walk::Library(_)),
        // `&T` is `Send` when `T` is (`module.marker.table.shared-sync`,
        // whose `Sync` test is #2288's next slice); a remote reference is
        // `Wire` (`module.marker.table.remote`).
        TyData::Ref {
            mutable,
            counted,
            inner,
            ..
        } => match walk {
            Walk::Send if !mutable && !counted => {
                stack.push(inner);
                true
            }
            Walk::Wire if counted => matches!(
                pool.get(inner),
                TyData::TraitValue { def, .. }
                    if s.remote != DefId::NONE && extends(pool, decls, def, s.remote)
            ),
            _ => false,
        },
        // A function value, a trait value, a provider context, a
        // projection or an opaque type crosses nowhere
        // (`module.marker.table.fn-value`, `.counted`, `.provider`).
        _ => false,
    }
}

/// Whether trait `def` names `base` among its supertraits, at any depth.
/// Cost: the supertrait graph above `def`, each trait once.
fn extends(pool: Types<'_>, decls: &dyn Declarations, def: DefId, base: DefId) -> bool {
    let mut seen = vec![def];
    let mut at = 0;
    while let Some(&d) = seen.get(at) {
        at += 1;
        for &t in decls.supertraits(d) {
            if let TyData::TraitValue { def: up, .. } = pool.get(t) {
                if up == base {
                    return true;
                }
                if !seen.contains(&up) {
                    seen.push(up);
                }
            }
        }
    }
    false
}

/// Whether `t` may copy (`ref.copy.structure`): no part held by value is
/// a `&mut`, a type declared `!Copy`, or a type parameter whose bound
/// fails. The parameters and inference variables it reaches are left in
/// `subs`, for the environment to answer. A projection and an opaque type
/// copy, as today.
///
/// A work stack, not recursion: a declaration's members nest as deep as
/// the program's types do, and a fixed depth cap answered "copies" past
/// it (#1777). Each distinct node with parts, and each parameter or
/// variable, is entered once (`marks`, by type id): that ends a cycle
/// through a declaration (`Node { next: Node? }`), and walks a part
/// shared by many fields (`Pair[X, X]`) or tuple slots (`(T, T)` nested
/// n deep, 2^n leaves) once, so the work is the type's distinct nodes
/// (#1886). Parts are taken left to right, so `subs` keeps the order a
/// recursive walk gives.
///
/// Across walks, `memo` keeps each asked type's answer, and on success
/// every entered node whose answer the walk knows. A walk stops at a part
/// with an answer and takes its parts (#1912): a generic call per level
/// of `wrap(wrap(...))` asks `Copy` of `W^i[?int]`, whose member
/// `W^(i-1)[?int]` the call below asked, so each level walks one new
/// node, not the `i` below it. The walk keeps its open path, so a node's
/// answer is known when its last part is done: nothing left, or the one
/// list of parameters and variables every part that leaves something
/// leaves (an interned list, compared by id). A node whose parts leave
/// different lists, or that reaches a node still open (a cycle), is not
/// kept, which bounds the work per node by its parts. So a chain asked
/// from the top down (`Two[Two[S::Item]]`, or `Two[Two[T]]`) answers each
/// level below from the first walk (#1985). Cost: the distinct nodes no
/// earlier walk answered, plus the answered parts' variables; a hash
/// probe per node with parts, and one interned list per parameter or
/// variable.
fn copy_parts(
    pool: Types<'_>,
    decls: &dyn Declarations,
    walk: Walk,
    root: Ty,
    subs: &mut Vec<Ty>,
    memo: &mut CopyMemo,
    explicit: &dyn Fn(Ty) -> bool,
) -> bool {
    /// What a node leaves for the environment, as far as the walk knows.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Left {
        Nothing,
        List(TyList),
        Unknown,
    }
    impl Left {
        fn and(self, other: Left) -> Left {
            match (self, other) {
                (Left::Nothing, x) | (x, Left::Nothing) => x,
                (Left::List(a), Left::List(b)) if a == b => self,
                _ => Left::Unknown,
            }
        }
    }
    /// Enter a node, or close it, the innermost open one, once its
    /// parts are done.
    enum Step {
        Enter(Ty),
        Close(Ty),
    }
    /// The open node that holds the part just met takes what it leaves.
    fn take(open: &mut [(Ty, Left)], left: Left) {
        if let Some(top) = open.last_mut() {
            top.1 = top.1.and(left);
        }
    }
    /// A parameter or variable met for the first time is left.
    fn leave(pool: Types<'_>, marks: &mut HashMap<Ty, Left>, subs: &mut Vec<Ty>, t: Ty) -> Left {
        if let Some(&left) = marks.get(&t) {
            return left;
        }
        let left = Left::List(pool.list(&[t]));
        marks.insert(t, left);
        subs.push(t);
        left
    }
    let start = subs.len();
    let mut stack: Vec<Step> = vec![Step::Enter(root)];
    // What each entered node leaves. A node on the open path reads
    // `Unknown` until it closes, so a cycle back to it is not kept. A
    // cycle is a declaration that holds itself: the checker reports it
    // and its `copy_decl` answers it as copying, so only another
    // `Declarations` walks one.
    let mut marks: HashMap<Ty, Left> = HashMap::default();
    // The open path, each node with what its parts so far leave.
    let mut open: Vec<(Ty, Left)> = Vec::new();
    // Entered nodes with parts and no parameter or variable: on success,
    // each copies with nothing left, a cycle through them included.
    let mut closed: Vec<Ty> = Vec::new();
    while let Some(step) = stack.pop() {
        let t = match step {
            Step::Enter(t) => t,
            Step::Close(t) => {
                let left = open.pop().map_or(Left::Unknown, |(_, left)| left);
                marks.insert(t, left);
                take(&mut open, left);
                continue;
            }
        };
        let data = pool.get(t);
        // A library marker's part with an explicit impl is left for that
        // impl to answer (`module.marker.explicit`).
        let answered = t != root && matches!(walk, Walk::Library(_)) && explicit(t);
        match data {
            _ if answered => {
                if !marks.contains_key(&t) {
                    crate::work::type_node();
                }
                let left = leave(pool, &mut marks, subs, t);
                take(&mut open, left);
                continue;
            }
            TyData::Param(_) | TyData::Infer(_) => {
                if !marks.contains_key(&t) {
                    crate::work::type_node();
                }
                let left = leave(pool, &mut marks, subs, t);
                take(&mut open, left);
                continue;
            }
            TyData::Tuple { .. } | TyData::Adt { .. } => {
                if let Some(&left) = marks.get(&t) {
                    take(&mut open, left);
                    continue;
                }
                match memo.get(&(walk, t)) {
                    Some(None) => return fails(memo, walk, root),
                    Some(Some(list)) => {
                        let list = *list;
                        for &s in pool.list_items(list) {
                            leave(pool, &mut marks, subs, s);
                        }
                        let left = if pool.list_items(list).is_empty() {
                            Left::Nothing
                        } else {
                            Left::List(list)
                        };
                        marks.insert(t, left);
                        take(&mut open, left);
                        continue;
                    }
                    // A library marker's walk may leave a part an
                    // explicit impl answers, so its nodes are not closed.
                    None if !pool.has_param(t)
                        && !pool.has_infer(t)
                        && !matches!(walk, Walk::Library(_)) =>
                    {
                        closed.push(t);
                    }
                    None => {}
                }
                marks.insert(t, Left::Unknown);
                open.push((t, Left::Nothing));
                stack.push(Step::Close(t));
            }
            TyData::Ref { .. }
            | TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Fn { .. }
            | TyData::TraitValue { .. }
            | TyData::Opaque { .. }
            | TyData::Const(_)
            | TyData::Assoc { .. }
            | TyData::Canon(_)
            | TyData::Row(_)
            | TyData::Context(_) => {}
        }
        crate::work::type_node();
        match data {
            TyData::Tuple { elems, rest } => {
                stack.extend(rest.map(Step::Enter));
                stack.extend(pool.list_items(elems).iter().rev().map(|&e| Step::Enter(e)));
            }
            TyData::Adt { def, args } if def == ANON_SUM => {
                stack.extend(pool.list_items(args).iter().rev().map(|&m| Step::Enter(m)));
            }
            TyData::Adt { def, args } => match decls.copy_decl(def) {
                CopyDecl::NotCopy(_) if walk == Walk::Copy => return fails(memo, walk, root),
                // A collection holds its type arguments by value for a
                // marker (`module.marker.structural`); a suspension and
                // any other built-in crosses nowhere.
                CopyDecl::Copies if walk != Walk::Copy => {
                    let s = decls.sealed();
                    if def == s.list || def == s.map {
                        let argv = pool.list_items(args);
                        stack.extend(argv.iter().rev().map(|&a| Step::Enter(a)));
                    } else if decls.type_decl(def) == TypeDecl::Builtin {
                        return fails(memo, walk, root);
                    }
                }
                CopyDecl::Copies => {}
                CopyDecl::Members(ms) | CopyDecl::NotCopy(ms) => {
                    let argv = pool.list_items(args);
                    stack.extend(ms.iter().rev().map(|&m| {
                        Step::Enter(pool.subst(m, &|p: ParamRef| {
                            (p.owner == def)
                                .then(|| argv.get(usize::from(p.index)).copied())
                                .flatten()
                        }))
                    }));
                }
            },
            // `Copy`: only a `&mut` held by value blocks a copy; a
            // projection and an opaque type copy, as today.
            TyData::Ref {
                mutable, counted, ..
            } if walk == Walk::Copy => {
                if mutable && !counted {
                    return fails(memo, walk, root);
                }
            }
            TyData::Param(_) | TyData::Infer(_) => {}
            _ if walk == Walk::Copy => {}
            // A marker judges every other node by its leaf rule.
            _ => {
                let mut parts = Vec::new();
                if !marker_leaf(pool, decls, walk, &data, &mut parts) {
                    return fails(memo, walk, root);
                }
                stack.extend(parts.into_iter().map(Step::Enter));
            }
        }
    }
    for t in closed {
        memo.insert((walk, t), Some(TyList::EMPTY));
    }
    for (&t, &left) in &marks {
        if matches!(pool.get(t), TyData::Param(_) | TyData::Infer(_)) {
            continue;
        }
        match left {
            Left::Nothing => {
                memo.insert((walk, t), Some(TyList::EMPTY));
            }
            Left::List(list) => {
                memo.insert((walk, t), Some(list));
            }
            Left::Unknown => {}
        }
    }
    memo.insert((walk, root), Some(pool.list(&subs[start..])));
    true
}

/// Whether some instance of a declaration may be `Copy`
/// (`module.rt.drop.not-copy.declaration`): the `Copy` row's own walk
/// ([`copy_parts`]) over the declaration's identity type `D[P..]`. The
/// walk leaves each parameter for an environment and judges none, so no
/// parameter counts, `?Copy` or not: only a part held by value that is a
/// `&mut`, a declaration declared `!Copy`, or one holding either, fails.
/// Cost: the identity type's distinct nodes, once per asked declaration.
#[must_use]
pub fn may_copy(pool: Types<'_>, decls: &dyn Declarations, identity: Ty) -> bool {
    copy_parts(
        pool,
        decls,
        Walk::Copy,
        identity,
        &mut Vec::new(),
        &mut CopyMemo::default(),
        &|_| false,
    )
}

/// Whether `t` may be `Send` (`module.marker.structural`): the `Send`
/// walk, which leaves each parameter for an environment and judges none.
/// Cost: the type's distinct nodes.
#[must_use]
pub fn may_send(pool: Types<'_>, decls: &dyn Declarations, t: Ty) -> bool {
    copy_parts(
        pool,
        decls,
        Walk::Send,
        t,
        &mut Vec::new(),
        &mut CopyMemo::default(),
        &|_| false,
    )
}

/// [`copy_parts`]'s answer for a type with a part that does not copy:
/// only the asked type is kept, since the walk stopped before its other
/// parts were answered.
fn fails(memo: &mut CopyMemo, walk: Walk, root: Ty) -> bool {
    memo.insert((walk, root), None);
    false
}

/// Structural requirements of `ref.generic.static.no-local`. Qualifiers
/// and captured value origins live outside Ty, so successful compound
/// proofs defer those checks to the ordinary region pass. A parameter
/// still needs its own bound, answered by the environment before this row.
fn static_row(pool: Types<'_>, self_ty: Ty) -> Row {
    let mut subs = Vec::new();
    match pool.get(self_ty) {
        TyData::Ref { inner, .. } => subs.push(inner),
        TyData::Tuple { elems, rest } => {
            subs.extend_from_slice(pool.list_items(elems));
            subs.extend(rest);
        }
        TyData::Adt { args, .. } => {
            subs.extend_from_slice(pool.list_items(args));
        }
        // A projection meets it when its parts do, as Rust's projection
        // rule has it (RFC 1214). An opaque type's hidden type is unknown
        // here, so it meets it only by an environment clause.
        TyData::Assoc { self_ty, args, .. } => {
            subs.push(self_ty);
            subs.extend_from_slice(pool.list_items(args));
        }
        // An environment clause answers a parameter before this row.
        TyData::Param(_) | TyData::Opaque { .. } => return Row::Fails(FailReason::NoImpl),
        // No structural part to ask. A function's captures are value
        // origins checked by the deferred requirement, not signature types.
        TyData::Prim(_)
        | TyData::Never
        | TyData::Poison
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

#[cfg(test)]
mod coverage_contracts {
    use super::*;

    struct Decl {
        sealed: SealedTraits,
        unavailable: bool,
    }

    impl Declarations for Decl {
        fn sealed(&self) -> &SealedTraits {
            &self.sealed
        }
        fn type_decl(&self, def: DefId) -> TypeDecl {
            assert!(def.get().is_some(), "anonymous sum has no declaration");
            TypeDecl::Builtin
        }
        fn supertraits(&self, _: DefId) -> &[Ty] {
            &[]
        }
        fn has_unavailable_member(&self, _: DefId) -> bool {
            self.unavailable
        }
        fn is_local(&self, def: DefId) -> bool {
            assert!(def.get().is_some(), "anonymous sum has no declaration");
            false
        }
        fn is_const_slot(&self, def: DefId, _: usize) -> bool {
            assert!(def.get().is_some(), "anonymous sum has no declaration");
            false
        }
        fn copy_decl(&self, def: DefId) -> CopyDecl {
            assert!(def.get().is_some(), "anonymous sum has no declaration");
            CopyDecl::Copies
        }
        fn trait_requires_copy(&self, _: DefId) -> bool {
            false
        }
    }

    #[test]
    fn anonymous_sum_rows_never_query_a_nominal_declaration() {
        let global = crate::InternPool::new();
        let p = global.types();
        let id = DefId::from_raw;
        let decls = Decl {
            sealed: SealedTraits {
                any_val: id(1),
                any_ref: id(2),
                inspectable: id(3),
                union_type: id(4),
                structure: id(5),
                copy: id(6),
                send: id(7),
                wire: id(8),
                ..SealedTraits::default()
            },
            unavailable: false,
        };
        let members = [Ty::I32, Ty::STRING];
        let sum = p.intern_ty(&TyData::sum(p.list(&members)));
        let mut memo = CopyMemo::default();
        for (trait_, builtin) in [
            (1, BuiltinImpl::AnyVal),
            (4, BuiltinImpl::Union),
            (5, BuiltinImpl::Structure),
            (6, BuiltinImpl::Copy),
            (7, BuiltinImpl::Marker),
            (8, BuiltinImpl::Marker),
        ] {
            assert_eq!(
                row(p, &decls, id(trait_), sum, &mut memo),
                Some(Row::Holds(builtin, Vec::new())),
            );
        }
        assert_eq!(
            row(p, &decls, id(2), sum, &mut memo),
            Some(Row::Fails(FailReason::WrongCategory)),
        );
        assert_eq!(
            row(p, &decls, id(3), sum, &mut memo),
            Some(Row::Holds(BuiltinImpl::Inspectable, members.to_vec())),
        );
    }

    #[test]
    fn default_reference_member_availability_delegates_for_both_permissions() {
        for unavailable in [false, true] {
            let decl = Decl {
                sealed: SealedTraits::default(),
                unavailable,
            };
            let declarations: &dyn Declarations = &decl;
            for mutable in [false, true] {
                assert_eq!(
                    declarations.has_unavailable_reference_member(DefId::from_raw(1), mutable),
                    unavailable
                );
            }
        }
    }

    #[test]
    fn projected_static_rows_keep_base_and_argument_subgoals() {
        let global = crate::InternPool::new();
        let p = global.types();
        let owner = DefId::from_raw(1);
        let base = p.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
        let argument = p.intern_ty(&TyData::Param(ParamRef { owner, index: 1 }));
        let reference = p.intern_ty(&TyData::Ref {
            inner: Ty::BOOL,
            mutable: false,
            counted: true,
        });
        let projection = |base, args: &[Ty]| {
            p.intern_ty(&TyData::Assoc {
                assoc: DefId::from_raw(2),
                trait_: owner,
                self_ty: base,
                args: p.list(args),
            })
        };
        assert_eq!(
            static_row(p, projection(base, &[Ty::I32, reference, argument])),
            Row::Holds(BuiltinImpl::Static, vec![base, reference, argument]),
        );
        assert_eq!(
            static_row(p, projection(Ty::I32, &[Ty::BOOL])),
            Row::Holds(BuiltinImpl::Static, vec![]),
        );
        let local = p.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        assert_eq!(
            static_row(p, projection(Ty::I32, &[local])),
            Row::Holds(BuiltinImpl::Static, vec![local]),
        );
        assert_eq!(
            static_row(p, local),
            Row::Holds(BuiltinImpl::Static, vec![])
        );
    }
}
