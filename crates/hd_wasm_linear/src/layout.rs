//! Linear layouts (`crates/hd_wasm_linear/DESIGN.md` §23.3): a type's Wasm
//! values in locals, parameters and results, and the same values in
//! memory, each at an offset.
//!
//! One walk gives both: a type's [`Mem`] lists its values as leaves, in
//! the order its locals hold them, each with its offset and width in
//! memory. A struct or a tuple is its fields' leaves, in declaration
//! order, each field at the offset rustc's `univariant` gives it. An enum
//! is rustc's tagged layout ([`Flat`]): its tag, then each variant's
//! payload as one struct at one offset; its values are one per distinct
//! leaf of its variants, so loads and stores need no branch on the tag. A
//! self-recursive enum and a
//! suspension are one address; a string is its address and byte
//! length; a trait value its data address and its vtable's address; a
//! list or a map the address of its buffer (§23.7); a function value its
//! code's table slot and its environment's address (§23.8). A reference,
//! the checker's `std.core.Ref[T]`, is the address of its place (§23.5,
//! §23.20); a reference to a trait value is that trait value, whose data
//! is already one address, as Rust's `&dyn Tr` is a fat pointer.

use hd_base::{SliceSort as _, StageResult};
use hd_mono::layout::{StdKind, strip_newtype};
use hd_mono::{ProgramEnv, subst};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};

use crate::module::{HEAP, Val, mem as memarg};
use crate::op::Op as I;
use crate::unsupported;

/// One Wasm value of a type, where it lies in memory: its offset in the
/// value, its width in bytes and whether a narrow load sign-extends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Leaf {
    pub offset: u32,
    pub val: Val,
    pub bytes: u8,
    pub signed: bool,
}

impl Leaf {
    /// The load of this leaf from the heap at an address on the stack,
    /// `base` bytes before the value.
    #[must_use]
    pub fn load(self, base: u32) -> I {
        let m = memarg(
            HEAP,
            u64::from(base + self.offset),
            u32::from(self.bytes).trailing_zeros(),
        );
        match (self.val, self.bytes, self.signed) {
            (Val::I32, 1, true) => I::I32Load8S(m),
            (Val::I32, 1, false) => I::I32Load8U(m),
            (Val::I32, 2, true) => I::I32Load16S(m),
            (Val::I32, 2, false) => I::I32Load16U(m),
            (Val::I32, _, _) => I::I32Load(m),
            (Val::I64, _, _) => I::I64Load(m),
            (Val::F32, _, _) => I::F32Load(m),
            (Val::F64, _, _) => I::F64Load(m),
        }
    }
}

impl Leaf {
    /// The store of this leaf to the heap at an address, then the value,
    /// on the stack, `base` bytes before the value.
    #[must_use]
    pub fn store(self, base: u32) -> I {
        let m = memarg(
            HEAP,
            u64::from(base + self.offset),
            u32::from(self.bytes).trailing_zeros(),
        );
        match (self.val, self.bytes) {
            (Val::I32, 1) => I::I32Store8(m),
            (Val::I32, 2) => I::I32Store16(m),
            (Val::I32, _) => I::I32Store(m),
            (Val::I64, _) => I::I64Store(m),
            (Val::F32, _) => I::F32Store(m),
            (Val::F64, _) => I::F64Store(m),
        }
    }
}

/// A type's layout: its size and alignment in memory, and its values.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mem {
    pub size: u32,
    pub align: u32,
    pub leaves: Vec<Leaf>,
}

impl Mem {
    const EMPTY: Mem = Mem {
        size: 0,
        align: 1,
        leaves: Vec::new(),
    };

    pub(crate) fn one(val: Val, bytes: u8, signed: bool) -> Mem {
        Mem {
            size: u32::from(bytes),
            align: u32::from(bytes),
            leaves: vec![Leaf {
                offset: 0,
                val,
                bytes,
                signed,
            }],
        }
    }

    /// An address: data, a self-recursive enum's cell, a frame.
    pub(crate) fn address() -> Mem {
        Mem::one(Val::I32, 4, false)
    }

    pub fn vals(&self) -> Vec<Val> {
        self.leaves.iter().map(|l| l.val).collect()
    }
}

/// One field of a struct, a tuple or a data payload: its first value's
/// index among the parent's values, its value count and its offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub first: usize,
    pub count: usize,
    pub offset: u32,
}

/// What a concrete type is to this backend.
pub enum Kind {
    /// Its values are in locals: a scalar, a string, an enum, a struct or
    /// tuple, a trait value.
    Value,
    /// A self-recursive enum: the address of a cell holding the enum's
    /// value as a non-recursive enum lays it out.
    Boxed,
    /// A reference to a place of type `T` (`std.core.Ref[T]`, or
    /// `std.core.Counted[T]` when `whole`): the place's address, or, when
    /// `T` is a trait value (`fat`), that trait value. A `whole` one is
    /// counted, so it refers to a whole object (`ref.box.object`): it is
    /// null or a payload start (§23.30).
    Ref { target: Ty, fat: bool, whole: bool },
}

/// Which kind `t` is.
pub fn kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Kind {
    let t = strip(pool, env, t);
    let TyData::Adt { def, args } = pool.get(t) else {
        return Kind::Value;
    };
    if env.ref_cells().holds(def) {
        let target = pool
            .list_items(args)
            .first()
            .map_or(Ty::VOID, |a| strip(pool, env, *a));
        // A reference to a trait value is the value itself.
        let fat = matches!(pool.get(target), TyData::TraitValue { .. });
        let whole = env.ref_cells().is_counted(def);
        return Kind::Ref { target, fat, whole };
    }
    if env.std_kind(def) != StdKind::Other {
        return Kind::Value;
    }
    if env.is_enum(def) {
        return if env.recursive_enum(def) {
            Kind::Boxed
        } else {
            Kind::Value
        };
    }
    Kind::Value
}

/// Whether reference type `t` is `&mut` or `*mut`: its carrier is
/// `std.core.RefMut` or `std.core.CountedMut` (`hd_types::Types::lower_refs`).
pub fn ref_mutable(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    matches!(pool.get(strip(pool, env, t)), TyData::Adt { def, .. } if env.ref_cells().is_mutable(def))
}

/// `t` with newtypes erased: what its layout reads.
pub fn strip(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Ty {
    strip_newtype(pool, env, t)
}

/// The kind of `t`, once stripped, when it is a list or a map.
pub fn coll_kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<StdKind> {
    match pool.get(strip(pool, env, t)) {
        TyData::Adt { def, .. } => {
            Some(env.std_kind(def)).filter(|k| matches!(k, StdKind::List | StdKind::Map))
        }
        _ => None,
    }
}

/// A tuple type's members in order: its elements, then its rest list
/// when it has one, laid out as one more member.
pub fn tuple_members(pool: &InternPool, t: Ty) -> Vec<Ty> {
    match pool.get(t) {
        TyData::Tuple { elems, rest } => {
            pool.list_items(elems).iter().copied().chain(rest).collect()
        }
        _ => Vec::new(),
    }
}

fn union_variants(pool: &InternPool, members: TyList) -> Vec<Vec<Ty>> {
    pool.list_items(members)
        .iter()
        .map(|member| vec![*member])
        .collect()
}

/// The values of a `t` value that are collections a copy of it copies
/// (`hd_mono::holds_coll`): a list's own, and each such value of a struct
/// field, at any depth of structs.
pub fn coll_leaves(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<u32> {
    if !hd_mono::holds_coll(pool, env, t) {
        return Vec::new();
    }
    let s = strip_newtype(pool, env, t);
    let TyData::Adt { def, args } = pool.get(s) else {
        return Vec::new();
    };
    if env.std_kind(def) != StdKind::Other {
        return vec![0];
    }
    let fields: Vec<Ty> = env
        .data_fields(def)
        .unwrap_or_default()
        .into_iter()
        .map(|f| subst(pool, env, def, args, f))
        .collect();
    let Ok((_, parts)) = payload_mem(pool, env, &fields) else {
        return Vec::new();
    };
    fields
        .iter()
        .zip(&parts)
        .flat_map(|(f, p)| {
            let at = u32::try_from(p.first).expect("values");
            coll_leaves(pool, env, *f).into_iter().map(move |k| at + k)
        })
        .collect()
}

/// The Wasm values of a concrete type.
pub fn vals(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Vec<Val>> {
    Ok(mem(pool, env, t)?.vals())
}

/// Each of a concrete type's Wasm values' path below the value, for
/// development tools' local names (`FunctionOrigins::locals`): a struct
/// value's field by name (`pos.x`), a tuple's member by index (`1`), and
/// the values of any other type of several (a string, a closure, an enum)
/// by their index among its values. A type of one value has the one path
/// `""`, and a type of none no path. A layout view (#664) can share it.
#[must_use]
pub fn value_paths(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<String> {
    let n = mem(pool, env, t).map_or(1, |m| m.leaves.len());
    match n {
        0 => return Vec::new(),
        1 => return vec![String::new()],
        _ => {}
    }
    let t = strip(pool, env, t);
    let fields: Option<(Vec<String>, Vec<Ty>)> = match pool.get(t) {
        TyData::Tuple { .. } => {
            let members = tuple_members(pool, t);
            Some(((0..members.len()).map(|k| k.to_string()).collect(), members))
        }
        TyData::Adt { def, args }
            if env.std_kind(def) == StdKind::Other
                && matches!(kind(pool, env, t), Kind::Value)
                && !env.is_enum(def) =>
        {
            let tys: Vec<Ty> = env
                .data_fields(def)
                .unwrap_or_default()
                .into_iter()
                .map(|f| subst(pool, env, def, args, f))
                .collect();
            let names = env
                .data_field_names(def)
                .filter(|ns| ns.len() == tys.len())
                .unwrap_or_else(|| (0..tys.len()).map(|k| k.to_string()).collect());
            Some((names, tys))
        }
        _ => None,
    };
    let by_index = || (0..n).map(|k| k.to_string()).collect();
    let Some((names, tys)) = fields else {
        return by_index();
    };
    let mut out = Vec::with_capacity(n);
    for (name, ty) in names.iter().zip(tys) {
        for below in value_paths(pool, env, ty) {
            out.push(if below.is_empty() {
                name.clone()
            } else {
                format!("{name}.{below}")
            });
        }
    }
    if out.len() == n { out } else { by_index() }
}

/// The types whose layouts `t`'s layout is made of, held by value: a
/// tuple's members, an optional's inner type, a value enum's payload
/// fields, a struct's fields. Every other type is one leaf or address.
pub fn value_parts(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<Ty> {
    match pool.get(t) {
        TyData::Tuple { .. } => tuple_members(pool, t),
        TyData::Option(inner) => vec![inner],
        TyData::Union(members) => pool.list_items(members).to_vec(),
        TyData::Adt { def, args }
            if env.std_kind(def) == StdKind::Other && matches!(kind(pool, env, t), Kind::Value) =>
        {
            match env.enum_variants(def, args) {
                Some(variants) => variants.into_iter().flatten().collect(),
                None => env
                    .data_fields(def)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| subst(pool, env, def, args, f))
                    .collect(),
            }
        }
        _ => Vec::new(),
    }
}

/// A type's result from its parts' results, which the second argument
/// reads ([`by_parts`]).
type PartFn<'a, T> = dyn Fn(Ty, &dyn Fn(Ty) -> T) -> T + 'a;

/// Computes a result for `t` and every type it holds by value
/// ([`value_parts`]), parts first, with a stack of its own: a long chain
/// of types held by value takes no native stack depth. `one` computes a
/// type's result from its parts' results, which its second argument reads.
fn by_parts<T: Clone>(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    done: &mut hd_base::HashMap<Ty, T>,
    one: &PartFn<'_, T>,
) -> T {
    let mut stack = vec![(strip(pool, env, t), false)];
    while let Some((u, ready)) = stack.pop() {
        if done.contains_key(&u) {
            continue;
        }
        let parts: Vec<Ty> = value_parts(pool, env, u)
            .into_iter()
            .map(|p| strip(pool, env, p))
            .filter(|p| !done.contains_key(p))
            .collect();
        if !ready && !parts.is_empty() {
            stack.push((u, true));
            stack.extend(parts.into_iter().map(|p| (p, false)));
            continue;
        }
        let got = one(u, &|p| {
            done.get(&strip(pool, env, p))
                .cloned()
                .expect("a part computed first")
        });
        done.insert(u, got);
    }
    done.get(&strip(pool, env, t))
        .cloned()
        .expect("the type computed")
}

/// The layouts and walks one link has computed, by stripped type
/// ([`cached`]); a type's layout is fixed for the whole link.
type Memo<T> = std::cell::RefCell<Option<hd_base::HashMap<Ty, T>>>;

thread_local! {
    static MEMS: Memo<StageResult<Mem>> = const { std::cell::RefCell::new(None) };
    static WALKS: Memo<Option<Walk>> = const { std::cell::RefCell::new(None) };
    static COUNT_WALKS: Memo<Option<Walk>> = const { std::cell::RefCell::new(None) };
    static DROPS: Memo<bool> = const { std::cell::RefCell::new(None) };
    static FLATS: Memo<StageResult<std::rc::Rc<Flat>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `link` with the layout cache in place: within it, [`mem`] and
/// [`walk`] compute each type once, so a long chain of types held by
/// value costs its length, not its square, at every use.
pub fn cached<R>(link: impl FnOnce() -> R) -> R {
    struct Reset {
        mems: Option<hd_base::HashMap<Ty, StageResult<Mem>>>,
        walks: Option<hd_base::HashMap<Ty, Option<Walk>>>,
        count_walks: Option<hd_base::HashMap<Ty, Option<Walk>>>,
        drops: Option<hd_base::HashMap<Ty, bool>>,
        flats: Option<hd_base::HashMap<Ty, StageResult<std::rc::Rc<Flat>>>>,
    }
    impl Drop for Reset {
        fn drop(&mut self) {
            MEMS.set(self.mems.take());
            WALKS.set(self.walks.take());
            COUNT_WALKS.set(self.count_walks.take());
            DROPS.set(self.drops.take());
            FLATS.set(self.flats.take());
        }
    }

    let _reset = Reset {
        mems: MEMS.take(),
        walks: WALKS.take(),
        count_walks: COUNT_WALKS.take(),
        drops: DROPS.take(),
        flats: FLATS.take(),
    };
    MEMS.set(Some(hd_base::HashMap::default()));
    WALKS.set(Some(hd_base::HashMap::default()));
    COUNT_WALKS.set(Some(hd_base::HashMap::default()));
    DROPS.set(Some(hd_base::HashMap::default()));
    FLATS.set(Some(hd_base::HashMap::default()));
    link()
}

/// [`cached`], unless a cache is in place already, which `link` then
/// shares: the ownership passes run inside a link (a body it reaches
/// first) or outside one (`Collect`).
pub fn cached_here<R>(link: impl FnOnce() -> R) -> R {
    if MEMS.with(|m| m.borrow().is_some()) {
        link()
    } else {
        cached(link)
    }
}

/// [`by_parts`] over `memo`'s map when a link has one in place, else over
/// a map of this call's own.
fn memoized<T: Clone + 'static>(
    memo: &'static std::thread::LocalKey<Memo<T>>,
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    one: &PartFn<'_, T>,
) -> T {
    let held = memo.take();
    let linked = held.is_some();
    let mut done = held.unwrap_or_default();
    let got = by_parts(pool, env, t, &mut done, one);
    if linked {
        memo.set(Some(done));
    }
    got
}

/// A concrete type's layout.
pub fn mem(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Mem> {
    memoized(&MEMS, pool, env, t, &|u, part| mem_one(pool, env, u, part))
}

/// `t`'s layout from its parts' layouts, which `part` reads.
fn mem_one(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    part: &dyn Fn(Ty) -> StageResult<Mem>,
) -> StageResult<Mem> {
    let t = strip(pool, env, t);
    Ok(match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::I8 => Mem::one(Val::I32, 1, true),
            Prim::U8 | Prim::Bool => Mem::one(Val::I32, 1, false),
            Prim::I16 => Mem::one(Val::I32, 2, true),
            Prim::U16 => Mem::one(Val::I32, 2, false),
            Prim::I32 | Prim::U32 | Prim::Usize | Prim::Char => Mem::one(Val::I32, 4, false),
            Prim::I64 | Prim::U64 => Mem::one(Val::I64, 8, false),
            Prim::F32 => Mem::one(Val::F32, 4, false),
            Prim::F64 => Mem::one(Val::F64, 8, false),
            Prim::String => record(&[Mem::address(), Mem::address()]).0,
        },
        TyData::Never => Mem::EMPTY,
        TyData::Tuple { .. } => payload_mem_by(&tuple_members(pool, t), part)?.0,
        TyData::Option(inner) => {
            let variants = [vec![], vec![inner]];
            enum_flat_by(t, &variants, nullable(pool, env, &variants), part)?
        }
        TyData::Union(members) => {
            let variants = union_variants(pool, members);
            enum_flat_by(t, &variants, false, part)?
        }
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::Suspend => Mem::address(),
            StdKind::List | StdKind::Map => Mem::address(),
            StdKind::Other => match kind(pool, env, t) {
                Kind::Boxed | Kind::Ref { fat: false, .. } => Mem::address(),
                Kind::Ref { fat: true, .. } => record(&[Mem::address(), Mem::address()]).0,
                Kind::Value => match env.enum_variants(def, args) {
                    Some(variants) => {
                        enum_flat_by(t, &variants, nullable(pool, env, &variants), part)?
                    }
                    None => payload_mem_by(&value_parts(pool, env, t), part)?.0,
                },
            },
        },
        TyData::Fn { .. } => record(&[Mem::address(), Mem::address()]).0,
        TyData::TraitValue { .. } => record(&[Mem::address(), Mem::address()]).0,
        // A `$.Context` value is the address of a counted context
        // (`rt::ctx_elem`).
        TyData::Context(_) => Mem::address(),
        TyData::Ref { .. } => return unsupported("references (job 12)"),
        // An instance that collection shares among reference arguments
        // (A1, `crates/hd_mono/DESIGN.md` §13.2) holds one as an address:
        // Collection selects it with [`crate::shared_reference_layout`].
        TyData::Canon(_) if hd_mono::is_class_ref(pool, t) => Mem::address(),
        _ => return unsupported(format!("values of type `{}`", pool.display(t))),
    })
}

/// Enum `t`'s layout from its parts' layouts, which `part` reads, kept
/// for [`flat`] in the link's cache: its values.
fn enum_flat_by(
    t: Ty,
    variants: &[Vec<Ty>],
    nullable: bool,
    part: &dyn Fn(Ty) -> StageResult<Mem>,
) -> StageResult<Mem> {
    let got = enum_mem_by(variants, nullable, part).map(std::rc::Rc::new);
    FLATS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    Ok(got?.mem.clone())
}

/// How one counted address of a [`Walk`] is counted: the leaf that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addr {
    /// [`Walk::Ref`]: counting finds the block through the page table.
    Ref,
    /// [`Walk::Known`]: a known payload start.
    Known,
}

/// Where a value's counted references are among its values (§23.4): what
/// counting walks to retain or release the value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Walk {
    /// Value `k` is an address in a counted object, or in static data: a
    /// string's bytes, a local reference, an open value; counting finds its
    /// block through the page table (§23.5).
    Ref(u32),
    /// Value `k` is null, a static object's, or the payload start of a
    /// counted object, eight bytes after its header: a boxed enum's cell, a collection's buffer, a closure's environment, a
    /// thin counted reference ([`block_start`]); counting
    /// reads the header at `k - 8` (§23.30).
    Known(u32),
    /// No counted address, but a value whose release runs a
    /// `std.rt.Drop` implementation: a `UniqList` of scalars, whose
    /// buffer's room is freed by its drop (`module.rt.drop.glue`). It
    /// stands alone; a walk with a counted address already releases.
    Glue,
    /// Each part in turn.
    Seq(Vec<Walk>),
    /// Value `k` is an enum's tag; arm `v`, when there is one, walks
    /// variant `v`'s payload.
    Switch(u32, Vec<Option<Walk>>),
    /// A [`nullable`] enum's address, which the inner walk counts unless
    /// it is null.
    Nullable(Box<Walk>),
}

impl Walk {
    /// The walk with each value index `k` replaced by `to(k)`.
    #[must_use]
    pub fn remap(&self, to: &dyn Fn(u32) -> u32) -> Walk {
        match self {
            Walk::Ref(k) => Walk::Ref(to(*k)),
            Walk::Known(k) => Walk::Known(to(*k)),
            Walk::Glue => Walk::Glue,
            Walk::Seq(ws) => Walk::Seq(ws.iter().map(|w| w.remap(to)).collect()),
            Walk::Switch(k, arms) => Walk::Switch(
                to(*k),
                arms.iter()
                    .map(|a| a.as_ref().map(|w| w.remap(to)))
                    .collect(),
            ),
            Walk::Nullable(w) => Walk::Nullable(Box::new(w.remap(to))),
        }
    }

    /// The walk without the values `skip`, which a copy replaces rather
    /// than counts; `None` when nothing is left.
    #[must_use]
    pub fn without(&self, skip: &[u32]) -> Option<Walk> {
        match self {
            Walk::Ref(k) => (!skip.contains(k)).then_some(Walk::Ref(*k)),
            Walk::Known(k) => (!skip.contains(k)).then_some(Walk::Known(*k)),
            Walk::Glue => Some(Walk::Glue),
            Walk::Seq(ws) => Walk::seq(ws.iter().filter_map(|w| w.without(skip)).collect()),
            Walk::Switch(k, arms) => Some(Walk::Switch(
                *k,
                arms.iter()
                    .map(|a| a.as_ref().and_then(|w| w.without(skip)))
                    .collect(),
            )),
            Walk::Nullable(w) => w.without(skip).map(|w| Walk::Nullable(Box::new(w))),
        }
    }

    /// The value index when the walk is one counted address, of either
    /// kind.
    #[must_use]
    pub fn single(&self) -> Option<u32> {
        match self {
            Walk::Ref(k) | Walk::Known(k) => Some(*k),
            Walk::Glue | Walk::Seq(_) | Walk::Switch(..) | Walk::Nullable(_) => None,
        }
    }

    /// Whether the walk is value 0 alone, as a plain count takes it: a
    /// value that is one such address can stand for itself where a reference
    /// view holds one counted address.
    #[must_use]
    pub fn plain_single(&self) -> bool {
        matches!(self, Walk::Ref(0) | Walk::Known(0))
    }

    /// How the walk's one counted address is counted, when it is one.
    #[must_use]
    pub fn addr(&self) -> Option<Addr> {
        match self {
            Walk::Ref(_) => Some(Addr::Ref),
            Walk::Known(_) => Some(Addr::Known),
            Walk::Glue | Walk::Seq(_) | Walk::Switch(..) | Walk::Nullable(_) => None,
        }
    }

    /// Whether the walk is a known payload start: [`Walk::Known`].
    #[must_use]
    pub fn known(&self) -> bool {
        matches!(self, Walk::Known(_))
    }

    fn seq(mut ws: Vec<Walk>) -> Option<Walk> {
        if ws.iter().any(|w| *w != Walk::Glue) {
            ws.retain(|w| *w != Walk::Glue);
        } else {
            ws.truncate(1);
        }
        match ws.len() {
            0 => None,
            1 => ws.pop(),
            _ => Some(Walk::Seq(ws)),
        }
    }
}

/// The counted references of a concrete type's values, or `None` when it
/// has none: scalars, and what holds no counted address. A type this
/// backend does not lay out has none here; its layout refuses it.
pub fn walk(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Walk> {
    memoized(&WALKS, pool, env, t, &|u, part| {
        walk_one(pool, env, u, part, false)
    })
}

/// [`walk`] as counting sees it: a thin local reference
/// (`ref.count.local`) is a plain address, which counts nothing, though
/// the ownership passes track it as a counted value ([`walk`]).
pub fn count_walk(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Walk> {
    memoized(&COUNT_WALKS, pool, env, t, &|u, part| {
        walk_one(pool, env, u, part, true)
    })
}

/// `t`'s walk from its parts' walks, which `part` reads.
fn walk_one(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    part: &dyn Fn(Ty) -> Option<Walk>,
    counting: bool,
) -> Option<Walk> {
    let t = strip(pool, env, t);
    match pool.get(t) {
        TyData::Prim(Prim::String) => Some(Walk::Ref(0)),
        TyData::Fn { .. } => Some(Walk::Known(1)),
        TyData::TraitValue { .. } => Some(Walk::Ref(0)),
        TyData::Context(_) => Some(Walk::Known(0)),
        TyData::Tuple { .. } => fields_walk_by(pool, env, &tuple_members(pool, t), part),
        TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => Some(Walk::Known(0)),
        TyData::Option(_) | TyData::Union(_) => switch_walk_by(pool, env, t, part),
        TyData::Adt { def, args } if env.std_kind(def) == StdKind::Other => {
            match kind(pool, env, t) {
                Kind::Boxed => Some(Walk::Known(0)),
                Kind::Ref {
                    fat: false,
                    whole: false,
                    ..
                } if counting => None,
                Kind::Ref { fat, whole, .. } => Some(if whole && !fat {
                    Walk::Known(0)
                } else {
                    Walk::Ref(0)
                }),
                Kind::Value => {
                    let w = match env.enum_variants(def, args) {
                        Some(_) => switch_walk_by(pool, env, t, part),
                        None => fields_walk_by(pool, env, &value_parts(pool, env, t), part),
                    };
                    w.or_else(|| env.has_drop(def).then_some(Walk::Glue))
                }
            }
        }
        TyData::Canon(_) if hd_mono::is_class_ref(pool, t) => Some(Walk::Ref(0)),
        _ => None,
    }
}

/// Whether this representation's counted pointer is a payload start,
/// never a string's interior bytes or an interior/erased reference: a
/// reference is one when it is counted and thin (§23.30).
pub fn block_start(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let t = strip(pool, env, t);
    match pool.get(t) {
        TyData::Fn { .. } | TyData::Context(_) => true,
        TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => true,
        _ => match kind(pool, env, t) {
            Kind::Boxed => true,
            Kind::Ref { fat, whole, .. } => whole && !fat,
            Kind::Value => false,
        },
    }
}

/// How a plain function's parameter takes its argument
/// (`crates/hd_wasm_linear/DESIGN.md` §23.25; `REFCOUNT_RUNTIME.md` §4.3), fixed
/// by the parameter's type alone so that every caller and the callee
/// agree without reading each other's bodies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Nothing in the value is counted.
    Uncounted,
    /// A value parameter `T`: the callee's own copy, which arrives with a
    /// count of each counted reference the walk names and which the
    /// callee releases. The caller moves its value in at its last use,
    /// else counts a copy.
    Owned(Walk),
    /// A `&T` parameter: a borrowed read. The caller keeps the argument
    /// alive for the call, and neither side counts it.
    Borrowed,
}

/// How a plain function takes a parameter of type `t`; `counts` is whether
/// the program counts at all.
pub fn pass(pool: &InternPool, env: &dyn ProgramEnv, counts: bool, t: Ty) -> Pass {
    if !counts {
        return Pass::Uncounted;
    }
    if matches!(kind(pool, env, t), Kind::Ref { .. }) {
        return Pass::Borrowed;
    }
    count_walk(pool, env, t).map_or(Pass::Uncounted, Pass::Owned)
}

/// The walk of fields laid out in order, as a struct, a tuple or a data
/// payload lays them out.
pub fn fields_walk(pool: &InternPool, env: &dyn ProgramEnv, fields: &[Ty]) -> Option<Walk> {
    fields_walk_by(pool, env, fields, &|f| count_walk(pool, env, f))
}

/// [`fields_walk`], with each field's own walk read by `part`.
fn fields_walk_by(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    fields: &[Ty],
    part: &dyn Fn(Ty) -> Option<Walk>,
) -> Option<Walk> {
    let (_, parts) = payload_mem(pool, env, fields).ok()?;
    let first = |p: &Part| u32::try_from(p.first).expect("values");
    Walk::seq(
        fields
            .iter()
            .zip(&parts)
            .filter_map(|(f, p)| {
                let at = first(p);
                part(*f).map(|w| w.remap(&|k| at + k))
            })
            .collect(),
    )
}

/// A closure environment's layout from its captures, each its type's
/// value or, when shared, a cell's address; with the places of the
/// captures and their counted references.
pub fn env_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    fields: &[(Ty, bool)],
) -> StageResult<(Mem, Vec<Part>, Option<Walk>)> {
    let ms = fields
        .iter()
        .map(|(t, shared)| {
            if *shared {
                Ok(Mem::address())
            } else {
                mem(pool, env, *t)
            }
        })
        .collect::<StageResult<Vec<_>>>()?;
    let (m, parts) = record(&ms);
    let walks = fields
        .iter()
        .zip(&parts)
        .filter_map(|((t, shared), p)| {
            let at = u32::try_from(p.first).expect("values");
            if *shared {
                Some(Walk::Known(at))
            } else {
                count_walk(pool, env, *t).map(|w| w.remap(&|k| at + k))
            }
        })
        .collect();
    Ok((m, parts, Walk::seq(walks)))
}

/// The walk of enum or optional type `t`: by the tag, each variant's
/// fields at their values.
pub fn switch_walk(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Walk> {
    switch_walk_by(pool, env, t, &|f| count_walk(pool, env, f))
}

/// [`switch_walk`], with each field's own walk read by `part`.
fn switch_walk_by(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    part: &dyn Fn(Ty) -> Option<Walk>,
) -> Option<Walk> {
    let variants = variants(pool, env, t)?;
    if nullable(pool, env, &variants) {
        // The address is value 0, null for variant 0.
        return part(variants[1][0]).map(|w| Walk::Nullable(Box::new(w)));
    }
    let flat = flat(pool, env, t).ok()?;
    let arms: Vec<Option<Walk>> = variants
        .iter()
        .zip(&flat.positions)
        .map(|(fields, at)| {
            Walk::seq(
                fields
                    .iter()
                    .zip(at)
                    .filter_map(|(f, ks)| part(*f).map(|w| w.remap(&|k| ks[k as usize])))
                    .collect(),
            )
        })
        .collect();
    if arms.iter().all(Option::is_none) {
        return None;
    }
    Some(Walk::Switch(flat.discr(), arms))
}

/// The captures of a closure environment in order, each at its alignment:
/// the whole layout and each capture's place in it. The environment's code
/// reads them at these offsets.
fn record(fields: &[Mem]) -> (Mem, Vec<Part>) {
    let mut out = Mem::EMPTY;
    let mut parts = Vec::new();
    for f in fields {
        let offset = out.size.next_multiple_of(f.align);
        parts.push(Part {
            first: out.leaves.len(),
            count: f.leaves.len(),
            offset,
        });
        out.leaves.extend(f.leaves.iter().map(|l| Leaf {
            offset: l.offset + offset,
            ..*l
        }));
        out.size = offset + f.size;
        out.align = out.align.max(f.align);
    }
    out.size = out.size.next_multiple_of(out.align);
    (out, parts)
}

/// A field's alignment group, rustc's `alignment_group_key` with no niche
/// (`rustc_abi` layout.rs:1155): `log2` of the larger of its alignment and
/// its size, so a field of four bytes at alignment 1 groups with the
/// 4-aligned ones.
fn align_group(m: &Mem) -> u32 {
    m.align.max(m.size).trailing_zeros()
}

/// Fields placed as rustc's `univariant` places them (layout.rs:1091,
/// decision 1 of `docs/future-work/ENUM-LAYOUT.md`): a struct's (`prefix`
/// 0) by alignment group, largest first; a payload behind a tag of
/// `prefix` bytes by group, smallest first, so that small fields pack
/// after the tag. The sort is stable. The leaves stay in declaration
/// order, each field's together, so a field's values in locals are its
/// [`Part`]; only the offsets move.
fn univariant(fields: &[Mem], prefix: u32) -> (Mem, Vec<Part>) {
    let mut order: Vec<usize> = (0..fields.len()).collect();
    if prefix == 0 {
        order.sort_stable_by_key(|&k| std::cmp::Reverse(align_group(&fields[k])));
    } else {
        order.sort_stable_by_key(|&k| align_group(&fields[k]));
    }
    let mut offsets = vec![0u32; fields.len()];
    let mut size = prefix;
    let mut align = prefix.max(1);
    for &k in &order {
        let f = &fields[k];
        let offset = size.next_multiple_of(f.align);
        offsets[k] = offset;
        size = offset + f.size;
        align = align.max(f.align);
    }
    let mut out = Mem {
        size: size.next_multiple_of(align),
        align,
        leaves: Vec::with_capacity(fields.iter().map(|f| f.leaves.len()).sum()),
    };
    let mut parts = Vec::with_capacity(fields.len());
    for (f, &offset) in fields.iter().zip(&offsets) {
        parts.push(Part {
            first: out.leaves.len(),
            count: f.leaves.len(),
            offset,
        });
        out.leaves.extend(f.leaves.iter().map(|l| Leaf {
            offset: l.offset + offset,
            ..*l
        }));
    }
    (out, parts)
}

/// A struct value's or a tuple's layout and its fields' places.
pub fn struct_mem(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<(Mem, Vec<Part>)> {
    let t = strip(pool, env, t);
    let fields = match pool.get(t) {
        TyData::Tuple { .. } => tuple_members(pool, t),
        TyData::Adt { def, args } if !env.is_enum(def) => env
            .data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| subst(pool, env, def, args, f))
            .collect(),
        _ => return unsupported("a field of a value that is not a struct or a tuple"),
    };
    payload_mem(pool, env, &fields)
}

/// The layout of an object's payload of `fields`, or of a struct's fields.
pub fn payload_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    fields: &[Ty],
) -> StageResult<(Mem, Vec<Part>)> {
    payload_mem_by(fields, &|f| mem(pool, env, f))
}

/// [`payload_mem`], with each field's layout read by `part`.
fn payload_mem_by(
    fields: &[Ty],
    part: &dyn Fn(Ty) -> StageResult<Mem>,
) -> StageResult<(Mem, Vec<Part>)> {
    let ms = fields
        .iter()
        .map(|f| part(*f))
        .collect::<StageResult<Vec<_>>>()?;
    Ok(univariant(&ms, 0))
}

/// Each variant's payload fields, each field's leaves as indices among
/// the enum's values.
pub type Positions = Vec<Vec<Vec<u32>>>;

/// An enum's layout (`docs/future-work/ENUM-LAYOUT.md` (b), tagged path):
/// in memory, its tag, then each variant's payload as one struct at one
/// offset; in locals, one Wasm value per distinct leaf of every variant
/// (owner, 2026-11-08: never two leaves in one value), as a [`Mem`] whose
/// leaves, in offset order, may overlap where variants do. Two variants
/// share a value where they have the same leaf at the same offset. The
/// tag's value (none for a [`nullable`] enum), and each variant's payload:
/// its fields' offsets, and each field leaf's offset in the enum and value.
///
/// A value of a variant not taken holds the bytes the taken variant puts
/// there ([`variant_steps`]), as a load of each value from memory gives:
/// so the values store in any order and copy with no branch on the tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Flat {
    pub mem: Mem,
    pub tag: Option<(Leaf, u32)>,
    pub positions: Positions,
    pub offsets: Vec<Vec<u32>>,
    pub leaves: Vec<Vec<Vec<Leaf>>>,
}

impl Flat {
    /// The values of field `f` of variant `v`.
    #[must_use]
    pub fn selection(&self, v: usize, f: usize) -> Vec<usize> {
        self.positions[v][f].iter().map(|&k| k as usize).collect()
    }

    /// The tag's value, for a [`Walk::Switch`]: a nullable enum's is its
    /// address, whose zero is variant 0.
    #[must_use]
    pub fn discr(&self) -> u32 {
        self.tag.map_or(0, |(_, k)| k)
    }
}

/// One step of building an enum's values, which the caller carries out:
/// push a payload leaf's value, or an instruction.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    Leaf(usize, usize),
    Op(I),
}

/// Carries out the steps `steps` ([`Step`]) in `f`: `leaf` pushes field
/// `f`'s leaf `k`.
pub fn run_steps(
    f: &mut crate::module::Func,
    steps: &[Step],
    leaf: &dyn Fn(&mut crate::module::Func, usize, usize),
) {
    for s in steps {
        match *s {
            Step::Leaf(fi, k) => leaf(f, fi, k),
            Step::Op(op) => f.op(op),
        }
    }
}

/// A narrow leaf's mask, as a 64-bit value.
fn mask64(bytes: u8) -> i64 {
    match bytes {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => -1,
    }
}

/// The steps that push the bytes the leaves `parts` of a variant put in
/// value `s`'s range, as `s` reads them: each part's bits, as a 64-bit
/// integer, shifted to its place in `s` and or-ed together, masked to
/// `s`'s width and read back as its type, sign-extended when `s` is a
/// narrow signed integer, as a load of `s` from memory reads it.
fn shadow_steps(s: Leaf, parts: &[(usize, usize, Leaf)], out: &mut Vec<Step>) {
    for (n, &(f, k, l)) in parts.iter().enumerate() {
        out.push(Step::Leaf(f, k));
        match l.val {
            Val::I32 => {
                if l.signed && l.bytes < 4 {
                    out.extend([
                        Step::Op(I::I32Const(if l.bytes == 1 { 0xff } else { 0xffff })),
                        Step::Op(I::I32And),
                    ]);
                }
                out.push(Step::Op(I::I64ExtendI32U));
            }
            Val::F32 => out.extend([Step::Op(I::I32ReinterpretF32), Step::Op(I::I64ExtendI32U)]),
            Val::I64 => {}
            Val::F64 => out.push(Step::Op(I::I64ReinterpretF64)),
        }
        if l.offset > s.offset {
            out.extend([
                Step::Op(I::I64Const(i64::from(8 * (l.offset - s.offset)))),
                Step::Op(I::I64Shl),
            ]);
        } else if l.offset < s.offset {
            out.extend([
                Step::Op(I::I64Const(i64::from(8 * (s.offset - l.offset)))),
                Step::Op(I::I64ShrU),
            ]);
        }
        if n > 0 {
            out.push(Step::Op(I::I64Or));
        }
    }
    if s.bytes < 8 {
        out.extend([Step::Op(I::I64Const(mask64(s.bytes))), Step::Op(I::I64And)]);
    }
    match s.val {
        Val::I32 => {
            out.push(Step::Op(I::I32WrapI64));
            match (s.signed, s.bytes) {
                (true, 1) => out.push(Step::Op(I::I32Extend8S)),
                (true, 2) => out.push(Step::Op(I::I32Extend16S)),
                (true | false, _) => {}
            }
        }
        Val::F32 => out.extend([Step::Op(I::I32WrapI64), Step::Op(I::F32ReinterpretI32)]),
        Val::I64 => {}
        Val::F64 => out.push(Step::Op(I::F64ReinterpretI64)),
    }
}

/// How to build variant `v`'s values: for each of the enum's values,
/// `None` where it is zero, else the steps that push it: the tag, a leaf
/// of the variant, or the bytes the variant's leaves put in another
/// variant's value ([`Flat`]).
///
/// Cost: one pass over the variant's leaves per value they overlap.
#[must_use]
pub fn variant_steps(flat: &Flat, v: u32) -> Vec<Option<Vec<Step>>> {
    let at = v as usize;
    let values = &flat.mem.leaves;
    let mut out: Vec<Option<Vec<Step>>> = vec![None; values.len()];
    let mut own: Vec<(usize, usize, Leaf)> = Vec::new();
    for (f, (ks, leaves)) in flat.positions[at].iter().zip(&flat.leaves[at]).enumerate() {
        for (k, (&slot, &l)) in ks.iter().zip(leaves).enumerate() {
            out[slot as usize] = Some(vec![Step::Leaf(f, k)]);
            own.push((f, k, l));
        }
    }
    if let Some((_, k)) = flat.tag
        && v != 0
    {
        out[k as usize] = Some(vec![Step::Op(I::I32Const(v.cast_signed()))]);
    }
    for (slot, s) in values.iter().enumerate() {
        if out[slot].is_some() || flat.tag.is_some_and(|(_, k)| k as usize == slot) {
            continue;
        }
        let end = s.offset + u32::from(s.bytes);
        let parts: Vec<(usize, usize, Leaf)> = own
            .iter()
            .copied()
            .filter(|(_, _, l)| l.offset < end && l.offset + u32::from(l.bytes) > s.offset)
            .collect();
        if !parts.is_empty() {
            let mut steps = Vec::new();
            shadow_steps(*s, &parts, &mut steps);
            out[slot] = Some(steps);
        }
    }
    out
}

/// The steps that push value `c` of a variant ([`variant_steps`]): its
/// steps, or a zero.
#[must_use]
pub fn value_or_zero(values: &[Leaf], c: usize, steps: Option<&Vec<Step>>) -> Vec<Step> {
    match steps {
        Some(s) => s.clone(),
        None => vec![Step::Op(zero_of(values[c].val))],
    }
}

/// A zero of `v`.
fn zero_of(v: Val) -> I {
    match v {
        Val::I32 => I::I32Const(0),
        Val::I64 => I::I64Const(0),
        Val::F32 => I::F32Const(wasm_encoder::Ieee32::new(0)),
        Val::F64 => I::F64Const(wasm_encoder::Ieee64::new(0)),
    }
}

/// The values among `values`, in offset order, whose bytes overlap any of
/// `leaves`'s: what holds a part's bytes, its own values and other
/// variants'.
#[must_use]
pub fn overlapping(values: &[Leaf], leaves: &[Leaf]) -> Vec<usize> {
    values
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            leaves.iter().any(|l| {
                l.offset < s.offset + u32::from(s.bytes) && l.offset + u32::from(l.bytes) > s.offset
            })
        })
        .map(|(k, _)| k)
        .collect()
}

/// Whether an enum of `variants` is a nullable address
/// (`types.option.repr`): variant 0 has no payload and variant 1 one thin
/// reference, never null. Its one value is that address, null for variant
/// 0; it has no tag.
pub fn nullable(pool: &InternPool, env: &dyn ProgramEnv, variants: &[Vec<Ty>]) -> bool {
    matches!(variants, [none, some] if none.is_empty()
        && some.len() == 1
        && matches!(kind(pool, env, some[0]), Kind::Ref { fat: false, .. }))
}

/// An enum's layout from its variants' payload types ([`Flat`]). Not
/// cached: [`flat`] is, by type.
pub fn enum_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    variants: &[Vec<Ty>],
) -> StageResult<Flat> {
    enum_mem_by(variants, nullable(pool, env, variants), &|f| {
        mem(pool, env, f)
    })
}

/// The layout of enum or optional type `t`, unboxed, once per type in a
/// link ([`cached`]).
pub fn flat(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<std::rc::Rc<Flat>> {
    let t = strip(pool, env, t);
    if let Some(got) = FLATS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&t)).cloned()) {
        return got;
    }
    let got = match variants(pool, env, t) {
        Some(vs) => enum_mem(pool, env, &vs).map(std::rc::Rc::new),
        None => unsupported("a variant of a type that is not an enum"),
    };
    FLATS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    got
}

/// [`enum_mem`], with each payload field's layout read by `part`: rustc's
/// tagged layout (`layout_of_enum`, layout.rs:751). The tag is the
/// smallest unsigned integer that counts the variants, widened to the
/// smallest first-field alignment of the payloads (layout.rs:891); each
/// payload is [`univariant`] behind it. A tag wider than 4 bytes keeps
/// its value in its low 4, and the rest is padding.
fn enum_mem_by(
    variants: &[Vec<Ty>],
    nullable: bool,
    part: &dyn Fn(Ty) -> StageResult<Mem>,
) -> StageResult<Flat> {
    if nullable {
        let mem = Mem::address();
        let leaf = mem.leaves[0];
        return Ok(Flat {
            mem,
            tag: None,
            positions: vec![vec![], vec![vec![0]]],
            offsets: vec![vec![], vec![0]],
            leaves: vec![vec![], vec![vec![leaf]]],
        });
    }
    let ms = variants
        .iter()
        .map(|fields| fields.iter().map(|f| part(*f)).collect())
        .collect::<StageResult<Vec<Vec<Mem>>>>()?;
    Ok(flat_of(&ms))
}

/// The tagged layout of an enum whose variants' payload fields are laid
/// out as `variants` ([`enum_mem_by`]).
pub(crate) fn flat_of(variants: &[Vec<Mem>]) -> Flat {
    let min: u32 = match variants.len() {
        0..=256 => 1,
        257..=65536 => 2,
        _ => 4,
    };
    let mut payloads = Vec::with_capacity(variants.len());
    let mut start_align = 256;
    for ms in variants {
        let (m, parts) = univariant(ms, min);
        if let Some(a) = ms
            .iter()
            .zip(&parts)
            .filter(|(f, _)| f.size > 0 || f.align > 1)
            .min_by_key(|(_, p)| p.offset)
            .map(|(f, _)| f.align)
        {
            start_align = start_align.min(a);
        }
        payloads.push((m, parts));
    }
    let ity = if start_align > min && start_align <= 8 {
        start_align
    } else {
        min
    };
    let mut size = ity;
    let mut align = ity;
    for (m, parts) in &mut payloads {
        // Only a zero-sized field sits right after the narrow tag.
        for p in parts.iter_mut() {
            if p.offset == min {
                p.offset = p.offset.max(ity);
            }
        }
        size = size.max(m.size);
        align = align.max(m.align);
    }
    let size = size.next_multiple_of(align);
    let tag = Leaf {
        offset: 0,
        val: Val::I32,
        bytes: match ity {
            1 => 1,
            2 => 2,
            _ => 4,
        },
        signed: false,
    };
    let mut values = vec![tag];
    for (m, _) in &payloads {
        values.extend(&m.leaves);
    }
    values.sort_stable();
    values.dedup();
    let index: hd_base::HashMap<Leaf, u32> = (0u32..).zip(&values).map(|(k, l)| (*l, k)).collect();
    let at = |l: &Leaf| index.get(l).copied().unwrap_or_default();
    let mut positions = Vec::with_capacity(payloads.len());
    let mut offsets = Vec::with_capacity(payloads.len());
    let mut leaves = Vec::with_capacity(payloads.len());
    for (m, parts) in &payloads {
        let fields: Vec<Vec<Leaf>> = parts
            .iter()
            .map(|p| m.leaves[p.first..p.first + p.count].to_vec())
            .collect();
        positions.push(
            fields
                .iter()
                .map(|ls| ls.iter().map(at).collect())
                .collect(),
        );
        offsets.push(parts.iter().map(|p| p.offset).collect());
        leaves.push(fields);
    }
    let tag_at = at(&tag);
    Flat {
        mem: Mem {
            size,
            align,
            leaves: values,
        },
        tag: Some((tag, tag_at)),
        positions,
        offsets,
        leaves,
    }
}

/// Whether a value of `t` holds, by value, a value whose type implements
/// `std.rt.Drop`: releasing it runs that implementation, so the value is
/// never bound twice (a `UniqList`).
pub fn holds_drop(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    memoized(&DROPS, pool, env, t, &|t, part| {
        if let TyData::Adt { def, .. } = pool.get(t)
            && env.has_drop(def)
        {
            return true;
        }
        value_parts(pool, env, t).into_iter().any(part)
    })
}

/// The variants' payload types of an enum or optional type, `None` for
/// any other type.
pub fn variants(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Vec<Vec<Ty>>> {
    let t = strip(pool, env, t);
    match pool.get(t) {
        // `None` is variant 0 and `Some` variant 1, as the tag says.
        TyData::Option(inner) => Some(vec![vec![], vec![inner]]),
        TyData::Union(members) => Some(union_variants(pool, members)),
        TyData::Adt { def, args } => env.enum_variants(def, args),
        _ => None,
    }
}

/// The width, signedness and float kind of a primitive type, as
/// arithmetic sees it: `bool` is an unsigned byte, `char` and `usize`
/// unsigned 32-bit words.
pub fn num(pool: &InternPool, t: Ty) -> (u8, bool, bool) {
    match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::I8 => (8, true, false),
            Prim::U8 | Prim::Bool => (8, false, false),
            Prim::I16 => (16, true, false),
            Prim::U16 => (16, false, false),
            Prim::I32 | Prim::String => (32, true, false),
            Prim::U32 | Prim::Usize | Prim::Char => (32, false, false),
            Prim::I64 => (64, true, false),
            Prim::U64 => (64, false, false),
            Prim::F32 => (32, true, true),
            Prim::F64 => (64, true, true),
        },
        _ => (32, false, false),
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn caches_are_absent() -> bool {
        MEMS.with(|m| m.borrow().is_none())
            && WALKS.with(|w| w.borrow().is_none())
            && DROPS.with(|d| d.borrow().is_none())
            && FLATS.with(|d| d.borrow().is_none())
    }

    #[test]
    fn a_panicking_cache_scope_restores_absent_caches() {
        assert!(caches_are_absent());
        let result = catch_unwind(AssertUnwindSafe(|| {
            cached(|| panic!("cache scope test"));
        }));
        assert!(result.is_err());
        assert!(caches_are_absent());
    }

    #[test]
    fn a_panicking_nested_cache_scope_restores_both_outer_caches() {
        cached(|| {
            MEMS.with(|m| {
                m.borrow_mut()
                    .as_mut()
                    .expect("outer layout cache")
                    .insert(Ty::VOID, Ok(Mem::EMPTY));
            });
            WALKS.with(|w| {
                w.borrow_mut()
                    .as_mut()
                    .expect("outer walk cache")
                    .insert(Ty::VOID, Some(Walk::Ref(7)));
            });

            let result = catch_unwind(AssertUnwindSafe(|| {
                cached(|| {
                    MEMS.with(|m| {
                        assert!(m.borrow().as_ref().is_some_and(hd_base::HashMap::is_empty));
                    });
                    WALKS.with(|w| {
                        assert!(w.borrow().as_ref().is_some_and(hd_base::HashMap::is_empty));
                    });
                    panic!("nested cache scope test");
                });
            }));
            assert!(result.is_err());

            MEMS.with(|m| {
                let m = m.borrow();
                let saved = m
                    .as_ref()
                    .and_then(|m| m.get(&Ty::VOID))
                    .expect("restored outer layout");
                assert!(saved.as_ref().is_ok_and(|m| *m == Mem::EMPTY));
            });
            WALKS.with(|w| {
                let w = w.borrow();
                assert_eq!(
                    w.as_ref().and_then(|w| w.get(&Ty::VOID)),
                    Some(&Some(Walk::Ref(7)))
                );
            });
        });
        assert!(caches_are_absent());
    }

    fn tuple_mem(pool: &InternPool, ty: Ty) -> StageResult<Mem> {
        if let Some(saved) = MEMS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&ty)).cloned()) {
            return saved;
        }
        let TyData::Tuple { elems, .. } = pool.get(ty) else {
            panic!("test tuple");
        };
        let item = pool.list_items(elems)[0];
        let computed = match pool.get(item) {
            TyData::Prim(Prim::I64) => Mem::one(Val::I64, 8, false),
            TyData::Prim(Prim::String) => record(&[Mem::address(), Mem::address()]).0,
            _ => panic!("test tuple item"),
        };
        MEMS.with(|m| {
            m.borrow_mut()
                .as_mut()
                .expect("layout cache")
                .insert(ty, Ok(computed.clone()));
        });
        Ok(computed)
    }

    #[test]
    fn a_later_layout_from_a_different_pool_matches_a_clean_cache() {
        let earlier = InternPool::default();
        let earlier_ty = earlier.intern_ty(&TyData::Tuple {
            elems: earlier.list(&[Ty::prim(Prim::I64)]),
            rest: None,
        });
        let later = InternPool::default();
        let later_ty = later.intern_ty(&TyData::Tuple {
            elems: later.list(&[Ty::STRING]),
            rest: None,
        });
        assert_eq!(earlier_ty, later_ty, "the pools reuse the same type index");
        let clean = cached(|| tuple_mem(&later, later_ty)).expect("clean layout");

        let result = catch_unwind(AssertUnwindSafe(|| {
            cached(|| {
                assert_eq!(
                    tuple_mem(&earlier, earlier_ty).expect("earlier layout"),
                    Mem::one(Val::I64, 8, false)
                );
                panic!("caught compiler panic");
            });
        }));
        assert!(result.is_err());

        let after = cached_here(|| tuple_mem(&later, later_ty)).expect("later layout");
        assert_eq!(after, clean);
    }
    fn i32m() -> Mem {
        Mem::one(Val::I32, 4, false)
    }

    fn values(f: &Flat) -> Vec<(u32, Val, u8)> {
        f.mem
            .leaves
            .iter()
            .map(|l| (l.offset, l.val, l.bytes))
            .collect()
    }

    #[test]
    fn a_payload_is_one_struct_behind_a_widened_tag() {
        // `Shape: Circle(f64); Rect(f64, f64); Tri(i32, i32, i32); Empty`:
        // the tag widens to 4 bytes and each payload starts after it. In
        // locals each leaf is its own value; `Circle` and `Rect` share the
        // `f64` at 8, and `Tri`'s `i32`s at 8 and 12 are their own.
        let f64m = Mem::one(Val::F64, 8, false);
        let shape = flat_of(&[
            vec![f64m.clone()],
            vec![f64m.clone(), f64m],
            vec![i32m(), i32m(), i32m()],
            vec![],
        ]);
        assert_eq!((shape.mem.size, shape.mem.align), (24, 8));
        assert_eq!(
            shape.offsets,
            [vec![8], vec![8, 16], vec![4, 8, 12], vec![]]
        );
        assert_eq!(
            values(&shape),
            [
                (0, Val::I32, 4),
                (4, Val::I32, 4),
                (8, Val::I32, 4),
                (8, Val::F64, 8),
                (12, Val::I32, 4),
                (16, Val::F64, 8)
            ]
        );
        assert_eq!(shape.positions[0], [vec![3]]);
        assert_eq!(shape.positions[1], [vec![3], vec![5]]);
        assert_eq!(shape.positions[2], [vec![1], vec![2], vec![4]]);
        // `Rect` puts its first `f64`'s halves in `Tri`'s `i32`s at 8 and
        // 12, so the values store in any order.
        let rect = variant_steps(&shape, 1);
        assert!(rect[2].is_some() && rect[4].is_some() && rect[1].is_none());
    }

    #[test]
    fn a_narrow_field_is_its_own_value() {
        // `Pair: L(bool, i32); M(i32)`: the tag stays a byte, `L`'s bool
        // packs after it in memory, and each is its own value.
        let bool_m = Mem::one(Val::I32, 1, false);
        let pair = flat_of(&[vec![bool_m.clone(), i32m()], vec![i32m()]]);
        assert_eq!((pair.mem.size, pair.mem.align), (8, 4));
        assert_eq!(
            values(&pair),
            [(0, Val::I32, 1), (1, Val::I32, 1), (4, Val::I32, 4)]
        );
        assert_eq!(pair.tag.map(|(_, k)| k), Some(0));
        assert_eq!(pair.positions, [vec![vec![1], vec![2]], vec![vec![2]]]);
        // `bool?` is two bytes and two values.
        let opt = flat_of(&[vec![], vec![bool_m]]);
        assert_eq!(values(&opt), [(0, Val::I32, 1), (1, Val::I32, 1)]);
    }

    #[test]
    fn a_struct_orders_its_fields_by_alignment_group() {
        // `Mixed { a: i32, b: f64, c: bool }` is 16 bytes, `b` first; its
        // leaves stay in declaration order.
        let (m, parts) = univariant(
            &[
                i32m(),
                Mem::one(Val::F64, 8, false),
                Mem::one(Val::I32, 1, false),
            ],
            0,
        );
        assert_eq!((m.size, m.align), (16, 8));
        assert_eq!(
            parts.iter().map(|p| p.offset).collect::<Vec<_>>(),
            [8, 0, 12]
        );
        assert_eq!(m.vals(), [Val::I32, Val::F64, Val::I32]);
    }
}
