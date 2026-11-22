//! The `InternPool` (`docs/compiler/data-structures.md` §3.4, §3.9.2): every type, row and
//! type list is one hash-consed item, so global type equality is one
//! integer compare. Storage is a two-byte shape tag, a `u32` data word, a `u32`
//! meta word (flags and node count) and variable parts in `extra`.
//! Types that hold inference variables live in the checked body's
//! `LocalPool` instead; [`Types`] reads and builds through both.

use hd_base::SliceSort as _;
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use hd_base::{AppendVec, DefId, HashMap, HashSet, InferVar, PathId};

/// A type: an `InternPool` index. Bit 31 set: a body-local index (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ty(pub u32);

/// An anonymous sum has no declaration; its ordered argument list is its
/// shape, including its arity (`types.union.identity`, `types.union.nesting`).
/// Its typed identity carries no declaration ID or allocated declaration.
pub const ANON_SUM: AdtId = AdtId::Sum;

/// A declared type, or an anonymous shape with no declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdtId {
    Declared(DefId),
    Sum,
    Product { rest: bool },
}

impl AdtId {
    /// Anonymous shapes have no declaration to query.
    #[must_use]
    pub const fn get(self) -> Option<DefId> {
        match self {
            Self::Declared(def) => def.get(),
            Self::Sum | Self::Product { .. } => None,
        }
    }
}

/// An interned list of types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TyList(pub u32);

/// An interned row (§3.4 "Rows").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Prim {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
    Bool,
    Char,
    String,
}

impl Prim {
    pub const ALL: [Prim; 14] = [
        Prim::I8,
        Prim::I16,
        Prim::I32,
        Prim::I64,
        Prim::U8,
        Prim::U16,
        Prim::U32,
        Prim::U64,
        Prim::Usize,
        Prim::F32,
        Prim::F64,
        Prim::Bool,
        Prim::Char,
        Prim::String,
    ];
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Prim::I8 => "i8",
            Prim::I16 => "i16",
            Prim::I32 => "i32",
            Prim::I64 => "i64",
            Prim::U8 => "u8",
            Prim::U16 => "u16",
            Prim::U32 => "u32",
            Prim::U64 => "u64",
            Prim::Usize => "usize",
            Prim::F32 => "f32",
            Prim::F64 => "f64",
            Prim::Bool => "bool",
            Prim::Char => "char",
            Prim::String => "string",
        }
    }
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(
            self,
            Prim::I8
                | Prim::I16
                | Prim::I32
                | Prim::I64
                | Prim::U8
                | Prim::U16
                | Prim::U32
                | Prim::U64
                | Prim::Usize
        )
    }
    #[must_use]
    pub const fn is_unsigned(self) -> bool {
        matches!(
            self,
            Prim::U8 | Prim::U16 | Prim::U32 | Prim::U64 | Prim::Usize
        )
    }
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Prim::F32 | Prim::F64)
    }
}

/// A declared type parameter: (owner item, index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParamRef {
    pub owner: DefId,
    pub index: u16,
}

/// An evaluated const generic argument (`types.generic.const.identity`):
/// its value together with its declared scalar type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstValue {
    /// An integer of `prim`, as its bits sign-extended (signed types) or
    /// zero-extended (unsigned types) to 64.
    Int {
        prim: Prim,
        bits: u64,
    },
    Bool(bool),
    Char(char),
    /// A string, as its root `PathKind::Text` path, which is stable across
    /// runs.
    Str(PathId),
}

impl ConstValue {
    /// The declared scalar type.
    #[must_use]
    pub fn prim(self) -> Prim {
        match self {
            ConstValue::Int { prim, .. } => prim,
            ConstValue::Bool(_) => Prim::Bool,
            ConstValue::Char(_) => Prim::Char,
            ConstValue::Str(_) => Prim::String,
        }
    }

    /// The value as one 64-bit word; its meaning follows [`Self::prim`].
    #[must_use]
    pub fn bits(self) -> u64 {
        match self {
            ConstValue::Int { bits, .. } => bits,
            ConstValue::Bool(b) => u64::from(b),
            ConstValue::Char(c) => u64::from(u32::from(c)),
            ConstValue::Str(s) => u64::from(s.raw()),
        }
    }

    /// The canonical literal spelling (`types.generic.const.display`): a
    /// decimal integer, `true` or `false`, or an escaped character or
    /// string literal. `text` reads a string's text from its path.
    #[must_use]
    pub fn literal(self, text: impl FnOnce(PathId) -> String) -> String {
        let escape = |c: char, quote: char| match c {
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            '\0' => "\\0".to_owned(),
            '$' if quote == '"' => "\\$".to_owned(),
            c if c == quote => format!("\\{c}"),
            c if c.is_control() => format!("\\u{{{:x}}}", u32::from(c)),
            c => c.to_string(),
        };
        match self {
            ConstValue::Int { prim, bits } if prim.is_unsigned() => bits.to_string(),
            ConstValue::Int { bits, .. } => i64::from_ne_bytes(bits.to_ne_bytes()).to_string(),
            ConstValue::Bool(b) => b.to_string(),
            ConstValue::Char(c) => format!("'{}'", escape(c, '\'')),
            ConstValue::Str(p) => {
                let body: String = text(p).chars().map(|c| escape(c, '"')).collect();
                format!("\"{body}\"")
            }
        }
    }

    /// The inverse of [`Self::prim`] and [`Self::bits`].
    #[must_use]
    pub fn from_bits(prim: Prim, bits: u64) -> Self {
        match prim {
            Prim::Bool => ConstValue::Bool(bits != 0),
            Prim::Char => ConstValue::Char(
                u32::try_from(bits)
                    .ok()
                    .and_then(char::from_u32)
                    .expect("const char"),
            ),
            Prim::String => ConstValue::Str(PathId::from_raw(crate::low32(bits))),
            prim => ConstValue::Int { prim, bits },
        }
    }
}

/// A declared row parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowParamRef {
    pub owner: DefId,
    pub index: u16,
}

/// A row: sorted keys and declared row parameters. Pending private rows
/// live only in the body and module tiers, never here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RowData {
    pub keys: Vec<Ty>,
    pub params: Vec<RowParamRef>,
}

/// A function type's dependency map (`fn.type.lifetime.identity`): which
/// input reference slots each result reference slot depends on, as the
/// lifetime of each slot of its inputs and then of its result
/// ([`Types::ref_slots`] order), numbered by first occurrence
/// ([`Types::fn_deps`]). `NONE` when the result has no reference slot,
/// but for an open map: a result with no reference slot that holds a type
/// parameter, where some input writes a lifetime, keeps which inputs write
/// one (`fn.type.lifetime.result.param`), and a result slot an instance
/// adds may depend on those ([`Types::fn_deps_written`]). It is part of the
/// type's identity and erased by [`Types::erase_fn_deps`], so it never
/// reaches a monomorphization key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FnDeps(u32);

impl FnDeps {
    pub const NONE: FnDeps = FnDeps(u32::MAX);
}

/// Where a reference slot of a function type's instance comes from
/// ([`Types::instance_deps`]): a slot of the type it instantiates, or a
/// position of what replaced one of its holes.
#[derive(Clone, Copy)]
enum Origin {
    Slot(usize),
    Hole(Ty, u32),
}

/// How a function type's `params` spell its inputs tuple
/// (`fn.type.ctor.inputs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Inputs {
    /// One parameter per element.
    Fixed,
    /// The inputs end in a rest element `List[T]...`: the last of
    /// `params` is that `List[T]` and a call collects its arguments
    /// (`fn.type.vararg-rest`).
    Rest,
    /// `params` holds one type that is the whole inputs tuple, of an arity
    /// not known here: a `Tuple`-bounded parameter, an inference variable,
    /// a placeholder or a projection, as in `Fn[Args, O, $ R]`
    /// (`fn.type.ctor.inputs`). Interning a tuple there gives the
    /// `Fixed` or `Rest` form, so `Fn[(A, B), O, $()]` is `fn(A, B) -> O`.
    Tuple,
}

impl Inputs {
    fn bits(self) -> u32 {
        match self {
            Inputs::Fixed => 0,
            Inputs::Rest => 2,
            Inputs::Tuple => 4,
        }
    }

    /// Decodes the flag word of a function type (`bits`).
    #[must_use]
    pub fn from_bits(w: u32) -> Inputs {
        if w & 4 != 0 {
            Inputs::Tuple
        } else if w & 2 != 0 {
            Inputs::Rest
        } else {
            Inputs::Fixed
        }
    }

    /// The flag word of a function type: bit 0 suspends, then the inputs.
    #[must_use]
    pub fn word(self, suspends: bool) -> u32 {
        u32::from(suspends) | self.bits()
    }
}

/// The decoded view of one type (`docs/compiler/data-structures.md` §3.4), owned for simplicity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyData {
    /// A primitive: an integer, a float, `bool`, `char` or `string`
    /// (`types.prim.set`).
    Prim(Prim),
    /// `never`, the type of an expression that does not finish
    /// (`types.never`).
    Never,
    /// Internal, no spec form: the type of an expression whose error was
    /// already reported. It unifies with anything, so one error isn't
    /// reported twice.
    Poison,
    /// A declared type or anonymous product/sum, with members in written order.
    /// A product's rest flag and final List member retain rest identity
    /// (`types.tuple.rest.same`); sums retain arity, duplicates and nesting.
    Adt(AdtId, TyList),
    /// A function type `fn(A) -> R $ Row` (`types.fn.constructor`);
    /// `suspends` marks a bang function, `SuspendFn`.
    Fn {
        params: TyList,
        result: Ty,
        row: RowId,
        suspends: bool,
        /// How `params` spell the inputs tuple (`fn.type.ctor.inputs`,
        /// `fn.type.vararg-rest`).
        inputs: Inputs,
        /// Which inputs each result borrows from
        /// (`fn.type.lifetime.identity`).
        deps: FnDeps,
    },
    /// A trait value `dyn Trait[args]`, with its associated types bound: the
    /// referent descriptor of a trait reference
    /// (`trait.dyn.referent-descriptor`, `trait.identity.trait-value`), and
    /// also every bound, supertrait and row key (`types.trait.static`).
    TraitValue {
        def: DefId,
        args: TyList,
        bindings: Vec<(DefId, Ty)>,
    },
    /// A generative return type (`types.opaque.occurrence`). Only `bound` is
    /// visible to checking; the defining body's result supplies its
    /// representation in mono (`types.opaque.representation`).
    Opaque {
        owner: DefId,
        /// Return-annotation occurrence in source order
        /// (`types.opaque.identity-position`); local functions start at
        /// their syntax node + 1.
        site: u32,
        /// Whether the declaring function has a local reference parameter,
        /// so that the hidden value may borrow from it
        /// (`types.opaque.borrows`): the type then holds a local reference
        /// in one reference slot of its own.
        borrows: bool,
        args: TyList,
        bound: Ty,
    },
    /// A type or const parameter (`types.generic.type-parameter.complete`).
    /// A const parameter only occurs alone in a const argument slot
    /// (`types.generic.const.type-position`).
    Param(ParamRef),
    /// An evaluated const argument in a const parameter's argument slot
    /// (`types.generic.const.identity`). It is not a type: it has no values
    /// and no layout.
    Const(ConstValue),
    /// An unnormalized projection: the associated item and its trait
    /// reference (`trait.assoc.projection`, `trait.assoc.self`).
    Assoc {
        assoc: DefId,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
    },
    /// A reference type: a local `&T` or `&mut T` (`ref.type.forms`), or a
    /// counted `*T` or `*mut T` (`ref.type.counted-forms`). One variant holds
    /// all four, as `ref.type.kinds` groups them; each kind is its own pool
    /// tag, so the kinds stay distinct types (`ref.type.distinct`,
    /// `ref.type.kind-distinct`), through every stage to the backend. A
    /// counted reference refers to a whole object (`ref.box.object`), so its
    /// address is a payload start (`crates/hd_wasm/DESIGN.md` §23.30).
    Ref {
        inner: Ty,
        mutable: bool,
        counted: bool,
    },
    /// Internal, no spec form: an inference variable, a type the checker
    /// has not solved yet; the solver substitutes its answer.
    Infer(InferVar),
    /// Internal, no spec form: a canonical placeholder of the solver
    /// (`crates/hd_check/DESIGN.md` (trait solver) §2.2).
    Canon(u8),
    /// A requirement row in a row parameter's argument slot
    /// (`req.row.parameter`, `req.row.definition`): an explicit
    /// `f::[$ Db]`, or the row a call solved for `$R`.
    Row(RowId),
    /// A reusable provider context, `$.Context[$ Row]` (`req.context.row`):
    /// indexed by one concrete, expanded row.
    Context(RowId),
}

impl TyData {
    /// An ordered anonymous sum, represented by the same ADT as an enum.
    #[must_use]
    pub fn sum(args: TyList) -> Self {
        Self::Adt(ANON_SUM, args)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
enum AdtTag {
    Declared,
    Sum,
    Product,
    RestProduct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
enum PoolTag {
    Prim,
    Never,
    Poison,
    Adt(AdtTag),
    Fn,
    TraitValue,
    Opaque,
    Param,
    Assoc,
    Ref,
    RefMut,
    CRef,
    CRefMut,
    Infer,
    Canon,
    List,
    Row,
    RowTy,
    Context,
    Const,
    /// A function type's dependency map: `[inputs, results, lifetimes...]`.
    Deps,
}

/// Meta flags (§3.9.2).
pub mod meta {
    pub const HAS_INFER: u32 = 1;
    pub const HAS_POISON: u32 = 2;
    pub const HAS_PARAM: u32 = 4;
    pub const HAS_ASSOC: u32 = 8;
    pub const HAS_CANON: u32 = 16;
    pub const HAS_REF: u32 = 32;
    pub const HAS_OPAQUE: u32 = 64;
    pub const HAS_FN: u32 = 128;
    /// The flag bits; the word's upper half is the depth (`depth`).
    pub const FLAGS: u32 = 0xFFFF;
    const DEPTH_SHIFT: u32 = 16;

    /// The depth a meta word records: type constructors on the longest
    /// path down to a leaf, a list or a row adding none. Saturates at
    /// `u16::MAX`, past every limit (`hd_base::limits::TYPE_DEPTH`).
    #[must_use]
    pub const fn depth(m: u32) -> u32 {
        m >> DEPTH_SHIFT
    }

    /// The flags of both words, and the greater depth: the meta of a
    /// list or row of parts with these metas.
    #[must_use]
    pub const fn join(a: u32, b: u32) -> u32 {
        let d = if depth(a) > depth(b) { a } else { b };
        (a | b) & FLAGS | d & !FLAGS
    }

    /// The meta of a type constructor over parts whose joined meta is
    /// `m`: one level deeper.
    #[must_use]
    pub const fn level(m: u32) -> u32 {
        let d = depth(m);
        let d = if d < FLAGS { d + 1 } else { d };
        m & FLAGS | d << DEPTH_SHIFT
    }
}

/// An item's reference slot counts, kept with it in the pool: the low
/// word is [`Types::ref_slots`], the high word
/// [`Types::fn_written_slots`]; a list's are its items' sums. Each count
/// saturates at `u32::MAX`, past every limit (`types.limit.references`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Slots(u64);

impl Slots {
    const NONE: Slots = Slots(0);

    fn new(refs: u32, written: u32) -> Self {
        Slots(u64::from(written) << 32 | u64::from(refs))
    }
    #[allow(clippy::cast_possible_truncation)]
    const fn refs(self) -> u32 {
        self.0 as u32
    }
    const fn written(self) -> u32 {
        (self.0 >> 32) as u32
    }
    fn plus(self, o: Slots) -> Slots {
        Slots::new(
            self.refs().saturating_add(o.refs()),
            self.written().saturating_add(o.written()),
        )
    }
}

impl PoolTag {
    /// Content-hash tag: its constructor in the low byte, ADT shape above it.
    /// Hash discrimination only; no index space or program limit is consumed.
    const fn word(self) -> u32 {
        match self {
            Self::Prim => 0,
            Self::Never => 1,
            Self::Poison => 2,
            Self::Fn => 6,
            Self::TraitValue => 7,
            Self::Opaque => 8,
            Self::Param => 9,
            Self::Assoc => 10,
            Self::Ref => 11,
            Self::RefMut => 12,
            Self::CRef => 13,
            Self::CRefMut => 14,
            Self::Infer => 15,
            Self::Canon => 16,
            Self::List => 17,
            Self::Row => 18,
            Self::RowTy => 19,
            Self::Context => 20,
            Self::Const => 21,
            Self::Deps => 22,
            Self::Adt(kind) => 3 | (kind as u32) << 8,
        }
    }

    /// Whether `data` is an offset into `extra` (§3.9.2's table); the other
    /// tags keep their whole payload inline in `data`.
    const fn has_record(self) -> bool {
        matches!(
            self,
            PoolTag::Adt(AdtTag::Declared)
                | PoolTag::Fn
                | PoolTag::TraitValue
                | PoolTag::Opaque
                | PoolTag::Param
                | PoolTag::Assoc
                | PoolTag::List
                | PoolTag::Row
                | PoolTag::Const
                | PoolTag::Deps
        )
    }
}

/// The pool's columns (§3.9.2): one fixed-size row per item, and the
/// variable parts of every item in one flat `extra` column. An item with
/// a record keeps the record's first `extra` word in `data`; the record's
/// length follows from its tag and, for rows and trait values, from count
/// words inside it. A list's items live in the `tys` column and its
/// record is `[start, len]` there, so a list is lent as one `&[Ty]`.
#[derive(Default)]
struct Cols {
    tag: AppendVec<PoolTag>,
    data: AppendVec<u32>,
    meta: AppendVec<u32>,
    /// Each item's reference slot counts ([`Slots`]), kept with the item as
    /// its meta word is, so a count is one read.
    slots: AppendVec<u64>,
    extra: AppendVec<u32>,
    tys: AppendVec<Ty>,
}

impl Cols {
    /// Appends one item. The caller serializes appends, so the columns
    /// stay row-aligned.
    fn push(&self, c: &Content<'_>, meta: u32, slots: Slots) -> u32 {
        let data = if c.tag == PoolTag::List {
            let len = crate::word(c.tys.len());
            let start = self.tys.push_run(len, Ty::NEVER, c.tys.iter().copied());
            self.extra.push_run(2, 0, [start, len])
        } else if c.tag.has_record() {
            let len = crate::word(c.head.len());
            self.extra.push_run(len, 0, c.head.iter().copied())
        } else {
            c.data
        };
        let i = self.tag.push(c.tag);
        self.data.push(data);
        self.meta.push(meta);
        self.slots.push(slots.0);
        i
    }

    /// The `extra` record of item `i` (empty for an inline item).
    fn record(&self, i: u32) -> &[u32] {
        self.record_of(self.tag[i], self.data[i])
    }

    /// The record of an item with this tag and `data` word.
    fn record_of(&self, tag: PoolTag, off: u32) -> &[u32] {
        if !tag.has_record() {
            return &[];
        }
        let x = |k: u32| self.extra[off + k];
        let len = match tag {
            PoolTag::Adt(AdtTag::Declared) | PoolTag::Param | PoolTag::List => 2,
            PoolTag::Assoc => 4,
            PoolTag::Const => 3,
            PoolTag::Opaque => 5,
            PoolTag::Fn => 5,
            PoolTag::Deps => 2 + x(0) + x(1),
            PoolTag::TraitValue => 3 + 2 * x(2),
            PoolTag::Row => {
                let n = x(0);
                2 + n + 2 * x(1 + n)
            }
            _ => unreachable!("inline tag"),
        };
        self.extra.run(off, len)
    }

    /// The items of list `i`, borrowed from the `tys` column.
    fn list(&self, i: u32) -> &[Ty] {
        debug_assert!(self.tag[i] == PoolTag::List, "not a list");
        let off = self.data[i];
        self.tys.run(self.extra[off], self.extra[off + 1])
    }
}

/// An item's content as the interner sees it: the tag, the inline `data`
/// word (inline tags only), the `extra` record (`head`) and, for a list,
/// its items, so content is hashed and compared where it lies.
#[derive(Clone, Copy)]
struct Content<'a> {
    tag: PoolTag,
    data: u32,
    head: &'a [u32],
    tys: &'a [Ty],
}

impl Content<'_> {
    /// A 64-bit content hash (Fx-style word mixing with a final fold). It
    /// picks shards and probe slots only; nothing hashed or printed reads it.
    fn hash(&self) -> u64 {
        const K: u64 = 0x517c_c1b7_2722_0a95;
        let mix = |h: u64, w: u32| (h.rotate_left(5) ^ u64::from(w)).wrapping_mul(K);
        let mut h = mix(0, self.tag.word());
        if self.tag.has_record() {
            h = self.head.iter().copied().fold(h, mix);
            h = self.tys.iter().map(|t| t.0).fold(h, mix);
        } else {
            h = mix(h, self.data);
        }
        h ^= h >> 29;
        h = h.wrapping_mul(K);
        h ^ (h >> 32)
    }

    /// Whether item `i` of `cols` has this content.
    fn matches(&self, cols: &Cols, i: u32) -> bool {
        match cols.tag[i] {
            t if t != self.tag => false,
            PoolTag::List => cols.list(i) == self.tys,
            t if t.has_record() => cols.record(i) == self.head,
            _ => cols.data[i] == self.data,
        }
    }
}
/// A slot word's hash and id.
fn split(w: u64) -> (u32, u32) {
    (low(w >> 32), low(w) - 1)
}

/// The low 32 bits of a word.
fn low(w: u64) -> u32 {
    crate::low32(w)
}

/// A hash-to-index table (§3.9.4: hash maps are only indexes): open
/// addressing over `(hash, id)` words. Content lives in the columns; a
/// probe compares the stored hash, then the item.
#[derive(Default)]
struct IdTable {
    /// `hash << 32 | (id + 1)`; 0 is an empty slot.
    slots: Vec<u64>,
    len: usize,
}

impl IdTable {
    fn find(&self, hash: u32, eq: impl Fn(u32) -> bool) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut s = hash as usize & mask;
        loop {
            let w = self.slots[s];
            if w == 0 {
                return None;
            }
            let (h, id) = split(w);
            if h == hash && eq(id) {
                return Some(id);
            }
            s = (s + 1) & mask;
        }
    }

    /// Inserts an id whose content `find` just missed.
    fn insert(&mut self, hash: u32, id: u32) {
        if (self.len + 1) * 4 > self.slots.len() * 3 {
            let old = std::mem::take(&mut self.slots);
            self.slots = vec![0; (old.len() * 2).max(64)];
            for w in old.into_iter().filter(|w| *w != 0) {
                self.place(w);
            }
        }
        self.place(u64::from(hash) << 32 | (u64::from(id) + 1));
        self.len += 1;
    }

    fn place(&mut self, w: u64) {
        let mask = self.slots.len() - 1;
        let mut s = split(w).0 as usize & mask;
        while self.slots[s] != 0 {
            s = (s + 1) & mask;
        }
        self.slots[s] = w;
    }
}

/// Dedup shards, selected by the content hash's top bits (§3.3).
const SHARDS: usize = 64;
/// Slots of each thread's read-through table (§3.3 item 6).
const READ_THROUGH_SLOTS: usize = 4096;

/// One shard on its own cache lines, so two shards' locks never share one.
#[repr(align(128))]
#[derive(Default)]
struct Shard(Mutex<IdTable>);

/// Numbers pools, so a thread's read-through table knows whose ids it holds.
static NEXT_POOL: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// This thread's read-through table (§3.3 item 6): the pool it serves
    /// and direct-mapped `hash << 32 | (id + 1)` slots. An interned item
    /// never changes, so a slot is never stale; a hit is still checked
    /// against the item's content, because slots are shared by hashes.
    static READ_THROUGH: RefCell<(u64, Vec<u64>)> = const { RefCell::new((0, Vec::new())) };
}

/// The global pool. Columns are append-only. Dedup goes through a
/// thread's read-through table, then one of 64 locked shards; a miss
/// appends under one append lock, which keeps the columns row-aligned.
pub struct InternPool {
    cols: Cols,
    append: Mutex<()>,
    shards: Box<[Shard]>,
    uid: u64,
}

impl Default for InternPool {
    fn default() -> Self {
        let p = Self {
            cols: Cols::default(),
            append: Mutex::new(()),
            shards: (0..SHARDS).map(|_| Shard::default()).collect(),
            uid: NEXT_POOL.fetch_add(1, Ordering::Relaxed),
        };
        // Pre-seeding (§3.3 item 5): primitives first, so `Ty::I32` etc. are constants.
        for prim in Prim::ALL {
            p.intern_ty(&TyData::Prim(prim));
        }
        p.intern_ty(&TyData::Never);
        p.intern_ty(&TyData::Poison);
        p.list(&[]);
        p.row(&RowData::default());
        // `void` is the empty tuple (`types.void`).
        p.product(TyList::EMPTY, None);
        p
    }
}

/// Bit 31 of an index: the item is in a body-local pool (§3.9.2).
const LOCAL: u32 = 1 << 31;
/// Bits 27..30 of a local index: its pool's generation, so an index used
/// against another body's pool fails loudly.
const GEN_SHIFT: u32 = 27;
const GEN_MASK: u32 = 0xF;
/// Bits 0..26 of a local index: the row.
const LOCAL_ROW: u32 = (1 << GEN_SHIFT) - 1;

static NEXT_LOCAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The body-local pool (§3.9.2, §3.4): the types that hold inference
/// variables, in the global pool's columns, owned by one body and dropped
/// with it, so variables never reach the shared pool. Unlike the design
/// it is hash-consed, through a body-local table, because the checker
/// compares variable-holding types with `==`. `resolve` interns the
/// variable-free results in the global pool.
pub struct LocalPool {
    cols: Cols,
    index: RefCell<IdTable>,
    generation: u32,
}

impl Default for LocalPool {
    fn default() -> Self {
        Self {
            cols: Cols::default(),
            index: RefCell::new(IdTable::default()),
            generation: NEXT_LOCAL.fetch_add(1, Ordering::Relaxed) & GEN_MASK,
        }
    }
}

impl LocalPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn intern(&self, c: &Content<'_>, meta: u32, slots: Slots) -> u32 {
        let h32 = low(c.hash());
        let mut idx = self.index.borrow_mut();
        let found = idx.find(h32, |i| c.matches(&self.cols, i));
        let row = found.unwrap_or_else(|| {
            let i = self.cols.push(c, meta, slots);
            idx.insert(h32, i);
            i
        });
        assert!(row <= LOCAL_ROW, "body-local pool over capacity");
        LOCAL | self.generation << GEN_SHIFT | row
    }

    /// Items in this pool.
    #[cfg(test)]
    pub(crate) fn len(&self) -> u32 {
        self.cols.tag.len()
    }
}

/// What [`Types::map_children`] does with each child of a type.
pub trait TyMap {
    fn ty(&mut self, t: Ty) -> Ty;
    fn row(&mut self, r: RowId) -> RowId;
}

/// A view of the types one piece of code can see: the global pool and,
/// while a body is checked, that body's local pool. Every read and build
/// goes through it; an index with bit 31 set reads the local pool, and a
/// built item that holds an inference variable goes there.
#[derive(Clone, Copy)]
pub struct Types<'a> {
    global: &'a InternPool,
    local: Option<&'a LocalPool>,
}

impl<'a> Types<'a> {
    /// The global pool alone: building a variable-holding type panics.
    #[must_use]
    pub fn of(global: &'a InternPool) -> Self {
        Self {
            global,
            local: None,
        }
    }

    /// The global pool and one body's local pool.
    #[must_use]
    pub fn with_local(global: &'a InternPool, local: &'a LocalPool) -> Self {
        Self {
            global,
            local: Some(local),
        }
    }

    /// The columns and row of an index.
    fn at(self, id: u32) -> (&'a Cols, u32) {
        if id & LOCAL == 0 {
            return (&self.global.cols, id);
        }
        let l = self.local.expect("a body-local type read outside its body");
        assert_eq!(
            (id >> GEN_SHIFT) & GEN_MASK,
            l.generation,
            "a body-local type read in another body"
        );
        (&l.cols, id & LOCAL_ROW)
    }

    fn meta_word(self, id: u32) -> u32 {
        let (c, i) = self.at(id);
        c.meta[i]
    }

    fn slots_word(self, id: u32) -> Slots {
        let (c, i) = self.at(id);
        Slots(c.slots[i])
    }

    /// Interns in the local pool when the item holds a variable, else in
    /// the global pool.
    fn intern(self, tag: PoolTag, data: u32, rec: (&[u32], &[Ty]), meta: u32, slots: Slots) -> u32 {
        if meta & meta::HAS_INFER == 0 {
            return self.global.intern(tag, data, rec, meta, slots);
        }
        let (head, tys) = rec;
        self.local
            .expect("an inference variable built outside a body")
            .intern(
                &Content {
                    tag,
                    data,
                    head,
                    tys,
                },
                meta,
                slots,
            )
    }
}

impl Ty {
    /// Whether this index is in a body-local pool.
    #[must_use]
    pub const fn is_local(self) -> bool {
        self.0 & LOCAL != 0
    }
    #[must_use]
    pub const fn prim(p: Prim) -> Ty {
        Ty(p as u32)
    }
    pub const I32: Ty = Ty::prim(Prim::I32);
    pub const BOOL: Ty = Ty::prim(Prim::Bool);
    pub const STRING: Ty = Ty::prim(Prim::String);
    pub const NEVER: Ty = Ty(14);
    pub const POISON: Ty = Ty(15);
    /// `void`, the empty tuple type `()` (`types.void`).
    pub const VOID: Ty = Ty(18);
}

impl TyList {
    pub const EMPTY: TyList = TyList(16);
}

impl RowId {
    pub const EMPTY: RowId = RowId(17);
}

impl InternPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns `tag` with an inline `data` word or with the record `head`
    /// then `tys`. A hit allocates nothing: the content is hashed and
    /// compared where it lies.
    fn intern(
        &self,
        tag: PoolTag,
        data: u32,
        (head, tys): (&[u32], &[Ty]),
        meta: u32,
        slots: Slots,
    ) -> u32 {
        // A global type never holds a local one (§3.4).
        assert!(
            meta & meta::HAS_INFER == 0,
            "an inference variable reached the global pool"
        );
        let c = Content {
            tag,
            data,
            head,
            tys,
        };
        let h = c.hash();
        let h32 = low(h);
        let slot = h32 as usize % READ_THROUGH_SLOTS;
        let cached = READ_THROUGH.with(|rt| {
            let rt = rt.borrow();
            if rt.0 != self.uid {
                return None;
            }
            let w = rt.1[slot];
            if w == 0 {
                return None;
            }
            let (sh, id) = split(w);
            (sh == h32 && c.matches(&self.cols, id)).then_some(id)
        });
        if let Some(i) = cached {
            return i;
        }
        // The hash's top six bits pick one of the 64 shards.
        let shard = &self.shards[usize::from(h.to_be_bytes()[0] >> 2)].0;
        let mut idx = shard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let found = idx.find(h32, |i| c.matches(&self.cols, i));
        let i = found.unwrap_or_else(|| {
            // A miss: append, publish, then index (§3.3 item 2).
            let i = {
                let _a = self
                    .append
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                self.cols.push(&c, meta, slots)
            };
            idx.insert(h32, i);
            i
        });
        drop(idx);
        READ_THROUGH.with(|rt| {
            let mut rt = rt.borrow_mut();
            if rt.0 != self.uid {
                rt.0 = self.uid;
                rt.1.clear();
                rt.1.resize(READ_THROUGH_SLOTS, 0);
            }
            rt.1[slot] = u64::from(h32) << 32 | (u64::from(i) + 1);
        });
        i
    }

    /// This pool as a [`Types`] view with no body-local pool.
    #[must_use]
    pub fn types(&self) -> Types<'_> {
        Types::of(self)
    }
    pub fn list(&self, tys: &[Ty]) -> TyList {
        self.types().list(tys)
    }
    /// A list's items, borrowed from the pool: no copy.
    #[must_use]
    pub fn list_items(&self, l: TyList) -> &[Ty] {
        self.types().list_items(l)
    }
    pub fn row(&self, row: &RowData) -> RowId {
        self.types().row(row)
    }
    #[must_use]
    pub fn row_data(&self, r: RowId) -> RowData {
        self.types().row_data(r)
    }
    pub fn product(&self, elems: TyList, rest: Option<Ty>) -> Ty {
        self.types().product(elems, rest)
    }
    #[must_use]
    pub fn product_parts(&self, t: Ty) -> Option<(&[Ty], Option<Ty>)> {
        self.types().product_parts(t)
    }
    pub fn intern_ty(&self, t: &TyData) -> Ty {
        self.types().intern_ty(t)
    }
    #[must_use]
    pub fn get(&self, t: Ty) -> TyData {
        self.types().get(t)
    }
    /// [`Types::adt`].
    #[must_use]
    pub fn adt(&self, t: Ty) -> Option<(DefId, TyList)> {
        self.types().adt(t)
    }
    /// [`Types::adt_parts`].
    #[must_use]
    pub fn adt_parts(&self, t: Ty) -> (DefId, TyList) {
        self.types().adt_parts(t)
    }
    /// [`Types::ref_inner`].
    #[must_use]
    pub fn ref_inner(&self, t: Ty) -> Ty {
        self.types().ref_inner(t)
    }
    /// [`Types::trait_value`].
    #[must_use]
    pub fn trait_value(&self, t: Ty) -> Option<(DefId, TyList)> {
        self.types().trait_value(t)
    }
    /// [`Types::trait_value_parts`].
    #[must_use]
    pub fn trait_value_parts(&self, t: Ty) -> (DefId, TyList) {
        self.types().trait_value_parts(t)
    }
    /// [`Types::sole_arg`].
    #[must_use]
    pub fn sole_arg(&self, t: Ty, def: DefId) -> Option<Ty> {
        self.types().sole_arg(t, def)
    }
    /// [`Types::applied`].
    pub fn applied(&self, def: DefId, arg: Ty) -> Ty {
        self.types().applied(def, arg)
    }
    #[must_use]
    pub fn has_poison(&self, t: Ty) -> bool {
        self.types().has_poison(t)
    }
    #[must_use]
    pub fn has_param(&self, t: Ty) -> bool {
        self.types().has_param(t)
    }
    #[must_use]
    pub fn has_opaque(&self, t: Ty) -> bool {
        self.types().has_opaque(t)
    }
    #[must_use]
    pub fn has_assoc(&self, t: Ty) -> bool {
        self.types().has_assoc(t)
    }
    pub fn subst(&self, t: Ty, f: &dyn Fn(ParamRef) -> Option<Ty>) -> Ty {
        self.types().subst(t, f)
    }
    pub fn subst_row(&self, r: RowId, f: &dyn Fn(ParamRef) -> Option<Ty>) -> RowId {
        self.types().subst_row(r, f)
    }
    /// [`Types::erase_fn_deps`].
    #[must_use]
    pub fn erase_fn_deps(&self, t: Ty) -> Ty {
        self.types().erase_fn_deps(t)
    }
    /// [`Types::erase_fn_deps_list`].
    #[must_use]
    pub fn erase_fn_deps_list(&self, l: TyList) -> TyList {
        self.types().erase_fn_deps_list(l)
    }
    /// Items in this pool, as the run's metrics report them.
    #[must_use]
    pub fn item_count(&self) -> u32 {
        self.cols.tag.len()
    }
}

impl<'a> Types<'a> {
    fn meta_of(self, t: Ty) -> u32 {
        self.meta_word(t.0)
    }

    fn list_meta(self, l: TyList) -> u32 {
        self.meta_word(l.0)
    }

    pub(crate) fn list_has_infer(self, l: TyList) -> bool {
        self.list_meta(l) & meta::HAS_INFER != 0
    }
    pub(crate) fn list_has_canon(self, l: TyList) -> bool {
        self.list_meta(l) & meta::HAS_CANON != 0
    }

    pub(crate) fn list_has_param(self, l: TyList) -> bool {
        self.list_meta(l) & meta::HAS_PARAM != 0
    }

    #[must_use]
    pub fn list(self, tys: &[Ty]) -> TyList {
        let meta = tys.iter().fold(0, |m, t| meta::join(m, self.meta_of(*t)));
        let slots = tys
            .iter()
            .fold(Slots::NONE, |s, t| s.plus(self.slots_word(t.0)));
        TyList(self.intern(PoolTag::List, 0, (&[], tys), meta, slots))
    }

    /// A list's items, borrowed from the pool: no copy.
    #[must_use]
    pub fn list_items(self, l: TyList) -> &'a [Ty] {
        let (c, i) = self.at(l.0);
        c.list(i)
    }

    /// Interns a row. A key that is itself a row (`TyData::Row`, a solved
    /// row variable) is flattened into this one (`req.row.set.parameter`).
    #[must_use]
    pub fn row(self, row: &RowData) -> RowId {
        let mut keys = Vec::new();
        let mut params = row.params.clone();
        for k in &row.keys {
            if let TyData::Row(r) = self.get(*k) {
                let inner = self.row_data(r);
                keys.extend(inner.keys);
                params.extend(inner.params);
            } else {
                keys.push(*k);
            }
        }
        keys.sort_stable_by_key(|t| t.0);
        keys.dedup();
        params.sort_stable_by_key(|p| (p.owner.raw(), p.index));
        params.dedup();
        let mut meta = keys.iter().fold(0, |m, t| meta::join(m, self.meta_of(*t)));
        if !params.is_empty() {
            meta |= meta::HAS_PARAM;
        }
        let mut words: Vec<u32> = vec![crate::word(keys.len())];
        words.extend(keys.iter().map(|t| t.0));
        words.push(crate::word(params.len()));
        for p in &params {
            words.push(p.owner.raw());
            words.push(u32::from(p.index));
        }
        RowId(self.intern(PoolTag::Row, 0, (&words, &[]), meta, Slots::NONE))
    }

    /// Interns a dependency map ([`FnDeps`]): `ins` gives each input
    /// slot's lifetime and `outs` each result slot's, any nonzero word
    /// naming one, 0 for a result slot with no source. Only what the
    /// result depends on counts: an input lifetime no result slot has
    /// becomes the slot's own, and lifetimes are renumbered by first
    /// occurrence, so maps that differ only in their names are one. A
    /// result lifetime no input has becomes 0.
    #[must_use]
    pub fn fn_deps(self, ins: &[u32], outs: &[u32]) -> FnDeps {
        if outs.is_empty() {
            return FnDeps::NONE;
        }
        let (words, _) = Self::dependency_words(ins, outs);
        FnDeps(self.intern(PoolTag::Deps, 0, (&words, &[]), 0, Slots::NONE))
    }

    /// Canonical order follows the inputs, independent of hash-table order.
    /// Work is one visit per input and two per output; no nesting or retry.
    fn dependency_words(ins: &[u32], outs: &[u32]) -> (Vec<u32>, usize) {
        let mut retained = HashSet::default();
        let mut work = 0;
        for &lifetime in outs {
            retained.insert(lifetime);
            work += 1;
        }
        let mut seen = HashMap::default();
        let mut words = vec![crate::word(ins.len()), crate::word(outs.len())];
        for (k, &l) in ins.iter().enumerate() {
            work += 1;
            let l = if retained.contains(&l) {
                l
            } else {
                1 << 31 | crate::word(k)
            };
            let next = seen.len();
            let at = *seen.entry(l).or_insert(next);
            words.push(crate::word(at + 1));
        }
        for &l in outs {
            work += 1;
            let at = (l != 0).then(|| seen.get(&l).copied()).flatten();
            words.push(at.map_or(0, |at| crate::word(at + 1)));
        }
        (words, work)
    }

    /// The map of a function type whose slots write `written`, one word
    /// per slot of `params` and then of `result`, 0 where none is written
    /// (`fn.type.lifetime.elide`): an input slot that writes none has a
    /// lifetime of its own, and a result slot that writes none takes the
    /// inputs' one lifetime, or none.
    #[must_use]
    pub fn fn_deps_written(self, params: &[Ty], result: Ty, written: &[u16]) -> FnDeps {
        let n_in: usize = params.iter().map(|&t| self.ref_slots(t)).sum();
        let n_out = self.ref_slots(result);
        let at = |k: usize| written.get(k).copied().filter(|&l| l != 0).map(u32::from);
        let ins: Vec<u32> = (0..n_in)
            .map(|k| at(k).unwrap_or(1 << 16 | crate::word(k)))
            .collect();
        let mut distinct = ins.clone();
        distinct.sort_stable();
        distinct.dedup();
        let elided = match distinct.as_slice() {
            [one] => *one,
            _ => 0,
        };
        let outs: Vec<u32> = (0..n_out).map(|j| at(n_in + j).unwrap_or(elided)).collect();
        if n_out == 0 && self.has_param(result) && (0..n_in).any(|k| at(k).is_some()) {
            // An open map: which inputs write a lifetime, which a type
            // parameter of the result may name (`fn.type.lifetime.result.param`).
            let open: Vec<bool> = (0..n_in).map(|k| at(k).is_some()).collect();
            return self.fn_deps_open(&open);
        }
        self.fn_deps(&ins, &outs)
    }

    /// An open dependency map ([`FnDeps`]): a result with no reference
    /// slot yet, and for each input slot whether it writes a lifetime,
    /// 1, or not, 0.
    pub(crate) fn fn_deps_open(self, written: &[bool]) -> FnDeps {
        let mut words = vec![0, 0];
        for (n, &x) in (1..).zip(written) {
            words[0] = n;
            words.push(u32::from(x));
        }
        FnDeps(self.intern(PoolTag::Deps, 0, (&words, &[]), 0, Slots::NONE))
    }

    /// The map an instance of a function type with an open map, whose
    /// inputs `ins` mark the ones that write a lifetime, takes once its
    /// result has `n_out` reference slots and its inputs `n_in`
    /// (`fn.type.lifetime.result.param`): each result slot depends on the
    /// inputs that write a lifetime, which a type parameter may name, and
    /// no others. Where the inputs no longer line up with the map, a
    /// result slot depends on none.
    fn fn_deps_instance(self, ins: &[u32], n_in: usize, n_out: usize) -> FnDeps {
        let lined = ins.len() == n_in;
        // An input that writes none has a lifetime of its own, past every
        // written one's 1.
        let ins: Vec<u32> = ((2..).zip(0..n_in))
            .map(|(own, k)| {
                if lined && ins.get(k) == Some(&1) {
                    1
                } else {
                    own
                }
            })
            .collect();
        let w = u32::from(ins.contains(&1));
        self.fn_deps(&ins, &vec![w; n_out])
    }

    /// The map of a function type rebuilt from one with `before`'s
    /// parameters, result and map by replacing type holes (a type
    /// parameter, an inference variable, a projection) in it
    /// (`fn.type.lifetime.result.param`): a reference slot a hole's
    /// replacement adds carries that hole's lifetime at that position of
    /// the replacement, which each occurrence of the hole shares, so a
    /// result slot depends on the input slots at the same position of the
    /// same hole, as `fn(T) -> T` at `&Point` depends on its input. A slot
    /// that `before` had keeps its lifetime, and with an open map a result
    /// slot a hole adds may also depend on the inputs that write a
    /// lifetime, when only one hole adds result slots. `before`'s map when
    /// nothing was added, or when the two do not line up or the map cannot
    /// say it (a union's members rebuilt). Cost: nothing for a result
    /// with no reference; else one walk of each type, which, as
    /// [`Self::ref_slots`], does not enter the function types inside it.
    fn instance_deps(self, (gp, gr, gd): (&[Ty], Ty, FnDeps), (ip, ir): (&[Ty], Ty)) -> FnDeps {
        // No result slot: the map stays `NONE` or open.
        if !self.has_ref(ir) {
            return gd;
        }
        let slots = |ps: &[Ty]| ps.iter().map(|&p| self.ref_slots(p)).sum::<usize>();
        let (g_in, g_out) = (slots(gp), self.ref_slots(gr));
        if slots(ip) == g_in && self.ref_slots(ir) == g_out {
            return gd;
        }
        let mut ins = Vec::new();
        let mut at = 0;
        for (&g, &i) in gp.iter().zip(ip) {
            if !self.slot_origins(g, i, &mut at, &mut ins) {
                return gd;
            }
        }
        let mut outs = Vec::new();
        let mut at_out = 0;
        if gp.len() != ip.len() || !self.slot_origins(gr, ir, &mut at_out, &mut outs) {
            return gd;
        }
        let (g_ins, g_outs, open) = match self.deps_of(gd) {
            Some((i, o)) if i.len() == g_in && o.len() == g_out && !o.is_empty() => {
                (i.to_vec(), o.to_vec(), Vec::new())
            }
            Some((i, [])) if i.len() == g_in => (Vec::new(), Vec::new(), i.to_vec()),
            _ => (Vec::new(), Vec::new(), Vec::new()),
        };
        // A slot with no written lifetime, and each (hole, position),
        // takes the next fresh word, past any a map writes.
        let mut holes: HashMap<(Ty, u32), u32> = HashMap::default();
        let mut fresh: u32 = 1 << 30;
        let mut word = |o: Origin| {
            let next = || {
                fresh = fresh.saturating_add(1);
                fresh
            };
            match o {
                Origin::Slot(k) => g_ins.get(k).copied().unwrap_or_else(next),
                Origin::Hole(h, m) => *holes.entry((h, m)).or_insert_with(next),
            }
        };
        let mut in_words: Vec<u32> = ins.iter().map(|&o| word(o)).collect();
        let out_words: Vec<u32> = outs
            .iter()
            .map(|&o| match o {
                Origin::Slot(j) => g_outs.get(j).copied().unwrap_or(0),
                Origin::Hole(h, m) => holes.get(&(h, m)).copied().unwrap_or(0),
            })
            .collect();
        // An open map: the inputs that write a lifetime may also be the
        // source of a slot a hole adds to the result, which one word says
        // only when one (hole, position) adds result slots.
        if open.contains(&1) {
            let added: hd_base::HashSet<u32> =
                out_words.iter().copied().filter(|&w| w != 0).collect();
            let mut one = added.iter().copied();
            let (Some(target), None) = (one.next(), one.next()) else {
                return gd;
            };
            for (w, o) in in_words.iter_mut().zip(&ins) {
                if let Origin::Slot(k) = o
                    && open.get(*k) == Some(&1)
                {
                    *w = target;
                }
            }
            if out_words.contains(&0) {
                return gd;
            }
        }
        self.fn_deps(&in_words, &out_words)
    }

    /// Appends the origin of each reference slot of `i`, an instance of
    /// `g`, in [`Self::ref_slots`] order, numbering `g`'s own slots from
    /// `at`. False when the two do not line up. Parts are pushed in
    /// reverse ([`hd_base::stack::all`]), so slots append in order.
    fn slot_origins(self, g: Ty, i: Ty, at: &mut usize, out: &mut Vec<Origin>) -> bool {
        hd_base::stack::all((g, i, 0), |(g, i, depth), stack| {
            if depth > hd_base::limits::TYPE_DEPTH {
                return false;
            }
            if g == i {
                let n = self.ref_slots(g);
                out.extend((*at..*at + n).map(Origin::Slot));
                *at += n;
                return true;
            }
            if !self.has_ref(i) && !self.has_ref(g) {
                return true;
            }
            let d = depth + 1;
            let pairs = |stack: &mut Vec<_>, a: TyList, b: TyList| {
                let (a, b) = (self.list_items(a), self.list_items(b));
                stack.extend(a.iter().zip(b).rev().map(|(&x, &y)| (x, y, d)));
                a.len() == b.len()
            };
            match (self.get(g), self.get(i)) {
                (TyData::Param(_) | TyData::Infer(_) | TyData::Assoc { .. }, _)
                    if self.ref_slots(g) == 0 =>
                {
                    let mut m: u32 = 0;
                    for _ in 0..self.ref_slots(i) {
                        out.push(Origin::Hole(g, m));
                        m = m.saturating_add(1);
                    }
                    true
                }
                (
                    TyData::Ref {
                        inner: a,
                        counted: ca,
                        ..
                    },
                    TyData::Ref {
                        inner: b,
                        counted: cb,
                        ..
                    },
                ) if ca == cb => {
                    if !ca {
                        out.push(Origin::Slot(*at));
                        *at += 1;
                    }
                    stack.push((a, b, d));
                    true
                }
                // A function type inside has no slot of the outer one.
                (TyData::Fn { .. }, TyData::Fn { .. }) => true,
                // Product identity includes the rest flag; the final member is
                // visited after fixed members, as the source's slots require.
                (TyData::Adt(x, a), TyData::Adt(y, b)) if x == y && x != AdtId::Sum => {
                    pairs(stack, a, b)
                }
                (
                    TyData::TraitValue {
                        def: x, args: a, ..
                    },
                    TyData::TraitValue {
                        def: y, args: b, ..
                    },
                ) if x == y => pairs(stack, a, b),
                _ => false,
            }
        })
    }

    /// A dependency map's input and result lifetimes; `None` for `NONE`.
    #[must_use]
    pub fn deps_of(self, d: FnDeps) -> Option<(&'a [u32], &'a [u32])> {
        if d == FnDeps::NONE {
            return None;
        }
        let (c, i) = self.at(d.0);
        let x = c.record_of(PoolTag::Deps, c.data[i]);
        let n = x[0] as usize;
        Some((&x[2..2 + n], &x[2 + n..]))
    }

    /// The dependency map of the function type `t`, under any `mut`,
    /// when it lines up with the type's reference slots.
    #[must_use]
    pub fn fn_ty_deps(self, t: Ty) -> Option<(&'a [u32], &'a [u32])> {
        match self.get(t) {
            TyData::Fn {
                params,
                result,
                deps,
                ..
            } => {
                let (ins, outs) = self.deps_of(deps)?;
                let n_in: usize = self
                    .list_items(params)
                    .iter()
                    .map(|&p| self.ref_slots(p))
                    .sum();
                (ins.len() == n_in && outs.len() == self.ref_slots(result)).then_some((ins, outs))
            }
            _ => None,
        }
    }

    fn row_meta(self, r: RowId) -> u32 {
        self.meta_word(r.0)
    }

    #[must_use]
    pub fn row_data(self, r: RowId) -> RowData {
        let (c, i) = self.at(r.0);
        let w = c.record(i);
        let n = w[0] as usize;
        let keys = w[1..=n].iter().map(|&k| Ty(k)).collect();
        let params = w[n + 2..]
            .chunks(2)
            .map(|c| RowParamRef {
                owner: DefId::from_raw(c[0]),
                index: crate::low16(c[1]),
            })
            .collect();
        RowData { keys, params }
    }

    /// Interns a type: in the body's local pool when it holds an inference
    /// variable, else in the global pool (§3.4). A function type whose
    /// inputs tuple is a tuple type takes its one-parameter-per-element
    /// form ([`Inputs::Tuple`]).
    #[must_use]
    pub fn intern_ty(self, t: &TyData) -> Ty {
        use PoolTag as T;
        if let TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs: Inputs::Tuple,
            deps,
        } = t
            && let [one] = self.list_items(*params)
            && let TyData::Adt(AdtId::Product { rest }, params) = self.get(*one)
        {
            let inputs = if rest { Inputs::Rest } else { Inputs::Fixed };
            return self.intern_ty(&TyData::Fn {
                params,
                result: *result,
                row: *row,
                suspends: *suspends,
                inputs,
                deps: *deps,
            });
        }
        // An open map whose result an instance gave reference slots
        // (`fn.type.lifetime.result.param`).
        if let TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs,
            deps,
        } = t
            && let Some((ins, [])) = self.deps_of(*deps)
            && self.ref_slots(*result) > 0
        {
            let n_in = self
                .list_items(*params)
                .iter()
                .map(|&p| self.ref_slots(p))
                .sum();
            let deps = self.fn_deps_instance(ins, n_in, self.ref_slots(*result));
            return self.intern_ty(&TyData::Fn {
                params: *params,
                result: *result,
                row: *row,
                suspends: *suspends,
                inputs: *inputs,
                deps,
            });
        }
        let j = meta::join;
        let (tag, data, meta): (PoolTag, u32, u32) = match t {
            TyData::Prim(p) => (T::Prim, *p as u32, 0),
            TyData::Never => (T::Never, 0, 0),
            TyData::Poison => (T::Poison, 0, meta::HAS_POISON),
            TyData::Adt(id, args) => {
                let (kind, data) = match id {
                    AdtId::Declared(_) => (AdtTag::Declared, 0),
                    AdtId::Sum => (AdtTag::Sum, args.0),
                    AdtId::Product { rest: false } => (AdtTag::Product, args.0),
                    AdtId::Product { rest: true } => (AdtTag::RestProduct, args.0),
                };
                (T::Adt(kind), data, meta::level(self.list_meta(*args)))
            }

            TyData::Fn {
                params,
                result,
                row,
                ..
            } => (
                T::Fn,
                0,
                meta::HAS_FN
                    | meta::level(j(
                        j(self.list_meta(*params), self.meta_of(*result)),
                        self.row_meta(*row),
                    )),
            ),
            TyData::TraitValue { args, bindings, .. } => (
                T::TraitValue,
                0,
                meta::level(
                    bindings
                        .iter()
                        .fold(self.list_meta(*args), |m, (_, t)| j(m, self.meta_of(*t))),
                ),
            ),
            TyData::Opaque {
                args,
                bound,
                borrows,
                ..
            } => (
                T::Opaque,
                0,
                meta::HAS_OPAQUE
                    | if *borrows { meta::HAS_REF } else { 0 }
                    | meta::level(j(self.list_meta(*args), self.meta_of(*bound))),
            ),
            TyData::Param(_) => (T::Param, 0, meta::HAS_PARAM),
            TyData::Const(_) => (T::Const, 0, 0),
            TyData::Assoc { self_ty, args, .. } => (
                T::Assoc,
                0,
                meta::HAS_ASSOC | meta::level(j(self.meta_of(*self_ty), self.list_meta(*args))),
            ),
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => (
                match (*counted, *mutable) {
                    (false, false) => T::Ref,
                    (false, true) => T::RefMut,
                    (true, false) => T::CRef,
                    (true, true) => T::CRefMut,
                },
                inner.0,
                meta::HAS_REF | meta::level(self.meta_of(*inner)),
            ),
            TyData::Infer(v) => (T::Infer, v.raw(), meta::HAS_INFER),
            TyData::Canon(i) => (T::Canon, u32::from(*i), meta::HAS_CANON),
            TyData::Row(r) => (T::RowTy, r.0, meta::level(self.row_meta(*r))),
            TyData::Context(r) => (T::Context, r.0, meta::level(self.row_meta(*r))),
        };
        let slots = self.slots_of_data(t);
        let mut buf = [0u32; 5];
        let rec: &[u32] = match t {
            TyData::Adt(AdtId::Declared(def), args) => {
                buf[..2].copy_from_slice(&[def.raw(), args.0]);
                &buf[..2]
            }

            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => {
                buf = [params.0, result.0, row.0, inputs.word(*suspends), deps.0];
                &buf
            }
            TyData::Opaque {
                owner,
                site,
                borrows,
                args,
                bound,
            } => {
                buf = [owner.raw(), *site, args.0, bound.0, u32::from(*borrows)];
                &buf
            }
            TyData::Param(p) => {
                buf[..2].copy_from_slice(&[p.owner.raw(), u32::from(p.index)]);
                &buf[..2]
            }
            TyData::Const(v) => {
                let bits = v.bits();
                buf[..3].copy_from_slice(&[v.prim() as u32, low(bits), low(bits >> 32)]);
                &buf[..3]
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                buf[..4].copy_from_slice(&[assoc.raw(), trait_.raw(), self_ty.0, args.0]);
                &buf[..4]
            }
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let mut b = bindings.clone();
                b.sort_stable_by_key(|(d, _)| d.raw());
                let mut w = vec![def.raw(), args.0, u32::try_from(b.len()).expect("bindings")];
                for (d, t) in b {
                    w.push(d.raw());
                    w.push(t.0);
                }
                return Ty(self.intern(tag, data, (&w, &[]), meta, slots));
            }
            _ => &[],
        };
        Ty(self.intern(tag, data, (rec, &[]), meta, slots))
    }

    /// The inputs tuple of a function type with these `params`
    /// (`fn.type.ctor.inputs`): `(A, B)`, `(A, List[T]...)`, or the one
    /// type that stands for it.
    #[must_use]
    pub fn inputs_tuple(self, params: TyList, inputs: Inputs) -> Ty {
        let ps = self.list_items(params);
        match (inputs, ps) {
            (Inputs::Tuple, [one]) => *one,
            (Inputs::Rest, [.., _]) => {
                self.intern_ty(&TyData::Adt(AdtId::Product { rest: true }, params))
            }
            _ => self.product(params, None),
        }
    }

    /// Constructs an anonymous product. Only the source's rest form needs
    /// a new combined member list; ordinary products reuse `elems` directly.
    #[must_use]
    pub fn product(self, elems: TyList, rest: Option<Ty>) -> Ty {
        let args = if let Some(rest) = rest {
            let mut parts = self.list_items(elems).to_vec();
            parts.push(rest);
            self.list(&parts)
        } else {
            elems
        };
        self.intern_ty(&TyData::Adt(
            AdtId::Product {
                rest: rest.is_some(),
            },
            args,
        ))
    }

    /// Product members borrowed in O(1); splitting the rest never interns
    /// or allocates a prefix. Malformed rest shapes have no parts.
    #[must_use]
    pub fn product_parts(self, t: Ty) -> Option<(&'a [Ty], Option<Ty>)> {
        let (c, i) = self.at(t.0);
        match c.tag[i] {
            PoolTag::Adt(AdtTag::Product) => Some((self.list_items(TyList(c.data[i])), None)),
            PoolTag::Adt(AdtTag::RestProduct) => {
                let (last, fixed) = self.list_items(TyList(c.data[i])).split_last()?;
                Some((fixed, Some(*last)))
            }
            _ => None,
        }
    }

    /// A declared ADT's definition and argument-list identity.
    /// ruled out: anonymous sums have no declaration to query.
    #[must_use]
    pub fn adt(self, t: Ty) -> Option<(DefId, TyList)> {
        let (c, i) = self.at(t.0);
        if c.tag[i] != PoolTag::Adt(AdtTag::Declared) {
            return None;
        }
        let off = c.data[i];
        let def = DefId::from_raw(c.extra[off]);
        def.get().map(|def| (def, TyList(c.extra[off + 1])))
    }

    /// The definition and arguments of a type already checked as an ADT.
    #[must_use]
    pub fn adt_parts(self, t: Ty) -> (DefId, TyList) {
        let (c, i) = self.at(t.0);
        // ruled out: callers have established the ADT constructor.
        debug_assert_eq!(c.tag[i], PoolTag::Adt(AdtTag::Declared));
        let off = c.data[i];
        (DefId::from_raw(c.extra[off]), TyList(c.extra[off + 1]))
    }

    /// The declared referent of an already checked reference constructor.
    #[must_use]
    pub fn ref_inner(self, t: Ty) -> Ty {
        let (c, i) = self.at(t.0);
        // ruled out: callers established a local or counted reference constructor.
        debug_assert!(matches!(
            c.tag[i],
            PoolTag::Ref | PoolTag::RefMut | PoolTag::CRef | PoolTag::CRefMut
        ));
        Ty(c.data[i])
    }

    /// `T` when `t` is `def[T]`: the payload of `Option[T]`, which `T?`
    /// spells (`types.option.sugar`), read without decoding `t`.
    #[must_use]
    pub fn sole_arg(self, t: Ty, def: DefId) -> Option<Ty> {
        match self.adt(t)? {
            (d, args) if d == def => match self.list_items(args) {
                &[arg] => Some(arg),
                _ => None,
            },
            _ => None,
        }
    }

    /// `def[arg]`: `Option[T]` for `T?` (`types.option.sugar`).
    #[must_use]
    pub fn applied(self, def: DefId, arg: Ty) -> Ty {
        self.intern_ty(&TyData::Adt(AdtId::Declared(def), self.list(&[arg])))
    }

    /// The function type `f` with `lead` before its parameters and, with
    /// `to`, another result and suspension; `None` when `f` is no function.
    #[must_use]
    pub fn fn_led(self, f: Ty, lead: Ty, to: Option<(Ty, bool)>) -> Option<Ty> {
        let TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs,
            deps,
        } = self.get(f)
        else {
            return None;
        };
        let (result, suspends) = to.unwrap_or((result, suspends));
        let params: Vec<Ty> = std::iter::once(lead)
            .chain(self.list_items(params).iter().copied())
            .collect();
        let params = self.list(&params);
        Some(self.intern_ty(&TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs,
            deps,
        }))
    }

    /// A trait value's trait and argument-list identity, without allocating
    /// associated bindings. `trait_value_bindings` borrows the bindings.
    #[must_use]
    pub fn trait_value(self, t: Ty) -> Option<(DefId, TyList)> {
        let (c, i) = self.at(t.0);
        if c.tag[i] != PoolTag::TraitValue {
            return None;
        }
        let off = c.data[i];
        Some((DefId::from_raw(c.extra[off]), TyList(c.extra[off + 1])))
    }

    /// Borrow the canonical, DefId-sorted associated bindings without a clone.
    /// `intern_ty` sorts these words before interning every trait value.
    #[must_use]
    pub fn trait_value_bindings(
        self,
        t: Ty,
    ) -> Option<impl ExactSizeIterator<Item = (DefId, Ty)> + 'a> {
        self.trait_binding_words(t).map(|words| {
            words
                .chunks_exact(2)
                .map(|pair| (DefId::from_raw(pair[0]), Ty(pair[1])))
        })
    }

    /// Find one associated binding in O(log bindings), with no allocation.
    #[must_use]
    pub fn trait_value_binding(self, t: Ty, name: DefId) -> Option<Ty> {
        let words = self.trait_binding_words(t)?;
        let (mut lo, mut hi) = (0, words.len() / 2);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match words[2 * mid].cmp(&name.raw()) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(Ty(words[2 * mid + 1])),
            }
        }
        None
    }

    fn trait_binding_words(self, t: Ty) -> Option<&'a [u32]> {
        let (c, i) = self.at(t.0);
        if c.tag[i] != PoolTag::TraitValue {
            return None;
        }
        let off = c.data[i];
        let len = c.extra[off + 2];
        Some(c.extra.run(off + 3, 2 * len))
    }

    /// The trait and argument-list identity of a type its producer made a
    /// trait value, as [`Types::adt_parts`] reads an ADT.
    #[must_use]
    pub fn trait_value_parts(self, t: Ty) -> (DefId, TyList) {
        let (c, i) = self.at(t.0);
        // ruled out: callers name the producer that makes `t` a trait value.
        debug_assert_eq!(c.tag[i], PoolTag::TraitValue);
        let off = c.data[i];
        (DefId::from_raw(c.extra[off]), TyList(c.extra[off + 1]))
    }

    /// Decodes a type (`docs/compiler/data-structures.md` §3.4).
    #[must_use]
    pub fn get(self, t: Ty) -> TyData {
        let (c, i) = self.at(t.0);
        let (tag, d) = (c.tag[i], c.data[i]);
        let x = c.record_of(tag, d);
        match tag {
            PoolTag::Prim => TyData::Prim(Prim::ALL[d as usize]),
            PoolTag::Never => TyData::Never,
            PoolTag::Poison => TyData::Poison,
            PoolTag::Adt(kind) => {
                let (id, args) = match kind {
                    AdtTag::Declared => (AdtId::Declared(DefId::from_raw(x[0])), TyList(x[1])),
                    AdtTag::Sum => (AdtId::Sum, TyList(d)),
                    AdtTag::Product => (AdtId::Product { rest: false }, TyList(d)),
                    AdtTag::RestProduct => (AdtId::Product { rest: true }, TyList(d)),
                };
                TyData::Adt(id, args)
            }

            PoolTag::Fn => TyData::Fn {
                params: TyList(x[0]),
                result: Ty(x[1]),
                row: RowId(x[2]),
                suspends: x[3] & 1 != 0,
                inputs: Inputs::from_bits(x[3]),
                deps: FnDeps(x[4]),
            },
            PoolTag::TraitValue => TyData::TraitValue {
                def: DefId::from_raw(x[0]),
                args: TyList(x[1]),
                bindings: x[3..]
                    .chunks(2)
                    .map(|c| (DefId::from_raw(c[0]), Ty(c[1])))
                    .collect(),
            },
            PoolTag::Opaque => TyData::Opaque {
                owner: DefId::from_raw(x[0]),
                site: x[1],
                borrows: x[4] != 0,
                args: TyList(x[2]),
                bound: Ty(x[3]),
            },
            PoolTag::Param => TyData::Param(ParamRef {
                owner: DefId::from_raw(x[0]),
                index: crate::low16(x[1]),
            }),
            PoolTag::Const => TyData::Const(ConstValue::from_bits(
                Prim::ALL[x[0] as usize],
                u64::from(x[1]) | u64::from(x[2]) << 32,
            )),
            PoolTag::Assoc => TyData::Assoc {
                assoc: DefId::from_raw(x[0]),
                trait_: DefId::from_raw(x[1]),
                self_ty: Ty(x[2]),
                args: TyList(x[3]),
            },
            PoolTag::Ref => TyData::Ref {
                inner: Ty(d),
                mutable: false,
                counted: false,
            },
            PoolTag::RefMut => TyData::Ref {
                inner: Ty(d),
                mutable: true,
                counted: false,
            },
            PoolTag::CRef => TyData::Ref {
                inner: Ty(d),
                mutable: false,
                counted: true,
            },
            PoolTag::CRefMut => TyData::Ref {
                inner: Ty(d),
                mutable: true,
                counted: true,
            },
            PoolTag::Infer => TyData::Infer(InferVar::from_raw(d)),
            // The word holds the `u8` that `TyData::Canon` was interned with.
            PoolTag::Canon => TyData::Canon(d.to_le_bytes()[0]),
            PoolTag::RowTy => TyData::Row(RowId(d)),
            PoolTag::Context => TyData::Context(RowId(d)),
            PoolTag::List | PoolTag::Row | PoolTag::Deps => unreachable!("not a type index"),
        }
    }

    #[must_use]
    pub fn has_infer(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_INFER != 0
    }
    #[must_use]
    pub fn has_poison(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_POISON != 0
    }
    #[must_use]
    pub fn has_opaque(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_OPAQUE != 0
    }
    #[must_use]
    pub fn has_param(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_PARAM != 0
    }
    /// Appends `t`'s child types to `out` in written order, as
    /// [`Self::map_children`] visits them, with a row's keys in the row's
    /// place. A leaf appends nothing. Cost: one pass over the children.
    pub fn children(self, t: Ty, out: &mut Vec<Ty>) {
        match self.get(t) {
            TyData::Adt(_, args) => {
                out.extend_from_slice(self.list_items(args));
            }
            TyData::Opaque { args, bound, .. } => {
                out.extend_from_slice(self.list_items(args));
                out.push(bound);
            }

            TyData::Ref { inner, .. } => out.push(inner),
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                out.extend_from_slice(self.list_items(params));
                out.push(result);
                out.extend(self.row_data(row).keys);
            }
            TyData::Row(row) | TyData::Context(row) => out.extend(self.row_data(row).keys),
            TyData::TraitValue { args, bindings, .. } => {
                out.extend_from_slice(self.list_items(args));
                out.extend(bindings.into_iter().map(|(_, b)| b));
            }
            TyData::Assoc { self_ty, args, .. } => {
                out.push(self_ty);
                out.extend_from_slice(self.list_items(args));
            }
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Param(_)
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_) => {}
        }
    }

    /// Rebuilds `t` with each child type mapped by `m.ty` and each row by
    /// `m.row`, children in written order: arguments, elements then rest,
    /// members, a referent, parameters then result then row, a bound after
    /// its arguments, bindings after arguments, an associated type's base
    /// before its arguments. A leaf comes back as it is. Every walk that
    /// rebuilds a type shares this one map. Cost: one call per child and
    /// one intern.
    pub fn map_children(self, t: Ty, m: &mut dyn TyMap) -> Ty {
        let list = |m: &mut dyn TyMap, l: TyList| {
            let items: Vec<Ty> = self.list_items(l).iter().map(|&x| m.ty(x)).collect();
            self.list(&items)
        };
        let data = match self.get(t) {
            TyData::Adt(def, args) => TyData::Adt(def, list(m, args)),
            TyData::Opaque {
                owner,
                site,
                borrows,
                args,
                bound,
            } => {
                let args = list(m, args);
                TyData::Opaque {
                    owner,
                    site,
                    borrows,
                    args,
                    bound: m.ty(bound),
                }
            }

            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: m.ty(inner),
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => {
                let before = params;
                let params = list(m, params);
                let result_before = result;
                let result = m.ty(result);
                let deps = self.instance_deps(
                    (self.list_items(before), result_before, deps),
                    (self.list_items(params), result),
                );
                TyData::Fn {
                    params,
                    result,
                    row: m.row(row),
                    suspends,
                    inputs,
                    deps,
                }
            }
            TyData::Row(r) => TyData::Row(m.row(r)),
            TyData::Context(r) => TyData::Context(m.row(r)),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let args = list(m, args);
                TyData::TraitValue {
                    def,
                    args,
                    bindings: bindings.into_iter().map(|(d, b)| (d, m.ty(b))).collect(),
                }
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let self_ty = m.ty(self_ty);
                TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty,
                    args: list(m, args),
                }
            }
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Param(_)
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_) => return t,
        };
        self.intern_ty(&data)
    }

    /// Whether a type names `Self` of `trait_` (parameter 0 of the trait),
    /// outside the base of an associated-type projection. Each distinct
    /// parameter-holding node and its child edges are visited once; the
    /// explicit stack keeps native stack use constant at any type depth.
    #[must_use]
    pub fn names_trait_self(self, t: Ty, trait_: DefId) -> bool {
        if !self.has_param(t) {
            return false;
        }
        let mut seen = hd_base::HashSet::default();
        let is_self = |p: ParamRef| p.owner == trait_ && p.index == 0;
        !hd_base::stack::all(t, |t, stack| {
            if !self.has_param(t) || !seen.insert(t) {
                return true;
            }
            crate::work::type_node();
            if let TyData::Param(p) = self.get(t) {
                return !is_self(p);
            }
            let start = stack.len();
            match self.get(t) {
                TyData::Assoc { self_ty, args, .. } if matches!(self.get(self_ty), TyData::Param(p) if is_self(p)) =>
                {
                    stack.extend_from_slice(self.list_items(args));
                }
                _ => self.children(t, stack),
            }
            stack[start..].reverse();
            true
        })
    }
    /// [`Self::names_trait_self`] over a row's keys.
    #[must_use]
    pub fn row_names_trait_self(self, r: RowId, trait_: DefId) -> bool {
        self.row_data(r)
            .keys
            .iter()
            .any(|k| self.names_trait_self(*k, trait_))
    }
    #[must_use]
    pub fn has_assoc(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_ASSOC != 0
    }
    /// Whether `t` holds a canonical placeholder (`crates/hd_check/DESIGN.md` (trait solver) §2.2).
    #[must_use]
    pub fn has_canon(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_CANON != 0
    }
    /// Whether `t` holds a reference type of either kind, local or counted,
    /// or an opaque type that borrows.
    #[must_use]
    pub fn has_ref(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_REF != 0
    }

    /// Whether `t` holds a function type.
    #[must_use]
    pub fn has_fn(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_FN != 0
    }

    /// Type constructors on the longest path from `t` down to a leaf
    /// ([`meta::depth`]), read from its meta word: no walk.
    #[must_use]
    pub fn depth(self, t: Ty) -> u32 {
        meta::depth(self.meta_of(t))
    }

    /// The local reference slots of `t`: each `&T` and `&mut T` outside
    /// function types, through counted references
    /// (`ref.local.holding.counted`), and each opaque type that borrows
    /// (`types.opaque.borrows`), in pre-order. A signature's
    /// written lifetimes follow this order (`FnSig::lifetimes`).
    #[must_use]
    pub fn ref_slots(self, t: Ty) -> usize {
        self.slots_word(t.0).refs() as usize
    }

    /// The reference slots of every function type in `t`, which a
    /// signature or a field records the written lifetimes of
    /// (`fn.type.lifetime.call.written`, `FnSig::fn_lifetimes`): each
    /// function type, after the function types in its own inputs and
    /// result, adds the [`Self::ref_slots`] of its inputs and then of its
    /// result. Anonymous sums keep their existing conservative signature
    /// maps; no record reaches a function type inside one.
    #[must_use]
    pub fn fn_written_slots(self, t: Ty) -> usize {
        self.slots_word(t.0).written() as usize
    }

    /// The [`Slots`] of a type built from `t`'s parts, from their own
    /// words: one read per part, so each distinct type is counted once,
    /// when it is interned, and a doubled type (`type R{i} = (R{i-1},
    /// R{i-1})`) costs its distinct nodes, not its `2^i` paths (#2024).
    fn slots_of_data(self, t: &TyData) -> Slots {
        let of = |t: Ty| self.slots_word(t.0);
        let list = |l: TyList| self.slots_word(l.0);
        match t {
            TyData::Ref { counted, inner, .. } => {
                let inner = of(*inner);
                Slots::new(
                    inner.refs().saturating_add(u32::from(!counted)),
                    inner.written(),
                )
            }
            TyData::Opaque { borrows, .. } => Slots::new(u32::from(*borrows), 0),

            // Anonymous sums retain their conservative signature maps.
            TyData::Adt(def, args) if *def == ANON_SUM => Slots::new(list(*args).refs(), 0),
            TyData::Adt(_, args) | TyData::TraitValue { args, .. } => list(*args),
            // A function type's own slots are outside every enclosing
            // type's reference slots; its written slots are those of the
            // function types in its inputs and result, then the reference
            // slots of its inputs and of its result.
            TyData::Fn { params, result, .. } => {
                let parts = list(*params).plus(of(*result));
                Slots::new(0, parts.written().saturating_add(parts.refs()))
            }
            _ => Slots::NONE,
        }
    }

    /// `t` with every function type's dependency map erased
    /// ([`FnDeps::NONE`]): the identity monomorphization keys and the
    /// backend read. A map is `NONE` unless its result holds a reference
    /// slot, so only a type with a function type and a reference
    /// ([`meta::HAS_FN`], [`meta::HAS_REF`]) is walked, each distinct part
    /// once. The checker applies it to a finished body and the driver to the
    /// declarations the backend reads; every reference type stays, its kind
    /// included (`ref.type.kind-distinct`).
    #[must_use]
    pub fn erase_fn_deps(self, t: Ty) -> Ty {
        Erase::Deps.ty(self, t)
    }

    /// [`Self::erase_fn_deps`] over a list.
    #[must_use]
    pub fn erase_fn_deps_list(self, l: TyList) -> TyList {
        Erase::Deps.list(self, l)
    }

    /// `t` with every counted reference made local, permission kept, and
    /// its dependency maps erased: two types that differ only by reference
    /// kinds, as a counted argument that lends itself to a local reference
    /// parameter does (`ref.convert.lend`), erase to one. Only a type with a
    /// reference ([`meta::HAS_REF`]) is walked, each distinct part once.
    #[must_use]
    pub fn erase_ref_kinds(self, t: Ty) -> Ty {
        Erase::Kinds.ty(self, t)
    }

    /// Replaces declared parameters: `f` maps a parameter to its argument,
    /// or `None` to keep it.
    ///
    /// Each distinct node is substituted once per call (memoized by type
    /// id, as `f` is a function of the parameter alone), so the work is the
    /// type's distinct nodes with a parameter, not its expansion as a tree:
    /// `(T, T)` nested n deep is n + 1 nodes, not 2^n leaves (#1886).
    pub fn subst(self, t: Ty, f: &dyn Fn(ParamRef) -> Option<Ty>) -> Ty {
        if !self.has_param(t) {
            return t;
        }
        self.subst_in(t, f, &mut SubstMemo::new())
    }

    fn subst_in(self, t: Ty, f: &dyn Fn(ParamRef) -> Option<Ty>, memo: &mut SubstMemo) -> Ty {
        if !self.has_param(t) {
            return t;
        }
        if let TyData::Param(p) = self.get(t) {
            return f(p).unwrap_or(t);
        }
        if let Some(done) = memo.get(t) {
            return done;
        }
        crate::work::type_node();
        let out = self.subst_node(t, f, memo);
        memo.insert(t, out);
        out
    }

    fn subst_node(self, t: Ty, f: &dyn Fn(ParamRef) -> Option<Ty>, memo: &mut SubstMemo) -> Ty {
        let l = |x: TyList, memo: &mut SubstMemo| {
            let items: Vec<Ty> = self
                .list_items(x)
                .iter()
                .map(|&e| self.subst_in(e, f, memo))
                .collect();
            self.list(&items)
        };
        let d = match self.get(t) {
            TyData::Opaque {
                owner,
                site,
                borrows,
                args,
                bound,
            } => TyData::Opaque {
                owner,
                site,
                borrows,
                args: l(args, memo),
                bound: self.subst_in(bound, f, memo),
            },
            TyData::Adt(def, args) => TyData::Adt(def, l(args, memo)),

            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: self.subst_in(inner, f, memo),
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => {
                let (ps, r) = (l(params, memo), self.subst_in(result, f, memo));
                TyData::Fn {
                    params: ps,
                    result: r,
                    row: self.subst_row_in(row, f, memo),
                    suspends,
                    inputs,
                    deps: self.instance_deps(
                        (self.list_items(params), result, deps),
                        (self.list_items(ps), r),
                    ),
                }
            }
            TyData::Row(r) => TyData::Row(self.subst_row_in(r, f, memo)),
            TyData::Context(r) => TyData::Context(self.subst_row_in(r, f, memo)),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => TyData::TraitValue {
                def,
                args: l(args, memo),
                bindings: bindings
                    .into_iter()
                    .map(|(d, b)| (d, self.subst_in(b, f, memo)))
                    .collect(),
            },
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => TyData::Assoc {
                assoc,
                trait_,
                self_ty: self.subst_in(self_ty, f, memo),
                args: l(args, memo),
            },
            other => other,
        };
        self.intern_ty(&d)
    }

    /// Replaces declared parameters in a row: a row parameter maps through
    /// `f` like a type parameter of the same owner and index, and its
    /// argument (a `TyData::Row`, or a row variable) joins the keys.
    pub fn subst_row(self, r: RowId, f: &dyn Fn(ParamRef) -> Option<Ty>) -> RowId {
        if self.row_meta(r) & meta::HAS_PARAM == 0 {
            return r;
        }
        self.subst_row_in(r, f, &mut SubstMemo::new())
    }

    fn subst_row_in(
        self,
        r: RowId,
        f: &dyn Fn(ParamRef) -> Option<Ty>,
        memo: &mut SubstMemo,
    ) -> RowId {
        if self.row_meta(r) & meta::HAS_PARAM == 0 {
            return r;
        }
        let d = self.row_data(r);
        let mut out = RowData {
            keys: d.keys.iter().map(|k| self.subst_in(*k, f, memo)).collect(),
            params: Vec::new(),
        };
        for p in d.params {
            match f(ParamRef {
                owner: p.owner,
                index: p.index,
            }) {
                Some(t) => out.keys.push(t),
                None => out.params.push(p),
            }
        }
        self.row(&out)
    }
}

/// One `subst` call's results by type id: the first few in a fixed array,
/// scanned (a substitution through a small type allocates nothing), then
/// a hash map once they overflow it.
struct SubstMemo {
    small: [(Ty, Ty); 8],
    len: usize,
    big: HashMap<Ty, Ty>,
}

impl SubstMemo {
    fn new() -> Self {
        Self {
            small: [(Ty(0), Ty(0)); 8],
            len: 0,
            big: HashMap::default(),
        }
    }

    fn get(&self, t: Ty) -> Option<Ty> {
        if self.big.is_empty() {
            self.small[..self.len]
                .iter()
                .find(|&&(k, _)| k == t)
                .map(|&(_, v)| v)
        } else {
            self.big.get(&t).copied()
        }
    }

    fn insert(&mut self, t: Ty, out: Ty) {
        if self.len < self.small.len() {
            self.small[self.len] = (t, out);
            self.len += 1;
        } else {
            if self.big.is_empty() {
                self.big.extend(self.small.iter().copied());
            }
            self.big.insert(t, out);
        }
    }
}

/// A projection written `Self::Out` in a trait names the trait's own
/// arguments implicitly: fills them in from the call's.
#[must_use]
pub fn with_assoc_args(pool: Types<'_>, t: Ty, trait_: DefId, args: TyList) -> Ty {
    if args == TyList::EMPTY || !pool.has_assoc(t) {
        return t;
    }
    let mut memo = HashMap::default();
    let mut children = Vec::new();
    hd_base::stack::all((t, false), |(t, ready), stack| {
        if !pool.has_assoc(t) || memo.contains_key(&t) {
            return true;
        }
        if !ready {
            children.clear();
            // Preserve this substitution's boundaries: tuple rests and
            // function rows stay as written, as do opaque/trait-value types.
            match pool.get(t) {
                TyData::Fn { params, result, .. } => {
                    children.extend_from_slice(pool.list_items(params));
                    children.push(result);
                }
                TyData::Opaque { .. }
                | TyData::TraitValue { .. }
                | TyData::Row(_)
                | TyData::Context(_) => return true,
                _ => pool.children(t, &mut children),
            }
            stack.push((t, true));
            stack.extend(children.iter().rev().map(|&t| (t, false)));
            return true;
        }
        crate::work::type_node();
        let mapped = |t| memo.get(&t).copied().unwrap_or(t);
        let list = |l| {
            pool.list(
                &pool
                    .list_items(l)
                    .iter()
                    .map(|&t| mapped(t))
                    .collect::<Vec<_>>(),
            )
        };
        let d = match pool.get(t) {
            TyData::Assoc {
                assoc,
                trait_: tr,
                self_ty,
                args: a,
            } => TyData::Assoc {
                assoc,
                trait_: tr,
                self_ty: mapped(self_ty),
                args: if tr == trait_ && a == TyList::EMPTY {
                    args
                } else {
                    list(a)
                },
            },
            TyData::Adt(def, a) => TyData::Adt(def, list(a)),

            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: mapped(inner),
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => TyData::Fn {
                params: list(params),
                result: mapped(result),
                row,
                suspends,
                inputs,
                deps,
            },
            _ => return true,
        };
        memo.insert(t, pool.intern_ty(&d));
        true
    });
    memo.get(&t).copied().unwrap_or(t)
}

/// What [`Types::erase_fn_deps`] and [`Types::erase_ref_kinds`] erase:
/// the walk is one, and each distinct part of a type is visited once, since
/// a type built by doubling shares its parts and a walk without the memo
/// would be exponential in its depth.
#[derive(Clone, Copy)]
enum Erase {
    /// Function types' dependency maps.
    Deps,
    /// Those, and each counted reference's kind.
    Kinds,
}

impl Erase {
    /// Whether a part with these meta flags may hold something to erase.
    const fn holds(self, m: u32) -> bool {
        match self {
            Erase::Deps => m & (meta::HAS_FN | meta::HAS_REF) == meta::HAS_FN | meta::HAS_REF,
            Erase::Kinds => m & meta::HAS_REF != 0,
        }
    }

    fn ty(self, p: Types<'_>, t: Ty) -> Ty {
        if !self.holds(p.meta_of(t)) {
            return t;
        }
        self.ty_in(p, t, &mut hd_base::HashMap::default())
    }

    fn list(self, p: Types<'_>, l: TyList) -> TyList {
        if !self.holds(p.list_meta(l)) {
            return l;
        }
        self.list_in(p, l, &mut hd_base::HashMap::default())
    }

    fn list_in(self, p: Types<'_>, l: TyList, memo: &mut hd_base::HashMap<Ty, Ty>) -> TyList {
        if !self.holds(p.list_meta(l)) {
            return l;
        }
        let items: Vec<Ty> = p
            .list_items(l)
            .iter()
            .map(|&e| self.ty_in(p, e, memo))
            .collect();
        p.list(&items)
    }

    fn ty_in(self, p: Types<'_>, t: Ty, memo: &mut hd_base::HashMap<Ty, Ty>) -> Ty {
        if !self.holds(p.meta_of(t)) {
            return t;
        }
        if let Some(&r) = memo.get(&t) {
            return r;
        }
        let mut children = Vec::new();
        hd_base::stack::all((t, false), |(t, ready), stack| {
            if !self.holds(p.meta_of(t)) || memo.contains_key(&t) {
                return true;
            }
            if !ready {
                stack.push((t, true));
                children.clear();
                p.children(t, &mut children);
                stack.extend(children.iter().rev().map(|&t| (t, false)));
                return true;
            }
            crate::work::type_node();
            let mapped = |t| memo.get(&t).copied().unwrap_or(t);
            let list = |l| {
                p.list(
                    &p.list_items(l)
                        .iter()
                        .map(|&t| mapped(t))
                        .collect::<Vec<_>>(),
                )
            };
            let map_row = |r| {
                if !self.holds(p.row_meta(r)) {
                    return r;
                }
                let d = p.row_data(r);
                p.row(&RowData {
                    keys: d.keys.into_iter().map(mapped).collect(),
                    params: d.params,
                })
            };
            let d = match p.get(t) {
                TyData::Ref {
                    inner,
                    mutable,
                    counted,
                } => TyData::Ref {
                    inner: mapped(inner),
                    mutable,
                    counted: counted && matches!(self, Erase::Deps),
                },
                TyData::Adt(def, args) => TyData::Adt(def, list(args)),
                TyData::Opaque {
                    owner,
                    site,
                    borrows,
                    args,
                    bound,
                } => TyData::Opaque {
                    owner,
                    site,
                    borrows,
                    args: list(args),
                    bound: mapped(bound),
                },

                TyData::Fn {
                    params,
                    result,
                    row,
                    suspends,
                    inputs,
                    deps: _,
                } => TyData::Fn {
                    params: list(params),
                    result: mapped(result),
                    row: map_row(row),
                    suspends,
                    inputs,
                    deps: FnDeps::NONE,
                },
                TyData::Row(r) => TyData::Row(map_row(r)),
                TyData::Context(r) => TyData::Context(map_row(r)),
                TyData::TraitValue {
                    def,
                    args,
                    bindings,
                } => TyData::TraitValue {
                    def,
                    args: list(args),
                    bindings: bindings.into_iter().map(|(d, b)| (d, mapped(b))).collect(),
                },
                TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty,
                    args,
                } => TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: mapped(self_ty),
                    args: list(args),
                },
                // A leaf holds nothing to erase.
                TyData::Prim(_)
                | TyData::Never
                | TyData::Poison
                | TyData::Param(_)
                | TyData::Const(_)
                | TyData::Infer(_)
                | TyData::Canon(_) => return true,
            };
            memo.insert(t, p.intern_ty(&d));
            true
        });
        memo.get(&t).copied().unwrap_or(t)
    }
}

#[cfg(test)]
mod tests {
    use super::{ANON_SUM, AdtId, PoolTag};
    /// #2024: `type R0 = &T`, `type R{i} = (R{i-1}, R{i-1})`, and a
    /// function type over each level. Each count is read from the item's
    /// own word, set once when the item is interned from its parts' words,
    /// so building and asking the chain costs pool items linear in its
    /// levels, where the walk took a step per path (`2^n`).
    #[test]
    fn slot_counts_of_a_doubled_chain_cost_its_distinct_nodes() {
        use super::{FnDeps, Inputs, InternPool, RowId, Ty, TyData};
        let chain = |n: u32| {
            let pool = InternPool::new();
            let before = pool.item_count();
            let mut r = pool.intern_ty(&TyData::Ref {
                inner: Ty::I32,
                mutable: false,
                counted: false,
            });
            let mut f = Ty::I32;
            for _ in 0..n {
                r = pool.product(pool.list(&[r, r]), None);
                let g = pool.intern_ty(&TyData::Fn {
                    params: pool.list(&[r]),
                    result: Ty::I32,
                    row: RowId::EMPTY,
                    suspends: false,
                    inputs: Inputs::Fixed,
                    deps: FnDeps::NONE,
                });
                f = pool.product(pool.list(&[f, g]), None);
            }
            let t = pool.types();
            (
                t.ref_slots(r),
                t.fn_written_slots(f),
                pool.item_count() - before,
            )
        };
        let (refs, written, small) = chain(20);
        assert_eq!(refs, 1 << 20);
        // Level i's function type writes the 2^i slots of its input.
        assert_eq!(written, (1 << 21) - 2);
        let (refs, written, large) = chain(200);
        assert_eq!(
            (refs, written),
            (u32::MAX as usize, u32::MAX as usize),
            "saturated"
        );
        assert!(
            large <= 10 * small + 10,
            "pool items for 20 and 200 levels: {small} and {large}"
        );
    }

    /// Each form's counts, as the walks over paths gave them: a counted
    /// reference adds no slot, a borrowing opaque type one, a union's
    /// members' slots count and its function types write none, and a
    /// function type's own slots are outside its enclosing type's.
    #[test]
    fn slot_counts_follow_each_type_form() {
        use super::{FnDeps, Inputs, InternPool, RowId, Ty, TyData};
        let pool = InternPool::new();
        let t = pool.types();
        let r = |inner, counted| {
            pool.intern_ty(&TyData::Ref {
                inner,
                mutable: true,
                counted,
            })
        };
        let local = r(Ty::I32, false);
        let counted = r(local, true);
        assert_eq!((t.ref_slots(local), t.ref_slots(counted)), (1, 1));
        let opaque = |borrows| {
            pool.intern_ty(&TyData::Opaque {
                owner: hd_base::DefId::from_raw(5),
                site: 0,
                borrows,
                args: pool.list(&[local]),
                bound: Ty::I32,
            })
        };
        assert_eq!(
            (t.ref_slots(opaque(true)), t.ref_slots(opaque(false))),
            (1, 0)
        );
        let f = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[local, counted]),
            result: local,
            row: RowId::EMPTY,
            suspends: false,
            inputs: Inputs::Fixed,
            deps: FnDeps::NONE,
        });
        assert_eq!((t.ref_slots(f), t.fn_written_slots(f)), (0, 3));
        let union = pool.intern_ty(&TyData::sum(pool.list(&[local, f])));
        assert_eq!((t.ref_slots(union), t.fn_written_slots(union)), (1, 0));
        let held = pool.product(pool.list(&[f, local]), Some(r(f, false)));
        assert_eq!((t.ref_slots(held), t.fn_written_slots(held)), (2, 6));
        let adt = pool.intern_ty(&TyData::Adt(
            AdtId::Declared(hd_base::DefId::from_raw(9)),
            pool.list(&[held, union]),
        ));
        assert_eq!((t.ref_slots(adt), t.fn_written_slots(adt)), (3, 6));
    }

    #[test]
    fn dependency_canonicalization_preserves_order_with_linear_work() {
        for width in [100, 1000] {
            for depth in [1, 20] {
                let slots = width * depth;
                let inputs: Vec<u32> = (1..=slots).collect();
                let outputs: Vec<u32> = inputs.iter().rev().copied().collect();
                let (words, work) = super::Types::dependency_words(&inputs, &outputs);
                assert_eq!(work, 3 * slots as usize);
                assert_eq!(&words[2..2 + slots as usize], &inputs);
                assert_eq!(&words[2 + slots as usize..], &outputs);
            }
        }
        let (words, work) = super::Types::dependency_words(&[7, 9, 7, 0], &[7, 0, 13]);
        assert_eq!(words, [4, 3, 1, 2, 1, 3, 1, 0, 0]);
        assert_eq!(work, 10);
        let pool = super::InternPool::new();
        assert_eq!(
            pool.types().fn_deps(&[7, 9, 7, 0], &[7, 0, 13]),
            pool.types().fn_deps(&[41, 73, 41, 0], &[41, 0, 99]),
            "renaming origins keeps the same interned dependency map"
        );
    }

    #[test]
    fn opaque_identity_substitutes_args_without_revealing_a_body() {
        let pool = super::InternPool::new();
        let owner = hd_base::DefId::from_raw(71);
        let parameter = super::ParamRef { owner, index: 0 };
        let t = pool.intern_ty(&super::TyData::Param(parameter));
        let bound = pool.intern_ty(&super::TyData::TraitValue {
            def: hd_base::DefId::from_raw(73),
            args: pool.list(&[t]),
            bindings: vec![],
        });
        let make = |owner, site, arg, bound| {
            pool.intern_ty(&super::TyData::Opaque {
                owner,
                site,
                borrows: false,
                args: pool.list(&[arg]),
                bound,
            })
        };
        let opaque = make(owner, 0, t, bound);
        let applied = pool.subst(opaque, &|p| (p == parameter).then_some(super::Ty::I32));
        let concrete_bound = pool.subst(bound, &|p| (p == parameter).then_some(super::Ty::I32));
        assert_eq!(applied, make(owner, 0, super::Ty::I32, concrete_bound));
        assert_ne!(applied, make(owner, 1, super::Ty::I32, concrete_bound));
        assert_ne!(
            applied,
            make(
                hd_base::DefId::from_raw(72),
                0,
                super::Ty::I32,
                concrete_bound
            )
        );
        assert_ne!(applied, make(owner, 0, super::Ty::STRING, concrete_bound));
        assert!(pool.has_opaque(applied));
        assert!(!pool.has_param(applied));
        assert_ne!(applied, super::Ty::I32);
    }

    use super::{
        FnDeps, Inputs, InternPool, LocalPool, ParamRef, Prim, RowData, RowId, Ty, TyData, TyList,
        Types,
    };
    use hd_base::DefId;

    #[test]
    fn head_readers_preserve_arguments_and_associated_bindings() {
        let g = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&g, &local);
        let var = p.intern_ty(&TyData::Infer(hd_base::InferVar::from_raw(3)));
        let def = DefId::from_raw(7);
        let assoc = DefId::from_raw(9);
        for inner in [Ty::I32, var] {
            let args = p.list(&[inner]);
            let adt_form = TyData::Adt(AdtId::Declared(def), args);
            let trait_form = TyData::TraitValue {
                def,
                args,
                bindings: vec![(assoc, inner)],
            };
            let adt = p.intern_ty(&adt_form);
            let value = p.intern_ty(&trait_form);
            assert_eq!(adt.is_local(), inner.is_local());
            assert_eq!(value.is_local(), inner.is_local());
            assert_eq!(p.adt(adt), Some((def, args)));
            assert_eq!(p.trait_value(adt), None);
            assert_eq!(p.trait_value(value), Some((def, args)));
            assert_eq!(p.adt(value), None);
            if !inner.is_local() {
                assert_eq!(g.adt(adt), Some((def, args)));
                assert_eq!(g.trait_value(value), Some((def, args)));
            }
            assert_eq!(p.list_items(args), &[inner]);
            assert_eq!(p.get(adt), adt_form);
            assert_eq!(p.get(value), trait_form, "associated bindings are retained");
        }
        // A local binding can make the record local while its arguments stay global.
        let args = p.list(&[Ty::I32]);
        let form = TyData::TraitValue {
            def,
            args,
            bindings: vec![(assoc, var)],
        };
        let value = p.intern_ty(&form);
        assert!(value.is_local() && !Ty(args.0).is_local());
        assert_eq!(p.trait_value(value), Some((def, args)));
        assert_eq!(p.list_items(args), &[Ty::I32]);
        assert_eq!(p.get(value), form, "local associated binding is retained");
    }

    #[test]
    fn checked_parts_read_global_and_local_records() {
        let g = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&g, &local);
        let variable = p.intern_ty(&TyData::Infer(hd_base::InferVar::from_raw(3)));
        let def = DefId::from_raw(7);
        for item in [Ty::I32, variable] {
            for mutable in [false, true] {
                for counted in [false, true] {
                    let reference = p.intern_ty(&TyData::Ref {
                        inner: item,
                        mutable,
                        counted,
                    });
                    assert_eq!(p.ref_inner(reference), item);
                    if !reference.is_local() {
                        assert_eq!(g.ref_inner(reference), item);
                    }
                }
            }
            let args = p.list(&[item]);
            let list = p.intern_ty(&TyData::Adt(AdtId::Declared(def), args));
            assert_eq!(p.adt_parts(list), (def, args));
            for rest in [None, Some(list)] {
                let tuple = p.product(args, rest);
                assert_eq!(p.product_parts(tuple), Some((p.list_items(args), rest)));
                if !tuple.is_local() {
                    assert_eq!(g.product_parts(tuple), Some((g.list_items(args), rest)));
                    assert_eq!(g.adt_parts(list), (def, args));
                }
            }
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn checked_parts_assert_the_constructor() {
        let g = InternPool::new();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.adt_parts(Ty::I32)))
                .is_err()
        );
        assert_eq!(g.product_parts(Ty::I32), None);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.ref_inner(Ty::I32)))
                .is_err()
        );
    }

    #[test]
    fn head_readers_do_not_unwrap_other_type_forms() {
        let g = InternPool::new();
        let p = g.types();
        let def = DefId::from_raw(7);
        let args = p.list(&[Ty::I32]);
        let adt = p.intern_ty(&TyData::Adt(AdtId::Declared(def), args));
        let value = p.intern_ty(&TyData::TraitValue {
            def,
            args,
            bindings: vec![],
        });
        for inner in [adt, value] {
            for form in [
                TyData::Ref {
                    inner,
                    mutable: false,
                    counted: false,
                },
                TyData::Ref {
                    inner,
                    mutable: true,
                    counted: false,
                },
                TyData::Adt(AdtId::Product { rest: false }, args),
                TyData::Adt(
                    AdtId::Product { rest: true },
                    p.list(
                        &p.list_items(args)
                            .iter()
                            .copied()
                            .chain([inner])
                            .collect::<Vec<_>>(),
                    ),
                ),
            ] {
                let t = p.intern_ty(&form);
                assert_eq!(p.adt(t), None);
                assert_eq!(p.trait_value(t), None);
            }
        }
        for t in [Ty::I32, Ty::NEVER, Ty::POISON, Ty::VOID] {
            assert_eq!(p.adt(t), None);
            assert_eq!(p.trait_value(t), None);
        }
    }

    #[test]
    fn head_readers_reject_foreign_body_even_for_a_wrong_tag() {
        let g = InternPool::new();
        let local = LocalPool::new();
        // Avoid an incidental four-bit stamp collision with concurrent tests.
        let other = loop {
            let candidate = LocalPool::new();
            if candidate.generation != local.generation {
                break candidate;
            }
        };
        let p = Types::with_local(&g, &local);
        let t = p.intern_ty(&TyData::Infer(hd_base::InferVar::from_raw(3)));
        let foreign = Types::with_local(&g, &other);
        let other_var = foreign.intern_ty(&TyData::Infer(hd_base::InferVar::from_raw(3)));
        assert_eq!(t.0 & super::LOCAL_ROW, other_var.0 & super::LOCAL_ROW);
        assert_ne!(t, other_var, "the bodies have distinct generations");
        let rejects = |read: &dyn Fn(), why: &str| {
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read))
                .expect_err("a body-local type must be read only in its body");
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .expect("ownership panic has a string message");
            assert!(message.contains(why), "unexpected panic: {message}");
        };
        let another = "a body-local type read in another body";
        rejects(
            &|| {
                let _ = foreign.adt(t);
            },
            another,
        );
        rejects(
            &|| {
                let _ = foreign.trait_value(t);
            },
            another,
        );
        let outside = "a body-local type read outside its body";
        rejects(
            &|| {
                let _ = g.types().adt(t);
            },
            outside,
        );
        rejects(
            &|| {
                let _ = g.types().trait_value(t);
            },
            outside,
        );
    }

    #[test]
    fn preseeded_constants() {
        let p = InternPool::new();
        assert_eq!(p.get(Ty::I32), TyData::Prim(Prim::I32));
        assert_eq!(p.get(Ty::NEVER), TyData::Never);
        assert_eq!(p.get(Ty::POISON), TyData::Poison);
        assert_eq!(p.list(&[]), TyList::EMPTY);
        assert_eq!(p.row(&RowData::default()), RowId::EMPTY);
        assert_eq!(p.product(TyList::EMPTY, None), Ty::VOID);
    }

    /// Threads racing on the same content get one item, through the
    /// shards and each thread's read-through table; two pools on one
    /// thread never share a read-through hit.
    #[test]
    fn concurrent_interning_dedups() {
        let p = InternPool::new();
        let ids: Vec<Vec<TyList>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..4)
                .map(|_| {
                    s.spawn(|| {
                        (0..2000u32)
                            .map(|n| {
                                let o = p.intern_ty(&TyData::Ref {
                                    inner: Ty::prim(Prim::ALL[(n % 14) as usize]),
                                    mutable: false,
                                    counted: false,
                                });
                                p.list(&[o, Ty::prim(Prim::ALL[(n / 14 % 14) as usize])])
                            })
                            .collect()
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("join")).collect()
        });
        assert!(ids.windows(2).all(|w| w[0] == w[1]));
        let q = InternPool::new();
        let a = q.list(&[Ty::BOOL, Ty::I32]);
        let b = p.list(&[Ty::BOOL, Ty::I32]);
        assert_eq!(q.list_items(a), p.list_items(b));
    }

    #[test]
    fn every_form_round_trips_and_dedups() {
        let p = InternPool::new();
        let list = p.list(&[Ty::I32, Ty::STRING]);
        let def = DefId::from_raw(7);
        let row = p.row(&RowData {
            keys: vec![Ty::STRING, Ty::I32],
            params: vec![],
        });
        let forms = [
            TyData::Adt(AdtId::Declared(def), list),
            TyData::Adt(AdtId::Product { rest: false }, list),
            TyData::Adt(
                AdtId::Product { rest: true },
                p.list(
                    &p.list_items(list)
                        .iter()
                        .copied()
                        .chain([Ty::BOOL])
                        .collect::<Vec<_>>(),
                ),
            ),
            TyData::sum(list),
            TyData::Fn {
                params: list,
                result: Ty::VOID,
                row,
                suspends: true,
                inputs: Inputs::Fixed,
                deps: crate::pool::FnDeps::NONE,
            },
            TyData::Fn {
                params: list,
                result: Ty::VOID,
                row,
                suspends: false,
                inputs: Inputs::Rest,
                deps: crate::pool::FnDeps::NONE,
            },
            TyData::TraitValue {
                def,
                args: TyList::EMPTY,
                bindings: vec![(DefId::from_raw(9), Ty::I32)],
            },
            TyData::Param(super::ParamRef {
                owner: def,
                index: 1,
            }),
            TyData::Assoc {
                assoc: DefId::from_raw(3),
                trait_: def,
                self_ty: Ty::I32,
                args: list,
            },
            TyData::Ref {
                inner: Ty::STRING,
                mutable: false,
                counted: false,
            },
            TyData::Ref {
                inner: Ty::I32,
                mutable: true,
                counted: false,
            },
            TyData::Canon(2),
        ];
        for f in forms {
            let t = p.intern_ty(&f);
            assert_eq!(p.get(t), f);
            assert_eq!(p.intern_ty(&f), t, "hash-consed");
        }
        assert_eq!(p.row_data(row).keys.len(), 2);
    }

    #[test]
    fn products_share_adt_identity_and_borrow_the_rest_without_a_prefix() {
        let p = InternPool::new();
        let fixed = p.list(&[Ty::I32, Ty::STRING]);
        let product = p.product(fixed, None);
        let sum = p.intern_ty(&TyData::sum(fixed));
        let rest = p.intern_ty(&TyData::Adt(AdtId::Product { rest: true }, fixed));
        assert_ne!(product, sum);
        assert_ne!(product, rest);
        assert_eq!(p.product(fixed, None), product);
        assert_eq!(
            p.product_parts(product),
            Some((&[Ty::I32, Ty::STRING][..], None))
        );
        assert_eq!(
            p.product_parts(rest),
            Some((&[Ty::I32][..], Some(Ty::STRING)))
        );
        assert_eq!(p.adt(product), None);
        assert_eq!(p.adt(sum), None);
        assert_eq!(p.product_parts(sum), None);
        let before = p.cols.tag.len();
        for _ in 0..1000 {
            assert_eq!(
                p.product_parts(rest),
                Some((&[Ty::I32][..], Some(Ty::STRING)))
            );
        }
        assert_eq!(p.cols.tag.len(), before);
        assert_eq!(p.product(p.list(&[Ty::I32]), Some(Ty::STRING)), rest);
        assert_eq!(std::mem::size_of::<PoolTag>(), 2);
    }

    #[test]
    fn union_identity_keeps_order_duplicates_nesting_and_substitution() {
        let p = InternPool::new();
        let union = |members: &[Ty]| p.intern_ty(&TyData::sum(p.list(members)));
        let ab = union(&[Ty::I32, Ty::STRING]);
        assert!(matches!(p.get(ab), TyData::Adt(ANON_SUM, _)));
        assert!(p.adt(ab).is_none());
        let ba = union(&[Ty::STRING, Ty::I32]);
        let duplicate = union(&[Ty::I32, Ty::I32]);
        let flat = union(&[Ty::I32, Ty::STRING, Ty::BOOL]);
        let nested_left = union(&[ab, Ty::BOOL]);
        let nested_right = union(&[Ty::I32, union(&[Ty::STRING, Ty::BOOL])]);

        assert_ne!(ab, ba);
        assert_ne!(duplicate, Ty::I32);
        assert_ne!(flat, nested_left);
        assert_ne!(flat, nested_right);
        assert_ne!(nested_left, nested_right);

        let owner = DefId::from_raw(81);
        let parameter = super::ParamRef { owner, index: 0 };
        let generic = union(&[
            p.intern_ty(&TyData::Param(parameter)),
            p.intern_ty(&TyData::Param(parameter)),
        ]);
        assert_eq!(
            p.subst(generic, &|p| (p == parameter).then_some(Ty::I32)),
            duplicate
        );
    }

    /// Rows are sets (`req.row.set.union`, `req.row.set.duplicate`,
    /// `req.row.set.parameter`): key order and repeats do not change the
    /// interned row, a solved row key flattens into its keys, and a
    /// different key set is a different row.
    #[test]
    fn rows_intern_as_sets() {
        let p = InternPool::new();
        let r = super::RowParamRef {
            owner: DefId::from_raw(4),
            index: 0,
        };
        let row = |keys: &[Ty], params: &[super::RowParamRef]| {
            p.row(&RowData {
                keys: keys.to_vec(),
                params: params.to_vec(),
            })
        };
        let ab = row(&[Ty::STRING, Ty::I32], &[r]);
        assert_eq!(row(&[Ty::I32, Ty::STRING], &[r]), ab);
        assert_eq!(row(&[Ty::I32, Ty::STRING, Ty::I32], &[r, r]), ab);
        let inner = p.intern_ty(&TyData::Row(row(&[Ty::I32], &[r])));
        assert_eq!(row(&[Ty::STRING, inner, Ty::I32], &[]), ab);
        assert_ne!(row(&[Ty::I32], &[r]), ab, "a missing key is another row");
        assert_ne!(row(&[Ty::I32, Ty::STRING], &[]), ab, "so is a missing R");
    }

    /// `Fn[Args, O, $()]` keeps `Args` as its one input until `Args` is a
    /// tuple, which then gives the plain or rest form.
    #[test]
    fn tuple_inputs_take_the_plain_form() {
        let p = InternPool::new();
        let param = p.intern_ty(&TyData::Param(super::ParamRef {
            owner: DefId::from_raw(7),
            index: 0,
        }));
        let of = |input: Ty| {
            p.intern_ty(&TyData::Fn {
                params: p.list(&[input]),
                result: Ty::I32,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Tuple,
                deps: crate::pool::FnDeps::NONE,
            })
        };
        let generic = of(param);
        assert!(matches!(
            p.get(generic),
            TyData::Fn {
                inputs: Inputs::Tuple,
                ..
            }
        ));
        let pair = p.list(&[Ty::I32, Ty::STRING]);
        let plain = p.intern_ty(&TyData::Fn {
            params: pair,
            result: Ty::I32,
            row: RowId::EMPTY,
            suspends: false,
            inputs: Inputs::Fixed,
            deps: crate::pool::FnDeps::NONE,
        });
        let tuple = p.product(pair, None);
        assert_eq!(of(tuple), plain);
        assert_eq!(p.subst(generic, &|_| Some(tuple)), plain);
        assert_eq!(p.types().inputs_tuple(pair, Inputs::Fixed), tuple);
        let rest = p.product(p.list(&[Ty::I32]), Some(Ty::STRING));
        let TyData::Fn { params, inputs, .. } = p.get(of(rest)) else {
            panic!("a function type");
        };
        assert_eq!((params, inputs), (pair, Inputs::Rest));
        assert_eq!(p.types().inputs_tuple(params, inputs), rest);
        let unit = p.product(TyList::EMPTY, None);
        assert!(matches!(
            p.get(of(unit)),
            TyData::Fn {
                params: TyList::EMPTY,
                inputs: Inputs::Fixed,
                ..
            }
        ));
        assert_eq!(of(Ty::VOID), of(unit), "`void` is `()`");
    }

    /// `&T`: a one-child type for the tests below.
    fn shared(inner: Ty) -> TyData {
        TyData::Ref {
            inner,
            mutable: false,
            counted: false,
        }
    }

    #[test]
    fn poison_marks_its_holders() {
        let p = InternPool::new();
        let opt = p.intern_ty(&shared(Ty::POISON));
        assert!(p.has_poison(opt));
    }

    /// Variable-holding types go to the body's local pool and never grow
    /// the global one; variable-free types built through the same view are
    /// global.
    #[test]
    fn variables_stay_body_local() {
        let g = InternPool::new();
        let before = g.item_count();
        let local = LocalPool::new();
        let p = Types::with_local(&g, &local);
        let v = p.intern_ty(&TyData::Infer(hd_base::InferVar::from_raw(3)));
        let opt = p.intern_ty(&shared(v));
        let l = p.list(&[opt, Ty::I32]);
        assert!(v.is_local() && opt.is_local() && Ty(l.0).is_local());
        assert_eq!(p.get(opt), shared(v));
        assert_eq!(p.list_items(l), &[opt, Ty::I32]);
        assert_eq!(p.intern_ty(&shared(v)), opt, "hash-consed locally");
        assert!(p.has_infer(opt));
        let plain = p.intern_ty(&shared(Ty::I32));
        assert!(!plain.is_local());
        assert_eq!(
            g.item_count(),
            before + 1,
            "only the variable-free type is global"
        );
        assert_eq!(local.len(), 3);
    }
    /// `fn(&'a i32, &i32) -> T` keeps which inputs write a lifetime, and
    /// an instance whose result holds a reference depends on those
    /// (`fn.type.lifetime.result.param`); with no written input, or
    /// inputs that no longer line up, it depends on none.
    #[test]
    fn an_open_map_gives_an_instance_result_the_written_inputs() {
        let pool = InternPool::new();
        let types = pool.types();
        let r = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        let t = pool.intern_ty(&TyData::Param(ParamRef {
            owner: DefId::from_raw(1),
            index: 0,
        }));
        let open = types.fn_deps_written(&[r, r], t, &[1, 0]);
        assert_eq!(types.deps_of(open), Some((&[1, 0][..], &[][..])));
        assert_eq!(types.fn_deps_written(&[r, r], t, &[]), FnDeps::NONE);
        assert_eq!(
            types.fn_deps_written(&[r, r], Ty::I32, &[1, 0]),
            FnDeps::NONE
        );
        let f = |params: &[Ty], result: Ty| {
            pool.intern_ty(&TyData::Fn {
                params: pool.list(params),
                result,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps: open,
            })
        };
        assert_eq!(
            types.fn_ty_deps(f(&[r, r], t)),
            Some((&[1, 0][..], &[][..]))
        );
        let at_first = types.fn_deps(&[1, 2], &[1]);
        assert_eq!(types.fn_ty_deps(f(&[r, r], r)), types.deps_of(at_first));
        let none = types.fn_deps(&[1, 2, 3], &[0]);
        assert_eq!(types.fn_ty_deps(f(&[r, r, r], r)), types.deps_of(none));
    }

    /// `fn(T) -> T` and `fn(T, U) -> T` at references: a result slot a
    /// type parameter fills depends on the input slots at the same
    /// position of the same parameter, and on no other
    /// (`fn.type.lifetime.result.param`), whether a substitution or an
    /// inference variable fills it.
    #[test]
    fn an_instance_result_shares_the_lifetimes_of_its_parameters_inputs() {
        let pool = InternPool::new();
        let types = pool.types();
        let r = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        let param = |index| {
            pool.intern_ty(&TyData::Param(ParamRef {
                owner: DefId::from_raw(1),
                index,
            }))
        };
        let (t, u) = (param(0), param(1));
        let f = |params: &[Ty], result: Ty| {
            pool.intern_ty(&TyData::Fn {
                params: pool.list(params),
                result,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps: FnDeps::NONE,
            })
        };
        let at = |x: Ty, y: Ty| move |p: ParamRef| Some(if p.index == 0 { x } else { y });
        let deps = |t| types.fn_ty_deps(t).map(|(i, o)| (i.to_vec(), o.to_vec()));
        let ident = types.subst(f(&[t], t), &at(r, r));
        assert_eq!(deps(ident), Some((vec![1], vec![1])));
        let first = types.subst(f(&[t, u], t), &at(r, r));
        assert_eq!(deps(first), Some((vec![1, 2], vec![1])));
        // A pair: each result position shares its own input position.
        let pair = pool.product(pool.list(&[r, r]), None);
        let both = types.subst(f(&[t], t), &at(pair, pair));
        assert_eq!(deps(both), Some((vec![1, 2], vec![1, 2])));
        // `U` only in the result has no source.
        let other = types.subst(f(&[t], u), &at(r, r));
        assert_eq!(deps(other), Some((vec![1], vec![0])));
    }

    /// [`Types::instance_deps`] over each shape a type parameter may sit
    /// in: under a local and a counted reference, in a tuple with a rest,
    /// in a generic instance, next to a part that changes but holds no
    /// reference, and with an open map; and the shapes it leaves the map
    /// of: an anonymous sum and a type deeper than `TYPE_DEPTH`.
    #[test]
    fn an_instance_map_follows_each_shape_a_type_parameter_fills() {
        let pool = InternPool::new();
        let types = pool.types();
        let rf = |inner, counted| {
            pool.intern_ty(&TyData::Ref {
                inner,
                mutable: false,
                counted,
            })
        };
        let r = rf(Ty::I32, false);
        let param = |index| {
            pool.intern_ty(&TyData::Param(ParamRef {
                owner: DefId::from_raw(1),
                index,
            }))
        };
        let (t, u) = (param(0), param(1));
        let f = |params: &[Ty], result: Ty, deps: FnDeps| {
            pool.intern_ty(&TyData::Fn {
                params: pool.list(params),
                result,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps,
            })
        };
        let plain = |params: &[Ty], result: Ty| f(params, result, FnDeps::NONE);
        let at = |x: Ty, y: Ty| move |p: ParamRef| Some(if p.index == 0 { x } else { y });
        let deps = |t| types.fn_ty_deps(t).map(|(i, o)| (i.to_vec(), o.to_vec()));
        let tuple = |elems: &[Ty], rest| pool.product(pool.list(elems), rest);
        // `fn(&T) -> &T` at `&i32`: the outer reference is the type's own
        // slot, whose map (none here) the instance keeps; the inner is
        // `T`'s, shared. `fn(*T) -> T` passes the counted reference by.
        let by_ref = types.subst(plain(&[rf(t, false)], rf(t, false)), &at(r, r));
        assert_eq!(deps(by_ref), Some((vec![1, 2], vec![0, 2])));
        let counted = types.subst(plain(&[rf(t, true)], t), &at(r, r));
        assert_eq!(deps(counted), Some((vec![1], vec![1])));
        // A tuple with a rest, and a generic instance.
        let list = |a| {
            pool.intern_ty(&TyData::Adt(
                AdtId::Declared(DefId::from_raw(5)),
                pool.list(&[a]),
            ))
        };
        let rest = types.subst(plain(&[tuple(&[t], Some(list(u)))], t), &at(r, r));
        assert_eq!(deps(rest), Some((vec![1, 2], vec![1])));
        let inst = types.subst(plain(&[list(t)], t), &at(r, r));
        assert_eq!(deps(inst), Some((vec![1], vec![1])));
        let trait_value = pool.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(6),
            args: pool.list(&[t]),
            bindings: Vec::new(),
        });
        let by_dyn = types.subst(plain(&[trait_value], t), &at(r, r));
        assert_eq!(deps(by_dyn), Some((vec![1], vec![1])));
        // `U` at `i32` changes a part that holds no reference.
        let beside = types.subst(plain(&[u, t], t), &at(r, Ty::I32));
        assert_eq!(deps(beside), Some((vec![1], vec![1])));
        // An open map, `fn(&'a i32, T) -> T`: the written input joins the
        // one hole the result takes. With `U` in the result too, which no
        // input fills, the map stays open.
        let open = types.fn_deps_written(&[r, t], t, &[1, 0]);
        let joined = types.subst(f(&[r, t], t, open), &at(r, r));
        assert_eq!(deps(joined), Some((vec![1, 1], vec![1])));
        let pair_out = tuple(&[t, u], None);
        let open2 = types.fn_deps_written(&[r, t], pair_out, &[1, 0]);
        let unjoined =
            types.instance_deps((&[r, t], pair_out, open2), (&[r, r], tuple(&[r, r], None)));
        assert_eq!(unjoined, open2);
        // A union in the inputs, or in the result, keeps the map.
        let union = pool.intern_ty(&TyData::sum(pool.list(&[t, rf(Ty::BOOL, false)])));
        let in_union = types.subst(plain(&[union], t), &at(r, r));
        assert_eq!(deps(in_union), None);
        let out_union = types.subst(plain(&[t], union), &at(r, r));
        assert_eq!(deps(out_union), None);
        // Past `TYPE_DEPTH`, the walk stops and keeps the map.
        let mut deep = (t, Ty::I32);
        for _ in 0..=hd_base::limits::TYPE_DEPTH {
            deep = (tuple(&[r, deep.0], None), tuple(&[r, deep.1], None));
        }
        let generic = plain(&[deep.0, u], u);
        let too_deep = types.subst(generic, &|p: ParamRef| {
            Some(if p.index == 0 { Ty::I32 } else { r })
        });
        assert_eq!(deps(too_deep), None);
    }

    /// [`Types::instance_deps`] lines a generic type up with its
    /// instance on its own stack: a type 1,000 levels deep, each level a
    /// local reference beside the next, maps on a 96 KiB thread.
    #[test]
    fn an_instance_map_at_the_type_depth_fits_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let types = pool.types();
                let r = pool.intern_ty(&TyData::Ref {
                    inner: Ty::I32,
                    mutable: false,
                    counted: false,
                });
                let t = pool.intern_ty(&TyData::Param(ParamRef {
                    owner: DefId::from_raw(1),
                    index: 0,
                }));
                let mut deep = (t, r);
                for _ in 1..hd_base::limits::TYPE_DEPTH {
                    let pair = |x| pool.product(pool.list(&[r, x]), None);
                    deep = (pair(deep.0), pair(deep.1));
                }
                let n = hd_base::limits::TYPE_DEPTH as usize;
                let open = types.fn_deps_written(&[deep.0], t, &[0]);
                let map = types.instance_deps((&[deep.0], t, open), (&[deep.1], r));
                let (ins, outs) = types.deps_of(map).expect("a map");
                assert_eq!((ins.len(), outs.len()), (n, 1));
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    #[test]
    fn trait_self_queries_at_depth_1000_fit_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let types = pool.types();
                let owner = DefId::from_raw(1);
                let param = |owner| pool.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
                let this = param(owner);
                let other = param(DefId::from_raw(2));
                let projection = |args| {
                    pool.intern_ty(&TyData::Assoc {
                        assoc: DefId::from_raw(3),
                        trait_: owner,
                        self_ty: this,
                        args,
                    })
                };
                let hidden = projection(TyList::EMPTY);
                let named = projection(pool.list(&[this]));
                for depth in [100, 1_000] {
                    for (leaf, expected) in
                        [(this, true), (other, false), (hidden, false), (named, true)]
                    {
                        let mut t = leaf;
                        for _ in 0..depth {
                            // Doubling the same child checks graph sharing too:
                            // the recursive search took 2^depth false paths.
                            t = pool.product(pool.list(&[t, t]), None);
                        }
                        let (found, work) =
                            crate::work::count_type_nodes(|| types.names_trait_self(t, owner));
                        assert_eq!(found, expected);
                        assert_eq!(work, depth + if leaf == named { 2 } else { 1 });
                    }
                }
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    #[test]
    fn associated_arguments_at_depth_1000_fit_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let types = pool.types();
                let owner = DefId::from_raw(1);
                let this = pool.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
                let args = pool.list(&[Ty::I32]);
                let projection = |trait_, args| {
                    pool.intern_ty(&TyData::Assoc {
                        assoc: DefId::from_raw(2),
                        trait_,
                        self_ty: this,
                        args,
                    })
                };
                let original = projection(owner, TyList::EMPTY);
                let filled = projection(owner, args);
                for depth in [100, 1_000] {
                    let mut chain = [original, filled];
                    for _ in 0..depth {
                        for t in &mut chain {
                            *t = pool.product(pool.list(&[*t, *t]), None);
                        }
                    }
                    let (out, work) = crate::work::count_type_nodes(|| {
                        super::with_assoc_args(types, chain[0], owner, args)
                    });
                    assert_eq!(out, chain[1]);
                    assert_eq!(work, depth + 1);
                    assert_eq!(
                        super::with_assoc_args(types, chain[0], owner, TyList::EMPTY),
                        chain[0]
                    );
                }
                let other = projection(DefId::from_raw(3), TyList::EMPTY);
                assert_eq!(super::with_assoc_args(types, other, owner, args), other);
                assert_eq!(
                    super::with_assoc_args(
                        types,
                        projection(owner, pool.list(&[original])),
                        owner,
                        args
                    ),
                    projection(owner, pool.list(&[filled]))
                );
                let row = pool.row(&RowData {
                    keys: vec![original],
                    params: vec![],
                });
                for t in [
                    pool.intern_ty(&TyData::Opaque {
                        owner,
                        site: 0,
                        borrows: false,
                        args: pool.list(&[original]),
                        bound: original,
                    }),
                    pool.intern_ty(&TyData::TraitValue {
                        def: owner,
                        args: pool.list(&[original]),
                        bindings: vec![(owner, original)],
                    }),
                    pool.intern_ty(&TyData::Row(row)),
                    pool.intern_ty(&TyData::Context(row)),
                    pool.product(TyList::EMPTY, Some(original)),
                    pool.intern_ty(&TyData::Fn {
                        params: TyList::EMPTY,
                        result: Ty::I32,
                        row,
                        suspends: false,
                        inputs: Inputs::Fixed,
                        deps: FnDeps::NONE,
                    }),
                ] {
                    assert_eq!(
                        super::with_assoc_args(types, t, owner, args),
                        t,
                        "existing substitution boundary"
                    );
                }
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    #[test]
    fn erasing_at_depth_1000_fits_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let types = pool.types();
                let reference = |counted| {
                    pool.intern_ty(&TyData::Ref {
                        inner: Ty::I32,
                        mutable: true,
                        counted,
                    })
                };
                let function = |r, deps| {
                    pool.intern_ty(&TyData::Fn {
                        params: pool.list(&[r]),
                        result: r,
                        row: RowId::EMPTY,
                        suspends: false,
                        inputs: Inputs::Fixed,
                        deps,
                    })
                };
                let counted = reference(true);
                let deps = types.fn_deps(&[1], &[1]);
                let leaf = function(counted, deps);
                let plain = function(counted, FnDeps::NONE);
                let local = function(reference(false), FnDeps::NONE);
                for depth in [100, 1_000] {
                    let mut chain = [leaf, plain, local];
                    for _ in 0..depth {
                        for t in &mut chain {
                            *t = pool.product(pool.list(&[*t, *t]), None);
                        }
                    }
                    let (out, work) =
                        crate::work::count_type_nodes(|| types.erase_fn_deps(chain[0]));
                    assert_eq!(out, chain[1]);
                    assert_eq!(work, depth + 1);
                    let (out, work) =
                        crate::work::count_type_nodes(|| types.erase_ref_kinds(chain[0]));
                    assert_eq!(out, chain[2]);
                    assert_eq!(work, depth + 2);
                    let (out, work) = crate::work::count_type_nodes(|| {
                        types.erase_fn_deps_list(pool.list(&[chain[0], chain[0]]))
                    });
                    assert_eq!(pool.list_items(out), &[chain[1], chain[1]]);
                    assert_eq!(work, depth + 1, "one memo shared by list roots");
                }
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    #[test]
    fn erasing_fn_deps_keeps_every_reference_kind() {
        use crate::pool::FnDeps;
        let pool = InternPool::new();
        let kinds: Vec<Ty> = [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .map(|(mutable, counted)| {
                pool.intern_ty(&TyData::Ref {
                    inner: Ty::I32,
                    mutable,
                    counted,
                })
            })
            .collect();
        let local = kinds[0];
        let types = pool.types();
        let deps = types.fn_deps(&[1], &[1]);
        assert_ne!(deps, FnDeps::NONE);
        let lending = |deps| {
            pool.intern_ty(&TyData::Fn {
                params: pool.list(&[local]),
                result: local,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps,
            })
        };
        let (with, without) = (lending(deps), lending(FnDeps::NONE));
        assert_ne!(with, without, "the map is part of the type");
        for (k, &reference) in kinds.iter().enumerate() {
            assert_eq!(pool.erase_fn_deps(reference), reference, "kind {k} stays");
            let held = pool.product(pool.list(&[reference, with]), None);
            let erased = pool.product(pool.list(&[reference, without]), None);
            assert_eq!(pool.erase_fn_deps(held), erased);
        }
        // A type built by doubling: 64 levels, each part walked once.
        let mut t = with;
        for _ in 0..64 {
            t = pool.product(pool.list(&[t, t]), None);
        }
        let erased = pool.erase_fn_deps(t);
        let mut u = without;
        for _ in 0..64 {
            u = pool.product(pool.list(&[u, u]), None);
        }
        assert_eq!(erased, u);
        let no_ref = pool.list(&[Ty::I32, without]);
        assert_eq!(
            pool.erase_fn_deps_list(no_ref),
            no_ref,
            "no reference, no walk"
        );
        // Erasing kinds keeps the permission and makes a counted one local.
        for (k, &reference) in kinds.iter().enumerate() {
            assert_eq!(types.erase_ref_kinds(reference), kinds[k & 1]);
        }
        let in_fn = |inner| {
            pool.intern_ty(&TyData::Fn {
                params: pool.list(&[inner]),
                result: Ty::VOID,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps: FnDeps::NONE,
            })
        };
        assert_eq!(types.erase_ref_kinds(in_fn(kinds[3])), in_fn(kinds[1]));
    }

    /// Every form that can hold a function type with a reference is walked:
    /// the erasure reaches the map through each holder, and a holder with
    /// nothing to erase comes back unchanged.
    #[test]
    fn erasing_fn_deps_reaches_every_holder_form() {
        use crate::pool::FnDeps;
        let pool = InternPool::new();
        let types = pool.types();
        let local = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        let lending = |deps| {
            pool.intern_ty(&TyData::Fn {
                params: pool.list(&[local]),
                result: local,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps,
            })
        };
        let (with, without) = (lending(types.fn_deps(&[1], &[1])), lending(FnDeps::NONE));
        let def = hd_base::DefId::from_raw(7);
        let other = hd_base::DefId::from_raw(8);
        let row = |t: Ty| {
            pool.row(&RowData {
                keys: vec![t],
                params: vec![],
            })
        };
        let holders: Vec<Box<dyn Fn(Ty) -> TyData + '_>> = vec![
            Box::new(|t| TyData::Adt(AdtId::Declared(def), pool.list(&[t]))),
            Box::new(|t| TyData::Opaque {
                owner: def,
                site: 1,
                borrows: false,
                args: pool.list(&[t]),
                bound: t,
            }),
            Box::new(|t| TyData::Adt(AdtId::Product { rest: true }, pool.list(&[Ty::I32, t]))),
            Box::new(|t| TyData::sum(pool.list(&[Ty::I32, t]))),
            Box::new(|t| TyData::Row(row(t))),
            Box::new(|t| TyData::Context(row(t))),
            Box::new(|t| TyData::TraitValue {
                def,
                args: pool.list(&[t]),
                bindings: vec![(other, t)],
            }),
            Box::new(|t| TyData::Assoc {
                assoc: def,
                trait_: other,
                self_ty: t,
                args: pool.list(&[t]),
            }),
        ];
        for (i, holder) in holders.iter().enumerate() {
            let held = pool.intern_ty(&holder(with));
            let erased = pool.intern_ty(&holder(without));
            assert_eq!(pool.erase_fn_deps(held), erased, "holder {i}");
            assert_eq!(pool.erase_fn_deps(erased), erased, "holder {i} unchanged");
        }
        // `sole_arg` answers only for one argument of the named definition.
        let one = pool.intern_ty(&TyData::Adt(AdtId::Declared(def), pool.list(&[Ty::I32])));
        let two = pool.intern_ty(&TyData::Adt(
            AdtId::Declared(def),
            pool.list(&[Ty::I32, Ty::I32]),
        ));
        assert_eq!(types.sole_arg(one, def), Some(Ty::I32));
        assert_eq!(types.sole_arg(two, def), None);
        assert_eq!(types.sole_arg(one, other), None);
    }
    #[test]
    fn trait_bindings_are_borrowed_in_name_order_and_binary_searched() {
        let pool = InternPool::new();
        let names = [hd_base::DefId::from_raw(30), hd_base::DefId::from_raw(10)];
        let key = pool.intern_ty(&TyData::TraitValue {
            def: hd_base::DefId::from_raw(7),
            args: TyList::EMPTY,
            bindings: vec![(names[0], Ty::STRING), (names[1], Ty::I32)],
        });
        let types = pool.types();
        assert_eq!(
            types.trait_value_bindings(key).unwrap().collect::<Vec<_>>(),
            [(names[1], Ty::I32), (names[0], Ty::STRING)]
        );
        assert_eq!(types.trait_value_binding(key, names[0]), Some(Ty::STRING));
        assert_eq!(types.trait_value_binding(key, names[1]), Some(Ty::I32));
        assert_eq!(
            types.trait_value_binding(key, hd_base::DefId::from_raw(20)),
            None
        );
        assert!(types.trait_value_bindings(Ty::I32).is_none());
    }
}
