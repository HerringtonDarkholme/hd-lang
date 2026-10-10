//! Value layouts at concrete types (wasm-layout.md §15.1, §15.2): the Wasm
//! values of every hd type in locals, parameters and results, and the
//! shapes emission needs to build and read them. The classes agree with
//! `hd_mono::layout::layout_of` (the value bound, slot sharing, `T?`
//! forms), and add the struct descriptors.
//!
//! A type's values are built from its parts' values, except where storage
//! erases a reference to `eqref` (an enum's slots and box, a list's or a
//! map's arrays), which reads only the parts' storage (`Flat`). So a
//! recursive type's Wasm types recur only through data fields and trait
//! values, possibly by way of tuples, optionals, functions and
//! suspensions. Those two are the nominal types. `lay_out` finds the
//! nominal types a type reaches with Tarjan's algorithm (an explicit
//! stack), and closes each strongly connected set as one recursion group
//! (§15.3), its members in canonical order, after the groups it names; a
//! nominal type outside any cycle is a group of one. Enums held by value
//! are laid out innermost first (`hd_mono::layout::value_enums`). So no
//! walk here is deeper than tuples, optionals and functions nest, however
//! long a chain of types is.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use hd_base::{DefId, StableHasher, StageResult};
use hd_mono::layout::{CanonMemo, LayoutEnv, StdKind, canon, sccs, value_enums};
use hd_mono::{ProgramEnv, class_ref, is_class_ref, key_order, subst};
use hd_types::{InternPool, ParamRef, Prim, Ty, TyData, TyList};

use crate::{Group, VT, WTy, unsupported};

/// The value bound (§15.1).
pub const BOUND: usize = 4;

/// An enum box is flat when its variants' payload fields number at most
/// this many in total, and subtypes otherwise (§15.2).
pub const FLAT_FIELDS: usize = 4;

/// A function value's provider context (codegen.md §12.4, one callable
/// ABI): every closure code takes, after its arguments, the key ids and
/// the providers of the caller's row for it, so a value of any narrower
/// row looks its own keys up and row subsumption is no instruction.
#[must_use]
pub fn ctx_keys() -> WTy {
    WTy::Array(VT::I64)
}

/// The providers of a context: per key, its value and its vtable.
#[must_use]
pub fn ctx_provs() -> WTy {
    WTy::Array(VT::Eq)
}

/// The id a context gives a key: the low bits of its content hash, which
/// covers the trait's arguments and bindings, so `Repo[User]` and
/// `Repo[Post]` have two ids (codegen.md §13.18).
#[must_use]
pub fn key_id(pool: &InternPool, env: &dyn ProgramEnv, k: Ty) -> i64 {
    let ph = |d: DefId| env.path_hash(d);
    let bytes = canon(pool, &ph, &mut CanonMemo::default(), k)
        .0
        .to_le_bytes();
    i64::from_le_bytes(bytes[..8].try_into().unwrap_or_default())
}

/// A path through a vtable's parent fields to a supertrait's vtable
/// (codegen.md §13.16): each step's vtable type and parent field, then
/// the supertrait's vtable key.
pub(crate) type SuperPath = (Vec<(WTy, u32)>, Ty);

/// A trait-value call's dispatch ([`Lay::dyn_slot`]): the receiver's
/// vtable type, the parent-field steps to the declaring trait's vtable,
/// that vtable's type, the method's slot, the slot's function type and
/// the row keys whose providers it takes last ([`Lay::slot_keys`]).
pub(crate) struct DynSlot {
    pub value_vt: WTy,
    pub steps: Vec<(WTy, u32)>,
    pub vt: WTy,
    pub slot: u32,
    pub sig: WTy,
    pub keys: Vec<Ty>,
}

/// What emission reads about the program's types.
pub struct Lay<'a> {
    pub pool: &'a InternPool,
    pub env: &'a dyn ProgramEnv,
    pub path: &'a dyn Fn(DefId) -> String,
    /// The program's laid-out nominal types, shared by its instances.
    pub shared: &'a Layouts,
    rec: RefCell<Rec>,
}

/// Nominal types' values shared by every `Lay` of one program build, so a
/// recursion group is laid out once rather than once per instance that
/// names it. Keys are the run's interned types.
#[derive(Default)]
pub struct Layouts(std::sync::Mutex<HashMap<Ty, Vec<VT>>>);

/// An enum's value layout: a tag, then shared payload slots (§15.1).
#[derive(Clone, Debug)]
pub struct EnumShape {
    /// Storage of each shared slot after the tag (references as `eqref`).
    pub slots: Vec<VT>,
    /// Per variant, per payload field: its slot indices and exact values.
    /// Under a subtype box, the indices are the field's struct fields in
    /// the variant's subtype, less one (the tag).
    pub fields: Vec<Vec<(Vec<usize>, Vec<VT>)>>,
    /// The box past the bound or of a self-recursive enum: tag and slots
    /// as one immutable struct, or the subtypes' base.
    pub boxed: Option<WTy>,
    /// A subtype box: per variant, its final subtype of `boxed` (tag, then
    /// the payload's exact values), none for a payloadless variant.
    /// Empty otherwise.
    pub subtypes: Vec<Option<WTy>>,
}

/// The `T?` forms (§15.2 rule 1).
#[derive(Clone, Debug)]
pub enum OptShape {
    /// A nullable reference; null is `.None`.
    NullRef(VT),
    /// `string?`: `(ref null $bytes, i64)`.
    NullStr,
    /// `(i32 tag, dflt(T)...)`.
    Tagged(Vec<VT>),
    /// Past the bound: a nullable box of `dflt(T)`.
    Boxed(WTy, Vec<VT>),
}

#[derive(Clone, Debug)]
pub enum Shape {
    Void,
    Scalar(VT),
    Str,
    ClassRef,
    Data {
        ty: WTy,
        /// Per field: its first struct field and its values.
        fields: Vec<(u32, Vec<VT>)>,
    },
    Enum(EnumShape),
    Opt(OptShape, Vec<VT>),
    Tuple {
        elems: Vec<Vec<VT>>,
        boxed: Option<WTy>,
    },
    List {
        elem: Vec<VT>,
        ty: WTy,
    },
    Map {
        key: Vec<VT>,
        val: Vec<VT>,
        ty: WTy,
    },
    Fn {
        base: WTy,
        code: WTy,
    },
    Dyn {
        trait_: DefId,
        args: TyList,
        vt: WTy,
        /// The vtable key (`Lay::dyn_key`).
        key: Ty,
    },
    Suspend {
        base: WTy,
        poll: WTy,
        result: Vec<VT>,
    },
}

/// Storage of a value in a shared slot or an array element: references
/// erased to `eqref` (A1, §15.2 "Arrays"), string bytes kept nullable.
#[must_use]
pub fn storage(v: &VT) -> VT {
    match v {
        VT::Ref(t, _) if **t == WTy::Bytes => VT::rn(WTy::Bytes),
        VT::Ref(..) => VT::Eq,
        x => x.clone(),
    }
}

/// The closure base struct of a code signature (§15.2 "closure").
#[must_use]
pub fn closure_base(code: &WTy) -> WTy {
    WTy::Struct {
        fields: vec![VT::r(code.clone())],
        sup: None,
        open: true,
    }
}

/// Fields of every suspension (suspension.md §14.1): the cancel function,
/// the resume state, the flags; then, per result layout, the poll
/// function; then, in a generated frame, the awaited child.
pub const F_CANCEL: u32 = 0;
pub const F_STATE: u32 = 1;
pub const F_FLAGS: u32 = 2;
pub const F_POLL: u32 = 3;
pub const F_CHILD: u32 = 4;
/// The first saved field of a generated frame.
pub const F_SAVED: u32 = 5;

/// Frame flags (suspension.md §14.1).
pub const ACTIVE: i32 = 1;
pub const DONE: i32 = 4;
pub const CANCELLED: i32 = 8;

/// The cancel function's type: `(frame) -> ()`, one for every layout.
#[must_use]
pub fn cancel_fn() -> WTy {
    WTy::Func(vec![VT::Eq], vec![])
}

/// `$Task`: the layout-independent prefix of every suspension, through
/// which a parent cancels a child of any result layout.
#[must_use]
pub fn task_base() -> WTy {
    WTy::Struct {
        fields: vec![VT::rn(cancel_fn()), VT::I32, VT::I32],
        sup: None,
        open: true,
    }
}

/// `$Suspend_L` (suspension.md §14.1) and its poll function's type
/// `(frame) -> (i32 ready, dflt(L)...)`. The competing-driver field is
/// not laid out yet.
#[must_use]
pub fn suspend_base(result: &[VT]) -> (WTy, WTy) {
    let mut res = vec![VT::I32];
    res.extend(result.iter().map(VT::dflt));
    let poll = WTy::Func(vec![VT::Eq], res);
    (
        WTy::Struct {
            fields: vec![VT::rn(cancel_fn()), VT::I32, VT::I32, VT::rn(poll.clone())],
            sup: Some(Box::new(task_base())),
            open: true,
        },
        poll,
    )
}

/// A suspension type over a base, with extra fields (a generated frame,
/// a host leaf, a combinator frame).
#[must_use]
pub fn frame_of(base: &WTy, extra: &[VT]) -> WTy {
    let mut fields = base.fields();
    fields.extend_from_slice(extra);
    WTy::Struct {
        fields,
        sup: Some(Box::new(base.clone())),
        open: false,
    }
}

/// The struct for erased values that are not one reference: an immutable
/// box of their values (§13.5.1, "As an open value").
#[must_use]
pub fn box_of(vts: &[VT]) -> WTy {
    WTy::Struct {
        fields: vts.to_vec(),
        sup: None,
        open: false,
    }
}

/// A list: `{len, one array per element value}` (structure of arrays).
#[must_use]
pub fn list_ty(elem: &[VT]) -> WTy {
    let mut fields = vec![VT::I32];
    fields.extend(elem.iter().map(|v| VT::r(WTy::Array(storage(v).dflt()))));
    WTy::Struct {
        fields,
        sup: None,
        open: false,
    }
}

/// A map's fields (codegen.md §13.12, representation-runtime.md §7.2):
/// the live entry count, the written entry count (tombstones included),
/// the probe index (entry number plus one, 0 empty, twice the entry
/// capacity, a power of two), the entry hashes (-1 marks a removed
/// entry), then one array per key value and per map value.
pub const M_LIVE: u32 = 0;
pub const M_USED: u32 = 1;
pub const M_INDEX: u32 = 2;
pub const M_HASHES: u32 = 3;
pub const M_KEYS: u32 = 4;

/// A map: `{live, used, index, hashes, key arrays, value arrays}`, its
/// entries in the order they were added.
#[must_use]
pub fn map_ty(key: &[VT], val: &[VT]) -> WTy {
    let ints = VT::r(WTy::Array(VT::I32));
    let mut fields = vec![VT::I32, VT::I32, ints.clone(), ints];
    for v in key.iter().chain(val) {
        fields.push(VT::r(WTy::Array(storage(v).dflt())));
    }
    WTy::Struct {
        fields,
        sup: None,
        open: false,
    }
}

/// The base of an enum's subtype box (§15.2): the tag, extended by one
/// final subtype per payload variant.
#[must_use]
pub fn enum_base() -> WTy {
    WTy::Struct {
        fields: vec![VT::I32],
        sup: None,
        open: true,
    }
}

/// A Wasm value as storage reads it: a reference to a built type is only
/// its nullability, since storage erases it (`storage`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Flat {
    /// A scalar, `eqref`, or string bytes.
    V(VT),
    /// A reference to a struct, array or function type.
    Ref(bool),
}

impl Flat {
    fn storage(&self) -> VT {
        match self {
            Flat::V(v) => storage(v),
            Flat::Ref(_) => VT::Eq,
        }
    }
    fn dflt(&self) -> Flat {
        match self {
            Flat::V(v) => Flat::V(v.dflt()),
            Flat::Ref(_) => Flat::Ref(true),
        }
    }
}

/// Which `T?` form an inner layout takes (§15.2 rule 1): `one_ref` when it
/// is one non-null reference or `eqref`, `string` for `string`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OptKind {
    NullRef,
    NullStr,
    Tagged,
    Boxed,
}

fn opt_kind(len: usize, one_ref: bool, string: bool) -> OptKind {
    if len == 1 && one_ref {
        OptKind::NullRef
    } else if string {
        OptKind::NullStr
    } else if len < BOUND {
        OptKind::Tagged
    } else {
        OptKind::Boxed
    }
}

/// An enum's value layout: the shared slots, per variant and payload field
/// its slots, and the box.
struct EnumForm {
    slots: Vec<VT>,
    at: Vec<Vec<Vec<usize>>>,
    boxed: EnumBox,
}

/// An enum's box (§15.2).
enum EnumBox {
    None,
    Flat(WTy),
    Sub(WTy),
}

/// The nominal types: their values given their heap type.
#[derive(Clone, Copy, Debug)]
enum Nominal {
    /// `(ref $T)`.
    Data,
    /// A trait value: `(eqref, (ref $VT))`.
    Dyn,
}

impl Nominal {
    fn vts(self, heap: WTy) -> Vec<VT> {
        match self {
            Nominal::Data => vec![VT::r(heap)],
            Nominal::Dyn => vec![VT::Eq, VT::r(heap)],
        }
    }
}

/// What layout has found so far, per `Lay`.
#[derive(Default)]
struct Rec {
    /// Laid-out nominal types: their values.
    done: HashMap<Ty, Vec<VT>>,
    /// The groups being built, innermost last: each member's position.
    /// A heap type naming a member holds `Back(position)` until the
    /// group closes.
    building: Vec<HashMap<Ty, u32>>,
    /// Per item, whether its declarations expand without bound.
    expansive: HashMap<DefId, bool>,
    /// Enum value layouts, by enum type.
    forms: HashMap<Ty, Arc<EnumForm>>,
}

/// A nominal type to lay out: its type, kind and item.
type Node = (Ty, Nominal, DefId);

/// The state of `Lay::lay_out`'s walk (Tarjan's algorithm): per visited
/// type, by visit number, its node, low link and whether it is on
/// `stack`; and the explicit call stack.
#[derive(Default)]
struct Walk {
    nodes: Vec<Node>,
    low: Vec<usize>,
    on: Vec<bool>,
    index: HashMap<Ty, usize>,
    stack: Vec<usize>,
    call: Vec<Frame>,
    /// Items on the current path, with how many of their types.
    path: HashMap<DefId, u32>,
}

/// A visited type and its successors, the next to visit at `next`.
struct Frame {
    node: usize,
    succ: Vec<Node>,
    next: usize,
}

/// Calls `f` with each `Back` index a type names, not looking into
/// closed groups.
fn each_back(t: &WTy, f: &mut dyn FnMut(u32)) {
    let vt = |v: &VT, f: &mut dyn FnMut(u32)| {
        if let VT::Ref(t, _) = v {
            each_back(t, f);
        }
    };
    match t {
        WTy::Back(i) => f(*i),
        WTy::Array(v) => vt(v, f),
        WTy::Struct { fields, sup, .. } => {
            for x in fields {
                vt(x, f);
            }
            if let Some(s) = sup {
                each_back(s, f);
            }
        }
        WTy::Func(p, r) => {
            for x in p.iter().chain(r) {
                vt(x, f);
            }
        }
        WTy::Bytes | WTy::Rec(..) => {}
    }
}

pub(crate) fn has_back(t: &WTy) -> bool {
    let mut any = false;
    each_back(t, &mut |_| any = true);
    any
}

/// Calls `f` with each type `t` references directly (fields, elements,
/// parameters, results, supertype).
fn each_ref(t: &WTy, f: &mut dyn FnMut(&WTy)) {
    let vt = |v: &VT, f: &mut dyn FnMut(&WTy)| {
        if let VT::Ref(t, _) = v {
            f(t);
        }
    };
    match t {
        WTy::Array(v) => vt(v, f),
        WTy::Struct { fields, sup, .. } => {
            for x in fields {
                vt(x, f);
            }
            if let Some(s) = sup {
                f(s);
            }
        }
        WTy::Func(p, r) => {
            for x in p.iter().chain(r) {
                vt(x, f);
            }
        }
        WTy::Bytes | WTy::Rec(..) | WTy::Back(_) => {}
    }
}

/// The types inside `t` (not `t` itself, not `Back`) that name a member
/// of the group being closed: they are on its cycle, so they join it.
fn inner_members(t: &WTy, out: &mut BTreeSet<WTy>) {
    each_ref(t, &mut |c| {
        if !matches!(c, WTy::Back(_)) && has_back(c) {
            inner_members(c, out);
            out.insert(c.clone());
        }
    });
}

/// A type's content key: the hash of its content, other groups by theirs.
fn content_key(t: &WTy) -> Vec<u8> {
    let mut h = StableHasher::new("wasm-rec-member");
    t.digest(&mut h);
    h.finish().0.to_be_bytes().to_vec()
}

impl<'a> Lay<'a> {
    #[must_use]
    pub fn new(
        pool: &'a InternPool,
        env: &'a dyn ProgramEnv,
        path: &'a dyn Fn(DefId) -> String,
        shared: &'a Layouts,
    ) -> Lay<'a> {
        Lay {
            pool,
            env,
            path,
            shared,
            rec: RefCell::default(),
        }
    }

    /// Records a group's types' values, here and for the program: the
    /// whole group under one lock, so another instance sees all of it or
    /// none.
    /// A type another instance laid out first keeps that instance's
    /// values (equal ones), so every copy shares one descriptor and
    /// compares by pointer.
    fn record(&self, laid: Vec<(Ty, Vec<VT>)>) {
        let laid: Vec<(Ty, Vec<VT>)> = match self.shared.0.lock() {
            Ok(mut s) => laid
                .into_iter()
                .map(|(t, v)| (t, s.entry(t).or_insert(v).clone()))
                .collect(),
            Err(_) => laid,
        };
        self.rec.borrow_mut().done.extend(laid);
    }

    pub(crate) fn strip(&self, t: Ty) -> Ty {
        match self.pool.get(t) {
            TyData::Mut(i) => self.strip(i),
            _ => t,
        }
    }

    /// A trait value's vtable key (codegen.md §13.5, "As built (#193)"):
    /// the trait, its arguments and the binding of every associated type
    /// the trait reaches, so a slot naming `Self::Item` has a concrete
    /// signature (trait.dyn.binding.signatures). The value type's own
    /// bindings join those its supertrait list fixes
    /// (trait.binding.super.meaning), so `dyn PriceFeed` and
    /// `dyn PriceFeed[Item = i32]` under `PriceFeed < Feed[Item = i32]`
    /// share one key. `t` is a trait value type, readonly or `mut`.
    pub(crate) fn dyn_key(&self, t: Ty) -> Ty {
        let pool = self.pool;
        let TyData::TraitValue {
            def,
            args,
            mut bindings,
        } = pool.get(self.strip(t))
        else {
            return t;
        };
        let reach = self.reached_assocs(def);
        bindings.retain(|(a, _)| reach.contains(a));
        for s in self.super_keys(def, args, &bindings) {
            if let TyData::TraitValue { bindings: sb, .. } = pool.get(s) {
                for (a, b) in sb {
                    if !bindings.iter().any(|(x, _)| *x == a) {
                        bindings.push((a, b));
                    }
                }
            }
        }
        pool.intern_ty(&TyData::TraitValue {
            def,
            args,
            bindings,
        })
    }

    /// The vtable key of a trait at arguments with no bindings written: a
    /// requirement key's or a host trait's.
    pub fn trait_key(&self, trait_: DefId, args: TyList) -> Ty {
        self.dyn_key(self.pool.intern_ty(&TyData::TraitValue {
            def: trait_,
            args,
            bindings: Vec::new(),
        }))
    }

    /// The associated types a trait declares or reaches through its
    /// supertraits (trait.binding.name-reach).
    fn reached_assocs(&self, trait_: DefId) -> Vec<DefId> {
        let mut out = Vec::new();
        let mut todo = vec![trait_];
        let mut seen = std::collections::HashSet::new();
        while let Some(t) = todo.pop() {
            if !seen.insert(t) {
                continue;
            }
            out.extend(self.env.trait_assocs(t));
            for s in self.env.decls().supertraits(t) {
                if let TyData::TraitValue { def, .. } = self.pool.get(*s) {
                    todo.push(def);
                }
            }
        }
        out
    }

    /// A trait value's direct supertraits' keys, in declared order: each
    /// supertrait at the value's arguments (`Self` being the class `REF`),
    /// with the supertrait list's bindings and those of `bindings` that
    /// the supertrait reaches (trait.dyn.binding.widen).
    fn super_keys(&self, trait_: DefId, args: TyList, bindings: &[(DefId, Ty)]) -> Vec<Ty> {
        let (pool, env) = (self.pool, self.env);
        let full = self.dyn_args(args);
        let mut out = Vec::new();
        for s in env.decls().supertraits(trait_) {
            let TyData::TraitValue {
                def,
                args: a,
                bindings: mut sb,
            } = pool.get(subst(pool, env, trait_, full, *s))
            else {
                continue;
            };
            let reach = self.reached_assocs(def);
            for (x, b) in bindings {
                if reach.contains(x) && !sb.iter().any(|(y, _)| y == x) {
                    sb.push((*x, *b));
                }
            }
            let sb = sb
                .into_iter()
                .map(|(x, b)| (x, self.at_value(bindings, b)))
                .collect();
            out.push(self.dyn_key(pool.intern_ty(&TyData::TraitValue {
                def,
                args: a,
                bindings: sb,
            })));
        }
        out
    }

    /// A trait method's declared type at a trait value: each projection
    /// of the class `REF` (`Self::Item`) read from the value's bindings
    /// (trait.dyn.binding.signatures).
    fn at_value(&self, bindings: &[(DefId, Ty)], t: Ty) -> Ty {
        let pool = self.pool;
        if !pool.has_assoc(t) {
            return t;
        }
        let list = |l: TyList| {
            pool.list(
                &pool
                    .list_items(l)
                    .iter()
                    .map(|x| self.at_value(bindings, *x))
                    .collect::<Vec<_>>(),
            )
        };
        let d = match pool.get(t) {
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let self_ty = self.at_value(bindings, self_ty);
                if is_class_ref(pool, self.strip(self_ty))
                    && let Some((_, b)) = bindings.iter().find(|(a, _)| *a == assoc)
                {
                    return *b;
                }
                TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty,
                    args: list(args),
                }
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(args),
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(elems),
                rest: rest.map(|r| self.at_value(bindings, r)),
            },
            TyData::Option(i) => TyData::Option(self.at_value(bindings, i)),
            TyData::Mut(i) => TyData::Mut(self.at_value(bindings, i)),
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
            } => TyData::Fn {
                params: list(params),
                result: self.at_value(bindings, result),
                row,
                suspends,
                inputs,
            },
            TyData::TraitValue {
                def,
                args,
                bindings: bs,
            } => TyData::TraitValue {
                def,
                args: list(args),
                bindings: bs
                    .into_iter()
                    .map(|(a, b)| (a, self.at_value(bindings, b)))
                    .collect(),
            },
            _ => return t,
        };
        let r = pool.intern_ty(&d);
        if pool.has_assoc(r) {
            hd_types::solver::normalize_concrete(pool.types(), self.env.impls(), r)
        } else {
            r
        }
    }

    /// A trait method's declared type `t` at the vtable key `key`.
    fn member_at(&self, key: Ty, m: DefId, t: Ty) -> Ty {
        let TyData::TraitValue { args, bindings, .. } = self.pool.get(key) else {
            return t;
        };
        let s = subst(self.pool, self.env, m, self.dyn_args(args), t);
        self.at_value(&bindings, s)
    }

    /// The Wasm values of a type in locals, parameters and results.
    pub fn vts(&self, t: Ty) -> StageResult<Vec<VT>> {
        let pool = self.pool;
        let t = self.strip(t);
        if is_class_ref(pool, t) {
            return Ok(vec![VT::Eq]);
        }
        Ok(match pool.get(t) {
            TyData::Prim(p) => match p {
                Prim::I64 | Prim::U64 => vec![VT::I64],
                Prim::F32 => vec![VT::F32],
                Prim::F64 => vec![VT::F64],
                Prim::String => vec![VT::r(WTy::Bytes), VT::I64],
                _ => vec![VT::I32],
            },
            TyData::Never => vec![],
            TyData::Adt { def, args } => match self.env.std_kind(def) {
                StdKind::List => {
                    let [e] = pool.list_items(args)[..] else {
                        return unsupported("a List without one argument");
                    };
                    vec![VT::r(list_ty(&self.stored(e)?))]
                }
                StdKind::Map => {
                    let [k, v] = pool.list_items(args)[..] else {
                        return unsupported("a Map without two arguments");
                    };
                    vec![VT::r(map_ty(&self.stored(k)?, &self.stored(v)?))]
                }
                StdKind::Suspend => {
                    let [r] = pool.list_items(args)[..] else {
                        return unsupported("a Suspend without one argument");
                    };
                    vec![VT::r(suspend_base(&self.vts(r)?).0)]
                }
                StdKind::Other => {
                    if self.env.enum_variants(def, args).is_some() {
                        let f = self.form(t)?;
                        return Ok(match &f.boxed {
                            EnumBox::Flat(b) | EnumBox::Sub(b) => vec![VT::r(b.clone())],
                            EnumBox::None => [vec![VT::I32], f.slots.clone()].concat(),
                        });
                    }
                    if self.env.data_fields(def).is_none() {
                        return unsupported(format!("the layout of `{}`", (self.path)(def)));
                    }
                    self.nominal(t, Nominal::Data, def)?
                }
            },
            // A rest element is the tuple's last member, a `List`.
            TyData::Tuple { elems, rest } => {
                let mut es = Vec::new();
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    es.extend(self.vts(e)?);
                }
                if es.len() > BOUND {
                    vec![VT::r(box_of(&es))]
                } else {
                    es
                }
            }
            TyData::Option(inner) => {
                let iv = self.vts(inner)?;
                let one_ref = matches!(iv.as_slice(), [VT::Ref(_, false) | VT::Eq]);
                match opt_kind(iv.len(), one_ref, self.strip(inner) == Ty::STRING) {
                    OptKind::NullRef => vec![iv[0].dflt()],
                    OptKind::NullStr => vec![VT::rn(WTy::Bytes), VT::I64],
                    OptKind::Tagged => [vec![VT::I32], iv.iter().map(VT::dflt).collect()].concat(),
                    OptKind::Boxed => {
                        let dv: Vec<VT> = iv.iter().map(VT::dflt).collect();
                        vec![VT::rn(box_of(&dv))]
                    }
                }
            }
            TyData::Fn {
                params,
                result,
                suspends,
                ..
            } => {
                vec![VT::r(closure_base(
                    &self.code_ty(params, result, suspends)?,
                ))]
            }
            TyData::TraitValue { def, .. } => self.nominal(self.dyn_key(t), Nominal::Dyn, def)?,
            // Per key in key order, its payload and vtable
            // (codegen.md §12.4, "Context values").
            TyData::Context(_) => vec![VT::r(ctx_provs())],
            _ => {
                return unsupported(format!(
                    "the layout of the non-concrete type {}",
                    pool.display(t)
                ));
            }
        })
    }

    /// The storage of a type's values (`storage`), read without building
    /// the types its references name.
    fn stored(&self, t: Ty) -> StageResult<Vec<VT>> {
        Ok(self.flat(t)?.iter().map(Flat::storage).collect())
    }

    /// A type's values as storage reads them (`Flat`). It walks only value
    /// positions, where a self-recursive enum is a reference
    /// (`hd_mono::layout::recursive_enums`), so it ends.
    fn flat(&self, t: Ty) -> StageResult<Vec<Flat>> {
        let pool = self.pool;
        let t = self.strip(t);
        if is_class_ref(pool, t) {
            return Ok(vec![Flat::V(VT::Eq)]);
        }
        Ok(match pool.get(t) {
            TyData::Prim(_) | TyData::Never => self.vts(t)?.into_iter().map(Flat::V).collect(),
            TyData::Adt { def, args } => match self.env.std_kind(def) {
                StdKind::Other => match self.env.enum_variants(def, args) {
                    // Boxed whatever its payloads: they are not walked.
                    Some(vs)
                        if self.env.recursive_enum(def) && vs.iter().any(|v| !v.is_empty()) =>
                    {
                        vec![Flat::Ref(false)]
                    }
                    Some(_) => {
                        let f = self.form(t)?;
                        match &f.boxed {
                            EnumBox::None => std::iter::once(VT::I32)
                                .chain(f.slots.iter().cloned())
                                .map(Flat::V)
                                .collect(),
                            EnumBox::Flat(_) | EnumBox::Sub(_) => vec![Flat::Ref(false)],
                        }
                    }
                    None => vec![Flat::Ref(false)],
                },
                StdKind::List | StdKind::Map | StdKind::Suspend => vec![Flat::Ref(false)],
            },
            TyData::Tuple { elems, rest } => {
                let mut es = Vec::new();
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    es.extend(self.flat(e)?);
                }
                if es.len() > BOUND {
                    vec![Flat::Ref(false)]
                } else {
                    es
                }
            }
            TyData::Option(inner) => {
                let iv = self.flat(inner)?;
                let one_ref = matches!(iv.as_slice(), [Flat::Ref(false) | Flat::V(VT::Eq)]);
                match opt_kind(iv.len(), one_ref, self.strip(inner) == Ty::STRING) {
                    OptKind::NullRef => vec![iv[0].dflt()],
                    OptKind::NullStr => vec![Flat::V(VT::rn(WTy::Bytes)), Flat::V(VT::I64)],
                    OptKind::Tagged => {
                        [vec![Flat::V(VT::I32)], iv.iter().map(Flat::dflt).collect()].concat()
                    }
                    OptKind::Boxed => vec![Flat::Ref(true)],
                }
            }
            TyData::Fn { .. } | TyData::Context(_) => vec![Flat::Ref(false)],
            TyData::TraitValue { .. } => vec![Flat::V(VT::Eq), Flat::Ref(false)],
            _ => {
                return unsupported(format!(
                    "the layout of the non-concrete type {}",
                    pool.display(t)
                ));
            }
        })
    }

    /// An enum type's value layout (`enum_form`). The enums it holds by
    /// value are laid out first, innermost first (`value_enums`), so laying
    /// one out reads only its payloads' forms and the recursion is only as
    /// deep as tuples and optionals nest.
    fn form(&self, t: Ty) -> StageResult<Arc<EnumForm>> {
        if let Some(f) = self.rec.borrow().forms.get(&t) {
            return Ok(f.clone());
        }
        let env: &dyn LayoutEnv = self.env;
        let mut order = value_enums(self.pool, env, t);
        // A self-recursive enum holds nothing by value: not listed.
        if order.last() != Some(&t) {
            order.push(t);
        }
        for e in order {
            if self.rec.borrow().forms.contains_key(&e) {
                continue;
            }
            let TyData::Adt { def, args } = self.pool.get(e) else {
                return unsupported("an enum layout of a type that is not an enum");
            };
            let Some(vs) = self.env.enum_variants(def, args) else {
                return unsupported("an enum layout of a type that is not an enum");
            };
            let f = Arc::new(self.enum_form(def, &vs)?);
            self.rec.borrow_mut().forms.insert(e, f);
        }
        Ok(self.rec.borrow().forms[&t].clone())
    }

    /// An enum's value layout (§15.1, §15.2) from its payloads' storage:
    /// the shared slots, each payload field's slots, and its box. A
    /// self-recursive enum is always boxed.
    fn enum_form(&self, def: DefId, variants: &[Vec<Ty>]) -> StageResult<EnumForm> {
        let mut slots: Vec<VT> = Vec::new();
        let mut at = Vec::new();
        for v in variants {
            let mut used = vec![false; slots.len()];
            let mut fs = Vec::new();
            for f in v {
                let mut here = Vec::new();
                for x in self.flat(*f)? {
                    let s = x.storage().dflt();
                    let k = (0..slots.len())
                        .find(|&k| !used[k] && slots[k] == s)
                        .unwrap_or_else(|| {
                            slots.push(s.clone());
                            used.push(false);
                            slots.len() - 1
                        });
                    used[k] = true;
                    here.push(k);
                }
                fs.push(here);
            }
            at.push(fs);
        }
        let boxed = if slots.is_empty() || (!self.env.recursive_enum(def) && slots.len() < BOUND) {
            EnumBox::None
        } else if variants.iter().map(Vec::len).sum::<usize>() <= FLAT_FIELDS {
            EnumBox::Flat(box_of(&[vec![VT::I32], slots.clone()].concat()))
        } else {
            EnumBox::Sub(enum_base())
        };
        Ok(EnumForm { slots, at, boxed })
    }

    /// A closure's code type: `(eqref env, params..., keys, providers) ->
    /// results`. A suspending function value's code returns its cold
    /// `mut Suspend[T]`, as a plain call of a suspending function does
    /// (suspension.md §14.1).
    fn code_ty(&self, params: TyList, result: Ty, suspends: bool) -> StageResult<WTy> {
        let mut ps = vec![VT::Eq];
        for p in self.pool.list_items(params).iter().copied() {
            ps.extend(self.vts(p)?);
        }
        ps.push(VT::rn(ctx_keys()));
        ps.push(VT::rn(ctx_provs()));
        let mut rs = self.vts(result)?;
        if suspends {
            rs = vec![VT::r(suspend_base(&rs).0)];
        }
        Ok(WTy::Func(ps, rs))
    }

    /// The values of a nominal type: laid out, a member of a group being
    /// built (naming its position), or laid out now with every nominal
    /// type it reaches.
    fn nominal(&self, t: Ty, kind: Nominal, head: DefId) -> StageResult<Vec<VT>> {
        {
            let r = self.rec.borrow();
            if let Some(v) = r.done.get(&t) {
                return Ok(v.clone());
            }
            if let Some(&p) = r.building.last().and_then(|b| b.get(&t)) {
                return Ok(kind.vts(WTy::Back(p)));
            }
        }
        // Laid out by another instance: its whole group was, the same way.
        // The walk itself reads only its own results, so a group another
        // thread is publishing never splits it.
        if let Some(v) = self.shared.0.lock().ok().and_then(|s| s.get(&t).cloned()) {
            self.rec.borrow_mut().done.insert(t, v.clone());
            return Ok(v);
        }
        self.lay_out((t, kind, head))?;
        match self.rec.borrow().done.get(&t) {
            Some(v) => Ok(v.clone()),
            None => unsupported("a nominal type its own layout did not reach"),
        }
    }

    /// The nominal types a nominal type's heap type names directly: those
    /// in its member types (`member_tys`), through tuples, optionals,
    /// functions and suspensions. Enums, lists and maps erase what they
    /// hold, so they name none.
    fn nominal_refs(&self, n: &Node) -> StageResult<Vec<Node>> {
        let pool = self.pool;
        let mut out = Vec::new();
        let mut todo = self.member_tys(n)?;
        while let Some(x) = todo.pop() {
            let x = self.strip(x);
            if is_class_ref(pool, x) {
                continue;
            }
            match pool.get(x) {
                TyData::Adt { def, args } => match self.env.std_kind(def) {
                    StdKind::Suspend => todo.extend(pool.list_items(args)),
                    StdKind::List | StdKind::Map => {}
                    StdKind::Other => {
                        if self.env.enum_variants(def, args).is_none() {
                            out.push((x, Nominal::Data, def));
                        }
                    }
                },
                TyData::Tuple { elems, .. } => todo.extend(pool.list_items(elems)),
                TyData::Option(i) => todo.push(i),
                TyData::Fn { params, result, .. } => {
                    todo.extend(pool.list_items(params));
                    todo.push(result);
                }
                TyData::TraitValue { def, .. } => {
                    out.push((self.dyn_key(x), Nominal::Dyn, def));
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// The types whose values make a nominal type's heap type: a data
    /// type's fields, or a trait's method parameters (after `self`),
    /// results and row keys at the trait value's arguments.
    fn member_tys(&self, &(t, kind, _): &Node) -> StageResult<Vec<Ty>> {
        let (pool, env) = (self.pool, self.env);
        Ok(match (kind, pool.get(t)) {
            (Nominal::Data, TyData::Adt { def, args }) => env
                .data_fields(def)
                .unwrap_or_default()
                .into_iter()
                .map(|f| subst(pool, env, def, args, f))
                .collect(),
            (Nominal::Dyn, TyData::TraitValue { def, .. }) => {
                let mut out = Vec::new();
                for m in env.trait_methods(def) {
                    for p in env.params(m).unwrap_or_default().into_iter().skip(1) {
                        out.push(self.member_at(t, m, p));
                    }
                    out.push(self.member_at(t, m, env.ret(m).unwrap_or(Ty::VOID)));
                    out.extend(self.slot_keys(t, m));
                }
                out.extend(self.dyn_supers(t));
                out
            }
            _ => return unsupported("a nominal type that is not data or a trait value"),
        })
    }

    /// A vtable key's direct supertraits (codegen.md §13.16), in declared
    /// order, each the supertrait's key at the value's arguments, `Self`
    /// being the class `REF`, with the bindings it reaches. They are the
    /// vtable's parent fields, after its method slots.
    pub fn dyn_supers(&self, key: Ty) -> Vec<Ty> {
        match self.pool.get(key) {
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => self.super_keys(def, args, &bindings),
            _ => Vec::new(),
        }
    }

    /// A trait method's instance arguments at a trait value: `Self` as the
    /// class `REF`, then the trait's arguments.
    fn dyn_args(&self, args: TyList) -> TyList {
        let pool = self.pool;
        let mut all = vec![class_ref(pool)];
        all.extend(pool.list_items(args));
        pool.list(&all)
    }

    /// Lays out `root` and every nominal type it reaches that is not laid
    /// out yet: Tarjan's algorithm with an explicit stack, so a long
    /// chain of types does not deepen the native stack. Each strongly
    /// connected set closes as one group (`close`), after the groups it
    /// names, so building a member's heap type finds every type it names
    /// laid out or in its own group.
    fn lay_out(&self, root: Node) -> StageResult<()> {
        let mut w = Walk::default();
        self.visit(&mut w, root)?;
        while let Some(f) = w.call.last_mut() {
            let v = f.node;
            if let Some(n) = f.succ.get(f.next).copied() {
                f.next += 1;
                if self.rec.borrow().done.contains_key(&n.0) {
                    continue;
                }
                match w.index.get(&n.0) {
                    None => self.visit(&mut w, n)?,
                    Some(&i) if w.on[i] => w.low[v] = w.low[v].min(i),
                    Some(_) => {}
                }
                continue;
            }
            w.call.pop();
            if let Some(k) = w.path.get_mut(&w.nodes[v].2) {
                *k -= 1;
            }
            if let Some(p) = w.call.last() {
                w.low[p.node] = w.low[p.node].min(w.low[v]);
            }
            if w.low[v] == v {
                let at = w.stack.iter().rposition(|&x| x == v).expect("on the stack");
                let ids = w.stack.split_off(at);
                for &i in &ids {
                    w.on[i] = false;
                }
                let members: Vec<Node> = ids.iter().map(|&i| w.nodes[i]).collect();
                self.build_group(&members)?;
            }
        }
        Ok(())
    }

    /// Enters a nominal type in `lay_out`'s walk.
    fn visit(&self, w: &mut Walk, n: Node) -> StageResult<()> {
        // The item is on the path at other arguments: its declarations must
        // not build ever larger arguments, or the walk would not end.
        if w.path.get(&n.2).is_some_and(|k| *k > 0) && self.expansive(n.2) {
            return unsupported(format!(
                "a type layout whose declarations recur at ever larger arguments (`{}`)",
                (self.path)(n.2)
            ));
        }
        let succ = self.nominal_refs(&n)?;
        let i = w.nodes.len();
        w.index.insert(n.0, i);
        *w.path.entry(n.2).or_default() += 1;
        w.nodes.push(n);
        w.low.push(i);
        w.on.push(true);
        w.stack.push(i);
        w.call.push(Frame {
            node: i,
            succ,
            next: 0,
        });
        Ok(())
    }

    /// Builds a strongly connected set's heap types, each naming the set's
    /// members by position, and closes them as a group.
    fn build_group(&self, members: &[Node]) -> StageResult<()> {
        let at: HashMap<Ty, u32> = members
            .iter()
            .enumerate()
            .map(|(k, m)| (m.0, u32::try_from(k).expect("group")))
            .collect();
        self.rec.borrow_mut().building.push(at);
        let raws: StageResult<Vec<WTy>> = members
            .iter()
            .map(|&(t, kind, _)| match kind {
                Nominal::Data => self.data_struct(t),
                Nominal::Dyn => self.vtable_struct(t),
            })
            .collect();
        self.rec.borrow_mut().building.pop();
        self.close(members, &raws?);
        Ok(())
    }

    /// Closes a group (§15.3) whose heap types name its members by
    /// position. A lone type that names no member is an ordinary type.
    /// Otherwise the group's members are its nominal types and the types
    /// inside them that name one; nominal members are ordered by
    /// `canon(T)`, the others by their encoding over those, supertypes
    /// first.
    fn close(&self, members: &[Node], raws: &[WTy]) {
        if let [raw] = raws
            && !has_back(raw)
        {
            // A group of one that names no member: link emits it as an
            // ordinary type. As a `Rec`, the walks over the types that
            // name it stop at it, and its copies share one descriptor.
            let (t, kind, _) = members[0];
            let group = Group::new(vec![raw.clone()]);
            self.record(vec![(t, kind.vts(WTy::Rec(group, 0)))]);
            return;
        }
        // Nominal members by canonical key: `rank[i]` of member `i`.
        let path_hash = |d: DefId| self.env.path_hash(d);
        let mut memo = CanonMemo::default();
        let keys: Vec<_> = members
            .iter()
            .map(|m| canon(self.pool, &path_hash, &mut memo, m.0))
            .collect();
        let mut by_key: Vec<usize> = (0..members.len()).collect();
        by_key.sort_by_key(|&i| keys[i]);
        let mut rank = vec![0u32; members.len()];
        for (r, &i) in by_key.iter().enumerate() {
            rank[i] = u32::try_from(r).expect("group");
        }
        let ranked: Vec<WTy> = by_key
            .iter()
            .map(|&i| {
                raws[i].map_refs(&mut |t| match t {
                    WTy::Back(h) => Some(WTy::Back(rank[*h as usize])),
                    _ => None,
                })
            })
            .collect();
        let mut inner = BTreeSet::new();
        for t in &ranked {
            inner_members(t, &mut inner);
        }
        // Order: supertype depth inside the group, nominal before inner,
        // then key.
        let mut all: Vec<(WTy, bool, Vec<u8>)> = ranked
            .iter()
            .zip(&by_key)
            .map(|(t, &i)| (t.clone(), true, keys[i].0.to_be_bytes().to_vec()))
            .collect();
        all.extend(inner.into_iter().map(|t| {
            let e = content_key(&t);
            (t, false, e)
        }));
        let nominal = ranked.len();
        let inner_at: HashMap<WTy, usize> = (nominal..all.len())
            .map(|i| (all[i].0.clone(), i))
            .collect();
        // Each member's supertype inside the group, if any.
        let sup: Vec<Option<usize>> = all
            .iter()
            .map(|(t, _, _)| match t {
                WTy::Struct { sup: Some(s), .. } => match &**s {
                    WTy::Back(r) => Some(*r as usize),
                    s => inner_at.get(s).copied(),
                },
                _ => None,
            })
            .collect();
        let depths: Vec<usize> = (0..all.len())
            .map(|i| {
                let (mut d, mut at) = (0, i);
                while let Some(s) = sup[at].filter(|_| d < all.len()) {
                    (d, at) = (d + 1, s);
                }
                d
            })
            .collect();
        let mut order: Vec<usize> = (0..all.len()).collect();
        order.sort_by(|&a, &b| {
            (depths[a], !all[a].1, &all[a].2).cmp(&(depths[b], !all[b].1, &all[b].2))
        });
        let mut pos = vec![0u32; all.len()];
        for (k, &i) in order.iter().enumerate() {
            pos[i] = u32::try_from(k).expect("group");
        }
        let group = Group::new(
            order
                .iter()
                .map(|&i| {
                    all[i].0.map_refs(&mut |t| match t {
                        WTy::Back(r) => Some(WTy::Back(pos[*r as usize])),
                        t => inner_at.get(t).map(|k| WTy::Back(pos[*k])),
                    })
                })
                .collect(),
        );
        let laid: Vec<(Ty, Vec<VT>)> = by_key
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let (t, kind, _) = members[i];
                (t, kind.vts(WTy::Rec(group.clone(), pos[k])))
            })
            .collect();
        self.record(laid);
    }

    /// A data type's struct: its fields' values in declaration order.
    fn data_struct(&self, t: Ty) -> StageResult<WTy> {
        let TyData::Adt { def, args } = self.pool.get(t) else {
            return unsupported("a data layout of a type that is not data");
        };
        let mut fields = Vec::new();
        for f in self.env.data_fields(def).unwrap_or_default() {
            fields.extend(self.vts(subst(self.pool, self.env, def, args, f))?);
        }
        Ok(WTy::Struct {
            fields,
            sup: None,
            open: false,
        })
    }

    /// A trait's vtable (codegen.md §13.5, §13.16): one code reference per
    /// method of the trait itself, in declaration order, then one
    /// immutable reference per direct supertrait, in declared order, to
    /// that supertrait's own vtable.
    fn vtable_struct(&self, t: Ty) -> StageResult<WTy> {
        let TyData::TraitValue { def, .. } = self.pool.get(t) else {
            return unsupported("a vtable of a type that is not a trait value");
        };
        let mut fields = Vec::new();
        for m in self.env.trait_methods(def) {
            fields.push(VT::r(self.slot_sig(t, m)?));
        }
        for s in self.dyn_supers(t) {
            match &self.vts(s)?[..] {
                [VT::Eq, vt @ VT::Ref(..)] => fields.push(vt.clone()),
                _ => return unsupported("a supertrait value that is not a payload and a vtable"),
            }
        }
        Ok(WTy::Struct {
            fields,
            sup: None,
            open: false,
        })
    }

    /// Whether an item's declarations expand without bound: a cycle of
    /// the parameter graph through a nested argument, where `D[T]` names
    /// `E[List[T]]` and `E` leads back to `D` (Kennedy and Pierce's
    /// expansive cycles, over the positions that name a struct type:
    /// data fields and trait method signatures; enums, lists and maps
    /// erase what they hold). A projection counts as expanding, since
    /// it is not resolved here.
    fn expansive(&self, head: DefId) -> bool {
        if let Some(&e) = self.rec.borrow().expansive.get(&head) {
            return e;
        }
        let e = self.find_expansive(head);
        self.rec.borrow_mut().expansive.insert(head, e);
        e
    }

    fn find_expansive(&self, head: DefId) -> bool {
        let pool = self.pool;
        // Nodes: (item, parameter). Edges: (from, to, expanding).
        let mut ids: HashMap<(DefId, u16), usize> = HashMap::new();
        let mut edges: Vec<(usize, usize, bool)> = Vec::new();
        let mut seen = std::collections::HashSet::from([head]);
        let mut todo = vec![head];
        let mut projection = false;
        while let Some(item) = todo.pop() {
            for t in self.member_types(item) {
                let mut occ = Vec::new();
                self.occurrences(t, &mut occ, &mut projection);
                for (to, j, arg) in occ {
                    if seen.insert(to) {
                        todo.push(to);
                    }
                    let mut params = Vec::new();
                    params_in(pool, arg, &mut params);
                    for p in params.into_iter().filter(|p| p.owner == item) {
                        let n = ids.len();
                        let a = *ids.entry((item, p.index)).or_insert(n);
                        let n = ids.len();
                        let b = *ids.entry((to, j)).or_insert(n);
                        let bare = pool.get(arg) == TyData::Param(p);
                        edges.push((a, b, !bare));
                    }
                }
            }
        }
        if projection {
            return true;
        }
        let mut succ = vec![Vec::new(); ids.len()];
        for &(a, b, _) in &edges {
            succ[a].push(b);
        }
        let comp = sccs(&succ);
        edges
            .iter()
            .any(|&(a, b, grows)| grows && comp[a] == comp[b])
    }

    /// The declared types a data type's struct or a trait's vtable is
    /// built from: data fields, or method parameters (after `self`) and
    /// results.
    fn member_types(&self, item: DefId) -> Vec<Ty> {
        let env = self.env;
        if let Some(fs) = env.data_fields(item) {
            return fs;
        }
        let mut out = Vec::new();
        for m in env.trait_methods(item) {
            out.extend(env.params(m).unwrap_or_default().into_iter().skip(1));
            out.extend(env.ret(m));
        }
        out
    }

    /// The data types and trait values `t` names in positions that build
    /// struct types: (item, parameter position, argument). A trait's
    /// arguments are its parameters from 1 (`Self` is 0).
    fn occurrences(&self, t: Ty, out: &mut Vec<(DefId, u16, Ty)>, projection: &mut bool) {
        let pool = self.pool;
        let args_of = |def: DefId, args: TyList, from: u16, out: &mut Vec<(DefId, u16, Ty)>| {
            for (j, &a) in pool.list_items(args).iter().enumerate() {
                let j = u16::try_from(j).expect("parameters") + from;
                out.push((def, j, a));
            }
        };
        match pool.get(t) {
            TyData::Adt { def, args } => match self.env.std_kind(def) {
                StdKind::List | StdKind::Map => {}
                StdKind::Suspend => {
                    for a in pool.list_items(args).iter().copied() {
                        self.occurrences(a, out, projection);
                    }
                }
                StdKind::Other => {
                    if self.env.enum_variants(def, args).is_some() {
                        return;
                    }
                    args_of(def, args, 0, out);
                    for a in pool.list_items(args).iter().copied() {
                        self.occurrences(a, out, projection);
                    }
                }
            },
            TyData::TraitValue { def, args, .. } => {
                args_of(def, args, 1, out);
                for a in pool.list_items(args).iter().copied() {
                    self.occurrences(a, out, projection);
                }
            }
            TyData::Tuple { elems, rest } => {
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    self.occurrences(e, out, projection);
                }
            }
            TyData::Option(i) | TyData::Mut(i) => self.occurrences(i, out, projection),
            TyData::Fn { params, result, .. } => {
                for e in pool.list_items(params).iter().copied() {
                    self.occurrences(e, out, projection);
                }
                self.occurrences(result, out, projection);
            }
            TyData::Assoc { .. } => *projection = true,
            _ => {}
        }
    }

    /// The full shape of a type, for emission: its values and those of
    /// its parts.
    pub fn shape(&self, t: Ty) -> StageResult<Shape> {
        let pool = self.pool;
        let t = self.strip(t);
        if is_class_ref(pool, t) {
            return Ok(Shape::ClassRef);
        }
        Ok(match pool.get(t) {
            TyData::Prim(p) => match p {
                Prim::I64 | Prim::U64 => Shape::Scalar(VT::I64),
                Prim::F32 => Shape::Scalar(VT::F32),
                Prim::F64 => Shape::Scalar(VT::F64),
                Prim::String => Shape::Str,
                _ => Shape::Scalar(VT::I32),
            },
            TyData::Never => Shape::Void,
            TyData::Adt { def, args } => match self.env.std_kind(def) {
                StdKind::List => {
                    let [e] = pool.list_items(args)[..] else {
                        return unsupported("a List without one argument");
                    };
                    let elem = self.vts(e)?;
                    Shape::List {
                        ty: list_ty(&elem),
                        elem,
                    }
                }
                StdKind::Map => {
                    let [k, v] = pool.list_items(args)[..] else {
                        return unsupported("a Map without two arguments");
                    };
                    let (key, val) = (self.vts(k)?, self.vts(v)?);
                    Shape::Map {
                        ty: map_ty(&key, &val),
                        key,
                        val,
                    }
                }
                StdKind::Suspend => {
                    let [r] = pool.list_items(args)[..] else {
                        return unsupported("a Suspend without one argument");
                    };
                    let result = self.vts(r)?;
                    let (base, poll_ty) = suspend_base(&result);
                    Shape::Suspend {
                        base,
                        poll: poll_ty,
                        result,
                    }
                }
                StdKind::Other => {
                    if let Some(vs) = self.env.enum_variants(def, args) {
                        return Ok(Shape::Enum(self.enum_shape(t, &vs)?));
                    }
                    let [VT::Ref(ty, false)] = &self.vts(t)?[..] else {
                        return unsupported("a data type that is not one reference");
                    };
                    let mut fields = Vec::new();
                    let mut n = 0;
                    for f in self.env.data_fields(def).unwrap_or_default() {
                        let vs = self.vts(subst(pool, self.env, def, args, f))?;
                        fields.push((u32::try_from(n).expect("fields"), vs.clone()));
                        n += vs.len();
                    }
                    Shape::Data {
                        ty: (**ty).clone(),
                        fields,
                    }
                }
            },
            TyData::Tuple { elems, rest } => {
                let mut es = Vec::new();
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    es.push(self.vts(e)?);
                }
                let n: usize = es.iter().map(Vec::len).sum();
                Shape::Tuple {
                    boxed: (n > BOUND).then(|| box_of(&es.concat())),
                    elems: es,
                }
            }
            TyData::Option(inner) => {
                let iv = self.vts(inner)?;
                let one_ref = matches!(iv.as_slice(), [VT::Ref(_, false) | VT::Eq]);
                let dv: Vec<VT> = iv.iter().map(VT::dflt).collect();
                let shape = match opt_kind(iv.len(), one_ref, self.strip(inner) == Ty::STRING) {
                    OptKind::NullRef => OptShape::NullRef(iv[0].clone()),
                    OptKind::NullStr => OptShape::NullStr,
                    OptKind::Tagged => OptShape::Tagged(dv),
                    OptKind::Boxed => OptShape::Boxed(box_of(&dv), dv),
                };
                Shape::Opt(shape, iv)
            }
            TyData::Fn {
                params,
                result,
                suspends,
                ..
            } => {
                let code = self.code_ty(params, result, suspends)?;
                Shape::Fn {
                    base: closure_base(&code),
                    code,
                }
            }
            TyData::TraitValue { def, args, .. } => {
                let key = self.dyn_key(t);
                Shape::Dyn {
                    trait_: def,
                    args,
                    vt: self.vtable(key)?,
                    key,
                }
            }
            _ => {
                return unsupported(format!(
                    "the layout of the non-concrete type {}",
                    pool.display(t)
                ));
            }
        })
    }

    fn enum_shape(&self, t: Ty, variants: &[Vec<Ty>]) -> StageResult<EnumShape> {
        let form = self.form(t)?;
        let EnumForm { slots, at, boxed } = &*form;
        let mut fields = Vec::new();
        let mut subtypes = Vec::new();
        for (v, ats) in variants.iter().zip(at.iter().cloned()) {
            let mut fs = Vec::new();
            let mut next = 0;
            let mut sub_fields = vec![VT::I32];
            for (f, at) in v.iter().zip(ats) {
                let vs = self.vts(*f)?;
                let at = if matches!(boxed, EnumBox::Sub(_)) {
                    let ks: Vec<usize> = (next..next + vs.len()).collect();
                    next += vs.len();
                    sub_fields.extend(vs.iter().cloned());
                    ks
                } else {
                    at
                };
                fs.push((at, vs));
            }
            if let EnumBox::Sub(base) = boxed {
                subtypes.push((!v.is_empty()).then(|| WTy::Struct {
                    fields: sub_fields,
                    sup: Some(Box::new(base.clone())),
                    open: false,
                }));
            }
            fields.push(fs);
        }
        Ok(EnumShape {
            slots: slots.clone(),
            fields,
            boxed: match boxed {
                EnumBox::None => None,
                EnumBox::Flat(b) | EnumBox::Sub(b) => Some(b.clone()),
            },
            subtypes,
        })
    }

    /// A vtable key's vtable (codegen.md §13.5, §13.16): its method slots,
    /// then its direct supertraits' vtables.
    pub fn vtable(&self, key: Ty) -> StageResult<WTy> {
        match &self.vts(key)?[..] {
            [VT::Eq, VT::Ref(vt, false)] => Ok((**vt).clone()),
            _ => unsupported("a trait value that is not a payload and a vtable"),
        }
    }

    /// The vtable type of a requirement key's provider: that of the key's
    /// trait at the key's arguments, as `$.with` builds it.
    pub fn key_vtable(&self, k: Ty) -> StageResult<WTy> {
        match self.pool.get(k) {
            TyData::TraitValue { .. } => self.vtable(self.dyn_key(k)),
            _ => unsupported("a requirement key that is not a trait"),
        }
    }

    /// Where a call of `method`, declared by `trait_`, goes through a
    /// receiver of the trait-value type `recv` (trait.dyn.value-methods,
    /// trait.dyn.bound.dispatch, codegen.md §13.16): the method's slot in
    /// the vtable of `trait_`, reached from the receiver's vtable through
    /// the parent fields when `trait_` is a supertrait. `targs` are the
    /// callee's type arguments under the caller's substitution, the
    /// trait's first.
    pub(crate) fn dyn_slot(
        &self,
        recv: Ty,
        trait_: DefId,
        targs: &[Ty],
        method: DefId,
    ) -> StageResult<DynSlot> {
        let Shape::Dyn {
            vt: value_vt,
            key: recv_key,
            ..
        } = self.shape(recv)?
        else {
            return unsupported("a trait-value call on a value that is not a trait value");
        };
        let n = self.env.trait_arity(trait_);
        let want = self.pool.list(&targs[..n.min(targs.len())]);
        let Some((steps, key)) = self.super_path(recv_key, trait_, want)? else {
            return unsupported(
                "a trait-value call of a method of a trait the value does not extend",
            );
        };
        let Some(slot) = self
            .env
            .trait_methods(trait_)
            .iter()
            .position(|m| *m == method)
        else {
            return unsupported("a trait-value call of a method its trait does not declare");
        };
        Ok(DynSlot {
            value_vt,
            steps,
            vt: self.vtable(key)?,
            slot: u32::try_from(slot).expect("slot"),
            sig: self.slot_sig(key, method)?,
            keys: self.slot_keys(key, method),
        })
    }

    /// The parent-field path from the vtable of the key `from` to that of
    /// the supertrait `to` (codegen.md §13.16): each step's vtable type
    /// and parent field, then the supertrait's key. `from` itself is the
    /// empty path. The search runs depth first in declared order; the
    /// supertrait at the arguments `want` wins over the same trait at
    /// other arguments.
    pub(crate) fn super_path(
        &self,
        from: Ty,
        to: DefId,
        want: TyList,
    ) -> StageResult<Option<SuperPath>> {
        let mut found: Option<SuperPath> = None;
        let mut path = Vec::new();
        self.find_super(from, to, want, &mut path, &mut found)?;
        Ok(found)
    }

    fn find_super(
        &self,
        at: Ty,
        to: DefId,
        want: TyList,
        path: &mut Vec<(WTy, u32)>,
        found: &mut Option<SuperPath>,
    ) -> StageResult<bool> {
        let TyData::TraitValue { def, args, .. } = self.pool.get(at) else {
            return unsupported("a supertrait path from a type that is not a trait value");
        };
        if def == to {
            if args == want {
                *found = Some((path.clone(), at));
                return Ok(true);
            }
            if found.is_none() {
                *found = Some((path.clone(), at));
            }
        }
        let vt = self.vtable(at)?;
        let own = self.env.trait_methods(def).len();
        for (k, s) in self.dyn_supers(at).into_iter().enumerate() {
            path.push((vt.clone(), u32::try_from(own + k).expect("field")));
            let exact = self.find_super(s, to, want, path, found)?;
            path.pop();
            if exact {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The requirement keys of `m`'s row at the vtable key `key`, in key
    /// order: the providers its slot takes after the arguments, as a
    /// direct call passes its callee's (codegen.md §12.4, §13.5.1).
    pub fn slot_keys(&self, key: Ty, m: DefId) -> Vec<Ty> {
        let (pool, env) = (self.pool, self.env);
        let TyData::TraitValue { args, bindings, .. } = pool.get(key) else {
            return Vec::new();
        };
        let keys = env
            .row_keys(m, self.dyn_args(args))
            .into_iter()
            .map(|k| self.at_value(&bindings, k));
        key_order(pool, &|d| env.path_hash(d), keys)
    }

    /// A vtable slot's signature: `(eqref self, params..., providers...)
    /// -> results`, a provider per key of the method's row (`slot_keys`);
    /// a suspending method's slot returns its cold `mut Suspend[T]`.
    /// `key` is the vtable key of the trait declaring `m`.
    pub fn slot_sig(&self, key: Ty, m: DefId) -> StageResult<WTy> {
        let mut ps = vec![VT::Eq];
        for p in self.env.params(m).unwrap_or_default().into_iter().skip(1) {
            ps.extend(self.vts(self.member_at(key, m, p))?);
        }
        for k in self.slot_keys(key, m) {
            ps.push(VT::Eq);
            ps.push(VT::r(self.key_vtable(k)?));
        }
        let ret = self.member_at(key, m, self.env.ret(m).unwrap_or(Ty::VOID));
        let mut rs = self.vts(ret)?;
        if self.env.suspends(m) {
            rs = vec![VT::r(suspend_base(&rs).0)];
        }
        Ok(WTy::Func(ps, rs))
    }
}

/// The parameters a type names, anywhere in it.
fn params_in(pool: &InternPool, t: Ty, out: &mut Vec<ParamRef>) {
    match pool.get(t) {
        TyData::Param(p) => out.push(p),
        TyData::Adt { args, .. } => {
            for a in pool.list_items(args).iter().copied() {
                params_in(pool, a, out);
            }
        }
        TyData::TraitValue { args, bindings, .. } => {
            for a in pool.list_items(args).iter().copied() {
                params_in(pool, a, out);
            }
            for (_, b) in bindings {
                params_in(pool, b, out);
            }
        }
        TyData::Tuple { elems, rest } => {
            for e in pool.list_items(elems).iter().copied().chain(rest) {
                params_in(pool, e, out);
            }
        }
        TyData::Option(i) | TyData::Mut(i) => params_in(pool, i, out),
        TyData::Fn { params, result, .. } => {
            for e in pool.list_items(params).iter().copied() {
                params_in(pool, e, out);
            }
            params_in(pool, result, out);
        }
        TyData::Assoc { self_ty, args, .. } => {
            params_in(pool, self_ty, out);
            for a in pool.list_items(args).iter().copied() {
                params_in(pool, a, out);
            }
        }
        _ => {}
    }
}
