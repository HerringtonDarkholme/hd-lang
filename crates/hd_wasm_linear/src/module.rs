//! The module under construction: functions with symbolic call targets,
//! the imports they name, globals, the function table, static vtables
//! and static data (`crates/hd_wasm_linear/DESIGN.md` §23.2, §23.8, §23.9).
//! [`Module::finish`] builds every helper a body names, numbers imports
//! first, then instances in content order, then helpers in their order,
//! and encodes the module.

use crate::op::{LabelListId, Op as Instruction};
use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use hd_base::{Hash128, NotImplemented, Stage, StageResult};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ElementSection, Elements, EntityType, ExportKind,
    ExportSection, Function, FunctionSection, GlobalSection, GlobalType, ImportSection,
    Instruction as WasmInstruction, MemorySection, MemoryType, NameMap, NameSection, RefType,
    TableSection, TableType, TypeSection, ValType,
};

use crate::rt::Helper;

/// The exchange buffer `hd.x`, the host contract's memory
/// (`crates/hd_run_wasmtime/DESIGN.md` §17.3).
pub const BUF: u32 = 0;

/// The heap memory (`crates/hd_wasm_linear/DESIGN.md` §23.2).
pub const HEAP: u32 = 1;

/// The shadow stack's memory (`crates/hd_wasm_linear/DESIGN.md` §23.29),
/// declared only when a signature or a wide body needs it.
pub const STACK: u32 = 2;

/// The shadow stack's pages: it grows down from their end, and a frame
/// below 0 wraps to an address past it, which traps.
pub const STACK_PAGES: u32 = 16;

/// The first address of static data: addresses below it are never used,
/// so 0 stays null.
pub const STATIC_BASE: u32 = 16;

/// The address of the one empty string (§23.7): every string of no bytes
/// is this view, whose bytes are never read.
pub const EMPTY: u32 = STATIC_BASE;

/// The size of a counted object's header, before its payload (§23.4).
pub const HEADER: u32 = 8;
/// The bytes of the collector word at the end of a block that takes part
/// in cycle collection (`crates/hd_wasm_linear/DESIGN.md` §23.28).
pub const CYCLE_WORD: u32 = 4;
/// The descriptor word's bit that marks an object of a cycle-capable type,
/// set at its allocation: only such objects take part in cycle collection
/// and carry a collector word (§23.28).
pub const CYCLIC: u32 = 1 << 31;
/// The descriptor word's bit that marks an object in the collector's
/// candidate buffer, which `std.rt.cycle` sets and clears.
pub const BUFFERED: u32 = 1 << 30;
/// First bit of the constant allocator class in the descriptor word.
pub const CLASS_SHIFT: u32 = 25;
/// Five descriptor bits carry a constant allocator class (zero is unknown).
pub const CLASS_MASK: u32 = 31 << CLASS_SHIFT;
/// The descriptor's release glue: its table slot plus one, zero for none.
pub const GLUE: u32 = (1 << CLASS_SHIFT) - 1;

/// A function: an instance by key, a runtime helper, or a host import.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sym {
    Inst(Hash128),
    /// A part of a suspending instance, beside its cold constructor.
    Part(Hash128, Part),
    Helper(Helper),
    Import(Import),
}

/// A function's dense index in its module's symbol table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymId(pub u32);
impl SymId {
    pub const fn idx(self) -> usize {
        self.0 as usize
    }
}

/// An indirect call's signature, interned independently of Wasm numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlobalId(pub u32);

/// One shared name/signature arena for a module and its function builders.
#[derive(Clone, Debug, Default)]
pub struct Symbols(Rc<RefCell<SymbolData>>);
#[derive(Debug, Default)]
struct SymbolData {
    globals: Vec<Global>,
    global_ids: hd_base::HashMap<Global, GlobalId>,
    names: Vec<Sym>,
    ids: hd_base::HashMap<Sym, SymId>,
    signatures: Vec<(Vec<Val>, Vec<Val>)>,
    signature_ids: BTreeMap<(Vec<Val>, Vec<Val>), SigId>,
}
impl Symbols {
    pub fn intern_global(&self, global: Global) -> GlobalId {
        let mut data = self.0.borrow_mut();
        if let Some(&id) = data.global_ids.get(&global) {
            return id;
        }
        let id = GlobalId(u32::try_from(data.globals.len()).expect("globals"));
        data.globals.push(global);
        data.global_ids.insert(global, id);
        id
    }
    pub fn global(&self, id: GlobalId) -> Global {
        self.0.borrow().globals[id.0 as usize]
    }
    pub fn len(&self) -> usize {
        self.0.borrow().names.len()
    }
    pub fn intern(&self, symbol: Sym) -> SymId {
        let mut data = self.0.borrow_mut();
        if let Some(&id) = data.ids.get(&symbol) {
            return id;
        }
        let id = SymId(u32::try_from(data.names.len()).expect("symbols"));
        data.names.push(symbol.clone());
        data.ids.insert(symbol, id);
        id
    }
    pub fn get(&self, id: SymId) -> Ref<'_, Sym> {
        Ref::map(self.0.borrow(), |data| &data.names[id.idx()])
    }
    pub fn cmp(&self, a: SymId, b: SymId) -> std::cmp::Ordering {
        let data = self.0.borrow();
        data.names[a.idx()].cmp(&data.names[b.idx()])
    }
    pub fn intern_signature(&self, params: Vec<Val>, results: Vec<Val>) -> SigId {
        let mut data = self.0.borrow_mut();
        let signature = (params, results);
        if let Some(&id) = data.signature_ids.get(&signature) {
            return id;
        }
        let id = SigId(u32::try_from(data.signatures.len()).expect("signatures"));
        data.signatures.push(signature.clone());
        data.signature_ids.insert(signature, id);
        id
    }
    pub fn signature(&self, id: SigId) -> Ref<'_, (Vec<Val>, Vec<Val>)> {
        Ref::map(self.0.borrow(), |data| &data.signatures[id.0 as usize])
    }
}

/// Membership in a reachability walk, one bit per symbol.
#[derive(Default)]
struct Reached(Vec<u64>);
impl Reached {
    fn insert(&mut self, id: SymId) -> bool {
        let word = id.idx() / 64;
        self.0.resize(self.0.len().max(word + 1), 0);
        let mask = 1 << (id.0 % 64);
        let fresh = self.0[word] & mask == 0;
        self.0[word] |= mask;
        fresh
    }
    fn contains(&self, id: SymId) -> bool {
        self.0
            .get(id.idx() / 64)
            .is_some_and(|w| w & (1 << (id.0 % 64)) != 0)
    }
    fn iter(&self) -> impl Iterator<Item = SymId> + '_ {
        self.0.iter().enumerate().flat_map(|(word, &bits)| {
            let mut bits = bits;
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros();
                bits &= bits - 1;
                Some(SymId(u32::try_from(word * 64).expect("symbols") + bit))
            })
        })
    }
}

/// Dense optional numbering, preserving the semantic order chosen by linking.
#[derive(Default)]
struct Numbers(Vec<Option<u32>>);
impl Numbers {
    fn new() -> Self {
        Self::default()
    }
    fn insert(&mut self, id: SymId, number: u32) {
        self.0.resize(self.0.len().max(id.idx() + 1), None);
        self.0[id.idx()] = Some(number);
    }
    fn get(&self, id: SymId) -> Option<&u32> {
        self.0.get(id.idx())?.as_ref()
    }
}
impl std::ops::Index<&SymId> for Numbers {
    type Output = u32;
    fn index(&self, id: &SymId) -> &u32 {
        self.get(*id).expect("numbered symbol")
    }
}

#[derive(Clone, Copy, Debug)]
enum Edge {
    Function(SymId),
    VTable(usize),
}

/// A suspending instance's part (`crates/hd_wasm_linear/DESIGN.md` §23.21),
/// defined beside its cold constructor, the instance's own function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// `(frame, arguments, providers) -> (results, frame)`: the body. A
    /// frame of 0 starts it fresh; any other resumes it. A result frame of
    /// 0 is Ready, any other Pending.
    Body,
    /// `(frame) -> (ready, results)`: the poll.
    Poll,
    /// `(frame)`: the cancel.
    Cancel,
    /// `(payload)`: the frame's release glue.
    Drop,
}

/// A host import (`crates/hd_run_wasmtime/DESIGN.md` §17.2): its module,
/// name and signature.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub params: Vec<Val>,
    pub results: Vec<Val>,
}

impl Import {
    #[must_use]
    pub fn new(module: &str, name: &str, params: &[Val], results: &[Val]) -> Self {
        Import {
            module: module.to_owned(),
            name: name.to_owned(),
            params: params.to_vec(),
            results: results.to_vec(),
        }
    }
}

/// A Wasm value type of this backend: scalars only, since every reference
/// is an `i32` address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Val {
    I32,
    I64,
    F32,
    F64,
}

impl Val {
    #[must_use]
    pub const fn wasm(self) -> ValType {
        match self {
            Val::I32 => ValType::I32,
            Val::I64 => ValType::I64,
            Val::F32 => ValType::F32,
            Val::F64 => ValType::F64,
        }
    }
}

/// A runtime global (`crates/hd_wasm_linear/DESIGN.md` §23.9), or one Wasm
/// value of a module storage binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Global {
    /// The forbidden-context counter (`req.drive.block-on.forbidden-contexts`).
    Forbid,
    /// The first free heap address of the bump allocation that serves
    /// until the std allocator (job 5) takes over.
    HeapTop,
    /// The end of the pages that bump allocation owns: at first the heap's
    /// initial size, then the end of the pages it last grew.
    HeapEnd,
    /// The run's map hash key (`map_hash_seed`): 0 until the first map
    /// key is hashed, which asks the host's `hash_seed` import.
    HashSeed,
    /// The `abandon()` depth: cancels under it abandon (§23.21).
    Aband,
    /// The `ABANDONED` bits that a frame's cancels saw.
    AbSeen,
    /// Set when an abandon, an aborted host call, or a `block_on` that took
    /// host completions leaves a party that no wake will reach: the driver
    /// polls its root again.
    Rewake,
    /// The entry's pending root frame, 0 for none.
    Root,
    /// The `hd.poll` context that owns the root, plus one.
    Owner,
    /// The wake table: its address and its capacity in handles.
    WakeTab,
    WakeCap,
    /// Weak waker registry: raw table, capacity, used slots, free head.
    NodeTab,
    NodeCap,
    NodeLen,
    NodeFree,
    /// The enclosing all! node and child bit while a child is polled.
    Waker,
    WakeBit,
    /// Child polls executed by all!, in dev builds only.
    AllPolls,
    /// Dead blocks awaiting their payload release, linked through their
    /// no-longer-counted header word (#625).
    ReleaseHead,
    /// One outer release drains `ReleaseHead`; nested releases only enqueue.
    Releasing,
    /// Set while the cycle collector traces an object, so that a release
    /// goes to the collector's `visit` instead of counting (§23.28).
    Tracing,
    /// The shadow stack pointer: the lowest address of the newest frame
    /// (§23.29).
    Stack,
    /// One value of a top-level binding: its item's path hash and the
    /// value's index in its layout.
    Storage(Hash128, u32, Val),
}

impl Global {
    const fn val(&self) -> Val {
        match self {
            Global::Forbid
            | Global::HeapTop
            | Global::HeapEnd
            | Global::Aband
            | Global::AbSeen
            | Global::Rewake
            | Global::Root
            | Global::Owner
            | Global::WakeTab
            | Global::WakeCap
            | Global::NodeTab
            | Global::NodeCap
            | Global::NodeLen
            | Global::NodeFree
            | Global::Waker
            | Global::WakeBit
            | Global::AllPolls
            | Global::ReleaseHead
            | Global::Releasing
            | Global::Tracing
            | Global::Stack => Val::I32,
            Global::HashSeed => Val::I64,
            Global::Storage(_, _, v) => *v,
        }
    }
}

/// One instruction of a body: a plain one, or one whose immediate
/// [`Module::finish`] resolves.
#[derive(Clone, Copy, Debug)]
pub enum Ins {
    Op(Instruction),
    Call(SymId),
    GlobalGet(GlobalId),
    GlobalSet(GlobalId),
    /// `i32.const` of the function's slot in the function table.
    TableSlot(SymId),
    /// A checked descriptor alias: a distinct slot, the same release glue.
    #[cfg(feature = "checked-report")]
    Descriptor(SymId, u8),
    /// `call_indirect` through the function table, at this signature.
    CallIndirect(SigId),
    /// `i32.const` of a static vtable's address ([`Module::vtable`]).
    VTable(usize),
    /// A suspending body's store of every local into its frame, which
    /// the body's emission expands once its locals are known.
    SaveAll,
    /// The loads of every local from the frame.
    LoadAll,
    /// The TIR instruction that anchors the calls after it: a row of the
    /// `hd.lines` table at each call that starts a new anchor
    /// (`crates/hd_wasm_linear/DESIGN.md` §15.9). [`Func::call`] writes it just
    /// before a call whose anchor is new; it writes no code.
    Anchor(u32),
}

// The instruction vector is copied by the local passes; names and variable
// length operands must stay in their arenas instead of growing each element.
const _: () = assert!(std::mem::size_of::<Ins>() <= 56);

/// One function body before numbering.
#[derive(Clone, Debug, Default)]
pub struct Func {
    pub symbols: Symbols,
    pub labels: Vec<Vec<u32>>,
    callees: Vec<Edge>,
    pub name: String,
    pub params: Vec<Val>,
    pub results: Vec<Val>,
    /// The locals after the parameters.
    pub locals: Vec<Val>,
    /// A body materializes a value wider than the direct-value bound.
    pub stack_values: bool,
    pub body: Vec<Ins>,
    /// Parallel to `body`, enabled only for development source mapping.
    pub origins: Option<Vec<Option<u32>>>,
    pub origin_at: Option<u32>,
    pub origin_item: Option<String>,
    pub origin_root: Option<u32>,
    pub origins_before_opt: Vec<Option<u32>>,
    /// With origins: each local's development name, by index, as
    /// [`crate::FunctionOrigins::locals`] reports it.
    pub local_names: Option<BTreeMap<u32, String>>,
    /// With origins: an instance's type arguments, as
    /// [`crate::FunctionOrigins::type_args`] reports them.
    pub origin_args: Vec<String>,
    /// With origins: the static data addresses the body pushes.
    pub statics: Option<BTreeSet<u32>>,
    count_origin_base: Option<u32>,
    count_origins: Vec<crate::views::CountOrigin>,
    count_sites: u32,
    /// A complete relocation-free body, including locals and the final end.
    pub raw: Option<Vec<u8>>,
    /// The TIR instruction whose code is being written, when it has a
    /// source position: it anchors the calls written now.
    pub at: Option<u32>,
    /// The anchor the last [`Ins::Anchor`] wrote.
    anchored: Option<u32>,
    /// The inlined frames an anchor `INLINED | k` names: entry `k` is the
    /// callee's stable path, its instruction and the call's anchor
    /// (`hd_tir::ir::Body::inlined`).
    pub inlined: Vec<(String, u32, u32)>,
}

impl Func {
    #[must_use]
    fn new(symbols: Symbols, name: impl Into<String>, params: Vec<Val>, results: Vec<Val>) -> Self {
        Func {
            symbols,
            name: name.into(),
            stack_values: params.len() > crate::wide::WIDTH || results.len() > crate::wide::WIDTH,
            params,
            results,
            ..Func::default()
        }
    }

    pub fn intern(&self, s: Sym) -> SymId {
        self.symbols.intern(s)
    }
    pub fn intern_global(&self, global: Global) -> GlobalId {
        self.symbols.intern_global(global)
    }
    pub fn symbol(&self, id: SymId) -> Ref<'_, Sym> {
        self.symbols.get(id)
    }
    pub fn signature(&self, id: SigId) -> Ref<'_, (Vec<Val>, Vec<Val>)> {
        self.symbols.signature(id)
    }
    pub fn table_slot(&mut self, s: Sym) {
        let id = self.intern(s);
        self.push(Ins::TableSlot(id));
    }
    pub fn intern_labels(&mut self, labels: Vec<u32>) -> LabelListId {
        let id = LabelListId(u32::try_from(self.labels.len()).expect("branch tables"));
        self.labels.push(labels);
        id
    }
    pub fn label_list(&self, id: LabelListId) -> &[u32] {
        &self.labels[id.0 as usize]
    }
    pub fn br_table(&mut self, labels: Vec<u32>, default: u32) {
        let id = self.intern_labels(labels);
        self.op(Instruction::BrTable(id, default));
    }
    fn edge(ins: Ins) -> Option<Edge> {
        match ins {
            Ins::Call(s) | Ins::TableSlot(s) => Some(Edge::Function(s)),
            #[cfg(feature = "checked-report")]
            Ins::Descriptor(s, _) => Some(Edge::Function(s)),
            Ins::VTable(k) => Some(Edge::VTable(k)),
            _ => None,
        }
    }
    pub fn rebuild_callees(&mut self) {
        self.callees.clear();
        self.callees
            .extend(self.body.iter().copied().filter_map(Self::edge));
    }

    pub fn set_origin_source_len(&mut self, len: usize) {
        self.count_origin_base = Some(u32::try_from(len).expect("TIR instructions"));
    }

    pub fn source_origin(&self, origin: Option<u32>) -> Option<u32> {
        origin.map(|o| self.count_origin(o).map_or(o, |n| n.origin))
    }

    fn count_origin(&self, origin: u32) -> Option<&crate::views::CountOrigin> {
        self.count_origins
            .get(origin.checked_sub(self.count_origin_base?)? as usize)
    }

    /// Annotates just emitted count code using opaque source-map tokens. A
    /// merged/expanded instruction keeps its label through the origin remapper.
    pub fn note_count(
        &mut self,
        start: usize,
        source_len: u32,
        action: crate::views::CountAction,
        subject: &str,
    ) {
        if self.origins.is_none() || start == self.body.len() {
            return;
        }
        let base = *self.count_origin_base.get_or_insert(source_len);
        assert_eq!(base, source_len);
        let site = self.count_sites;
        self.count_sites += 1;
        let mut tokens = BTreeMap::new();
        for k in start..self.body.len() {
            let Some(original) = self.source_origin(self.origins.as_ref().expect("mapped")[k])
            else {
                continue;
            };
            assert!(
                original < base,
                "real source instruction precedes metadata tokens"
            );
            let token = *tokens.entry(original).or_insert_with(|| {
                let token = base
                    .checked_add(u32::try_from(self.count_origins.len()).expect("origin notes"))
                    .expect("origin token");
                self.count_origins.push(crate::views::CountOrigin {
                    origin: original,
                    site,
                    action,
                    subject: subject.to_owned(),
                });
                token
            });
            self.origins.as_mut().expect("mapped")[k] = Some(token);
        }
    }

    /// Sets local `l`, a parameter or a local, to its type's zero.
    pub fn zero(&mut self, l: u32) {
        let at = l as usize;
        let v = match self.params.get(at) {
            Some(v) => *v,
            None => self.locals[at - self.params.len()],
        };
        self.op(match v {
            Val::I32 => Instruction::I32Const(0),
            Val::I64 => Instruction::I64Const(0),
            Val::F32 => Instruction::F32Const(0.0.into()),
            Val::F64 => Instruction::F64Const(0.0.into()),
        });
        self.set(l);
    }

    /// Names local `l` for development tools, when they are tracked.
    pub fn name_local(&mut self, l: u32, name: impl Into<String>) {
        if let Some(names) = &mut self.local_names {
            names.insert(l, name.into());
        }
    }

    /// Records that the body pushes `at` as a static data address, when
    /// development tools track it.
    pub fn mark_static(&mut self, at: u32) {
        if let Some(statics) = &mut self.statics {
            statics.insert(at);
        }
    }

    /// Pushes static data address `at`.
    pub fn static_addr(&mut self, at: u32) {
        self.mark_static(at);
        self.u32(at);
    }

    /// A fresh local of type `v`: its index.
    pub fn local(&mut self, v: Val) -> u32 {
        self.locals.push(v);
        u32::try_from(self.params.len() + self.locals.len() - 1).expect("locals")
    }

    pub fn push(&mut self, ins: Ins) {
        if let Some(origins) = &mut self.origins {
            assert_eq!(origins.len(), self.body.len());
            origins.push(self.origin_at);
        }
        let action = if self.origins.is_some() {
            match &ins {
                Ins::Call(s) => crate::views::count_action(&self.symbols.get(*s), None),
                _ => None,
            }
        } else {
            None
        };
        let start = self.body.len();
        if let Some(edge) = Self::edge(ins) {
            self.callees.push(edge);
        }
        self.body.push(ins);
        // Label raw emitter helper calls too. Typed counting overwrites this
        // fallback with its source type; inlining preserves the same site.
        if let (Some(action), Some(base), Some(origin)) =
            (action, self.count_origin_base, self.origin_at)
            && self.count_origin(origin).is_none()
        {
            self.note_count(start, base, action, "counted reference");
        }
    }

    pub fn drain(&mut self, range: std::ops::Range<usize>) {
        if let Some(origins) = &mut self.origins {
            origins.drain(range.clone());
        }
        self.body.drain(range);
    }

    pub fn remove(&mut self, index: usize) {
        if let Some(origins) = &mut self.origins {
            origins.remove(index);
        }
        self.body.remove(index);
    }

    pub fn op(&mut self, i: Instruction) {
        self.push(Ins::Op(i));
    }

    pub fn call(&mut self, s: Sym) {
        self.anchor();
        let id = self.intern(s);
        self.push(Ins::Call(id));
    }

    /// `call_indirect` through the function table, at this signature.
    pub fn call_indirect(&mut self, params: Vec<Val>, results: Vec<Val>) {
        self.anchor();
        let id = self.symbols.intern_signature(params, results);
        self.push(Ins::CallIndirect(id));
    }

    /// Writes [`Ins::Anchor`] before a call whose anchor is new.
    fn anchor(&mut self) {
        if let Some(at) = self.at
            && self.anchored != self.at
        {
            self.push(Ins::Anchor(at));
            self.anchored = self.at;
        }
    }

    pub fn helper(&mut self, h: Helper) {
        // A panic helper never returns: say so at the call site, so the
        // engine need not treat the call as returning and keep state live
        // across it (#165).
        let diverges = matches!(h, Helper::Panic(_) | Helper::PanicStr | Helper::PanicWith);
        self.call(Sym::Helper(h));
        if diverges {
            self.op(Instruction::Unreachable);
        }
    }

    pub fn i32(&mut self, v: i32) {
        self.op(Instruction::I32Const(v));
    }

    /// An address or a length: the bits of `v` as an `i32` immediate.
    pub fn u32(&mut self, v: u32) {
        self.op(Instruction::I32Const(v.cast_signed()));
    }

    pub fn get(&mut self, l: u32) {
        self.op(Instruction::LocalGet(l));
    }

    pub fn set(&mut self, l: u32) {
        self.op(Instruction::LocalSet(l));
    }

    pub fn global_get(&mut self, g: Global) {
        let id = self.intern_global(g);
        self.push(Ins::GlobalGet(id));
    }

    pub fn global_set(&mut self, g: Global) {
        let id = self.intern_global(g);
        self.push(Ins::GlobalSet(id));
    }
}

/// A memory argument of `memory` with no offset, aligned to `align`
/// bytes' log.
#[must_use]
pub const fn mem(memory: u32, offset: u64, align: u32) -> wasm_encoder::MemArg {
    wasm_encoder::MemArg {
        offset,
        align,
        memory_index: memory,
    }
}

/// The module under construction.
#[derive(Default)]
pub struct Module {
    pub track_origins: bool,
    /// Reachable package capabilities and their generated adapter schemas.
    pub package_contract: hd_host_abi::schema::Contract,
    /// Defined functions by symbol. A helper is defined when
    /// [`Module::finish`] finds a body naming it.
    symbols: Symbols,
    funcs: Vec<Option<Func>>,
    /// Instances in the order they were defined: content order.
    inst_order: Vec<SymId>,
    /// Static data from [`STATIC_BASE`], deduplicated by content.
    data: Vec<u8>,
    data_at: BTreeMap<Vec<u8>, u32>,
    /// Static vtables: each one's slots, then the vtables of its direct
    /// supertraits, laid out after the data.
    vtables: Vec<(Vec<SymId>, Vec<usize>)>,
    exports: Vec<(String, SymId)>,
    /// Development fixture roots, independent of the public export ABI.
    retained: Vec<SymId>,
    /// The std runtime that helpers call, when the program links it.
    pub runtime: Option<crate::Runtime>,
    /// Each instance by its key, for a caller to read how its callee takes
    /// its parameters (`crates/hd_wasm_linear/DESIGN.md` §23.25).
    pub inst_of: hd_base::HashMap<Hash128, hd_base::InstId>,
    /// Each suspending instance's frame payload size.
    pub frames: BTreeMap<Hash128, u32>,
    /// The top-level bindings some reference refers to, by path hash:
    /// each lives in a static slot at this address, not in Wasm globals
    /// (`crates/hd_wasm_linear/DESIGN.md` §23.20).
    pub addressed: BTreeMap<Hash128, u32>,
    /// In checked mode, each stored binding or once-cell, which the audit
    /// at exit releases and zeros first (§23.24).
    pub stored: BTreeMap<Hash128, Stored>,
    /// A release build: its function names go in `hd.names`, not `name`
    /// (`crates/hd_wasm_linear/DESIGN.md` §15.9).
    pub release: bool,
    /// Set by [`Module::finish`] when no object of the program takes part
    /// in cycle collection: the helpers then call the runtime's releases
    /// without collector hooks and never collect, so `std.rt.cycle` is not
    /// linked (§23.28).
    pub acyclic: bool,
    /// The instances of `std.rt.cycle`, whose module init an acyclic
    /// program does not run.
    pub cycle_insts: BTreeSet<Hash128>,
    /// Whether [`Module::finish`] inlines the count fast paths
    /// ([`crate::Input::inline_counts`]).
    pub inline_counts: bool,
    /// Some emitted object has a constant small allocation class.
    pub fixed_allocations: bool,
    /// Reachable all! nodes use generation-checked weak waker targets.
    pub wakers: bool,
    /// Reachable program operations or exported entry hooks can cancel.
    /// Selected from the conservative reachable graph before specialization.
    pub cancellable: bool,
    /// With origins: what each static data address holds, for
    /// development tools ([`crate::StaticData`]).
    pub static_labels: BTreeMap<u32, crate::StaticData>,
    /// With origins: each top-level binding's or once-cell's path, by its
    /// path hash, which names its globals.
    pub storage_names: BTreeMap<Hash128, String>,
}

/// Stored values and their counted-reference walk (§23.24), with a static
/// slot when addressed. A once-cell includes its uncounted flag after its
/// result values so teardown resets the cache between test cases.
#[derive(Clone, Debug)]
pub struct Stored {
    pub vals: Vec<Val>,
    pub walk: crate::layout::Walk,
    pub slot: Option<(u32, Vec<crate::layout::Leaf>)>,
}

/// Whether `f` allocates an object that takes part in cycle collection:
/// a descriptor word with [`CYCLIC`], which `rt::push_desc` pushes as a
/// constant right after its glue's table slot.
fn cyclic_desc(f: &Func) -> bool {
    #[cfg(feature = "checked-report")]
    if f.body.windows(2).any(|w| matches!(w, [Ins::Descriptor(_, _), Ins::Op(Instruction::I32Const(c))] if c.cast_unsigned() & CYCLIC != 0)) {
        return true;
    }
    f.body.windows(2).any(|w| {
        matches!(
            w,
            [Ins::TableSlot(_), Ins::Op(Instruction::I32Const(c))]
                if c.cast_unsigned() & CYCLIC != 0
        )
    })
}

impl Module {
    pub fn func(&self, name: impl Into<String>, params: Vec<Val>, results: Vec<Val>) -> Func {
        Func::new(self.symbols.clone(), name, params, results)
    }
    /// Defines a symbol without moving its semantic ordering into the id.
    pub fn define(&mut self, s: Sym, mut f: Func) {
        let instance = matches!(s, Sym::Inst(_) | Sym::Part(..));
        let id = self.symbols.intern(s);
        assert!(
            Rc::ptr_eq(&self.symbols.0, &f.symbols.0),
            "function belongs to module"
        );
        if instance {
            self.inst_order.push(id);
        }
        f.rebuild_callees();
        self.funcs
            .resize_with(self.funcs.len().max(id.idx() + 1), || None);
        self.funcs[id.idx()] = Some(f);
    }
    pub fn export(&mut self, name: impl Into<String>, s: Sym) {
        self.exports.push((name.into(), self.symbols.intern(s)));
    }
    pub fn retain_instances(&mut self, keys: &[Hash128]) {
        let keys: BTreeSet<Hash128> = keys.iter().copied().collect();
        for (k, f) in self.funcs.iter().enumerate() {
            let id = SymId(u32::try_from(k).expect("symbols"));
            if f.is_some()
                && matches!(&*self.symbols.get(id), Sym::Inst(h) | Sym::Part(h, _) if keys.contains(h))
            {
                self.retained.push(id);
            }
        }
        self.retained.sort_by(|a, b| self.symbols.cmp(*a, *b));
        self.retained.dedup();
    }
    fn keep(&mut self, mut wanted: impl FnMut(SymId, &Sym) -> bool) {
        for (k, f) in self.funcs.iter_mut().enumerate() {
            let id = SymId(u32::try_from(k).expect("symbols"));
            if f.is_some() && !wanted(id, &self.symbols.get(id)) {
                *f = None;
            }
        }
    }

    /// The heap address of `bytes` in static data, placed on first use
    /// and aligned to `align`.
    pub fn static_bytes(&mut self, bytes: &[u8], align: u32) -> u32 {
        if let Some(at) = self.data_at.get(bytes) {
            return *at;
        }
        let align = align.max(1) as usize;
        while !self.data.len().is_multiple_of(align) {
            self.data.push(0);
        }
        let at = STATIC_BASE + u32::try_from(self.data.len()).expect("static data");
        self.data.extend_from_slice(bytes);
        self.data_at.insert(bytes.to_vec(), at);
        if self.track_origins {
            self.static_labels
                .insert(at, crate::StaticData::Bytes(bytes.to_vec()));
        }
        at
    }

    /// Names static data address `at` for development tools.
    pub fn label_static(&mut self, at: u32, label: &str) {
        if self.track_origins {
            self.static_labels
                .insert(at, crate::StaticData::Named(label.to_owned()));
        }
    }

    /// The heap address of `size` new zero bytes of static data, aligned
    /// to `align`, which no other use shares: a slot the program writes.
    pub fn static_slot(&mut self, size: u32, align: u32) -> u32 {
        let align = align.max(1) as usize;
        while !self.data.len().is_multiple_of(align) {
            self.data.push(0);
        }
        let at = STATIC_BASE + u32::try_from(self.data.len()).expect("static data");
        self.data.resize(self.data.len() + size.max(1) as usize, 0);
        at
    }

    /// The number of a static vtable whose slots are `slots`' table
    /// indices, one `i32` each; [`Ins::VTable`] names its address.
    pub fn vtable(&mut self, slots: Vec<Sym>) -> usize {
        self.vtable_with(slots, Vec::new())
    }

    /// The number of a static vtable whose slots are `slots`' table
    /// indices, then the addresses of the vtables `parents`, one `i32`
    /// each.
    pub fn vtable_with(&mut self, slots: Vec<Sym>, parents: Vec<usize>) -> usize {
        let v = (
            slots.into_iter().map(|s| self.symbols.intern(s)).collect(),
            parents,
        );
        if let Some(k) = self.vtables.iter().position(|x| *x == v) {
            return k;
        }
        self.vtables.push(v);
        self.vtables.len() - 1
    }

    /// Builds the helpers that the exports reach, to a fixed point, and
    /// drops every function that nothing reaches: an instance collected
    /// only for the runtime, such as the allocator of a program that never
    /// allocates (§23.6, "Reachable code only").
    fn reach(&mut self) {
        let seen = self.reached();
        self.keep(|s, _| seen.contains(s));
        self.inst_order.retain(|s| seen.contains(*s));
    }

    /// Reuse the existing no-view link fact before cycle selection: a
    /// physical projection is an address offset plus its owned count.
    /// Removing its unreachable view helper also prevents that helper's
    /// allocation descriptor from needlessly linking the collector.
    fn physical_projections(&mut self) {
        let runtime = self.runtime;
        for f in self.funcs.iter_mut().flatten() {
            let body = std::mem::take(&mut f.body);
            let origins = f.origins.as_mut().map(std::mem::take);
            let outer = f.origin_at;
            for (i, ins) in body.into_iter().enumerate() {
                f.origin_at = origins.as_ref().and_then(|o| o[i]);
                let projection = match ins {
                    Ins::Call(id) => match &*self.symbols.get(id) {
                        Sym::Helper(crate::rt::Helper::RefProject {
                            offset, borrowed, ..
                        }) => Some((*offset, *borrowed)),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((offset, borrowed)) = projection {
                    if offset != 0 {
                        f.u32(offset);
                        f.op(Instruction::I32Add);
                    }
                    if !borrowed && let Some(runtime) = runtime {
                        let at = f.local(Val::I32);
                        f.set(at);
                        f.get(at);
                        f.call(Sym::Inst(runtime.retain));
                        f.get(at);
                    }
                } else {
                    f.push(ins);
                }
            }
            f.origin_at = outer;
            f.rebuild_callees();
        }
    }

    /// What the exports reach, building the helpers among it.
    fn reached(&mut self) -> Reached {
        let mut seen = Reached::default();
        let mut work: Vec<SymId> = self.exports.iter().map(|(_, s)| *s).collect();
        work.extend(self.retained.iter().copied());
        work.extend(self.vtables.iter().flat_map(|v| v.0.iter()).copied());
        while let Some(s) = work.pop() {
            if matches!(&*self.symbols.get(s), Sym::Import(_)) || !seen.insert(s) {
                continue;
            }
            if self.funcs.get(s.idx()).is_none_or(Option::is_none) {
                let helper = match &*self.symbols.get(s) {
                    Sym::Helper(h) => Some(h.clone()),
                    _ => None,
                };
                if let Some(h) = helper {
                    let f = crate::rt::helper(&h, self);
                    self.define(Sym::Helper(h), f);
                }
            }
            let Some(Some(f)) = self.funcs.get(s.idx()) else {
                continue;
            };
            for edge in &f.callees {
                match edge {
                    Edge::Function(c) => work.push(*c),
                    Edge::VTable(k) => work.extend(self.vtables[*k].0.iter().copied()),
                }
            }
        }
        seen
    }

    /// Builds the reachable helpers, numbers every function and encodes
    /// the module.
    pub fn finish(self) -> StageResult<Vec<u8>> {
        self.finish_with_origins().map(|mapped| mapped.wasm)
    }

    pub fn finish_with_origins(mut self) -> StageResult<crate::MappedModule> {
        // Discover cancellation roots without following the cancel slots of
        // every cold frame. Once a root exists, expand those slots before
        // choosing physical layouts and cycle-collection support.
        self.cancellable = false;
        let original_runtime = self.runtime;
        for f in self.funcs.iter_mut().flatten() {
            f.origins_before_opt = f
                .origins
                .as_ref()
                .map(|os| os.iter().map(|o| f.source_origin(*o)).collect())
                .unwrap_or_default();
        }
        let mut seen = self.reached();
        self.cancellable = seen.iter().any(|s| {
            matches!(
                &*self.symbols.get(s),
                Sym::Helper(
                    crate::rt::Helper::CancellationRoot
                        | crate::rt::Helper::EntryCancel { .. }
                        | crate::rt::Helper::CleanupCold
                        | crate::rt::Helper::Race { part: 0, .. }
                        | crate::rt::Helper::WakerNew
                )
            )
        });
        if self.cancellable {
            self.keep(|_, s| !matches!(s, Sym::Helper(_)));
            seen = self.reached();
        }
        if !seen.iter().any(|s| {
            matches!(
                &*self.symbols.get(s),
                Sym::Helper(crate::rt::Helper::RefView { .. })
            )
        }) {
            self.physical_projections();
            seen = self.reached();
        }
        let cyclic = self.funcs.iter().enumerate().any(|(k, f)| {
            f.as_ref().is_some_and(|f| {
                seen.contains(SymId(u32::try_from(k).expect("symbols"))) && cyclic_desc(f)
            })
        });
        // Read the linked helper feature, including functions reused from
        // the code cache, rather than mutable state from fresh emission.
        self.fixed_allocations = seen.iter().any(|s| {
            matches!(
                &*self.symbols.get(s),
                Sym::Helper(crate::rt::Helper::NewFixed { .. })
            )
        });
        self.wakers = seen.iter().any(|s| {
            matches!(
                &*self.symbols.get(s),
                Sym::Helper(crate::rt::Helper::WakerNew)
            )
        });
        if self.fixed_allocations || self.wakers {
            self.keep(|_, s| !matches!(s, Sym::Helper(_)));
        }
        if let Some(r) = self.runtime
            && !cyclic
        {
            // No allocation takes part in cycle collection: build the
            // helpers again over the runtime's releases without hooks.
            self.acyclic = true;
            self.runtime = Some(crate::Runtime {
                free: r.acyclic.free,
                release: r.acyclic.release,
                release_known: r.acyclic.release_known,
                ..r
            });
            self.keep(|_, s| !matches!(s, Sym::Helper(_)));
            // Code that calls the collector itself, as std's own tests do,
            // still needs its state: run its module init after all.
            let seen = self.reached();
            if self
                .cycle_insts
                .iter()
                .any(|k| seen.contains(self.symbols.intern(Sym::Inst(*k))))
            {
                self.cycle_insts.clear();
                self.keep(|_, s| !matches!(s, Sym::Helper(_)));
            }
        }
        self.reach();
        let physical = !self.funcs.iter().enumerate().any(|(k, f)| {
            f.is_some()
                && matches!(
                    &*self.symbols.get(SymId(u32::try_from(k).expect("symbols"))),
                    Sym::Helper(crate::rt::Helper::RefView { .. })
                )
        });
        for f in self.funcs.iter_mut().flatten() {
            let body = std::mem::take(&mut f.body);
            let origins = f.origins.as_mut().map(std::mem::take);
            let outer = f.origin_at;
            for (position, ins) in body.into_iter().enumerate() {
                f.origin_at = origins.as_ref().and_then(|o| o[position]);
                let helper = match ins {
                    Ins::Call(id) => match &*self.symbols.get(id) {
                        Sym::Helper(crate::rt::Helper::RefIsView) => 1,
                        Sym::Helper(crate::rt::Helper::RefViewsAbsent) => 2,
                        Sym::Helper(crate::rt::Helper::CanCancel) => 3,
                        Sym::Helper(crate::rt::Helper::CancellationRoot) => 4,
                        _ => 0,
                    },
                    _ => 0,
                };
                match helper {
                    1 if physical => {
                        f.op(Instruction::Drop);
                        f.i32(0);
                    }
                    2 => f.i32(i32::from(physical)),
                    3 => f.i32(i32::from(self.cancellable)),
                    4 => {}
                    _ => f.push(ins),
                }
            }
            f.origin_at = outer;
            f.rebuild_callees();
        }
        self.reach();
        crate::opt::constants(&mut self.funcs, &self.symbols);
        for f in self.funcs.iter_mut().flatten() {
            crate::opt::fold_constant_branches(f);
        }
        let mut arity = vec![None; self.symbols.len()];
        for (k, f) in self.funcs.iter().enumerate() {
            if let Some(f) = f {
                arity[k] = Some((f.params.len(), f.results.len()));
            }
        }
        for f in self.funcs.iter_mut().flatten() {
            crate::opt::locals(f, &arity);
            if self
                .runtime
                .is_some_and(|r| crate::opt::count_pairs(f, r.retain))
            {
                crate::opt::locals(f, &arity);
            }
        }
        // Checked mode keeps every count a call, so its runtime audits them.
        if self.inline_counts && self.runtime.is_some_and(|r| r.checked.is_none()) {
            let mut inlined = false;
            for f in self.funcs.iter_mut().flatten() {
                if crate::opt::inline_counts(f, self.acyclic) {
                    crate::opt::locals(f, &arity);
                    inlined = true;
                }
            }
            if inlined {
                for f in self.funcs.iter_mut().flatten() {
                    f.rebuild_callees();
                }
                self.reach();
            }
        }
        let stack = crate::wide::lower(&mut self.funcs, &self.exports, &self.symbols)?;
        // Wide lowering rebuilds each body's callees from its final code.
        // Calls removed by local/count optimization no longer keep helpers.
        self.reach();
        // Every import and global a body names; imports come first.
        let mut import_ids = Reached::default();
        let mut globals: BTreeSet<Global> = BTreeSet::new();
        for f in self.funcs.iter().flatten() {
            for i in &f.body {
                match i {
                    Ins::Call(id) | Ins::TableSlot(id) => {
                        if matches!(&*self.symbols.get(*id), Sym::Import(_)) {
                            import_ids.insert(*id);
                        }
                    }
                    Ins::GlobalGet(g) | Ins::GlobalSet(g) => {
                        globals.insert(self.symbols.global(*g));
                    }
                    _ => {}
                }
            }
        }
        let imports: BTreeSet<Import> = import_ids
            .iter()
            .map(|id| match &*self.symbols.get(id) {
                Sym::Import(im) => im.clone(),
                _ => unreachable!("import"),
            })
            .collect();
        let mut index = Numbers::new();
        let mut next = 0u32;
        for im in &imports {
            index.insert(self.symbols.intern(Sym::Import(im.clone())), next);
            next += 1;
        }
        let mut defined: Vec<SymId> = self.inst_order.clone();
        let mut helpers: Vec<_> = self
            .funcs
            .iter()
            .enumerate()
            .filter_map(|(k, f)| {
                let id = SymId(u32::try_from(k).expect("symbols"));
                (f.is_some() && matches!(&*self.symbols.get(id), Sym::Helper(_))).then_some(id)
            })
            .collect();
        helpers.sort_by(|a, b| self.symbols.cmp(*a, *b));
        defined.extend(helpers);
        for s in &defined {
            index.insert(*s, next);
            next += 1;
        }
        let undefined = |s: &SymId| {
            NotImplemented::bug(
                Stage::Link,
                format!(
                    "the linear backend names an undefined function {:?}",
                    *self.symbols.get(*s)
                ),
            )
        };
        // The function table: every function a slot or a vtable names,
        // in the order first named.
        let mut slots: Vec<u32> = Vec::new();
        let mut slot_of = Numbers::new();
        let mut slot = |s: &SymId, slots: &mut Vec<u32>| -> StageResult<u32> {
            if let Some(k) = slot_of.get(*s) {
                return Ok(*k);
            }
            let f = *index.get(*s).ok_or_else(|| undefined(s))?;
            let k = u32::try_from(slots.len()).expect("table");
            slots.push(f);
            slot_of.insert(*s, k);
            Ok(k)
        };
        #[cfg(feature = "checked-report")]
        let mut descriptors: BTreeMap<(SymId, u8), u32> = BTreeMap::new();
        // Vtables follow the data, one `i32` slot index each.
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut vtable_at = Vec::new();
        for v in &self.vtables {
            vtable_at.push(STATIC_BASE + u32::try_from(self.data.len()).expect("static data"));
            for s in &v.0 {
                let k = slot(s, &mut slots)?;
                self.data.extend_from_slice(&k.to_le_bytes());
            }
            // A parent is always numbered before the vtables that name it.
            for p in &v.1 {
                self.data.extend_from_slice(&vtable_at[*p].to_le_bytes());
            }
        }
        let static_end = STATIC_BASE + u32::try_from(self.data.len()).expect("static data");
        let heap_base = static_end.next_multiple_of(8);
        let gindex: BTreeMap<Global, u32> = globals
            .iter()
            .enumerate()
            .map(|(k, g)| (*g, u32::try_from(k).expect("globals")))
            .collect();

        let mut mapped_functions = Vec::new();
        let mut types = TypeSection::new();
        let mut type_of: BTreeMap<(Vec<Val>, Vec<Val>), u32> = BTreeMap::new();
        let mut ty = |params: &[Val], results: &[Val], types: &mut TypeSection| {
            let key = (params.to_vec(), results.to_vec());
            if let Some(t) = type_of.get(&key) {
                return *t;
            }
            let t = u32::try_from(type_of.len()).expect("types");
            types.ty().function(
                params.iter().map(|v| v.wasm()),
                results.iter().map(|v| v.wasm()),
            );
            type_of.insert(key, t);
            t
        };
        let mut import_sec = ImportSection::new();
        for im in &imports {
            let t = ty(&im.params, &im.results, &mut types);
            import_sec.import(&im.module, &im.name, EntityType::Function(t));
        }
        let mut funcs = FunctionSection::new();
        let mut code = CodeSection::new();
        let mut names = NameMap::new();
        let mut lines: Vec<(u32, Vec<(u32, u32)>)> = Vec::new();
        let (mut frames, mut paths) = (Vec::new(), Vec::new());
        for im in &imports {
            names.append(
                index[&self.symbols.intern(Sym::Import(im.clone()))],
                &format!("{}/{}", im.module, im.name),
            );
        }
        // A helper name that several helpers share, at different
        // arguments, takes its place among them as a suffix. An instance
        // keeps its item's `path#sub`, which a panic's frames read.
        let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
        for s in &defined {
            *seen
                .entry(
                    self.funcs[s.idx()]
                        .as_ref()
                        .expect("defined function")
                        .name
                        .as_str(),
                )
                .or_default() += 1;
        }
        let mut taken: BTreeMap<&str, u32> = BTreeMap::new();
        let mut own: Vec<(u32, String)> = Vec::new();
        for s in &defined {
            let f = self.funcs[s.idx()].as_ref().expect("defined function");
            let name = if f.name.starts_with("rt:") && seen[f.name.as_str()] > 1 {
                let k = taken.entry(f.name.as_str()).or_default();
                *k += 1;
                format!("{}#{}", f.name, *k - 1)
            } else {
                f.name.clone()
            };
            own.push((index[s], name));
            funcs.function(ty(&f.params, &f.results, &mut types));
            if let Some(raw) = &f.raw {
                assert!(f.locals.is_empty() && f.body.is_empty());
                code.raw(raw);
                continue;
            }
            let mut body = Function::new_with_locals_types(f.locals.iter().map(|v| v.wasm()));
            let mut mapped_instructions = Vec::new();
            let mut mapped_counts = Vec::new();
            let mut count_sites = BTreeSet::new();
            let mut at = None;
            let mut rows: Vec<(u32, u32)> = Vec::new();
            let (mut map, mut kept) = (vec![None; f.inlined.len()], Vec::new());
            for (position, i) in f.body.iter().enumerate() {
                if self.track_origins && !matches!(i, Ins::Anchor(_)) {
                    mapped_instructions.push(crate::InstructionOrigin {
                        offset: u32::try_from(body.byte_len()).expect("offset"),
                        origin: f.source_origin(f.origins.as_ref().and_then(|o| o[position])),
                    });
                }
                if self.track_origins {
                    let origin = f.origins.as_ref().and_then(|o| o[position]);
                    let note = origin.and_then(|o| f.count_origin(o));
                    let action = match i {
                        Ins::Call(s) => {
                            crate::views::count_action(&self.symbols.get(*s), self.runtime.as_ref())
                                .or_else(|| {
                                    crate::views::count_action(
                                        &self.symbols.get(*s),
                                        original_runtime.as_ref(),
                                    )
                                })
                        }
                        // A Wasmtime count fast path expands into a header
                        // store plus a slow-path call. They are one source
                        // operation and share the same count-site token.
                        Ins::Op(Instruction::I32Store(m))
                            if self.inline_counts && m.offset == 0 =>
                        {
                            note.map(|n| n.action)
                        }
                        _ => None,
                    };
                    if let Some(action) = action
                        && note.is_none_or(|n| count_sites.insert(n.site))
                    {
                        mapped_counts.push(crate::views::CountOperation {
                            offset: u32::try_from(body.byte_len()).expect("offset"),
                            origin: f.source_origin(origin),
                            action,
                            subject: note.map_or_else(
                                || {
                                    if action == crate::views::CountAction::Free {
                                        "block".to_owned()
                                    } else {
                                        "counted reference".to_owned()
                                    }
                                },
                                |n| n.subject.clone(),
                            ),
                        });
                    }
                }
                // A call at a new anchor starts a row: its offset in the
                // body, locals included, and the anchor.
                if let (Ins::Call(_) | Ins::CallIndirect(..), Some(a)) = (i, at) {
                    let a = reach(a, &f.inlined, &mut map, &mut kept, &mut paths);
                    if rows.last().is_none_or(|&(_, last)| last != a) {
                        rows.push((u32::try_from(body.byte_len()).expect("offset"), a));
                    }
                }
                match i {
                    Ins::Anchor(a) => {
                        at = Some(*a);
                        &mut body
                    }
                    Ins::Op(op) => body.instruction(&op.to_wasm(&f.labels)),
                    Ins::Call(c) => body.instruction(&WasmInstruction::Call(
                        *index.get(*c).ok_or_else(|| undefined(c))?,
                    )),
                    Ins::GlobalGet(g) => body.instruction(&WasmInstruction::GlobalGet(
                        gindex[&self.symbols.global(*g)],
                    )),
                    Ins::GlobalSet(g) => body.instruction(&WasmInstruction::GlobalSet(
                        gindex[&self.symbols.global(*g)],
                    )),
                    #[cfg(feature = "checked-report")]
                    Ins::Descriptor(t, kind) => {
                        let k = descriptor_slot(*t, *kind, &index, &mut slots, &mut descriptors)?;
                        body.instruction(&WasmInstruction::I32Const(k.cast_signed()))
                    }
                    Ins::TableSlot(t) => {
                        #[cfg(feature = "checked-report")]
                        let descriptor = self.runtime.is_some_and(|r| r.checked.is_some())
                            && matches!(f.body.get(position + 1), Some(Ins::Op(Instruction::I32Const(c))) if c.cast_unsigned() == 1 || c.cast_unsigned() == CYCLIC + 1)
                            && matches!(
                                f.body.get(position + 2),
                                Some(Ins::Op(Instruction::I32Add))
                            );
                        #[cfg(feature = "checked-report")]
                        let k = if descriptor {
                            descriptor_slot(*t, 0, &index, &mut slots, &mut descriptors)?
                        } else {
                            slot(t, &mut slots)?
                        };
                        #[cfg(not(feature = "checked-report"))]
                        let k = slot(t, &mut slots)?;
                        if k >= GLUE {
                            return crate::unsupported(
                                "a function table exceeding the descriptor glue field",
                            );
                        }
                        body.instruction(&WasmInstruction::I32Const(k.cast_signed()))
                    }
                    Ins::CallIndirect(id) => {
                        let signature = self.symbols.signature(*id);
                        body.instruction(&WasmInstruction::CallIndirect {
                            type_index: ty(&signature.0, &signature.1, &mut types),
                            table_index: 0,
                        })
                    }
                    Ins::VTable(k) => {
                        body.instruction(&WasmInstruction::I32Const(vtable_at[*k].cast_signed()))
                    }
                    Ins::SaveAll | Ins::LoadAll => {
                        return Err(NotImplemented::bug(
                            Stage::Link,
                            "a frame save or load that its body did not expand",
                        ));
                    }
                };
            }
            if self.track_origins {
                mapped_instructions.push(crate::InstructionOrigin {
                    offset: u32::try_from(body.byte_len()).expect("offset"),
                    origin: f.origin_root,
                });
                let names = f.local_names.as_ref();
                let locals = (0..f.params.len() + f.locals.len())
                    .map(|l| names.and_then(|n| n.get(&u32::try_from(l).expect("locals")).cloned()))
                    .collect();
                let mut statics: BTreeMap<u32, crate::StaticData> = BTreeMap::new();
                for at in f.statics.iter().flatten() {
                    let label = match (*at, self.static_labels.get(at)) {
                        (_, Some(l)) => l.clone(),
                        (EMPTY, None) => crate::StaticData::Named("\"\"".to_owned()),
                        (_, None) => continue,
                    };
                    statics.insert(*at, label);
                }
                let mut globals_named: BTreeMap<u32, String> = BTreeMap::new();
                for i in &f.body {
                    match i {
                        Ins::VTable(k) => {
                            let slots = self.vtables[*k]
                                .0
                                .iter()
                                .map(|s| index.get(*s).copied().ok_or_else(|| undefined(s)))
                                .collect::<StageResult<_>>()?;
                            statics.insert(vtable_at[*k], crate::StaticData::VTable(slots));
                        }
                        Ins::GlobalGet(g) | Ins::GlobalSet(g) => {
                            globals_named.insert(
                                gindex[&self.symbols.global(*g)],
                                global_name(&self.symbols.global(*g), &self.storage_names),
                            );
                        }
                        _ => {}
                    }
                }
                mapped_functions.push(crate::FunctionOrigins {
                    index: index[s],
                    name: own.last().expect("function name").1.clone(),
                    item: f.origin_item.clone(),
                    type_args: f.origin_args.clone(),
                    before_opt: f.origins_before_opt.clone(),
                    counts: mapped_counts,
                    instructions: mapped_instructions,
                    locals,
                    statics: statics.into_iter().collect(),
                    globals: globals_named.into_iter().collect(),
                });
            }
            body.instruction(&WasmInstruction::End);
            code.function(&body);
            if !rows.is_empty() {
                lines.push((index[s], rows));
            }
            if !kept.is_empty() {
                frames.push((index[s], kept));
            }
        }
        let mut tables = TableSection::new();
        let mut elements = ElementSection::new();
        // A `call_indirect` needs the table even when nothing fills it, as
        // release glue's does in a program whose objects hold no counted
        // reference.
        let indirect = self
            .funcs
            .iter()
            .flatten()
            .any(|f| f.body.iter().any(|i| matches!(i, Ins::CallIndirect(..))));
        let table = indirect || !slots.is_empty();
        if table {
            let n = u64::try_from(slots.len()).expect("table");
            tables.table(TableType {
                element_type: RefType::FUNCREF,
                table64: false,
                minimum: n,
                maximum: Some(n),
                shared: false,
            });
        }
        if !slots.is_empty() {
            elements.active(
                None,
                &ConstExpr::i32_const(0),
                Elements::Functions(slots.as_slice().into()),
            );
        }
        let mut memories = MemorySection::new();
        // Memory 0: the exchange buffer, grown by the host's results.
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        // Memory 1: the heap, whose first pages hold static data.
        let heap_pages = heap_base.div_ceil(1 << 16).max(1);
        memories.memory(MemoryType {
            minimum: u64::from(heap_pages),
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        // Memory 2: the shadow stack, when a wide signature uses it.
        if stack {
            memories.memory(MemoryType {
                minimum: u64::from(STACK_PAGES),
                maximum: Some(u64::from(STACK_PAGES)),
                memory64: false,
                shared: false,
                page_size_log2: None,
            });
        }
        let mut global_sec = GlobalSection::new();
        for g in &globals {
            let v = g.val();
            let init = match (g, v) {
                (Global::HeapTop, _) => ConstExpr::i32_const(heap_base.cast_signed()),
                (Global::HeapEnd, _) => ConstExpr::i32_const((heap_pages << 16).cast_signed()),
                (Global::Stack, _) => ConstExpr::i32_const((STACK_PAGES << 16).cast_signed()),
                (_, Val::I32) => ConstExpr::i32_const(0),
                (_, Val::I64) => ConstExpr::i64_const(0),
                (_, Val::F32) => ConstExpr::f32_const(wasm_encoder::Ieee32::from(0.0)),
                (_, Val::F64) => ConstExpr::f64_const(wasm_encoder::Ieee64::from(0.0)),
            };
            global_sec.global(
                GlobalType {
                    val_type: v.wasm(),
                    mutable: true,
                    shared: false,
                },
                &init,
            );
        }
        let mut exports = ExportSection::new();
        for (name, s) in &self.exports {
            exports.export(
                name,
                ExportKind::Func,
                *index.get(*s).ok_or_else(|| undefined(s))?,
            );
        }
        if !self.release
            && let Some(index) = gindex.get(&Global::AllPolls)
        {
            exports.export("hd.all_polls", ExportKind::Global, *index);
        }
        exports.export("hd.x", ExportKind::Memory, BUF);
        #[cfg(feature = "checked-report")]
        if self.runtime.is_some_and(|r| r.checked.is_some()) {
            exports.export("hd.heap", ExportKind::Memory, HEAP);
        }
        let mut data = DataSection::new();
        if !self.data.is_empty() {
            data.active(
                HEAP,
                &ConstExpr::i32_const(STATIC_BASE.cast_signed()),
                self.data.iter().copied(),
            );
        }
        let mut module = wasm_encoder::Module::new();
        if !self.package_contract.interfaces.is_empty() {
            self.package_contract
                .interfaces
                .sort_by(|a, b| (&a.identity, a.handle).cmp(&(&b.identity, b.handle)));
            self.package_contract
                .validate()
                .map_err(|why| NotImplemented::bug(Stage::Link, why))?;
            module.section(&wasm_encoder::CustomSection {
                name: hd_host_abi::schema::SECTION.into(),
                data: self.package_contract.encode().into(),
            });
        }
        module.section(&types);
        if !imports.is_empty() {
            module.section(&import_sec);
        }
        module.section(&funcs);
        if table {
            module.section(&tables);
        }
        module.section(&memories);
        if !globals.is_empty() {
            module.section(&global_sec);
        }
        module.section(&exports);
        if !slots.is_empty() {
            module.section(&elements);
        }
        module.section(&code);
        if !self.data.is_empty() {
            module.section(&data);
        }
        if !lines.is_empty() {
            module.section(&line_table(&lines));
        }
        if self.release {
            module.section(&compact_names(&own));
        } else {
            for (f, name) in &own {
                names.append(*f, name);
            }
            let mut name_sec = NameSection::new();
            name_sec.functions(&names);
            module.section(&name_sec);
        }
        #[cfg(feature = "checked-report")]
        if self.runtime.is_some_and(|r| r.checked.is_some()) {
            let mut data = vec![1];
            let mut descriptors: Vec<_> = descriptors.iter().collect();
            descriptors.sort_by(|((a, ka), _), ((b, kb), _)| {
                self.symbols.cmp(*a, *b).then_with(|| ka.cmp(kb))
            });
            for ((_, kind), slot) in descriptors {
                data.extend_from_slice(&(slot + 1).to_le_bytes());
                data.push(*kind);
            }
            module.section(&wasm_encoder::CustomSection {
                name: "hd.cycles".into(),
                data: data.into(),
            });
        }
        // Last, so that `hd build`, which resolves the frames into
        // `hd.positions`, can cut it off.
        if !paths.is_empty() {
            module.section(&inlined_frames(&frames, &paths));
        }
        Ok(crate::MappedModule {
            wasm: module.finish(),
            functions: mapped_functions,
        })
    }
}

/// A global's name for development tools: a top-level binding's or
/// once-cell's path, with the value's index after the first, or the
/// runtime global's own name in snake case (`heap_top`).
fn global_name(g: &Global, storage: &BTreeMap<Hash128, String>) -> String {
    if let Global::Storage(h, k, _) = g {
        let path = storage.get(h).map_or("binding", String::as_str);
        return if *k == 0 {
            path.to_owned()
        } else {
            format!("{path}.{k}")
        };
    }
    let mut out = String::new();
    for (k, c) in format!("{g:?}").chars().enumerate() {
        if c.is_ascii_uppercase() && k > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// The `hd.lines` section (`crates/hd_wasm_linear/DESIGN.md` §15.9), as the Wasm
/// GC backend writes it: per function with anchored calls, by function
/// index, its rows as (offset from the previous row's, anchor).
fn line_table(functions: &[(u32, Vec<(u32, u32)>)]) -> wasm_encoder::CustomSection<'static> {
    use wasm_encoder::Encode;
    let mut sorted: Vec<&(u32, Vec<(u32, u32)>)> = functions.iter().collect();
    sorted.sort_unstable_by_key(|(f, _)| *f);
    let mut data = Vec::new();
    u32::try_from(sorted.len()).expect("rows").encode(&mut data);
    for (f, rows) in sorted {
        f.encode(&mut data);
        u32::try_from(rows.len()).expect("rows").encode(&mut data);
        let mut last = 0;
        for &(off, at) in rows {
            (off - last).encode(&mut data);
            at.encode(&mut data);
            last = off;
        }
    }
    wasm_encoder::CustomSection {
        name: "hd.lines".into(),
        data: data.into(),
    }
}

/// The `hd.inlined` section: the callees' paths, then per function by
/// index the inlined frames its `hd.lines` rows reach, each as a path's
/// index, the callee's instruction and the anchor of its call
/// ([`Func::inlined`]).
fn inlined_frames(
    frames: &[(u32, Vec<[u32; 3]>)],
    paths: &[String],
) -> wasm_encoder::CustomSection<'static> {
    use wasm_encoder::Encode;
    let mut data = Vec::new();
    u32::try_from(paths.len()).expect("paths").encode(&mut data);
    for p in paths {
        p.as_str().encode(&mut data);
    }
    u32::try_from(frames.len())
        .expect("frames")
        .encode(&mut data);
    let mut sorted: Vec<_> = frames.iter().collect();
    sorted.sort_unstable_by_key(|(f, _)| *f);
    for (f, entries) in sorted {
        f.encode(&mut data);
        u32::try_from(entries.len())
            .expect("frames")
            .encode(&mut data);
        for w in entries.iter().flatten() {
            w.encode(&mut data);
        }
    }
    wasm_encoder::CustomSection {
        name: "hd.inlined".into(),
        data: data.into(),
    }
}

/// Inlined anchor `a` of a function whose frames are `all`, renumbered
/// into `kept`, the frames its rows reach with their parents, as
/// (path index in `paths`, instruction, parent); `map` holds each frame's
/// new number. Any other anchor is itself.
fn reach(
    a: u32,
    all: &[(String, u32, u32)],
    map: &mut [Option<u32>],
    kept: &mut Vec<[u32; 3]>,
    paths: &mut Vec<String>,
) -> u32 {
    let k = (a & !hd_tir::ir::INLINED) as usize;
    if a & hd_tir::ir::INLINED == 0 {
        return a;
    } else if let Some(n) = map[k] {
        return hd_tir::ir::INLINED | n;
    }
    let (path, inst, parent) = &all[k];
    let parent = reach(*parent, all, map, kept, paths);
    let p = paths.iter().position(|x| x == path).unwrap_or_else(|| {
        paths.push(path.clone());
        paths.len() - 1
    });
    kept.push([u32::try_from(p).expect("paths"), *inst, parent]);
    let n = u32::try_from(kept.len() - 1).expect("frames");
    map[k] = Some(n);
    hd_tir::ir::INLINED | n
}

/// The `hd.names` section of a release build (`crates/hd_wasm_linear/DESIGN.md`
/// §15.5), which writes no `name` section: each defined function's index
/// and the name `name` would give it.
fn compact_names(own: &[(u32, String)]) -> wasm_encoder::CustomSection<'static> {
    use wasm_encoder::Encode;
    let mut data = Vec::new();
    u32::try_from(own.len())
        .expect("functions")
        .encode(&mut data);
    for (f, name) in own {
        f.encode(&mut data);
        name.as_str().encode(&mut data);
    }
    wasm_encoder::CustomSection {
        name: "hd.names".into(),
        data: data.into(),
    }
}

#[cfg(feature = "checked-report")]
fn descriptor_slot(
    s: SymId,
    kind: u8,
    index: &Numbers,
    slots: &mut Vec<u32>,
    ids: &mut BTreeMap<(SymId, u8), u32>,
) -> StageResult<u32> {
    let key = (s, kind);
    if let Some(k) = ids.get(&key) {
        return Ok(*k);
    }
    let f = *index
        .get(s)
        .ok_or_else(|| NotImplemented::bug(Stage::Link, "undefined descriptor glue"))?;
    let k = u32::try_from(slots.len()).expect("descriptor table");
    if k >= GLUE {
        return crate::unsupported("a descriptor table exceeding the glue field");
    }
    slots.push(f);
    ids.insert(key, k);
    Ok(k)
}

#[cfg(all(test, feature = "checked-report"))]
mod checked_descriptor_tests {
    use super::*;
    #[test]
    fn kinds_alias_one_release_function_in_distinct_slots() {
        let glue = SymId(3);
        let mut index = Numbers::new();
        index.insert(glue, 7);
        let mut slots = Vec::new();
        let mut ids = BTreeMap::new();
        assert_eq!(
            descriptor_slot(glue, 1, &index, &mut slots, &mut ids).unwrap(),
            0
        );
        assert_eq!(
            descriptor_slot(glue, 2, &index, &mut slots, &mut ids).unwrap(),
            1
        );
        assert_eq!(
            descriptor_slot(glue, 1, &index, &mut slots, &mut ids).unwrap(),
            0
        );
        assert_eq!(slots, [7, 7]);
    }
}

#[cfg(test)]
mod dense_symbol_tests {
    use super::*;

    fn encoded(reverse_interning: bool) -> Vec<u8> {
        let mut m = Module::default();
        let a = Sym::Import(Import::new("test", "a", &[], &[Val::I32]));
        let b = Sym::Import(Import::new("test", "b", &[], &[Val::I32]));
        if reverse_interning {
            m.symbols.intern(b.clone());
            m.symbols.intern(a.clone());
            m.symbols.intern_global(Global::HashSeed);
        } else {
            m.symbols.intern(a.clone());
            m.symbols.intern(b.clone());
            m.symbols.intern_global(Global::Forbid);
        }
        let leaf = Sym::Inst(Hash128(1));
        let root = Sym::Inst(Hash128(2));
        let mut f = m.func("leaf", vec![], vec![Val::I32]);
        f.u32(7);
        m.define(leaf.clone(), f);
        let mut f = m.func("root", vec![], vec![Val::I32]);
        f.global_get(Global::Forbid);
        f.op(Instruction::Drop);
        f.global_get(Global::HashSeed);
        f.op(Instruction::Drop);
        f.call(b);
        f.op(Instruction::Drop);
        f.table_slot(leaf);
        f.call_indirect(vec![], vec![Val::I32]);
        f.op(Instruction::Drop);
        f.op(Instruction::Block(wasm_encoder::BlockType::Empty));
        f.u32(0);
        f.br_table(vec![0, 0], 0);
        f.op(Instruction::End);
        f.call(a);
        m.define(root.clone(), f);
        m.export("root", root);
        m.finish().expect("module encodes")
    }

    #[test]
    fn arena_insertion_order_does_not_change_wasm_numbering() {
        let a = encoded(false);
        let b = encoded(true);
        assert_eq!(a, b);
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&a)
            .expect("valid Wasm");
    }

    #[test]
    fn reachability_bits_cross_word_boundaries() {
        let mut reached = Reached::default();
        for id in [0, 63, 64, 130] {
            assert!(reached.insert(SymId(id)));
            assert!(!reached.insert(SymId(id)));
        }
        assert!(!reached.contains(SymId(62)));
        assert_eq!(
            reached.iter().map(|id| id.0).collect::<Vec<_>>(),
            [0, 63, 64, 130]
        );
    }

    #[test]
    fn cancellation_fact_is_specialized_and_markers_are_not_linked() {
        for explicit in [false, true] {
            let mut m = Module::default();
            let root = Sym::Inst(Hash128(42));
            let mut f = m.func("probe", vec![], vec![Val::I32]);
            if explicit {
                f.helper(crate::rt::Helper::CancellationRoot);
            }
            f.helper(crate::rt::Helper::CanCancel);
            m.define(root.clone(), f);
            m.export("probe", root);
            let wasm = m.finish().expect("module encodes");
            wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
                .validate_all(&wasm)
                .expect("valid Wasm");
            let bodies: Vec<_> = wasmparser::Parser::new(0)
                .parse_all(&wasm)
                .filter_map(|p| match p.expect("payload") {
                    wasmparser::Payload::CodeSectionEntry(body) => Some(body),
                    _ => None,
                })
                .collect();
            assert_eq!(bodies.len(), 1, "link-time helpers disappear");
            let ops: Vec<_> = bodies[0]
                .get_operators_reader()
                .expect("operators")
                .into_iter()
                .collect::<Result<_, _>>()
                .expect("valid operators");
            assert!(
                matches!(ops.as_slice(), [wasmparser::Operator::I32Const { value }, wasmparser::Operator::End] if *value == i32::from(explicit))
            );
        }
    }

    #[test]
    fn final_reachability_removes_a_call_folded_out_of_the_body() {
        let mut m = Module::default();
        let dead = Sym::Inst(Hash128(1));
        let root = Sym::Inst(Hash128(2));
        let f = m.func("dead_after_fold", vec![], vec![]);
        m.define(dead.clone(), f);
        let mut f = m.func("root", vec![], vec![]);
        f.i32(0);
        f.op(Instruction::If(wasm_encoder::BlockType::Empty));
        f.call(dead);
        f.op(Instruction::End);
        m.define(root.clone(), f);
        m.export("root", root);
        let wasm = m.finish().expect("module encodes");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&wasm)
            .expect("valid Wasm");
        let functions = wasmparser::Parser::new(0)
            .parse_all(&wasm)
            .filter(|p| matches!(p, Ok(wasmparser::Payload::CodeSectionEntry(_))))
            .count();
        assert_eq!(functions, 1, "only the exported root is still linked");
    }
}
