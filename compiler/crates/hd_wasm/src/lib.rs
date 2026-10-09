#![forbid(unsafe_code)]
//! `hd_wasm`: Wasm GC emission (codegen.md §12, wasm-layout.md §15) and link
//! (§13.10). `Emit` walks one instance's `hd_tir::ir::Body` under its
//! substitution (`emit`), with value layouts from `layout` and call targets
//! from collection (no selection here). Runtime pieces the compiler builds
//! (literal getters, number formatting, panic stubs, host provider stubs,
//! the wake table, `race!`'s frame, vtable adapters, the entry exports)
//! are `rt` helpers. A suspending instance's code entry is its cold
//! constructor, with its state-machine body, poll and cancel functions as
//! parts (suspension.md §14.1). A code entry holds symbolic relocations:
//! callees by instance key, part or helper, globals by binding path hash,
//! Wasm types by their structural descriptor; `Link` assigns indices.

pub mod asm;
pub mod emit;
pub mod layout;
pub mod meta;
pub mod rt;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, NotImplemented, StableHasher, Stage, StageResult};
use wasm_encoder::{
    CodeSection, CompositeInnerType, CompositeType, ConstExpr, DataCountSection, DataSection,
    ElementSection, Elements, EntityType, ExportKind, ExportSection, FieldType, FuncType,
    FunctionSection, GlobalSection, GlobalType, HeapType, ImportSection, MemorySection, MemoryType,
    Module, RefType, StorageType, StructType, SubType, TypeSection, ValType,
};

pub use emit::{emit, emit_adapter, entry, script_entry, test_entry};
pub use rt::Helper;

/// A Wasm value type, with references to structural type descriptors.
/// Ordered as a derived order would be, with a shared descriptor compared
/// by pointer first (`Ord` below).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VT {
    I32,
    I64,
    F32,
    F64,
    /// `eqref`, nullable: erased storage (the A1 class `REF`, wasm-layout.md §15.1).
    Eq,
    /// `(ref $T)` or `(ref null $T)`.
    Ref(Arc<WTy>, bool),
}

/// A Wasm heap type, described by structure (hd needs no nominal Wasm
/// types, codegen.md §13.7): equal descriptors are one type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WTy {
    /// `(array (mut i8))`: string bytes.
    Bytes,
    /// `(array (mut T))`.
    Array(VT),
    /// A struct of mutable fields; `open` is a non-final type that closures,
    /// suspension frames and their subtypes extend.
    Struct {
        fields: Vec<VT>,
        sup: Option<Box<WTy>>,
        open: bool,
    },
    Func(Vec<VT>, Vec<VT>),
    /// Member `i` of a recursion group (wasm-layout.md §15.3).
    Rec(Arc<Group>, u32),
    /// Inside a recursion group's member: the group's member `i`.
    Back(u32),
}

/// A recursion group (wasm-layout.md §15.3): its members in canonical
/// order, naming each other as `Back`. A group is identified by a hash of
/// its members, which names other groups by their hashes, so groups
/// compare, order and hash in constant time however deep the types they
/// name nest.
pub struct Group {
    members: Vec<WTy>,
    hash: Hash128,
}

impl Group {
    #[must_use]
    pub fn new(members: Vec<WTy>) -> Arc<Group> {
        let mut h = StableHasher::new("wasm-rec-group");
        h.u32(u32::try_from(members.len()).expect("group"));
        for m in &members {
            m.digest(&mut h);
        }
        Arc::new(Group {
            members,
            hash: h.finish(),
        })
    }
}

/// A group shows its hash and size, not the groups it names: a type's
/// text stays as small as the type.
impl std::fmt::Debug for Group {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Group({:032x}, {})", self.hash.0, self.members.len())
    }
}

impl std::ops::Deref for Group {
    type Target = [WTy];
    fn deref(&self) -> &[WTy] {
        &self.members
    }
}

impl PartialEq for Group {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for Group {}

impl std::hash::Hash for Group {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl Ord for Group {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.hash.cmp(&other.hash)
    }
}

impl PartialOrd for Group {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// The order of `VT` and `WTy` is the derived one (variant, then fields in
// order), except that two references to one shared descriptor compare
// equal without walking it, and groups compare by their hash: descriptors
// are trees that share their parts, and link and the helper table order
// them often.

impl VT {
    fn variant(&self) -> u8 {
        match self {
            VT::I32 => 0,
            VT::I64 => 1,
            VT::F32 => 2,
            VT::F64 => 3,
            VT::Eq => 4,
            VT::Ref(..) => 5,
        }
    }
}

impl Ord for VT {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (VT::Ref(a, n), VT::Ref(b, m)) => {
                let by = if Arc::ptr_eq(a, b) {
                    std::cmp::Ordering::Equal
                } else {
                    a.cmp(b)
                };
                by.then(n.cmp(m))
            }
            _ => self.variant().cmp(&other.variant()),
        }
    }
}

impl PartialOrd for VT {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl WTy {
    fn variant(&self) -> u8 {
        match self {
            WTy::Bytes => 0,
            WTy::Array(_) => 1,
            WTy::Struct { .. } => 2,
            WTy::Func(..) => 3,
            WTy::Rec(..) => 4,
            WTy::Back(_) => 5,
        }
    }
}

impl Ord for WTy {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (WTy::Array(a), WTy::Array(b)) => a.cmp(b),
            (
                WTy::Struct {
                    fields: f,
                    sup: s,
                    open: o,
                },
                WTy::Struct {
                    fields: g,
                    sup: t,
                    open: p,
                },
            ) => f.cmp(g).then_with(|| s.cmp(t)).then(o.cmp(p)),
            (WTy::Func(p, r), WTy::Func(q, s)) => p.cmp(q).then_with(|| r.cmp(s)),
            (WTy::Rec(g, i), WTy::Rec(h, j)) => g.hash.cmp(&h.hash).then(i.cmp(j)),
            (WTy::Back(i), WTy::Back(j)) => i.cmp(j),
            _ => self.variant().cmp(&other.variant()),
        }
    }
}

impl PartialOrd for WTy {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl VT {
    #[must_use]
    pub fn r(t: WTy) -> VT {
        VT::Ref(Arc::new(t), false)
    }
    #[must_use]
    pub fn rn(t: WTy) -> VT {
        VT::Ref(Arc::new(t), true)
    }
    /// The defaultable form (wasm-layout.md §15.2): references nullable.
    #[must_use]
    pub fn dflt(&self) -> VT {
        match self {
            VT::Ref(t, _) => VT::Ref(t.clone(), true),
            v => v.clone(),
        }
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            VT::I32 => w.u8(0),
            VT::I64 => w.u8(1),
            VT::F32 => w.u8(2),
            VT::F64 => w.u8(3),
            VT::Eq => w.u8(4),
            VT::Ref(t, n) => {
                w.u8(if *n { 6 } else { 5 });
                t.encode(w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>, depth: u8) -> Option<VT> {
        Self::decode_in(r, depth, None)
    }
    fn decode_in(r: &mut Reader<'_>, depth: u8, group: Option<u32>) -> Option<VT> {
        Some(match r.u8() {
            0 => VT::I32,
            1 => VT::I64,
            2 => VT::F32,
            3 => VT::F64,
            4 => VT::Eq,
            5 => VT::Ref(Arc::new(WTy::decode_in(r, depth, group)?), false),
            6 => VT::Ref(Arc::new(WTy::decode_in(r, depth, group)?), true),
            _ => return None,
        })
    }
}

pub(crate) fn encode_vts(v: &[VT], w: &mut Writer) {
    w.len_of(v);
    for x in v {
        x.encode(w);
    }
}

pub(crate) fn decode_vts(r: &mut Reader<'_>, depth: u8) -> Option<Vec<VT>> {
    let n = r.count();
    (0..n).map(|_| VT::decode(r, depth)).collect()
}

impl WTy {
    /// Feeds a group member's content to its group's hash: other groups by
    /// their hashes, so the walk ends at them.
    pub(crate) fn digest(&self, h: &mut StableHasher) {
        let vts = |v: &[VT], h: &mut StableHasher| {
            h.u32(u32::try_from(v.len()).expect("values"));
            for x in v {
                match x {
                    VT::Ref(t, n) => {
                        h.u8(if *n { 6 } else { 5 });
                        t.digest(h);
                    }
                    x => h.u8(x.variant()),
                }
            }
        };
        match self {
            WTy::Bytes => h.u8(0),
            WTy::Array(v) => {
                h.u8(1);
                vts(std::slice::from_ref(v), h);
            }
            WTy::Struct { fields, sup, open } => {
                h.u8(2);
                vts(fields, h);
                h.u8(u8::from(*open));
                match sup {
                    Some(s) => {
                        h.u8(1);
                        s.digest(h);
                    }
                    None => h.u8(0),
                }
            }
            WTy::Func(p, r) => {
                h.u8(3);
                vts(p, h);
                vts(r, h);
            }
            WTy::Rec(g, i) => {
                h.u8(4);
                h.hash(g.hash);
                h.u32(*i);
            }
            WTy::Back(i) => {
                h.u8(5);
                h.u32(*i);
            }
        }
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            WTy::Bytes => w.u8(0),
            WTy::Array(v) => {
                w.u8(1);
                v.encode(w);
            }
            WTy::Struct { fields, sup, open } => {
                w.u8(2);
                encode_vts(fields, w);
                w.u8(u8::from(*open));
                match sup {
                    Some(s) => {
                        w.u8(1);
                        s.encode(w);
                    }
                    None => w.u8(0),
                }
            }
            WTy::Func(p, r) => {
                w.u8(3);
                encode_vts(p, w);
                encode_vts(r, w);
            }
            WTy::Rec(g, i) => {
                w.u8(4);
                w.u32(table_index(g));
                w.u32(*i);
            }
            WTy::Back(i) => {
                w.u8(5);
                w.u32(*i);
            }
        }
    }
    /// Decodes a type; `None` when malformed, including a `Back` outside
    /// a group or past its end. `group` is the enclosing group's size.
    fn decode(r: &mut Reader<'_>, depth: u8) -> Option<WTy> {
        Self::decode_in(r, depth, None)
    }
    fn decode_in(r: &mut Reader<'_>, depth: u8, group: Option<u32>) -> Option<WTy> {
        let depth = depth.checked_add(1).filter(|d| *d < 64)?;
        let vts = |r: &mut Reader<'_>| -> Option<Vec<VT>> {
            (0..r.count())
                .map(|_| VT::decode_in(r, depth, group))
                .collect()
        };
        Some(match r.u8() {
            0 => WTy::Bytes,
            1 => WTy::Array(VT::decode_in(r, depth, group)?),
            2 => {
                let fields = vts(r)?;
                let open = r.u8() == 1;
                let sup = if r.u8() == 1 {
                    Some(Box::new(WTy::decode_in(r, depth, group)?))
                } else {
                    None
                };
                WTy::Struct { fields, sup, open }
            }
            3 => WTy::Func(vts(r)?, vts(r)?),
            4 => {
                let g = table_group(r.u32())?;
                let i = r.u32();
                ((i as usize) < g.len()).then_some(())?;
                WTy::Rec(g, i)
            }
            5 => {
                let i = r.u32();
                (i < group?).then_some(())?;
                WTy::Back(i)
            }
            _ => return None,
        })
    }
    /// The type with a recursion group's member opened: its references to
    /// the group's members as `Rec` types (what a field read gives).
    #[must_use]
    pub fn unrolled(&self) -> std::borrow::Cow<'_, WTy> {
        match self {
            WTy::Rec(g, i) => std::borrow::Cow::Owned(g[*i as usize].close(g)),
            t => std::borrow::Cow::Borrowed(t),
        }
    }
    /// A member of `g` with its `Back` references as `Rec` types.
    fn close(&self, g: &Arc<Group>) -> WTy {
        self.map_refs(&mut |t| match t {
            WTy::Back(j) => Some(WTy::Rec(g.clone(), *j)),
            _ => None,
        })
    }
    /// Rebuilds the type, replacing each referenced type (a field's, an
    /// element's, a parameter's, a result's, the supertype) for which `f`
    /// answers; the rest are rebuilt in turn. A `Rec` is closed and kept.
    #[must_use]
    pub fn map_refs(&self, f: &mut dyn FnMut(&WTy) -> Option<WTy>) -> WTy {
        let mut at = |t: &WTy| f(t).unwrap_or_else(|| t.map_refs(f));
        let mut vt = |v: &VT| match v {
            VT::Ref(t, n) => VT::Ref(Arc::new(at(t)), *n),
            v => v.clone(),
        };
        match self {
            WTy::Array(v) => WTy::Array(vt(v)),
            WTy::Struct { fields, sup, open } => WTy::Struct {
                fields: fields.iter().map(&mut vt).collect(),
                sup: sup.as_ref().map(|s| Box::new(at(s))),
                open: *open,
            },
            WTy::Func(p, r) => {
                WTy::Func(p.iter().map(&mut vt).collect(), r.iter().map(vt).collect())
            }
            t @ (WTy::Bytes | WTy::Rec(..) | WTy::Back(_)) => t.clone(),
        }
    }
    /// A struct's fields, a recursion group's member opened.
    #[must_use]
    pub fn fields(&self) -> Vec<VT> {
        match &*self.unrolled() {
            WTy::Struct { fields, .. } => fields.clone(),
            _ => Vec::new(),
        }
    }
}

/// The functions a suspending instance lowers to besides its cold
/// constructor (suspension.md §14.1): `f$body`, `f$poll`, `f$cancel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    Body,
    Poll,
    Cancel,
}

impl Part {
    fn tag(self) -> u8 {
        match self {
            Part::Body => 0,
            Part::Poll => 1,
            Part::Cancel => 2,
        }
    }
    fn from_tag(t: u8) -> Option<Part> {
        [Part::Body, Part::Poll, Part::Cancel]
            .get(usize::from(t))
            .copied()
    }
}

/// A global symbol: a top-level binding's storage, one global per Wasm
/// value of its layout (wasm-layout.md §15.4, "module storage"), or a
/// runtime global.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GSym {
    /// The binding's path hash, the component, its (defaultable) type.
    Binding(Hash128, u32, VT),
    /// A runtime global by name, with its type.
    Rt(String, VT),
}

impl GSym {
    #[must_use]
    pub fn vt(&self) -> &VT {
        match self {
            GSym::Binding(_, _, v) | GSym::Rt(_, v) => v,
        }
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            GSym::Binding(h, k, v) => {
                w.u8(0);
                w.hash(*h);
                w.u32(*k);
                v.encode(w);
            }
            GSym::Rt(n, v) => {
                w.u8(1);
                w.str(n);
                v.encode(w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>) -> Option<GSym> {
        Some(match r.u8() {
            0 => GSym::Binding(r.hash(), r.u32(), VT::decode(r, 0)?),
            1 => GSym::Rt(r.str().to_owned(), VT::decode(r, 0)?),
            _ => return None,
        })
    }
}

/// A function symbol (codegen.md §13.8 `FuncTarget`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sym {
    /// A collected instance, by instance key (a suspending instance's
    /// cold constructor).
    Inst(Hash128),
    /// Another function of a suspending instance.
    Part(Hash128, Part),
    /// A host import: module, name and Wasm signature.
    Import {
        module: String,
        name: String,
        params: Vec<VT>,
        results: Vec<VT>,
    },
    /// A compiler-generated runtime function.
    Helper(Helper),
}

/// A relocation target (codegen.md §13.8 `Reloc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reloc {
    Func(Sym),
    Type(WTy),
    Global(GSym),
}

/// A code entry (codegen.md §13.8): locals and instructions as bytes, index
/// immediates as 5-byte padded LEBs, with relocations sorted by offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub params: Vec<VT>,
    pub results: Vec<VT>,
    pub body: Vec<u8>,
    pub relocs: Vec<(u32, Reloc)>,
    /// A suspending instance's other functions (suspension.md §14.1).
    pub parts: Vec<(Part, Code)>,
}

impl Sym {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            Sym::Inst(k) => {
                w.u8(0);
                w.hash(*k);
            }
            Sym::Import {
                module,
                name,
                params,
                results,
            } => {
                w.u8(1);
                w.str(module);
                w.str(name);
                encode_vts(params, w);
                encode_vts(results, w);
            }
            Sym::Helper(h) => {
                w.u8(2);
                h.encode(w);
            }
            Sym::Part(k, p) => {
                w.u8(3);
                w.hash(*k);
                w.u8(p.tag());
            }
        }
    }
    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<Sym> {
        Some(match r.u8() {
            0 => Sym::Inst(r.hash()),
            1 => Sym::Import {
                module: r.str().to_owned(),
                name: r.str().to_owned(),
                params: decode_vts(r, 0)?,
                results: decode_vts(r, 0)?,
            },
            2 => Sym::Helper(Helper::decode(r)?),
            3 => Sym::Part(r.hash(), Part::from_tag(r.u8())?),
            _ => return None,
        })
    }
}

/// While a code entry is encoded or decoded, its recursion groups' table:
/// each group the entry names, directly or through another group, once,
/// after the groups it names, so a type names a group by its place in the
/// table and no encoding nests one group in another.
enum Table {
    /// Finding the groups the entry names directly.
    Find(Vec<Arc<Group>>, HashSet<Hash128>),
    /// Each group's place.
    Enc(HashMap<Hash128, u32>),
    /// The groups decoded so far.
    Dec(Vec<Arc<Group>>),
}

thread_local! {
    static TABLE: std::cell::RefCell<Option<Table>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` with `table` as the current table; returns `f`'s result and
/// the table after it.
fn with_table<T>(table: Table, f: impl FnOnce() -> T) -> (T, Table) {
    let prev = TABLE.with(|c| c.replace(Some(table)));
    let out = f();
    let table = TABLE
        .with(|c| c.replace(prev))
        .expect("the table set above");
    (out, table)
}

/// A group's place in the current table (while finding, a placeholder).
fn table_index(g: &Arc<Group>) -> u32 {
    TABLE.with(|c| match c.borrow_mut().as_mut() {
        Some(Table::Find(found, seen)) => {
            if seen.insert(g.hash) {
                found.push(g.clone());
            }
            0
        }
        Some(Table::Enc(at)) => at[&g.hash],
        _ => unreachable!("a recursion group is encoded only within a code entry"),
    })
}

/// The decoded group at place `i`, if decoded already.
fn table_group(i: u32) -> Option<Arc<Group>> {
    TABLE.with(|c| match c.borrow().as_ref() {
        Some(Table::Dec(groups)) => groups.get(i as usize).cloned(),
        _ => None,
    })
}

/// The groups a type names, not looking into them.
fn groups_in(t: &WTy, out: &mut Vec<Arc<Group>>) {
    let vt = |v: &VT, out: &mut Vec<Arc<Group>>| {
        if let VT::Ref(t, _) = v {
            groups_in(t, out);
        }
    };
    match t {
        WTy::Rec(g, _) => out.push(g.clone()),
        WTy::Array(v) => vt(v, out),
        WTy::Struct { fields, sup, .. } => {
            for f in fields {
                vt(f, out);
            }
            if let Some(s) = sup {
                groups_in(s, out);
            }
        }
        WTy::Func(p, r) => {
            for x in p.iter().chain(r) {
                vt(x, out);
            }
        }
        WTy::Bytes | WTy::Back(_) => {}
    }
}

/// `roots` and every group they name, each after the groups it names
/// (depth first, with an explicit stack), leaving out the groups `known`
/// accepts and what only they name.
fn groups_after_parts(
    roots: Vec<Arc<Group>>,
    known: &dyn Fn(&Arc<Group>) -> bool,
) -> Vec<Arc<Group>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack: Vec<(Arc<Group>, bool)> = roots.into_iter().rev().map(|g| (g, false)).collect();
    while let Some((g, finished)) = stack.pop() {
        if finished {
            out.push(g);
            continue;
        }
        if !seen.insert(g.hash) || known(&g) {
            continue;
        }
        let mut parts = Vec::new();
        for m in g.iter() {
            groups_in(m, &mut parts);
        }
        stack.push((g, true));
        stack.extend(
            parts
                .into_iter()
                .rev()
                .filter(|p| !seen.contains(&p.hash))
                .map(|p| (p, false)),
        );
    }
    out
}

impl Drop for Group {
    /// Takes apart a chain of groups each held only by the one before it
    /// with a loop, not one nested drop per group.
    fn drop(&mut self) {
        let mut stack: Vec<Arc<Group>> = Vec::new();
        for m in std::mem::take(&mut self.members) {
            take_groups(m, &mut stack);
        }
        while let Some(g) = stack.pop() {
            if let Some(mut g) = Arc::into_inner(g) {
                for m in std::mem::take(&mut g.members) {
                    take_groups(m, &mut stack);
                }
            }
        }
    }
}

/// Moves the groups a type holds onto `out`, dropping the rest of it.
fn take_groups(t: WTy, out: &mut Vec<Arc<Group>>) {
    let vt = |v: VT, out: &mut Vec<Arc<Group>>| {
        if let VT::Ref(t, _) = v
            && let Some(t) = Arc::into_inner(t)
        {
            take_groups(t, out);
        }
    };
    match t {
        WTy::Rec(g, _) => out.push(g),
        WTy::Array(v) => vt(v, out),
        WTy::Struct { fields, sup, .. } => {
            for f in fields {
                vt(f, out);
            }
            if let Some(s) = sup {
                take_groups(*s, out);
            }
        }
        WTy::Func(p, r) => {
            for x in p.into_iter().chain(r) {
                vt(x, out);
            }
        }
        WTy::Bytes | WTy::Back(_) => {}
    }
}

impl Code {
    /// The code entry's bytes (codegen.md §13.8): ID-free. First the
    /// table of its recursion groups (`Table`), then the entry.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let ((), found) = with_table(Table::Find(Vec::new(), HashSet::new()), || {
            self.encode_into(&mut Writer::default());
        });
        let Table::Find(roots, _) = found else {
            unreachable!("the table set above")
        };
        let groups = groups_after_parts(roots, &|_| false);
        let at: HashMap<Hash128, u32> = groups
            .iter()
            .enumerate()
            .map(|(k, g)| (g.hash, u32::try_from(k).expect("groups")))
            .collect();
        let (bytes, _) = with_table(Table::Enc(at), || {
            let mut w = Writer::default();
            w.len_of(&groups);
            for g in &groups {
                w.len_of(&g.members);
                for m in g.iter() {
                    m.encode(&mut w);
                }
            }
            self.encode_into(&mut w);
            w.bytes
        });
        bytes
    }

    fn encode_into(&self, w: &mut Writer) {
        encode_vts(&self.params, w);
        encode_vts(&self.results, w);
        w.blob(&self.body);
        w.len_of(&self.relocs);
        for (at, r) in &self.relocs {
            w.u32(*at);
            match r {
                Reloc::Func(s) => {
                    w.u8(0);
                    s.encode(w);
                }
                Reloc::Type(t) => {
                    w.u8(1);
                    t.encode(w);
                }
                Reloc::Global(g) => {
                    w.u8(2);
                    g.encode(w);
                }
            }
        }
        w.len_of(&self.parts);
        for (p, c) in &self.parts {
            w.u8(p.tag());
            c.encode_into(w);
        }
    }

    /// Decodes a code entry; `None` when malformed (a cache miss).
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Code> {
        let mut r = Reader::new(bytes);
        let (c, _) = with_table(Table::Dec(Vec::new()), || {
            for _ in 0..r.count() {
                let n = u32::try_from(r.count()).ok()?;
                let members: Vec<WTy> = (0..n)
                    .map(|_| WTy::decode_in(&mut r, 0, Some(n)))
                    .collect::<Option<_>>()?;
                let g = Group::new(members);
                TABLE.with(|c| {
                    if let Some(Table::Dec(groups)) = c.borrow_mut().as_mut() {
                        groups.push(g);
                    }
                });
            }
            Self::decode_from(&mut r, 0)
        });
        r.ok().then_some(c?)
    }

    fn decode_from(r: &mut Reader<'_>, depth: u8) -> Option<Code> {
        let params = decode_vts(r, 0)?;
        let results = decode_vts(r, 0)?;
        let body = r.blob().to_vec();
        let n = r.count();
        let mut relocs = Vec::new();
        for _ in 0..n {
            let at = r.u32();
            let reloc = match r.u8() {
                0 => Reloc::Func(Sym::decode(r)?),
                1 => Reloc::Type(WTy::decode(r, 0)?),
                2 => Reloc::Global(GSym::decode(r)?),
                _ => return None,
            };
            if (at as usize) + 5 > body.len() {
                return None;
            }
            relocs.push((at, reloc));
        }
        let mut parts = Vec::new();
        let n = r.count();
        if depth > 0 && n != 0 {
            return None;
        }
        for _ in 0..n {
            let p = Part::from_tag(r.u8())?;
            parts.push((p, Self::decode_from(r, 1)?));
        }
        Some(Code {
            params,
            results,
            body,
            relocs,
            parts,
        })
    }
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Emit, what))
}

pub(crate) fn padded(out: &mut Vec<u8>, v: u32) {
    for k in 0..4 {
        out.push(u8::try_from((v >> (7 * k)) & 0x7f).expect("7 bits") | 0x80);
    }
    out.push(u8::try_from((v >> 28) & 0x7f).expect("7 bits"));
}

// ------------------------------------------------------------------- link

/// The type section under construction (wasm-layout.md §15.3): each type
/// once, its dependencies first. A type outside a recursion group is a
/// group of one; a `Rec` type's group is emitted whole, after the types
/// it names outside itself.
#[derive(Default)]
struct Types {
    idx: BTreeMap<WTy, u32>,
    /// Each emitted type by its resolved form, indices in place of named
    /// types: a type with the resolved form of a group's member is that
    /// member, so a descriptor built outside the group (a function's type
    /// from its parameters, a closure base from its code type) names the
    /// one Wasm type.
    resolved: HashMap<Resolved, u32>,
    sec: TypeSection,
    n: u32,
}

/// A type with its references resolved to indices.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Resolved {
    kind: u8,
    vals: Vec<RVal>,
    results: Vec<RVal>,
    sup: Option<u32>,
    open: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum RVal {
    I8,
    I32,
    I64,
    F32,
    F64,
    Eq,
    Ref(u32, bool),
}

impl Types {
    fn val(&mut self, v: &VT) -> ValType {
        rval_type(self.rval(v, &mut |_| None))
    }
    /// A value resolved; `inner` answers a group's own `Back` references.
    fn rval(&mut self, v: &VT, inner: &mut dyn FnMut(u32) -> Option<u32>) -> RVal {
        match v {
            VT::I32 => RVal::I32,
            VT::I64 => RVal::I64,
            VT::F32 => RVal::F32,
            VT::F64 => RVal::F64,
            VT::Eq => RVal::Eq,
            VT::Ref(t, n) => RVal::Ref(self.target(t, inner), *n),
        }
    }
    fn target(&mut self, t: &WTy, inner: &mut dyn FnMut(u32) -> Option<u32>) -> u32 {
        match t {
            WTy::Back(j) => inner(*j).expect("a `Back` inside its group"),
            t => self.of(t),
        }
    }
    /// The resolved form of a type (not a `Rec` or `Back`), its
    /// dependencies emitted first.
    fn resolve(&mut self, t: &WTy, inner: &mut dyn FnMut(u32) -> Option<u32>) -> Resolved {
        let mut r = Resolved {
            kind: 0,
            vals: Vec::new(),
            results: Vec::new(),
            sup: None,
            open: false,
        };
        match t {
            WTy::Bytes => r.vals.push(RVal::I8),
            WTy::Array(v) => {
                r.kind = 1;
                r.vals.push(self.rval(v, inner));
            }
            WTy::Struct { fields, sup, open } => {
                r.kind = 2;
                r.sup = sup.as_ref().map(|s| self.target(s, inner));
                r.vals = fields.iter().map(|f| self.rval(f, inner)).collect();
                r.open = *open;
            }
            WTy::Func(p, rs) => {
                r.kind = 3;
                r.vals = p.iter().map(|v| self.rval(v, inner)).collect();
                r.results = rs.iter().map(|v| self.rval(v, inner)).collect();
            }
            WTy::Rec(..) | WTy::Back(_) => unreachable!("a group member is resolved in its group"),
        }
        r
    }
    fn of(&mut self, t: &WTy) -> u32 {
        if let Some(&i) = self.idx.get(t) {
            return i;
        }
        let i = match t {
            WTy::Rec(g, k) => {
                self.group(g);
                return self.idx[&WTy::Rec(g.clone(), *k)];
            }
            WTy::Back(_) => unreachable!("a `Back` outside its group"),
            t => {
                let r = self.resolve(t, &mut |_| None);
                if let Some(&i) = self.resolved.get(&r) {
                    i
                } else {
                    self.sec.ty().subtype(&subtype(&r));
                    let i = self.n;
                    self.n += 1;
                    self.resolved.insert(r, i);
                    i
                }
            }
        };
        self.idx.insert(t.clone(), i);
        i
    }
    /// Emits a recursion group: the types its members name outside it,
    /// then its members in their canonical order.
    fn group(&mut self, g: &Arc<Group>) {
        // The groups it names first, each after the groups it names, in a
        // loop: emitting one then finds the groups it names emitted.
        let idx = &self.idx;
        let order = groups_after_parts(vec![g.clone()], &|p| {
            idx.contains_key(&WTy::Rec(p.clone(), 0))
        });
        for p in order {
            self.emit_group(&p);
        }
    }

    /// Emits a group whose named groups are emitted.
    fn emit_group(&mut self, g: &Arc<Group>) {
        // A group of one that names no member is an ordinary type (the
        // Wasm form of a type outside `rec` is that group).
        if let [m] = &g[..]
            && !layout::has_back(m)
        {
            let i = self.of(m);
            self.idx.insert(WTy::Rec(g.clone(), 0), i);
            return;
        }
        // Outside types first: resolving with a placeholder for the
        // group's own members emits them.
        for m in g.iter() {
            let _ = self.resolve(m, &mut |_| Some(0));
        }
        let base = self.n;
        let len = u32::try_from(g.len()).expect("group");
        let rs: Vec<Resolved> = g
            .iter()
            .map(|m| self.resolve(m, &mut |j| (j < len).then_some(base + j)))
            .collect();
        self.sec
            .ty()
            .rec(rs.iter().map(subtype).collect::<Vec<_>>());
        self.n += len;
        for (j, r) in rs.into_iter().enumerate() {
            let at = base + u32::try_from(j).expect("group");
            self.resolved.entry(r).or_insert(at);
            self.idx.insert(WTy::Rec(g.clone(), at - base), at);
        }
    }
}

fn rval_type(v: RVal) -> ValType {
    match v {
        RVal::I32 | RVal::I8 => ValType::I32,
        RVal::I64 => ValType::I64,
        RVal::F32 => ValType::F32,
        RVal::F64 => ValType::F64,
        RVal::Eq => ValType::Ref(RefType::EQREF),
        RVal::Ref(i, nullable) => ValType::Ref(RefType {
            nullable,
            heap_type: HeapType::Concrete(i),
        }),
    }
}

fn subtype(r: &Resolved) -> SubType {
    let field = |v: &RVal| FieldType {
        element_type: if *v == RVal::I8 {
            StorageType::I8
        } else {
            StorageType::Val(rval_type(*v))
        },
        mutable: true,
    };
    let inner = match r.kind {
        0 | 1 => CompositeInnerType::Array(wasm_encoder::ArrayType(field(&r.vals[0]))),
        2 => CompositeInnerType::Struct(StructType {
            fields: r.vals.iter().map(field).collect::<Vec<_>>().into(),
        }),
        _ => CompositeInnerType::Func(FuncType::new(
            r.vals.iter().map(|v| rval_type(*v)),
            r.results.iter().map(|v| rval_type(*v)),
        )),
    };
    SubType {
        is_final: !r.open,
        supertype_idxs: r.sup.into_iter().collect(),
        composite_type: CompositeType {
            inner,
            shared: false,
            descriptor: None,
            describes: None,
        },
    }
}

/// `Link(P)`: helpers, index assignment, the type section, the literal
/// pool, globals, relocation patching (codegen.md §13.10). Inputs are in
/// content order, so the bytes are deterministic. `exports` are the entry
/// functions (`hd.init`, `hd.poll`, `hd.wake`); `names` are the
/// instances' item paths for the dev `name` section.
pub fn link(
    codes: &[(Hash128, Code)],
    names: &[String],
    exports: &[(String, Helper)],
) -> StageResult<Vec<u8>> {
    // Every function body: instances, then their parts.
    let mut all: Vec<(Sym, &Code, String)> = Vec::new();
    for ((k, c), n) in codes.iter().zip(names) {
        all.push((Sym::Inst(*k), c, n.clone()));
        for (p, pc) in &c.parts {
            all.push((Sym::Part(*k, *p), pc, format!("{n}${p:?}")));
        }
    }
    // Helpers, imports and globals reachable from the code, to a fixed point.
    let mut helpers: BTreeMap<Helper, Code> = BTreeMap::new();
    let mut imports: BTreeSet<(String, String, Vec<VT>, Vec<VT>)> = BTreeSet::new();
    let mut gsyms: BTreeSet<GSym> = BTreeSet::new();
    let mut todo: Vec<Helper> = exports.iter().map(|e| e.1.clone()).collect();
    let mut scan = |c: &Code, todo: &mut Vec<Helper>| {
        for (_, r) in &c.relocs {
            match r {
                Reloc::Func(Sym::Helper(h)) => todo.push(h.clone()),
                Reloc::Func(Sym::Import {
                    module,
                    name,
                    params,
                    results,
                }) => {
                    imports.insert((
                        module.clone(),
                        name.clone(),
                        params.clone(),
                        results.clone(),
                    ));
                }
                Reloc::Global(g) => {
                    gsyms.insert(g.clone());
                }
                _ => {}
            }
        }
    };
    for (_, c, _) in &all {
        scan(c, &mut todo);
    }
    while let Some(h) = todo.pop() {
        if helpers.contains_key(&h) {
            continue;
        }
        let c = rt::helper_code(&h)?;
        scan(&c, &mut todo);
        helpers.insert(h, c);
    }
    // Literals: one passive segment, deduplicated by content.
    let mut lits: BTreeMap<Vec<u8>, (u32, u32)> = BTreeMap::new();
    let mut data = Vec::new();
    for h in helpers.keys() {
        if let Helper::Lit(bytes) = h {
            let off = u32::try_from(data.len()).expect("data");
            data.extend_from_slice(bytes);
            lits.insert(
                bytes.clone(),
                (off, u32::try_from(bytes.len()).expect("lit")),
            );
        }
    }
    let mut types = Types::default();
    let mut imps = ImportSection::new();
    let mut func_idx: BTreeMap<Sym, u32> = BTreeMap::new();
    let mut nfuncs = 0u32;
    for (module, name, params, results) in &imports {
        let t = types.of(&WTy::Func(params.clone(), results.clone()));
        imps.import(module, name, EntityType::Function(t));
        func_idx.insert(
            Sym::Import {
                module: module.clone(),
                name: name.clone(),
                params: params.clone(),
                results: results.clone(),
            },
            nfuncs,
        );
        nfuncs += 1;
    }
    let mut bodies: Vec<&Code> = Vec::new();
    for (h, c) in &helpers {
        func_idx.insert(Sym::Helper(h.clone()), nfuncs);
        nfuncs += 1;
        bodies.push(c);
    }
    for (sym, c, _) in &all {
        func_idx.insert(sym.clone(), nfuncs);
        nfuncs += 1;
        bodies.push(c);
    }
    // Globals: one lazily filled cell per literal (wasm-layout.md §15.4),
    // then module storage and runtime globals, by symbol order.
    let mut globals = GlobalSection::new();
    let mut lit_global: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
    for (i, bytes) in lits.keys().enumerate() {
        let t = types.of(&WTy::Bytes);
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(t),
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(HeapType::Concrete(t)),
        );
        lit_global.insert(bytes.clone(), u32::try_from(i).expect("globals"));
    }
    let mut global_idx: BTreeMap<GSym, u32> = BTreeMap::new();
    for g in &gsyms {
        let vt = g.vt().dflt();
        let val_type = types.val(&vt);
        let init = match &vt {
            VT::I32 => ConstExpr::i32_const(0),
            VT::I64 => ConstExpr::i64_const(0),
            VT::F32 => ConstExpr::f32_const(0.0_f32.into()),
            VT::F64 => ConstExpr::f64_const(0.0_f64.into()),
            VT::Eq => ConstExpr::ref_null(HeapType::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Eq,
            }),
            VT::Ref(t, _) => ConstExpr::ref_null(HeapType::Concrete(types.of(t))),
        };
        globals.global(
            GlobalType {
                val_type,
                mutable: true,
                shared: false,
            },
            &init,
        );
        global_idx.insert(
            g.clone(),
            u32::try_from(lits.len() + global_idx.len()).expect("globals"),
        );
    }
    let mut funcs = FunctionSection::new();
    let mut code_sec = CodeSection::new();
    let mut declared = BTreeSet::new();
    let helper_list: Vec<&Helper> = helpers.keys().collect();
    for (n, c) in bodies.iter().enumerate() {
        let t = types.of(&WTy::Func(c.params.clone(), c.results.clone()));
        funcs.function(t);
        let lit;
        let c = if let Some(Helper::Lit(bytes)) = helper_list.get(n) {
            let (off, len) = lits[bytes];
            lit = rt::lit_code(off, len, lit_global[bytes]);
            &lit
        } else {
            *c
        };
        let mut body = c.body.clone();
        for (at, r) in &c.relocs {
            let v = match r {
                Reloc::Func(s) => {
                    let Some(&f) = func_idx.get(s) else {
                        return unsupported(format!("a relocation with no target: {s:?}"));
                    };
                    if body.get(*at as usize - 1) == Some(&0xd2) {
                        declared.insert(f);
                    }
                    f
                }
                Reloc::Type(t) => types.of(t),
                Reloc::Global(g) => {
                    let Some(&i) = global_idx.get(g) else {
                        return unsupported(format!("a global relocation with no global: {g:?}"));
                    };
                    i
                }
            };
            let mut b = Vec::new();
            padded(&mut b, v);
            body[*at as usize..*at as usize + 5].copy_from_slice(&b);
        }
        code_sec.raw(&body);
    }
    let mut export_sec = ExportSection::new();
    for (name, h) in exports {
        let Some(&f) = func_idx.get(&Sym::Helper(h.clone())) else {
            return unsupported("an entry export has no code");
        };
        export_sec.export(name, ExportKind::Func, f);
    }
    let mut mems = MemorySection::new();
    if !imports.is_empty() {
        mems.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        export_sec.export("hd.x", ExportKind::Memory, 0);
    }
    let mut elems = ElementSection::new();
    if !declared.is_empty() {
        let fs: Vec<u32> = declared.into_iter().collect();
        elems.declared(Elements::Functions(fs.into()));
    }
    let mut module = Module::new();
    module.section(&types.sec);
    module.section(&imps);
    module.section(&funcs);
    if !imports.is_empty() {
        module.section(&mems);
    }
    module.section(&globals);
    module.section(&export_sec);
    module.section(&elems);
    module.section(&DataCountSection { count: 1 });
    module.section(&code_sec);
    let mut ds = DataSection::new();
    ds.passive(data);
    module.section(&ds);
    // The dev pipeline's standard `name` section (codegen.md §13.10 step 9).
    let mut fnames = wasm_encoder::NameMap::new();
    let nh = u32::try_from(helpers.len()).expect("helpers");
    let ni = u32::try_from(imports.len()).expect("imports");
    for (k, (module_name, name, _, _)) in imports.iter().enumerate() {
        fnames.append(
            u32::try_from(k).expect("k"),
            &format!("{module_name}/{name}"),
        );
    }
    for (k, h) in helper_list.iter().enumerate() {
        let text = format!("{h:?}");
        let short: String = text.chars().take(60).collect();
        fnames.append(ni + u32::try_from(k).expect("k"), &format!("rt:{short}"));
    }
    for (k, (_, _, n)) in all.iter().enumerate() {
        fnames.append(ni + nh + u32::try_from(k).expect("k"), n);
    }
    let mut ns = wasm_encoder::NameSection::new();
    ns.functions(&fnames);
    module.section(&ns);
    Ok(module.finish())
}

#[cfg(test)]
mod tests {
    use super::{Code, Group, Reloc, Types, VT, WTy};
    use std::sync::Arc;

    /// `{next: (ref null $F)}` and `$F = (eqref) -> (ref null $S)`, one group.
    fn group() -> Arc<Group> {
        Group::new(vec![
            WTy::Struct {
                fields: vec![VT::rn(WTy::Back(1))],
                sup: None,
                open: false,
            },
            WTy::Func(vec![VT::Eq], vec![VT::rn(WTy::Back(0))]),
        ])
    }

    #[test]
    fn recursion_groups_round_trip_and_reject_stray_backs() {
        let code = Code {
            params: vec![VT::r(WTy::Rec(group(), 0))],
            results: vec![],
            body: vec![0; 5],
            relocs: vec![(0, Reloc::Type(WTy::Rec(group(), 1)))],
            parts: vec![],
        };
        assert_eq!(Code::decode(&code.encode()), Some(code));
        let stray = Code {
            params: vec![VT::r(WTy::Back(0))],
            results: vec![],
            body: vec![],
            relocs: vec![],
            parts: vec![],
        };
        assert_eq!(Code::decode(&stray.encode()), None);
    }

    #[test]
    fn a_group_is_emitted_once_and_its_members_unrolled_are_the_members() {
        let mut t = Types::default();
        let g = group();
        let s = t.of(&WTy::Rec(g.clone(), 0));
        let f = t.of(&WTy::Rec(g.clone(), 1));
        assert_eq!((s, f), (0, 1));
        // The function type built from its parameters outside the group,
        // and the struct opened, name the members.
        let outside = WTy::Func(vec![VT::Eq], vec![VT::rn(WTy::Rec(g.clone(), 0))]);
        assert_eq!(t.of(&outside), f);
        assert_eq!(t.of(&WTy::Rec(g.clone(), 0).unrolled()), s);
        assert_eq!(WTy::Rec(g, 0).fields(), [VT::rn(WTy::Rec(group(), 1))]);
        assert_eq!(t.n, 2);
    }
}
