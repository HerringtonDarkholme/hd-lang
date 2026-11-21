#![forbid(unsafe_code)]
//! `hd_layout`: target-independent layouts and the type facts read from
//! them, shared by the ownership passes and every backend
//! (`docs/future-work/BACKEND_NEUTRAL_TIR.md`, step 1), as rustc's
//! `rustc_abi` serves `rustc_codegen_llvm` and `rustc_codegen_cranelift`.
//! A backend maps a [`Val`] to its own value type, as rustc's backends map
//! a `Primitive`.
//!
//! Linear layouts (`crates/hd_wasm/DESIGN.md` §23.3): a type's scalar
//! values in locals, parameters and results, and the same values in
//! memory, each at an offset.
//!
//! One walk gives both: a type's [`Mem`] lists its values as leaves, in
//! the order its locals hold them, each with its offset and width in
//! memory. A struct or a tuple is its fields' leaves, in declaration
//! order, each field at the offset rustc's `univariant` gives it. An enum's
//! [`Flat`] selects tagged or niche-filled layout as rustc does. Tagged
//! layouts store a tag before the variant payloads; niche-filled layouts
//! encode discriminants in unused payload values. One present variant
//! needs no discriminant. Its values are one per distinct leaf, so
//! whole-value loads and stores need no branch on the discriminant. A
//! suspension is one address; recursive enum links are explicit references.
//! A string is its address and byte
//! length; a trait value its data address and its vtable's address; a
//! list or a map the address of its buffer (§23.7); a function value its
//! code's table slot and its environment's address (§23.8). A reference
//! of either kind is the address of its place (§23.5,
//! §23.20); a reference to a trait value is that trait value, whose data
//! is already one address, as Rust's `&dyn Tr` is a fat pointer.

pub mod facts;
pub mod layouts;
pub mod shims;
pub mod work;

pub use layouts::{Fields, Layout, LayoutId, Layouts, Repr, Tag, Variants, layout, layouts};

use hd_base::limits::VALUE_SIZE;
use hd_base::{NotImplemented, SliceSort as _, Stage, StageResult};
use hd_mono::layout::StdKind;
pub use hd_mono::layout::strip_newtype;
use hd_mono::{ProgramEnv, subst};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};

/// A scalar value class (rustc's `Primitive`): every reference is an
/// `i32` address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Val {
    I32,
    I64,
    F32,
    F64,
}

impl Val {
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::I32 => 0,
            Self::I64 => 1,
            Self::F32 => 2,
            Self::F64 => 3,
        }
    }
}

/// One scalar value of a type, where it lies in memory: its offset in the
/// value, its width in bytes and whether a narrow load sign-extends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Leaf {
    pub offset: u32,
    pub val: Val,
    pub bytes: u8,
    pub signed: bool,
}

/// Bytes of a value that have invalid values (rustc's `Niche`,
/// `rustc_abi` lib.rs): the unsigned scalar leaf at `offset`, `bytes`
/// wide, whose valid values are `start..=end`, wrapping at its width. Its
/// other values are free for an enclosing enum's other variants
/// (`crates/hd_wasm/DESIGN.md` §23.14). `size` is the scalar's whole
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

    #[must_use]
    pub fn one(val: Val, bytes: u8, signed: bool) -> Mem {
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
    #[must_use]
    pub fn valid(bytes: u8, start: u32, end: u32) -> Mem {
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
    #[must_use]
    pub fn address() -> Mem {
        Mem::one(Val::I32, 4, false)
    }

    /// An address that is never null (decision 2: null is an address's one
    /// niche): a thin reference, a boxed cell, a list's or a map's buffer,
    /// a string's bytes, a vtable.
    #[must_use]
    pub fn nonnull() -> Mem {
        Mem::valid(4, 1, u32::MAX)
    }

    /// A string: its bytes' address, never null, and its length.
    #[must_use]
    pub fn string() -> Mem {
        pair(Mem::nonnull(), Mem::address())
    }

    #[must_use]
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

impl Part {
    /// Its first value's index among the parent's values, as a value
    /// index.
    #[must_use]
    pub fn first_leaf(&self) -> u32 {
        // ruled out: a laid-out value is at most `VALUE_SIZE` bytes
        // (`value_size` refuses more as `type-too-large`) and each value is
        // at least one byte, so a value index is below `VALUE_SIZE`, a u32.
        u32::try_from(self.first).unwrap_or(u32::MAX)
    }
}

/// What a concrete type is to this backend.
pub enum Kind {
    /// Its values are in locals: a scalar, a string, an enum, a struct or
    /// tuple, a trait value.
    Value,
    /// The address of a counted cell containing an enum value.
    Boxed,
    /// A reference to a place of type `T` (`&T` or `&mut T`, or `*T` or
    /// `*mut T` when `whole`): the place's address, or, when
    /// `T` is a trait value (`fat`), that trait value. A `whole` one is
    /// counted, so it refers to a whole object (`ref.box.object`): it is
    /// null or a payload start (§23.30).
    Ref { target: Ty, fat: bool, whole: bool },
}

/// Which kind `t` is.
pub fn kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Kind {
    let t = strip_newtype(pool, env, t);
    let (def, args) = match pool.get(t) {
        TyData::Ref { inner, counted, .. } => {
            return Kind::Ref {
                target: strip_newtype(pool, env, inner),
                fat: fat(pool, env, inner),
                whole: counted,
            };
        }
        TyData::Adt { def, args } => (def, args),
        _ => return Kind::Value,
    };
    if env.std_kind(def) != StdKind::Other {
        return Kind::Value;
    }
    // The instance decides, as its resolved payloads do (`enum_boxing`).
    if env.is_enum(def) {
        return if env.recursive_enum(def, args) {
            Kind::Boxed
        } else {
            Kind::Value
        };
    }
    Kind::Value
}

/// Whether a reference to `target` is fat: a reference to a trait value
/// is the value itself.
fn fat(pool: &InternPool, env: &dyn ProgramEnv, target: Ty) -> bool {
    matches!(
        pool.get(strip_newtype(pool, env, target)),
        TyData::TraitValue { .. }
    )
}

/// Whether reference type `t` is `&mut` or `*mut`.
pub fn ref_mutable(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    matches!(
        pool.get(strip_newtype(pool, env, t)),
        TyData::Ref { mutable: true, .. }
    )
}

/// The kind of `t`, once stripped, when it is a list or a map.
pub fn coll_kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<StdKind> {
    match pool.get(strip_newtype(pool, env, t)) {
        TyData::Adt { def, .. } => {
            Some(env.std_kind(def)).filter(|k| matches!(k, StdKind::List | StdKind::Map))
        }
        _ => None,
    }
}

/// The type of struct, tuple or newtype field `f` of `t`.
pub fn field_ty(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, f: u32) -> Option<Ty> {
    match pool.get(t) {
        TyData::Adt { def, args } => {
            let fs = env.data_fields(def)?;
            Some(hd_mono::subst(pool, env, def, args, *fs.get(f as usize)?))
        }
        TyData::Tuple { .. } => tuple_members(pool, t).get(f as usize).copied(),
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

pub fn union_variants(pool: &InternPool, members: TyList) -> Vec<Vec<Ty>> {
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

/// The most scalar values a struct, tuple or enum passes as values. A larger one
/// passes as one address (rustc's `PassMode::Indirect`, which the Rust ABI
/// picks above two pointers): passing it costs one value however many it
/// holds, so a value forwarded down a chain of calls costs each call the
/// same. 16 keeps ordinary records, a seven-`f64` body included, direct
/// (owner, #2031).
pub const INDIRECT: usize = 16;

/// Whether a value of `t` may live by address (#2031, steps 4 and 6;
/// rustc's `BackendRepr::Memory`): a struct, a tuple or an enum of more
/// than [`INDIRECT`] values, which may hold counted references and `Drop`
/// hooks. Its counts move with its bits; a release walks them from the
/// address (`emit/indirect.rs` `release_plan`), after its hooks ran on a
/// heap copy (`rt::HookPlan`). One probe of its [`Layout`], which lists
/// no leaves of a type past [`INDIRECT`].
pub fn by_address(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<bool> {
    Ok(layout(pool, env, t)?.repr == Repr::Memory)
}

/// Whether stripped `t` holds its parts inline: a tuple, a union, or a
/// struct or enum kept by value.
fn inline(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Tuple { .. } | TyData::Union(_) => true,
        TyData::Adt { def, .. } => {
            env.std_kind(def) == StdKind::Other && matches!(kind(pool, env, t), Kind::Value)
        }
        _ => false,
    }
}

/// The scalar values of a concrete type.
pub fn vals(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Vec<Val>> {
    Ok(mem(pool, env, t)?.vals())
}

/// Each of a concrete type's scalar values' path below the value, for
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
    let t = strip_newtype(pool, env, t);
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
    let mut stack = vec![(strip_newtype(pool, env, t), false)];
    while let Some((u, ready)) = stack.pop() {
        if done.contains_key(&u) {
            continue;
        }
        let parts: Vec<Ty> = value_parts(pool, env, u)
            .into_iter()
            .map(|p| strip_newtype(pool, env, p))
            .filter(|p| !done.contains_key(p))
            .collect();
        if !ready && !parts.is_empty() {
            stack.push((u, true));
            stack.extend(parts.into_iter().map(|p| (p, false)));
            continue;
        }
        crate::work::layout_parts(1);
        let got = one(u, &|p| {
            done.get(&strip_newtype(pool, env, p))
                .cloned()
                .expect("a part computed first")
        });
        done.insert(u, got);
    }
    done.get(&strip_newtype(pool, env, t))
        .cloned()
        .expect("the type computed")
}

/// The layouts and walks one link has computed, by stripped type
/// ([`cached`]); a type's layout is fixed for the whole link.
type Memo<T> = std::cell::RefCell<Option<hd_base::HashMap<Ty, T>>>;
type VariantTypes = std::rc::Rc<Vec<Vec<Ty>>>;

thread_local! {
    static MEMS: Memo<StageResult<Mem>> = const { std::cell::RefCell::new(None) };
    static TRACKS: Memo<bool> = const { std::cell::RefCell::new(None) };
    static COUNTS: Memo<bool> = const { std::cell::RefCell::new(None) };
    static DROPS: Memo<bool> = const { std::cell::RefCell::new(None) };
    static THIN_REFS: Memo<bool> = const { std::cell::RefCell::new(None) };
    static LOCAL_REFS: Memo<bool> = const { std::cell::RefCell::new(None) };
    static FLATS: Memo<StageResult<std::rc::Rc<Flat>>> = const { std::cell::RefCell::new(None) };
    static VARIANTS: Memo<Option<VariantTypes>> = const { std::cell::RefCell::new(None) };
    static MEMBERS: Memo<StageResult<std::rc::Rc<Members>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `link` with the layout cache in place: within it, [`mem`] and
/// [`counted`] compute each type once, so a long chain of types held by
/// value costs its length, not its square, at every use.
pub fn cached<R>(link: impl FnOnce() -> R) -> R {
    struct Reset {
        mems: Option<hd_base::HashMap<Ty, StageResult<Mem>>>,
        layouts: Option<std::rc::Rc<Layouts>>,
        tracks: Option<hd_base::HashMap<Ty, bool>>,
        counts: Option<hd_base::HashMap<Ty, bool>>,
        drops: Option<hd_base::HashMap<Ty, bool>>,
        thin_refs: Option<hd_base::HashMap<Ty, bool>>,
        local_refs: Option<hd_base::HashMap<Ty, bool>>,
        flats: Option<hd_base::HashMap<Ty, StageResult<std::rc::Rc<Flat>>>>,
        variants: Option<hd_base::HashMap<Ty, Option<VariantTypes>>>,
        members: Option<hd_base::HashMap<Ty, StageResult<std::rc::Rc<Members>>>>,
    }
    impl Drop for Reset {
        fn drop(&mut self) {
            MEMS.set(self.mems.take());
            layouts::LAYOUTS.set(self.layouts.take());
            TRACKS.set(self.tracks.take());
            COUNTS.set(self.counts.take());
            DROPS.set(self.drops.take());
            THIN_REFS.set(self.thin_refs.take());
            LOCAL_REFS.set(self.local_refs.take());
            FLATS.set(self.flats.take());
            VARIANTS.set(self.variants.take());
            MEMBERS.set(self.members.take());
        }
    }

    let _reset = Reset {
        mems: MEMS.take(),
        layouts: layouts::LAYOUTS.take(),
        tracks: TRACKS.take(),
        counts: COUNTS.take(),
        drops: DROPS.take(),
        thin_refs: THIN_REFS.take(),
        local_refs: LOCAL_REFS.take(),
        flats: FLATS.take(),
        variants: VARIANTS.take(),
        members: MEMBERS.take(),
    };
    MEMS.set(Some(hd_base::HashMap::default()));
    layouts::LAYOUTS.set(Some(std::rc::Rc::default()));
    TRACKS.set(Some(hd_base::HashMap::default()));
    COUNTS.set(Some(hd_base::HashMap::default()));
    DROPS.set(Some(hd_base::HashMap::default()));
    THIN_REFS.set(Some(hd_base::HashMap::default()));
    LOCAL_REFS.set(Some(hd_base::HashMap::default()));
    FLATS.set(Some(hd_base::HashMap::default()));
    VARIANTS.set(Some(hd_base::HashMap::default()));
    MEMBERS.set(Some(hd_base::HashMap::default()));
    link()
}

/// A layout cache scope's state on its thread: the memo maps.
struct Scope {
    mems: Option<hd_base::HashMap<Ty, StageResult<Mem>>>,
    layouts: Option<std::rc::Rc<Layouts>>,
    tracks: Option<hd_base::HashMap<Ty, bool>>,
    counts: Option<hd_base::HashMap<Ty, bool>>,
    drops: Option<hd_base::HashMap<Ty, bool>>,
    thin_refs: Option<hd_base::HashMap<Ty, bool>>,
    local_refs: Option<hd_base::HashMap<Ty, bool>>,
    flats: Option<hd_base::HashMap<Ty, StageResult<std::rc::Rc<Flat>>>>,
    variants: Option<hd_base::HashMap<Ty, Option<VariantTypes>>>,
    members: Option<hd_base::HashMap<Ty, StageResult<std::rc::Rc<Members>>>>,
}

impl Scope {
    /// The thread's scope, leaving none in place.
    fn take() -> Self {
        Self {
            mems: MEMS.take(),
            layouts: layouts::LAYOUTS.take(),
            tracks: TRACKS.take(),
            counts: COUNTS.take(),
            drops: DROPS.take(),
            thin_refs: THIN_REFS.take(),
            local_refs: LOCAL_REFS.take(),
            flats: FLATS.take(),
            variants: VARIANTS.take(),
            members: MEMBERS.take(),
        }
    }

    /// An empty scope.
    fn fresh() -> Self {
        Self {
            mems: Some(hd_base::HashMap::default()),
            layouts: Some(std::rc::Rc::default()),
            tracks: Some(hd_base::HashMap::default()),
            counts: Some(hd_base::HashMap::default()),
            drops: Some(hd_base::HashMap::default()),
            thin_refs: Some(hd_base::HashMap::default()),
            local_refs: Some(hd_base::HashMap::default()),
            flats: Some(hd_base::HashMap::default()),
            variants: Some(hd_base::HashMap::default()),
            members: Some(hd_base::HashMap::default()),
        }
    }

    /// Puts the scope in place on the thread.
    fn put(self) {
        MEMS.set(self.mems);
        layouts::LAYOUTS.set(self.layouts);
        TRACKS.set(self.tracks);
        COUNTS.set(self.counts);
        DROPS.set(self.drops);
        THIN_REFS.set(self.thin_refs);
        LOCAL_REFS.set(self.local_refs);
        FLATS.set(self.flats);
        VARIANTS.set(self.variants);
        MEMBERS.set(self.members);
    }
}

thread_local! {
    /// The scope [`cached_for_run`] keeps on this thread between the
    /// bodies of one run, with the run's key.
    static RUN_SCOPE: std::cell::RefCell<Option<(u64, Scope)>> = const { std::cell::RefCell::new(None) };
}

/// Runs `body` with the layout cache in place: a link's, when `body` runs
/// inside one, else the cache of run `run` on this thread, which every
/// body of that run on this thread shares (`Collect`). A type's layout is
/// fixed for a run, as for a link: the run's types live in one pool, and
/// what the program says of them does not change during it. The key is
/// unique per run, so a body of another run never reads this one's
/// layouts: a thread keeps one run's cache, which the next run's first
/// body replaces. A panic in `body` drops the cache.
pub fn cached_for_run<R>(run: u64, body: impl FnOnce() -> R) -> R {
    struct Keep {
        run: u64,
        outer: Option<Scope>,
    }
    impl Drop for Keep {
        fn drop(&mut self) {
            let ours = Scope::take();
            if let Some(outer) = self.outer.take() {
                outer.put();
            }
            if !std::thread::panicking() {
                RUN_SCOPE.set(Some((self.run, ours)));
            }
        }
    }
    if MEMS.with(|m| m.borrow().is_some()) {
        return body();
    }
    let ours = match RUN_SCOPE.take() {
        Some((key, scope)) if key == run => scope,
        _ => Scope::fresh(),
    };
    let _keep = Keep {
        run,
        outer: Some(Scope::take()),
    };
    ours.put();
    body()
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

/// A concrete type's layout. A type not laid out yet has its [`Layout`]
/// checked first, so a type past [`VALUE_SIZE`] is `type-too-large`
/// before any of its parts' leaves is copied.
pub fn mem(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Mem> {
    let u = strip_newtype(pool, env, t);
    if let Some(got) = MEMS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&u).cloned())) {
        return got;
    }
    layout(pool, env, u)?;
    memoized(&MEMS, pool, env, u, &|u, part| {
        mem_one(pool, env, u, part, true)
    })
}

/// `t`'s layout from its parts' layouts, which `part` reads; an enum's
/// kept for [`flat`] when `full`, as no [`Layout`] is.
fn mem_one(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    part: &dyn Fn(Ty) -> StageResult<Mem>,
    full: bool,
) -> StageResult<Mem> {
    let t = strip_newtype(pool, env, t);
    let named = || too_large(&hd_mono::shown_type(pool, env, t));
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
        TyData::Tuple { .. } => payload_mem_by(&tuple_members(pool, t), part, &named)?.0,
        TyData::Union(members) => {
            let variants = union_variants(pool, members);
            enum_flat_by(t, &variants, part, &named, full)?
        }
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::Suspend => Mem::address(),
            StdKind::List | StdKind::Map => Mem::nonnull(),
            StdKind::Other => match kind(pool, env, t) {
                Kind::Boxed => Mem::nonnull(),
                Kind::Ref { fat, .. } => ref_mem(fat),
                Kind::Value => match env.enum_variants(def, args) {
                    Some(variants) => enum_flat_by(t, &variants, part, &named, full)?,
                    None => payload_mem_by(&value_parts(pool, env, t), part, &named)?.0,
                },
            },
        },
        // A function value's code is a table slot, which may be 0, and its
        // environment is 0 when it captures nothing: no niche.
        TyData::Fn { .. } => pair(Mem::address(), Mem::address()),
        TyData::TraitValue { .. } => pair(Mem::address(), Mem::nonnull()),
        // A `$.Context` value is the address of a counted context
        // (`rt::ctx_elem`).
        TyData::Context(_) => Mem::address(),
        TyData::Ref { inner, .. } => ref_mem(fat(pool, env, inner)),
        // An instance that collection shares among reference arguments
        // (A1, `crates/hd_mono/DESIGN.md` §13.2) holds one as an address:
        // Collection selects it with [`crate::facts::shared_reference_layout`].
        other => {
            // ruled out: checking finalizes value types before collection;
            // mono::subst instantiates parameters/projections and reveals
            // opaque results from their body facts (Fn fields: #1570).
            // Rows and const arguments are metadata, never value layouts.
            // The only remaining value form is Cx::classify's A1 reference.
            assert!(matches!(other, TyData::Canon(_)) && hd_mono::is_class_ref(pool, t));
            Mem::address()
        }
    })
}

/// A reference's layout: its place's address, nonnull, or, to a trait
/// value (`fat`), that trait value.
fn ref_mem(fat: bool) -> Mem {
    if fat {
        pair(Mem::address(), Mem::nonnull())
    } else {
        Mem::nonnull()
    }
}

/// Enum `t`'s layout from its parts' layouts, which `part` reads, kept
/// for [`flat`] in the link's cache: its values.
fn enum_flat_by(
    t: Ty,
    variants: &[Vec<Ty>],
    part: &dyn Fn(Ty) -> StageResult<Mem>,
    named: &dyn Fn() -> NotImplemented,
    full: bool,
) -> StageResult<Mem> {
    let got = enum_mem_by(variants, part, named).map(std::rc::Rc::new);
    if !full {
        return Ok(got?.mem.clone());
    }
    FLATS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    Ok(got?.mem.clone())
}

/// How one counted address is counted: the leaf that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addr {
    /// Counting finds the block through the page table.
    Ref,
    /// A known payload start.
    Known,
}

/// One reference among a value's values ([`ref_leaves`]): its index,
/// whether counting counts it (a local reference, thin or `&dyn`, counts
/// nothing, `ref.count.local`), and the variant tests that hold it,
/// outermost first, each a discriminant over the whole value's values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLeaf {
    pub value: u32,
    pub counted: bool,
    pub guards: Guards,
}

/// Whether a value of `t` holds one reference, at its first value, under
/// no variant test.
#[must_use]
pub fn one_ref_first(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    matches!(
        ref_leaves(pool, env, t).as_slice(),
        [RefLeaf { value: 0, guards, .. }] if guards.is_empty()
    )
}

/// The variant tests over a value: each discriminant, the variant it must
/// say and how many variants its enum has, outermost first.
pub type Guards = Vec<(Discr, usize, usize)>;

/// The references among a value of `t`'s values, with the variant tests
/// that hold each. O(the type's nodes) per call; for development views and
/// a cold frame's local references, each asked once per type.
#[must_use]
pub fn ref_leaves(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<RefLeaf> {
    let mut out = Vec::new();
    // Each pending type: where its values sit among the root's, and the
    // tests over it.
    let mut work: Vec<(Ty, Vec<u32>, Guards)> = Vec::new();
    let width = vals(pool, env, t).map_or(0, |v| v.len());
    work.push((
        t,
        (0..u32::try_from(width).unwrap_or(0)).collect(),
        Vec::new(),
    ));
    while let Some((t, at, guards)) = work.pop() {
        let t = strip_newtype(pool, env, t);
        let mut leaf = |k: u32, counted: bool| {
            if let Some(&value) = at.get(k as usize) {
                out.push(RefLeaf {
                    value,
                    counted,
                    guards: guards.clone(),
                });
            }
        };
        let fields = |tys: &[Ty], work: &mut Vec<_>| {
            if let Ok((_, parts)) = payload_mem(pool, env, tys) {
                for (&f, p) in tys.iter().zip(&parts).rev() {
                    let ks = at[p.first..p.first + p.count].to_vec();
                    work.push((f, ks, guards.clone()));
                }
            }
        };
        match pool.get(t) {
            TyData::Prim(Prim::String) | TyData::TraitValue { .. } => leaf(0, true),
            TyData::Fn { .. } => leaf(1, true),
            TyData::Context(_) => leaf(0, true),
            TyData::Tuple { .. } => fields(&tuple_members(pool, t), &mut work),
            TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => leaf(0, true),
            TyData::Ref { counted, .. } => leaf(0, counted),
            TyData::Canon(_) if hd_mono::is_class_ref(pool, t) => leaf(0, true),
            TyData::Union(_) | TyData::Adt { .. } => match kind(pool, env, t) {
                Kind::Boxed => leaf(0, true),
                Kind::Ref { whole, .. } => leaf(0, whole),
                Kind::Value => {
                    if let Some(vs) = variants(pool, env, t) {
                        let Ok(Some(flat)) = flat(pool, env, t) else {
                            continue;
                        };
                        for (v, (fs, ps)) in vs.iter().zip(&flat.positions).enumerate().rev() {
                            let mut g = guards.clone();
                            match flat.disc {
                                Disc::One(k) if k as usize != v => continue,
                                Disc::One(_) => {}
                                Disc::Cell(d) => g.push((
                                    Discr {
                                        value: at[d.value as usize],
                                        ..d
                                    },
                                    v,
                                    vs.len(),
                                )),
                            }
                            for (&f, ks) in fs.iter().zip(ps).rev() {
                                let ks = ks.iter().map(|&k| at[k as usize]).collect();
                                work.push((f, ks, g.clone()));
                            }
                        }
                    } else {
                        fields(&value_parts(pool, env, t), &mut work);
                    }
                }
            },
            // A scalar, and what holds no value of its own here: no reference.
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Opaque { .. }
            | TyData::Param(_)
            | TyData::Const(_)
            | TyData::Assoc { .. }
            | TyData::Infer(_)
            | TyData::Canon(_)
            | TyData::Row(_) => {}
        }
    }
    out
}

/// Whether the ownership passes track a value of `t`: it holds a counted
/// or local reference, or a `Drop` hook. Memoized per link.
#[must_use]
pub fn tracked(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    memoized(&TRACKS, pool, env, t, &|u, part| {
        holds_one(pool, env, u, part, false)
    })
}

/// Whether counting a value of `t` does anything: it holds a counted
/// reference (a local one counts nothing, `ref.count.local`) or a `Drop`
/// hook. Memoized per link.
#[must_use]
pub fn counted(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    memoized(&COUNTS, pool, env, t, &|u, part| {
        holds_one(pool, env, u, part, true)
    })
}

/// Whether `t` holds a reference (only a counted one when `counting`) or
/// a `Drop` hook, from its parts' answers, which `part` reads.
fn holds_one(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    part: &dyn Fn(Ty) -> bool,
    counting: bool,
) -> bool {
    let t = strip_newtype(pool, env, t);
    let held = |whole: bool| whole || !counting;
    match pool.get(t) {
        TyData::Prim(Prim::String)
        | TyData::Fn { .. }
        | TyData::TraitValue { .. }
        | TyData::Context(_) => true,
        TyData::Tuple { .. } => tuple_members(pool, t).into_iter().any(part),
        TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => true,
        TyData::Union(_) => {
            variants(pool, env, t).is_some_and(|vs| vs.iter().flatten().any(|&f| part(f)))
        }
        TyData::Ref { counted, .. } => held(counted),
        TyData::Adt { def, args } => match kind(pool, env, t) {
            Kind::Boxed => true,
            Kind::Ref { whole, .. } => held(whole),
            Kind::Value => {
                let any = match env.enum_variants(def, args) {
                    Some(_) => variants(pool, env, t)
                        .is_some_and(|vs| vs.iter().flatten().any(|&f| part(f))),
                    None => value_parts(pool, env, t).into_iter().any(part),
                };
                any || env.has_drop(def)
            }
        },
        TyData::Canon(_) if hd_mono::is_class_ref(pool, t) => true,
        _ => false,
    }
}

/// A cheap ownership query, shared per type within the link. Concrete
/// frame construction later computes the guarded leaf plan once per parameter.
pub fn holds_local_ref(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    memoized(
        &LOCAL_REFS,
        pool,
        env,
        t,
        &|t, part| match kind(pool, env, t) {
            Kind::Ref { whole, .. } => !whole,
            Kind::Value => value_parts(pool, env, t).into_iter().any(part),
            Kind::Boxed => false,
        },
    )
}

/// Whether a value of `t` holds a thin reference by value: a `*T` or a
/// `&T` whose target is no trait value, in itself or in a tuple member,
/// a struct field or a value enum's payload. Variance turns one kind into
/// the other in place, so the answer is the same for both, and for every
/// type a representation-preserving conversion relates to `t`. A fat
/// reference counts either way, and a collection's elements or a boxed
/// enum's payload are not held by value.
pub fn holds_thin_ref(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    memoized(&THIN_REFS, pool, env, t, &|t, part| {
        if let Kind::Ref { fat, .. } = kind(pool, env, t) {
            return !fat;
        }
        value_parts(pool, env, t).into_iter().any(part)
    })
}

/// A closure environment's layout from its captures, each its type's
/// value or, when shared, a cell's address; with the places of the
/// captures.
pub fn env_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    fields: &[(Ty, bool)],
) -> StageResult<(Mem, Vec<Part>)> {
    let laid = |of: &dyn Fn(Ty) -> StageResult<Mem>| {
        let ms = fields
            .iter()
            .map(|(t, shared)| if *shared { Ok(Mem::address()) } else { of(*t) })
            .collect::<StageResult<Vec<_>>>()?;
        record(&ms).map_err(|TooLarge| too_large("a closure's captures"))
    };
    // The sum fits before any capture's leaves are copied, as in [`mem`].
    laid(&|t| layout(pool, env, t).map(|l| l.part()))?;
    laid(&|t| mem(pool, env, t))
}

/// The captures of a closure environment in order, each at its alignment:
/// the whole layout and each capture's place in it. The environment's code
/// reads them at these offsets. Its niche is its fields' largest, the
/// first of equals.
fn record(fields: &[Mem]) -> Result<(Mem, Vec<Part>), TooLarge> {
    let offsets = offsets_in(fields, 0..fields.len(), 0)?;
    Ok(place(fields, &offsets, 0, Bias::Start))
}

/// Two 4-byte scalars one after the other: a string, a function value, a
/// trait value, a reference to one.
fn pair(a: Mem, b: Mem) -> Mem {
    place(&[a, b], &[0, 4], 0, Bias::Start).0
}

/// A layout whose values would take more than [`VALUE_SIZE`] bytes
/// (`types.limit.value-size`): found before any of its leaves is copied,
/// so a type past the limit costs its fields' sizes, never its leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge;

/// `bytes` as a value size, or [`TooLarge`] past [`VALUE_SIZE`].
fn fits(bytes: u64) -> Result<u32, TooLarge> {
    u32::try_from(bytes)
        .ok()
        .filter(|&b| b <= VALUE_SIZE)
        .ok_or(TooLarge)
}

/// The `type-too-large` stop for the values of `what`
/// (`types.limit.value-size.error`); rustc's `LayoutError::SizeOverflow`
/// message.
fn too_large(what: &str) -> NotImplemented {
    NotImplemented::too_large(
        Stage::Emit,
        format!(
            "values of `{what}` are too big for the target: one value may take at most {VALUE_SIZE} bytes"
        ),
    )
}

/// Each field's offset when `fields` are placed in `order` after `prefix`
/// bytes, each at its alignment: the sums in `u64`, each end checked
/// against [`VALUE_SIZE`], and the whole, rounded to the largest
/// alignment, too. Every later `u32` sum of the layout is at most that
/// size, so none wraps in either compiler profile.
///
/// Cost: one pass over the fields; the size is the sum the layout makes
/// anyway.
fn offsets_in(
    fields: &[Mem],
    order: impl IntoIterator<Item = usize>,
    prefix: u32,
) -> Result<Vec<u32>, TooLarge> {
    let mut offsets = vec![0u32; fields.len()];
    let mut end = u64::from(prefix);
    let mut align = prefix.max(1);
    for k in order {
        let f = &fields[k];
        let offset = fits(end.next_multiple_of(u64::from(f.align)))?;
        offsets[k] = offset;
        end = fits(u64::from(offset) + u64::from(f.size))?.into();
        align = align.max(f.align);
    }
    fits(end.next_multiple_of(u64::from(align)))?;
    Ok(offsets)
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
        crate::work::layout_leaves(f.leaves.len());
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
/// 1091; decision 1 of `crates/hd_wasm/DESIGN.md` §23.14): a struct's
/// (`prefix` 0) by alignment group, largest first, then its largest niche
/// first; a payload behind a tag of `prefix` bytes by group, smallest
/// first, the largest niche last, so that small fields pack after the
/// tag. When the niche is then neither at the start nor at the end, the
/// layout biased to put it last is kept where that leaves more room before
/// it than either edge had (layout.rs:288): an enclosing enum's other
/// variants fit there. Both sorts are stable.
///
/// Cost: two sorts of the fields, at most.
fn univariant(fields: &[Mem], prefix: u32) -> Result<(Mem, Vec<Part>), TooLarge> {
    let got = univariant_biased(fields, prefix, Bias::Start)?;
    let Some(n) = got.0.niche else {
        return Ok(got);
    };
    let head = n.offset;
    let tail = got.0.size - head - u32::from(n.size);
    if fields.len() < 2 || head == 0 || tail == 0 {
        return Ok(got);
    }
    let alt = univariant_biased(fields, prefix, Bias::End)?;
    Ok(match alt.0.niche {
        Some(m) if m.offset > head && m.offset > tail => alt,
        Some(_) | None => got,
    })
}

/// [`univariant`] with one bias: rustc's `univariant_biased`.
fn univariant_biased(
    fields: &[Mem],
    prefix: u32,
    bias: Bias,
) -> Result<(Mem, Vec<Part>), TooLarge> {
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
    let offsets = offsets_in(fields, order, prefix)?;
    Ok(place(fields, &offsets, prefix, bias))
}

/// The layout of an object's payload of `fields`, or of a struct's fields.
pub fn payload_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    fields: &[Ty],
) -> StageResult<(Mem, Vec<Part>)> {
    let named = || {
        let shown: Vec<String> = fields
            .iter()
            .map(|&f| hd_mono::shown_type(pool, env, f))
            .collect();
        too_large(&format!("({})", shown.join(", ")))
    };
    // The sum fits before any field's leaves are copied, as in [`mem`].
    payload_mem_by(fields, &|f| layout(pool, env, f).map(|l| l.part()), &named)?;
    payload_mem_by(fields, &|f| mem(pool, env, f), &named)
}

/// A struct's or a tuple's member types, substituted, and where each
/// lies in its values and memory ([`payload_mem`]).
pub struct Members {
    pub tys: Vec<Ty>,
    pub parts: Vec<Part>,
}

/// The members of the stripped struct or tuple type `t` ([`Members`]);
/// none for another type. Within a link ([`cached`]) each type's are
/// laid out once, so n reads of the fields of an n-field struct cost n
/// probes, not n layouts of n fields each (#1926).
pub fn members(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
) -> StageResult<std::rc::Rc<Members>> {
    let t = strip_newtype(pool, env, t);
    crate::work::member_layouts(1);
    if let Some(got) = MEMBERS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&t).cloned())) {
        return got;
    }
    let tys: Vec<Ty> = match pool.get(t) {
        TyData::Tuple { .. } => tuple_members(pool, t),
        TyData::Adt { def, args } => env
            .data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| subst(pool, env, def, args, f))
            .collect(),
        _ => Vec::new(),
    };
    crate::work::member_layouts(tys.len());
    let got =
        payload_mem(pool, env, &tys).map(|(_, parts)| std::rc::Rc::new(Members { tys, parts }));
    MEMBERS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    got
}

/// [`payload_mem`], with each field's layout read by `part`.
/// past [`VALUE_SIZE`], the stop `named` makes.
fn payload_mem_by(
    fields: &[Ty],
    part: &dyn Fn(Ty) -> StageResult<Mem>,
    named: &dyn Fn() -> NotImplemented,
) -> StageResult<(Mem, Vec<Part>)> {
    let ms = fields
        .iter()
        .map(|f| part(*f))
        .collect::<StageResult<Vec<_>>>()?;
    univariant(&ms, 0).map_err(|TooLarge| named())
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

/// An enum's layout (`crates/hd_wasm/DESIGN.md` §23.14): in memory, each
/// variant's payload as one struct at one offset, behind a tag or around
/// a niche ([`Disc`]); in locals, one scalar value per distinct leaf of
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
    let named = || {
        let shown: Vec<String> = variants
            .iter()
            .map(|fields| {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|&f| hd_mono::shown_type(pool, env, f))
                    .collect();
                format!("({})", fields.join(", "))
            })
            .collect();
        too_large(&format!("an enum of {}", shown.join(" | ")))
    };
    // The size fits before any payload's leaves are copied, as in [`mem`].
    enum_mem_by(
        variants,
        &|f| layout(pool, env, f).map(|l| l.part()),
        &named,
    )?;
    enum_mem_by(variants, &|f| mem(pool, env, f), &named)
}

/// The layout of enum or optional type `t`, unboxed, once per type in a
/// link ([`cached`]); `None` for a type that is neither.
pub fn flat(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
) -> StageResult<Option<std::rc::Rc<Flat>>> {
    let t = strip_newtype(pool, env, t);
    if let Some(got) = FLATS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&t)).cloned()) {
        return got.map(Some);
    }
    let Some(vs) = variants(pool, env, t) else {
        return Ok(None);
    };
    let got = enum_mem(pool, env, &vs).map(std::rc::Rc::new);
    FLATS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    got.map(Some)
}

/// [`enum_mem`], with each payload field's layout read by `part`.
/// Past [`VALUE_SIZE`], the stop `named` makes.
fn enum_mem_by(
    variants: &[Vec<Ty>],
    part: &dyn Fn(Ty) -> StageResult<Mem>,
    named: &dyn Fn() -> NotImplemented,
) -> StageResult<Flat> {
    let ms = variants
        .iter()
        .map(|fields| fields.iter().map(|f| part(*f)).collect())
        .collect::<StageResult<Vec<Vec<Mem>>>>()?;
    flat_of(&ms).map_err(|TooLarge| named())
}

/// The layout of an enum whose variants' payload fields are laid out as
/// `variants`: rustc's `layout_of_enum` (layout.rs:573). Of the
/// niche-filling layout ([`niche_filled`]) and the tagged one
/// ([`tagged`]), the smaller; of equal sizes, the one with more free
/// niche values; else the tagged one (layout.rs:1070).
///
/// Cost: [`univariant`] twice per variant, and one pass over the leaves.
pub fn flat_of(variants: &[Vec<Mem>]) -> Result<Flat, TooLarge> {
    // rustc's `layout_of_struct_or_enum` (layout.rs:340): with no variant
    // present the enum is `never`, and with one it is that variant's
    // struct, with no discriminant.
    let mut present = (0..variants.len()).filter(|&i| !absent(&variants[i]));
    match (present.next(), present.next()) {
        (None, _) => return single(variants, None),
        (Some(k), None) => return single(variants, Some(k)),
        (Some(_), Some(_)) => {}
    }
    let tagged = tagged(variants)?;
    let free = |f: &Flat| f.mem.niche.map_or(0, Niche::available);
    Ok(match niche_filled(variants)? {
        Some(n)
            if n.mem.size < tagged.mem.size
                || n.mem.size == tagged.mem.size && free(&n) > free(&tagged) =>
        {
            n
        }
        Some(_) | None => tagged,
    })
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
fn single(variants: &[Vec<Mem>], only: Option<usize>) -> Result<Flat, TooLarge> {
    let payloads = variants
        .iter()
        .map(|ms| univariant(ms, 0))
        .collect::<Result<Vec<_>, _>>()?;
    let (mem, k) = match only {
        Some(k) => (payloads[k].0.clone(), k),
        None => (Mem::NEVER, 0),
    };
    // ruled out: more variants than u32 counts; an enum's variants are
    // `u32` indices in TIR.
    let k = u32::try_from(k).expect("a variant index fits u32");
    Ok(assemble(
        &payloads,
        mem.size,
        mem.align,
        mem.niche,
        Say::One(k),
        variants,
    ))
}

/// The niche-filling layout (rustc's `layout_of_enum` step 1,
/// layout.rs:591): the largest variant (the last of equals) is untagged,
/// its largest niche holding the other variants' values, `reserve`d
/// upward from its valid range's end (decision 3); every other variant
/// fits before the niche or after it within the untagged one's size.
/// `None` when the largest has no niche with room, or a variant does not
/// fit. Its sums stay in `u32`: each payload is at most [`VALUE_SIZE`],
/// so a sum of one and an offset of at most 8 bytes past another never
/// wraps, and [`flat_of`] keeps it only when it is no larger than the
/// tagged layout, which [`tagged`] checked against [`VALUE_SIZE`].
fn niche_filled(variants: &[Vec<Mem>]) -> Result<Option<Flat>, TooLarge> {
    let mut payloads = variants
        .iter()
        .map(|ms| univariant(ms, 0))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(niche_layout(variants, &mut payloads))
}

/// [`niche_filled`] from the payloads laid out as structs.
fn niche_layout(variants: &[Vec<Mem>], payloads: &mut [(Mem, Vec<Part>)]) -> Option<Flat> {
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
        payloads,
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
fn tagged(variants: &[Vec<Mem>]) -> Result<Flat, TooLarge> {
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
        let (m, parts) = univariant(ms, min)?;
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
    let size = fits(u64::from(size).next_multiple_of(u64::from(align)))?;
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
    Ok(assemble(
        &payloads,
        size,
        align,
        Some(niche),
        Say::Cell(tag, Enc::Direct),
        variants,
    ))
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
/// never bound twice (a `UniqList`). A newtype layer counts as well as its
/// base: `type Flag(usize)` with a `Drop` of its own holds one.
pub fn holds_drop(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let held = DROPS.take();
    let linked = held.is_some();
    let mut done = held.unwrap_or_default();
    let got = drops_by_parts(pool, env, t, &mut done);
    if linked {
        DROPS.set(Some(done));
    }
    got
}

/// [`holds_drop`] over `memo`, a map the caller keeps across links and
/// bodies: each type is answered once per map, so asking it of the
/// growing types of many bodies costs each new type once (#1912).
pub fn holds_drop_in(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    memo: &mut hd_base::HashMap<Ty, bool>,
) -> bool {
    drops_by_parts(pool, env, t, memo)
}

/// [`holds_drop`] of `t` and of every type it holds by value
/// ([`value_parts`]), parts first, with a stack of its own, into `done`.
/// Unlike the layout memos, `done` is keyed by the type itself, not the
/// stripped one: a newtype is a node whose one part is its base, so a
/// layer's own `Drop` counts. A part met again while it is open (a cyclic
/// newtype, which has no values) adds nothing to the edge that meets it.
/// Each type is answered once per memo: O(types + parts).
fn drops_by_parts(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    done: &mut hd_base::HashMap<Ty, bool>,
) -> bool {
    let mut open = hd_base::HashSet::default();
    let mut stack = vec![(t, false)];
    while let Some((u, ready)) = stack.pop() {
        if done.contains_key(&u) {
            continue;
        }
        let parts = value_parts(pool, env, u);
        if !ready {
            // A `(u, false)` entry pushed before `u` opened lies below
            // `(u, true)`, so it pops once `u` is done.
            open.insert(u);
            let pending: Vec<Ty> = parts
                .iter()
                .copied()
                .filter(|p| !done.contains_key(p) && !open.contains(p))
                .collect();
            if !pending.is_empty() {
                stack.push((u, true));
                stack.extend(pending.into_iter().map(|p| (p, false)));
                continue;
            }
        }
        crate::work::member_layouts(1);
        let own = matches!(pool.get(u), TyData::Adt { def, .. } if env.has_drop(def));
        let got = own || parts.iter().any(|p| done.get(p).copied().unwrap_or(false));
        open.remove(&u);
        done.insert(u, got);
    }
    done.get(&t).copied().unwrap_or(false)
}

/// Visits the newtype layers of `t` outermost first, `t` itself when it
/// is one, and stops at the base [`strip_newtype`] returns, which it does not
/// visit. Each step is `strip_newtype`'s, so the walk ends where it does,
/// a cyclic newtype's bound included: O(layers) per call.
pub fn newtype_layers(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, visit: &mut dyn FnMut(Ty)) {
    let base = strip_newtype(pool, env, t);
    let mut layer = t;
    // A layer short of `strip_newtype`'s base is a newtype `Adt`, since
    // `strip_newtype` took a step from it, so the guard's last two
    // conditions hold whenever the first does.
    while layer != base
        && let Some((def, args)) = pool.adt(layer)
        && let Some(inner) = env.newtype_base(def)
    {
        crate::work::member_layouts(1);
        visit(layer);
        let substitutions = pool.list_items(args);
        layer = pool.subst(inner, &|p: hd_types::ParamRef| {
            (p.owner == def)
                .then(|| substitutions.get(usize::from(p.index)).copied())
                .flatten()
        });
    }
}

/// The variants' payload types of an enum or optional type, `None` for
/// any other type. Within a link, materialize each stripped type once and
/// share its payload storage; cache non-enums too. Each warm query is O(1).
pub fn variants(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<VariantTypes> {
    let t = strip_newtype(pool, env, t);
    cached_variants(t, || match pool.get(t) {
        TyData::Union(members) => Some(union_variants(pool, members)),
        TyData::Adt { def, args } => env.enum_variants(def, args),
        _ => None,
    })
}

fn cached_variants(t: Ty, build: impl FnOnce() -> Option<Vec<Vec<Ty>>>) -> Option<VariantTypes> {
    if let Some(got) = VARIANTS.with(|m| m.borrow().as_ref().and_then(|m| m.get(&t)).cloned()) {
        return got;
    }
    let got = build().map(|vs| {
        crate::work::member_layouts(vs.len());
        std::rc::Rc::new(vs)
    });
    VARIANTS.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.insert(t, got.clone());
        }
    });
    got
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
            && TRACKS.with(|w| w.borrow().is_none())
            && COUNTS.with(|w| w.borrow().is_none())
            && DROPS.with(|d| d.borrow().is_none())
            && THIN_REFS.with(|r| r.borrow().is_none())
            && LOCAL_REFS.with(|r| r.borrow().is_none())
            && FLATS.with(|d| d.borrow().is_none())
            && VARIANTS.with(|d| d.borrow().is_none())
    }

    #[test]
    fn variant_queries_share_payloads_and_cache_negative_results() {
        cached(|| {
            let builds = std::cell::Cell::new(0);
            let query = |ty, value| {
                cached_variants(ty, || {
                    builds.set(builds.get() + 1);
                    value
                })
            };
            let first = query(Ty::STRING, Some(vec![vec![Ty::VOID]]));
            let second = query(Ty::STRING, Some(vec![vec![Ty::STRING]]));
            assert!(matches!((&first, &second), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b)));
            assert!(query(Ty::VOID, None).is_none());
            assert!(query(Ty::VOID, Some(vec![])).is_none());
            assert_eq!(builds.get(), 2);
            assert_eq!(first.as_deref(), Some(&vec![vec![Ty::VOID]]));
        });
    }

    // #2030: outside a link, the bodies of one run on a thread share one
    // cache; another run's key starts a fresh one, and a panic drops it.
    #[test]
    fn a_run_cache_is_shared_by_its_bodies_and_by_no_other_run() {
        let builds = std::cell::Cell::new(0);
        let query = |run| {
            cached_for_run(run, || {
                cached_variants(Ty::STRING, || {
                    builds.set(builds.get() + 1);
                    Some(vec![vec![Ty::VOID]])
                })
            })
        };
        let first = query(1);
        let second = query(1);
        assert!(matches!((&first, &second), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b)));
        assert_eq!(builds.get(), 1, "one build for the run's two bodies");
        assert!(
            MEMS.with(|m| m.borrow().is_none()),
            "no cache in place between bodies"
        );
        query(2);
        assert_eq!(builds.get(), 2, "another run builds its own");
        query(1);
        assert_eq!(
            builds.get(),
            3,
            "the thread kept run 2's cache, not run 1's"
        );
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            cached_for_run(1, || panic!("caught compiler panic"));
        }));
        assert!(panicked.is_err());
        assert!(
            MEMS.with(|m| m.borrow().is_none()),
            "the panic leaves no cache in place"
        );
        query(1);
        assert_eq!(builds.get(), 4, "a panic drops the run's cache");
    }

    #[test]
    fn nested_variant_cache_restores_outer_state_on_return_and_unwind() {
        cached(|| {
            let outer = cached_variants(Ty::STRING, || Some(vec![vec![Ty::VOID]]));
            for unwind in [false, true] {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    cached(|| {
                        let inner = cached_variants(Ty::STRING, || Some(vec![vec![Ty::STRING]]));
                        assert_eq!(inner.as_deref(), Some(&vec![vec![Ty::STRING]]));
                        let shared =
                            cached_for_run(u64::MAX, || cached_variants(Ty::STRING, || None));
                        assert!(
                            matches!((&inner, &shared), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b))
                        );
                        if unwind {
                            std::panic::resume_unwind(Box::new("nested variant cache test"));
                        }
                    });
                }));
                assert_eq!(result.is_err(), unwind);
                let restored = cached_variants(Ty::STRING, || None);
                assert!(
                    matches!((&outer, &restored), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b))
                );
            }
        });
        let later = cached(|| cached_variants(Ty::STRING, || Some(vec![vec![Ty::STRING]])));
        assert_eq!(later.as_deref(), Some(&vec![vec![Ty::STRING]]));
        assert!(caches_are_absent());
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
            TRACKS.with(|w| {
                w.borrow_mut()
                    .as_mut()
                    .expect("outer tracked cache")
                    .insert(Ty::VOID, true);
            });

            let result = catch_unwind(AssertUnwindSafe(|| {
                cached(|| {
                    MEMS.with(|m| {
                        assert!(m.borrow().as_ref().is_some_and(hd_base::HashMap::is_empty));
                    });
                    TRACKS.with(|w| {
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
            TRACKS.with(|w| {
                let w = w.borrow();
                assert_eq!(w.as_ref().and_then(|w| w.get(&Ty::VOID)), Some(&true));
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

        let after = cached(|| tuple_mem(&later, later_ty)).expect("later layout");
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
        ])
        .expect("fits");
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
        // `Rect`'s first `f64` overlaps `Tri`'s `i32`s at 8 and 12.
        assert_eq!(
            overlapping(&shape.mem.leaves, &shape.leaves[1][0]),
            [2, 3, 4]
        );
    }

    #[test]
    fn a_narrow_field_is_its_own_value() {
        // `Pair: L(bool, i32); M(i32)`: the tag stays a byte, `L`'s bool
        // packs after it in memory, and each is its own value.
        let bool_m = Mem::one(Val::I32, 1, false);
        let pair = flat_of(&[vec![bool_m.clone(), i32m()], vec![i32m()]]).expect("fits");
        assert_eq!((pair.mem.size, pair.mem.align), (8, 4));
        assert_eq!(
            values(&pair),
            [(0, Val::I32, 1), (1, Val::I32, 1), (4, Val::I32, 4)]
        );
        assert_eq!(pair.cell().map(|d| d.value), Some(0));
        assert_eq!(pair.positions, [vec![vec![1], vec![2]], vec![vec![2]]]);
        // `bool?` is two bytes and two values.
        let opt = flat_of(&[vec![], vec![bool_m]]).expect("fits");
        assert_eq!(values(&opt), [(0, Val::I32, 1), (1, Val::I32, 1)]);
    }

    #[test]
    fn a_nested_optional_fills_the_inner_tags_niche() {
        // `i32??` (the doc's walkthrough): `i32?`'s 4-byte tag holds 0 or 1,
        // so the outer `None` is 2 in it, with no tag of its own.
        let inner = flat_of(&[vec![], vec![i32m()]]).expect("fits");
        assert_eq!(inner.cell().map(|d| d.enc), Some(Enc::Direct));
        let outer = flat_of(&[vec![], vec![inner.mem.clone()]]).expect("fits");
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
        let third = flat_of(&[vec![], vec![outer.mem.clone()]]).expect("fits");
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
        let opt = flat_of(&[vec![], vec![i32m()]]).expect("fits");
        let a = flat_of(&[vec![opt.mem.clone()], vec![i32m()]]).expect("fits");
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
        let char_opt = flat_of(&[vec![], vec![Mem::valid(4, 0, 0x10_FFFF)]]).expect("fits");
        assert_eq!(char_opt.mem.size, 4);
        let raw = |f: &Flat, v| f.cell().and_then(|d| d.enc.raw(v));
        assert_eq!(raw(&char_opt, 0), Some(0x11_0000));
        let mut b = Mem::valid(1, 0, 1);
        for none in 2..=4 {
            let f = flat_of(&[vec![], vec![b.clone()]]).expect("fits");
            assert_eq!((f.mem.size, raw(&f, 0)), (1, Some(none)));
            b = f.mem.clone();
        }
        let ptr = flat_of(&[vec![], vec![Mem::nonnull()]]).expect("fits");
        assert_eq!(
            (ptr.mem.size, raw(&ptr, 0), ptr.mem.niche),
            (4, Some(0), None)
        );
    }

    #[test]
    fn an_absent_variant_takes_no_discriminant() {
        // `WithNever: A(i32); B(never)`: `B` is absent, so the enum is
        // `A`'s struct, with no tag (rustc's `Variants::Single`).
        let one = flat_of(&[vec![i32m()], vec![Mem::NEVER]]).expect("fits");
        assert_eq!((one.mem.size, one.disc), (4, Disc::One(0)));
        assert_eq!(one.test(0), Test::Const(true));
        assert_eq!(one.test(1), Test::Const(false));
        // `WithNever2: A(i32); B(i64, never); C`: `B` is uninhabited but
        // not absent, so it keeps its space behind the tag.
        let i64m = Mem::one(Val::I64, 8, false);
        let two = flat_of(&[vec![i32m()], vec![i64m, Mem::NEVER], vec![]]).expect("fits");
        assert_eq!(
            (two.mem.size, two.cell().map(|d| d.enc)),
            (16, Some(Enc::Direct))
        );
        // No variant present: `never`.
        let none = flat_of(&[vec![Mem::NEVER]]).expect("fits");
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
        )
        .expect("fits");
        assert_eq!((m.size, m.align), (16, 8));
        assert_eq!(
            parts.iter().map(|p| p.offset).collect::<Vec<_>>(),
            [8, 0, 12]
        );
        assert_eq!(m.vals(), [Val::I32, Val::F64, Val::I32]);
    }

    /// A field of `size` bytes at alignment `align` with no leaves: the
    /// limit is about sizes, which no leaf changes.
    fn sized(size: u32, align: u32) -> Mem {
        Mem {
            size,
            align,
            leaves: Vec::new(),
            niche: None,
            uninhabited: false,
        }
    }

    #[test]
    fn a_value_is_at_most_value_size_bytes() {
        // Exactly 2^31 - 1 bytes fits.
        let (m, _) = record(&[sized(VALUE_SIZE - 1, 1), sized(1, 1)]).expect("at the limit");
        assert_eq!(m.size, VALUE_SIZE);
        // One byte more ends past it.
        assert_eq!(
            record(&[sized(VALUE_SIZE, 1), sized(1, 1)]).err(),
            Some(TooLarge)
        );
        // An aligned field after 2^31 - 3 bytes would start at 2^31.
        assert_eq!(
            record(&[sized(VALUE_SIZE - 2, 1), sized(0, 4)]).err(),
            Some(TooLarge)
        );
        // 2^31 - 1 bytes rounded up to a 4-byte alignment is 2^31.
        assert_eq!(
            record(&[sized(4, 4), sized(VALUE_SIZE - 4, 1)]).err(),
            Some(TooLarge)
        );
        // No sum wraps: two fields of 2^31 - 1 bytes each are past it,
        // not 2^32 - 2 bytes wrapped to fit.
        assert_eq!(
            univariant(&[sized(VALUE_SIZE, 1), sized(VALUE_SIZE, 1)], 0).err(),
            Some(TooLarge)
        );
    }

    #[test]
    fn an_enum_past_the_limit_after_its_padding_is_too_large() {
        // A tag and 2^31 - 2 bytes fit; the other variant's 8-byte
        // alignment rounds the enum up to 2^31 bytes.
        let big = vec![sized(VALUE_SIZE - 1, 1)];
        let aligned = vec![sized(8, 8)];
        assert_eq!(flat_of(&[big.clone(), aligned]).err(), Some(TooLarge));
        // Without it, 2^31 - 1 bytes in all.
        let fits = flat_of(&[big, vec![sized(1, 1)]]).expect("at the limit");
        assert_eq!(fits.mem.size, VALUE_SIZE);
        // A payload past the limit before any tag.
        assert_eq!(
            flat_of(&[vec![sized(VALUE_SIZE, 1), sized(1, 1)], vec![]]).err(),
            Some(TooLarge)
        );
    }
}
