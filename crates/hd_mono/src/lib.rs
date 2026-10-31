#![forbid(unsafe_code)]
//! `hd_mono`: monomorphizing collection (`crates/hd_mono/DESIGN.md` §13.1 to §13.3) over
//! `hd_tir::ir` bodies into the `InstanceTable`, with A1 classes and
//! instance keys from `layout`, and trait selection through the solver's
//! `select` plus a head match. Collection records each call's target, each
//! closure's code instance and each trait-value coercion's vtable, so
//! emission never selects again. Reads TIR and interfaces only, never
//! syntax. Bodies of every package (std included) are collected alike.

pub mod defaults;
pub mod erased;
pub mod layout;
pub mod opaque;
pub mod plan;

use hd_base::SliceSort as _;
use hd_base::{HashMap, HashSet};
use std::collections::BTreeSet;
use std::sync::Arc;

use hd_base::{
    BuildProfile, DefId, Hash128, InstId, NodeIdx, NotImplemented, StableHasher, Stage, StageResult,
};
use hd_tir::ir::{Body, Callee, ChoiceKind, Coercion, IntrinsicOp, Tag};
use hd_types::solver::{
    BodyMemo, ConcreteTraitRef, Declarations, GlobalMemo, ImplRef, ImplUniverseId, Impls, ParamEnv,
    Selection, SolveCx, Solver, TraitRef,
};
use hd_types::{InternPool, ParamRef, Prim, RowId, Ty, TyData, TyList};

pub use crate::layout::Sub;
use crate::layout::{
    A1Class, CanonMemo, KeyArg, LayoutEnv, MAX_CHAIN, MAX_DEPTH, StdKind, a1_class, canon,
    instance_key,
};
use crate::plan::{BodyPlan, Planner, Source};

/// What collection reads about the program (the driver implements it over
/// TIR and interfaces).
pub trait ProgramEnv: LayoutEnv {
    /// The RC layout's A1 reference-storage class. `None` uses the
    /// generic reference classification; concrete layouts retain their keys.
    fn shared_reference_layout(&self, _ty: Ty) -> Option<bool> {
        None
    }
    fn body(&self, def: DefId) -> Option<&Body>;
    /// A concrete replacement for `def`'s ordinary body at `args`, when a
    /// post-monomorphization transformation changed it. Collection and
    /// emission both use the returned body.
    fn instance_body(&self, _def: DefId, _args: TyList) -> StageResult<Option<Body>> {
        Ok(None)
    }
    /// Release (#1118): `def`'s exclusive copy, which borrows through the
    /// parameters in its mask across calls that may write, and which a
    /// call may run where `hd_tir::own::exclusive_call` holds.
    fn exclusive_body(&self, _def: DefId) -> Option<(u64, Arc<Body>)> {
        None
    }
    /// Codegen-only result facts. The driver decodes the raw checked body,
    /// without recursively entering its ownership once-cell.
    fn opaque_result(
        &self,
        _pool: &InternPool,
        _owner: DefId,
        _site: u32,
    ) -> Option<opaque::HiddenResult> {
        None
    }
    /// The declaring trait when an opaque result belongs to a trait method.
    fn opaque_trait(&self, _method: DefId) -> Option<DefId> {
        None
    }
    /// Per type parameter of the item itself: has a bound (A1, `crates/hd_mono/DESIGN.md`
    /// §13.2: exact).
    fn bounded(&self, def: DefId) -> Option<Vec<bool>>;
    /// Per type parameter of the item itself: is a row parameter (`$R`).
    fn generic_rows(&self, def: DefId) -> Vec<bool>;
    /// The declared result type.
    fn ret(&self, def: DefId) -> Option<Ty>;
    /// The declared parameter types (`self` first for a method).
    fn params(&self, def: DefId) -> Option<Vec<Ty>>;
    /// Whether the function suspends (`fn f!`).
    fn suspends(&self, def: DefId) -> bool;
    /// Whether the function is marked `@cold`: each direct call of it goes
    /// through the function table (`annot.inline.cold`).
    fn cold(&self, _def: DefId) -> bool {
        false
    }
    /// The requirement keys of an instance's row, each the whole trait
    /// value type with its substituted arguments and bindings (`crates/hd_mono/DESIGN.md`
    /// §13.18), in key order (`key_order`, §12.4): the declared keys, and
    /// the keys of each row parameter's argument, so providers are passed
    /// per instance.
    fn row_keys(&self, def: DefId, args: TyList) -> Vec<Ty>;
    /// A method's owner (impl, trait, or the data type or enum that owns
    /// its declaration `Structure`'s generated methods) and how many of
    /// the instance's arguments are the owner's: an impl's or a type's
    /// parameters, or a trait's `Self` and parameters.
    fn parent(&self, def: DefId) -> Option<(DefId, usize)>;
    /// The item whose type parameters `def`'s types name: a default body's
    /// declaring function, method or data type, whose instance arguments
    /// it shares (`crates/hd_mono/DESIGN.md` §13.13); `def` itself for every other item.
    fn generics_owner(&self, def: DefId) -> DefId;
    /// An impl's head: self type, trait arguments, parameter count.
    fn impl_head(&self, impl_: DefId) -> Option<(Ty, TyList, usize)>;
    /// The data types `std.core.Ref[T]` and `std.core.Counted[T]` that `&T`
    /// and `*T` lower to (`hd_types::Types::lower_refs`); selection raises
    /// them back (`hd_types::Types::raise_refs`), the form impl heads name.
    fn ref_cells(&self) -> hd_types::RefCells;
    /// The suspension type `Suspend[T]`, a suspending call's cold value.
    fn suspend_def(&self) -> DefId;
    /// Whether `impl_` is a `@derive` implementation on a nominal
    /// newtype: its methods forward to the base type's implementation,
    /// so with erased newtypes calls use the base method directly and
    /// no adapter instance is collected.
    fn is_derived_newtype_impl(&self, impl_: DefId) -> bool;
    /// The method of `impl_` that implements the trait method `method`.
    fn impl_method(&self, impl_: DefId, method: DefId) -> Option<DefId>;
    /// A trait's own methods, in declaration order (the vtable shape),
    /// without compiler-intrinsic members and the associated functions,
    /// which a trait value cannot dispatch (trait.dyn.member.assoc-function).
    fn trait_methods(&self, trait_: DefId) -> Vec<DefId>;
    /// A trait's own parameter count, `Self` excluded.
    fn trait_arity(&self, trait_: DefId) -> usize;
    /// The associated types a trait itself declares.
    fn trait_assocs(&self, trait_: DefId) -> Vec<DefId>;
    /// The compiler's lowering key of a body-less std function
    /// (`@intrinsic("key")`, or a compiler-supplied item by name).
    fn intrinsic(&self, def: DefId) -> Option<String>;
    /// `std.json`'s `encode`, `decode`, their two trait declarations, and
    /// `std.result`'s checked unwrap after a boundary `decode`.
    fn json_items(&self) -> Option<JsonItems>;
    /// Public HD capability -> internal wire trait and ordinary generic
    /// library adapter. Rich message state never crosses a host boundary.
    fn host_bridge(&self, _key: Ty) -> Option<HostBridge> {
        None
    }
    /// A package-declared host trait selects the value-preserving JS bridge.
    fn is_package_host_trait(&self, _def: DefId) -> bool {
        false
    }

    /// Data fields, for the struct types a program needs.
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>>;
    /// The names of `def`'s data fields, in [`ProgramEnv::data_fields`]'
    /// order, for development tools that name a value's parts
    /// (`hd_wasm_linear::layout::value_paths`); `None` when unknown.
    fn data_field_names(&self, _def: DefId) -> Option<Vec<String>> {
        None
    }
    /// Variant and payload field names in `LayoutEnv::enum_variants` order,
    /// for generated host interfaces. `None` when names are unavailable.
    fn enum_variant_names(&self, _def: DefId) -> Option<Vec<(String, Vec<String>)>> {
        None
    }
    /// Whether `def` is a `struct` declaration: a value whose copies copy
    /// the collections it holds (`crates/hd_wasm_linear/DESIGN.md` §15.2,
    /// "Collection values").
    fn is_struct(&self, def: DefId) -> bool {
        let _ = def;
        false
    }
    /// Whether a data type has a field that is not `pub`: its value crosses
    /// a boundary only through its `Serialize` and `Deserialize`
    /// (`module.boundary.out`, `module.boundary.in`). A newtype's inner
    /// value has no visibility.
    fn has_private_field(&self, def: DefId) -> bool;
    fn path_hash(&self, def: DefId) -> Hash128;
    /// A declaration's printable name in a `TypeId` (trait.typeid.name.*):
    /// a prelude name as written, any other declaration by its absolute
    /// qualified name.
    fn type_name(&self, def: DefId) -> String;
    /// The same declaration as it prints (trait.typeid.name.*): a package
    /// root by its manifest name, a single-file program's by nothing.
    fn printed_name(&self, def: DefId) -> String;
    /// The item's stable path, for diagnostics.
    fn describe(&self, def: DefId) -> String;
    /// The text of a string const argument (`hd_types::ConstValue::Str`).
    fn const_text(&self, p: hd_base::PathId) -> String;
    /// Where selection reads impls: owner lookup over the whole program's
    /// folders (`crates/hd_check/DESIGN.md` (trait solver) §3.2, §8.3), the program being the impl
    /// universe.
    fn impls(&self) -> Impls<'_>;
    /// The program's impl universe and the run's proof memo, which
    /// selection shares with checking (`crates/hd_check/DESIGN.md` (trait solver) §7.1).
    fn solving(&self) -> (ImplUniverseId, &GlobalMemo);
    /// Where an impl's row lives.
    fn impl_row(&self, impl_: DefId) -> Option<ImplRef>;
    /// The declarations the solver's compiler-supplied rows read
    /// (`crates/hd_check/DESIGN.md` (trait solver) §3.9), so selection answers sealed traits as
    /// checking does.
    fn decls(&self) -> &dyn Declarations;
    /// The body of a compiler-supplied impl's method at a concrete self
    /// type, when the compiler generates one in TIR (a tuple's
    /// `Structure`, `crates/hd_mono/DESIGN.md` §13.6): its signature is the trait
    /// method's with `Self` replaced, so its instance's arguments are the
    /// self type, then the trait's and the method's own. `None` when
    /// emission lowers the method itself (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
    /// The requested body first, followed by the cached helper bodies it calls.
    fn supplied_body(&self, method: DefId, self_ty: Ty) -> StageResult<Option<Vec<Body>>>;
    /// The body of one of the `dbg` functions the compiler supplies per
    /// concrete argument (`dbg_printer[ty]`, `dbg[args]`; spec/lang/10-modules.md
    /// "Debug Printing"). `debug`: `ty` implements `Debug`.
    fn dbg_body(&self, item: DefId, key: &str, ty: Ty, debug: bool) -> StageResult<Body>;
    /// `body`, which collection built (a function reference's call body,
    /// [`adapter_call_body`]), after the ownership passes, as the driver
    /// runs them over every body it supplies.
    fn own_generated(&self, body: Body) -> StageResult<Body>;
    /// Bit k: every instance of `item` receives its value parameter k (the
    /// receiver first) lent, so a call passes it with no count and glue
    /// that calls the instance counts none for it. Only the optimized
    /// pipeline's release summaries set one
    /// (`docs/future-work/RELEASE_INTERPROC.md`).
    fn lends(&self, _item: DefId) -> u64 {
        0
    }
    /// `std.format.Debug`, whose printer is preferred by `dbg_printer`.
    fn debug_trait(&self) -> DefId;
    /// Private template method and Structure trait; no public conformance is added.
    fn dbg_structure(&self) -> Option<(DefId, DefId)> {
        None
    }

    /// The items a map key's hashing and equality call (`crates/hd_mono/DESIGN.md`
    /// §13.12): the `Eq` trait, its method (which has no vtable slot, as it
    /// names `Self`) and `std.hash.key_bucket_hash`, the keyed hash a map
    /// buckets a key that is not inline by.
    fn map_key_items(&self) -> Option<(DefId, DefId, DefId)>;
    /// The keyed hash a map buckets an inline key by (`crates/hd_mono/DESIGN.md`
    /// §13.12): `std.hash.string_bucket_hash` for a `string`, else
    /// `std.hash.int_bucket_hash`, which takes the key's bits as a `u64`.
    fn inline_bucket_hash(&self, string: bool) -> DefId;
    /// The build's profile (`cli.profile.*`), which decides whether
    /// emitted integer arithmetic checks overflow and shift counts.
    fn build_profile(&self) -> BuildProfile;
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

/// Whether `t` is a trait value, readonly or `mut`.
pub fn is_trait_value(pool: &InternPool, t: Ty) -> bool {
    let t = match pool.get(t) {
        TyData::Mut(x) => x,
        _ => t,
    };
    matches!(pool.get(t), TyData::TraitValue { .. })
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
    /// (`Inspectable.runtime_type`, `crates/hd_check/DESIGN.md` (trait solver) §3.9): the method's
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

/// A row's requirement keys in key order (`crates/hd_mono/DESIGN.md` §12.4, §13.18): the
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
    keyed.sort_stable_by_key(|p| p.0);
    keyed.dedup_by_key(|p| p.0);
    keyed.into_iter().map(|p| p.1).collect()
}

/// What collection recorded for one instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Call(CallTarget),
    /// An omitted argument's default body (`crates/hd_mono/DESIGN.md` §13.13): its
    /// instance, and whether the body makes a call, so emission brackets
    /// it with the forbidden-context counter (§12.3).
    Default {
        call: CallTarget,
        bracket: bool,
    },
    /// An omitted argument whose default body is one scalar constant, such
    /// as `retries: i32 = 3`: the constant's bits, inlined at the call
    /// with no default body instance (§13.13).
    ConstDefault(u64),
    /// A coercion to a trait value: its vtable's targets.
    VTable(VTable),
    /// A trait method call at a trait-value receiver: it dispatches
    /// through the receiver's table (`crates/hd_mono/DESIGN.md` §13.5,
    /// trait.dyn.bound.reference-dispatch), so collection pushes nothing.
    Dyn,
    /// A closure's code instance.
    Closure(Hash128),
    /// At a `CallValue` its body's plan makes direct (`plan`, rule 1): the
    /// callee is a known closure, whose code instance this is
    /// (`crates/hd_opt/DESIGN.md` (known closure calls), options A1 and A2). Emission calls it directly,
    /// the value passed as the environment, instead of reading the code
    /// field.
    KnownClosure(Hash128),
    /// A map operation on a key that is not inline
    /// (`layout::inline_map_key`): `key_bucket_hash` and `Eq.eq` at the
    /// key type (`crates/hd_mono/DESIGN.md` §13.12).
    MapKey {
        hash: CallTarget,
        eq: CallTarget,
    },
    /// A map operation on an inline key: its keyed bucket hash,
    /// `std.hash.string_bucket_hash` or `std.hash.int_bucket_hash`
    /// (`crates/hd_mono/DESIGN.md` §13.12); equality stays inline.
    MapInline(CallTarget),
    /// An open instruction of an erased body (`crates/hd_mono/DESIGN.md` §13.5.1): it
    /// calls thunk `k` of the witness the body was passed.
    Open(u32),
    /// At [`INFO`] of an erased body: its witness family.
    Erased {
        family: Hash128,
    },
    /// At [`INFO`] of a thunk: the instruction it emits.
    Thunk {
        at: u32,
    },
}

/// The `calls` entry of an instance that describes the instance itself
/// (an erased body's family, a thunk's instruction), not an instruction.
pub const INFO: u32 = u32::MAX;

/// The targets of one vtable at a concrete type (`crates/hd_mono/DESIGN.md` §13.5,
/// §13.16): the trait, one target per method slot, and per direct
/// supertrait, in declared order, that supertrait's vtable at the same
/// type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VTable {
    pub trait_: DefId,
    pub slots: Vec<CallTarget>,
    /// Per slot, how its adapter hands the payload to the target.
    pub recv: Vec<SlotRecv>,
    pub parents: Vec<VTable>,
}

impl VTable {
    /// Whether some slot of the tree reads through a reference payload.
    #[must_use]
    pub fn has_read(&self) -> bool {
        self.recv.contains(&SlotRecv::ReadThrough) || self.parents.iter().any(VTable::has_read)
    }
}

/// How a vtable slot's adapter passes the trait value's payload to the
/// slot's target (`crates/hd_wasm_linear/DESIGN.md` §15.2, "Trait value
/// references").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotRecv {
    /// The payload itself: the concrete value, or, for a payload that is a
    /// reference, the reference a `&self` or `&mut self` target takes.
    Payload,
    /// The payload is a reference (`std.core.Ref[T]`) and the target takes
    /// the value: the adapter reads the value through the reference first.
    ReadThrough,
}

/// A reference whose immediate referent is an erased trait can widen to
/// that trait or a declared supertrait without wrapping its referent again.
/// Nested references still erase their reference-valued referent normally.
pub fn trait_reference_widens(pool: &InternPool, env: &dyn ProgramEnv, from: Ty, to: Ty) -> bool {
    let TyData::Adt { def, args } = pool.get(from) else {
        return false;
    };
    if !env.ref_cells().holds(def) {
        return false;
    }
    let mut inner = pool.list_items(args)[0];
    if let TyData::Mut(t) = pool.get(inner) {
        inner = t;
    }
    let (TyData::TraitValue { def: from, .. }, TyData::TraitValue { def: to, .. }) =
        (pool.get(inner), pool.get(to))
    else {
        return false;
    };
    let mut todo = vec![from];
    let mut seen = HashSet::default();
    while let Some(trait_) = todo.pop() {
        if trait_ == to {
            return true;
        }
        if !seen.insert(trait_) {
            continue;
        }
        for sup in env.decls().supertraits(trait_) {
            if let TyData::TraitValue { def, .. } = pool.get(*sup) {
                todo.push(def);
            }
        }
    }
    false
}

#[derive(Clone, Copy, Debug)]
pub struct HostBridge {
    pub wire: Ty,
    pub adapter: DefId,
}

/// The `std.json` and `std.serde` items a host boundary codec calls: JSON
/// `encode` and `decode` at a private-field type, the two traits the
/// boundary direction needs, and `Result`'s checked unwrap after a
/// boundary `decode` (`module.boundary.serialize.exact`,
/// `.deserialize.exact`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonItems {
    pub package_encode: DefId,
    pub package_decode: DefId,
    pub encode: DefId,
    pub decode: DefId,
    pub serialize: DefId,
    pub deserialize: DefId,
    pub result_is_ok: DefId,
    pub result_expect: DefId,
    /// `std.rt.host_provider`, which puts a host handle that enters hd as
    /// a trait value reference behind a new object
    /// (`BoundaryCodecs::handle_ref`).
    pub host_provider: DefId,
}

/// A boundary `decode` followed by its checked unwrap: the `decode`
/// instance at the value type, and `is_ok` plus `expect` at the
/// `decode` result's type arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JsonDecode {
    pub decode: Hash128,
    pub is_ok: Hash128,
    pub expect: Hash128,
}

/// The JSON codecs of one private-field boundary type, by crossing
/// direction (`module.boundary.direction`): `encode` for a value leaving
/// hd, `decode` for one entering it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BoundaryCodecs {
    pub package_encode: Option<Hash128>,
    pub package_decode: Option<JsonDecode>,
    pub package_decoded: Option<Ty>,
    pub encode: Option<Hash128>,
    pub decode: Option<JsonDecode>,
    /// The `decode` result type, whose Wasm values the boundary decoder
    /// checks and unwraps.
    pub decoded: Option<Ty>,
    /// For a trait value reference `&mut dyn H` to a host handle entering
    /// hd, as `Net.connect!` returns (`std-net.handles.ref`): the
    /// `std.rt.host_provider` instance at `dyn H`, which puts the handle's
    /// trait value behind a new object (`crates/hd_wasm_linear/DESIGN.md` §15.2).
    pub handle_ref: Option<Hash128>,
}

/// The JSON codecs of the private-field types in the entry rows'
/// provider signatures, by stripped type.
pub type BoundaryMap = HashMap<Ty, BoundaryCodecs>;

/// The output of `Collect` (`crates/hd_mono/DESIGN.md` §11.3).
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
    /// Bodies whose constant defaults were folded before collection.
    pub instance_bodies: HashMap<InstId, Arc<Body>>,
    /// The witnesses of generic methods called through trait values
    /// (`crates/hd_mono/DESIGN.md` §13.5.1), which link writes.
    pub witnesses: Witnesses,
    /// The JSON codecs of the private-field types in the entry rows'
    /// provider signatures, by stripped type, which the host boundary
    /// calls instead of reading private fields directly.
    pub boundary: BoundaryMap,
    /// The trait methods that scanned code calls through a trait value
    /// ([`Target::Dyn`], a function reference's adapter included).
    pub dyn_methods: HashSet<DefId>,
}

impl Collected {
    /// The TIR of an instance whose item has no body of its own: the
    /// compiler-supplied method's, at the instance's self type.
    #[must_use]
    pub fn supplied_body(&self, pool: &InternPool, item: DefId, args: TyList) -> Option<&Body> {
        let self_ty = pool.list_items(args).first().copied()?;
        self.supplied.get(&(item, self_ty)).map(AsRef::as_ref)
    }

    /// Whether the program may call trait method `method` through a trait
    /// value, so a host provider's slot for it needs its host call
    /// (`crates/hd_wasm_linear/DESIGN.md`, host providers).
    #[must_use]
    pub fn calls_dyn(&self, method: DefId) -> bool {
        self.dyn_methods.contains(&method)
    }
}

pub use erased::{
    Witnesses, declared_row_keys, erased_args, has_row_params, is_open, open_insts, open_param,
    open_row, outlined_store_local, own_count, row_args_open, row_is_open, row_param_keys, rowless,
    thunk_capture_locals, thunk_operands, witness_args, witness_family,
};
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
/// base becomes concrete resolves to the impl's binding (`crates/hd_mono/DESIGN.md` §13).
#[must_use]
pub fn subst(pool: &InternPool, env: &dyn ProgramEnv, item: DefId, args: TyList, t: Ty) -> Ty {
    let s = substitute(pool, env, item, args, t);
    let s = reveal(pool, env, s).unwrap_or(Ty::POISON);
    if pool.has_assoc(s) {
        let types = pool.types();
        let cell = env.ref_cells();
        let source = types.raise_refs(cell, s);
        let normalized = hd_types::solver::normalize_concrete(types, env.impls(), source);
        types.lower_refs(cell, normalized)
    } else {
        s
    }
}

/// `t` with every opaque result in it replaced by its hidden type, as
/// [`subst`] opens them; `None` when one cannot be opened (a trait
/// method's result at a receiver that selects no impl, a cycle).
#[must_use]
pub fn reveal(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Ty> {
    if !pool.has_opaque(t) {
        return Some(t);
    }
    opaque::reveal_with(
        pool,
        t,
        &|owner, site, args| opaque_result(pool, env, owner, site, args),
        &|owner, site| {
            env.opaque_trait(owner)
                .filter(|_| {
                    env.ret(owner)
                        .is_some_and(|ret| opaque::result_at(pool, ret, ret, site).is_some())
                })
                .map_or(0, |trait_| 1 + env.trait_arity(trait_))
        },
    )
    .ok()
}

/// Substitute parameters without opening another hidden-result equation.
fn substitute(pool: &InternPool, env: &dyn ProgramEnv, item: DefId, args: TyList, t: Ty) -> Ty {
    let item = env.generics_owner(item);
    let a = pool.list_items(args);
    let parent = env.parent(item);
    let n = parent.map_or(0, |p| p.1);
    pool.subst(t, &|p: ParamRef| {
        if p.owner == item {
            a.get(n + p.index as usize).copied()
        } else if parent.is_some_and(|(o, _)| o == p.owner) {
            a.get(p.index as usize).copied()
        } else {
            None
        }
    })
}

/// Open the selected implementation's result while keeping checking head-only.
fn opaque_result(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    method: DefId,
    site: u32,
    args: TyList,
) -> Option<opaque::HiddenResult> {
    let Some(trait_) = env.opaque_trait(method) else {
        return env.opaque_result(pool, method, site);
    };
    let declared_result = env.ret(method)?;
    if opaque::result_at(pool, declared_result, declared_result, site).is_none() {
        // Nested named functions in a trait default keep their own occurrence
        // sites; those are body facts, not the trait method's return contract.
        return env.opaque_result(pool, method, site);
    }
    let actual = pool.list_items(args);
    let (&self_ty, rest) = actual.split_first()?;
    let n = env.trait_arity(trait_);
    let trait_args = pool.list(rest.get(..n)?);
    let types = pool.types();
    let cell = env.ref_cells();
    let (universe, global) = env.solving();
    let penv = ParamEnv::default();
    let mut memo = BodyMemo::default();
    let mut cx = SolveCx {
        pool: types,
        env: &penv,
        universe,
        impls: env.impls(),
        decls: env.decls(),
        body_memo: &mut memo,
        global,
    };
    let Selection::Impl { impl_row, .. } = hd_types::solver::TableSolver
        .select(
            &mut cx,
            ConcreteTraitRef(TraitRef {
                trait_,
                self_ty: types.raise_refs(cell, self_ty),
                args: types.list(
                    &pool
                        .list_items(trait_args)
                        .iter()
                        .map(|&t| types.raise_refs(cell, t))
                        .collect::<Vec<_>>(),
                ),
            }),
        )
        .ok()?
    else {
        return None;
    };
    let table = env.impls().table(impl_row.module)?;
    let row = impl_row.row as usize;
    let imp = table.def[row];
    let Some(selected) = env.impl_method(imp, method) else {
        // The trait's default body shares its Self/trait/method arguments.
        return env.opaque_result(pool, method, site);
    };
    let (head, formals, count) = env.impl_head(imp)?;
    let mut values = vec![None; count];
    unify(pool, imp, head, self_ty, &mut values);
    for (&formal, &actual) in pool
        .list_items(formals)
        .iter()
        .zip(pool.list_items(trait_args))
    {
        unify(pool, imp, formal, actual, &mut values);
    }
    if values.iter().any(Option::is_none) {
        hd_types::solver::apply_binds(types, env.impls(), table, row, &mut values);
    }
    let mut selected_args: Vec<Ty> = values.into_iter().collect::<Option<_>>()?;
    selected_args.extend_from_slice(&rest[n..]);
    let selected_result = substitute(
        pool,
        env,
        selected,
        pool.list(&selected_args),
        env.ret(selected)?,
    );
    let declared = substitute(pool, env, method, args, env.ret(method)?);
    // The selected signature can use either a concrete return or its own opaque
    // declaration. The latter stays lazy so reveal's cycle guard still applies.
    let result = opaque::result_at(pool, declared, selected_result, site)?;
    Some((args, result))
}

/// The instruction types of this instance's final body, substituted once
/// for emission. Use the supplied body after specialization and ownership
/// lowering, including for backend-generated bodies.
#[must_use]
pub fn instruction_types(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    item: DefId,
    args: TyList,
    body: &Body,
) -> Vec<Ty> {
    body.ty
        .iter()
        .map(|&t| subst(pool, env, item, args, t))
        .collect()
}

/// Whether copying a value of type `t` copies collections
/// (`crates/hd_wasm_linear/DESIGN.md` §15.2, "Collection values"): a `List` or a
/// `Map` value, or a struct holding one in its fields, at any depth of
/// struct fields. Generic code over such a type is laid out exactly, never
/// as the A1 class `REF`, so a copy in it knows what it copies.
#[must_use]
pub fn holds_coll(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    holds_coll_at(pool, env, t, 0)
}

fn holds_coll_at(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, depth: u32) -> bool {
    if depth > 16 {
        return false;
    }
    match pool.get(t) {
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::List | StdKind::Map => true,
            StdKind::Other => {
                if let Some(base) = env.newtype_base(def) {
                    return holds_coll_at(pool, env, subst(pool, env, def, args, base), depth + 1);
                }
                env.is_struct(def)
                    && env.data_fields(def).unwrap_or_default().iter().any(|f| {
                        holds_coll_at(pool, env, subst(pool, env, def, args, *f), depth + 1)
                    })
            }
            StdKind::Suspend | StdKind::Buffer => false,
        },
        _ => false,
    }
}

/// The printable name of `t`'s runtime identity (trait.typeid.name.*),
/// which a `TypeId` holds as its key (its printable name is
/// [`printed_type_name`]). Equality of `TypeId`s compares keys
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
    name_into(pool, env, false, t, &mut out, usize::MAX);
    out
}

/// The printable name of `t` (trait.typeid.name.*): what `Display` for a
/// `TypeId` prints. It spells `t` as `type_name` does, but each
/// declaration by its printable name, so two types of different identity
/// can print alike.
#[must_use]
pub fn printed_type_name(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> String {
    let t = match pool.get(t) {
        TyData::Mut(x) => x,
        _ => t,
    };
    let mut out = String::new();
    name_into(pool, env, true, t, &mut out, usize::MAX);
    out
}

fn def_name(env: &dyn ProgramEnv, printed: bool, def: DefId) -> String {
    if printed {
        env.printed_name(def)
    } else {
        env.type_name(def)
    }
}

/// The longest type a diagnostic prints in full, in bytes; a longer one is
/// cut there and ends in `...`. A type that nests 33 deep can have 2^33
/// leaves (`instantiation-too-deep`, `crates/hd_mono/DESIGN.md` §13.4).
pub const SHOWN_TYPE_MAX: usize = 120;

/// `t` as `type_name` spells it, cut at `SHOWN_TYPE_MAX` bytes: the walk
/// stops there, so its cost is bounded too, however large the type's tree.
#[must_use]
pub fn shown_type(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> String {
    let mut out = String::new();
    name_into(pool, env, false, t, &mut out, SHOWN_TYPE_MAX);
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
fn name_into(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    printed: bool,
    t: Ty,
    out: &mut String,
    limit: usize,
) {
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
            name_into(pool, env, printed, *x, out, limit);
        }
    };
    match pool.get(t) {
        TyData::Opaque { bound, .. } => {
            out.push_str("impl ");
            name_into(pool, env, printed, bound, out, limit);
        }
        TyData::Prim(p) => out.push_str(p.name()),
        TyData::Never => out.push_str("never"),
        TyData::Adt { def, args } if env.ref_cells().holds(def) => {
            let inner = pool.list_items(args)[0];
            let inner = if let TyData::Mut(inner) = pool.get(inner) {
                out.push_str("&mut ");
                inner
            } else {
                out.push('&');
                inner
            };
            name_into(pool, env, printed, inner, out, limit);
        }
        TyData::Adt { def, args } => {
            out.push_str(&def_name(env, printed, def));
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
            out.push_str(&def_name(env, printed, def));
            let args = pool.list_items(args);
            if !args.is_empty() || !bindings.is_empty() {
                out.push('[');
                list(args, ", ", out);
                for (k, (a, b)) in bindings.iter().enumerate() {
                    if k > 0 || !args.is_empty() {
                        out.push_str(", ");
                    }
                    let n = def_name(env, printed, *a);
                    out.push_str(n.rsplit('.').next().unwrap_or(&n));
                    out.push_str(" = ");
                    name_into(pool, env, printed, *b, out, limit);
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
                name_into(pool, env, printed, r, out, limit);
                out.push_str("...");
            }
            out.push(')');
        }
        TyData::Option(i) => {
            name_into(pool, env, printed, i, out, limit);
            out.push('?');
        }
        TyData::Mut(i) => {
            out.push_str("mut ");
            name_into(pool, env, printed, i, out, limit);
        }
        // The kind is erased after checking (`ref.type.kind-runtime`), so
        // a run-time name keeps one spelling.
        TyData::Ref { inner, mutable, .. } => {
            out.push_str(if mutable { "&mut " } else { "&" });
            name_into(pool, env, printed, inner, out, limit);
        }
        // Not inspectable (trait.inspectable.not.*), but a diagnostic
        // spells them, and a typed fact's identity names its pattern's
        // arguments (`annot.handle.fact.exact`): the row is part of it.
        TyData::Fn {
            params,
            result,
            row,
            suspends,
            ..
        } => {
            out.push_str(if suspends { "fn!(" } else { "fn(" });
            list(pool.list_items(params), ", ", out);
            out.push_str(") -> ");
            name_into(pool, env, printed, result, out, limit);
            let keys = pool.row_data(row).keys;
            if !keys.is_empty() {
                out.push_str(" $ ");
                list(&keys, " + ", out);
            }
        }
        TyData::Row(r) => {
            out.push_str("$(");
            list(&pool.row_data(r).keys, " + ", out);
            out.push(')');
        }
        TyData::Context(r) => {
            out.push_str("$.Context[$(");
            list(&pool.row_data(r).keys, " + ", out);
            out.push_str(")]");
        }
        TyData::Assoc { assoc, self_ty, .. } => {
            out.push('<');
            name_into(pool, env, printed, self_ty, out, limit);
            let n = def_name(env, printed, assoc);
            out.push_str(">.");
            out.push_str(n.rsplit('.').next().unwrap_or(&n));
        }
        // `types.generic.const.display`.
        TyData::Const(v) => out.push_str(&v.literal(|p| env.const_text(p))),
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
pub fn unify(pool: &InternPool, owner: DefId, pattern: Ty, t: Ty, out: &mut Vec<Option<Ty>>) {
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
                row: ar,
                inputs: ai,
                ..
            },
            TyData::Fn {
                params: b,
                result: s,
                row: br,
                inputs: bi,
                ..
            },
        ) => {
            if ai == bi {
                for (x, y) in pool
                    .list_items(a)
                    .iter()
                    .copied()
                    .zip(pool.list_items(b).iter().copied())
                {
                    unify(pool, owner, x, y, out);
                }
            } else {
                // `Fn[Args, O, $ R]` against a concrete function type:
                // `Args` is its whole inputs tuple (`fn.type.ctor.inputs`).
                let p = pool.types();
                unify(
                    pool,
                    owner,
                    p.inputs_tuple(a, ai),
                    p.inputs_tuple(b, bi),
                    out,
                );
            }
            unify(pool, owner, r, s, out);
            unify_row(pool, owner, ar, br, out);
        }
        (TyData::Row(a), TyData::Row(b)) => unify_row(pool, owner, a, b, out),
        _ => {}
    }
}

/// Matches an impl head's row parameters, filling their instance arguments
/// with the concrete row as a `TyData::Row`, just as `unify` does for type
/// parameters. `Types::subst_row` consumes that same representation.
fn unify_row(
    pool: &InternPool,
    owner: DefId,
    pattern: RowId,
    actual: RowId,
    out: &mut Vec<Option<Ty>>,
) {
    for p in pool.row_data(pattern).params {
        if p.owner != owner {
            continue;
        }
        let i = p.index as usize;
        if out.len() <= i {
            out.resize(i + 1, None);
        }
        let row = pool.intern_ty(&TyData::Row(actual));
        out[i].get_or_insert(row);
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
/// enum's payloads, a trait's method parameters, results and row keys (a
/// slot's provider parameters) and its supertraits (a vtable's parent
/// fields, with the bindings they fix), as declared.
fn declared_types(env: &dyn ProgramEnv, def: DefId) -> Vec<Ty> {
    if let Some(fs) = env.data_fields(def) {
        return fs;
    }
    if let Some(vs) = env.enum_declared(def) {
        return vs.into_iter().flatten().collect();
    }
    let mut out = env.decls().supertraits(def).to_vec();
    for m in env.trait_methods(def) {
        out.extend(env.params(m).unwrap_or_default());
        out.extend(env.ret(m));
        out.extend(env.row_keys(m, TyList::EMPTY));
    }
    out
}

/// `layout_hash(T)` (`crates/hd_mono/DESIGN.md` §13.8): the hash of `canon(T)` and of the
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
    items_in(pool, t, &mut items, &mut HashSet::default());
    let mut hashes: Vec<Hash128> = items
        .into_iter()
        .map(|d| declarations_hash(pool, env, d, canons, memo))
        .collect();
    hashes.sort_stable();
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
        // The kind and shape: `declared_types` flattens an enum's payloads,
        // so `data D: x: string` and `enum D: V(x: string)` would hash
        // alike, though their layouts differ.
        if env.data_fields(d).is_some() {
            h.u8(1);
        } else if let Some(vs) = env.enum_declared(d) {
            h.u8(2);
            h.u32(u32::try_from(vs.len()).expect("variants"));
            for v in &vs {
                h.u32(u32::try_from(v.len()).expect("fields"));
            }
        } else {
            h.u8(3);
        }
        let mut names = Vec::new();
        let mut seen = HashSet::default();
        let tys = declared_types(env, d);
        h.u32(u32::try_from(tys.len()).expect("types"));
        for t in tys {
            h.hash(canon(pool, &ph, canons, t));
            items_in(pool, t, &mut names, &mut seen);
        }
        names.sort_stable_by_key(|n| n.raw());
        names.dedup();
        (h.finish(), names)
    };
    // Per visited item, by visit number: the item, its own hash, the items
    // it names, its low link and whether it is on `stack`.
    let (own, names) = read(root, canons);
    let mut index: HashMap<DefId, usize> = HashMap::from_iter([(root, 0)]);
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
        let inside: hd_base::HashSet<DefId> = members.iter().map(|&m| nodes[m].0).collect();
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
        owns.sort_stable();
        outside.sort_stable();
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

/// What selection picked (`crates/hd_check/DESIGN.md` (trait solver) §8.3): an impl and its
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
    /// chosen impl)`: the impl and its arguments (`crates/hd_check/DESIGN.md` (trait solver) §8.3).
    selected: HashMap<SelectKey, Picked>,
    /// Per item: which of its own type parameters need their exact
    /// representation (A1's representation summary, §13.2).
    exact: HashMap<DefId, Vec<bool>>,
    /// Least fixed-point propagation of parameters whose exact type identity
    /// a reachable weak constructor observes.
    weak_exact: HashMap<DefId, Vec<bool>>,
    permission_sensitive: HashMap<DefId, bool>,
    permission_shared: HashMap<Ty, Ty>,
    /// `canon` per type, for the build: instance keys, layout hashes and
    /// call targets read each type's digest once.
    canons: CanonMemo,
    /// `declarations_hash` per item, for the build.
    layouts: HashMap<DefId, Hash128>,
    /// Nesting depth per type, so a type shared by its parts is walked
    /// once however large its tree (`crates/hd_mono/DESIGN.md` §13.4).
    depths: HashMap<Ty, u32>,
    /// The instruction being scanned: its item and syntax node, where an
    /// instance request over a limit is reported.
    at: Option<(DefId, NodeIdx)>,
    /// Set when collection stops at a limit.
    too_deep: Option<TooDeep>,
    out: Collected,
    work: Vec<InstId>,
    /// Erased bodies, `dyn` call pairs and their thunks (§13.5.1).
    witness: erased::WitnessCx,
    /// Whether the program joins REPL inputs ([`collect`]).
    joined_inputs: bool,
    /// The pipeline's body-unit passes (`plan`).
    planner: &'a dyn Planner,
    /// Each body's plan, once per build: by item, and by self type for a
    /// compiler-supplied body.
    plans: HashMap<(DefId, Option<Ty>), Option<Arc<BodyPlan>>>,
}

#[derive(Default)]
struct WeakExactNode {
    bits: Vec<bool>,
    calls: Vec<(DefId, TyList)>,
}

/// A call its body's plan gives argument sources (`BodyPlan::spec_args`),
/// in the instance being scanned.
struct Site<'b> {
    body: &'b Body,
    /// The call instruction.
    at: usize,
    /// Its arguments' sources.
    sources: &'b [Option<Source>],
    /// Whether the call is in the scanned instance's own sub-body.
    own: bool,
    /// The scanned instance's known closures.
    known: &'b [(u32, Hash128)],
    /// The scanned instance's targets so far: the call's operands precede
    /// it, so theirs are in.
    calls: &'b HashMap<u32, Target>,
}

/// What collection rule 2 (specialization) does at a call.
enum Specialize {
    /// Call this copy.
    Copy(CallTarget),
    /// Call the plain instance.
    Plain,
    /// Record nothing (`Cx::copy_target`).
    Skip,
}

/// Collection stopped at an instantiation limit (`crates/hd_mono/DESIGN.md` §13.4):
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

/// The body of one call of `item` at `args` through `target`, a call
/// emission lowers in place (an intrinsic, a built-in family's method, a
/// trait method at a trait-value `Self`): its parameters are `item`'s, at
/// `args`, and its one `Call` takes them as its operands and returns the
/// result, so a function reference to such a target lowers as a direct
/// call at the same types does (§13.11). A suspending item's call is its
/// cold constructor, whose frame the body returns (`fn.ref.suspending`).
pub fn adapter_call_body(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    item: DefId,
    args: TyList,
    target: &Target,
) -> StageResult<Body> {
    use hd_tir::ir::{BodyKind, Providers, TirBuilder, TirSink, local_flags};
    let s = |t: Ty| subst(pool, env, item, args, t);
    let callee = match target {
        Target::Dyn => {
            let Some((trait_, _)) = env.parent(item) else {
                return err("a dynamic function reference to a method outside a trait");
            };
            let all = pool.list_items(args);
            let Some((&self_ty, targs)) = all.split_first() else {
                return err("a trait member reference without `Self`");
            };
            Callee::TraitMethod {
                trait_,
                method: item,
                self_ty,
                targs: pool.list(targs),
                choice: (ChoiceKind::TraitValue, 0),
            }
        }
        _ => Callee::Item {
            def: item,
            targs: args,
        },
    };
    let ret = s(env.ret(item).unwrap_or(Ty::VOID));
    let call_ty = if env.suspends(item) {
        pool.intern_ty(&TyData::Adt {
            def: env.suspend_def(),
            args: pool.list(&[ret]),
        })
    } else {
        ret
    };
    let none = NodeIdx::NONE;
    let mut tb = TirBuilder::new(item, BodyKind::Fn);
    let params: Vec<_> = env
        .params(item)
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let t = s(p);
            (
                tb.local(t, hd_base::Symbol::from_raw(0), local_flags::PARAM, none),
                t,
            )
        })
        .collect();
    let blk = tb.open_block();
    let operands: Vec<_> = params.iter().map(|&(l, t)| tb.get(l, t, none)).collect();
    let call = tb.call(&callee, &operands, Providers::None, call_ty, none);
    let root = tb.close_block(blk, Some(call), call_ty, none);
    tb.finish(root, &[])
        .map_err(|_| NotImplemented::bug(Stage::Collect, "a function reference's call body"))
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
    /// instance over a limit of `crates/hd_mono/DESIGN.md` §13.4 stops collection. `raw`
    /// is the request's type arguments before A1 classification: when a
    /// request re-enters an ancestor instance with a differently shaped
    /// request, the recursion grows concrete types through erased
    /// positions, which is polymorphic recursion
    /// (`types.generic.polymorphic-recursion`).
    fn push(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        raw: TyList,
        parent: InstId,
    ) -> StageResult<Hash128> {
        self.push_known(item, sub, args, raw, &[], parent)
    }

    /// [`Self::push`] of the instance specialized on `known` (option A2 of
    /// `crates/hd_opt/DESIGN.md` (known closure calls)), or of the plain one when `known` is empty.
    fn push_known(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        raw: TyList,
        known: &[(u32, Hash128)],
        parent: InstId,
    ) -> StageResult<Hash128> {
        self.push_specialized(item, sub, args, raw, known, &[], parent)
    }

    fn push_specialized(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        raw: TyList,
        known: &[(u32, Hash128)],
        defaults: &[(u32, defaults::Value)],
        parent: InstId,
    ) -> StageResult<Hash128> {
        let table = &self.out.table;
        if let Some(id) = table.get_with_defaults(item, sub, args, known, defaults) {
            let key = table.key[id.idx()];
            // Re-entering an ancestor instance with a differently shaped
            // request passes ever larger concrete types through
            // reference layouts that classify alike: each cycle wraps
            // again, so collection would never reach a limit on its own
            // (`types.generic.polymorphic-recursion`). An identical
            // request is ordinary recursion; a hit outside the
            // requester's ancestry is a shared instance, not a cycle.
            let changed = raw != table.args[id.idx()];
            let mut ancestor = parent;
            let mut cycle = false;
            while !cycle && ancestor != InstId::NONE {
                cycle = ancestor == id;
                ancestor = table.parent[ancestor.idx()];
            }
            if cycle && changed {
                return Err(self.stop_poly_recursion(item, raw, parent));
            }
            return Ok(key);
        }
        let table = &self.out.table;
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
        let key = defaults::key(self.key(item, sub, args), known, defaults);
        let depth = u8::try_from(depth).expect("within MAX_DEPTH");
        let (id, _) = self
            .out
            .table
            .push_with_defaults(item, sub, args, known, defaults, depth, parent, key);
        self.work.push(id);
        Ok(key)
    }

    /// Stage 8 (#1118): call `at` of `body` (instance `caller`'s) as a
    /// call of its callee's exclusive copy, where the callee has one, the
    /// target is its plain instance with no folded facts, and the
    /// arguments qualify (`hd_tir::own::exclusive_call`); else `t`.
    fn exclusive_target(
        &mut self,
        body: &Body,
        at: usize,
        t: CallTarget,
        caller: InstId,
    ) -> StageResult<CallTarget> {
        if t.kind != TargetKind::Instance {
            return Ok(t);
        }
        let Some((relies, copy)) = self.env.exclusive_body(t.item) else {
            return Ok(t);
        };
        let table = &self.out.table;
        let plain = table.get(t.item, Sub::Body(0), t.args);
        if plain.is_none_or(|p| table.key[p.idx()] != t.key) {
            return Ok(t);
        }
        let flag = hd_tir::ir::sub_flags::EXCLUSIVE;
        let own = match body.sub_flags.first() {
            Some(f) if f & flag != 0 => {
                let item = table.item[caller.idx()];
                self.env.exclusive_body(item).map_or(0, |e| e.0)
            }
            _ => 0,
        };
        if !hd_tir::own::exclusive_call(body, at, relies, own)
            || self.env.instance_body(t.item, t.args)?.is_some()
        {
            return Ok(t);
        }
        let mark = [defaults::EXCLUSIVE];
        let key =
            self.push_specialized(t.item, Sub::Body(0), t.args, t.args, &[], &mark, caller)?;
        let table = &self.out.table;
        if let Some(id) = table.get_with_defaults(t.item, Sub::Body(0), t.args, &[], &mark) {
            self.out.instance_bodies.entry(id).or_insert(copy);
        }
        Ok(CallTarget { key, ..t })
    }

    /// A direct call that omits a pure constant default can use a body whose
    /// constant decisions were folded before either collection or emission.
    fn default_target(
        &mut self,
        body: &Body,
        at: usize,
        def: DefId,
        targs: TyList,
        parent: InstId,
    ) -> StageResult<Option<CallTarget>> {
        if self.env.intrinsic(def).is_some() {
            return Ok(None);
        }
        let Some(callee) = self.env.body(def) else {
            return Ok(None);
        };
        let operands = body.record(body.data[at][1]);
        let values: Vec<_> = operands[..operands.len().saturating_sub(3)]
            .iter()
            .enumerate()
            .filter_map(|(p, &word)| {
                let i = hd_tir::ir::Ref(word).as_inst()?.0 as usize;
                if body.tags.get(i) != Some(&Tag::DefaultCall) {
                    return None;
                }
                let default = *body.record(body.data[i][0]).first()?;
                let value = defaults::constant(self.env.body(DefId::from_raw(default))?)?;
                Some((u32::try_from(p).expect("params"), value))
            })
            .collect();
        if values.is_empty() {
            return Ok(None);
        }
        let Some(transformed) = defaults::specialize(callee, &values) else {
            return Ok(None);
        };
        let own_from = self.env.parent(def).map_or(0, |p| p.1);
        let args = self.classify(def, self.pool.list_items(targs), own_from)?;
        let inst = rowless(self.pool, self.env, def, args);
        let raw = rowless(self.pool, self.env, def, targs);
        let key = self.push_specialized(def, Sub::Body(0), inst, raw, &[], &values, parent)?;
        if let Some(id) = self
            .out
            .table
            .get_with_defaults(def, Sub::Body(0), inst, &[], &values)
        {
            self.out
                .instance_bodies
                .entry(id)
                .or_insert_with(|| Arc::new(transformed));
        }
        let ret = self.env.ret(def).unwrap_or(Ty::VOID);
        Ok(Some(CallTarget {
            key,
            item: def,
            args,
            ret: subst(self.pool, self.env, def, args, ret),
            kind: TargetKind::Instance,
        }))
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
            TyData::Opaque { args, bound, .. } => {
                parts.extend(pool.list_items(args));
                parts.push(bound);
            }
            TyData::Tuple { elems, rest } => {
                parts.extend(pool.list_items(elems));
                parts.extend(rest);
            }
            TyData::Option(i) | TyData::Mut(i) | TyData::Ref { inner: i, .. } => parts.push(i),
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
            TyData::Row(r) | TyData::Context(r) => row_keys(r, &mut parts),
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Param(_)
            | TyData::Const(_)
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

    /// Records the stop at a limit (`crates/hd_mono/DESIGN.md` §13.4): the request of
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

    /// Records polymorphic recursion through reference layouts
    /// (`types.generic.polymorphic-recursion`): the request of `item`
    /// re-enters an ancestor instance with a differently shaped request,
    /// so concrete types grow through positions that classify alike and
    /// no depth limit trips. Same `instantiation-too-deep` error as
    /// [`Self::stop_too_deep`].
    fn stop_poly_recursion(&mut self, item: DefId, args: TyList, parent: InstId) -> NotImplemented {
        let table = &self.out.table;
        let mut ids = Vec::new();
        let mut p = parent;
        while p != InstId::NONE {
            ids.push(p);
            p = table.parent[p.idx()];
        }
        ids.reverse();
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
        let message = format!(
            "instantiating {last} does not end: its type arguments grow through reference layouts that classify alike; the chain: {}",
            shown.join(" -> ")
        );
        self.too_deep = Some(TooDeep {
            at: self.at,
            message,
        });
        NotImplemented::new(Stage::Collect, "instantiation-too-deep")
    }

    /// The least fixed point of generic parameters whose exact identity a
    /// reachable `weak`/`weak_mut` observes. Only direct weak calls seed the
    /// graph; ordinary generic calls propagate those seeds through their type
    /// arguments, so a recursion SCC with no weak call remains unchanged.
    fn weak_exact_params(&mut self, root: DefId) -> Vec<bool> {
        if let Some(bits) = self.weak_exact.get(&root) {
            return bits.clone();
        }
        let (pool, env) = (self.pool, self.env);
        let mut nodes: HashMap<DefId, WeakExactNode> = HashMap::default();
        let mut todo = vec![root];
        while let Some(def) = todo.pop() {
            if nodes.contains_key(&def) {
                continue;
            }
            let n = env.bounded(def).map_or(0, |b| b.len());
            let mut node = WeakExactNode {
                bits: vec![false; n],
                calls: Vec::new(),
            };
            if let Some(body) = env.body(def) {
                for i in 0..body.len() {
                    if body.tags[i] == Tag::DefaultCall {
                        let record = body.record(body.data[i][0]);
                        let (Some(&callee), Some(&targs)) = (record.first(), record.get(1)) else {
                            continue;
                        };
                        let callee = DefId::from_raw(callee);
                        if env.body(callee).is_some() {
                            node.calls.push((callee, TyList(targs)));
                            todo.push(callee);
                        }
                        continue;
                    }
                    if !matches!(body.tags[i], Tag::Call | Tag::Await) {
                        continue;
                    }
                    match Callee::from_words(body.record(body.data[i][0])) {
                        Some(Callee::Item { def: callee, targs }) => {
                            if env.intrinsic(callee).is_some_and(|key| {
                                matches!(key.as_str(), "weak_new" | "weak_new_mut")
                            }) {
                                for &t in pool.list_items(targs) {
                                    pinned(pool, env, def, t, true, &mut node.bits);
                                }
                            } else if env.body(callee).is_some() {
                                node.calls.push((callee, targs));
                                todo.push(callee);
                            }
                        }
                        Some(Callee::TraitMethod {
                            trait_,
                            method,
                            self_ty,
                            targs,
                            choice: (ChoiceKind::Impl, selected),
                        }) => {
                            let impl_ = DefId::from_raw(selected);
                            let items = pool.list_items(targs);
                            let n_trait = env.trait_arity(trait_).min(items.len());
                            let (callee, args) = if let Some(callee) =
                                env.impl_method(impl_, method)
                            {
                                let Some((head, heads, n)) = env.impl_head(impl_) else {
                                    continue;
                                };
                                let mut values = vec![None; n];
                                unify(pool, impl_, head, self_ty, &mut values);
                                for (&formal, &actual) in
                                    pool.list_items(heads).iter().zip(&items[..n_trait])
                                {
                                    unify(pool, impl_, formal, actual, &mut values);
                                }
                                let Some(mut args) = values.into_iter().collect::<Option<Vec<_>>>()
                                else {
                                    continue;
                                };
                                args.extend_from_slice(&items[n_trait..]);
                                (callee, pool.list(&args))
                            } else {
                                let mut args = vec![self_ty];
                                args.extend_from_slice(items);
                                (method, pool.list(&args))
                            };
                            if env.body(callee).is_some() {
                                node.calls.push((callee, args));
                                todo.push(callee);
                            }
                        }
                        _ => {}
                    }
                }
            }
            nodes.insert(def, node);
        }
        loop {
            let before: HashMap<DefId, Vec<bool>> = nodes
                .iter()
                .map(|(def, node)| (*def, node.bits.clone()))
                .collect();
            let mut changed = false;
            for (def, node) in &mut nodes {
                for (callee, targs) in &node.calls {
                    let Some(exact) = before.get(callee) else {
                        continue;
                    };
                    let old = node.bits.clone();
                    let own_from = env.parent(*callee).map_or(0, |(_, n)| n);
                    for (&t, &inside) in pool
                        .list_items(*targs)
                        .get(own_from..)
                        .unwrap_or_default()
                        .iter()
                        .zip(exact)
                    {
                        pinned(pool, env, *def, t, inside, &mut node.bits);
                    }
                    changed |= old != node.bits;
                }
            }
            if !changed {
                break;
            }
        }
        for (def, node) in nodes {
            self.weak_exact.insert(def, node.bits);
        }
        self.weak_exact.get(&root).cloned().unwrap_or_default()
    }

    /// A1's representation summary of `def` (`crates/hd_mono/DESIGN.md` §13.2): per own
    /// type parameter, whether a type of its signature or body holds the
    /// parameter where the layout depends on its exact representation.
    /// Only a value of the parameter itself, `mut`, an optional, and the
    /// erased storage of a `List`, `Map` or enum payload keep it movable:
    /// a data type's fields, a tuple, a function type, a trait value or a
    /// projection that mentions it change their Wasm type with it. A
    /// requirement key that mentions it (the item's own row, a function
    /// type's row, a `$.with` key) changes which provider the instance
    /// passes and reads, so it is exact too (`crates/hd_mono/DESIGN.md` §13.18).
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
                if matches!(b.tags[i], Tag::With | Tag::ContextNew) {
                    types.extend(b.record(b.data[i][0]).chunks(2).map(|c| Ty(c[0])));
                    continue;
                }
                if !matches!(b.tags[i], Tag::Call | Tag::Await) {
                    continue;
                }
                match Callee::from_words(b.record(b.data[i][0])) {
                    Some(Callee::Item { def: callee, targs }) => {
                        if env
                            .intrinsic(callee)
                            .is_some_and(|key| matches!(key.as_str(), "weak_new" | "weak_new_mut"))
                        {
                            for &t in pool.list_items(targs) {
                                pinned(pool, env, def, t, true, &mut out);
                            }
                        }
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
        for (dst, weak) in out.iter_mut().zip(self.weak_exact_params(def)) {
            *dst |= weak;
        }
        self.exact.insert(def, out.clone());
        out
    }

    /// A reference's permission affects identity and implementation selection,
    /// not its physical layout. Keep it when this body or a generic callee
    /// can observe it; otherwise share the same instance for both permissions.
    fn observes_permission(&mut self, def: DefId, visiting: &mut Vec<DefId>) -> bool {
        if let Some(&sensitive) = self.permission_sensitive.get(&def) {
            return sensitive;
        }
        if visiting.contains(&def) {
            return true;
        }
        let (pool, env) = (self.pool, self.env);
        let Some(body) = env.body(def) else {
            return true;
        };
        visiting.push(def);
        let sensitive = (0..body.len()).any(|i| match body.tags[i] {
            Tag::CallValue | Tag::AwaitValue | Tag::DefaultCall => true,
            Tag::Coerce => {
                let kind = body.record(body.data[i][1]).first().copied();
                matches!(kind, Some(k) if k == Coercion::ToTraitValue as u32 || k == Coercion::RefToTraitValue as u32)
                    && pool.has_param(value_ty(body, body.data[i][0]))
            }
            Tag::Call | Tag::Await => match Callee::from_words(body.record(body.data[i][0])) {
                Some(Callee::TraitMethod { self_ty, targs, .. }) => pool.has_param(self_ty) || pool.list_items(targs).iter().any(|t| pool.has_param(*t)),
                Some(Callee::Item { def: callee, targs }) if pool.list_items(targs).iter().any(|t| pool.has_param(*t)) => {
                    if let Some(key) = env.intrinsic(callee) {
                        matches!(
                            key.as_str(),
                            "type_id_of"
                                | "downcast_ref"
                                | "downcast_mut"
                                | "weak_new"
                                | "weak_new_mut"
                        )
                    } else { self.observes_permission(callee, visiting) }
                }
                _ => false,
            },
            _ => false,
        });
        visiting.pop();
        self.permission_sensitive.insert(def, sensitive);
        sensitive
    }

    fn shared_permission_type(&mut self, ty: Ty) -> Ty {
        if let Some(&shared) = self.permission_shared.get(&ty) {
            return shared;
        }
        let pool = self.pool;
        let cell = self.env.ref_cells();
        let mut list = |args: TyList| {
            let args: Vec<Ty> = pool
                .list_items(args)
                .iter()
                .map(|t| self.shared_permission_type(*t))
                .collect();
            pool.list(&args)
        };
        let data = match pool.get(ty) {
            TyData::Adt { def, args } if cell.holds(def) => {
                let inner = pool.list_items(args)[0];
                let inner = match pool.get(inner) {
                    TyData::Mut(inner) => inner,
                    _ => inner,
                };
                let inner = self.shared_permission_type(inner);
                TyData::Adt {
                    def,
                    args: pool.list(&[inner]),
                }
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(args),
            },
            TyData::Option(inner) => TyData::Option(self.shared_permission_type(inner)),
            TyData::Mut(inner) => TyData::Mut(self.shared_permission_type(inner)),
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(elems),
                rest: rest.map(|t| self.shared_permission_type(t)),
            },
            // A function, trait value or projection carries a signature or
            // dictionary whose selection can distinguish the permissions.
            _ => return ty,
        };
        let shared = pool.intern_ty(&data);
        self.permission_shared.insert(ty, shared);
        shared
    }

    /// A1 (`crates/hd_mono/DESIGN.md` §13.2): an unbounded, move-only argument of a
    /// reference layout is keyed and laid out as the class `REF`.
    fn classify(&mut self, def: DefId, args: &[Ty], own_from: usize) -> StageResult<TyList> {
        let bounded = self.env.bounded(def).unwrap_or_default();
        let pinned = self.exact_params(def);
        let shared_args: Vec<Ty> = args
            .iter()
            .map(|t| self.shared_permission_type(*t))
            .collect();
        let permission_sensitive =
            shared_args != args && self.observes_permission(def, &mut Vec::new());
        let mut a = Vec::new();
        for (k, &raw) in args.iter().enumerate() {
            let t = if permission_sensitive {
                raw
            } else {
                shared_args[k]
            };
            // A row argument is part of the instance (`crates/hd_mono/DESIGN.md` §12.4:
            // its keys are the instance's provider parameters).
            let exact = k < own_from
                || bounded.get(k - own_from).copied().unwrap_or(true)
                || pinned.get(k - own_from).copied().unwrap_or(false)
                || is_class_ref(self.pool, t)
                || matches!(self.pool.get(t), TyData::Row(_))
                || self.env.body(def).is_none()
                || holds_coll(self.pool, self.env, t);
            let shared = if exact {
                false
            } else {
                match self.env.shared_reference_layout(t) {
                    Some(shared) => shared,
                    None => a1_class(self.pool, self.env, t)? == A1Class::Ref,
                }
            };
            a.push(if shared { class_ref(self.pool) } else { t });
        }
        Ok(self.pool.list(&a))
    }

    /// The target of a call of `def` at full arguments `args`.
    fn target(&mut self, def: DefId, args: TyList, parent: InstId) -> StageResult<CallTarget> {
        let ret = self.env.ret(def).unwrap_or(Ty::VOID);
        if let Some(key) = self
            .env
            .intrinsic(def)
            .filter(|k| matches!(k.as_str(), "dbg" | "dbg_printer"))
        {
            return self.dbg_target(def, &key, args, parent);
        }
        // Every other intrinsic goes through the backend's lowering table,
        // even when its std declaration has a source body. That body is only
        // a placeholder: compiling it would turn an unlowered intrinsic into
        // a run-time `panic("intrinsic")` instead of a build error.
        if let Some(key) = self.env.intrinsic(def) {
            return Ok(CallTarget {
                key: Hash128(0),
                item: def,
                args,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Intrinsic(key),
            });
        }
        // A supplied tuple traversal calls its cached factory directly. Its
        // exact first argument selects the generated body; never A1-erase it.
        if self
            .pool
            .list_items(args)
            .first()
            .is_some_and(|&self_ty| self.out.supplied.contains_key(&(def, self_ty)))
        {
            let key = self.push(def, Sub::Body(0), args, args, parent)?;
            return Ok(CallTarget {
                key,
                item: def,
                args,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Instance,
            });
        }
        // A body-less inherent method is an intrinsic method of std
        // (`trait.impl.intrinsic`), such as `f64.sqrt`: the backend lowers
        // it per self type, the impl's head under the call's arguments.
        if self.env.body(def).is_none()
            && let Some((owner, _)) = self.env.parent(def)
            && let Some((head, _, _)) = self.env.impl_head(owner)
        {
            return Ok(CallTarget {
                key: Hash128(0),
                item: def,
                args: TyList::EMPTY,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Builtin {
                    method: String::new(),
                    self_ty: subst(self.pool, self.env, def, args, head),
                },
            });
        }
        if self.env.body(def).is_none() {
            return Err(NotImplemented::new(
                Stage::Collect,
                format!(
                    "a call of the body-less `{}`, which is not an intrinsic",
                    self.env.describe(def)
                ),
            ));
        }
        let own_from = self.env.parent(def).map_or(0, |p| p.1);
        let raw = args;
        let args = self.classify(def, self.pool.list_items(args), own_from)?;
        // One instance serves every row of the item's own row parameters
        // (`req.poly.one-body`, §12.4): it is keyed at the open row, and
        // the target keeps the call's rows, whose keys the caller passes
        // in the context.
        let (pool, env) = (self.pool, self.env);
        let key = self.push(
            def,
            Sub::Body(0),
            rowless(pool, env, def, args),
            rowless(pool, env, def, raw),
            parent,
        )?;
        Ok(CallTarget {
            key,
            item: def,
            args,
            ret: subst(self.pool, self.env, def, args, ret),
            kind: TargetKind::Instance,
        })
    }

    /// Selects the implementation of `trait_` for `self_ty` (`crates/hd_mono/DESIGN.md`
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
        // The sealed `Inspectable` is the compiler's at every concrete
        // type: its one method spells the type, so selection needs no
        // proof. Checking enforces where the language requires the bound
        // (`trait.inspectable.*`); a compiler-made lookup that the checker
        // exempts, a typed fact's read through a handle and its place in a
        // `Facts`, needs a runtime identity whatever its type arguments
        // are (`annot.handle.fact.key`, `annot.handle.fact.exact`).
        if choice.is_none() && trait_ == env.decls().sealed().inspectable {
            self.selected.insert(key, Picked::Builtin);
            return Ok(Picked::Builtin);
        }
        let view = env.impls();
        let at = if let Some(d) = choice {
            let Some(at) = env.impl_row(d) else {
                return err("a chosen impl outside the impl tables");
            };
            at
        } else {
            // Impl heads name `&T`, which bodies and declarations reach
            // here lowered to `Ref[T]`: select on the raised form. The
            // arguments below unify the lowered head with `self_ty`.
            let cell = env.ref_cells();
            let types = self.pool.types();
            let tref = ConcreteTraitRef(TraitRef {
                trait_,
                self_ty: types.raise_refs(cell, self_ty),
                args: types.list(
                    &types
                        .list_items(trait_args)
                        .iter()
                        .map(|a| types.raise_refs(cell, *a))
                        .collect::<Vec<_>>(),
                ),
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

    /// Collects the JSON codecs of the private-field types in the entry
    /// rows' provider signatures, before the work queue scans: each host
    /// capability method's arguments cross out of hd and its result
    /// crosses in (`module.boundary.direction`), so an argument's private
    /// types need `encode` and a result's need `decode` plus its checked
    /// unwrap. Checking already reports a missing trait at the signature;
    /// selection here keeps the codec instances reachable for emission.
    /// A trait value a result brings in is a host handle, whose methods
    /// cross the same boundary.
    fn note_boundary_codecs(&mut self, roots: &[(DefId, TyList)]) -> StageResult<()> {
        let mut visited = HashSet::default();
        let mut keys = Vec::new();
        for (def, args) in roots {
            keys.extend(
                self.env
                    .row_keys(*def, *args)
                    .into_iter()
                    .map(|key| (key, false)),
            );
        }
        while let Some((key, package)) = keys.pop() {
            if let Some(bridge) = self.env.host_bridge(key) {
                keys.push((bridge.wire, package));
                continue;
            }
            let TyData::TraitValue {
                def: trait_,
                args: targs,
                ..
            } = self.pool.get(key)
            else {
                continue;
            };
            let package = package || self.env.is_package_host_trait(trait_);
            if !visited.insert((trait_, targs, package)) {
                continue;
            }
            let full = self.pool.list(
                &std::iter::once(class_ref(self.pool))
                    .chain(self.pool.list_items(targs).iter().copied())
                    .collect::<Vec<_>>(),
            );
            for &parent in self.env.decls().supertraits(trait_) {
                keys.push((subst(self.pool, self.env, trait_, full, parent), package));
            }
            for m in self.env.trait_methods(trait_) {
                let params = self.env.params(m).unwrap_or_default();
                for p in params.into_iter().skip(1) {
                    let p = subst(self.pool, self.env, m, full, p);
                    self.note_boundary_value(p, true, package, &mut Vec::new())?;
                }
                let ret = self.env.ret(m).unwrap_or(Ty::VOID);
                let ret = subst(self.pool, self.env, m, full, ret);
                self.note_boundary_value(ret, false, package, &mut keys)?;
            }
        }
        Ok(())
    }

    /// Records the JSON codec a private-field type in a boundary value
    /// needs: `encode` when `out` (the value leaves hd), `decode` plus
    /// its checked unwrap otherwise. A public wrapper still needs the
    /// codecs of the private types inside it. The trait values inside it
    /// go to `handles`.
    fn note_boundary_value(
        &mut self,
        ty: Ty,
        out: bool,
        package: bool,
        handles: &mut Vec<(Ty, bool)>,
    ) -> StageResult<()> {
        let mut todo = vec![ty];
        let mut seen = HashSet::default();
        while let Some(ty) = todo.pop() {
            let mut ty = ty;
            while let TyData::Mut(inner) = self.pool.get(ty) {
                ty = inner;
            }
            if !seen.insert(ty) {
                continue;
            }
            match self.pool.get(ty) {
                // A trait value reference to a host handle crosses as the
                // handle, which `host_provider` puts behind a new object.
                TyData::Adt { def, args } if self.env.ref_cells().holds(def) => {
                    if let [inner] = self.pool.list_items(args)[..]
                        && !out
                        && is_trait_value(self.pool, inner)
                        && let Some(json) = self.env.json_items()
                    {
                        // The wrapper builds a handle value. Reference permission
                        // stays in the boundary key, not its payload type.
                        let inner = match self.pool.get(inner) {
                            TyData::Mut(inner) => inner,
                            _ => inner,
                        };
                        let (provider, value) = self
                            .env
                            .host_bridge(inner)
                            .map_or((json.host_provider, inner), |bridge| {
                                (bridge.adapter, bridge.wire)
                            });
                        let at = self.pool.list(&[value]);
                        let wrap = self.target(provider, at, InstId::NONE)?;
                        self.out.boundary.entry(ty).or_default().handle_ref = Some(wrap.key);
                    }
                    todo.extend(self.pool.list_items(args).iter().copied());
                }
                TyData::Adt { def, args } => {
                    if self.env.has_private_field(def) {
                        self.note_private_boundary(ty, out, package)?;
                    }
                    todo.extend(self.pool.list_items(args).iter().copied());
                    for field in self.env.data_fields(def).unwrap_or_default() {
                        todo.push(subst(self.pool, self.env, def, args, field));
                    }
                    if let Some(variants) = self.env.enum_declared(def) {
                        for variant in variants {
                            for field in variant {
                                todo.push(subst(self.pool, self.env, def, args, field));
                            }
                        }
                    }
                }
                TyData::Option(inner) => todo.push(inner),
                TyData::Tuple { elems, rest } => {
                    todo.extend(self.pool.list_items(elems).iter().copied());
                    todo.extend(rest);
                }
                TyData::TraitValue { .. } => handles.push((ty, package)),
                _ => {}
            }
        }
        Ok(())
    }

    fn note_private_boundary(&mut self, ty: Ty, out: bool, package: bool) -> StageResult<()> {
        let Some(json) = self.env.json_items() else {
            return err("a host boundary without its JSON codecs");
        };
        if out {
            self.select(json.serialize, ty, TyList::EMPTY, None)?;
            let def = if package {
                json.package_encode
            } else {
                json.encode
            };
            let encode = self.target(def, self.pool.list(&[ty]), InstId::NONE)?;
            let entry = self.out.boundary.entry(ty).or_default();
            if package {
                entry.package_encode = Some(encode.key);
            } else {
                entry.encode = Some(encode.key);
            }
        } else {
            self.select(json.deserialize, ty, TyList::EMPTY, None)?;
            let decode = self.target(
                if package {
                    json.package_decode
                } else {
                    json.decode
                },
                self.pool.list(&[ty]),
                InstId::NONE,
            )?;
            let TyData::Adt { args, .. } = self.pool.get(decode.ret) else {
                return err("a boundary `decode` without its `Result`");
            };
            let is_ok = self.target(json.result_is_ok, args, InstId::NONE)?;
            let expect = self.target(json.result_expect, args, InstId::NONE)?;
            let entry = self.out.boundary.entry(ty).or_default();
            let decoder = Some(JsonDecode {
                decode: decode.key,
                is_ok: is_ok.key,
                expect: expect.key,
            });
            if package {
                entry.package_decode = decoder;
                entry.package_decoded = Some(decode.ret);
            } else {
                entry.decode = decoder;
                entry.decoded = Some(decode.ret);
            }
        }
        Ok(())
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
            let Some(bodies) = self.env.supplied_body(method, self_ty)? else {
                return Ok(None);
            };
            // Register the whole family before any body is scanned: directories
            // call field/variant factories, which also have no source body.
            for body in bodies {
                self.out
                    .supplied
                    .entry((body.item, self_ty))
                    .or_insert_with(|| Arc::new(body));
            }
        }
        let args = self.pool.list(&[&[self_ty], targs].concat());
        let key = self.push(method, Sub::Body(0), args, args, parent)?;
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

    /// A call of `dbg_printer` or `dbg`, whose body the compiler writes at the
    /// exact argument (`ProgramEnv::dbg_body`): an instance keyed like any
    /// other, with no A1 class, since the body reads the type's declaration.
    fn dbg_target(
        &mut self,
        def: DefId,
        key: &str,
        args: TyList,
        parent: InstId,
    ) -> StageResult<CallTarget> {
        let Some(&ty) = self.pool.list_items(args).first() else {
            return err("a `dbg` function without its type argument");
        };
        if !self.out.supplied.contains_key(&(def, ty)) {
            // A class placeholder or a type parameter has no declaration to
            // print: its text is implementation-defined (`module.dbg.value.generic`).
            let debug = key == "dbg_printer" && self.implements_debug(ty);
            let body = self.env.dbg_body(def, key, ty, debug)?;
            self.out.supplied.insert((def, ty), Arc::new(body));
        }
        let key = self.push(def, Sub::Body(0), args, args, parent)?;
        Ok(CallTarget {
            key,
            item: def,
            args,
            ret: subst(
                self.pool,
                self.env,
                def,
                args,
                self.env.ret(def).unwrap_or(Ty::VOID),
            ),
            kind: TargetKind::Instance,
        })
    }

    /// Whether the concrete `ty` implements `Debug`, bounds included.
    fn implements_debug(&mut self, ty: Ty) -> bool {
        if is_class_ref(self.pool, ty) {
            return false;
        }
        // A tuple's `Debug` is a template that checks its elements only when
        // it is instantiated, so the elements decide here.
        if let TyData::Tuple { elems, rest } = self.pool.get(ty) {
            let parts: Vec<Ty> = self
                .pool
                .list_items(elems)
                .iter()
                .copied()
                .chain(rest)
                .collect();
            return parts.into_iter().all(|t| self.implements_debug(t));
        }
        let debug = self.env.debug_trait();
        self.select(debug, ty, TyList::EMPTY, None).is_ok()
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

    /// `t` with an outer nominal newtype erased to its base, keeping an
    /// outer `mut`: `None` when `t` is no newtype. Derived-newtype
    /// adapters forward to the base implementation, so with erased
    /// newtypes calls use the base method directly (no adapter
    /// instance).
    fn erase_self_ty(&self, t: Ty) -> Option<Ty> {
        let pool = self.pool;
        let (inner, is_mut) = match pool.get(t) {
            TyData::Mut(i) => (i, true),
            _ => (t, false),
        };
        let base = crate::layout::strip_newtype(pool, self.env, inner);
        (base != inner).then(|| {
            if is_mut {
                pool.intern_ty(&TyData::Mut(base))
            } else {
                base
            }
        })
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
        // A derived implementation on a nominal newtype forwards to the
        // base type's implementation: erase and select there, collecting
        // no adapter instance. Hand-written methods on newtypes keep
        // their bodies.
        if self.env.is_derived_newtype_impl(impl_)
            && let Some(base) = self.erase_self_ty(self_ty)
        {
            return self.method_target(trait_, method, base, targs, None, parent);
        }
        let found = self.env.impl_method(impl_, method);
        match found {
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
    /// (`crates/hd_mono/DESIGN.md` §13.5, §13.16): a target per method slot, then each
    /// direct supertrait's vtable at the same type, its arguments
    /// substituted with `Self` as `from`. Each slot's key joins `reps`.
    fn vtable(
        &mut self,
        trait_: DefId,
        trait_args: TyList,
        from: Ty,
        through: bool,
        parent: InstId,
        reps: &mut StableHasher,
    ) -> StageResult<VTable> {
        let (pool, env) = (self.pool, self.env);
        let targs = pool.list_items(trait_args);
        let cell = env.ref_cells();
        let is_ref = |t: Ty| matches!(pool.get(t), TyData::Adt { def, .. } if cell.holds(def));
        let takes_ref = |d: DefId| {
            env.params(d)
                .and_then(|ps| ps.first().copied())
                .is_some_and(is_ref)
        };
        // A payload that is a reference to `S` dispatches to `S`'s own
        // implementation (`RefToTraitValue`), except for a sealed trait,
        // which the compiler answers at the reference type itself.
        let inner = match pool.get(from) {
            TyData::Adt { def, args } if through && cell.holds(def) => {
                pool.list_items(args).first().copied()
            }
            _ => None,
        }
        .filter(|_| !env.decls().sealed().supplies(trait_));
        if inner.is_some() {
            reps.u8(1);
        }
        let mut slots = Vec::new();
        let mut recv = Vec::new();
        for m in env.trait_methods(trait_) {
            let (t, r) = match inner {
                Some(s) => {
                    // A generic method's slot holds its erased body (§13.5.1).
                    let t = if erased::own_count(env, m) > 0 {
                        self.erased_target(trait_, m, s, targs, parent)?
                    } else {
                        self.method_target(trait_, m, s, targs, None, parent)?
                    };
                    // The slot ABI follows the trait declaration. An impl
                    // for a reference can lower a value receiver to that
                    // reference type without making the slot a reference
                    // receiver.
                    let r = if takes_ref(m) {
                        SlotRecv::Payload
                    } else {
                        SlotRecv::ReadThrough
                    };
                    (t, r)
                }
                None if takes_ref(m) => {
                    return err("a reference-receiver vtable without a reference payload");
                }
                None if erased::own_count(env, m) > 0 => (
                    self.erased_target(trait_, m, from, targs, parent)?,
                    SlotRecv::Payload,
                ),
                None => (
                    self.method_target(trait_, m, from, targs, None, parent)?,
                    SlotRecv::Payload,
                ),
            };
            reps.hash(t.key);
            if r != SlotRecv::Payload {
                reps.u8(r as u8);
            }
            slots.push(t);
            recv.push(r);
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
            parents.push(self.vtable(def, args, from, through, parent, reps)?);
        }
        Ok(VTable {
            trait_,
            slots,
            recv,
            parents,
        })
    }

    /// The parameters of `def` a known value is worth specializing on, by
    /// its plan as a callee (`BodyPlan::spec_params`).
    fn spec_params(&mut self, def: DefId) -> Option<Vec<bool>> {
        let body = self.env.body(def)?;
        self.plan(def, None, body)?.spec_params.clone()
    }

    /// How many specializations nest at `id`: 0 for a plain instance, else
    /// one more than at the instance that requested it. A sub-body that
    /// carries its instance's known closures (rule 3) is part of that
    /// instance, not one more.
    fn copy_depth(&self, mut id: InstId) -> u32 {
        let table = &self.out.table;
        let mut d = 0;
        while id != InstId::NONE && !table.known[id.idx()].is_empty() {
            d += u32::from(table.sub[id.idx()] == Sub::Body(0));
            id = table.parent[id.idx()];
        }
        d
    }

    /// Collection rule 2 of `crates/hd_opt/DESIGN.md` (optimization pipelines) §2.2, specialization, at the
    /// call `at.at` of `def`: the instance of `def` specialized on the
    /// known closures the call passes for the parameters the callee's plan
    /// marks (`spec_params`). An argument is a known closure when its
    /// source is an instruction whose target is a closure's code instance,
    /// or a parameter the scanned instance is specialized on: a copy that
    /// passes it to itself finds itself. Its type must be the parameter's
    /// at the copy's arguments. A copy past the planner's `spec_depth`, or
    /// no known closure, leaves the plain instance.
    ///
    /// A call outside the instance's own sub-body that passes a parameter
    /// on is skipped: this instance never emits it, and the sub-body that
    /// does may be a copy that resolves it to itself.
    fn copy_target(
        &mut self,
        at: &Site<'_>,
        def: DefId,
        targs: TyList,
        id: InstId,
    ) -> StageResult<Specialize> {
        let Some(marked) = self.spec_params(def) else {
            return Ok(Specialize::Plain);
        };
        let (pool, env) = (self.pool, self.env);
        let params = env.params(def).unwrap_or_default();
        let body = at.body;
        let rec = body.record(body.data[at.at][1]);
        let args = &rec[..rec.len().saturating_sub(3)];
        if args.len() != marked.len()
            || params.len() != marked.len()
            || at.sources.len() != args.len()
        {
            return Ok(Specialize::Plain);
        }
        let mut found = Vec::new();
        for (p, (&w, &source)) in args.iter().zip(at.sources).enumerate() {
            if !marked[p] {
                continue;
            }
            match source {
                Some(Source::Made(m)) => {
                    if let Some(&Target::Closure(k)) = at.calls.get(&m) {
                        found.push((p, k, w));
                    }
                }
                Some(Source::Param(q)) => {
                    if !at.own {
                        return Ok(Specialize::Skip);
                    }
                    if let Some(&(_, k)) = at.known.iter().find(|k| k.0 == q) {
                        found.push((p, k, w));
                    }
                }
                None => {}
            }
        }
        if found.is_empty() {
            return Ok(Specialize::Plain);
        }
        let own_from = env.parent(def).map_or(0, |p| p.1);
        let cargs = self.classify(def, pool.list_items(targs), own_from)?;
        let (item, iargs) = (self.out.table.item[id.idx()], self.out.table.args[id.idx()]);
        let known: Vec<(u32, Hash128)> = found
            .into_iter()
            .filter(|&(p, _, w)| {
                subst(pool, env, def, cargs, params[p])
                    == subst(pool, env, item, iargs, value_ty(body, w))
            })
            .map(|(p, k, _)| (u32::try_from(p).expect("params"), k))
            .collect();
        if known.is_empty()
            || (self
                .out
                .table
                .get_known(def, Sub::Body(0), rowless(pool, env, def, cargs), &known)
                .is_none()
                && self.copy_depth(id) >= self.planner.spec_depth())
        {
            return Ok(Specialize::Plain);
        }
        let inst = rowless(pool, env, def, cargs);
        let key = self.push_known(def, Sub::Body(0), inst, inst, &known, id)?;
        let ret = env.ret(def).unwrap_or(Ty::VOID);
        // As in `target`: keyed at the open row, called at the call's rows.
        Ok(Specialize::Copy(CallTarget {
            key,
            item: def,
            args: cargs,
            ret: subst(pool, env, def, cargs, ret),
            kind: TargetKind::Instance,
        }))
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

    /// What an item body's code reads of its own signature, which its TIR
    /// does not hold: its parameter and result layouts, whether it
    /// suspends, and its row's keys, which are its provider parameters
    /// (`crates/hd_mono/DESIGN.md` §12.4). Two programs may give one item path one body
    /// under two rows, as `fn read() -> i32 $ Clock: 42` and
    /// `fn read() -> i32 $ Clock + Logger: 42`, and must not share code.
    fn hash_signature(&mut self, item: DefId, args: TyList, reps: &mut StableHasher) {
        let (pool, env) = (self.pool, self.env);
        let s = |t: Ty| subst(pool, env, item, args, t);
        let mut types: Vec<Ty> = env.params(item).unwrap_or_default();
        types.extend(env.ret(item));
        for t in types {
            reps.hash(layout_hash(
                pool,
                env,
                s(t),
                &mut self.canons,
                &mut self.layouts,
            ));
        }
        reps.u8(u8::from(env.suspends(item)));
        // A row parameter's context is a parameter too.
        reps.u8(u8::from(has_row_params(env, item)));
        let ph = |d: DefId| env.path_hash(d);
        for k in env.row_keys(item, args) {
            reps.hash(canon(pool, &ph, &mut self.canons, k));
        }
    }

    /// A function reference's adapter (`crates/hd_mono/DESIGN.md` §13.11): its one call,
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
                // At a trait-value `Self`, the adapter dispatches through
                // the receiver's table (trait.dyn.bound.reference-dispatch).
                let value_ty = match pool.get(self_ty) {
                    TyData::Adt { def, args } if env.ref_cells().holds(def) => {
                        pool.list_items(args)[0]
                    }
                    _ => self_ty,
                };
                if is_trait_value(pool, value_ty) {
                    reps.hash(env.path_hash(item));
                    self.record_pair(trait_, item, targs)?;
                    self.out.dyn_methods.insert(item);
                    Target::Dyn
                } else {
                    Target::Call(self.method_target(trait_, item, self_ty, targs, None, id)?)
                }
            }
            _ => Target::Call(self.target(item, args, id)?),
        };
        if let Target::Call(c) = &t {
            self.hash_target(c, &mut reps);
        }
        // A target emission lowers at each call is called from a body of
        // one `Call` (§13.11), which goes through the ownership passes as
        // every other body does.
        if !matches!(&t, Target::Call(c) if c.kind == TargetKind::Instance) {
            let body = adapter_call_body(pool, env, item, args, &t)?;
            let body = env.own_generated(body)?;
            self.out.instance_bodies.insert(id, Arc::new(body));
        }
        Sub::Adapter.hash_into(&mut reps);
        if self.out.calls.len() <= id.idx() {
            self.out.calls.resize_with(id.idx() + 1, HashMap::default);
            self.out.callee_reps.resize(id.idx() + 1, Hash128(0));
        }
        self.out.calls[id.idx()] = HashMap::from_iter([(0, t)]);
        self.out.callee_reps[id.idx()] = reps.finish();
        Ok(())
    }

    /// The key operations of a map of type `map` (`crates/hd_mono/DESIGN.md` §13.12):
    /// the keyed bucket hash of an inline key, else `key_bucket_hash` and
    /// `Eq.eq` at the key type, selected here so the TIR carries only the
    /// key type.
    fn map_key(&mut self, map: Ty, parent: InstId, reps: &mut StableHasher) -> StageResult<Target> {
        let pool = self.pool;
        let mut map = map;
        loop {
            map = match pool.get(map) {
                TyData::Mut(x) => x,
                TyData::Adt { def, args } if self.env.ref_cells().holds(def) => {
                    pool.list_items(args)[0]
                }
                _ => break,
            };
        }
        let TyData::Adt { args, .. } = pool.get(map) else {
            return err("a map operation on a non-map type");
        };
        let Some(&key) = pool.list_items(args).first() else {
            return err("a map type without a key");
        };
        if layout::inline_map_key(pool, self.env, key) {
            let stripped = layout::strip_newtype(pool, self.env, key);
            let string = matches!(pool.get(stripped), TyData::Prim(Prim::String));
            let hash = self.target(self.env.inline_bucket_hash(string), TyList::EMPTY, parent)?;
            self.hash_target(&hash, reps);
            return Ok(Target::MapInline(hash));
        }
        let Some((eq_trait, eq_method, key_hash)) = self.env.map_key_items() else {
            return err("an `Eq` trait without its method");
        };
        let eq = self.method_target(eq_trait, eq_method, key, &[], None, parent)?;
        let hash = self.target(key_hash, pool.list(&[key]), parent)?;
        self.hash_target(&eq, reps);
        self.hash_target(&hash, reps);
        Ok(Target::MapKey { hash, eq })
    }

    /// The plan of `body`, the body of `def` or the one supplied for it at
    /// self type `supplied`: asked of the planner once per build.
    fn plan(&mut self, def: DefId, supplied: Option<Ty>, body: &Body) -> Option<Arc<BodyPlan>> {
        let (planner, env) = (self.planner, self.env);
        self.plans
            .entry((def, supplied))
            .or_insert_with(|| planner.plan(def, body, env))
            .clone()
    }

    fn scan(&mut self, id: InstId) -> StageResult<()> {
        let (item, sub, args) = (
            self.out.table.item[id.idx()],
            self.out.table.sub[id.idx()],
            self.out.table.args[id.idx()],
        );
        match sub {
            Sub::Adapter => return self.scan_adapter(id),
            Sub::Body(_) | Sub::Erased | Sub::Thunk(_) => {}
        }
        let pool = self.pool;
        let env = self.env;
        // A compiler-supplied method's instance has no item body: its
        // TIR was generated at its self type (`supplied_target`).
        let supplied = pool
            .list_items(args)
            .first()
            .and_then(|&t| Some((t, self.out.supplied.get(&(item, t))?.clone())));
        if supplied.is_none()
            && !self.out.instance_bodies.contains_key(&id)
            && let Some(body) = env.instance_body(item, args)?
        {
            self.out.instance_bodies.insert(id, Arc::new(body));
        }
        let instance_body = self.out.instance_bodies.get(&id).cloned();
        let Some(body) = instance_body
            .as_deref()
            .or_else(|| supplied.as_ref().map(|s| &*s.1))
            .or(env.body(item))
        else {
            return err("an instance whose item has no TIR");
        };
        let mut calls = HashMap::default();
        let mut reps = StableHasher::new("callee-reps");
        let mut seen = hd_base::HashSet::default();
        let s = |t: Ty| subst(pool, env, item, args, t);
        // An erased body outlines its open instructions, and a thunk is
        // one of them at concrete arguments (`crates/hd_mono/DESIGN.md` §13.5.1).
        let mut open: HashMap<u32, u32> = HashMap::default();
        let mut only = None;
        match sub {
            Sub::Erased => {
                for (k, ix) in open_insts(pool, env, body, item, args)?
                    .into_iter()
                    .enumerate()
                {
                    open.insert(ix, u32::try_from(k).expect("open instructions"));
                }
                let key = self.out.table.key[id.idx()];
                let Some(&family) = self.witness.family_of.get(&key) else {
                    return err("an erased body outside every witness family");
                };
                reps.hash(family);
                calls.insert(INFO, Target::Erased { family });
            }
            Sub::Thunk(k) => {
                let erased = erased_args(pool, env, item, args);
                let opens = open_insts(pool, env, body, item, erased)?;
                let Some(&ix) = opens.get(usize::from(k)) else {
                    return err("a thunk past its erased body's open instructions");
                };
                only = Some(ix as usize);
                reps.u32(ix);
                // Its signature is the erased body's view of its operands
                // and result: open ones cross as `eqref`.
                for w in thunk_operands(body, ix).into_iter().chain([ix]) {
                    let t = value_ty(body, w);
                    reps.u8(u8::from(is_open(pool, subst(pool, env, item, erased, t))));
                }
                for (local, _) in thunk_capture_locals(body, ix) {
                    let t = body.local_ty[local as usize];
                    reps.u8(u8::from(is_open(pool, subst(pool, env, item, erased, t))));
                }
                calls.insert(INFO, Target::Thunk { at: ix });
            }
            Sub::Body(_) | Sub::Adapter => {}
        }
        let hidden = hidden_insts(body, sub);
        // A thunk emits one instruction and takes no plan.
        let plan = if instance_body.is_some() {
            self.planner.plan(item, body, env)
        } else if only.is_none() {
            self.plan(item, supplied.as_ref().map(|s| s.0), body)
        } else {
            None
        };
        let known = self.out.table.known[id.idx()].clone();
        // Each instruction's sub-body, where rules 2 and 3 ask whether an
        // instruction is in this instance's own sub-body.
        let subs = (plan.as_ref().is_some_and(|p| !p.spec_args.is_empty()) || !known.is_empty())
            .then(|| body.inst_subs());
        for i in 0..body.len() {
            if only.is_some_and(|o| o != i) || hidden.as_ref().is_some_and(|h| h[i]) {
                continue;
            }
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
            if let Some(&k) = open.get(&ix) {
                calls.insert(ix, Target::Open(k));
                continue;
            }
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
                            let targs = pool.list(&targs);
                            if let Some(target) = self.default_target(body, i, def, targs, id)? {
                                self.hash_target(&target, &mut reps);
                                calls.insert(ix, Target::Call(target));
                                continue;
                            }
                            let sources = plan.as_ref().and_then(|p| p.spec_args_at(ix));
                            let copy = match (sources, &subs) {
                                (Some(sources), Some(subs)) => {
                                    let at = Site {
                                        body,
                                        at: i,
                                        sources,
                                        own: match sub {
                                            Sub::Body(k) => u32::from(k) == subs[i],
                                            _ => true,
                                        },
                                        known: &known,
                                        calls: &calls,
                                    };
                                    self.copy_target(&at, def, targs, id)?
                                }
                                _ => Specialize::Plain,
                            };
                            match copy {
                                Specialize::Copy(t) => t,
                                Specialize::Plain => self.target(def, targs, id)?,
                                Specialize::Skip => continue,
                            }
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
                            // (an intrinsic member): its arguments are
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
                            // A call at a trait-value receiver goes
                            // through the value's table, whatever the
                            // checker chose: a bound at `T = dyn Tr`
                            // included (trait.dyn.bound.reference-dispatch).
                            let value_ty = match pool.get(self_ty) {
                                TyData::Adt { def, args } if env.ref_cells().holds(def) => {
                                    pool.list_items(args)[0]
                                }
                                _ => self_ty,
                            };
                            if is_trait_value(pool, value_ty) {
                                reps.hash(env.path_hash(method));
                                self.record_pair(trait_, method, &targs)?;
                                self.out.dyn_methods.insert(method);
                                calls.insert(ix, Target::Dyn);
                                continue;
                            }
                            let private_structure = env
                                .dbg_structure()
                                .is_some_and(|(caller, tr)| caller == item && tr == trait_);
                            let supplied = match choice.0 {
                                _ if private_structure => {
                                    self.supplied_target(method, self_ty, &targs, id)?
                                }
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
                                    ChoiceKind::TraitValue => {
                                        return err("a trait-value choice at a concrete receiver");
                                    }
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
                                self.method_target(trait_, method, self_ty, &targs, pick, id)?
                            }
                        }
                    };
                    if matches!(&t.kind, TargetKind::Intrinsic(key) if key == "map_probe") {
                        let Some(&map) = body.record(b).first() else {
                            return err("a map probe without its map");
                        };
                        let target = self.map_key(s(value_ty(body, map)), id, &mut reps)?;
                        calls.insert(ix, target);
                        continue;
                    }
                    let t = self.exclusive_target(body, i, t, id)?;
                    self.hash_target(&t, &mut reps);
                    calls.insert(ix, Target::Call(t));
                }
                // An omitted argument (`crates/hd_mono/DESIGN.md` §13.2 step 6, §13.13): the
                // default body's instance at the call's type arguments.
                Tag::DefaultCall => {
                    let (Some(&d), Some(&l)) = (body.record(a).first(), body.record(a).get(1))
                    else {
                        return err("a malformed default call record");
                    };
                    let def = DefId::from_raw(d);
                    if let Some(bits) = env.body(def).and_then(scalar_constant) {
                        reps.u8(2);
                        reps.u64(bits);
                        calls.insert(ix, Target::ConstDefault(bits));
                        continue;
                    }
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
                    let inst = body.is_instantiation(i);
                    // A generic local function's defining closure makes
                    // no value; each use instantiates it (`sub_flags::GENERIC`).
                    if body.sub_is_generic(usize::from(sub_k)) && !inst {
                        continue;
                    }
                    let targs = if inst {
                        // Its own parameters follow the instance's: the
                        // arguments before them are this instance's, those
                        // of sibling local functions' slots unused.
                        let rec = body.record(b);
                        let owner = env.generics_owner(item);
                        let base = env.parent(owner).map_or(0, |p| p.1) + rec[2] as usize;
                        let cur = pool.list_items(args);
                        let mut v: Vec<Ty> = (0..base)
                            .map(|j| cur.get(j).copied().unwrap_or(Ty::VOID))
                            .collect();
                        v.extend(pool.list_items(TyList(rec[3])).iter().copied().map(s));
                        pool.list(&v)
                    } else {
                        args
                    };
                    // Collection rule 3 of `crates/hd_opt/DESIGN.md` (optimization pipelines) §2.2, carrying:
                    // a closure that a specialized instance creates carries
                    // the known closures for the parameters its plan lists,
                    // so those calls are direct there too. One outside the
                    // instance's own sub-body is never emitted here: the
                    // instance that emits it decides what it carries.
                    let carried: Vec<(u32, Hash128)> = match &subs {
                        Some(subs) if !known.is_empty() => {
                            if !matches!(sub, Sub::Body(k) if u32::from(k) == subs[i]) {
                                continue;
                            }
                            let listed = plan.as_ref().map_or(&[][..], |p| p.carry_at(ix));
                            known
                                .iter()
                                .filter(|&&(p, _)| listed.contains(&p))
                                .copied()
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    let defaults = self.out.table.defaults[id.idx()].clone();
                    let key = self.push_specialized(
                        item,
                        Sub::Body(sub_k),
                        targs,
                        targs,
                        &carried,
                        &defaults,
                        id,
                    )?;
                    if let Some(body) = &instance_body
                        && let Some(child) = self.out.table.get_with_defaults(
                            item,
                            Sub::Body(sub_k),
                            targs,
                            &carried,
                            &defaults,
                        )
                    {
                        self.out
                            .instance_bodies
                            .entry(child)
                            .or_insert_with(|| body.clone());
                    }
                    calls.insert(ix, Target::Closure(key));
                }
                // A function reference is a closure whose code is the
                // adapter instance of the item at its substituted type
                // arguments (`crates/hd_mono/DESIGN.md` §13.11).
                Tag::ItemRef => {
                    let (Some(&d), Some(&l)) = (body.record(a).first(), body.record(b).first())
                    else {
                        return err("a malformed function reference record");
                    };
                    let targs: Vec<Ty> =
                        pool.list_items(TyList(l)).iter().copied().map(s).collect();
                    let pushed = pool.list(&targs);
                    let key = self.push(DefId::from_raw(d), Sub::Adapter, pushed, pushed, id)?;
                    calls.insert(ix, Target::Closure(key));
                }
                Tag::Coerce => {
                    let rec = body.record(b);
                    let through = rec.first().copied() == Some(Coercion::RefToTraitValue as u32);
                    if !through && rec.first().copied() != Some(Coercion::ToTraitValue as u32) {
                        continue;
                    }
                    let from = s(value_ty(body, a));
                    // A trait value converted to a trait value, as at
                    // `T = dyn Error` erased to `dyn Inspectable`, is a
                    // widening: no implementation, no vtable
                    // (trait.erase.keeps, trait.dyn.widen).
                    let mut value_ty = from;
                    if through {
                        loop {
                            match pool.get(value_ty) {
                                TyData::Mut(inner) => value_ty = inner,
                                TyData::Adt { def, args } if env.ref_cells().holds(def) => {
                                    value_ty = pool.list_items(args)[0];
                                }
                                _ => break,
                            }
                        }
                    }
                    let sealed = env.decls().sealed();
                    let sealed_identity = matches!(pool.get(ty), TyData::TraitValue { def, .. } if def == sealed.inspectable || def == sealed.any);
                    if is_trait_value(pool, from)
                        || ((!sealed_identity || trait_reference_widens(pool, env, from, ty))
                            && is_trait_value(pool, value_ty))
                    {
                        continue;
                    }
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
                    // A `dyn Any` is laid out as a `dyn Inspectable`, so its
                    // vtable is `Inspectable`'s canonical dynamic key.
                    let sealed = env.decls().sealed();
                    let (trait_, trait_args) =
                        if trait_ == sealed.any && sealed.inspectable != DefId::NONE {
                            (sealed.inspectable, TyList::EMPTY)
                        } else {
                            (trait_, trait_args)
                        };
                    let vt = self.vtable(trait_, trait_args, from, through, id, &mut reps)?;
                    calls.insert(ix, Target::VTable(vt));
                }
                // A map's key operations (`crates/hd_mono/DESIGN.md` §13.12): a literal
                // inserts each entry, and lookup, insertion and removal
                // hash and compare the key through its impls.
                Tag::NewMap if !body.record(b).is_empty() => {
                    let t = self.map_key(ty, id, &mut reps)?;
                    calls.insert(ix, t);
                }
                Tag::Intrinsic
                    if matches!(
                        IntrinsicOp::from_u32(a),
                        Some(
                            IntrinsicOp::MapGet
                                | IntrinsicOp::MapSet
                                | IntrinsicOp::MapRemove
                                | IntrinsicOp::MapEntry
                        )
                    ) =>
                {
                    let Some(&m) = body.record(b).first() else {
                        return err("a map operation without its map");
                    };
                    let t = self.map_key(s(value_ty(body, m)), id, &mut reps)?;
                    calls.insert(ix, t);
                }
                _ => {}
            }
        }
        if let Some(plan) = &plan {
            direct_calls(body, plan, sub, hidden.as_deref(), &known, &mut calls);
        }
        if sub == Sub::Body(0) {
            self.hash_signature(item, args, &mut reps);
        }
        sub.hash_into(&mut reps);
        if self.out.calls.len() <= id.idx() {
            self.out.calls.resize_with(id.idx() + 1, HashMap::default);
            self.out.callee_reps.resize(id.idx() + 1, Hash128(0));
        }
        self.out.calls[id.idx()] = calls;
        self.out.callee_reps[id.idx()] = reps.finish();
        Ok(())
    }
}

/// The instructions an instance of `sub` does not reach: those inside a
/// generic local function's body (`sub_flags::GENERIC`) that is neither
/// `sub` nor a sub enclosing it, so the instance's arguments bind none of
/// that function's own type parameters. `None` when the body has no
/// generic local function.
fn hidden_insts(body: &Body, sub: Sub) -> Option<Vec<bool>> {
    let n = body.sub_root.len();
    if !(0..n).any(|k| body.sub_is_generic(k)) {
        return None;
    }
    let subs = body.inst_subs();
    // A sub's parent is the sub holding its defining closure, opened
    // before it, so it has the smaller number.
    let mut parent = vec![0usize; n];
    for (i, &s) in subs.iter().enumerate() {
        if body.tags[i] == Tag::Closure && !body.is_instantiation(i) {
            let k = body.data[i][0] as usize;
            if k < n && (s as usize) < k {
                parent[k] = s as usize;
            }
        }
    }
    let k = match sub {
        Sub::Body(k) => usize::from(k).min(n - 1),
        _ => 0,
    };
    let mut visible = vec![false; n];
    let mut c = k;
    loop {
        visible[c] = true;
        if c == 0 {
            break;
        }
        c = parent[c];
    }
    let hidden_sub: Vec<bool> = (0..n)
        .map(|s| {
            let mut c = s;
            loop {
                if body.sub_is_generic(c) && !visible[c] {
                    return true;
                }
                if c == 0 {
                    return false;
                }
                c = parent[c];
            }
        })
        .collect();
    Some(
        subs.iter()
            .map(|&s| hidden_sub.get(s as usize).copied().unwrap_or(false))
            .collect(),
    )
}

/// Collection rule 1 of `crates/hd_opt/DESIGN.md` (optimization pipelines) §2.2, direct calls: each
/// `CallValue` the plan gives a callee source records
/// `Target::KnownClosure` when that source is a known closure in the
/// instance being scanned. A `Made` source is one when its instruction's
/// target is a closure's code instance. A `Param` source is one, in the
/// instance's own sub-body `sub`, when `known` holds that parameter.
fn direct_calls(
    body: &Body,
    plan: &BodyPlan,
    sub: Sub,
    hidden: Option<&[bool]>,
    known: &[(u32, Hash128)],
    calls: &mut HashMap<u32, Target>,
) {
    let subs = plan
        .direct
        .iter()
        .any(|d| matches!(d.1, Source::Param(_)))
        .then(|| body.inst_subs());
    for &(ix, source) in &plan.direct {
        let i = ix as usize;
        if body.tags.get(i) != Some(&Tag::CallValue)
            || hidden.is_some_and(|h| h[i])
            || calls.contains_key(&ix)
        {
            continue;
        }
        let key = match source {
            Source::Made(m) => match calls.get(&m) {
                Some(&Target::Closure(key)) => key,
                _ => continue,
            },
            Source::Param(p) => {
                let own = subs
                    .as_ref()
                    .is_some_and(|s| matches!(sub, Sub::Body(k) if u32::from(k) == s[i]));
                match known.iter().find(|k| k.0 == p) {
                    Some(&(_, key)) if own => key,
                    _ => continue,
                }
            }
        };
        calls.insert(ix, Target::KnownClosure(key));
    }
}

/// Whether a body may run other code: a call of any form, or a map
/// operation that hashes and compares its key through impls (§13.12). A
/// body that does not can never reach `block_on`, so its `DefaultCall`
/// needs no forbidden-context bracket (`crates/hd_mono/DESIGN.md` §12.3).
/// The bits of a default body that is one scalar constant: a root block
/// with no instruction whose value is a constant other than a string
/// (`crates/hd_mono/DESIGN.md` §13.13).
fn scalar_constant(body: &Body) -> Option<u64> {
    let root = *body.sub_root.first()? as usize;
    if body.tags.get(root) != Some(&Tag::Block) || !body.record(body.data[root][0]).is_empty() {
        return None;
    }
    let tail = hd_tir::ir::Ref(body.data[root][1]);
    if tail.as_inst().is_some() || tail.0 == hd_tir::ir::NONE {
        return None;
    }
    let &(ty, bits) = body
        .consts
        .get((tail.0 & !hd_tir::ir::Ref::CONST_BIT) as usize)?;
    (ty != Ty::STRING).then_some(bits)
}

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
                    | IntrinsicOp::MapRemove
                    | IntrinsicOp::MapEntry
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
                layout::StdKind::List | layout::StdKind::Map | layout::StdKind::Buffer
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
        TyData::Row(row) | TyData::Context(row) => {
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
    if let Some((def, _)) = pool.adt(t)
        && env.data_fields(def).is_some()
        && out.insert(DefIdOrd(env.path_hash(def), def.raw()))
    {
        for f in env.data_fields(def).unwrap_or_default() {
            note_data(pool, env, f, out);
        }
    }
}

/// Collection from `root` (`crates/hd_mono/DESIGN.md` §13.1) and the init bodies `inits`
/// of the groups it reaches, in initialization order: every reachable
/// instance, each call's target, the imports and data types; or the stop
/// at an instantiation limit. With `joined_inputs` (a REPL session's
/// program, every kept input joined), each init body is also a root of the
/// host boundary, since each input's init runs with the entry's providers.
/// `refs`
/// are function references the module holds though its code takes none:
/// each item's adapter instance at its type arguments, as a REPL
/// session's prelude holds them for later inputs (`f1c55020:future-work/compiler/repl-plan.md` job C8).
/// `planner` holds the pipeline's body-unit passes, whose plans
/// collection applies (`plan`).
pub fn collect(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    solver: &dyn Solver,
    root: DefId,
    inits: &[DefId],
    extra: &[(DefId, TyList)],
    refs: &[(DefId, TyList)],
    joined_inputs: bool,
    planner: &dyn Planner,
) -> StageResult<Result<Collected, TooDeep>> {
    collect_roots(
        pool,
        env,
        solver,
        Some(root),
        inits,
        extra,
        refs,
        joined_inputs,
        planner,
    )
}

/// Collects development fixture bodies at their supplied types, without an
/// executable entry or host-boundary codec discovery. `roots` may be empty;
/// their keys are returned in [`Collected::extra`]. Ordinary calls collect
/// named generic instantiations and closures as in [`collect`].
pub fn collect_fixtures(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    solver: &dyn Solver,
    inits: &[DefId],
    roots: &[(DefId, TyList)],
    planner: &dyn Planner,
) -> StageResult<Result<Collected, TooDeep>> {
    collect_roots(pool, env, solver, None, inits, roots, &[], false, planner)
}

fn collect_roots(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    solver: &dyn Solver,
    root: Option<DefId>,
    inits: &[DefId],
    extra: &[(DefId, TyList)],
    refs: &[(DefId, TyList)],
    joined_inputs: bool,
    planner: &dyn Planner,
) -> StageResult<Result<Collected, TooDeep>> {
    let mut cx = Cx {
        pool,
        env,
        solver,
        penv: ParamEnv::default(),
        memo: BodyMemo::default(),
        selected: HashMap::default(),
        exact: HashMap::default(),
        weak_exact: HashMap::default(),
        permission_sensitive: HashMap::default(),
        permission_shared: HashMap::default(),
        canons: CanonMemo::default(),
        layouts: HashMap::default(),
        depths: HashMap::default(),
        at: None,
        too_deep: None,
        out: Collected::default(),
        work: Vec::new(),
        witness: erased::WitnessCx::default(),
        joined_inputs,
        planner,
        plans: HashMap::default(),
    };
    if let Err(e) = cx.run(root, inits, extra, refs) {
        return match cx.too_deep.take() {
            Some(t) => Ok(Err(t)),
            None => Err(e),
        };
    }
    let mut out = cx.out;
    out.witnesses = cx.witness.finish();
    out.calls.resize_with(out.table.len(), HashMap::default);
    out.callee_reps.resize(out.table.len(), Hash128(0));
    Ok(Ok(out))
}

impl Cx<'_> {
    fn run(
        &mut self,
        root: Option<DefId>,
        inits: &[DefId],
        extra: &[(DefId, TyList)],
        refs: &[(DefId, TyList)],
    ) -> StageResult<()> {
        if let Some(root) = root {
            self.push(
                root,
                Sub::Body(0),
                TyList::EMPTY,
                TyList::EMPTY,
                InstId::NONE,
            )?;
        }
        for i in inits {
            let k = self.push(*i, Sub::Body(0), TyList::EMPTY, TyList::EMPTY, InstId::NONE)?;
            self.out.inits.push(k);
        }
        // Extra roots with type arguments: the test cases after the first
        // and the `std.rt` result functions (`docs/compiler/engines-and-test-runner.md`
        // §19.1).
        for (d, args) in extra {
            let t = self.target(*d, *args, InstId::NONE)?;
            self.out.extra.push(t.key);
        }
        for (d, args) in refs {
            self.push(*d, Sub::Adapter, *args, *args, InstId::NONE)?;
        }
        if let Some(root) = root {
            let mut boundary_roots = vec![(root, TyList::EMPTY)];
            boundary_roots.extend(extra.iter().copied());
            // A session's joined REPL inputs each run their init with the
            // entry's providers, so each init body is a root of the boundary
            // too.
            if self.joined_inputs {
                boundary_roots.extend(inits.iter().map(|d| (*d, TyList::EMPTY)));
            }
            self.note_boundary_codecs(&boundary_roots)?;
        }
        // Breadth-first in push order (`crates/hd_mono/DESIGN.md` §13.4): the first
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
