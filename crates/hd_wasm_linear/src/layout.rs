//! Linear layouts (`crates/hd_wasm_linear/DESIGN.md` §23.3): a type's Wasm
//! values in locals, parameters and results, and the same values in
//! memory, each at an offset.
//!
//! One walk gives both: a type's [`Mem`] lists its values as leaves, in
//! the order its locals hold them, each with its offset and width in
//! memory. A struct or a tuple is its fields' leaves, each field at its
//! alignment. An enum is its tag, then slots that its variants share: the
//! `k`th value of one Wasm type in a variant's payload is in that type's
//! `k`th slot, so a variant fills only the slots it needs and loads and
//! stores need no branch on the tag. A data value, a self-recursive enum
//! and a suspension are one address; a string is its address and byte
//! length; a trait value its data address and its vtable's address; a
//! list or a map the address of its buffer (§23.7); a function value its
//! code's table slot and its environment's address (§23.8). A reference,
//! the checker's `std.core.Ref[T]`, is the address of its place (§23.5,
//! §23.20); a reference to a trait value is that trait value, whose data
//! is already one address, as Rust's `&dyn Tr` is a fat pointer.

use hd_base::StageResult;
use hd_mono::layout::{StdKind, strip_newtype};
use hd_mono::{ProgramEnv, subst};
use hd_types::{InternPool, Prim, Ty, TyData};

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
#[derive(Clone, Debug, PartialEq, Eq)]
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

    fn one(val: Val, bytes: u8, signed: bool) -> Mem {
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
    fn address() -> Mem {
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
    /// A data value: the address of an object whose payload holds
    /// `fields` (`§23.4`).
    Data(Vec<Ty>),
    /// A self-recursive enum: the address of a cell holding the enum's
    /// value as a non-recursive enum lays it out.
    Boxed,
    /// A reference to a place of type `T` (`std.core.Ref[T]`): the
    /// place's address, or, when `T` is a trait value (`fat`), that trait
    /// value.
    Ref { target: Ty, fat: bool },
}

/// Which kind `t` is.
pub fn kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Kind {
    let t = strip(pool, env, t);
    let TyData::Adt { def, args } = pool.get(t) else {
        return Kind::Value;
    };
    if def == env.ref_cell() {
        let target = pool
            .list_items(args)
            .first()
            .map_or(Ty::VOID, |a| strip(pool, env, *a));
        // A reference to an open trait value is one open value
        // (`emit/erased.rs`), however the caller lays it out.
        let fat = matches!(pool.get(target), TyData::TraitValue { .. })
            && !hd_mono::is_open(pool, target);
        return Kind::Ref { target, fat };
    }
    if env.std_kind(def) != StdKind::Other {
        return Kind::Value;
    }
    if env.enum_variants(def, args).is_some() {
        return if env.recursive_enum(def) {
            Kind::Boxed
        } else {
            Kind::Value
        };
    }
    match env.data_fields(def) {
        Some(fs) if !env.is_struct(def) => Kind::Data(
            fs.into_iter()
                .map(|f| subst(pool, env, def, args, f))
                .collect(),
        ),
        _ => Kind::Value,
    }
}

/// Whether reference type `t` (`std.core.Ref[T]`) is `&mut`: its target
/// is `Mut[T]` (`hd_types::Types::lower_refs`).
pub fn ref_mutable(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    match pool.get(strip(pool, env, t)) {
        TyData::Adt { def, args } if def == env.ref_cell() => pool
            .list_items(args)
            .first()
            .is_some_and(|a| matches!(pool.get(*a), TyData::Mut(_))),
        _ => false,
    }
}

/// `t` with newtypes erased and `mut` dropped: what its layout reads.
pub fn strip(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Ty {
    let mut t = t;
    loop {
        let s = strip_newtype(pool, env, t);
        match pool.get(s) {
            TyData::Mut(inner) => t = inner,
            _ => return s,
        }
    }
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

/// The values of a `t` value that are collections a copy of it copies
/// (`hd_mono::holds_coll`): a list's own, and each such value of a struct
/// field, at any depth of structs. A legacy `mut` view shares its list
/// and holds none.
pub fn coll_leaves(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<u32> {
    // An open value's copies are made by the thunks that know its type.
    if hd_mono::is_open(pool, t) || !hd_mono::holds_coll(pool, env, t) {
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
                && env.enum_variants(def, args).is_none() =>
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
    if hd_mono::is_open(pool, t) {
        return Vec::new();
    }
    match pool.get(t) {
        TyData::Tuple { .. } => tuple_members(pool, t),
        TyData::Option(inner) => vec![inner],
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
}

/// Runs `link` with the layout cache in place: within it, [`mem`] and
/// [`walk`] compute each type once, so a long chain of types held by
/// value costs its length, not its square, at every use.
pub fn cached<R>(link: impl FnOnce() -> R) -> R {
    let outer = (MEMS.take(), WALKS.take());
    MEMS.set(Some(hd_base::HashMap::default()));
    WALKS.set(Some(hd_base::HashMap::default()));
    let r = link();
    MEMS.set(outer.0);
    WALKS.set(outer.1);
    r
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
    // A value of an open type in an erased body is one counted address,
    // whatever the caller's type (`emit/erased.rs`).
    if hd_mono::is_open(pool, t) {
        return Ok(Mem::address());
    }
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
        TyData::Option(inner) => enum_mem_by(&[vec![], vec![inner]], part)?.0,
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::Suspend => Mem::address(),
            StdKind::List | StdKind::Map | StdKind::Buffer => Mem::address(),
            StdKind::Other => match kind(pool, env, t) {
                Kind::Data(_) | Kind::Boxed | Kind::Ref { fat: false, .. } => Mem::address(),
                Kind::Ref { fat: true, .. } => record(&[Mem::address(), Mem::address()]).0,
                Kind::Value => match env.enum_variants(def, args) {
                    Some(variants) => enum_mem_by(&variants, part)?.0,
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

/// Where a value's counted references are among its values (§23.4): what
/// counting walks to retain or release the value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Walk {
    /// Value `k` is an address in a counted object, or in static data: a
    /// string's bytes, a reference, an open value; counting finds its
    /// block through the page table (§23.5).
    Ref(u32),
    /// Value `k` is null, a static object's, or the payload start of a
    /// counted object, eight bytes after its header: a data value, a boxed
    /// enum's cell, a collection's buffer, a closure's environment
    /// ([`block_start`]); counting reads the header at `k - 8` (§23.30).
    Known(u32),
    /// Each part in turn.
    Seq(Vec<Walk>),
    /// Value `k` is an enum's tag; arm `v`, when there is one, walks
    /// variant `v`'s payload.
    Switch(u32, Vec<Option<Walk>>),
}

impl Walk {
    /// The walk with each value index `k` replaced by `to(k)`.
    #[must_use]
    pub fn remap(&self, to: &dyn Fn(u32) -> u32) -> Walk {
        match self {
            Walk::Ref(k) => Walk::Ref(to(*k)),
            Walk::Known(k) => Walk::Known(to(*k)),
            Walk::Seq(ws) => Walk::Seq(ws.iter().map(|w| w.remap(to)).collect()),
            Walk::Switch(k, arms) => Walk::Switch(
                to(*k),
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
            Walk::Seq(ws) => Walk::seq(ws.iter().filter_map(|w| w.without(skip)).collect()),
            Walk::Switch(k, arms) => Some(Walk::Switch(
                *k,
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
            Walk::Seq(_) | Walk::Switch(..) => None,
        }
    }

    fn seq(mut ws: Vec<Walk>) -> Option<Walk> {
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
        walk_one(pool, env, u, part)
    })
}

/// `t`'s walk from its parts' walks, which `part` reads.
fn walk_one(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    part: &dyn Fn(Ty) -> Option<Walk>,
) -> Option<Walk> {
    let t = strip(pool, env, t);
    if hd_mono::is_open(pool, t) {
        return Some(Walk::Ref(0));
    }
    match pool.get(t) {
        TyData::Prim(Prim::String) => Some(Walk::Ref(0)),
        TyData::Fn { .. } => Some(Walk::Known(1)),
        TyData::TraitValue { .. } => Some(Walk::Ref(0)),
        TyData::Context(_) => Some(Walk::Known(0)),
        TyData::Tuple { .. } => fields_walk_by(pool, env, &tuple_members(pool, t), part),
        TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => Some(Walk::Known(0)),
        TyData::Option(inner) => switch_walk_by(pool, env, &[vec![], vec![inner]], part),
        TyData::Adt { def, args } if env.std_kind(def) == StdKind::Other => {
            match kind(pool, env, t) {
                Kind::Data(_) | Kind::Boxed => Some(Walk::Known(0)),
                Kind::Ref { .. } => Some(Walk::Ref(0)),
                Kind::Value => {
                    if let Some(variants) = env.enum_variants(def, args) {
                        return switch_walk_by(pool, env, &variants, part);
                    }
                    fields_walk_by(pool, env, &value_parts(pool, env, t), part)
                }
            }
        }
        TyData::Canon(_) if hd_mono::is_class_ref(pool, t) => Some(Walk::Ref(0)),
        _ => None,
    }
}

/// Whether this representation's counted pointer is a payload start,
/// never a string's interior bytes or an interior/erased reference.
pub fn block_start(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let t = strip(pool, env, t);
    if hd_mono::is_open(pool, t) {
        return false;
    }
    match pool.get(t) {
        TyData::Fn { .. } | TyData::Context(_) => true,
        TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => true,
        _ => matches!(kind(pool, env, t), Kind::Data(_) | Kind::Boxed),
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
    let Some(w) = walk(pool, env, t) else {
        return Pass::Uncounted;
    };
    if matches!(kind(pool, env, t), Kind::Ref { .. }) {
        Pass::Borrowed
    } else {
        Pass::Owned(w)
    }
}

/// The walk of fields laid out in order, as a struct, a tuple or a data
/// payload lays them out.
pub fn fields_walk(pool: &InternPool, env: &dyn ProgramEnv, fields: &[Ty]) -> Option<Walk> {
    fields_walk_by(pool, env, fields, &|f| walk(pool, env, f))
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
                walk(pool, env, *t).map(|w| w.remap(&|k| at + k))
            }
        })
        .collect();
    Ok((m, parts, Walk::seq(walks)))
}

/// The walk of an enum of `variants`: by the tag, each variant's fields at
/// the slots they share.
pub fn switch_walk(pool: &InternPool, env: &dyn ProgramEnv, variants: &[Vec<Ty>]) -> Option<Walk> {
    switch_walk_by(pool, env, variants, &|f| walk(pool, env, f))
}

/// [`switch_walk`], with each field's own walk read by `part`.
fn switch_walk_by(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    variants: &[Vec<Ty>],
    part: &dyn Fn(Ty) -> Option<Walk>,
) -> Option<Walk> {
    let (_, positions) = enum_mem(pool, env, variants).ok()?;
    let arms: Vec<Option<Walk>> = variants
        .iter()
        .zip(&positions)
        .map(|(fields, at)| {
            Walk::seq(
                fields
                    .iter()
                    .zip(at)
                    .filter_map(|(f, ks)| {
                        part(*f)
                            .map(|w| w.remap(&|k| u32::try_from(ks[k as usize]).expect("values")))
                    })
                    .collect(),
            )
        })
        .collect();
    if arms.iter().all(Option::is_none) {
        return None;
    }
    Some(Walk::Switch(0, arms))
}

/// The fields of a struct, a tuple or a data payload in order, each at its
/// alignment: the whole layout and each field's place in it.
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

/// A struct value's or a tuple's layout and its fields' places.
pub fn struct_mem(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<(Mem, Vec<Part>)> {
    let t = strip(pool, env, t);
    let fields = match pool.get(t) {
        TyData::Tuple { .. } => tuple_members(pool, t),
        TyData::Adt { def, args } if env.enum_variants(def, args).is_none() => env
            .data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| subst(pool, env, def, args, f))
            .collect(),
        _ => return unsupported("a field of a value that is not a struct or a tuple"),
    };
    payload_mem(pool, env, &fields)
}

/// The layout of a data object's payload, or of a struct's fields.
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
    Ok(record(&ms))
}

/// The order of the slots after an enum's tag: 4-byte values first, so
/// alignment pads at most once.
const SLOT_ORDER: [Val; 4] = [Val::I32, Val::F32, Val::I64, Val::F64];

/// Each variant's payload fields, each field's values as indices among
/// the enum's values.
pub type Positions = Vec<Vec<Vec<usize>>>;

/// An enum's layout from its variants' payload types: the tag, 1, 2 or 4
/// bytes by the variant count, then the shared slots, each 4 or 8 bytes
/// wide; and where each payload field's values lie.
pub fn enum_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    variants: &[Vec<Ty>],
) -> StageResult<(Mem, Positions)> {
    enum_mem_by(variants, &|f| mem(pool, env, f))
}

/// [`enum_mem`], with each payload field's layout read by `part`.
fn enum_mem_by(
    variants: &[Vec<Ty>],
    part: &dyn Fn(Ty) -> StageResult<Mem>,
) -> StageResult<(Mem, Positions)> {
    let mut payloads = Vec::new();
    let mut slots = [0usize; 4];
    for fields in variants {
        let ms = fields
            .iter()
            .map(|f| part(*f))
            .collect::<StageResult<Vec<_>>>()?;
        for (k, v) in SLOT_ORDER.iter().enumerate() {
            let n = ms
                .iter()
                .flat_map(|m| &m.leaves)
                .filter(|l| l.val == *v)
                .count();
            slots[k] = slots[k].max(n);
        }
        payloads.push(ms);
    }
    let tag_bytes = match variants.len() {
        0..=256 => 1,
        257..=65536 => 2,
        _ => 4,
    };
    let mut leaves = vec![Leaf {
        offset: 0,
        val: Val::I32,
        bytes: tag_bytes,
        signed: false,
    }];
    let mut size = u32::from(tag_bytes);
    let mut align = u32::from(tag_bytes);
    let mut start = [0usize; 4];
    for (k, v) in SLOT_ORDER.iter().enumerate() {
        start[k] = leaves.len();
        let bytes: u8 = if matches!(v, Val::I64 | Val::F64) {
            8
        } else {
            4
        };
        for _ in 0..slots[k] {
            let offset = size.next_multiple_of(u32::from(bytes));
            leaves.push(Leaf {
                offset,
                val: *v,
                bytes,
                signed: false,
            });
            size = offset + u32::from(bytes);
            align = align.max(u32::from(bytes));
        }
    }
    let mut positions = Vec::new();
    for ms in payloads {
        let mut used = [0usize; 4];
        let mut fields = Vec::new();
        for m in ms {
            let mut at = Vec::new();
            for l in &m.leaves {
                let k = SLOT_ORDER.iter().position(|v| *v == l.val).unwrap_or(0);
                at.push(start[k] + used[k]);
                used[k] += 1;
            }
            fields.push(at);
        }
        positions.push(fields);
    }
    Ok((
        Mem {
            size: size.next_multiple_of(align),
            align,
            leaves,
        },
        positions,
    ))
}

/// The variants' payload types of an enum or optional type, `None` for
/// any other type.
pub fn variants(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Vec<Vec<Ty>>> {
    let t = strip(pool, env, t);
    match pool.get(t) {
        // `None` is variant 0 and `Some` variant 1, as the tag says.
        TyData::Option(inner) => Some(vec![vec![], vec![inner]]),
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
        TyData::Mut(inner) => num(pool, inner),
        _ => (32, false, false),
    }
}
