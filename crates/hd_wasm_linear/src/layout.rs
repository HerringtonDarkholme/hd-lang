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

/// Bytes of a value that have invalid values (rustc's `Niche`,
/// `rustc_abi` lib.rs): the unsigned scalar leaf at `offset`, `bytes`
/// wide, whose valid values are `start..=end`, wrapping at its width. Its
/// other values are free for an enclosing enum's other variants
/// (`docs/future-work/ENUM-LAYOUT.md`). `size` is the scalar's whole
/// width, which a tag widened to 8 bytes keeps although its leaf is its
/// low 4: an enum places a variant after the niche past `size` bytes, as
/// rustc places it past `niche.value.size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Niche {
    pub offset: u32,
    pub bytes: u8,
    pub size: u8,
    pub start: u32,
    pub end: u32,
}

impl Niche {
    /// The largest value of the niche's width.
    #[must_use]
    pub fn max(self) -> u32 {
        u32::MAX >> (32 - 8 * u32::from(self.bytes))
    }

    /// How many values are free: rustc's `Niche::available`.
    #[must_use]
    pub fn available(self) -> u32 {
        self.start.wrapping_sub(self.end).wrapping_sub(1) & self.max()
    }

    /// `count` free values for an enclosing enum's variants (decision 3:
    /// always upward from the valid range's end, wrapping): the first one
    /// and the niche with them now valid. `None` when fewer are free.
    fn reserve(self, count: u32) -> Option<(u32, Niche)> {
        if count == 0 || count > self.available() {
            return None;
        }
        let start = self.end.wrapping_add(1) & self.max();
        let end = self.end.wrapping_add(count) & self.max();
        Some((start, Niche { end, ..self }))
    }

    fn at(self, offset: u32) -> Niche {
        Niche {
            offset: self.offset + offset,
            ..self
        }
    }
}

/// A type's layout: its size and alignment in memory, and its values; its
/// largest niche ([`Niche`]), and whether no value has the type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mem {
    pub size: u32,
    pub align: u32,
    pub leaves: Vec<Leaf>,
    pub niche: Option<Niche>,
    pub uninhabited: bool,
}

impl Mem {
    /// `never`, and an enum with no variant a value can have.
    const NEVER: Mem = Mem {
        size: 0,
        align: 1,
        leaves: Vec::new(),
        niche: None,
        uninhabited: true,
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
            niche: None,
            uninhabited: false,
        }
    }

    /// An unsigned `i32` scalar of `bytes` whose valid values are
    /// `start..=end`: `bool`, `char`, a non-null address.
    pub(crate) fn valid(bytes: u8, start: u32, end: u32) -> Mem {
        Mem {
            niche: Some(Niche {
                offset: 0,
                bytes,
                size: bytes,
                start,
                end,
            }),
            ..Mem::one(Val::I32, bytes, false)
        }
    }

    /// An address: a frame, a context, data no rule says is never null.
    pub(crate) fn address() -> Mem {
        Mem::one(Val::I32, 4, false)
    }

    /// An address that is never null (decision 2: null is an address's one
    /// niche): a thin reference, a boxed cell, a list's or a map's buffer,
    /// a string's bytes, a vtable.
    pub(crate) fn nonnull() -> Mem {
        Mem::valid(4, 1, u32::MAX)
    }

    /// A string: its bytes' address, never null, and its length.
    pub(crate) fn string() -> Mem {
        record(&[Mem::nonnull(), Mem::address()]).0
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
            Prim::U8 => Mem::one(Val::I32, 1, false),
            Prim::Bool => Mem::valid(1, 0, 1),
            Prim::I16 => Mem::one(Val::I32, 2, true),
            Prim::U16 => Mem::one(Val::I32, 2, false),
            Prim::I32 | Prim::U32 | Prim::Usize => Mem::one(Val::I32, 4, false),
            Prim::Char => Mem::valid(4, 0, 0x10_FFFF),
            Prim::I64 | Prim::U64 => Mem::one(Val::I64, 8, false),
            Prim::F32 => Mem::one(Val::F32, 4, false),
            Prim::F64 => Mem::one(Val::F64, 8, false),
            Prim::String => Mem::string(),
        },
        TyData::Never => Mem::NEVER,
        TyData::Tuple { .. } => payload_mem_by(&tuple_members(pool, t), part)?.0,
        TyData::Union(members) => {
            let variants = union_variants(pool, members);
            enum_flat_by(t, &variants, part)?
        }
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::Suspend => Mem::address(),
            StdKind::List | StdKind::Map => Mem::nonnull(),
            StdKind::Other => match kind(pool, env, t) {
                Kind::Boxed | Kind::Ref { fat: false, .. } => Mem::nonnull(),
                Kind::Ref { fat: true, .. } => record(&[Mem::address(), Mem::nonnull()]).0,
                Kind::Value => match env.enum_variants(def, args) {
                    Some(variants) => enum_flat_by(t, &variants, part)?,
                    None => payload_mem_by(&value_parts(pool, env, t), part)?.0,
                },
            },
        },
        // A function value's code is a table slot, which may be 0, and its
        // environment is 0 when it captures nothing: no niche.
        TyData::Fn { .. } => record(&[Mem::address(), Mem::address()]).0,
        TyData::TraitValue { .. } => record(&[Mem::address(), Mem::nonnull()]).0,
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
    part: &dyn Fn(Ty) -> StageResult<Mem>,
) -> StageResult<Mem> {
    let got = enum_mem_by(variants, part).map(std::rc::Rc::new);
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
    /// An enum's discriminant; arm `v`, when there is one, walks variant
    /// `v`'s payload, where the discriminant says `v` ([`Enc::is`]).
    /// The discriminant is boxed: a walk is part of many helpers' keys.
    Switch(Box<Discr>, Vec<Option<Walk>>),
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
            Walk::Switch(d, arms) => Walk::Switch(
                Box::new(Discr {
                    value: to(d.value),
                    ..**d
                }),
                arms.iter()
                    .map(|a| a.as_ref().map(|w| w.remap(to)))
                    .collect(),
            ),
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
            Walk::Switch(d, arms) => Some(Walk::Switch(
                d.clone(),
                arms.iter()
                    .map(|a| a.as_ref().and_then(|w| w.without(skip)))
                    .collect(),
            )),
        }
    }

    /// The value index when the walk is one counted address, of either
    /// kind.
    #[must_use]
    pub fn single(&self) -> Option<u32> {
        match self {
            Walk::Ref(k) | Walk::Known(k) => Some(*k),
            Walk::Glue | Walk::Seq(_) | Walk::Switch(..) => None,
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
            Walk::Glue | Walk::Seq(_) | Walk::Switch(..) => None,
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
        TyData::Union(_) => switch_walk_by(pool, env, t, part),
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
    let flat = flat(pool, env, t).ok()??;
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
    match flat.disc {
        // One variant: its walk, with no test.
        Disc::One(k) => arms.into_iter().nth(k as usize).flatten(),
        Disc::Cell(_) if arms.iter().all(Option::is_none) => None,
        Disc::Cell(d) => Some(Walk::Switch(Box::new(d), arms)),
    }
}

/// The captures of a closure environment in order, each at its alignment:
/// the whole layout and each capture's place in it. The environment's code
/// reads them at these offsets. Its niche is its fields' largest, the
/// first of equals.
fn record(fields: &[Mem]) -> (Mem, Vec<Part>) {
    let offsets: Vec<u32> = fields
        .iter()
        .scan(0u32, |end, f| {
            let offset = end.next_multiple_of(f.align);
            *end = offset + f.size;
            Some(offset)
        })
        .collect();
    place(fields, &offsets, 0, Bias::Start)
}

/// Which niche a struct keeps among its fields' equal largest ones, and
/// where [`univariant`] moves it: rustc's `NicheBias`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bias {
    Start,
    End,
}

/// The layout of `fields` at `offsets`, after `prefix` bytes: its leaves
/// in declaration order, each field's together, so a field's values in
/// locals are its [`Part`]; its niche the largest of its fields', the
/// first of equals in memory order (`Bias::Start`) or the last
/// (`Bias::End`), as rustc's `univariant_biased` keeps it.
fn place(fields: &[Mem], offsets: &[u32], prefix: u32, bias: Bias) -> (Mem, Vec<Part>) {
    let mut out = Mem {
        size: prefix,
        align: prefix.max(1),
        leaves: Vec::with_capacity(fields.iter().map(|f| f.leaves.len()).sum()),
        niche: None,
        uninhabited: false,
    };
    let mut parts = Vec::with_capacity(fields.len());
    for (f, &offset) in fields.iter().zip(offsets) {
        parts.push(Part {
            first: out.leaves.len(),
            count: f.leaves.len(),
            offset,
        });
        out.leaves.extend(f.leaves.iter().map(|l| Leaf {
            offset: l.offset + offset,
            ..*l
        }));
        out.size = out.size.max(offset + f.size);
        out.align = out.align.max(f.align);
        out.uninhabited |= f.uninhabited;
    }
    let mut by_offset: Vec<usize> = (0..fields.len()).collect();
    by_offset.sort_stable_by_key(|&k| offsets[k]);
    let mut best = 0;
    for k in by_offset {
        let Some(n) = fields[k].niche else {
            continue;
        };
        let more = match bias {
            Bias::Start => n.available() > best,
            Bias::End => n.available() >= best,
        };
        if more && n.available() > 0 {
            best = n.available();
            out.niche = Some(n.at(offsets[k]));
        }
    }
    out.size = out.size.next_multiple_of(out.align);
    (out, parts)
}

/// Fields placed as rustc's `univariant` places them (layout.rs:265,
/// 1091; decision 1 of `docs/future-work/ENUM-LAYOUT.md`): a struct's
/// (`prefix` 0) by alignment group, largest first, then its largest niche
/// first; a payload behind a tag of `prefix` bytes by group, smallest
/// first, the largest niche last, so that small fields pack after the
/// tag. When the niche is then neither at the start nor at the end, the
/// layout biased to put it last is kept where that leaves more room before
/// it than either edge had (layout.rs:288): an enclosing enum's other
/// variants fit there. Both sorts are stable.
///
/// Cost: two sorts of the fields, at most.
fn univariant(fields: &[Mem], prefix: u32) -> (Mem, Vec<Part>) {
    let got = univariant_biased(fields, prefix, Bias::Start);
    let Some(n) = got.0.niche else {
        return got;
    };
    let head = n.offset;
    let tail = got.0.size - head - u32::from(n.size);
    if fields.len() < 2 || head == 0 || tail == 0 {
        return got;
    }
    let alt = univariant_biased(fields, prefix, Bias::End);
    match alt.0.niche {
        Some(m) if m.offset > head && m.offset > tail => alt,
        Some(_) | None => got,
    }
}

/// [`univariant`] with one bias: rustc's `univariant_biased`.
fn univariant_biased(fields: &[Mem], prefix: u32, bias: Bias) -> (Mem, Vec<Part>) {
    let available = |f: &Mem| f.niche.map_or(0, Niche::available);
    let largest = fields.iter().map(available).max().unwrap_or(0);
    let max_align = fields.iter().map(|f| f.align).max().unwrap_or(1);
    // rustc's `alignment_group_key` (layout.rs:1155): `log2` of the larger
    // of a field's alignment and size, so four bytes at alignment 1 group
    // with the 4-aligned fields; with a niche about, capped at the largest
    // alignment (start bias), or the field with the largest niche at its
    // own alignment (end bias).
    let group = |f: &Mem| {
        let size_as_align = f.align.max(f.size).trailing_zeros();
        match bias {
            _ if largest == 0 => size_as_align,
            Bias::Start => max_align.trailing_zeros().min(size_as_align),
            Bias::End if available(f) == largest => f.align.trailing_zeros(),
            Bias::End => size_as_align,
        }
    };
    let mut order: Vec<usize> = (0..fields.len()).collect();
    if fields.len() > 1 {
        if prefix == 0 {
            order.sort_stable_by_key(|&k| {
                let f = &fields[k];
                let inner = f.niche.map_or(0, |n| match bias {
                    Bias::Start => u64::from(n.offset),
                    Bias::End => !u64::from(f.size - n.offset),
                });
                let niche = match bias {
                    Bias::Start => !u64::from(available(f)),
                    Bias::End => u64::from(available(f)),
                };
                (std::cmp::Reverse(group(f)), niche, inner)
            });
        } else {
            order.sort_stable_by_key(|&k| (group(&fields[k]), available(&fields[k])));
        }
    }
    let mut offsets = vec![0u32; fields.len()];
    let mut size = prefix;
    for &k in &order {
        let f = &fields[k];
        let offset = size.next_multiple_of(f.align);
        offsets[k] = offset;
        size = offset + f.size;
    }
    place(fields, &offsets, prefix, bias)
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

/// How an enum's discriminant cell ([`Discr`]) says its variant (rustc's
/// `TagEncoding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Enc {
    /// The cell is a tag holding the variant's index.
    Direct,
    /// No tag: variant `untagged` writes nothing, its payload's niche
    /// holding one of its valid values; variant `v` in `first..=last`
    /// writes `v - first + start` in the niche, wrapping at its `bytes`.
    Niche {
        untagged: u32,
        first: u32,
        last: u32,
        start: u32,
        bytes: u8,
    },
}

/// A test of a discriminant cell's value ([`Enc::is`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Is {
    /// No value is the variant: one absent beside a niche's variants.
    Never,
    /// The cell holds this value.
    Eq(u32),
    /// The cell holds none of the `count` values from `start`, wrapping at
    /// `bytes`: a niche's untagged variant.
    Outside { start: u32, count: u32, bytes: u8 },
}

impl Enc {
    /// The test that a value is variant `v` (`read_discr` against `v`).
    #[must_use]
    pub fn is(self, v: u32) -> Is {
        match self {
            Enc::Direct => Is::Eq(v),
            Enc::Niche {
                untagged,
                first,
                last,
                start,
                bytes,
            } => {
                if v == untagged {
                    Is::Outside {
                        start,
                        count: last - first + 1,
                        bytes,
                    }
                } else if (first..=last).contains(&v) {
                    Is::Eq(niche_value(v - first, start, bytes))
                } else {
                    Is::Never
                }
            }
        }
    }

    /// The value variant `v` writes in the cell (`write_discr`), `None` for
    /// the untagged one, which writes nothing.
    #[must_use]
    pub fn raw(self, v: u32) -> Option<u32> {
        match self {
            Enc::Direct => Some(v),
            Enc::Niche {
                untagged,
                first,
                start,
                bytes,
                ..
            } => (v != untagged).then(|| niche_value(v.wrapping_sub(first), start, bytes)),
        }
    }

    /// The variant a cell holding `raw` says (`read_discr` at compile time).
    #[must_use]
    pub fn reads(self, raw: u32) -> u32 {
        match self {
            Enc::Direct => raw,
            Enc::Niche {
                untagged,
                first,
                last,
                start,
                bytes,
            } => {
                let rel = niche_value(raw.wrapping_sub(start), 0, bytes);
                if rel <= last - first {
                    first + rel
                } else {
                    untagged
                }
            }
        }
    }
}

/// The `k`th value from `start` of a niche `bytes` wide.
fn niche_value(k: u32, start: u32, bytes: u8) -> u32 {
    start.wrapping_add(k) & (u32::MAX >> (32 - 8 * u32::from(bytes)))
}

impl Is {
    /// The instructions that turn the cell's value on the stack into an
    /// `i32` that is nonzero exactly when the test holds: one compare, or
    /// none for a null address's niche (the address itself), and a
    /// subtraction and a range compare for an untagged variant beside
    /// several niche values.
    #[must_use]
    pub fn ops(self) -> Vec<I> {
        match self {
            Is::Never => vec![I::Drop, I::I32Const(0)],
            Is::Eq(0) => vec![I::I32Eqz],
            Is::Eq(k) => vec![I::I32Const(k.cast_signed()), I::I32Eq],
            Is::Outside {
                start: 0, count: 1, ..
            } => vec![],
            Is::Outside {
                start, count: 1, ..
            } => {
                vec![I::I32Const(start.cast_signed()), I::I32Ne]
            }
            Is::Outside {
                start,
                count,
                bytes,
            } => {
                let mut ops = Is::rel(start, count, bytes);
                ops.extend([I::I32Const((count - 1).cast_signed()), I::I32GtU]);
                ops
            }
        }
    }

    /// The instructions that turn a niche cell on the stack into its
    /// index among the `count` values from `start`: any index past them
    /// for any other value. A narrow niche whose values wrap past its
    /// width is masked; one that does not wrap needs no mask, as a value
    /// below `start` leaves a negative, a large unsigned index.
    #[must_use]
    pub fn rel(start: u32, count: u32, bytes: u8) -> Vec<I> {
        let max = u32::MAX >> (32 - 8 * u32::from(bytes));
        let mut ops = Vec::new();
        if start != 0 {
            ops.extend([I::I32Const(start.cast_signed()), I::I32Sub]);
        }
        if bytes < 4 && u64::from(start) + u64::from(count) > u64::from(max) + 1 {
            ops.extend([I::I32Const(max.cast_signed()), I::I32And]);
        }
        ops
    }
}

/// An enum's discriminant cell: the leaf that holds its tag or niche,
/// that leaf's index among the enum's values, and how it says the
/// variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Discr {
    pub leaf: Leaf,
    pub value: u32,
    pub enc: Enc,
}

/// What says an enum value's variant (rustc's `Variants`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Disc {
    /// One variant is present (`Variants::Single`): no bytes say it.
    One(u32),
    /// Its discriminant cell (`Variants::Multiple`).
    Cell(Discr),
}

/// How to test that an enum value is a variant ([`Flat::test`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// Known with no read: an enum of one variant.
    Const(bool),
    /// A test of the discriminant cell's value.
    Read(Discr, Is),
}

/// Pushes the variant index of an enum whose cell's value `push` pushes,
/// given the value's index (`read_discr`,
/// `docs/future-work/ENUM-LAYOUT.md`): the tag itself, a
/// constant, or for a niche one compare and a `select`. Several niche
/// values need the index twice, in a local of `f`'s.
pub fn read_discr(
    f: &mut crate::module::Func,
    disc: Disc,
    push: &dyn Fn(&mut crate::module::Func, u32),
) {
    let d = match disc {
        Disc::One(k) => return f.op(I::I32Const(k.cast_signed())),
        Disc::Cell(d) => d,
    };
    match d.enc {
        Enc::Direct => push(f, d.value),
        Enc::Niche {
            untagged,
            first,
            last,
            start,
            ..
        } if first == last => {
            f.op(I::I32Const(first.cast_signed()));
            f.op(I::I32Const(untagged.cast_signed()));
            push(f, d.value);
            for op in Is::Eq(start).ops() {
                f.op(op);
            }
            f.op(I::Select);
        }
        Enc::Niche {
            untagged,
            first,
            last,
            start,
            bytes,
        } => {
            let rel = f.local(Val::I32);
            push(f, d.value);
            for op in Is::rel(start, last - first + 1, bytes) {
                f.op(op);
            }
            f.op(I::LocalTee(rel));
            f.op(I::I32Const(first.cast_signed()));
            f.op(I::I32Add);
            f.op(I::I32Const(untagged.cast_signed()));
            f.op(I::LocalGet(rel));
            f.op(I::I32Const((last - first).cast_signed()));
            f.op(I::I32LeU);
            f.op(I::Select);
        }
    }
}

/// An enum's layout (`docs/future-work/ENUM-LAYOUT.md`): in memory, each
/// variant's payload as one struct at one offset, behind a tag or around
/// a niche ([`Disc`]); in locals, one Wasm value per distinct leaf of
/// every variant (owner, 2026-11-08: never two leaves in one value), as a
/// [`Mem`] whose leaves, in offset order, may overlap where variants do.
/// Two variants share a value where they have the same leaf at the same
/// offset. What says the variant, and each variant's payload: its
/// fields' offsets, and each field leaf's offset in the enum and value.
///
/// A value of a variant not taken holds the bytes the taken variant puts
/// there ([`variant_steps`]), as a load of each value from memory gives:
/// so the values store in any order and copy with no branch on the tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Flat {
    pub mem: Mem,
    pub disc: Disc,
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

    /// The test that a value is variant `v`.
    #[must_use]
    pub fn test(&self, v: u32) -> Test {
        match self.disc {
            Disc::One(k) => Test::Const(v == k),
            Disc::Cell(d) => Test::Read(d, d.enc.is(v)),
        }
    }

    /// The discriminant cell; `None` for one variant.
    #[must_use]
    pub fn cell(&self) -> Option<Discr> {
        match self.disc {
            Disc::One(_) => None,
            Disc::Cell(d) => Some(d),
        }
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
/// `None` where it is zero, else the steps that push it: the tag or niche
/// value it writes (`write_discr`), a leaf
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
    // `write_discr`: the tag, or the niche value of a variant that is not
    // the untagged one, in the cell.
    let written = flat
        .cell()
        .and_then(|d| Some((d.value as usize, d.enc.raw(v)?)));
    if let Some((k, raw)) = written
        && raw != 0
    {
        out[k] = Some(vec![Step::Op(I::I32Const(raw.cast_signed()))]);
    }
    for (slot, s) in values.iter().enumerate() {
        if out[slot].is_some() || written.is_some_and(|(k, _)| k == slot) {
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

/// An enum's layout from its variants' payload types ([`Flat`]). Not
/// cached: [`flat`] is, by type.
pub fn enum_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    variants: &[Vec<Ty>],
) -> StageResult<Flat> {
    enum_mem_by(variants, &|f| mem(pool, env, f))
}

/// The layout of enum or optional type `t`, unboxed, once per type in a
/// link ([`cached`]); `None` for a type that is neither.
pub fn flat(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
) -> StageResult<Option<std::rc::Rc<Flat>>> {
    let t = strip(pool, env, t);
    let Some(vs) = variants(pool, env, t) else {
        return Ok(None);
    };
    if let Some(got) = FLATS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&t)).cloned()) {
        return got.map(Some);
    }
    let got = enum_mem(pool, env, &vs).map(std::rc::Rc::new);
    FLATS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    got.map(Some)
}

/// [`enum_mem`], with each payload field's layout read by `part`.
fn enum_mem_by(variants: &[Vec<Ty>], part: &dyn Fn(Ty) -> StageResult<Mem>) -> StageResult<Flat> {
    let ms = variants
        .iter()
        .map(|fields| fields.iter().map(|f| part(*f)).collect())
        .collect::<StageResult<Vec<Vec<Mem>>>>()?;
    Ok(flat_of(&ms))
}

/// The layout of an enum whose variants' payload fields are laid out as
/// `variants`: rustc's `layout_of_enum` (layout.rs:573). Of the
/// niche-filling layout ([`niche_filled`]) and the tagged one
/// ([`tagged`]), the smaller; of equal sizes, the one with more free
/// niche values; else the tagged one (layout.rs:1070).
///
/// Cost: [`univariant`] twice per variant, and one pass over the leaves.
pub(crate) fn flat_of(variants: &[Vec<Mem>]) -> Flat {
    // rustc's `layout_of_struct_or_enum` (layout.rs:340): with no variant
    // present the enum is `never`, and with one it is that variant's
    // struct, with no discriminant.
    let mut present = (0..variants.len()).filter(|&i| !absent(&variants[i]));
    match (present.next(), present.next()) {
        (None, _) => return single(variants, None),
        (Some(k), None) => return single(variants, Some(k)),
        (Some(_), Some(_)) => {}
    }
    let tagged = tagged(variants);
    let free = |f: &Flat| f.mem.niche.map_or(0, Niche::available);
    match niche_filled(variants) {
        Some(n)
            if n.mem.size < tagged.mem.size
                || n.mem.size == tagged.mem.size && free(&n) > free(&tagged) =>
        {
            n
        }
        Some(_) | None => tagged,
    }
}

/// Whether a variant of `fields` is uninhabited: a field is.
fn uninhabited(fields: &[Mem]) -> bool {
    fields.iter().any(|f| f.uninhabited)
}

/// Whether a variant of `fields` is absent (rustc, layout.rs:88): no value
/// has it, and its fields take no bytes and no alignment, so it needs
/// neither a discriminant nor space. `B(never)` is; `B(i64, never)` is
/// not, and keeps its space.
fn absent(fields: &[Mem]) -> bool {
    uninhabited(fields) && fields.iter().all(|f| f.size == 0 && f.align == 1)
}

/// The layout of an enum whose one present variant is `only`
/// (rustc's `Variants::Single`): that variant's struct, every absent one
/// empty at offset 0, and no discriminant. With none present the enum is
/// `never`: no bytes, and no value.
fn single(variants: &[Vec<Mem>], only: Option<usize>) -> Flat {
    let payloads: Vec<(Mem, Vec<Part>)> = variants.iter().map(|ms| univariant(ms, 0)).collect();
    let (mem, k) = match only {
        Some(k) => (payloads[k].0.clone(), k),
        None => (Mem::NEVER, 0),
    };
    // ruled out: more variants than u32 counts; an enum's variants are
    // `u32` indices in TIR.
    let k = u32::try_from(k).expect("a variant index fits u32");
    assemble(
        &payloads,
        mem.size,
        mem.align,
        mem.niche,
        Say::One(k),
        variants,
    )
}

/// The niche-filling layout (rustc's `layout_of_enum` step 1,
/// layout.rs:591): the largest variant (the last of equals) is untagged,
/// its largest niche holding the other variants' values, `reserve`d
/// upward from its valid range's end (decision 3); every other variant
/// fits before the niche or after it within the untagged one's size.
/// `None` when the largest has no niche with room, or a variant does not
/// fit.
fn niche_filled(variants: &[Vec<Mem>]) -> Option<Flat> {
    let mut payloads: Vec<(Mem, Vec<Part>)> = variants.iter().map(|ms| univariant(ms, 0)).collect();
    let untagged = (0..payloads.len()).max_by_key(|&i| payloads[i].0.size)?;
    let needs: Vec<usize> = (0..payloads.len())
        .filter(|&i| i != untagged && !absent(&variants[i]))
        .collect();
    let (&first, &last) = (needs.first()?, needs.last()?);
    let count = u32::try_from(last - first + 1).ok()?;
    let niche = payloads[untagged].0.niche?;
    let (start, valid) = niche.reserve(count)?;
    let align = payloads.iter().map(|(m, _)| m.align).max().unwrap_or(1);
    let size = payloads[untagged].0.size.next_multiple_of(align);
    for (i, (m, parts)) in payloads.iter_mut().enumerate() {
        if i == untagged || m.size <= niche.offset {
            continue;
        }
        let o = (niche.offset + u32::from(niche.size)).next_multiple_of(m.align);
        if o + m.size > size {
            return None;
        }
        for l in &mut m.leaves {
            l.offset += o;
        }
        for p in parts.iter_mut() {
            p.offset += o;
        }
    }
    let cell = Leaf {
        offset: niche.offset,
        val: Val::I32,
        bytes: niche.bytes,
        signed: false,
    };
    let enc = Enc::Niche {
        untagged: u32::try_from(untagged).ok()?,
        first: u32::try_from(first).ok()?,
        last: u32::try_from(last).ok()?,
        start,
        bytes: niche.bytes,
    };
    Some(assemble(
        &payloads,
        size,
        align,
        Some(valid),
        Say::Cell(cell, enc),
        variants,
    ))
}

/// The tagged layout (rustc's `layout_of_enum` step 2, layout.rs:751): the
/// tag is the smallest unsigned integer that counts the inhabited
/// variants, widened to the smallest first-field alignment of the
/// payloads (layout.rs:891); each payload is [`univariant`] behind it. A
/// tag wider than 4 bytes keeps its value in its low 4, and the rest is
/// padding. Its niche is the tag's values no inhabited variant takes.
fn tagged(variants: &[Vec<Mem>]) -> Flat {
    // The discriminants of the inhabited variants; with none, 0 alone.
    let mut live = (0u32..)
        .zip(variants)
        .filter(|(_, fs)| !uninhabited(fs))
        .map(|(k, _)| k);
    let lo = live.next().unwrap_or(0);
    let hi = live.last().unwrap_or(lo);
    let min: u32 = match hi {
        0..=0xff => 1,
        0x100..=0xffff => 2,
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
    let bytes: u8 = match ity {
        1 => 1,
        2 => 2,
        _ => 4,
    };
    let tag = Leaf {
        offset: 0,
        val: Val::I32,
        bytes,
        signed: false,
    };
    let niche = Niche {
        offset: 0,
        bytes,
        // ruled out: a tag wider than 8 bytes; `ity` is at most 8.
        size: u8::try_from(ity).expect("a tag of at most 8 bytes"),
        start: lo,
        end: hi,
    };
    assemble(
        &payloads,
        size,
        align,
        Some(niche),
        Say::Cell(tag, Enc::Direct),
        variants,
    )
}

/// What says the variant of an enum being assembled ([`Disc`]): the one
/// variant, or the cell's leaf and how it says it.
#[derive(Clone, Copy)]
enum Say {
    One(u32),
    Cell(Leaf, Enc),
}

/// An enum's [`Flat`] from its payloads at their places: its values, one
/// per distinct leaf of the cell and the payloads, in offset order, and
/// each payload field's values among them.
fn assemble(
    payloads: &[(Mem, Vec<Part>)],
    size: u32,
    align: u32,
    niche: Option<Niche>,
    say: Say,
    variants: &[Vec<Mem>],
) -> Flat {
    let mut values: Vec<Leaf> = match say {
        Say::One(_) => Vec::new(),
        Say::Cell(cell, _) => vec![cell],
    };
    for (m, _) in payloads {
        values.extend(&m.leaves);
    }
    values.sort_stable();
    values.dedup();
    let index: hd_base::HashMap<Leaf, u32> = (0u32..).zip(&values).map(|(k, l)| (*l, k)).collect();
    // Every leaf looked up is one of `values`, which holds them all.
    let at = |l: &Leaf| index[l];
    let mut positions = Vec::with_capacity(payloads.len());
    let mut offsets = Vec::with_capacity(payloads.len());
    let mut leaves = Vec::with_capacity(payloads.len());
    for (m, parts) in payloads {
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
    let disc = match say {
        Say::One(k) => Disc::One(k),
        Say::Cell(leaf, enc) => Disc::Cell(Discr {
            leaf,
            value: at(&leaf),
            enc,
        }),
    };
    Flat {
        disc,
        mem: Mem {
            size,
            align,
            leaves: values,
            niche: niche.filter(|n| n.available() > 0),
            uninhabited: variants.iter().all(|fs| uninhabited(fs)),
        },
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
                    .insert(Ty::VOID, Ok(Mem::NEVER));
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
                assert!(saved.as_ref().is_ok_and(|m| *m == Mem::NEVER));
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
            TyData::Prim(Prim::String) => Mem::string(),
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
        assert_eq!(pair.cell().map(|d| d.value), Some(0));
        assert_eq!(pair.positions, [vec![vec![1], vec![2]], vec![vec![2]]]);
        // `bool?` is two bytes and two values.
        let opt = flat_of(&[vec![], vec![bool_m]]);
        assert_eq!(values(&opt), [(0, Val::I32, 1), (1, Val::I32, 1)]);
    }

    #[test]
    fn a_nested_optional_fills_the_inner_tags_niche() {
        // `i32??` (the doc's walkthrough): `i32?`'s 4-byte tag holds 0 or 1,
        // so the outer `None` is 2 in it, with no tag of its own.
        let inner = flat_of(&[vec![], vec![i32m()]]);
        assert_eq!(inner.cell().map(|d| d.enc), Some(Enc::Direct));
        let outer = flat_of(&[vec![], vec![inner.mem.clone()]]);
        assert_eq!((outer.mem.size, outer.mem.align), (8, 4));
        let enc = |f: &Flat| f.cell().map(|d| d.enc);
        assert_eq!(
            enc(&outer),
            Some(Enc::Niche {
                untagged: 1,
                first: 0,
                last: 0,
                start: 2,
                bytes: 4
            })
        );
        assert_eq!(values(&outer), [(0, Val::I32, 4), (4, Val::I32, 4)]);
        let raw = |f: &Flat, v| f.cell().and_then(|d| d.enc.raw(v));
        assert_eq!(raw(&outer, 0), Some(2));
        assert_eq!(raw(&outer, 1), None);
        // Each level takes the next value up: `i32???`'s `None` is 3.
        let third = flat_of(&[vec![], vec![outer.mem.clone()]]);
        assert_eq!(third.mem.size, 8);
        assert_eq!(raw(&third, 0), Some(3));
        let reads = |raw| third.cell().map(|d| d.enc.reads(raw));
        assert_eq!(reads(0), Some(1));
        assert_eq!(reads(3), Some(0));
    }

    #[test]
    fn a_smaller_variant_fits_after_the_niche() {
        // `A: Maybe(i32?); Must(i32)` (the doc's walkthrough): `Must` is
        // 2 in `Maybe`'s inner tag, its `i32` at 4, sharing `Some`'s value.
        let opt = flat_of(&[vec![], vec![i32m()]]);
        let a = flat_of(&[vec![opt.mem.clone()], vec![i32m()]]);
        assert_eq!((a.mem.size, a.mem.align), (8, 4));
        assert_eq!(
            a.cell().map(|d| d.enc),
            Some(Enc::Niche {
                untagged: 0,
                first: 1,
                last: 1,
                start: 2,
                bytes: 4
            })
        );
        assert_eq!(a.offsets, [vec![0], vec![4]]);
        assert_eq!(a.positions, [vec![vec![0, 1]], vec![vec![1]]]);
        let is = |v| match a.test(v) {
            Test::Read(_, is) => Some(is),
            Test::Const(_) => None,
        };
        assert_eq!(is(1), Some(Is::Eq(2)));
        assert_eq!(
            is(0),
            Some(Is::Outside {
                start: 2,
                count: 1,
                bytes: 4
            })
        );
    }

    #[test]
    fn scalar_niches_extend_upward_from_their_valid_end() {
        // Decision 3: `char?`'s `None` is 0x110000; `bool???` is one byte,
        // its `None`s 2, 3 and 4; a null address is `(*T)?`'s `None`.
        let char_opt = flat_of(&[vec![], vec![Mem::valid(4, 0, 0x10_FFFF)]]);
        assert_eq!(char_opt.mem.size, 4);
        let raw = |f: &Flat, v| f.cell().and_then(|d| d.enc.raw(v));
        assert_eq!(raw(&char_opt, 0), Some(0x11_0000));
        let mut b = Mem::valid(1, 0, 1);
        for none in 2..=4 {
            let f = flat_of(&[vec![], vec![b.clone()]]);
            assert_eq!((f.mem.size, raw(&f, 0)), (1, Some(none)));
            b = f.mem.clone();
        }
        let ptr = flat_of(&[vec![], vec![Mem::nonnull()]]);
        assert_eq!(
            (ptr.mem.size, raw(&ptr, 0), ptr.mem.niche),
            (4, Some(0), None)
        );
    }

    #[test]
    fn an_absent_variant_takes_no_discriminant() {
        // `WithNever: A(i32); B(never)`: `B` is absent, so the enum is
        // `A`'s struct, with no tag (rustc's `Variants::Single`).
        let one = flat_of(&[vec![i32m()], vec![Mem::NEVER]]);
        assert_eq!((one.mem.size, one.disc), (4, Disc::One(0)));
        assert_eq!(one.test(0), Test::Const(true));
        assert_eq!(one.test(1), Test::Const(false));
        // `WithNever2: A(i32); B(i64, never); C`: `B` is uninhabited but
        // not absent, so it keeps its space behind the tag.
        let i64m = Mem::one(Val::I64, 8, false);
        let two = flat_of(&[vec![i32m()], vec![i64m, Mem::NEVER], vec![]]);
        assert_eq!(
            (two.mem.size, two.cell().map(|d| d.enc)),
            (16, Some(Enc::Direct))
        );
        // No variant present: `never`.
        let none = flat_of(&[vec![Mem::NEVER]]);
        assert!(none.mem.uninhabited && none.mem.size == 0);
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
