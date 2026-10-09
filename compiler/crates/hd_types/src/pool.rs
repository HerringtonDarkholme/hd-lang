//! The `InternPool` (data-structures.md §3.4, §3.9.2): every type, row and
//! type list is one hash-consed item, so global type equality is one
//! integer compare. Storage is a one-byte tag, a `u32` data word, a `u32`
//! meta word (flags and node count) and variable parts in `extra`.
//! Types that hold inference variables live in the checked body's
//! `LocalPool` instead; [`Types`] reads and builds through both.

use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use hd_base::{AppendVec, DefId, InferVar};

/// A type: an `InternPool` index. Bit 31 set: a body-local index (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ty(pub u32);

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
    Void,
}

impl Prim {
    pub const ALL: [Prim; 15] = [
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
        Prim::Void,
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
            Prim::Void => "void",
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

/// A declared row parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowParamRef {
    pub owner: DefId,
    pub index: u16,
}

/// A row: sorted keys and declared row parameters. Pending private rows
/// (`PendingPart`) live only in the body and module tiers, never here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RowData {
    pub keys: Vec<Ty>,
    pub params: Vec<RowParamRef>,
}

/// The decoded view of one type (§3.4 `TyView`), owned for simplicity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyData {
    Prim(Prim),
    Never,
    Poison,
    Adt {
        def: DefId,
        args: TyList,
    },
    Tuple {
        elems: TyList,
        rest: Option<Ty>,
    },
    Option(Ty),
    Fn {
        params: TyList,
        result: Ty,
        row: RowId,
        suspends: bool,
    },
    TraitValue {
        def: DefId,
        args: TyList,
        bindings: Vec<(DefId, Ty)>,
    },
    Param(ParamRef),
    /// An unnormalized projection: the associated item and its trait reference.
    Assoc {
        assoc: DefId,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
    },
    Mut(Ty),
    Infer(InferVar),
    /// A canonical placeholder of the solver (trait-solver.md §2.2).
    Canon(u8),
    /// A requirement row in a row parameter's argument slot (an explicit
    /// `f::[$ Db]`, or the row a call solved for `$R`).
    Row(RowId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
enum PoolTag {
    Prim,
    Never,
    Poison,
    Adt,
    Tuple,
    TupleRest,
    Option,
    Fn,
    TraitValue,
    Param,
    Assoc,
    Mut,
    Infer,
    Canon,
    List,
    Row,
    RowTy,
}

/// Meta flags (§3.9.2).
pub mod meta {
    pub const HAS_INFER: u32 = 1;
    pub const HAS_POISON: u32 = 2;
    pub const HAS_PARAM: u32 = 4;
    pub const HAS_ASSOC: u32 = 8;
    pub const HAS_CANON: u32 = 16;
}

impl PoolTag {
    /// Whether `data` is an offset into `extra` (§3.9.2's table); the other
    /// tags keep their whole payload inline in `data`.
    const fn has_record(self) -> bool {
        matches!(
            self,
            PoolTag::Adt
                | PoolTag::TupleRest
                | PoolTag::Fn
                | PoolTag::TraitValue
                | PoolTag::Param
                | PoolTag::Assoc
                | PoolTag::List
                | PoolTag::Row
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
    extra: AppendVec<u32>,
    tys: AppendVec<Ty>,
}

impl Cols {
    /// Appends one item. The caller serializes appends, so the columns
    /// stay row-aligned.
    fn push(&self, c: &Content<'_>, meta: u32) -> u32 {
        let data = if c.tag == PoolTag::List {
            let len = u32::try_from(c.tys.len()).expect("list");
            let start = self.tys.push_run(len, Ty::NEVER, c.tys.iter().copied());
            self.extra.push_run(2, 0, [start, len])
        } else if c.tag.has_record() {
            let len = u32::try_from(c.head.len()).expect("record");
            self.extra.push_run(len, 0, c.head.iter().copied())
        } else {
            c.data
        };
        let i = self.tag.push(c.tag);
        self.data.push(data);
        self.meta.push(meta);
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
            PoolTag::Adt | PoolTag::TupleRest | PoolTag::Param | PoolTag::List => 2,
            PoolTag::Fn | PoolTag::Assoc => 4,
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
        let mut h = mix(0, self.tag as u32);
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
    u32::try_from(w & 0xFFFF_FFFF).expect("low word")
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

    fn intern(&self, c: &Content<'_>, meta: u32) -> u32 {
        let h32 = low(c.hash());
        let mut idx = self.index.borrow_mut();
        let found = idx.find(h32, |i| c.matches(&self.cols, i));
        let row = found.unwrap_or_else(|| {
            let i = self.cols.push(c, meta);
            idx.insert(h32, i);
            i
        });
        assert!(row <= LOCAL_ROW, "body-local pool over capacity");
        LOCAL | self.generation << GEN_SHIFT | row
    }

    /// Items in this pool.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.cols.tag.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cols.tag.is_empty()
    }
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

    #[must_use]
    pub fn global(self) -> &'a InternPool {
        self.global
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

    /// Interns in the local pool when the item holds a variable, else in
    /// the global pool.
    fn intern(self, tag: PoolTag, data: u32, rec: (&[u32], &[Ty]), meta: u32) -> u32 {
        if meta & meta::HAS_INFER == 0 {
            return self.global.intern(tag, data, rec, meta);
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
    pub const VOID: Ty = Ty::prim(Prim::Void);
    pub const NEVER: Ty = Ty(15);
    pub const POISON: Ty = Ty(16);
}

impl TyList {
    pub const EMPTY: TyList = TyList(17);
}

impl RowId {
    pub const EMPTY: RowId = RowId(18);
}

impl InternPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns `tag` with an inline `data` word or with the record `head`
    /// then `tys`. A hit allocates nothing: the content is hashed and
    /// compared where it lies.
    fn intern(&self, tag: PoolTag, data: u32, (head, tys): (&[u32], &[Ty]), meta: u32) -> u32 {
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
        let shard = &self.shards[usize::try_from(h >> 58).expect("shard")].0;
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
                self.cols.push(&c, meta)
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
    pub fn intern_ty(&self, t: &TyData) -> Ty {
        self.types().intern_ty(t)
    }
    #[must_use]
    pub fn get(&self, t: Ty) -> TyData {
        self.types().get(t)
    }
    #[must_use]
    pub fn has_infer(&self, t: Ty) -> bool {
        self.types().has_infer(t)
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
    pub fn has_assoc(&self, t: Ty) -> bool {
        self.types().has_assoc(t)
    }
    pub fn subst(&self, t: Ty, f: &dyn Fn(ParamRef) -> Option<Ty>) -> Ty {
        self.types().subst(t, f)
    }
    pub fn subst_row(&self, r: RowId, f: &dyn Fn(ParamRef) -> Option<Ty>) -> RowId {
        self.types().subst_row(r, f)
    }
    #[must_use]
    pub fn display(&self, t: Ty) -> String {
        self.types().display(t)
    }
    #[must_use]
    pub fn len(&self) -> u32 {
        self.cols.tag.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cols.tag.is_empty()
    }
}

impl<'a> Types<'a> {
    fn meta_of(self, t: Ty) -> u32 {
        self.meta_word(t.0)
    }

    fn list_meta(self, l: TyList) -> u32 {
        self.meta_word(l.0)
    }

    #[must_use]
    pub fn list(self, tys: &[Ty]) -> TyList {
        let meta = tys.iter().fold(0, |m, t| m | self.meta_of(*t));
        TyList(self.intern(PoolTag::List, 0, (&[], tys), meta))
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
        keys.sort_by_key(|t| t.0);
        keys.dedup();
        params.sort_by_key(|p| (p.owner.raw(), p.index));
        params.dedup();
        let mut meta = keys.iter().fold(0, |m, t| m | self.meta_of(*t));
        if !params.is_empty() {
            meta |= meta::HAS_PARAM;
        }
        let mut words: Vec<u32> = vec![u32::try_from(keys.len()).expect("row keys")];
        words.extend(keys.iter().map(|t| t.0));
        words.push(u32::try_from(params.len()).expect("row params"));
        for p in &params {
            words.push(p.owner.raw());
            words.push(u32::from(p.index));
        }
        RowId(self.intern(PoolTag::Row, 0, (&words, &[]), meta))
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
                index: u16::try_from(c[1]).expect("row param"),
            })
            .collect();
        RowData { keys, params }
    }

    /// Interns a type: in the body's local pool when it holds an inference
    /// variable, else in the global pool (§3.4).
    #[must_use]
    pub fn intern_ty(self, t: &TyData) -> Ty {
        use PoolTag as T;
        let (tag, data, meta): (PoolTag, u32, u32) = match t {
            TyData::Prim(p) => (T::Prim, *p as u32, 0),
            TyData::Never => (T::Never, 0, 0),
            TyData::Poison => (T::Poison, 0, meta::HAS_POISON),
            TyData::Adt { args, .. } => (T::Adt, 0, self.list_meta(*args)),
            TyData::Tuple { elems, rest: None } => (T::Tuple, elems.0, self.list_meta(*elems)),
            TyData::Tuple {
                elems,
                rest: Some(r),
            } => (T::TupleRest, 0, self.list_meta(*elems) | self.meta_of(*r)),
            TyData::Option(inner) => (T::Option, inner.0, self.meta_of(*inner)),
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => (
                T::Fn,
                0,
                self.list_meta(*params) | self.meta_of(*result) | self.row_meta(*row),
            ),
            TyData::TraitValue { args, bindings, .. } => (
                T::TraitValue,
                0,
                bindings
                    .iter()
                    .fold(self.list_meta(*args), |m, (_, t)| m | self.meta_of(*t)),
            ),
            TyData::Param(_) => (T::Param, 0, meta::HAS_PARAM),
            TyData::Assoc { self_ty, args, .. } => (
                T::Assoc,
                0,
                meta::HAS_ASSOC | self.meta_of(*self_ty) | self.list_meta(*args),
            ),
            TyData::Mut(inner) => (T::Mut, inner.0, self.meta_of(*inner)),
            TyData::Infer(v) => (T::Infer, v.raw(), meta::HAS_INFER),
            TyData::Canon(i) => (T::Canon, u32::from(*i), meta::HAS_CANON),
            TyData::Row(r) => (T::RowTy, r.0, self.row_meta(*r)),
        };
        let mut buf = [0u32; 4];
        let rec: &[u32] = match t {
            TyData::Adt { def, args } => {
                buf[..2].copy_from_slice(&[def.raw(), args.0]);
                &buf[..2]
            }
            TyData::Tuple {
                elems,
                rest: Some(r),
            } => {
                buf[..2].copy_from_slice(&[elems.0, r.0]);
                &buf[..2]
            }
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => {
                buf = [params.0, result.0, row.0, u32::from(*suspends)];
                &buf
            }
            TyData::Param(p) => {
                buf[..2].copy_from_slice(&[p.owner.raw(), u32::from(p.index)]);
                &buf[..2]
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                buf = [assoc.raw(), trait_.raw(), self_ty.0, args.0];
                &buf
            }
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let mut b = bindings.clone();
                b.sort_by_key(|(d, _)| d.raw());
                let mut w = vec![def.raw(), args.0, u32::try_from(b.len()).expect("bindings")];
                for (d, t) in b {
                    w.push(d.raw());
                    w.push(t.0);
                }
                return Ty(self.intern(tag, data, (&w, &[]), meta));
            }
            _ => &[],
        };
        Ty(self.intern(tag, data, (rec, &[]), meta))
    }

    /// Decodes a type (§3.4 `TyView`).
    #[must_use]
    pub fn get(self, t: Ty) -> TyData {
        let (c, i) = self.at(t.0);
        let (tag, d) = (c.tag[i], c.data[i]);
        let x = c.record_of(tag, d);
        match tag {
            PoolTag::Prim => TyData::Prim(Prim::ALL[d as usize]),
            PoolTag::Never => TyData::Never,
            PoolTag::Poison => TyData::Poison,
            PoolTag::Adt => TyData::Adt {
                def: DefId::from_raw(x[0]),
                args: TyList(x[1]),
            },
            PoolTag::Tuple => TyData::Tuple {
                elems: TyList(d),
                rest: None,
            },
            PoolTag::TupleRest => TyData::Tuple {
                elems: TyList(x[0]),
                rest: Some(Ty(x[1])),
            },
            PoolTag::Option => TyData::Option(Ty(d)),
            PoolTag::Fn => TyData::Fn {
                params: TyList(x[0]),
                result: Ty(x[1]),
                row: RowId(x[2]),
                suspends: x[3] != 0,
            },
            PoolTag::TraitValue => TyData::TraitValue {
                def: DefId::from_raw(x[0]),
                args: TyList(x[1]),
                bindings: x[3..]
                    .chunks(2)
                    .map(|c| (DefId::from_raw(c[0]), Ty(c[1])))
                    .collect(),
            },
            PoolTag::Param => TyData::Param(ParamRef {
                owner: DefId::from_raw(x[0]),
                index: u16::try_from(x[1]).expect("param"),
            }),
            PoolTag::Assoc => TyData::Assoc {
                assoc: DefId::from_raw(x[0]),
                trait_: DefId::from_raw(x[1]),
                self_ty: Ty(x[2]),
                args: TyList(x[3]),
            },
            PoolTag::Mut => TyData::Mut(Ty(d)),
            PoolTag::Infer => TyData::Infer(InferVar::from_raw(d)),
            PoolTag::Canon => TyData::Canon(u8::try_from(d).expect("canon")),
            PoolTag::RowTy => TyData::Row(RowId(d)),
            PoolTag::List | PoolTag::Row => unreachable!("not a type index"),
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
    pub fn has_param(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_PARAM != 0
    }
    #[must_use]
    pub fn has_assoc(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_ASSOC != 0
    }
    /// Whether `t` holds a canonical placeholder (trait-solver.md §2.2).
    #[must_use]
    pub fn has_canon(self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_CANON != 0
    }
    /// Replaces declared parameters: `f` maps a parameter to its argument,
    /// or `None` to keep it.
    pub fn subst(self, t: Ty, f: &dyn Fn(ParamRef) -> Option<Ty>) -> Ty {
        if !self.has_param(t) {
            return t;
        }
        let l = |x: TyList| {
            self.list(
                &self
                    .list_items(x)
                    .iter()
                    .copied()
                    .map(|e| self.subst(e, f))
                    .collect::<Vec<_>>(),
            )
        };
        let d = match self.get(t) {
            TyData::Param(p) => return f(p).unwrap_or(t),
            TyData::Adt { def, args } => TyData::Adt { def, args: l(args) },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: l(elems),
                rest: rest.map(|r| self.subst(r, f)),
            },
            TyData::Option(i) => TyData::Option(self.subst(i, f)),
            TyData::Mut(i) => TyData::Mut(self.subst(i, f)),
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => TyData::Fn {
                params: l(params),
                result: self.subst(result, f),
                row: self.subst_row(row, f),
                suspends,
            },
            TyData::Row(r) => TyData::Row(self.subst_row(r, f)),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => TyData::TraitValue {
                def,
                args: l(args),
                bindings: bindings
                    .into_iter()
                    .map(|(d, b)| (d, self.subst(b, f)))
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
                self_ty: self.subst(self_ty, f),
                args: l(args),
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
        let d = self.row_data(r);
        let mut out = RowData {
            keys: d.keys.iter().map(|k| self.subst(*k, f)).collect(),
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

    /// Prints a type with run IDs for items (`#n`); output that a user
    /// sees goes through stable paths instead (§6.5).
    #[must_use]
    pub fn display(self, t: Ty) -> String {
        let list = |l: TyList| {
            self.list_items(l)
                .iter()
                .map(|x| self.display(*x))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self.get(t) {
            TyData::Prim(p) => p.name().to_owned(),
            TyData::Never => "never".into(),
            TyData::Poison => "{poison}".into(),
            TyData::Adt { def, args } if args == TyList::EMPTY => format!("#{}", def.raw()),
            TyData::Adt { def, args } => format!("#{}[{}]", def.raw(), list(args)),
            TyData::Tuple { elems, rest } => {
                format!(
                    "({}{})",
                    list(elems),
                    rest.map_or(String::new(), |r| format!(", ...{}", self.display(r)))
                )
            }
            TyData::Option(i) => format!("{}?", self.display(i)),
            TyData::Fn {
                params,
                result,
                suspends,
                ..
            } => {
                format!(
                    "fn{}({}) -> {}",
                    if suspends { "!" } else { "" },
                    list(params),
                    self.display(result)
                )
            }
            TyData::TraitValue { def, .. } => format!("dyn #{}", def.raw()),
            TyData::Param(p) => format!("T{}@{}", p.index, p.owner.raw()),
            TyData::Assoc { assoc, self_ty, .. } => {
                format!("<{}>::#{}", self.display(self_ty), assoc.raw())
            }
            TyData::Mut(i) => format!("mut {}", self.display(i)),
            TyData::Infer(v) => format!("?{}", v.raw()),
            TyData::Canon(i) => format!("^{i}"),
            TyData::Row(r) => {
                let d = self.row_data(r);
                let mut parts: Vec<String> = d.keys.iter().map(|k| self.display(*k)).collect();
                parts.extend(
                    d.params
                        .iter()
                        .map(|p| format!("R{}@{}", p.index, p.owner.raw())),
                );
                format!("$({})", parts.join(" + "))
            }
        }
    }
}

/// A projection written `Self::Out` in a trait names the trait's own
/// arguments implicitly: fills them in from the call's.
#[must_use]
pub fn with_assoc_args(pool: Types<'_>, t: Ty, trait_: DefId, args: TyList) -> Ty {
    if args == TyList::EMPTY {
        return t;
    }
    let list = |l: TyList| {
        pool.list(
            &pool
                .list_items(l)
                .iter()
                .copied()
                .map(|x| with_assoc_args(pool, x, trait_, args))
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
            self_ty: with_assoc_args(pool, self_ty, trait_, args),
            args: if tr == trait_ && a == TyList::EMPTY {
                args
            } else {
                list(a)
            },
        },
        TyData::Adt { def, args: a } => TyData::Adt { def, args: list(a) },
        TyData::Tuple { elems, rest } => TyData::Tuple {
            elems: list(elems),
            rest,
        },
        TyData::Option(i) => TyData::Option(with_assoc_args(pool, i, trait_, args)),
        TyData::Mut(i) => TyData::Mut(with_assoc_args(pool, i, trait_, args)),
        TyData::Fn {
            params,
            result,
            row,
            suspends,
        } => TyData::Fn {
            params: list(params),
            result: with_assoc_args(pool, result, trait_, args),
            row,
            suspends,
        },
        _ => return t,
    };
    pool.intern_ty(&d)
}

#[cfg(test)]
mod tests {
    use super::{InternPool, LocalPool, Prim, RowData, RowId, Ty, TyData, TyList, Types};
    use hd_base::DefId;

    #[test]
    fn preseeded_constants() {
        let p = InternPool::new();
        assert_eq!(p.get(Ty::I32), TyData::Prim(Prim::I32));
        assert_eq!(p.get(Ty::NEVER), TyData::Never);
        assert_eq!(p.get(Ty::POISON), TyData::Poison);
        assert_eq!(p.list(&[]), TyList::EMPTY);
        assert_eq!(p.row(&RowData::default()), RowId::EMPTY);
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
                                let o = p.intern_ty(&TyData::Option(Ty::prim(
                                    Prim::ALL[(n % 15) as usize],
                                )));
                                p.list(&[o, Ty::prim(Prim::ALL[(n / 15 % 15) as usize])])
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
            TyData::Adt { def, args: list },
            TyData::Tuple {
                elems: list,
                rest: None,
            },
            TyData::Tuple {
                elems: list,
                rest: Some(Ty::BOOL),
            },
            TyData::Option(Ty::I32),
            TyData::Fn {
                params: list,
                result: Ty::VOID,
                row,
                suspends: true,
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
            TyData::Mut(Ty::STRING),
            TyData::Canon(2),
        ];
        for f in forms {
            let t = p.intern_ty(&f);
            assert_eq!(p.get(t), f);
            assert_eq!(p.intern_ty(&f), t, "hash-consed");
        }
        assert_eq!(p.row_data(row).keys.len(), 2);
        let opt = p.intern_ty(&TyData::Option(Ty::POISON));
        assert!(p.has_poison(opt));
    }

    /// Variable-holding types go to the body's local pool and never grow
    /// the global one; variable-free types built through the same view are
    /// global.
    #[test]
    fn variables_stay_body_local() {
        let g = InternPool::new();
        let before = g.len();
        let local = LocalPool::new();
        let p = Types::with_local(&g, &local);
        let v = p.intern_ty(&TyData::Infer(hd_base::InferVar::from_raw(3)));
        let opt = p.intern_ty(&TyData::Option(v));
        let l = p.list(&[opt, Ty::I32]);
        assert!(v.is_local() && opt.is_local() && Ty(l.0).is_local());
        assert_eq!(p.get(opt), TyData::Option(v));
        assert_eq!(p.list_items(l), &[opt, Ty::I32]);
        assert_eq!(p.intern_ty(&TyData::Option(v)), opt, "hash-consed locally");
        assert!(p.has_infer(opt));
        let plain = p.intern_ty(&TyData::Option(Ty::I32));
        assert!(!plain.is_local());
        assert_eq!(g.len(), before + 1, "only the variable-free type is global");
        assert_eq!(local.len(), 3);
    }
}
