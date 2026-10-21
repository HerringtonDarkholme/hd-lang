//! The module under construction: functions with symbolic call targets,
//! the imports they name, globals, the function table, static vtables
//! and static data (`crates/hd_wasm_linear/DESIGN.md` §23.2, §23.8, §23.9).
//! [`Module::finish`] builds every helper a body names, numbers imports
//! first, then instances in content order, then helpers in their order,
//! and encodes the module.

use std::collections::{BTreeMap, BTreeSet};

use hd_base::{Hash128, NotImplemented, Stage, StageResult};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ElementSection, Elements, EntityType, ExportKind,
    ExportSection, Function, FunctionSection, GlobalSection, GlobalType, ImportSection,
    Instruction, MemorySection, MemoryType, NameMap, NameSection, RefType, TableSection, TableType,
    TypeSection, ValType,
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
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// Set when an abandon or an aborted host call leaves a party that no
    /// wake will reach: the driver polls its root again.
    Rewake,
    /// The entry's pending root frame, 0 for none.
    Root,
    /// The `hd.poll` context that owns the root, plus one.
    Owner,
    /// The wake table: its address and its capacity in handles.
    WakeTab,
    WakeCap,
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
#[derive(Clone, Debug)]
pub enum Ins {
    Op(Instruction<'static>),
    Call(Sym),
    GlobalGet(Global),
    GlobalSet(Global),
    /// `i32.const` of the function's slot in the function table.
    TableSlot(Sym),
    /// A checked descriptor alias: a distinct slot, the same release glue.
    #[cfg(feature = "checked-report")]
    Descriptor(Sym, u8),
    /// `call_indirect` through the function table, at this signature.
    CallIndirect(Vec<Val>, Vec<Val>),
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

/// One function body before numbering.
#[derive(Clone, Debug, Default)]
pub struct Func {
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
}

impl Func {
    #[must_use]
    pub fn new(name: impl Into<String>, params: Vec<Val>, results: Vec<Val>) -> Self {
        Func {
            name: name.into(),
            stack_values: params.len() > crate::wide::WIDTH || results.len() > crate::wide::WIDTH,
            params,
            results,
            ..Func::default()
        }
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
                Ins::Call(s) => crate::views::count_action(s, None),
                _ => None,
            }
        } else {
            None
        };
        let start = self.body.len();
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

    pub fn op(&mut self, i: Instruction<'static>) {
        self.push(Ins::Op(i));
    }

    pub fn call(&mut self, s: Sym) {
        self.anchor();
        self.push(Ins::Call(s));
    }

    /// `call_indirect` through the function table, at this signature.
    pub fn call_indirect(&mut self, params: Vec<Val>, results: Vec<Val>) {
        self.anchor();
        self.push(Ins::CallIndirect(params, results));
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
        self.call(Sym::Helper(h));
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
        self.push(Ins::GlobalGet(g));
    }

    pub fn global_set(&mut self, g: Global) {
        self.push(Ins::GlobalSet(g));
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
    /// Defined functions by symbol. A helper is defined when
    /// [`Module::finish`] finds a body naming it.
    funcs: BTreeMap<Sym, Func>,
    /// Instances in the order they were defined: content order.
    inst_order: Vec<Sym>,
    /// Static data from [`STATIC_BASE`], deduplicated by content.
    data: Vec<u8>,
    data_at: BTreeMap<Vec<u8>, u32>,
    /// Static vtables: each one's slots, then the vtables of its direct
    /// supertraits, laid out after the data.
    vtables: Vec<(Vec<Sym>, Vec<usize>)>,
    exports: Vec<(String, Sym)>,
    /// Development fixture roots, independent of the public export ABI.
    retained: BTreeSet<Sym>,
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
    /// Defines `s` as `f`.
    pub fn define(&mut self, s: Sym, f: Func) {
        if matches!(s, Sym::Inst(_) | Sym::Part(..)) {
            self.inst_order.push(s.clone());
        }
        self.funcs.insert(s, f);
    }

    /// Exports function `s` as `name`.
    pub fn export(&mut self, name: impl Into<String>, s: Sym) {
        self.exports.push((name.into(), s));
    }

    /// Keeps fixture instances and their generated suspension parts, without
    /// exporting them (wide signatures still use the internal lowering ABI).
    pub fn retain_instances(&mut self, keys: &[Hash128]) {
        let keys: BTreeSet<Hash128> = keys.iter().copied().collect();
        self.retained.extend(
            self.funcs
                .keys()
                .filter(|s| match s {
                    Sym::Inst(k) | Sym::Part(k, _) => keys.contains(k),
                    _ => false,
                })
                .cloned(),
        );
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
        let v = (slots, parents);
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
        self.funcs.retain(|s, _| seen.contains(s));
        self.inst_order.retain(|s| seen.contains(s));
    }

    /// Reuse the existing no-view link fact before cycle selection: a
    /// physical projection is an address offset plus its owned count.
    /// Removing its unreachable view helper also prevents that helper's
    /// allocation descriptor from needlessly linking the collector.
    fn physical_projections(&mut self) {
        let runtime = self.runtime;
        for f in self.funcs.values_mut() {
            let body = std::mem::take(&mut f.body);
            let origins = f.origins.as_mut().map(std::mem::take);
            let outer = f.origin_at;
            for (i, ins) in body.into_iter().enumerate() {
                f.origin_at = origins.as_ref().and_then(|o| o[i]);
                if let Ins::Call(Sym::Helper(crate::rt::Helper::RefProject {
                    offset,
                    borrowed,
                    ..
                })) = ins
                {
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
        }
    }

    /// What the exports reach, building the helpers among it.
    fn reached(&mut self) -> BTreeSet<Sym> {
        let mut seen: BTreeSet<Sym> = BTreeSet::new();
        let mut work: Vec<Sym> = self.exports.iter().map(|(_, s)| s.clone()).collect();
        work.extend(self.retained.iter().cloned());
        work.extend(self.vtables.iter().flat_map(|v| v.0.iter()).cloned());
        while let Some(s) = work.pop() {
            if matches!(s, Sym::Import(_)) || !seen.insert(s.clone()) {
                continue;
            }
            if let Sym::Helper(h) = &s
                && !self.funcs.contains_key(&s)
            {
                let f = crate::rt::helper(h, self);
                self.funcs.insert(s.clone(), f);
            }
            let Some(f) = self.funcs.get(&s) else {
                continue;
            };
            for i in &f.body {
                match i {
                    Ins::Call(c) | Ins::TableSlot(c) => work.push(c.clone()),
                    #[cfg(feature = "checked-report")]
                    Ins::Descriptor(c, _) => work.push(c.clone()),
                    Ins::VTable(k) => work.extend(self.vtables[*k].0.iter().cloned()),
                    _ => {}
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
        let original_runtime = self.runtime;
        for f in self.funcs.values_mut() {
            f.origins_before_opt = f
                .origins
                .as_ref()
                .map(|os| os.iter().map(|o| f.source_origin(*o)).collect())
                .unwrap_or_default();
        }
        let mut seen = self.reached();
        if !seen
            .iter()
            .any(|s| matches!(s, Sym::Helper(crate::rt::Helper::RefView { .. })))
        {
            self.physical_projections();
            seen = self.reached();
        }
        let cyclic = self
            .funcs
            .iter()
            .any(|(s, f)| seen.contains(s) && cyclic_desc(f));
        // Read the linked helper feature, including functions reused from
        // the code cache, rather than mutable state from fresh emission.
        self.fixed_allocations = seen
            .iter()
            .any(|s| matches!(s, Sym::Helper(crate::rt::Helper::NewFixed { .. })));
        if self.fixed_allocations {
            self.funcs.retain(|s, _| !matches!(s, Sym::Helper(_)));
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
            self.funcs.retain(|s, _| !matches!(s, Sym::Helper(_)));
            // Code that calls the collector itself, as std's own tests do,
            // still needs its state: run its module init after all.
            let seen = self.reached();
            if self
                .cycle_insts
                .iter()
                .any(|k| seen.contains(&Sym::Inst(*k)))
            {
                self.cycle_insts.clear();
                self.funcs.retain(|s, _| !matches!(s, Sym::Helper(_)));
            }
        }
        self.reach();
        let physical = !self
            .funcs
            .keys()
            .any(|s| matches!(s, Sym::Helper(crate::rt::Helper::RefView { .. })));
        for f in self.funcs.values_mut() {
            let body = std::mem::take(&mut f.body);
            let origins = f.origins.as_mut().map(std::mem::take);
            let outer = f.origin_at;
            for (position, ins) in body.into_iter().enumerate() {
                f.origin_at = origins.as_ref().and_then(|o| o[position]);
                match ins {
                    Ins::Call(Sym::Helper(crate::rt::Helper::RefIsView)) if physical => {
                        f.op(Instruction::Drop);
                        f.i32(0);
                    }
                    Ins::Call(Sym::Helper(crate::rt::Helper::RefViewsAbsent)) => {
                        f.i32(i32::from(physical));
                    }
                    ins => f.push(ins),
                }
            }
            f.origin_at = outer;
        }
        self.reach();
        crate::opt::constants(&mut self.funcs);
        for f in self.funcs.values_mut() {
            crate::opt::fold_constant_branches(f);
        }
        let arity: BTreeMap<Sym, (usize, usize)> = self
            .funcs
            .iter()
            .map(|(s, f)| (s.clone(), (f.params.len(), f.results.len())))
            .collect();
        for f in self.funcs.values_mut() {
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
            for f in self.funcs.values_mut() {
                if crate::opt::inline_counts(f, self.acyclic) {
                    crate::opt::locals(f, &arity);
                    inlined = true;
                }
            }
            if inlined {
                self.reach();
            }
        }
        let stack = crate::wide::lower(&mut self.funcs, &self.exports)?;
        // Every import and global a body names; imports come first.
        let mut imports: BTreeSet<Import> = BTreeSet::new();
        let mut globals: BTreeSet<Global> = BTreeSet::new();
        for f in self.funcs.values() {
            for i in &f.body {
                match i {
                    Ins::Call(Sym::Import(im)) | Ins::TableSlot(Sym::Import(im)) => {
                        imports.insert(im.clone());
                    }
                    Ins::GlobalGet(g) | Ins::GlobalSet(g) => {
                        globals.insert(g.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut index: BTreeMap<Sym, u32> = BTreeMap::new();
        let mut next = 0u32;
        for im in &imports {
            index.insert(Sym::Import(im.clone()), next);
            next += 1;
        }
        let mut defined: Vec<Sym> = self.inst_order.clone();
        defined.extend(
            self.funcs
                .keys()
                .filter(|s| matches!(s, Sym::Helper(_)))
                .cloned(),
        );
        for s in &defined {
            index.insert(s.clone(), next);
            next += 1;
        }
        let undefined = |s: &Sym| {
            NotImplemented::bug(
                Stage::Link,
                format!("the linear backend names an undefined function {s:?}"),
            )
        };
        // The function table: every function a slot or a vtable names,
        // in the order first named.
        let mut slots: Vec<u32> = Vec::new();
        let mut slot_of: BTreeMap<Sym, u32> = BTreeMap::new();
        let mut slot = |s: &Sym, slots: &mut Vec<u32>| -> StageResult<u32> {
            if let Some(k) = slot_of.get(s) {
                return Ok(*k);
            }
            let f = *index.get(s).ok_or_else(|| undefined(s))?;
            let k = u32::try_from(slots.len()).expect("table");
            slots.push(f);
            slot_of.insert(s.clone(), k);
            Ok(k)
        };
        #[cfg(feature = "checked-report")]
        let mut descriptors: BTreeMap<(Sym, u8), u32> = BTreeMap::new();
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
            .map(|(k, g)| (g.clone(), u32::try_from(k).expect("globals")))
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
        for im in &imports {
            names.append(
                index[&Sym::Import(im.clone())],
                &format!("{}/{}", im.module, im.name),
            );
        }
        // A helper name that several helpers share, at different
        // arguments, takes its place among them as a suffix. An instance
        // keeps its item's `path#sub`, which a panic's frames read.
        let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
        for s in &defined {
            *seen.entry(self.funcs[s].name.as_str()).or_default() += 1;
        }
        let mut taken: BTreeMap<&str, u32> = BTreeMap::new();
        let mut own: Vec<(u32, String)> = Vec::new();
        for s in &defined {
            let f = &self.funcs[s];
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
                        Ins::Call(s) => crate::views::count_action(s, self.runtime.as_ref())
                            .or_else(|| crate::views::count_action(s, original_runtime.as_ref())),
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
                if let (Ins::Call(_) | Ins::CallIndirect(..), Some(a)) = (i, at)
                    && rows.last().is_none_or(|&(_, last)| last != a)
                {
                    rows.push((u32::try_from(body.byte_len()).expect("offset"), a));
                }
                match i {
                    Ins::Anchor(a) => {
                        at = Some(*a);
                        &mut body
                    }
                    Ins::Op(op) => body.instruction(op),
                    Ins::Call(c) => body.instruction(&Instruction::Call(
                        *index.get(c).ok_or_else(|| undefined(c))?,
                    )),
                    Ins::GlobalGet(g) => body.instruction(&Instruction::GlobalGet(gindex[g])),
                    Ins::GlobalSet(g) => body.instruction(&Instruction::GlobalSet(gindex[g])),
                    #[cfg(feature = "checked-report")]
                    Ins::Descriptor(t, kind) => {
                        let k = descriptor_slot(t, *kind, &index, &mut slots, &mut descriptors)?;
                        body.instruction(&Instruction::I32Const(k.cast_signed()))
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
                            descriptor_slot(t, 0, &index, &mut slots, &mut descriptors)?
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
                        body.instruction(&Instruction::I32Const(k.cast_signed()))
                    }
                    Ins::CallIndirect(p, r) => body.instruction(&Instruction::CallIndirect {
                        type_index: ty(p, r, &mut types),
                        table_index: 0,
                    }),
                    Ins::VTable(k) => {
                        body.instruction(&Instruction::I32Const(vtable_at[*k].cast_signed()))
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
                                .map(|s| index.get(s).copied().ok_or_else(|| undefined(s)))
                                .collect::<StageResult<_>>()?;
                            statics.insert(vtable_at[*k], crate::StaticData::VTable(slots));
                        }
                        Ins::GlobalGet(g) | Ins::GlobalSet(g) => {
                            globals_named.insert(gindex[g], global_name(g, &self.storage_names));
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
            body.instruction(&Instruction::End);
            code.function(&body);
            if !rows.is_empty() {
                lines.push((index[s], rows));
            }
        }
        let mut tables = TableSection::new();
        let mut elements = ElementSection::new();
        // A `call_indirect` needs the table even when nothing fills it, as
        // release glue's does in a program whose objects hold no counted
        // reference.
        let indirect = self
            .funcs
            .values()
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
                *index.get(s).ok_or_else(|| undefined(s))?,
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
            for ((_, kind), slot) in &descriptors {
                data.extend_from_slice(&(slot + 1).to_le_bytes());
                data.push(*kind);
            }
            module.section(&wasm_encoder::CustomSection {
                name: "hd.cycles".into(),
                data: data.into(),
            });
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
    s: &Sym,
    kind: u8,
    index: &BTreeMap<Sym, u32>,
    slots: &mut Vec<u32>,
    ids: &mut BTreeMap<(Sym, u8), u32>,
) -> StageResult<u32> {
    let key = (s.clone(), kind);
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
        let glue = Sym::Inst(Hash128(1));
        let index = BTreeMap::from([(glue.clone(), 7)]);
        let mut slots = Vec::new();
        let mut ids = BTreeMap::new();
        assert_eq!(
            descriptor_slot(&glue, 1, &index, &mut slots, &mut ids).unwrap(),
            0
        );
        assert_eq!(
            descriptor_slot(&glue, 2, &index, &mut slots, &mut ids).unwrap(),
            1
        );
        assert_eq!(
            descriptor_slot(&glue, 1, &index, &mut slots, &mut ids).unwrap(),
            0
        );
        assert_eq!(slots, [7, 7]);
    }
}
