#![forbid(unsafe_code)]
//! `hd_mono`: monomorphizing collection (codegen.md §13.1 to §13.3) over
//! `hd_tir::ir` bodies into the `InstanceTable`, with A1 classes and
//! instance keys from `layout`, and trait selection through the solver's
//! `select` plus a head match. Collection records each call's target, each
//! closure's code instance and each trait-value coercion's vtable, so
//! emission never selects again. Reads TIR and interfaces only, never
//! syntax. Bodies of every package (std included) are collected alike.

pub mod layout;
pub mod passes;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use hd_base::{DefId, Hash128, InstId, NodeIdx, NotImplemented, StableHasher, Stage, StageResult};
use hd_tir::ir::{Body, Callee, ChoiceKind, Coercion, IntrinsicOp, Tag};
use hd_types::solver::{
    BodyMemo, ConcreteTraitRef, Declarations, GlobalMemo, ImplRef, ImplUniverseId, Impls, ParamEnv,
    Selection, SolveCx, Solver, TraitRef,
};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

pub use crate::layout::Sub;
use crate::layout::{
    A1Class, CanonMemo, KeyArg, LayoutEnv, MAX_CHAIN, MAX_DEPTH, a1_class, canon, instance_key,
};

/// What collection reads about the program (the driver implements it over
/// TIR and interfaces).
pub trait ProgramEnv: LayoutEnv {
    fn body(&self, def: DefId) -> Option<&Body>;
    /// Per type parameter of the item itself: has a bound (A1, codegen.md
    /// §13.2: exact).
    fn bounded(&self, def: DefId) -> Option<Vec<bool>>;
    /// The declared result type.
    fn ret(&self, def: DefId) -> Option<Ty>;
    /// The declared parameter types (`self` first for a method).
    fn params(&self, def: DefId) -> Option<Vec<Ty>>;
    /// Whether the function suspends (`fn f!`).
    fn suspends(&self, def: DefId) -> bool;
    /// The requirement keys of an instance's row, each the whole trait
    /// value type with its substituted arguments and bindings (codegen.md
    /// §13.18), in key order (`key_order`, §12.4): the declared keys, and
    /// the keys of each row parameter's argument, so providers are passed
    /// per instance.
    fn row_keys(&self, def: DefId, args: TyList) -> Vec<Ty>;
    /// A method's owner (impl or trait) and how many of the instance's
    /// arguments are the owner's: an impl's parameters, or a trait's
    /// `Self` and parameters.
    fn parent(&self, def: DefId) -> Option<(DefId, usize)>;
    /// The item whose type parameters `def`'s types name: a default body's
    /// declaring function, method or data type, whose instance arguments
    /// it shares (codegen.md §13.13); `def` itself for every other item.
    fn generics_owner(&self, def: DefId) -> DefId;
    /// An impl's head: self type, trait arguments, parameter count.
    fn impl_head(&self, impl_: DefId) -> Option<(Ty, TyList, usize)>;
    /// The method of `impl_` that implements the trait method `method`.
    fn impl_method(&self, impl_: DefId, method: DefId) -> Option<DefId>;
    /// A trait's own methods, in declaration order (the vtable shape),
    /// without those the compiler lowers at each call (an intrinsic
    /// member, `Inspectable.downcast`).
    fn trait_methods(&self, trait_: DefId) -> Vec<DefId>;
    /// A trait's own parameter count, `Self` excluded.
    fn trait_arity(&self, trait_: DefId) -> usize;
    /// The compiler's lowering key of a body-less std function
    /// (`@intrinsic("key")`, or a compiler-supplied item by name).
    fn intrinsic(&self, def: DefId) -> Option<String>;
    /// Data fields, for the struct types a program needs.
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>>;
    fn path_hash(&self, def: DefId) -> Hash128;
    /// A declaration's printable name in a `TypeId` (trait.typeid.name.*):
    /// a prelude name as written, any other declaration by its absolute
    /// qualified name.
    fn type_name(&self, def: DefId) -> String;
    /// The item's stable path, for diagnostics.
    fn describe(&self, def: DefId) -> String;
    /// Where selection reads impls: owner lookup over the whole program's
    /// folders (trait-solver.md §3.2, §8.3), the program being the impl
    /// universe.
    fn impls(&self) -> Impls<'_>;
    /// The program's impl universe and the run's proof memo, which
    /// selection shares with checking (trait-solver.md §7.1).
    fn solving(&self) -> (ImplUniverseId, &GlobalMemo);
    /// Where an impl's row lives.
    fn impl_row(&self, impl_: DefId) -> Option<ImplRef>;
    /// The declarations the solver's compiler-supplied rows read
    /// (trait-solver.md §3.9), so selection answers sealed traits as
    /// checking does.
    fn decls(&self) -> &dyn Declarations;
    /// The body of a compiler-supplied impl's method at a concrete self
    /// type, when the compiler generates one in TIR (a tuple's
    /// `Structure`, codegen.md §13.6): its signature is the trait
    /// method's with `Self` replaced, so its instance's arguments are the
    /// self type, then the trait's and the method's own. `None` when
    /// emission lowers the method itself (trait-solver.md §3.9).
    fn supplied_body(&self, method: DefId, self_ty: Ty) -> StageResult<Option<Body>>;
    /// The items a map key's hashing and equality call (codegen.md
    /// §13.12): the `Eq` trait and `std.hash.hash_of`.
    fn map_key_items(&self) -> (DefId, DefId);
}

/// The A1 class `REF` as a type argument: a reserved canonical
/// placeholder, which never occurs in a concrete instance otherwise.
#[must_use]
pub fn class_ref(pool: &InternPool) -> Ty {
    pool.intern_ty(&TyData::Canon(0xF0))
}

#[must_use]
pub fn is_class_ref(pool: &InternPool, t: Ty) -> bool {
    pool.get(t) == TyData::Canon(0xF0)
}

/// How a call is lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// A direct call of a collected instance.
    Instance,
    /// A compiler lowering of a body-less std function, by intrinsic key.
    Intrinsic(String),
    /// A body-less method of a built-in family (`impl[N < Num] Display for
    /// N`), or a method of a sealed trait's compiler-supplied impl
    /// (`Inspectable.runtime_type`, trait-solver.md §3.9): the method's
    /// name at a concrete self type.
    Builtin { method: String, self_ty: Ty },
}

/// Where one call goes: the callee instance's key, its item, arguments and
/// result type under its own arguments (an erased result is cast back at
/// the caller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallTarget {
    pub key: Hash128,
    pub item: DefId,
    pub args: TyList,
    pub ret: Ty,
    pub kind: TargetKind,
}

/// A row's requirement keys in key order (codegen.md §12.4, §13.18): the
/// trait value types among `keys`, sorted by their content hash
/// (`layout::canon`) and each once, so a caller and its callee agree on
/// the order without comparing run-local type numbers. `Repo[User]` and
/// `Repo[Post]` are two keys.
pub fn key_order(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    keys: impl IntoIterator<Item = Ty>,
) -> Vec<Ty> {
    let mut memo = CanonMemo::default();
    let mut keyed: Vec<(Hash128, Ty)> = keys
        .into_iter()
        .filter(|k| matches!(pool.get(*k), TyData::TraitValue { .. }))
        .map(|k| (canon(pool, path_hash, &mut memo, k), k))
        .collect();
    keyed.sort_by_key(|p| p.0);
    keyed.dedup_by_key(|p| p.0);
    keyed.into_iter().map(|p| p.1).collect()
}

/// What collection recorded for one instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Call(CallTarget),
    /// An omitted argument's default body (codegen.md §13.13): its
    /// instance, and whether the body makes a call, so emission brackets
    /// it with the forbidden-context counter (§12.3).
    Default {
        call: CallTarget,
        bracket: bool,
    },
    /// A coercion to a trait value: its vtable's targets.
    VTable(VTable),
    /// A closure's code instance.
    Closure(Hash128),
    /// A `$.with`'s providers: per key, its vtable's targets.
    Withs(Vec<VTable>),
    /// A map operation on a key that is not inline
    /// (`layout::inline_map_key`): `hash_of` and `Eq.eq` at the key type
    /// (codegen.md §13.12).
    MapKey {
        hash: CallTarget,
        eq: CallTarget,
    },
}

/// The targets of one vtable at a concrete type (codegen.md §13.5,
/// §13.16): the trait, one target per method slot, and per direct
/// supertrait, in declared order, that supertrait's vtable at the same
/// type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VTable {
    pub trait_: DefId,
    pub slots: Vec<CallTarget>,
    pub parents: Vec<VTable>,
}

/// The output of `Collect` (codegen.md §11.3).
#[derive(Debug, Default)]
pub struct Collected {
    pub table: InstanceTable,
    /// Per instance: each instruction's target.
    pub calls: Vec<HashMap<u32, Target>>,
    /// Per instance: the hash of what its code reads besides its own TIR:
    /// callees' representation summaries and signatures (walking skeleton,
    /// SK-3) and the layouts of its types; each code key holds it.
    pub callee_reps: Vec<Hash128>,
    pub imports: BTreeSet<u32>,
    /// Data types the program builds or reads.
    pub data: BTreeSet<DefIdOrd>,
    /// The init bodies' instances, in initialization order.
    pub inits: Vec<Hash128>,
    /// The extra roots' instances, in the order the caller gave them.
    pub extra: Vec<Hash128>,
    /// The compiler-supplied methods' bodies (`ProgramEnv::supplied_body`)
    /// the program reaches, by `(trait method, self type)`.
    pub supplied: HashMap<(DefId, Ty), Arc<Body>>,
}

impl Collected {
    /// The TIR of an instance whose item has no body of its own: the
    /// compiler-supplied method's, at the instance's self type.
    #[must_use]
    pub fn supplied_body(&self, pool: &InternPool, item: DefId, args: TyList) -> Option<&Body> {
        let self_ty = pool.list_items(args).first().copied()?;
        self.supplied.get(&(item, self_ty)).map(AsRef::as_ref)
    }
}

pub use layout::InstanceTable;

/// A `DefId` ordered by its stable path hash, never by run ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefIdOrd(pub Hash128, pub u32);

impl DefIdOrd {
    #[must_use]
    pub fn def(self) -> DefId {
        DefId::from_raw(self.1)
    }
}

/// Substitutes an instance's arguments for its item's parameters: the
/// owner's (impl or trait) first, then the item's own. A projection whose
/// base becomes concrete resolves to the impl's binding (codegen.md §13).
#[must_use]
pub fn subst(pool: &InternPool, env: &dyn ProgramEnv, item: DefId, args: TyList, t: Ty) -> Ty {
    let item = env.generics_owner(item);
    let a = pool.list_items(args);
    let parent = env.parent(item);
    let n = parent.map_or(0, |p| p.1);
    let s = pool.subst(t, &|p: ParamRef| {
        if p.owner == item {
            a.get(n + p.index as usize).copied()
        } else if parent.is_some_and(|(o, _)| o == p.owner) {
            a.get(p.index as usize).copied()
        } else {
            None
        }
    });
    if pool.has_assoc(s) {
        hd_types::solver::normalize_concrete(pool.types(), env.impls(), s)
    } else {
        s
    }
}

/// The printable name of `t`'s runtime identity (trait.typeid.name.*),
/// which a `TypeId` holds as its key. Equality of `TypeId`s compares keys
/// (`lib/std/inspect.hd`), so the name spells exactly what runtime
/// identity compares (trait.identity.*): each declaration by its qualified
/// name, applied to its arguments; an inner `mut`, never the outer one;
/// `T?` for `Option[T]`. Types are concrete here, aliases expanded.
#[must_use]
pub fn type_name(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> String {
    let t = match pool.get(t) {
        TyData::Mut(x) => x,
        _ => t,
    };
    let mut out = String::new();
    name_into(pool, env, t, &mut out, usize::MAX);
    out
}

/// The longest type a diagnostic prints in full, in bytes; a longer one is
/// cut there and ends in `...`. A type that nests 33 deep can have 2^33
/// leaves (`instantiation-too-deep`, codegen.md §13.4).
pub const SHOWN_TYPE_MAX: usize = 120;

/// `t` as `type_name` spells it, cut at `SHOWN_TYPE_MAX` bytes: the walk
/// stops there, so its cost is bounded too, however large the type's tree.
#[must_use]
pub fn shown_type(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> String {
    let mut out = String::new();
    name_into(pool, env, t, &mut out, SHOWN_TYPE_MAX);
    if out.len() > SHOWN_TYPE_MAX {
        let mut cut = SHOWN_TYPE_MAX;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("...");
    }
    out
}

/// Spells `t` into `out`, and stops once `out` is longer than `limit`.
fn name_into(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, out: &mut String, limit: usize) {
    if out.len() > limit {
        return;
    }
    let list = |l: &[Ty], sep: &str, out: &mut String| {
        for (k, x) in l.iter().enumerate() {
            if out.len() > limit {
                return;
            }
            if k > 0 {
                out.push_str(sep);
            }
            name_into(pool, env, *x, out, limit);
        }
    };
    match pool.get(t) {
        TyData::Prim(p) => out.push_str(p.name()),
        TyData::Never => out.push_str("never"),
        TyData::Adt { def, args } => {
            out.push_str(&env.type_name(def));
            if !pool.list_items(args).is_empty() {
                out.push('[');
                list(pool.list_items(args), ", ", out);
                out.push(']');
            }
        }
        // A trait value type is its trait applied to its arguments
        // (trait.identity.trait-value), printed without `dyn`.
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => {
            out.push_str(&env.type_name(def));
            let args = pool.list_items(args);
            if !args.is_empty() || !bindings.is_empty() {
                out.push('[');
                list(args, ", ", out);
                for (k, (a, b)) in bindings.iter().enumerate() {
                    if k > 0 || !args.is_empty() {
                        out.push_str(", ");
                    }
                    let n = env.type_name(*a);
                    out.push_str(n.rsplit('.').next().unwrap_or(&n));
                    out.push_str(" = ");
                    name_into(pool, env, *b, out, limit);
                }
                out.push(']');
            }
        }
        TyData::Tuple { elems, rest } => {
            out.push('(');
            let elems = pool.list_items(elems);
            list(elems, ", ", out);
            if let Some(r) = rest {
                if !elems.is_empty() {
                    out.push_str(", ");
                }
                name_into(pool, env, r, out, limit);
                out.push_str("...");
            }
            out.push(')');
        }
        TyData::Option(i) => {
            name_into(pool, env, i, out, limit);
            out.push('?');
        }
        TyData::Mut(i) => {
            out.push_str("mut ");
            name_into(pool, env, i, out, limit);
        }
        // Not inspectable (trait.inspectable.not.*): never recorded, but a
        // diagnostic spells them.
        TyData::Fn {
            params,
            result,
            suspends,
            ..
        } => {
            out.push_str(if suspends { "fn!(" } else { "fn(" });
            list(pool.list_items(params), ", ", out);
            out.push_str(") -> ");
            name_into(pool, env, result, out, limit);
        }
        TyData::Row(r) => {
            out.push_str("$(");
            list(&pool.row_data(r).keys, " + ", out);
            out.push(')');
        }
        TyData::Assoc { assoc, self_ty, .. } => {
            out.push('<');
            name_into(pool, env, self_ty, out, limit);
            let n = env.type_name(assoc);
            out.push_str(">.");
            out.push_str(n.rsplit('.').next().unwrap_or(&n));
        }
        TyData::Poison | TyData::Param(_) | TyData::Infer(_) | TyData::Canon(_) => {
            out.push_str(&pool.display(t));
        }
    }
}

fn key_args(pool: &InternPool, args: TyList) -> Vec<KeyArg> {
    pool.list_items(args)
        .iter()
        .copied()
        .map(|t| {
            if is_class_ref(pool, t) {
                KeyArg::Class(A1Class::Ref)
            } else {
                KeyArg::Canon(t)
            }
        })
        .collect()
}

/// Matches an impl head's `pattern` (over the parameters of `owner`)
/// against a concrete type, filling `out`.
fn unify(pool: &InternPool, owner: DefId, pattern: Ty, t: Ty, out: &mut Vec<Option<Ty>>) {
    let strip = |x: Ty| match pool.get(x) {
        TyData::Mut(i) => i,
        _ => x,
    };
    let (pattern, t) = (strip(pattern), strip(t));
    match (pool.get(pattern), pool.get(t)) {
        (TyData::Param(p), _) if p.owner == owner => {
            let i = p.index as usize;
            if out.len() <= i {
                out.resize(i + 1, None);
            }
            out[i].get_or_insert(t);
        }
        (TyData::Adt { args: a, .. }, TyData::Adt { args: b, .. })
        | (TyData::TraitValue { args: a, .. }, TyData::TraitValue { args: b, .. })
        | (TyData::Tuple { elems: a, .. }, TyData::Tuple { elems: b, .. }) => {
            for (x, y) in pool
                .list_items(a)
                .iter()
                .copied()
                .zip(pool.list_items(b).iter().copied())
            {
                unify(pool, owner, x, y, out);
            }
        }
        (TyData::Option(x), TyData::Option(y)) => unify(pool, owner, x, y, out),
        (
            TyData::Fn {
                params: a,
                result: r,
                ..
            },
            TyData::Fn {
                params: b,
                result: s,
                ..
            },
        ) => {
            for (x, y) in pool
                .list_items(a)
                .iter()
                .copied()
                .zip(pool.list_items(b).iter().copied())
            {
                unify(pool, owner, x, y, out);
            }
            unify(pool, owner, r, s, out);
        }
        _ => {}
    }
}

/// The items a type names: data types, enums and traits (a projection's
/// trait included). `seen` holds the types already walked, so a type shared
/// by its parts is walked once however large its tree.
fn items_in(pool: &InternPool, t: Ty, out: &mut Vec<DefId>, seen: &mut HashSet<Ty>) {
    if !seen.insert(t) {
        return;
    }
    let list = |l: TyList, out: &mut Vec<DefId>, seen: &mut HashSet<Ty>| {
        for &x in pool.list_items(l) {
            items_in(pool, x, out, seen);
        }
    };
    match pool.get(t) {
        TyData::Adt { def, args } => {
            out.push(def);
            list(args, out, seen);
        }
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => {
            out.push(def);
            list(args, out, seen);
            for (_, b) in bindings {
                items_in(pool, b, out, seen);
            }
        }
        TyData::Assoc {
            trait_,
            self_ty,
            args,
            ..
        } => {
            out.push(trait_);
            items_in(pool, self_ty, out, seen);
            list(args, out, seen);
        }
        TyData::Tuple { elems, rest } => {
            list(elems, out, seen);
            if let Some(r) = rest {
                items_in(pool, r, out, seen);
            }
        }
        TyData::Option(i) | TyData::Mut(i) => items_in(pool, i, out, seen),
        TyData::Fn { params, result, .. } => {
            list(params, out, seen);
            items_in(pool, result, out, seen);
        }
        _ => {}
    }
}

/// An item's declared types that layouts read: a data type's fields, an
/// enum's payloads, a trait's method parameters and results, as declared.
fn declared_types(env: &dyn ProgramEnv, def: DefId) -> Vec<Ty> {
    if let Some(fs) = env.data_fields(def) {
        return fs;
    }
    if let Some(vs) = env.enum_declared(def) {
        return vs.into_iter().flatten().collect();
    }
    let mut out = Vec::new();
    for m in env.trait_methods(def) {
        out.extend(env.params(m).unwrap_or_default());
        out.extend(env.ret(m));
    }
    out
}

/// `layout_hash(T)` (codegen.md §13.8): the hash of `canon(T)` and of the
/// declarations of every item `T` names, and of every item their declared
/// types name, however far. A type's layout reads only those, at `T`'s
/// arguments, so the hash covers a whole recursion group and ends on any
/// recursion, a growing one (`E[List[T]]` in `E[T]`) included, since it
/// walks declarations, of which a program has finitely many.
fn layout_hash(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    canons: &mut CanonMemo,
    memo: &mut HashMap<DefId, Hash128>,
) -> Hash128 {
    let ph = |d: DefId| env.path_hash(d);
    let mut h = StableHasher::new("layout-hash");
    h.hash(canon(pool, &ph, canons, t));
    let mut items = Vec::new();
    items_in(pool, t, &mut items, &mut HashSet::new());
    let mut hashes: Vec<Hash128> = items
        .into_iter()
        .map(|d| declarations_hash(pool, env, d, canons, memo))
        .collect();
    hashes.sort_unstable();
    hashes.dedup();
    h.u32(u32::try_from(hashes.len()).expect("items"));
    for x in hashes {
        h.hash(x);
    }
    h.finish()
}

/// The hash of an item's declaration and of every declaration it reaches
/// through its declared types, memoized per item for the build. Items that
/// reach each other share one hash, over their own declarations and the
/// hashes of the items they reach outside themselves: Tarjan's algorithm
/// with an explicit stack.
fn declarations_hash(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    root: DefId,
    canons: &mut CanonMemo,
    memo: &mut HashMap<DefId, Hash128>,
) -> Hash128 {
    if let Some(h) = memo.get(&root) {
        return *h;
    }
    let ph = |d: DefId| env.path_hash(d);
    // An item's own hash and the items its declared types name.
    let read = |d: DefId, canons: &mut CanonMemo| {
        let mut h = StableHasher::new("declaration");
        h.hash(ph(d));
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let tys = declared_types(env, d);
        h.u32(u32::try_from(tys.len()).expect("types"));
        for t in tys {
            h.hash(canon(pool, &ph, canons, t));
            items_in(pool, t, &mut names, &mut seen);
        }
        names.sort_unstable_by_key(|n| n.raw());
        names.dedup();
        (h.finish(), names)
    };
    // Per visited item, by visit number: the item, its own hash, the items
    // it names, its low link and whether it is on `stack`.
    let (own, names) = read(root, canons);
    let mut index: HashMap<DefId, usize> = HashMap::from([(root, 0)]);
    let mut nodes = vec![(root, own, names)];
    let (mut low, mut on, mut stack) = (vec![0], vec![true], vec![0]);
    let mut call: Vec<(usize, usize)> = vec![(0, 0)];
    while let Some(&mut (v, ref mut k)) = call.last_mut() {
        if let Some(&w) = nodes[v].2.get(*k) {
            *k += 1;
            if memo.contains_key(&w) {
                continue;
            }
            match index.get(&w) {
                None => {
                    let i = nodes.len();
                    index.insert(w, i);
                    let (own, names) = read(w, canons);
                    nodes.push((w, own, names));
                    low.push(i);
                    on.push(true);
                    stack.push(i);
                    call.push((i, 0));
                }
                Some(&wi) if on[wi] => low[v] = low[v].min(wi),
                Some(_) => {}
            }
            continue;
        }
        call.pop();
        if let Some(&(p, _)) = call.last() {
            low[p] = low[p].min(low[v]);
        }
        if low[v] != v {
            continue;
        }
        let at = stack.iter().rposition(|&x| x == v).expect("on the stack");
        let members = stack.split_off(at);
        let inside: std::collections::HashSet<DefId> =
            members.iter().map(|&m| nodes[m].0).collect();
        let mut owns: Vec<Hash128> = Vec::new();
        let mut outside: Vec<Hash128> = Vec::new();
        for &m in &members {
            on[m] = false;
            owns.push(nodes[m].1);
            for n in &nodes[m].2 {
                if !inside.contains(n) {
                    outside.push(memo[n]);
                }
            }
        }
        owns.sort_unstable();
        outside.sort_unstable();
        outside.dedup();
        let mut h = StableHasher::new("declarations");
        h.u32(u32::try_from(owns.len()).expect("items"));
        for o in owns {
            h.hash(o);
        }
        h.u32(u32::try_from(outside.len()).expect("items"));
        for o in outside {
            h.hash(o);
        }
        let hash = h.finish();
        for m in members {
            memo.insert(nodes[m].0, hash);
        }
    }
    memo[&root]
}

type SelectKey = (DefId, Ty, TyList, Option<DefId>);

/// What selection picked (trait-solver.md §8.3): an impl and its
/// arguments, or the compiler's impl of a sealed trait (§3.9), whose
/// methods are lowered per type.
#[derive(Clone)]
enum Picked {
    Impl(DefId, Vec<Ty>),
    Builtin,
}

struct Cx<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    solver: &'a dyn Solver,
    /// Selection's context: no parameters in scope, one memo for the build.
    penv: ParamEnv,
    memo: BodyMemo,
    /// The build's selections, by `(trait, self type, trait arguments,
    /// chosen impl)`: the impl and its arguments (trait-solver.md §8.3).
    selected: HashMap<SelectKey, Picked>,
    /// Per item: which of its own type parameters need their exact
    /// representation (A1's representation summary, §13.2).
    exact: HashMap<DefId, Vec<bool>>,
    /// `canon` per type, for the build: instance keys, layout hashes and
    /// call targets read each type's digest once.
    canons: CanonMemo,
    /// `declarations_hash` per item, for the build.
    layouts: HashMap<DefId, Hash128>,
    /// Nesting depth per type, so a type shared by its parts is walked
    /// once however large its tree (codegen.md §13.4).
    depths: HashMap<Ty, u32>,
    /// The instruction being scanned: its item and syntax node, where an
    /// instance request over a limit is reported.
    at: Option<(DefId, NodeIdx)>,
    /// Set when collection stops at a limit.
    too_deep: Option<TooDeep>,
    out: Collected,
    work: Vec<InstId>,
}

/// Collection stopped at an instantiation limit (codegen.md §13.4):
/// `instantiation-too-deep`, at the call that would exceed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooDeep {
    /// The item whose body holds that call, and the call's syntax node;
    /// `None` when the request has no instruction (a root, an adapter).
    pub at: Option<(DefId, NodeIdx)>,
    pub message: String,
}

/// The type of a value operand: an instruction's, or a constant's when
/// the word has `Ref::CONST_BIT`.
fn value_ty(body: &Body, word: u32) -> Ty {
    let c = hd_tir::ir::Ref::CONST_BIT;
    let t = if word & c == 0 {
        body.ty.get(word as usize).copied()
    } else {
        body.consts.get((word & !c) as usize).map(|k| k.0)
    };
    t.unwrap_or(Ty::POISON)
}

fn err<T>(what: &str) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Collect, what))
}

impl Cx<'_> {
    fn key(&mut self, item: DefId, sub: Sub, args: TyList) -> Hash128 {
        let env = self.env;
        let ph = |d: DefId| env.path_hash(d);
        let args = key_args(self.pool, args);
        instance_key(self.pool, &ph, &mut self.canons, item, sub, &args)
    }

    /// Pushes an instance that `parent` requests; returns its key. A new
    /// instance over a limit of codegen.md §13.4 stops collection.
    fn push(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        parent: InstId,
    ) -> StageResult<Hash128> {
        let table = &self.out.table;
        if let Some(id) = table.get(item, sub, args) {
            return Ok(table.key[id.idx()]);
        }
        let chain = if parent == InstId::NONE {
            0
        } else {
            table.chain_len(parent) + 1
        };
        let pool = self.pool;
        let depth = pool
            .list_items(args)
            .iter()
            .map(|&t| self.depth_of(t))
            .max()
            .unwrap_or(0);
        if depth > u32::from(MAX_DEPTH) || chain > MAX_CHAIN {
            return Err(self.stop_too_deep(item, args, parent, depth, chain));
        }
        let key = self.key(item, sub, args);
        let depth = u8::try_from(depth).expect("within MAX_DEPTH");
        let (id, _) = self.out.table.push(item, sub, args, depth, parent, key);
        self.work.push(id);
        Ok(key)
    }

    /// The nesting depth of `t`: 1 for a type with no type parts, else one
    /// more than its deepest part. A row counts its keys as parts.
    fn depth_of(&mut self, t: Ty) -> u32 {
        if let Some(&d) = self.depths.get(&t) {
            return d;
        }
        let pool = self.pool;
        let mut parts: Vec<Ty> = Vec::new();
        let row_keys = |r: hd_types::RowId, parts: &mut Vec<Ty>| {
            parts.extend(pool.row_data(r).keys);
        };
        match pool.get(t) {
            TyData::Adt { args, .. } => parts.extend(pool.list_items(args)),
            TyData::Tuple { elems, rest } => {
                parts.extend(pool.list_items(elems));
                parts.extend(rest);
            }
            TyData::Option(i) | TyData::Mut(i) => parts.push(i),
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                parts.extend(pool.list_items(params));
                parts.push(result);
                row_keys(row, &mut parts);
            }
            TyData::TraitValue { args, bindings, .. } => {
                parts.extend(pool.list_items(args));
                parts.extend(bindings.into_iter().map(|(_, b)| b));
            }
            TyData::Assoc { self_ty, args, .. } => {
                parts.push(self_ty);
                parts.extend(pool.list_items(args));
            }
            TyData::Row(r) => row_keys(r, &mut parts),
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Param(_)
            | TyData::Infer(_)
            | TyData::Canon(_) => {}
        }
        let d = 1 + parts
            .into_iter()
            .map(|p| self.depth_of(p))
            .max()
            .unwrap_or(0);
        self.depths.insert(t, d);
        d
    }

    /// Records the stop at a limit (codegen.md §13.4): the request of
    /// `item` at `args` from `parent`, at the instruction being scanned,
    /// with the first three instances of the chain and the last.
    fn stop_too_deep(
        &mut self,
        item: DefId,
        args: TyList,
        parent: InstId,
        depth: u32,
        chain: u32,
    ) -> NotImplemented {
        let table = &self.out.table;
        let mut ids = Vec::new();
        let mut p = parent;
        while p != InstId::NONE {
            ids.push(p);
            p = table.parent[p.idx()];
        }
        ids.reverse();
        // Each type argument is cut at `SHOWN_TYPE_MAX`: the last
        // instance's can have 2^33 leaves.
        let name = |item: DefId, args: TyList| {
            let args = self.pool.list_items(args);
            if args.is_empty() {
                format!("`{}`", self.env.describe(item))
            } else {
                let shown: Vec<String> = args
                    .iter()
                    .map(|&t| shown_type(self.pool, self.env, t))
                    .collect();
                format!("`{}[{}]`", self.env.describe(item), shown.join(", "))
            }
        };
        let mut shown: Vec<String> = ids
            .iter()
            .take(3)
            .map(|id| name(table.item[id.idx()], table.args[id.idx()]))
            .collect();
        if ids.len() > 3 {
            shown.push("...".to_owned());
        }
        shown.push(name(item, args));
        let last = format!("`{}`", self.env.describe(item));
        let why = if depth > u32::from(MAX_DEPTH) {
            format!("its type arguments nest {depth} deep, past the limit of {MAX_DEPTH}")
        } else {
            format!("its request chain is {chain} long, past the limit of {MAX_CHAIN}")
        };
        let message = format!(
            "instantiating {last} does not end: {why}; the chain: {}",
            shown.join(" -> ")
        );
        self.too_deep = Some(TooDeep {
            at: self.at,
            message,
        });
        NotImplemented::new(Stage::Collect, "instantiation-too-deep")
    }

    /// A1's representation summary of `def` (codegen.md §13.2): per own
    /// type parameter, whether a type of its signature or body holds the
    /// parameter where the layout depends on its exact representation.
    /// Only a value of the parameter itself, `mut`, an optional, and the
    /// erased storage of a `List`, `Map` or enum payload keep it movable:
    /// a data type's fields, a tuple, a function type, a trait value or a
    /// projection that mentions it change their Wasm type with it. A
    /// requirement key that mentions it (the item's own row, a function
    /// type's row, a `$.with` key) changes which provider the instance
    /// passes and reads, so it is exact too (codegen.md §13.18).
    fn exact_params(&mut self, def: DefId) -> Vec<bool> {
        if let Some(e) = self.exact.get(&def) {
            return e.clone();
        }
        let n = self.env.bounded(def).map_or(0, |b| b.len());
        let mut out = vec![false; n];
        let pool = self.pool;
        let env = self.env;
        let mut types: Vec<Ty> = env.params(def).unwrap_or_default();
        types.extend(env.ret(def));
        // Unsubstituted: the keys over the item's own parameters.
        types.extend(env.row_keys(def, TyList::EMPTY));
        if let Some(b) = env.body(def) {
            types.extend(b.ty.iter().copied());
            types.extend(b.local_ty.iter().copied());
            for i in 0..b.len() {
                if b.tags[i] == Tag::With {
                    types.extend(b.record(b.data[i][0]).chunks(2).map(|c| Ty(c[0])));
                    continue;
                }
                if !matches!(b.tags[i], Tag::Call | Tag::Await) {
                    continue;
                }
                match Callee::from_words(b.record(b.data[i][0])) {
                    Some(Callee::Item { targs, .. }) => {
                        types.extend(pool.list_items(targs).iter().copied());
                    }
                    Some(Callee::TraitMethod { self_ty, targs, .. }) => {
                        types.push(self_ty);
                        types.extend(pool.list_items(targs).iter().copied());
                    }
                    None => {}
                }
            }
        }
        for t in types {
            pinned(pool, env, def, t, false, &mut out);
        }
        self.exact.insert(def, out.clone());
        out
    }

    /// A1 (codegen.md §13.2): an unbounded, move-only argument of a
    /// reference layout is keyed and laid out as the class `REF`.
    fn classify(&mut self, def: DefId, args: &[Ty], own_from: usize) -> StageResult<TyList> {
        let bounded = self.env.bounded(def).unwrap_or_default();
        let pinned = self.exact_params(def);
        let mut a = Vec::new();
        for (k, &t) in args.iter().enumerate() {
            // A row argument is part of the instance (codegen.md §12.4:
            // its keys are the instance's provider parameters).
            let exact = k < own_from
                || bounded.get(k - own_from).copied().unwrap_or(true)
                || pinned.get(k - own_from).copied().unwrap_or(false)
                || is_class_ref(self.pool, t)
                || matches!(self.pool.get(t), TyData::Row(_))
                || self.env.body(def).is_none();
            a.push(
                if !exact && a1_class(self.pool, self.env, t)? == A1Class::Ref {
                    class_ref(self.pool)
                } else {
                    t
                },
            );
        }
        Ok(self.pool.list(&a))
    }

    /// The target of a call of `def` at full arguments `args`.
    fn target(&mut self, def: DefId, args: TyList, parent: InstId) -> StageResult<CallTarget> {
        let ret = self.env.ret(def).unwrap_or(Ty::VOID);
        // A std function whose body the compiler supplies, though its
        // source has a placeholder body (`race!`'s frame, the categorized
        // `panic`, `std.rt`'s `entry_write`, the string byte primitives).
        if let Some(key) = self.env.intrinsic(def).filter(|k| {
            matches!(
                k.as_str(),
                "task_race_frame"
                    | "panic"
                    | "entry_write"
                    | "bytes_len"
                    | "bytes_at"
                    | "bytes_slice"
                    | "bytes_concat"
                    | "string_from_bytes"
                    | "char_scalar"
                    | "char_from_scalar"
            )
        }) {
            return Ok(CallTarget {
                key: Hash128(0),
                item: def,
                args,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Intrinsic(key),
            });
        }
        if self.env.body(def).is_none() {
            let Some(key) = self.env.intrinsic(def) else {
                return Err(NotImplemented::new(
                    Stage::Collect,
                    format!(
                        "a call of the body-less `{}`, which is not an intrinsic",
                        self.env.describe(def)
                    ),
                ));
            };
            return Ok(CallTarget {
                key: Hash128(0),
                item: def,
                args,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Intrinsic(key),
            });
        }
        let own_from = self.env.parent(def).map_or(0, |p| p.1);
        let args = self.classify(def, self.pool.list_items(args), own_from)?;
        let key = self.push(def, Sub::Body(0), args, parent)?;
        Ok(CallTarget {
            key,
            item: def,
            args,
            ret: subst(self.pool, self.env, def, args, ret),
            kind: TargetKind::Instance,
        })
    }

    /// Selects the implementation of `trait_` for `self_ty` (codegen.md
    /// §13.2 `select`): the impl and its arguments.
    fn select(
        &mut self,
        trait_: DefId,
        self_ty: Ty,
        trait_args: TyList,
        choice: Option<DefId>,
    ) -> StageResult<Picked> {
        let key = (trait_, self_ty, trait_args, choice);
        if let Some(hit) = self.selected.get(&key) {
            return Ok(hit.clone());
        }
        let env = self.env;
        let view = env.impls();
        let at = if let Some(d) = choice {
            let Some(at) = env.impl_row(d) else {
                return err("a chosen impl outside the impl tables");
            };
            at
        } else {
            let tref = ConcreteTraitRef(TraitRef {
                trait_,
                self_ty,
                args: trait_args,
            });
            let (universe, global) = env.solving();
            let mut cx = SolveCx {
                pool: self.pool.types(),
                env: &self.penv,
                universe,
                impls: view,
                decls: env.decls(),
                body_memo: &mut self.memo,
                global,
            };
            let picked = self.solver.select(&mut cx, tref).map_err(|mut e| {
                e.what = format!(
                    "{} (`{}` at `{}`)",
                    e.what,
                    env.describe(trait_),
                    self.pool.display(self_ty)
                );
                e
            })?;
            match picked {
                Selection::Impl { impl_row, .. } => impl_row,
                Selection::Builtin(_) => {
                    self.selected.insert(key, Picked::Builtin);
                    return Ok(Picked::Builtin);
                }
            }
        };
        let Some(t) = view.table(at.module) else {
            return err("a selection outside the impl tables");
        };
        let row = at.row as usize;
        let impl_ = t.def[row];
        let Some((head, targs, n)) = env.impl_head(impl_) else {
            return err("a selected impl without a head");
        };
        let mut out = vec![None; n];
        unify(self.pool, impl_, head, self_ty, &mut out);
        for (x, y) in self
            .pool
            .list_items(targs)
            .iter()
            .copied()
            .zip(self.pool.list_items(trait_args).iter().copied())
        {
            unify(self.pool, impl_, x, y, &mut out);
        }
        // Parameters that a bound's binding fixes (`Bind` steps).
        if out.iter().any(Option::is_none) {
            out.resize(out.len().max(n), None);
            hd_types::solver::apply_binds(self.pool.types(), view, t, row, &mut out);
        }
        let mut args = Vec::new();
        for a in out.into_iter().take(n) {
            let Some(a) = a else {
                return err("an impl parameter that its head does not fix (a `Bind` step)");
            };
            args.push(a);
        }
        let picked = Picked::Impl(impl_, args);
        self.selected.insert(key, picked.clone());
        Ok(picked)
    }

    /// A call of a compiler-supplied impl's method whose body the compiler
    /// generates per self type (`ProgramEnv::supplied_body`): an instance
    /// of the trait method at `[self_ty, targs...]`, keyed and cached like
    /// any instance. `None` when emission lowers the method itself.
    fn supplied_target(
        &mut self,
        method: DefId,
        self_ty: Ty,
        targs: &[Ty],
        parent: InstId,
    ) -> StageResult<Option<CallTarget>> {
        let key = (method, self_ty);
        if !self.out.supplied.contains_key(&key) {
            let Some(body) = self.env.supplied_body(method, self_ty)? else {
                return Ok(None);
            };
            self.out.supplied.insert(key, Arc::new(body));
        }
        let args = self.pool.list(&[&[self_ty], targs].concat());
        let key = self.push(method, Sub::Body(0), args, parent)?;
        Ok(Some(CallTarget {
            key,
            item: method,
            args,
            ret: subst(
                self.pool,
                self.env,
                method,
                args,
                self.env.ret(method).unwrap_or(Ty::VOID),
            ),
            kind: TargetKind::Instance,
        }))
    }

    /// A call of a method the compiler lowers per self type: a sealed
    /// trait's (§3.9), or a body-less method of a built-in family.
    fn builtin_target(&self, method: DefId, self_ty: Ty, targs: &[Ty]) -> CallTarget {
        CallTarget {
            key: Hash128(0),
            item: method,
            args: TyList::EMPTY,
            ret: subst(
                self.pool,
                self.env,
                method,
                self.pool.list(&[&[self_ty], targs].concat()),
                self.env.ret(method).unwrap_or(Ty::VOID),
            ),
            kind: TargetKind::Builtin {
                method: String::new(),
                self_ty,
            },
        }
    }

    /// The target of a trait method at a concrete self type: the impl's
    /// method, the trait's default body, or a built-in lowering.
    fn method_target(
        &mut self,
        trait_: DefId,
        method: DefId,
        self_ty: Ty,
        targs: &[Ty],
        choice: Option<DefId>,
        parent: InstId,
    ) -> StageResult<CallTarget> {
        let n_trait = self.env.trait_arity(trait_);
        let trait_args = self.pool.list(&targs[..n_trait.min(targs.len())]);
        let method_args = &targs[n_trait.min(targs.len())..];
        let (impl_, impl_args) = match self.select(trait_, self_ty, trait_args, choice)? {
            Picked::Impl(d, a) => (d, a),
            Picked::Builtin => {
                if let Some(t) = self.supplied_target(method, self_ty, targs, parent)? {
                    return Ok(t);
                }
                return Ok(self.builtin_target(method, self_ty, targs));
            }
        };
        match self.env.impl_method(impl_, method) {
            Some(m) if self.env.body(m).is_some() => {
                let mut all = impl_args;
                all.extend_from_slice(method_args);
                self.target(m, self.pool.list(&all), parent)
            }
            Some(m) => Ok(CallTarget {
                item: m,
                ..self.builtin_target(method, self_ty, targs)
            }),
            None => {
                let mut all = vec![self_ty];
                all.extend_from_slice(targs);
                self.target(method, self.pool.list(&all), parent)
            }
        }
    }

    /// The vtable of `trait_[trait_args]` at the concrete type `from`
    /// (codegen.md §13.5, §13.16): a target per method slot, then each
    /// direct supertrait's vtable at the same type, its arguments
    /// substituted with `Self` as `from`. Each slot's key joins `reps`.
    fn vtable(
        &mut self,
        trait_: DefId,
        trait_args: TyList,
        from: Ty,
        parent: InstId,
        reps: &mut StableHasher,
    ) -> StageResult<VTable> {
        let (pool, env) = (self.pool, self.env);
        let targs = pool.list_items(trait_args);
        let mut slots = Vec::new();
        for m in env.trait_methods(trait_) {
            let t = self.method_target(trait_, m, from, targs, None, parent)?;
            reps.hash(t.key);
            slots.push(t);
        }
        let mut full = vec![from];
        full.extend(targs.iter().copied());
        let full = pool.list(&full);
        let mut parents = Vec::new();
        for sup in env.decls().supertraits(trait_).to_vec() {
            let TyData::TraitValue { def, args, .. } =
                pool.get(subst(pool, env, trait_, full, sup))
            else {
                return err("a supertrait that is not a trait");
            };
            parents.push(self.vtable(def, args, from, parent, reps)?);
        }
        Ok(VTable {
            trait_,
            slots,
            parents,
        })
    }

    /// What a code entry reads of a call's target: the callee's identity,
    /// instance and signature (walking skeleton, SK-3).
    fn hash_target(&mut self, t: &CallTarget, reps: &mut StableHasher) {
        let (pool, env) = (self.pool, self.env);
        let memo = &mut self.canons;
        reps.hash(env.path_hash(t.item));
        reps.hash(t.key);
        let ph = |d: DefId| env.path_hash(d);
        reps.hash(canon(pool, &ph, memo, t.ret));
        for p in env.params(t.item).unwrap_or_default() {
            reps.hash(canon(pool, &ph, memo, subst(pool, env, t.item, t.args, p)));
        }
        for b in env.bounded(t.item).unwrap_or_default() {
            reps.u8(u8::from(b));
        }
        for k in env.row_keys(t.item, t.args) {
            reps.hash(canon(pool, &ph, memo, k));
        }
    }

    /// A function reference's adapter (codegen.md §13.11): its one call,
    /// of the item at the instance's arguments, recorded as instruction
    /// 0. A trait member's arguments are `[Self, trait args..., own...]`,
    /// and its implementation is selected at that `Self` (the reference
    /// carries no evidence choice).
    fn scan_adapter(&mut self, id: InstId) -> StageResult<()> {
        let (item, args) = (self.out.table.item[id.idx()], self.out.table.args[id.idx()]);
        // An adapter has no TIR, so no instruction to point at.
        self.at = None;
        let (pool, env) = (self.pool, self.env);
        let mut reps = StableHasher::new("callee-reps");
        let s = |t: Ty| subst(pool, env, item, args, t);
        let mut types: Vec<Ty> = env.params(item).unwrap_or_default();
        types.extend(env.ret(item));
        for t in types {
            let t = s(t);
            reps.hash(layout_hash(
                pool,
                env,
                t,
                &mut self.canons,
                &mut self.layouts,
            ));
            note_data(pool, env, t, &mut self.out.data);
        }
        reps.u8(u8::from(env.suspends(item)));
        let t = match env.parent(item) {
            Some((trait_, _)) if env.impl_head(trait_).is_none() => {
                let all = pool.list_items(args);
                let Some((&self_ty, targs)) = all.split_first() else {
                    return err("a trait member reference without `Self`");
                };
                if matches!(pool.get(self_ty), TyData::TraitValue { .. }) {
                    return err("a trait member reference whose `Self` is a trait value");
                }
                self.method_target(trait_, item, self_ty, targs, None, id)?
            }
            _ => self.target(item, args, id)?,
        };
        self.hash_target(&t, &mut reps);
        Sub::Adapter.hash_into(&mut reps);
        if self.out.calls.len() <= id.idx() {
            self.out.calls.resize_with(id.idx() + 1, HashMap::new);
            self.out.callee_reps.resize(id.idx() + 1, Hash128(0));
        }
        self.out.calls[id.idx()] = HashMap::from([(0, Target::Call(t))]);
        self.out.callee_reps[id.idx()] = reps.finish();
        Ok(())
    }

    /// The key operations of a map of type `map` (codegen.md §13.12):
    /// `None` for an inline key, else `hash_of` and `Eq.eq` at the key
    /// type, selected here so the TIR carries only the key type.
    fn map_key(
        &mut self,
        map: Ty,
        parent: InstId,
        reps: &mut StableHasher,
    ) -> StageResult<Option<Target>> {
        let pool = self.pool;
        let map = match pool.get(map) {
            TyData::Mut(x) => x,
            _ => map,
        };
        let TyData::Adt { args, .. } = pool.get(map) else {
            return err("a map operation on a non-map type");
        };
        let Some(&key) = pool.list_items(args).first() else {
            return err("a map type without a key");
        };
        if layout::inline_map_key(pool, key) {
            return Ok(None);
        }
        let (eq_trait, hash_of) = self.env.map_key_items();
        let Some(&eq_method) = self.env.trait_methods(eq_trait).first() else {
            return err("an `Eq` trait without its method");
        };
        let eq = self.method_target(eq_trait, eq_method, key, &[], None, parent)?;
        let hash = self.target(hash_of, pool.list(&[key]), parent)?;
        self.hash_target(&eq, reps);
        self.hash_target(&hash, reps);
        Ok(Some(Target::MapKey { hash, eq }))
    }

    fn scan(&mut self, id: InstId) -> StageResult<()> {
        let (item, sub, args) = (
            self.out.table.item[id.idx()],
            self.out.table.sub[id.idx()],
            self.out.table.args[id.idx()],
        );
        match sub {
            Sub::Adapter => return self.scan_adapter(id),
            Sub::Body(_) => {}
        }
        let pool = self.pool;
        let env = self.env;
        // A compiler-supplied method's instance has no item body: its
        // TIR was generated at its self type (`supplied_target`).
        let supplied = match env.body(item) {
            Some(_) => None,
            None => pool
                .list_items(args)
                .first()
                .and_then(|t| self.out.supplied.get(&(item, *t)))
                .cloned(),
        };
        let Some(body) = env.body(item).or(supplied.as_deref()) else {
            return err("an instance whose item has no TIR");
        };
        let mut calls = HashMap::new();
        let mut reps = StableHasher::new("callee-reps");
        let mut seen = std::collections::HashSet::new();
        let s = |t: Ty| subst(pool, env, item, args, t);
        for i in 0..body.len() {
            let ty = s(body.ty[i]);
            if seen.insert(ty) {
                reps.hash(layout_hash(
                    pool,
                    env,
                    ty,
                    &mut self.canons,
                    &mut self.layouts,
                ));
            }
            note_data(pool, env, ty, &mut self.out.data);
            // A generated body's nodes are in no source file.
            self.at = body
                .syn
                .get(i)
                .filter(|n| supplied.is_none() && **n != NodeIdx::NONE)
                .map(|&n| (item, n));
            let ix = u32::try_from(i).expect("insts");
            let [a, b] = body.data[i];
            match body.tags[i] {
                Tag::CallHost => {
                    self.out.imports.insert(a);
                }
                Tag::Call | Tag::Await => {
                    let Some(c) = Callee::from_words(body.record(a)) else {
                        return err("a malformed callee record");
                    };
                    let t = match c {
                        Callee::Item { def, targs } => {
                            let targs: Vec<Ty> =
                                pool.list_items(targs).iter().copied().map(s).collect();
                            self.target(def, pool.list(&targs), id)?
                        }
                        Callee::TraitMethod {
                            trait_,
                            method,
                            self_ty,
                            targs,
                            choice,
                        } => {
                            let self_ty = s(self_ty);
                            let targs: Vec<Ty> =
                                pool.list_items(targs).iter().copied().map(s).collect();
                            // A member the compiler lowers at each call, at
                            // any receiver, a trait value included
                            // (`Inspectable.downcast`): its arguments are
                            // `[Self, method arguments...]`.
                            if let Some(key) = env.intrinsic(method) {
                                let t = CallTarget {
                                    key: Hash128(0),
                                    item: method,
                                    args: pool.list(&[&[self_ty], &targs[..]].concat()),
                                    ret: ty,
                                    kind: TargetKind::Intrinsic(key),
                                };
                                self.hash_target(&t, &mut reps);
                                calls.insert(ix, Target::Call(t));
                                continue;
                            }
                            let supplied = match choice.0 {
                                ChoiceKind::Builtin => {
                                    self.supplied_target(method, self_ty, &targs, id)?
                                }
                                _ => None,
                            };
                            if let Some(t) = supplied {
                                t
                            } else {
                                let pick = match choice.0 {
                                    ChoiceKind::Impl => Some(DefId::from_raw(choice.1)),
                                    ChoiceKind::Bound => None,
                                    ChoiceKind::TraitValue => continue,
                                    ChoiceKind::Builtin => {
                                        let name = env.path_hash(method);
                                        reps.hash(name);
                                        calls.insert(
                                            ix,
                                            Target::Call(CallTarget {
                                                key: Hash128(0),
                                                item: method,
                                                args: TyList::EMPTY,
                                                ret: ty,
                                                kind: TargetKind::Builtin {
                                                    method: String::new(),
                                                    self_ty,
                                                },
                                            }),
                                        );
                                        continue;
                                    }
                                };
                                if matches!(pool.get(self_ty), TyData::TraitValue { .. }) {
                                    continue;
                                }
                                self.method_target(trait_, method, self_ty, &targs, pick, id)?
                            }
                        }
                    };
                    self.hash_target(&t, &mut reps);
                    calls.insert(ix, Target::Call(t));
                }
                // An omitted argument (codegen.md §13.2 step 6, §13.13): the
                // default body's instance at the call's type arguments.
                Tag::DefaultCall => {
                    let (Some(&d), Some(&l)) = (body.record(a).first(), body.record(a).get(1))
                    else {
                        return err("a malformed default call record");
                    };
                    let def = DefId::from_raw(d);
                    let targs: Vec<Ty> =
                        pool.list_items(TyList(l)).iter().copied().map(s).collect();
                    let t = self.target(def, pool.list(&targs), id)?;
                    let bracket = env.body(def).is_some_and(makes_call);
                    reps.u8(u8::from(bracket));
                    self.hash_target(&t, &mut reps);
                    calls.insert(ix, Target::Default { call: t, bracket });
                }
                Tag::Closure => {
                    let sub_k = u16::try_from(a).expect("subs");
                    let key = self.push(item, Sub::Body(sub_k), args, id)?;
                    calls.insert(ix, Target::Closure(key));
                }
                // A function reference is a closure whose code is the
                // adapter instance of the item at its substituted type
                // arguments (codegen.md §13.11).
                Tag::ItemRef => {
                    let (Some(&d), Some(&l)) = (body.record(a).first(), body.record(b).first())
                    else {
                        return err("a malformed function reference record");
                    };
                    let targs: Vec<Ty> =
                        pool.list_items(TyList(l)).iter().copied().map(s).collect();
                    let key = self.push(DefId::from_raw(d), Sub::Adapter, pool.list(&targs), id)?;
                    calls.insert(ix, Target::Closure(key));
                }
                Tag::Coerce => {
                    let rec = body.record(b);
                    if rec.first().copied() != Some(Coercion::ToTraitValue as u32) {
                        continue;
                    }
                    let from = s(value_ty(body, a));
                    let from = match pool.get(from) {
                        TyData::Mut(x) => x,
                        _ => from,
                    };
                    let to = match pool.get(ty) {
                        TyData::Mut(x) => x,
                        _ => ty,
                    };
                    let TyData::TraitValue {
                        def: trait_,
                        args: trait_args,
                        ..
                    } = pool.get(to)
                    else {
                        return err("a trait-value coercion to a non-trait type");
                    };
                    let vt = self.vtable(trait_, trait_args, from, id, &mut reps)?;
                    calls.insert(ix, Target::VTable(vt));
                }
                Tag::With => {
                    let rec = body.record(a).to_vec();
                    let mut withs = Vec::new();
                    for pair in rec.chunks(2) {
                        let [k, v] = pair else {
                            return err("a malformed `$.with` record");
                        };
                        let TyData::TraitValue {
                            def: trait_,
                            args: trait_args,
                            ..
                        } = pool.get(s(Ty(*k)))
                        else {
                            return err("a `$.with` key that is not a trait");
                        };
                        let from = s(value_ty(body, *v));
                        let from = match pool.get(from) {
                            TyData::Mut(x) => x,
                            _ => from,
                        };
                        withs.push(self.vtable(trait_, trait_args, from, id, &mut reps)?);
                    }
                    calls.insert(ix, Target::Withs(withs));
                }
                // A map's key operations (codegen.md §13.12): a literal
                // inserts each entry, and lookup, insertion and removal
                // hash and compare the key through its impls.
                Tag::NewMap if !body.record(b).is_empty() => {
                    if let Some(t) = self.map_key(ty, id, &mut reps)? {
                        calls.insert(ix, t);
                    }
                }
                Tag::Intrinsic
                    if matches!(
                        IntrinsicOp::from_u32(a),
                        Some(
                            IntrinsicOp::MapGet
                                | IntrinsicOp::MapSet
                                | IntrinsicOp::MapIndex
                                | IntrinsicOp::MapRemove
                        )
                    ) =>
                {
                    let Some(&m) = body.record(b).first() else {
                        return err("a map operation without its map");
                    };
                    if let Some(t) = self.map_key(s(value_ty(body, m)), id, &mut reps)? {
                        calls.insert(ix, t);
                    }
                }
                _ => {}
            }
        }
        sub.hash_into(&mut reps);
        if self.out.calls.len() <= id.idx() {
            self.out.calls.resize_with(id.idx() + 1, HashMap::new);
            self.out.callee_reps.resize(id.idx() + 1, Hash128(0));
        }
        self.out.calls[id.idx()] = calls;
        self.out.callee_reps[id.idx()] = reps.finish();
        Ok(())
    }
}

/// Whether a body may run other code: a call of any form, or a map
/// operation that hashes and compares its key through impls (§13.12). A
/// body that does not can never reach `block_on`, so its `DefaultCall`
/// needs no forbidden-context bracket (codegen.md §12.3).
fn makes_call(body: &Body) -> bool {
    (0..body.len()).any(|i| match body.tags[i] {
        Tag::Call
        | Tag::CallValue
        | Tag::CallDyn
        | Tag::DefaultCall
        | Tag::Await
        | Tag::AwaitValue
        | Tag::AwaitAll
        | Tag::AwaitRace => true,
        Tag::NewMap => !body.record(body.data[i][1]).is_empty(),
        Tag::Intrinsic => matches!(
            IntrinsicOp::from_u32(body.data[i][0]),
            Some(
                IntrinsicOp::MapGet
                    | IntrinsicOp::MapSet
                    | IntrinsicOp::MapIndex
                    | IntrinsicOp::MapRemove
            )
        ),
        _ => false,
    })
}

/// Marks in `out` each own parameter of `def` that `t` holds where its
/// exact representation matters (`inside`: below a data type, tuple,
/// function type, trait value or projection, or in a requirement key).
fn pinned(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    def: DefId,
    t: Ty,
    inside: bool,
    out: &mut [bool],
) {
    if !pool.has_param(t) {
        return;
    }
    match pool.get(t) {
        TyData::Param(p) => {
            if inside
                && p.owner == def
                && let Some(x) = out.get_mut(usize::from(p.index))
            {
                *x = true;
            }
        }
        TyData::Mut(x) | TyData::Option(x) => pinned(pool, env, def, x, inside, out),
        TyData::Adt { def: d, args } => {
            // `List`, `Map` and enum payloads store references erased.
            let erased = matches!(
                env.std_kind(d),
                layout::StdKind::List | layout::StdKind::Map
            ) || env.enum_variants(d, args).is_some();
            for a in pool.list_items(args).iter().copied() {
                pinned(pool, env, def, a, inside || !erased, out);
            }
        }
        TyData::Tuple { elems, rest } => {
            for e in pool.list_items(elems).iter().copied().chain(rest) {
                pinned(pool, env, def, e, true, out);
            }
        }
        TyData::Fn {
            params,
            result,
            row,
            ..
        } => {
            for e in pool.list_items(params).iter().copied().chain([result]) {
                pinned(pool, env, def, e, true, out);
            }
            for k in pool.row_data(row).keys {
                pinned(pool, env, def, k, true, out);
            }
        }
        TyData::Row(row) => {
            for k in pool.row_data(row).keys {
                pinned(pool, env, def, k, true, out);
            }
        }
        TyData::TraitValue { args, bindings, .. } => {
            for e in pool
                .list_items(args)
                .iter()
                .copied()
                .chain(bindings.iter().map(|b| b.1))
            {
                pinned(pool, env, def, e, true, out);
            }
        }
        TyData::Assoc { self_ty, args, .. } => {
            for e in pool.list_items(args).iter().copied().chain([self_ty]) {
                pinned(pool, env, def, e, true, out);
            }
        }
        _ => {}
    }
}

fn note_data(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, out: &mut BTreeSet<DefIdOrd>) {
    if let TyData::Adt { def, .. } = pool.get(t)
        && env.data_fields(def).is_some()
        && out.insert(DefIdOrd(env.path_hash(def), def.raw()))
    {
        for f in env.data_fields(def).unwrap_or_default() {
            note_data(pool, env, f, out);
        }
    }
}

/// Collection from `root` (codegen.md §13.1) and the init bodies `inits`
/// of the groups it reaches, in initialization order: every reachable
/// instance, each call's target, the imports and data types; or the stop
/// at an instantiation limit.
pub fn collect(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    solver: &dyn Solver,
    root: DefId,
    inits: &[DefId],
    extra: &[(DefId, TyList)],
) -> StageResult<Result<Collected, TooDeep>> {
    let mut cx = Cx {
        pool,
        env,
        solver,
        penv: ParamEnv::default(),
        memo: BodyMemo::default(),
        selected: HashMap::new(),
        exact: HashMap::new(),
        canons: CanonMemo::default(),
        layouts: HashMap::new(),
        depths: HashMap::new(),
        at: None,
        too_deep: None,
        out: Collected::default(),
        work: Vec::new(),
    };
    if let Err(e) = cx.run(root, inits, extra) {
        return match cx.too_deep.take() {
            Some(t) => Ok(Err(t)),
            None => Err(e),
        };
    }
    let mut out = cx.out;
    out.calls.resize_with(out.table.len(), HashMap::new);
    out.callee_reps.resize(out.table.len(), Hash128(0));
    Ok(Ok(out))
}

impl Cx<'_> {
    fn run(&mut self, root: DefId, inits: &[DefId], extra: &[(DefId, TyList)]) -> StageResult<()> {
        self.push(root, Sub::Body(0), TyList::EMPTY, InstId::NONE)?;
        for i in inits {
            let k = self.push(*i, Sub::Body(0), TyList::EMPTY, InstId::NONE)?;
            self.out.inits.push(k);
        }
        // Extra roots with type arguments: the test cases after the first
        // and the `std.rt` result functions (engines-and-test-runner.md
        // §19.1).
        for (d, args) in extra {
            let t = self.target(*d, *args, InstId::NONE)?;
            self.out.extra.push(t.key);
        }
        // Breadth-first in push order (codegen.md §13.4): the first
        // instance over a limit is the same on every run.
        let mut next = 0;
        while next < self.work.len() {
            let id = self.work[next];
            next += 1;
            self.scan(id)?;
        }
        Ok(())
    }
}
