//! Runtime pieces the compiler generates (`crates/hd_wasm_linear/DESIGN.md`
//! §23.9): the entry exports, panics, the exchange-buffer copy, the bump
//! allocation that serves until the std allocator (job 5), integer text
//! and the host provider stubs. Each is a [`Helper`], built once on its
//! first request.

use crate::op::Op as Instruction;
use hd_base::{DefId, Hash128, StageResult};
use hd_types::{Ty, TyData, TyList};
use wasm_encoder::BlockType;

use crate::layout::{Leaf, Walk};
use crate::map::{self, KeyOps};
use crate::module::{
    BUF, CYCLIC, EMPTY, Func, GLUE, Global, HEADER, HEAP, Import, Ins, Module, Sym, Val, mem,
};
use crate::{Entry, Input, unsupported};

/// A `TypeId` value (`emit/dyn_value.rs`): the key and the printable name
/// it holds, two views of static data that are never counted.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeIdParts {
    pub name: String,
    pub shown: String,
}

/// Where one value of an optional map entry comes from in
/// [`Helper::MapStep`]: the tag, a value of the entry, or a zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OptVal {
    Tag,
    Elem(u32),
    Zero,
}

/// The layout of the value at a reference's address, which a vtable
/// slot reads to pass by value (`SlotRecv::ReadThrough`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefRead {
    pub value: RefLayout,
}

/// How a vtable slot whose method has a row passes its providers on: its
/// last parameter is a context (`emit/ctx.rs`), in which it looks up the
/// key ids `ids`, the target's own row in key order, and which it passes
/// on when the target has row parameters (`forward`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotCtx {
    pub ids: Vec<i64>,
    pub forward: bool,
}

/// Pushes slot parameters `1..=n` of `f`, then, for a slot that takes a
/// context as parameter `n + 1`, the providers of `ctx.ids` from it and
/// the context itself when the target takes it.
fn push_slot_params(f: &mut Func, n: usize, ctx: Option<&SlotCtx>) {
    for k in 1..=n {
        f.get(u32::try_from(k).expect("params"));
    }
    if let Some(sc) = ctx {
        let c = u32::try_from(n + 1).expect("params");
        let mut provs = Vec::new();
        for id in &sc.ids {
            provs.push(crate::emit::ctx_lookup(f, c, *id));
        }
        for [d, v] in provs {
            f.get(d);
            f.get(v);
        }
        if sc.forward {
            f.get(c);
        }
    }
}

/// A reference's value representation and its counted fields. A view's
/// getter/setter exchange erased values; concrete layouts box those that
/// are not already one counted address.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefLayout {
    pub size: u32,
    pub leaves: Vec<Leaf>,
    pub walk: Option<Walk>,
    /// The leaves a copy takes through the copy glue (`rt:coll_copy`).
    pub colls: Vec<u32>,
    pub boxed: bool,
    pub known: bool,
}

/// A generated function, by what it does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// A field reference forwards reads and writes through its parent view.
    RefProject {
        borrowed: bool,
        parent: RefLayout,
        field: RefLayout,
        first: u32,
        offset: u32,
    },
    RefFieldGet {
        parent: RefLayout,
        field: RefLayout,
        first: u32,
        offset: u32,
    },
    RefFieldSet {
        parent: RefLayout,
        field: RefLayout,
        first: u32,
        offset: u32,
    },
    /// Whether an address names a counted representation view.
    RefIsView,
    /// A link-time proof: no reachable representation-view constructor.
    RefViewsAbsent,
    /// A link-time fact: some reachable operation can cancel a task.
    CanCancel,
    /// Marks an explicit cancellation operation; erased before optimization.
    CancellationRoot,
    /// A view's descriptor releases its original reference.
    RefViewDrop,
    /// The canonical identity serial, following views and field projections.
    RefAddress,
    /// A counted view of a reference with this source representation.
    RefView {
        source: RefLayout,
    },
    /// A view's getter/setter: source-native values cross as erased ones.
    RefGet {
        source: RefLayout,
    },
    RefSet {
        source: RefLayout,
    },
    /// `hd.init`: the inits in order; a flagged one belongs to a module
    /// other than the entry's and runs as a forbidden context
    /// (`req.drive.block-on.forbidden-contexts`).
    EntryInit {
        inits: Vec<(Hash128, bool)>,
    },
    /// `hd.poll(ctx)`: runs `main` with one host provider per vtable in
    /// `providers` (data 0), passes its results to `report` when there is
    /// one, then runs `then` as the script's later inits; returns the exit
    /// status (0 without a report).
    EntryPoll {
        main: Hash128,
        providers: Vec<(usize, Option<Hash128>)>,
        results: u32,
        report: Option<Hash128>,
        then: Vec<(Hash128, bool)>,
        /// A suspending root's result values: `hd.poll` builds its frame
        /// once and polls it (§23.21), answering -1 while it is Pending.
        bang: Option<Vec<Val>>,
    },
    /// `(ctx) -> status`: checked mode's entry poll (§23.24), `poll`'s
    /// answer. Once that is not Pending, it frees the wake table, releases
    /// every top-level binding ([`Helper::Teardown`]) and audits that no
    /// block is left. A test case's instance serves the next case, so its
    /// end then runs `reinit`, every module's init again, which gives the
    /// next case the bindings a fresh instance has.
    CheckedPoll {
        poll: Box<Helper>,
        reinit: Vec<Hash128>,
    },
    /// `(ctx) -> status`: `poll`'s answer, after a cycle collection
    /// (§23.28): the entry is idle when the answer is Pending, and done
    /// otherwise. Checked mode collects in [`Helper::CheckedPoll`] instead.
    CollectPoll {
        poll: Box<Helper>,
    },
    /// `hd.reset()`: after a REPL input that trapped
    /// ([`crate::Entry::Inputs`]), clears what the trap left set: the
    /// forbidden-context and abandon counters, the root and its owner, the
    /// wake table (left to leak, as its frames may be gone) and a release
    /// in progress. The bindings and the heap stay.
    EntryReset,
    /// `()`: checked mode releases the counted references that each
    /// top-level binding holds, and zeroes them, before the audit at an
    /// entry's end.
    Teardown,
    /// `()`: a cycle collection (§23.28): std's `collect` with the three
    /// function values below, whose environments are empty.
    Collect,
    /// `(env, block, ctx)`: the collector's `trace`, the release glue that
    /// the descriptor of the object at `block` names, run on its payload
    /// while std's `release` hands each reference to the collector.
    CycleTrace,
    /// `(env, address, ctx)`: the collector's `release`, one count less on
    /// what `address` is in ([`Helper::ReleaseRef`]).
    CycleRelease,
    /// `(env, block, ctx)`: the collector's `free`, std's `free` of the
    /// block.
    CycleFree,
    /// `(address)`: tags the new frame at `address` for the collector
    /// (std's `opaque` entry). Idle frames use their release glue for
    /// tracing; a running frame's saved slots are stale and not entered.
    Opaque,
    /// `()`: the panic report `MESSAGE` (`CATEGORY: TEXT`), then a trap.
    Panic(String),
    /// `(text)`: the `explicit-panic` report of `panic(text)`.
    PanicStr,
    /// `(category, message)`: std's `panic_with`, whose report shows the
    /// message after the category when it is not empty
    /// (`flow.panic.report`).
    PanicWith,
    /// `(text) -> len`: copies a string into the exchange buffer at 0,
    /// growing the buffer to fit, and returns its length.
    ToBuf,
    /// `(x: f64, y: f64) -> f64`: the shared portable IEEE power helper.
    PowF64,
    /// `(x: f64, y: f64) -> f64`: the shared portable truncated remainder.
    RemF64,
    /// `(size) -> address`: `size` bytes of the heap, 8-aligned, from the
    /// bump pointer; the heap grows to fit, and a refused growth is
    /// `heap-exhausted`.
    Alloc,
    /// `(size, descriptor) -> address`: a new counted object (§23.4) of
    /// `size` payload bytes, from std's allocator: its count 1 and its
    /// descriptor word, the table slot of its [`Helper::DropFields`] plus
    /// one, or 0 when its payload holds no counted reference. The address
    /// is its payload's.
    New,
    /// The same ABI as New, with a statically known payload size and cycle bit.
    NewFixed {
        size: u32,
        cyclic: bool,
    },
    /// A known small class, with page-list maintenance on the std slow path.
    AllocClass {
        class: u32,
        bytes: u32,
    },
    /// Free an object using its descriptor class when available.
    FreeClass,
    /// `(len) -> address`: a new string buffer (§23.7) of `len` bytes, a
    /// counted object with nothing to release in it.
    Str,
    /// `(values)`: one more count on each counted reference in a value
    /// laid out as `vals`, which `walk` finds.
    Retain {
        vals: Vec<Val>,
        walk: Walk,
    },
    /// `(values)`: one count less on each, as [`Helper::ReleaseRef`] takes.
    Release {
        vals: Vec<Val>,
        walk: Walk,
    },
    /// `(address)`: one count less on the object that holds `address`. At
    /// its last count, the references in its payload are released through
    /// its descriptor and its block is freed.
    ReleaseRef,
    /// Count a statically known payload start without a block lookup.
    RetainKnown,
    ReleaseKnown,
    /// `(payload)`: the release glue of an object whose payload is laid
    /// out as `leaves`: one count less on each counted reference in it.
    DropFields {
        leaves: Vec<Leaf>,
        walk: Walk,
    },
    /// `(payload)`: one more count on each counted reference in a new
    /// object's payload, which its construction copied in.
    RetainFields {
        leaves: Vec<Leaf>,
        walk: Walk,
    },
    /// `(list) -> bool`: whether an element reference pins the list: its
    /// buffer has an owner word and a count over 1 (§23.20).
    ListPinned,
    /// `(list) -> list`: a copy of the list's buffer, whose elements take
    /// their own counts: how [`Helper::CollCopy`] copies a pinned list,
    /// through the owner word's slot.
    ListClone {
        elem: Elem,
    },
    /// `(capacity) -> buffer`: a new, empty list buffer with room for
    /// `capacity` elements laid out as `elem`.
    ListBuf {
        elem: Elem,
    },
    /// `(payload)`: the release glue of a list buffer: one count less on
    /// each counted reference of each element.
    DropElems {
        elem: Elem,
    },
    /// `(coll) -> coll`: a copy of a list or a map (`types.coll.value`):
    /// one more count on its buffer, or a clone through the owner word's
    /// slot when it is pinned or exposed.
    CollCopy,
    /// `(ctx, key) -> entry`: the entry of key id `key` in context `ctx`
    /// ([`ctx_elem`]), `LIST_ELEMS` before it. Checking covers every key a
    /// body looks up, so a miss is internal and traps.
    CtxFind,
    /// `(n, base) -> ctx`: a new context of `n` entries, which the caller
    /// fills, then the entries of context `base` (0 for none), which take
    /// counts of their own.
    CtxNew,
    /// `(at, n)`: the `n` elements at `at`, just copied from a buffer that
    /// keeps them too, take their own counts; each collection among an
    /// element's values becomes its copy ([`Helper::CollCopy`]).
    CopyElems {
        elem: Elem,
    },
    /// `(list) -> list`: the list's buffer made its own before a change:
    /// a buffer it shares is copied, and the place's count goes to the
    /// copy.
    ListUnique {
        elem: Elem,
    },
    /// `(list) -> list`: a `push`'s slow path: a pinned list panics
    /// (`ref.elem.pinned`); otherwise the list's buffer, its own already
    /// (an owned body's `IsUnique` made it so), moved to one with room
    /// for the `push` when full.
    ListGrow {
        elem: Elem,
    },
    /// `(capacity) -> table`: a new, empty map table (`map.rs`) with room
    /// for `capacity` entries laid out as `entry`, its index cleared.
    MapTable {
        entry: map::Entry,
    },
    /// `(payload)`: the release glue of a map table: one count less on
    /// each counted reference of each live entry.
    DropEntries {
        entry: map::Entry,
    },
    /// `(key) -> i32`: a key's bucket hash, made non-negative.
    MapHash {
        key: Vec<Val>,
        ops: KeyOps,
    },
    /// `(table, hash, key) -> entry`: the number of the key's entry, or -1.
    MapFind {
        entry: map::Entry,
        ops: KeyOps,
    },
    /// `(map, hash, key, value) -> map`: puts the entry, taking over one
    /// count of the key and of the value; the map's table is its own
    /// already, and moves when full.
    MapPut {
        entry: map::Entry,
        ops: KeyOps,
    },
    /// `(map) -> map`: the map's full table moved to one with room.
    MapGrow {
        entry: map::Entry,
    },
    /// `(map) -> map`: the map's table made its own before a change: a
    /// table it shares is copied, its entries taking their own counts. A
    /// table with an owner word has one holder (a pinned map).
    MapUnique {
        entry: map::Entry,
    },
    /// `(map) -> map`: a copy of the map's table: how [`Helper::CollCopy`]
    /// copies a pinned map, through the owner word's slot, as
    /// [`Helper::ListClone`] copies a pinned list.
    MapClone {
        entry: map::Entry,
    },
    /// `(map, pos, end) -> slot`: the first live entry in `pos..end`, or
    /// `end`.
    MapSlotNext,
    /// `(data, params..) -> results`: a vtable slot (`emit/dyn_value.rs`):
    /// calls `target` with the payload, loaded from the box at `data` as
    /// `leaves`, or `data` itself without them, then the parameters.
    /// The dynamic call lends its arguments; `own` has the walk of each of
    /// `target`'s parameters, the receiver first, that it owns
    /// (`layout::Pass`), and the slot counts a copy of each of those.
    DynSlot {
        target: Hash128,
        leaves: Option<Vec<Leaf>>,
        params: Vec<Val>,
        results: Vec<Val>,
        own: Vec<(u32, Option<Walk>)>,
        ctx: Option<SlotCtx>,
    },
    /// `(ref, params..) -> results`: a vtable slot whose payload is a
    /// reference and whose target takes the value: reads it through the
    /// reference's `get`, calls `target`, then releases the value read.
    DynRead {
        target: Hash128,
        read: RefRead,
        params: Vec<Val>,
        results: Vec<Val>,
        own: Vec<(u32, Option<Walk>)>,
        ctx: Option<SlotCtx>,
    },
    /// `(env, at, ctx) -> (usize, (K, V))?`: the step closure code of
    /// std's `map_steps` (`map.rs`).
    MapStep {
        entry: map::Entry,
        opt: Vec<OptVal>,
        results: Vec<Val>,
    },
    /// `(list, n) -> list`: std's `list_truncate`: a pinned list whose
    /// length would change panics; otherwise the buffer is made the list's
    /// own, the elements from `n` on are released and the length becomes
    /// `n`.
    ListTruncate {
        elem: Elem,
    },
    /// `() -> TypeId`: a `TypeId` of static strings.
    TypeIdOf(TypeIdParts),
    /// `(data) -> TypeId`: a vtable's `runtime_type` slot, the `TypeId`
    /// of the type the vtable is built at.
    RuntimeType(TypeIdParts),
    /// A vtable slot no call reaches: it traps.
    DynTrap {
        params: Vec<Val>,
        results: Vec<Val>,
    },
    /// `(list: List[u8]) -> string`: a new string of the list's bytes,
    /// which std has checked are UTF-8.
    StrFromBytes,
    /// `(list: List[string]) -> string`: every part's bytes in order,
    /// copied once into one new string.
    ConcatAll,
    /// `(value: i64) -> string`: an integer's decimal text, read as
    /// signed when `signed`.
    FmtInt {
        signed: bool,
    },
    /// `(a, b) -> bool`: two strings' bytes are equal.
    StrEq,
    /// `(a, b) -> i32`: -1, 0 or 1 as `a` sorts before, with or after `b`,
    /// byte by byte and a prefix first (`expr.ord.string`).
    StrCmp,
    /// `(at, len, from) -> index`: std's `bytes_index_json`, the first
    /// byte at or after `from` that JSON must escape, or `len` (#428).
    BytesIndexJson,
    /// `(args) -> result`: the `hd:prim` import `name` (spec/std/README.md,
    /// "Standard library primitives"). A string argument goes in the
    /// exchange buffer, its length the import's parameter; a string result
    /// comes back there, in the host codec, and is copied into a new
    /// buffer.
    HostPrim {
        name: String,
        args: Vec<PrimArg>,
        result: Option<Val>,
    },
    /// The forbidden-context check of `block_on`
    /// (`req.drive.block-on.forbidden-contexts`).
    ForbidCheck,
    /// A host provider's vtable slot for a method that may wait
    /// (`crates/hd_run_wasmtime/DESIGN.md` §17.2): `(data, arguments) ->
    /// frame`, a cold leaf frame (`task.rs`) holding the arguments laid
    /// out as `area`, whose poll is [`Helper::HostPoll`].
    HostCold {
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        res: crate::task::HostRes,
    },
    /// `(frame) -> (ready, results)`: the leaf's `.start`, then its
    /// `.finish` once the wake table holds its handle.
    HostPoll {
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        res: crate::task::HostRes,
    },
    /// `(frame)`: a leaf's cancel, which aborts a waiting call.
    HostCancel,
    /// A host provider's vtable slot for a method that never waits:
    /// `(data, arguments) -> results`, one import call, its structured
    /// arguments and result in the exchange buffer (`boundary.rs`).
    HostCall {
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        result: crate::boundary::CallRes,
    },
    /// `(data, params) -> results`: a host handle's vtable slot, which
    /// calls `inner`, a host call or a cold leaf, with the handle number
    /// from the box at `data` first (`boundary.rs`).
    HandleSlot {
        inner: Box<Helper>,
        params: Vec<Val>,
        results: Vec<Val>,
    },
    /// `(payload)`: remove the package handle's host binding on last release.
    PackageHandleRelease {
        module: String,
    },
    /// `(end)`: grows the exchange buffer to `end` bytes.
    Fit,
    /// `(off, v: i64) -> off'`: unsigned LEB128 into the buffer.
    LebPut,
    /// `(off) -> (v: i64, off')`: unsigned LEB128 from the buffer.
    LebGet,
    /// `(off, values) -> off'`: a boundary value's encoder.
    Enc(crate::boundary::LTy),
    /// `(off) -> (values, off')`: a boundary value's decoder.
    Dec(crate::boundary::LTy),
    /// `() -> frame`: a new frame of the suspending instance (§23.21).
    Frame(Hash128),
    /// `(frame) -> i32`: a poll's runtime checks; 1 answers Pending.
    PollCheck,
    /// `(frame) -> i32`: a cancel's runtime checks; 0 leaves the frame.
    CancelCheck,
    /// `() -> i32`: starts a cancel's `ABANDONED` carry.
    CancelEnter,
    /// `(frame, seen)`: ends it.
    CancelLeave,
    /// `(n)`: `hd.wake(n)`, the completed handles into the wake table.
    WakeMark,
    /// `(h) -> i32`: whether handle `h` completed, which it takes.
    WakeTake,
    /// `(handle) -> address`: grows the 16-byte wake registrations.
    WakeSlot,
    /// `(handle)`: registers the current waker, or removes it.
    WakeRegister,
    WakeRemove,
    /// `(node, bit)`: marks the child and each enclosing all!.
    WakeSignal,
    /// `(mask) -> node`; `(node)` updates its enclosing waker.
    WakerNew,
    WakerParent,
    /// Weak registry allocation, lookup and node release glue.
    WakerSlot,
    WakerLive,
    WakerDrop,
    /// `(head slot, parent, bit)`: adds a distinct waiter; `(head)` notifies it.
    WakeLink,
    WakeNotify,
    /// `hd.cancel` and `hd.abandon`: cancels the pending root, under
    /// `abandon()` for the second.
    EntryCancel {
        abandon: bool,
    },
    /// `hd.scenario.cleanup`: `() -> i32`, a call of the scenario's
    /// top-level `cleanup_ran`.
    EntryCall {
        func: Hash128,
    },
    /// `(address)`: one more count on what the address is in, when the
    /// program counts.
    RetainAny,
    /// `(payload)`: one count less on a frame's child.
    ChildDrop,
    /// `(task) -> frame`: `cancel!`'s cleanup frame (`task.rs`).
    CleanupCold,
    /// `(frame) -> (ready, abandoned)`.
    CleanupPoll,
    /// `(frame)`.
    CleanupCancel,
    /// `() -> frame`: a frame that never completes, of results `results`.
    PendingCold {
        results: Vec<Val>,
    },
    /// `(frame) -> (ready, results)`: always Pending.
    PendingPoll {
        results: Vec<Val>,
    },
    /// `(frame)`.
    PendingCancel,
    /// `race!`'s frame over a list of tasks of results `results`: part 0
    /// `(list) -> frame`, 1 its poll, 2 its cancel.
    Race {
        part: u8,
        results: Vec<Val>,
        walk: Option<Walk>,
    },
    /// `(payload)`: a race frame's glue.
    RaceDrop {
        results: Vec<Val>,
        walk: Option<Walk>,
    },
    /// A flat homogeneous all frame: constructor, poll, cancel, release
    /// glue, ready-tree traversal, and first-poll tree initialization.
    All {
        part: u8,
        elem: Elem,
    },
    /// A kept suspension of a generic method called through a trait
    /// value, over the erased frame whose poll answers an open value
    /// (`emit/erased.rs`): part 0 `(child) -> frame`, 1 its poll, which
    /// answers the value laid out as `leaves` that the open value holds, a
    /// box of it or 0 when it has no leaves. `walk` finds its counted
    /// references, and `counts` says whether the program counts. Its
    /// cancel is [`Helper::CleanupCancel`] and its glue
    /// [`Helper::ChildDrop`].
    OpenFrame {
        part: u8,
        reference: Option<RefLayout>,
        leaves: Vec<Leaf>,
        walk: Option<Walk>,
        counts: bool,
    },
}

/// A list's element as the list glue sees it: its stride, its values in
/// memory, which of them are counted, which are collections that a
/// copy of the element copies (`layout::coll_leaves`), and whether the
/// list's header and buffers take part in cycle collection
/// (`crate::cycle`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Elem {
    pub size: u32,
    pub leaves: Vec<Leaf>,
    pub walk: Option<Walk>,
    pub colls: Vec<u32>,
    pub cyclic: bool,
}

impl Elem {
    /// Whether an element copied out of a shared buffer needs more than
    /// its bytes: counts, or copies of its collections.
    #[must_use]
    pub fn counts(&self) -> bool {
        self.walk.is_some() || !self.colls.is_empty()
    }
}

/// The owner word of a list buffer and of a map table (§23.7), at the same
/// payload offset of both: 0 while the buffer may be shared, or, once an
/// element reference pinned the collection, the function-table slot of
/// its [`Helper::ListClone`] or [`Helper::MapClone`] plus one, with
/// [`LIST_EXPOSED`] for transitive exposure. A buffer whose owner word is
/// set has one holder, so each count past the first is an element
/// reference's: the collection is pinned while its buffer's count is over
/// 1 (§23.20).
pub const LIST_OWN: u64 = 8;

/// A list containing collection values has exposed an element. Keep its
/// buffer unshared even after the direct pin ends: a descendant reference
/// may still be alive. The remaining owner bits name the clone glue.
pub const LIST_EXPOSED: u32 = 1 << 31;

/// A list buffer's payload: the capacity in elements, the length, the
/// owner word ([`LIST_OWN`]), then the elements inline from
/// [`LIST_ELEMS`].
pub const LIST_LEN: u64 = 4;
pub const LIST_ELEMS: u32 = 16;

/// A context (`crates/hd_mono/DESIGN.md` §12.4, `emit/ctx.rs`): a list
/// buffer whose elements are entries of [`CTX_ENTRY`] bytes, each a key id
/// (`i64`) and its provider, the trait value's counted data and its
/// vtable. A function value's code and a function with row parameters
/// take one, 0 for none, and look their keys up in it; a `$.Context`
/// value is one too, its entries in key order.
#[must_use]
pub fn ctx_elem() -> Elem {
    let leaf = |offset, val, bytes| Leaf {
        offset,
        val,
        bytes,
        signed: false,
    };
    Elem {
        size: CTX_ENTRY,
        leaves: vec![
            leaf(0, Val::I64, 8),
            leaf(8, Val::I32, 4),
            leaf(12, Val::I32, 4),
        ],
        walk: Some(Walk::Ref(1)),
        colls: Vec::new(),
        cyclic: true,
    }
}

/// A context entry's size, and its provider's data and vtable offsets
/// from the address `LIST_ELEMS` before it: past the key id.
pub const CTX_ENTRY: u32 = 16;
pub const CTX_DATA: u64 = 24;
pub const CTX_VTABLE: u64 = 28;

/// The address of the one static empty buffer, shared by every empty list
/// and map literal: its payload's 24 zero bytes are an empty list buffer
/// and an empty map table. Its count word is 0, a static object's: no
/// count changes it, and as no count is 1, a change always copies it
/// first. An empty list's value is this buffer itself.
pub fn empty_buffer(m: &mut Module) -> u32 {
    let bytes = [0u8; 32];
    let at = m.static_bytes(&bytes, 8) + HEADER;
    m.label_static(at, "empty-buffer");
    at
}

/// How a host primitive's argument crosses: a string, or a scalar as
/// itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrimArg {
    Str,
    Scalar(Val),
}

/// Defines the entry exports of `input.entry` in `m`.
pub fn entry(input: &Input<'_>, m: &mut Module) -> StageResult<()> {
    let (main, key, before, after, report) = match &input.entry {
        Entry::Fixture { roots } => {
            m.retain_instances(roots);
            return Ok(());
        }
        Entry::Main {
            main,
            key,
            inits,
            report,
        } => (*main, *key, inits.clone(), Vec::new(), *report),
        Entry::Scenario {
            main,
            key,
            inits,
            report,
            cleanup,
            ..
        } => {
            if let Some(func) = cleanup {
                m.export(
                    "hd.scenario.cleanup",
                    Sym::Helper(Helper::EntryCall { func: *func }),
                );
            }
            (*main, *key, inits.clone(), Vec::new(), *report)
        }
        Entry::Script {
            init,
            key,
            before,
            after,
            report,
        } => (*init, *key, before.clone(), after.clone(), *report),
        Entry::Tests {
            cases,
            inits,
            runtime_inits,
        } => {
            // The module under test is not an entry module
            // (`module.init.tests.no-entry`), so every init is forbidden.
            for (j, ks) in inits.iter().enumerate() {
                let inits = ks.iter().map(|k| (*k, true)).collect();
                m.export(
                    format!("hd.init.{j}"),
                    Sym::Helper(Helper::EntryInit { inits }),
                );
            }
            let mut bang = false;
            let mut reinit: Vec<Hash128> = Vec::new();
            for k in inits.iter().flatten() {
                if !reinit.contains(k) && !runtime_inits.contains(k) {
                    reinit.push(*k);
                }
            }
            for (i, (def, key, report)) in cases.iter().enumerate() {
                let poll = poll(input, m, *def, *key, *report, Vec::new())?;
                let poll = checked(m, poll, reinit.clone());
                bang |= m.frames.contains_key(key);
                m.export(format!("hd.test.{i}"), poll);
            }
            if bang {
                m.export("hd.wake", Sym::Helper(Helper::WakeMark));
            }
            #[cfg(feature = "checked-report")]
            if let Some(c) = m.runtime.and_then(|r| r.checked) {
                m.export("hd.cycles.pointer", Sym::Inst(c.pointer));
                m.export("hd.cycles.length", Sym::Inst(c.length));
            }
            return Ok(());
        }
        Entry::Inputs(inputs) => {
            let mut bang = false;
            for i in inputs {
                let n = i.number;
                m.export(
                    format!("hd.init.{n}"),
                    Sym::Helper(Helper::EntryInit {
                        inits: i.before.clone(),
                    }),
                );
                let poll = poll(input, m, i.init, i.key, None, i.after.clone())?;
                m.export(format!("hd.poll.{n}"), collecting(m, poll));
                bang |= m.frames.contains_key(&i.key);
            }
            m.export("hd.reset", Sym::Helper(Helper::EntryReset));
            if bang {
                m.export("hd.wake", Sym::Helper(Helper::WakeMark));
                m.export(
                    "hd.cancel",
                    Sym::Helper(Helper::EntryCancel { abandon: false }),
                );
                m.export(
                    "hd.abandon",
                    Sym::Helper(Helper::EntryCancel { abandon: true }),
                );
            }
            return Ok(());
        }
        Entry::Unsupported(what) => return unsupported(*what),
    };
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: before }));
    let poll = poll(input, m, main, key, report, after)?;
    // A scenario's host reads its bindings through
    // `hd.scenario.cleanup` after the root has ended, so it keeps them
    // and has no audit.
    let poll = if matches!(input.entry, Entry::Scenario { .. }) {
        collecting(m, poll)
    } else {
        checked(m, poll, Vec::new())
    };
    m.export("hd.poll", poll);
    // Checked mode's count totals, for measuring elision (§23.24).
    if let Some(c) = m.runtime.and_then(|r| r.checked) {
        m.export("hd.retained", Sym::Inst(c.retained));
        m.export("hd.released", Sym::Inst(c.released));
        #[cfg(feature = "checked-report")]
        {
            m.export("hd.cycles.pointer", Sym::Inst(c.pointer));
            m.export("hd.cycles.length", Sym::Inst(c.length));
        }
    }
    if m.frames.contains_key(&key) {
        m.export("hd.wake", Sym::Helper(Helper::WakeMark));
        m.export(
            "hd.cancel",
            Sym::Helper(Helper::EntryCancel { abandon: false }),
        );
        m.export(
            "hd.abandon",
            Sym::Helper(Helper::EntryCancel { abandon: true }),
        );
    }
    Ok(())
}

/// The poll of a root: item `main`'s instance `key` with its host
/// providers, its result reported by `report`, then the inits `then`.
fn poll(
    input: &Input<'_>,
    m: &mut Module,
    main: DefId,
    key: Hash128,
    report: Option<Hash128>,
    then: Vec<(Hash128, bool)>,
) -> StageResult<Sym> {
    let mut providers = Vec::new();
    for k in input.env.row_keys(main, TyList::EMPTY) {
        let by_ref = input.host_refs.iter().find(|(t, _)| *t == k).map(|r| r.1);
        providers.push((host_vtable(input, m, k)?, by_ref));
    }
    let ret = input.env.ret(main).unwrap_or(Ty::VOID);
    let rvals = crate::layout::vals(input.pool, input.env, ret)?;
    let bang = m.frames.contains_key(&key).then(|| rvals.clone());
    Ok(Sym::Helper(Helper::EntryPoll {
        main: key,
        providers,
        results: u32::try_from(rvals.len()).expect("results"),
        report,
        then,
        bang,
    }))
}

/// In checked mode, `poll` behind the audit at its end
/// ([`Helper::CheckedPoll`]), then the inits `reinit`; with the runtime,
/// `poll` behind a cycle collection ([`Helper::CollectPoll`]); else `poll`
/// itself.
fn checked(m: &Module, poll: Sym, reinit: Vec<Hash128>) -> Sym {
    match poll {
        Sym::Helper(h) if m.runtime.is_some_and(|r| r.checked.is_some()) => {
            Sym::Helper(Helper::CheckedPoll {
                poll: Box::new(h),
                reinit,
            })
        }
        other => collecting(m, other),
    }
}

/// `poll` behind a cycle collection ([`Helper::CollectPoll`]) when the
/// program links the runtime.
fn collecting(m: &Module, poll: Sym) -> Sym {
    match poll {
        Sym::Helper(h) if m.runtime.is_some() => {
            Sym::Helper(Helper::CollectPoll { poll: Box::new(h) })
        }
        other => other,
    }
}

/// [`Helper::CheckedPoll`].
fn checked_poll(poll: &Helper, reinit: &[Hash128], m: &Module) -> Func {
    let mut f = m.func("rt:checked_poll", vec![Val::I32], vec![Val::I32]);
    let Some((r, c)) = m.runtime.and_then(|r| Some((r, r.checked?))) else {
        return f;
    };
    let status = f.local(Val::I32);
    f.get(0);
    f.helper(poll.clone());
    f.set(status);
    f.get(status);
    f.i32(-1);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    // The wake table is the runtime's own, grown on demand: no handle
    // waits once the root has ended.
    f.global_get(Global::WakeTab);
    f.op(Instruction::If(BlockType::Empty));
    // A raw block, never a counted object: the free that skips the
    // collector's candidate test.
    f.global_get(Global::WakeTab);
    f.call(Sym::Inst(r.acyclic.free));
    f.i32(0);
    f.global_set(Global::WakeTab);
    f.i32(0);
    f.global_set(Global::WakeCap);
    f.op(Instruction::End);
    f.helper(Helper::Teardown);
    // The bindings are gone, so a cycle they held is garbage now, and a
    // handle leaked inside one is still reported (§23.28).
    if !m.acyclic {
        f.helper(Helper::Collect);
    }
    // Nodes invalidate their weak slots during teardown/collection, so
    // keep the registry until both have completed.
    if m.wakers {
        f.global_get(Global::NodeTab);
        f.op(Instruction::If(BlockType::Empty));
        f.global_get(Global::NodeTab);
        f.call(Sym::Inst(r.acyclic.free));
        for g in [
            Global::NodeTab,
            Global::NodeCap,
            Global::NodeLen,
            Global::NodeFree,
        ] {
            f.i32(0);
            f.global_set(g);
        }
        f.op(Instruction::End);
    }
    f.i32(0);
    f.call(Sym::Inst(c.audit));
    for k in reinit {
        call_init(&mut f, *k, true);
    }
    f.op(Instruction::Else);
    if !m.acyclic {
        f.helper(Helper::Collect);
    }
    f.op(Instruction::End);
    f.get(status);
    f
}

/// [`Helper::Teardown`]: each binding [`Module::stored`] names releases
/// its values, from its globals or its static slot, and is zeroed.
fn teardown(m: &Module) -> Func {
    let mut f = m.func("rt:teardown", vec![], vec![]);
    for (h, s) in &m.stored {
        let ls: Vec<u32> = s.vals.iter().map(|v| f.local(*v)).collect();
        for (k, (v, l)) in s.vals.iter().zip(&ls).enumerate() {
            let g = Global::Storage(*h, u32::try_from(k).expect("k"), *v);
            if let Some((at, leaves)) = &s.slot {
                f.u32(*at);
                f.op(leaves[k].load(0));
            } else {
                f.global_get(g);
            }
            f.set(*l);
        }
        if let Walk::Ref(k) = s.walk {
            f.get(ls[k as usize]);
            f.helper(Helper::ReleaseRef);
        } else {
            for l in &ls {
                f.get(*l);
            }
            f.helper(Helper::Release {
                vals: s.vals.clone(),
                walk: s.walk.clone(),
            });
        }
        // Zeros release as nothing, so a later init's store of the
        // binding releases nothing twice.
        for (k, v) in s.vals.iter().enumerate() {
            let g = Global::Storage(*h, u32::try_from(k).expect("k"), *v);
            if let Some((at, leaves)) = &s.slot {
                f.u32(*at);
                crate::task::zero(&mut f, *v);
                f.op(leaves[k].store(0));
            } else {
                crate::task::zero(&mut f, *v);
                f.global_set(g);
            }
        }
    }
    f
}

/// Pushes each host provider: no data beside its vtable of host stubs; for
/// a key whose trait has a `&self` or `&mut self` method, that trait value
/// behind a new object (`req.ref.host.reference-values`), the reference that
/// `std.rt.host_provider` returns, beside the vtable again.
pub fn host_providers(f: &mut Func, providers: &[(usize, Option<Hash128>)]) {
    for (v, by_ref) in providers {
        f.i32(0);
        f.push(Ins::VTable(*v));
        if let Some(k) = by_ref {
            f.call(Sym::Inst(*k));
        }
    }
}

/// The vtable of the host's provider of capability `key`, in the trait's
/// method order (`ProgramEnv::trait_methods`): each slot a host call or a
/// host leaf over the exchange buffer's codecs (`boundary.rs`), or a trap
/// with no import for a method no reachable code calls.
fn host_vtable(input: &Input<'_>, m: &mut Module, key: Ty) -> StageResult<usize> {
    let TyData::TraitValue { def, args, .. } = input.pool.get(key) else {
        return unsupported("a provider key that is not a trait");
    };
    let p = (input.path)(def).replace('/', ".");
    if args != TyList::EMPTY {
        return unsupported(format!("the default provider of `{p}`"));
    }
    let Some(host) = hd_host_abi::host_trait(&p) else {
        return profile_vtable(input, m, def, &p);
    };
    let mut slots = Vec::new();
    for method in input.env.trait_methods(def) {
        // A method no reachable code calls links no host method.
        if !input.collected.calls_dyn(method) {
            slots.push(Sym::Helper(crate::boundary::uncalled_slot(input, method)?));
            continue;
        }
        let mp = (input.path)(method);
        let name = mp.rsplit(['.', '/']).next().unwrap_or("");
        let Some(hm) = host.methods.iter().find(|x| x.name == name) else {
            return unsupported(format!("the host method `{name}`"));
        };
        let row = crate::boundary::Row::of(hm);
        slots.push(Sym::Helper(crate::boundary::host_slot(
            input, m, host.key, &row, method, false, None,
        )?));
    }
    Ok(m.vtable(slots))
}

/// The vtable of a host trait that a runtime profile declares itself
/// (`spec/conformance/README.md`, Runtime Profiles), which the ABI table
/// does not list: each method is imported from `hd:<Trait>` by its name
/// and waits when it suspends; every argument and result crosses in the
/// exchange buffer, so a result gets each check of its declared type
/// (`module.profile.host-result.*`), and a private-field type crosses as
/// its JSON text (`module.boundary.serialize.exact`).
fn profile_vtable(
    input: &Input<'_>,
    m: &mut Module,
    def: hd_base::DefId,
    p: &str,
) -> StageResult<usize> {
    let package = (input.package_host)(def);
    let module = package
        .as_ref()
        .map(|identity| crate::boundary::package_module(identity, false));
    let key = module
        .as_deref()
        .and_then(|m| m.strip_prefix("hd:"))
        .unwrap_or_else(|| p.rsplit('.').next().unwrap_or(""));
    let at = crate::boundary::AtProfile {
        codecs: &input.collected.boundary,
        out: false,
        handles: &[],
        package: package.is_some(),
        arguments: None,
    };
    let mut methods = Vec::new();
    let vtable = profile_tree(input, m, def, def, key, at, &mut methods, &mut Vec::new())?;
    if let (Some(identity), Some(module)) = (package, module) {
        crate::boundary::record_package_interface(
            m,
            hd_host_abi::schema::Interface {
                identity,
                module,
                selectors: (input.package_selectors)(def),
                handle: false,
                methods,
            },
        )?;
    }
    Ok(vtable)
}

fn profile_tree(
    input: &Input<'_>,
    m: &mut Module,
    def: hd_base::DefId,
    target: hd_base::DefId,
    key: &str,
    at: crate::boundary::AtProfile<'_>,
    methods: &mut Vec<hd_host_abi::schema::Method>,
    seen: &mut Vec<hd_base::DefId>,
) -> StageResult<usize> {
    use hd_host_abi::{Codec, Wait};
    if seen.contains(&def) {
        return unsupported("cyclic package host supertraits");
    }
    seen.push(def);
    let mut slots = Vec::new();
    for method in input.env.trait_methods(def) {
        if !input.collected.calls_dyn(method) {
            slots.push(Sym::Helper(crate::boundary::uncalled_slot_at(
                input,
                method,
                at.arguments,
            )?));
            continue;
        }
        let mp = (input.path)(method);
        let member = mp.rsplit(['.', '/']).next().unwrap_or("");
        let name = if at.package {
            crate::boundary::package_wire_name(input, target, def, member)?
        } else {
            member.to_owned()
        };
        let (params, ret) = crate::boundary::signature(input.pool, input.env, method, at.arguments);
        let codecs = vec![Codec::Buffer(""); params.len()];
        let row = crate::boundary::Row {
            name: &name,
            params: &codecs,
            result: if crate::layout::vals(input.pool, input.env, ret)?.is_empty() {
                Codec::Void
            } else {
                Codec::Buffer("")
            },
            wait: if input.env.suspends(method) {
                Wait::May
            } else {
                Wait::Never
            },
        };
        slots.push(Sym::Helper(crate::boundary::host_slot(
            input,
            m,
            key,
            &row,
            method,
            false,
            Some(at),
        )?));
    }
    if at.package {
        for method in input
            .env
            .trait_methods(def)
            .into_iter()
            .filter(|d| input.collected.calls_dyn(*d))
        {
            crate::boundary::add_package_method(
                methods,
                crate::boundary::package_method(input, method, target, def, at.arguments)?,
            )?;
        }
    }
    let mut parents = Vec::new();
    if at.package {
        for &sup in input.env.decls().supertraits(def) {
            let full = at
                .arguments
                .unwrap_or_else(|| input.pool.list(&[hd_mono::class_ref(input.pool)]));
            let sup = hd_mono::subst(input.pool, input.env, def, full, sup);
            let TyData::TraitValue {
                def: parent, args, ..
            } = input.pool.get(sup)
            else {
                return unsupported("a host supertrait that is not a trait");
            };
            let args = input.pool.list(
                &std::iter::once(hd_mono::class_ref(input.pool))
                    .chain(input.pool.list_items(args).iter().copied())
                    .collect::<Vec<_>>(),
            );
            parents.push(profile_tree(
                input,
                m,
                parent,
                target,
                key,
                crate::boundary::AtProfile {
                    arguments: Some(args),
                    ..at
                },
                methods,
                seen,
            )?);
        }
    }
    seen.pop();
    Ok(m.vtable_with(slots, parents))
}

/// Adds `by` to the forbidden-context counter.
fn forbid_by(f: &mut Func, by: i32) {
    f.global_get(Global::Forbid);
    f.i32(by);
    f.op(Instruction::I32Add);
    f.global_set(Global::Forbid);
}

/// Calls init `k`, as a forbidden context when flagged.
fn call_init(f: &mut Func, k: Hash128, forbid: bool) {
    if forbid {
        forbid_by(f, 1);
    }
    f.call(Sym::Inst(k));
    if forbid {
        forbid_by(f, -1);
    }
}

fn rt_import(name: &str, params: &[Val], results: &[Val]) -> Sym {
    Sym::Import(Import::new("hd:rt", name, params, results))
}

/// [`Helper::DynRead`]'s function: the value at the data's address, lent
/// to the call as the receiver.
fn dyn_read(
    target: Hash128,
    read: &RefRead,
    params: &[Val],
    results: &[Val],
    own: &[(u32, Option<Walk>)],
    ctx: Option<&SlotCtx>,
    m: &Module,
) -> Func {
    let mut all = vec![Val::I32];
    all.extend(params.iter().copied());
    let mut f = m.func("rt:dyn_read", all, results.to_vec());
    let held = ref_load(&mut f, 0, &read.value);
    let rvals: Vec<Val> = ref_vals(&read.value);
    let recv = |f: &mut Func| {
        for l in &held {
            f.get(*l);
        }
    };
    // The read gave the receiver a count of its own: a target that owns
    // the receiver takes it over, and it is released after one that
    // borrows it. Only the other owned arguments take counts here.
    let mut owned = own.to_vec();
    let recv_owned = owned.first_mut().is_some_and(|r| r.1.take().is_some());
    own_lent(&mut f, &rvals, &recv, params, &owned, m);
    recv(&mut f);
    push_slot_params(&mut f, params.len() - usize::from(ctx.is_some()), ctx);
    f.call(Sym::Inst(target));
    if !recv_owned {
        ref_count(&mut f, &held, &read.value, false);
    }
    f
}

/// Writes the literal `text` to standard error.
fn write_lit(f: &mut Func, m: &mut Module, text: &str) {
    let at = m.static_bytes(text.as_bytes(), 1);
    f.u32(at);
    f.u32(u32::try_from(text.len()).expect("literal"));
    f.helper(Helper::ToBuf);
    f.call(rt_import("stderr", &[Val::I32], &[]));
}

/// Writes the string in locals `s` and `s + 1` to standard error.
fn write_str(f: &mut Func, s: u32) {
    f.get(s);
    f.get(s + 1);
    f.helper(Helper::ToBuf);
    f.call(rt_import("stderr", &[Val::I32], &[]));
}

/// Builds helper `h`; it may ask `m` for more helpers and static data.
pub fn helper(h: &Helper, m: &mut Module) -> Func {
    #[cfg_attr(not(feature = "checked-report"), allow(unused_mut))]
    let mut f = match h {
        Helper::CanCancel => {
            let mut f = m.func("rt:can_cancel", vec![], vec![Val::I32]);
            f.i32(i32::from(m.cancellable));
            f
        }
        Helper::CancellationRoot => m.func("rt:cancellation_root", vec![], vec![]),
        Helper::RefViewsAbsent => {
            let mut f = m.func("rt:ref_views_absent", vec![], vec![Val::I32]);
            f.i32(0);
            f
        }
        Helper::RefProject { .. } | Helper::RefFieldGet { .. } | Helper::RefFieldSet { .. } => {
            ref_field_helper(h, m)
        }
        Helper::RefIsView
        | Helper::RefViewDrop
        | Helper::RefAddress
        | Helper::RefView { .. }
        | Helper::RefGet { .. }
        | Helper::RefSet { .. } => ref_helper(h, m),
        Helper::PowF64 | Helper::RemF64 => {
            let (name, code) = if matches!(h, Helper::PowF64) {
                ("rt:pow_f64", crate::pow::pow_f64_code())
            } else {
                ("rt:rem_f64", crate::rem::rem_f64_code())
            };
            let mut f = m.func(name, vec![Val::F64, Val::F64], vec![Val::F64]);
            f.raw = Some(code);
            f
        }
        Helper::Frame(_)
        | Helper::PollCheck
        | Helper::CancelCheck
        | Helper::CancelEnter
        | Helper::CancelLeave
        | Helper::WakeMark
        | Helper::WakeTake
        | Helper::WakeSlot
        | Helper::WakeRegister
        | Helper::WakeRemove
        | Helper::WakeSignal
        | Helper::WakerNew
        | Helper::WakerParent
        | Helper::WakerSlot
        | Helper::WakerLive
        | Helper::WakerDrop
        | Helper::WakeLink
        | Helper::WakeNotify
        | Helper::HostCold { .. }
        | Helper::HostPoll { .. }
        | Helper::HostCancel
        | Helper::HostCall { .. }
        | Helper::EntryCancel { .. }
        | Helper::ChildDrop
        | Helper::CleanupCold
        | Helper::CleanupPoll
        | Helper::CleanupCancel
        | Helper::PendingCold { .. }
        | Helper::PendingPoll { .. }
        | Helper::PendingCancel
        | Helper::Race { .. }
        | Helper::RaceDrop { .. }
        | Helper::All { .. }
        | Helper::OpenFrame { .. } => crate::task::helper(h, m),
        Helper::HandleSlot {
            inner,
            params,
            results,
        } => crate::boundary::handle_slot(m, inner, params, results),
        Helper::PackageHandleRelease { module } => {
            let mut f = m.func("rt:package_handle_release", vec![Val::I32], vec![]);
            if !m.acyclic {
                f.global_get(Global::Tracing);
                f.op(Instruction::If(BlockType::Empty));
                f.op(Instruction::Return);
                f.op(Instruction::End);
            }
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
            f.call(Sym::Import(Import {
                module: module.clone(),
                name: "$release".into(),
                params: vec![Val::I32],
                results: vec![],
            }));
            f
        }
        Helper::Fit => crate::boundary::fit(m),
        Helper::LebPut => crate::boundary::leb_put(m),
        Helper::LebGet => crate::boundary::leb_get(m),
        Helper::Enc(b) => crate::boundary::enc(b, m),
        Helper::Dec(b) => crate::boundary::dec(b, m),
        Helper::EntryCall { func } => {
            let mut f = m.func("rt:entry_call", vec![], vec![Val::I32]);
            f.call(Sym::Inst(*func));
            f
        }
        Helper::EntryReset => {
            let mut f = m.func("rt:entry_reset", vec![], vec![]);
            for g in [
                Global::Forbid,
                Global::Aband,
                Global::AbSeen,
                Global::Rewake,
                Global::Root,
                Global::Owner,
                Global::WakeTab,
                Global::WakeCap,
                Global::Waker,
                Global::WakeBit,
                Global::Releasing,
                Global::Tracing,
            ] {
                f.i32(0);
                f.global_set(g);
            }
            f
        }
        Helper::EntryInit { inits } => {
            let mut f = m.func("rt:entry_init", vec![], vec![]);
            for (k, forbid) in inits {
                // A program that links no collector needs no collector state.
                if !(m.acyclic && m.cycle_insts.contains(k)) {
                    call_init(&mut f, *k, *forbid);
                }
            }
            f
        }
        Helper::EntryPoll {
            main,
            providers,
            results: _,
            report,
            then: _,
            bang: Some(rvals),
        } => crate::task::root_poll(m, *main, providers, rvals, *report),
        Helper::EntryPoll {
            main,
            providers,
            results,
            report,
            then,
            bang: None,
        } => {
            // Parameter 0 is the driver's `PollContext`; a plain entry
            // ignores it.
            let mut f = m.func("rt:entry_poll", vec![Val::I32], vec![Val::I32]);
            host_providers(&mut f, providers);
            f.call(Sym::Inst(*main));
            if let Some(k) = report {
                f.call(Sym::Inst(*k));
            } else {
                for _ in 0..*results {
                    f.op(Instruction::Drop);
                }
                // A script's later inits: its own bodies, which share the
                // first's providers and result, and the others, as
                // forbidden contexts.
                for (k, forbid) in then {
                    if !forbid {
                        host_providers(&mut f, providers);
                    }
                    call_init(&mut f, *k, *forbid);
                    if !forbid {
                        for _ in 0..*results {
                            f.op(Instruction::Drop);
                        }
                    }
                }
                f.i32(0);
            }
            f
        }
        Helper::CheckedPoll { poll, reinit } => checked_poll(poll, reinit, m),
        Helper::CollectPoll { poll } => {
            let mut f = m.func("rt:collect_poll", vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.helper((**poll).clone());
            if !m.acyclic {
                f.helper(Helper::Collect);
            }
            f
        }
        Helper::Teardown => teardown(m),
        Helper::Collect => collect(m),
        Helper::CycleTrace => {
            // The glue's releases go to the collector's `visit` while
            // `Tracing` is set (`release_ref`).
            let mut f = m.func("rt:cycle_trace", vec![Val::I32; 3], vec![]);
            let desc = f.local(Val::I32);
            f.get(1);
            f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
            glue_of(&mut f);
            f.set(desc);
            f.get(desc);
            f.op(Instruction::If(BlockType::Empty));
            f.i32(1);
            f.global_set(Global::Tracing);
            f.get(1);
            f.u32(HEADER);
            f.op(Instruction::I32Add);
            f.get(desc);
            f.i32(1);
            f.op(Instruction::I32Sub);
            f.call_indirect(vec![Val::I32], vec![]);
            f.i32(0);
            f.global_set(Global::Tracing);
            f.op(Instruction::End);
            f
        }
        Helper::CycleRelease => {
            let mut f = m.func("rt:cycle_release", vec![Val::I32; 3], vec![]);
            f.get(1);
            f.helper(Helper::ReleaseRef);
            f
        }
        Helper::Opaque => {
            let mut f = m.func("rt:opaque", vec![Val::I32], vec![]);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.cycle.opaque));
            }
            f
        }
        Helper::CycleFree => {
            let mut f = m.func("rt:cycle_free", vec![Val::I32; 3], vec![]);
            #[cfg(feature = "checked-report")]
            if let Some(c) = m.runtime.and_then(|r| r.checked) {
                f.get(1);
                f.call(Sym::Inst(c.collected));
            }
            free_object(&mut f, m, 1);
            f
        }
        Helper::Panic(msg) => {
            let mut f = m.func("rt:panic", vec![], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_lit(&mut f, m, &format!("{msg}\n"));
            f.op(Instruction::Unreachable);
            f
        }
        Helper::PanicStr => {
            let mut f = m.func("rt:panic_str", vec![Val::I32, Val::I32], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_lit(&mut f, m, "explicit-panic: ");
            write_str(&mut f, 0);
            write_lit(&mut f, m, "\n");
            f.op(Instruction::Unreachable);
            f
        }
        Helper::PanicWith => {
            let mut f = m.func("rt:panic_with", vec![Val::I32; 4], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_str(&mut f, 0);
            f.get(3);
            f.op(Instruction::If(BlockType::Empty));
            write_lit(&mut f, m, ": ");
            write_str(&mut f, 2);
            f.op(Instruction::End);
            write_lit(&mut f, m, "\n");
            f.op(Instruction::Unreachable);
            f
        }
        Helper::ToBuf => {
            let mut f = m.func("rt:to_buf", vec![Val::I32, Val::I32], vec![Val::I32]);
            // Grow the buffer by the pages the text needs beyond its size.
            f.get(1);
            f.op(Instruction::MemorySize(BUF));
            f.i32(16);
            f.op(Instruction::I32Shl);
            f.op(Instruction::I32GtU);
            f.op(Instruction::If(BlockType::Empty));
            f.get(1);
            f.op(Instruction::MemorySize(BUF));
            f.i32(16);
            f.op(Instruction::I32Shl);
            f.op(Instruction::I32Sub);
            f.i32(0xffff);
            f.op(Instruction::I32Add);
            f.i32(16);
            f.op(Instruction::I32ShrU);
            f.op(Instruction::MemoryGrow(BUF));
            f.i32(-1);
            f.op(Instruction::I32Eq);
            f.op(Instruction::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "heap-exhausted: the exchange buffer cannot grow".into(),
            ));
            f.op(Instruction::End);
            f.op(Instruction::End);
            f.i32(0);
            f.get(0);
            f.get(1);
            f.op(Instruction::MemoryCopy {
                src_mem: HEAP,
                dst_mem: BUF,
            });
            f.get(1);
            f
        }
        Helper::Alloc => alloc(m),
        Helper::StrEq => str_eq(m),
        Helper::New => new_object(m, None),
        Helper::NewFixed { size, cyclic } => new_object(m, Some((*size, *cyclic))),
        Helper::AllocClass { class, bytes } => alloc_class(m, *class, *bytes),
        Helper::FreeClass => free_class(m),
        Helper::Str => {
            let mut f = m.func("rt:str", vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.i32(0);
            f.helper(Helper::New);
            f
        }
        Helper::Retain { vals, walk } => {
            let mut f = m.func("rt:retain", vals.clone(), vec![]);
            let on = retain_call(m);
            walk_code(&mut f, walk, &|f, k| f.get(k), &on);
            f
        }
        Helper::Release { vals, walk } => {
            let mut f = m.func("rt:release", vals.clone(), vec![]);
            walk_code(&mut f, walk, &|f, k| f.get(k), &|f| {
                f.helper(Helper::ReleaseRef);
            });
            f
        }
        Helper::ReleaseRef => release_ref(m, false),
        Helper::ReleaseKnown => release_ref(m, true),
        Helper::RetainKnown => {
            let mut f = m.func("rt:retain_known", vec![Val::I32], vec![]);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain_known));
            }
            f
        }
        Helper::DropFields { leaves, walk } => fields_glue(m, "rt:drop", leaves, walk, &|f| {
            f.helper(Helper::ReleaseRef);
        }),
        Helper::RetainFields { leaves, walk } => {
            fields_glue(m, "rt:retain_fields", leaves, walk, &retain_call(m))
        }
        Helper::ListPinned => list_pinned(m),
        Helper::ListClone { elem } => list_clone(m, elem),
        Helper::ListBuf { elem } => list_buf(m, elem),
        Helper::DropElems { elem } => drop_elems(m, elem),
        Helper::CollCopy => coll_copy(m),
        Helper::CtxFind => ctx_find(m),
        Helper::CtxNew => ctx_new(m),
        Helper::CopyElems { elem } => copy_elems(elem, m),
        Helper::ListUnique { elem } => list_unique(elem, m),
        Helper::ListGrow { elem } => list_grow(elem, m),
        Helper::MapTable { entry } => map::table(m, entry),
        Helper::DropEntries { entry } => map::drop_entries(m, entry),
        Helper::MapHash { key, ops } => map::hash(m, key, ops),
        Helper::MapFind { entry, ops } => map::find(m, entry, ops),
        Helper::MapPut { entry, ops } => map::put(m, entry, ops),
        Helper::MapGrow { entry } => map::grow(entry, m),
        Helper::MapUnique { entry } => map::unique(entry, m, false),
        Helper::MapClone { entry } => map::unique(entry, m, true),
        Helper::MapSlotNext => map::slot_next(m),
        Helper::DynSlot {
            target,
            leaves,
            params,
            results,
            own,
            ctx,
        } => {
            let mut all = vec![Val::I32];
            all.extend(params.iter().copied());
            let mut f = m.func("rt:dyn_slot", all, results.clone());
            let recv = |f: &mut Func| match leaves {
                Some(ls) => {
                    for l in ls {
                        f.get(0);
                        f.op(l.load(0));
                    }
                }
                None => f.get(0),
            };
            let rvals = leaves
                .as_ref()
                .map_or(vec![Val::I32], |ls| ls.iter().map(|l| l.val).collect());
            own_lent(&mut f, &rvals, &recv, params, own, m);
            recv(&mut f);
            push_slot_params(
                &mut f,
                params.len() - usize::from(ctx.is_some()),
                ctx.as_ref(),
            );
            f.call(Sym::Inst(*target));
            f
        }
        Helper::DynRead {
            target,
            read,
            params,
            results,
            own,
            ctx,
        } => dyn_read(*target, read, params, results, own, ctx.as_ref(), m),
        Helper::ListTruncate { elem } => list_truncate(m, elem),
        Helper::TypeIdOf(parts) => type_id(parts, vec![], m),
        Helper::RuntimeType(parts) => type_id(parts, vec![Val::I32], m),
        Helper::MapStep {
            entry,
            opt,
            results,
        } => map::step(m, entry, opt, results),
        Helper::DynTrap { params, results } => {
            let mut f = m.func("rt:dyn_trap", params.clone(), results.clone());
            f.op(Instruction::Unreachable);
            f
        }
        Helper::StrFromBytes => str_from_bytes(m),
        Helper::ConcatAll => concat_all(m),
        Helper::StrCmp => str_cmp(m),
        Helper::BytesIndexJson => {
            let mut f = m.func("rt:bytes_index_json", vec![Val::I32; 3], vec![Val::I32]);
            f.raw = Some(crate::scan::json_index_code());
            f
        }
        Helper::HostPrim { name, args, result } => host_prim(m, name, args, *result),
        Helper::FmtInt { signed } => fmt_int(m, *signed),
        Helper::ForbidCheck => {
            let mut f = m.func("rt:forbid_check", vec![], vec![]);
            f.global_get(Global::Forbid);
            f.op(Instruction::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "suspension-forbidden-context: block_on in a forbidden context".into(),
            ));
            f.op(Instruction::End);
            f
        }
        Helper::RetainAny => {
            let mut f = m.func("rt:retain_any", vec![Val::I32], vec![]);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain));
            }
            f
        }
    };
    #[cfg(feature = "checked-report")]
    match h {
        Helper::ListBuf { .. } | Helper::MapTable { .. } => checked_kind(&mut f, m, 5),
        _ => {}
    }
    f
}

/// [`Helper::Alloc`]: a bump allocation from `HeapTop` up to `HeapEnd`,
/// the end of the pages it owns. One that does not fit starts on pages it
/// grows for itself, so it never reaches a page that `std.rt.memory.grow`
/// gave std (`module.rt.memory.grow.owner`); the rest of its old pages is
/// left unused.
fn alloc(m: &Module) -> Func {
    let mut f = m.func("rt:alloc", vec![Val::I32], vec![Val::I32]);
    let (at, n, pages) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let exhausted = |f: &mut Func| {
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Panic(
            "heap-exhausted: an allocation does not fit the heap".into(),
        ));
        f.op(Instruction::End);
    };
    // n = (size + 7) & !7, refusing a wrap past 4 GiB.
    f.get(0);
    f.i32(7);
    f.op(Instruction::I32Add);
    f.i32(-8);
    f.op(Instruction::I32And);
    f.set(n);
    f.get(n);
    f.get(0);
    f.op(Instruction::I32LtU);
    exhausted(&mut f);
    f.global_get(Global::HeapTop);
    f.set(at);
    // It fits when n <= HeapEnd - at.
    f.get(n);
    f.global_get(Global::HeapEnd);
    f.get(at);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32GtU);
    f.op(Instruction::If(BlockType::Empty));
    // pages = n / 64 KiB, rounded up.
    f.get(n);
    f.i32(16);
    f.op(Instruction::I32ShrU);
    f.get(n);
    f.i32(0xffff);
    f.op(Instruction::I32And);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32Add);
    f.set(pages);
    f.get(pages);
    f.op(Instruction::MemoryGrow(HEAP));
    f.set(at);
    f.get(at);
    f.i32(-1);
    f.op(Instruction::I32Eq);
    exhausted(&mut f);
    f.get(at);
    f.get(pages);
    f.op(Instruction::I32Add);
    f.i32(16);
    f.op(Instruction::I32Shl);
    f.global_set(Global::HeapEnd);
    f.get(at);
    f.i32(16);
    f.op(Instruction::I32Shl);
    f.set(at);
    f.op(Instruction::End);
    f.get(at);
    f.get(n);
    f.op(Instruction::I32Add);
    f.global_set(Global::HeapTop);
    f.get(at);
    f
}

/// [`Helper::FmtInt`]: the digits go backward into a 20-byte buffer,
/// the most a `u64` or a signed `i64` needs; the string is its tail.
fn fmt_int(m: &Module, signed: bool) -> Func {
    let mut f = m.func(
        if signed { "rt:fmt_i64" } else { "rt:fmt_u64" },
        vec![Val::I64],
        vec![Val::I32, Val::I32],
    );
    let (buf, at, neg, v) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I64),
    );
    f.i32(20);
    f.helper(Helper::Str);
    f.set(buf);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(0);
    f.set(v);
    if signed {
        // The magnitude of a negative value, read unsigned, is `0 - v`:
        // that holds for `i64::MIN` too.
        f.get(0);
        f.op(Instruction::I64Const(0));
        f.op(Instruction::I64LtS);
        f.set(neg);
        f.get(neg);
        f.op(Instruction::If(BlockType::Empty));
        f.op(Instruction::I64Const(0));
        f.get(0);
        f.op(Instruction::I64Sub);
        f.set(v);
        f.op(Instruction::End);
    }
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.set(at);
    f.get(at);
    f.get(v);
    f.op(Instruction::I64Const(10));
    f.op(Instruction::I64RemU);
    f.op(Instruction::I32WrapI64);
    f.i32(i32::from(b'0'));
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
    f.get(v);
    f.op(Instruction::I64Const(10));
    f.op(Instruction::I64DivU);
    f.set(v);
    f.get(v);
    f.op(Instruction::I64Const(0));
    f.op(Instruction::I64Ne);
    f.op(Instruction::BrIf(0));
    f.op(Instruction::End);
    if signed {
        f.get(neg);
        f.op(Instruction::If(BlockType::Empty));
        f.get(at);
        f.i32(1);
        f.op(Instruction::I32Sub);
        f.set(at);
        f.get(at);
        f.i32(i32::from(b'-'));
        f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
        f.op(Instruction::End);
    }
    f.get(at);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.get(at);
    f.op(Instruction::I32Sub);
    f
}

/// [`Helper::StrEq`]: equal lengths, then each byte.
fn str_eq(m: &Module) -> Func {
    let mut f = m.func("rt:str_eq", vec![Val::I32; 4], vec![Val::I32]);
    let k = f.local(Val::I32);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(k);
    f.get(1);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.get(2);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(k);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(k);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(1);
    f
}

/// [`Helper::StrCmp`]: the first differing byte decides; when the shorter
/// is a prefix of the longer, the lengths do.
fn str_cmp(m: &Module) -> Func {
    let mut f = m.func("rt:str_cmp", vec![Val::I32; 4], vec![Val::I32]);
    let (k, n, x, y) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    // The sign of `x - y` as -1, 0 or 1, for two values in locals.
    let sign = |f: &mut Func, x: u32, y: u32| {
        f.get(x);
        f.get(y);
        f.op(Instruction::I32GtU);
        f.get(x);
        f.get(y);
        f.op(Instruction::I32LtU);
        f.op(Instruction::I32Sub);
    };
    f.get(1);
    f.get(3);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32LtU);
    f.op(Instruction::Select);
    f.set(n);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(k);
    f.get(n);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.set(x);
    f.get(2);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.set(y);
    f.get(x);
    f.get(y);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    sign(&mut f, x, y);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(k);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(k);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    sign(&mut f, 1, 3);
    f
}

/// [`Helper::HostPrim`]. The host encodes a string result as its LEB128
/// length and then its bytes, and answers the encoding's length, so the
/// bytes are what follows the prefix: the bytes up to the first without
/// its high bit.
fn host_prim(m: &Module, name: &str, args: &[PrimArg], result: Option<Val>) -> Func {
    let mut params = Vec::new();
    let mut imports = Vec::new();
    for a in args {
        match a {
            PrimArg::Str => {
                params.extend([Val::I32, Val::I32]);
                imports.push(Val::I32);
            }
            PrimArg::Scalar(v) => {
                params.push(*v);
                imports.push(*v);
            }
        }
    }
    let results = match result {
        Some(v) => vec![v],
        None => vec![Val::I32, Val::I32],
    };
    let mut f = m.func(format!("rt:prim/{name}"), params, results);
    let mut p = 0;
    for a in args {
        f.get(p);
        p += 1;
        if *a == PrimArg::Str {
            f.get(p);
            p += 1;
            f.helper(Helper::ToBuf);
        }
    }
    let import = Import::new("hd:prim", name, &imports, &[result.unwrap_or(Val::I32)]);
    f.call(Sym::Import(import));
    if result.is_some() {
        return f;
    }
    let (n, at, len, out) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.set(n);
    // `at`: one past the length prefix.
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Load8U(mem(BUF, 0, 0)));
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(at);
    f.i32(0x80);
    f.op(Instruction::I32And);
    f.op(Instruction::BrIf(0));
    f.op(Instruction::End);
    f.get(n);
    f.get(at);
    f.op(Instruction::I32Sub);
    f.set(len);
    f.get(len);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.u32(EMPTY);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(len);
    f.helper(Helper::Str);
    f.set(out);
    f.get(out);
    f.get(at);
    f.get(len);
    f.op(Instruction::MemoryCopy {
        src_mem: BUF,
        dst_mem: HEAP,
    });
    f.get(out);
    f.get(len);
    f
}

/// A vtable slot's counts for its target (`crates/hd_wasm_linear/DESIGN.md`
/// §23.25): a dynamic call lends the receiver, pushed by `recv` with
/// layout `rvals`, and the arguments, in the slot's parameters from 1 on,
/// laid out as `params`. `own` has, for each of the target's parameters,
/// the receiver first, how many values it has and its walk when the target
/// owns it; each of those takes a count of its own here.
fn own_lent(
    f: &mut Func,
    rvals: &[Val],
    recv: &dyn Fn(&mut Func),
    params: &[Val],
    own: &[(u32, Option<Walk>)],
    m: &Module,
) {
    if m.runtime.is_none() {
        return;
    }
    let Some(((_, r), rest)) = own.split_first() else {
        return;
    };
    own_arg(f, rvals, r.as_ref(), recv);
    let mut at = 1u32;
    for (n, w) in rest {
        let first = at;
        let vals = &params[(first - 1) as usize..(first - 1 + n) as usize];
        own_arg(f, vals, w.as_ref(), &|f| {
            for k in first..first + n {
                f.get(k);
            }
        });
        at += n;
    }
}

/// One more count on a value that a helper lends to a plain function that
/// owns its parameter (`layout::Pass::Owned`, `crates/hd_wasm_linear/DESIGN.md`
/// §23.25): `get` pushes the value, of layout `vals`; `walk` is the
/// parameter's when the callee owns it.
pub(crate) fn own_arg(f: &mut Func, vals: &[Val], walk: Option<&Walk>, get: &dyn Fn(&mut Func)) {
    if let Some(w) = walk {
        get(f);
        f.helper(Helper::Retain {
            vals: vals.to_vec(),
            walk: w.clone(),
        });
    }
}

/// What retains the address on the stack: std's `retain`, or nothing when
/// the program counts nothing.
pub(crate) fn retain_call(m: &Module) -> impl Fn(&mut Func) {
    let retain = m.runtime.map(|r| r.retain);
    move |f: &mut Func| match retain {
        Some(k) => f.call(Sym::Inst(k)),
        None => f.op(Instruction::Drop),
    }
}

/// Emits `walk` over the values that `get(k)` pushes, running `on` with
/// each counted address on the stack: an enum's arms by its tag.
pub(crate) fn walk_code(
    f: &mut Func,
    walk: &Walk,
    get: &dyn Fn(&mut Func, u32),
    on: &dyn Fn(&mut Func),
) {
    match walk {
        Walk::Ref(k) => {
            get(f, *k);
            on(f);
        }
        Walk::Seq(ws) => {
            for w in ws {
                walk_code(f, w, get, on);
            }
        }
        Walk::Switch(tag, arms) => {
            for (v, arm) in arms.iter().enumerate() {
                let Some(arm) = arm else {
                    continue;
                };
                get(f, *tag);
                f.u32(u32::try_from(v).expect("variants"));
                f.op(Instruction::I32Eq);
                f.op(Instruction::If(BlockType::Empty));
                walk_code(f, arm, get, on);
                f.op(Instruction::End);
            }
        }
    }
}

/// A `(payload)` glue function: loads every leaf of the payload, then
/// walks them.
fn fields_glue(
    m: &Module,
    name: &str,
    leaves: &[Leaf],
    walk: &Walk,
    on: &dyn Fn(&mut Func),
) -> Func {
    let mut f = m.func(name, vec![Val::I32], vec![]);
    let ls: Vec<u32> = leaves.iter().map(|l| f.local(l.val)).collect();
    for (l, at) in leaves.iter().zip(&ls) {
        f.get(0);
        f.op(l.load(0));
        f.set(*at);
    }
    walk_code(&mut f, walk, &|f, k| f.get(ls[k as usize]), on);
    f
}

/// Pushes a new object's descriptor word: its release glue's table slot
/// plus one, or 0 for none; with [`CYCLIC`] when its type is cycle-capable
/// (`crate::cycle`), which only an object with glue can be.
pub(crate) fn push_desc(f: &mut Func, glue: Option<Sym>, cyclic: bool) {
    match glue {
        Some(g) => {
            f.table_slot(g);
            f.u32(if cyclic { CYCLIC + 1 } else { 1 });
            f.op(Instruction::I32Add);
        }
        None => f.i32(0),
    }
}

/// Pushes the release glue's table slot plus one from the descriptor word
/// on the stack, without the collector and allocation-class bits.
pub(crate) fn glue_of(f: &mut Func) {
    f.u32(GLUE);
    f.op(Instruction::I32And);
}

/// The allocator's twenty small classes, including the object's header and
/// its optional collector word. Zero/large objects keep the ordinary path.
fn small_class(size: u32) -> Option<(u32, u32)> {
    const BYTES: [u32; 20] = [
        16, 32, 48, 64, 80, 96, 112, 128, 160, 208, 256, 320, 400, 512, 640, 800, 1024, 1280, 1600,
        2048,
    ];
    BYTES.into_iter().enumerate().find_map(|(i, bytes)| {
        (size <= bytes).then(|| (u32::try_from(i + 1).expect("size class"), bytes))
    })
}

/// The allocation total is in the copied allocator metadata prefix.
fn note_allocation(f: &mut Func, m: &Module, table: u32) {
    let next = f.local(Val::I32);
    f.get(table);
    f.op(Instruction::I32Load(mem(HEAP, 244, 2)));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(next);
    if !m.release {
        f.get(next);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Panic("integer-overflow: integer overflow".into()));
        f.op(Instruction::End);
    }
    f.get(table);
    f.get(next);
    f.op(Instruction::I32Store(mem(HEAP, 244, 2)));
}

fn allocated_slot(f: &mut Func, page: u32, slot: u32) {
    f.get(page);
    f.get(page);
    f.op(Instruction::I32Load16U(mem(HEAP, 12, 1)));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store16(mem(HEAP, 12, 1)));
    f.get(slot);
    f.op(Instruction::Return);
}

/// Pop a free slot or bump within the head page when it remains non-full.
/// The last available slot, a missing page/table, and page-list maintenance
/// stay in std. There is no call between reading metadata and updating it.
fn alloc_class(m: &Module, class: u32, bytes: u32) -> Func {
    let mut f = m.func(
        format!("rt:alloc_class/{class}"),
        vec![Val::I32],
        vec![Val::I32],
    );
    let Some(r) = m.runtime else {
        f.get(0);
        f.helper(Helper::Alloc);
        return f;
    };
    if r.checked.is_some() {
        f.get(0);
        f.call(Sym::Inst(r.alloc));
        return f;
    }
    let (table, page, slot, next, bump) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.call(Sym::Inst(r.small.metadata));
    f.set(table);
    f.get(table);
    f.op(Instruction::If(BlockType::Empty));
    f.get(table);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(class * 4), 2)));
    f.set(page);
    f.get(page);
    f.op(Instruction::If(BlockType::Empty));
    f.get(page);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(slot);
    f.get(page);
    f.op(Instruction::I32Load16U(mem(HEAP, 14, 1)));
    f.i32(4);
    f.op(Instruction::I32Shl);
    f.set(bump);
    f.get(slot);
    f.op(Instruction::If(BlockType::Empty));
    f.get(slot);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(next);
    f.get(next);
    f.get(bump);
    f.u32(65536 - bytes);
    f.op(Instruction::I32LeU);
    f.op(Instruction::I32Or);
    f.op(Instruction::If(BlockType::Empty));
    note_allocation(&mut f, m, table);
    f.get(page);
    f.get(next);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    allocated_slot(&mut f, page, slot);
    f.op(Instruction::End);
    f.op(Instruction::Else);
    f.get(bump);
    f.u32(65536 - 2 * bytes);
    f.op(Instruction::I32LeU);
    f.op(Instruction::If(BlockType::Empty));
    note_allocation(&mut f, m, table);
    f.get(page);
    f.get(bump);
    f.u32(bytes);
    f.op(Instruction::I32Add);
    f.i32(4);
    f.op(Instruction::I32ShrU);
    f.op(Instruction::I32Store16(mem(HEAP, 14, 1)));
    f.get(page);
    f.get(bump);
    f.op(Instruction::I32Add);
    f.set(slot);
    allocated_slot(&mut f, page, slot);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(0);
    f.u32(class);
    f.call(Sym::Inst(r.small.alloc));
    f
}

/// A live block on its class's head page needs only a free-list push and
/// live decrement. Empty head pages remain cached. Other pages use std's
/// known-class slow path; buffered candidates keep the collector's free.
fn free_class(m: &Module) -> Func {
    let mut f = m.func("rt:free_class", vec![Val::I32], vec![]);
    let Some(r) = m.runtime else {
        return f;
    };
    if r.checked.is_some() || !m.fixed_allocations {
        f.get(0);
        f.call(Sym::Inst(r.free));
        return f;
    }
    let (desc, class, table, page) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(desc);
    f.get(desc);
    f.u32(crate::module::CLASS_MASK);
    f.op(Instruction::I32And);
    f.u32(crate::module::CLASS_SHIFT);
    f.op(Instruction::I32ShrU);
    f.set(class);
    f.get(class);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32Eqz);
    f.get(desc);
    f.u32(crate::module::BUFFERED);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.i32(-65536);
    f.op(Instruction::I32And);
    f.set(page);
    f.call(Sym::Inst(r.small.metadata));
    f.set(table);
    f.get(table);
    f.get(class);
    f.i32(2);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(page);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.get(page);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(page);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(page);
    f.get(page);
    f.op(Instruction::I32Load16U(mem(HEAP, 12, 1)));
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Store16(mem(HEAP, 12, 1)));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.get(class);
    f.call(Sym::Inst(r.small.free));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.call(Sym::Inst(r.free));
    f
}

fn free_object(f: &mut Func, m: &Module, block: u32) {
    let Some(r) = m.runtime else {
        return;
    };
    f.get(block);
    if m.fixed_allocations && r.checked.is_none() {
        f.helper(Helper::FreeClass);
    } else {
        f.call(Sym::Inst(r.free));
    }
}

/// [`Helper::New`]: a block of the header and the payload from std's
/// `alloc`, or from the bump pointer when the program counts nothing.
///
/// With the runtime, a collection runs first when the collector's buffer
/// is full, and an object whose descriptor has [`CYCLIC`] takes part in
/// collections: its block has room for the collector word at its end,
/// which std's `track` zeroes (§23.28).
fn new_object(m: &Module, fixed: Option<(u32, bool)>) -> Func {
    let small = fixed.and_then(|(size, cyclic)| {
        m.runtime.filter(|r| r.checked.is_none())?;
        let bytes = size
            .checked_add(HEADER)?
            .checked_add(if cyclic && !m.acyclic {
                crate::module::CYCLE_WORD
            } else {
                0
            })?;
        small_class(bytes)
    });
    let name = small.map_or_else(
        || "rt:new".into(),
        |(class, _)| format!("rt:new/class-{class}"),
    );
    let mut f = m.func(name, vec![Val::I32, Val::I32], vec![Val::I32]);
    let block = f.local(Val::I32);
    let cycles = m.runtime.filter(|_| !m.acyclic);
    if let Some(r) = cycles {
        f.call(Sym::Inst(r.cycle.due));
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Collect);
        f.op(Instruction::End);
    }
    f.get(0);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    match m.runtime {
        Some(r) => {
            if !m.acyclic {
                f.get(1);
                f.i32(31);
                f.op(Instruction::I32ShrU);
                f.u32(crate::module::CYCLE_WORD);
                f.op(Instruction::I32Mul);
                f.op(Instruction::I32Add);
            }
            if let Some((class, bytes)) = small {
                f.helper(Helper::AllocClass { class, bytes });
            } else {
                f.call(Sym::Inst(r.alloc));
            }
        }
        None => f.helper(Helper::Alloc),
    }
    f.set(block);
    f.get(block);
    f.i32(1);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(block);
    f.get(1);
    if let Some((class, _)) = small {
        f.u32(class << crate::module::CLASS_SHIFT);
        f.op(Instruction::I32Or);
    }
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    if let Some(r) = cycles {
        f.get(1);
        f.i32(0);
        f.op(Instruction::I32LtS);
        f.op(Instruction::If(BlockType::Empty));
        f.get(block);
        f.call(Sym::Inst(r.cycle.track));
        f.op(Instruction::End);
    }
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f
}

/// [`Helper::Collect`].
fn collect(m: &Module) -> Func {
    let mut f = m.func("rt:collect", vec![], vec![]);
    let Some(r) = m.runtime.filter(|_| !m.acyclic) else {
        return f;
    };
    for h in [Helper::CycleTrace, Helper::CycleRelease, Helper::CycleFree] {
        f.table_slot(Sym::Helper(h));
        f.i32(0);
    }
    f.call(Sym::Inst(r.cycle.collect));
    f
}

/// [`Helper::ReleaseRef`]: std identifies a block's final count before it
/// enters this allocation-free worklist. Its count word then links pending
/// blocks; its payload and descriptor stay intact until its turn. A nested
/// descriptor release enqueues and returns, so recursive types use a bounded
/// native stack. While the collector traces an object (`Tracing`), the
/// reference goes to its `visit` instead, checked here so that `visit` is
/// called only during a collection (§23.28).
fn release_ref(m: &Module, known: bool) -> Func {
    let mut f = m.func("rt:release_ref", vec![Val::I32], vec![]);
    let Some(r) = m.runtime else {
        return f;
    };
    let (block, desc) = (f.local(Val::I32), f.local(Val::I32));
    if !m.acyclic {
        f.global_get(Global::Tracing);
        f.op(Instruction::If(BlockType::Empty));
        f.get(0);
        f.call(Sym::Inst(r.cycle.visit));
        f.op(Instruction::If(BlockType::Empty));
        f.op(Instruction::Return);
        f.op(Instruction::End);
        f.op(Instruction::End);
    }
    f.get(0);
    f.call(Sym::Inst(if known { r.release_known } else { r.release }));
    f.set(block);
    f.get(block);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);

    // The final release already removed the block from cycle candidates.
    f.get(block);
    f.global_get(Global::ReleaseHead);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(block);
    f.global_set(Global::ReleaseHead);
    f.global_get(Global::Releasing);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.i32(1);
    f.global_set(Global::Releasing);

    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.global_get(Global::ReleaseHead);
    f.set(block);
    f.get(block);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.global_set(Global::ReleaseHead);
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    glue_of(&mut f);
    f.set(desc);
    f.get(desc);
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f.get(desc);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.call_indirect(vec![Val::I32], vec![]);
    f.op(Instruction::End);
    free_object(&mut f, m, block);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(0);
    f.global_set(Global::Releasing);
    f
}

/// [`Helper::CollCopy`]: one more count on the buffer of a list or a map,
/// which counts its holders. A pinned collection's buffer has one holder,
/// so its copy is a clone (`ref.elem.pin`); an unpinned one whose buffer
/// still has an owner word gives the word up, as the buffer is shared now.
/// Exposed ancestors stay unshared even without a direct pin: their
/// recursively copied elements may contain a still-pinned descendant.
fn coll_copy(m: &Module) -> Func {
    let mut f = m.func("rt:coll_copy", vec![Val::I32], vec![Val::I32]);
    let (b, own) = (0, f.local(Val::I32));
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.set(own);
    f.get(own);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(Instruction::I32GtU);
    f.get(own);
    f.u32(LIST_EXPOSED);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Or);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.get(own);
    f.u32(!LIST_EXPOSED);
    f.op(Instruction::I32And);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.call_indirect(vec![Val::I32], vec![Val::I32]);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_OWN, 2)));
    f.op(Instruction::End);
    f.get(b);
    f.helper(Helper::RetainKnown);
    f.get(b);
    f
}

/// [`Helper::ListPinned`].
fn list_pinned(m: &Module) -> Func {
    let mut f = m.func("rt:list_pinned", vec![Val::I32], vec![Val::I32]);
    let b = 0;
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(Instruction::I32GtU);
    f.op(Instruction::I32And);
    f
}

/// [`Helper::ListClone`].
fn list_clone(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:list_clone", vec![Val::I32], vec![Val::I32]);
    let (b, n, nb) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.set(nb);
    f.get(nb);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(nb);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    if elem.counts() {
        f.get(nb);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(n);
        f.helper(Helper::CopyElems { elem: elem.clone() });
    }
    f.get(nb);
    f
}

/// [`Helper::ListBuf`]: a capacity whose bytes pass 4 GiB is
/// `heap-exhausted`.
fn list_buf(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:list_buf", vec![Val::I32], vec![Val::I32]);
    let b = f.local(Val::I32);
    if let Some(most) =
        (u32::MAX - LIST_ELEMS - HEADER - crate::module::CYCLE_WORD).checked_div(elem.size)
    {
        f.get(0);
        f.u32(most);
        f.op(Instruction::I32GtU);
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Panic(
            "heap-exhausted: a list does not fit the heap".into(),
        ));
        f.op(Instruction::End);
    }
    f.get(0);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    let glue = elem
        .walk
        .as_ref()
        .map(|_| Sym::Helper(Helper::DropElems { elem: elem.clone() }));
    push_desc(&mut f, glue, elem.cyclic);
    f.helper(Helper::New);
    f.set(b);
    f.get(b);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_OWN, 2)));
    f.get(b);
    f
}

/// [`Helper::CtxFind`].
fn ctx_find(m: &Module) -> Func {
    let mut f = m.func("rt:ctx_find", vec![Val::I32, Val::I64], vec![Val::I32]);
    let end = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Unreachable);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.u32(CTX_ENTRY);
    f.op(Instruction::I32Mul);
    f.get(0);
    f.op(Instruction::I32Add);
    f.set(end);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(0);
    f.get(end);
    f.op(Instruction::I32GeU);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Unreachable);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I64Load(mem(HEAP, u64::from(LIST_ELEMS), 3)));
    f.get(1);
    f.op(Instruction::I64Eq);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.u32(CTX_ENTRY);
    f.op(Instruction::I32Add);
    f.set(0);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(0);
    f
}

/// [`Helper::CtxNew`].
fn ctx_new(m: &Module) -> Func {
    let mut f = m.func("rt:ctx_new", vec![Val::I32, Val::I32], vec![Val::I32]);
    let (len, c, at) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(1);
    f.op(Instruction::If(BlockType::Empty));
    f.get(1);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(len);
    f.op(Instruction::End);
    f.get(0);
    f.get(len);
    f.op(Instruction::I32Add);
    f.helper(Helper::ListBuf { elem: ctx_elem() });
    f.set(c);
    f.get(c);
    f.get(0);
    f.get(len);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(len);
    f.op(Instruction::If(BlockType::Empty));
    f.get(c);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(0);
    f.u32(CTX_ENTRY);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(at);
    f.get(1);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(len);
    f.u32(CTX_ENTRY);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    f.get(at);
    f.get(len);
    f.helper(Helper::CopyElems { elem: ctx_elem() });
    f.op(Instruction::End);
    f.get(c);
    f
}

/// Emits a loop over the `n` elements of `size` bytes from the address in
/// local `at`, which it advances: `body` runs with `at` at each element.
fn each_elem(f: &mut Func, at: u32, n: u32, size: u32, body: &dyn Fn(&mut Func)) {
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(n);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    body(f);
    f.get(at);
    f.u32(size);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(n);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.set(n);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// A function of `params`, which it ignores, that returns a `TypeId`: its
/// key and its name, two views of static data.
fn type_id(parts: &TypeIdParts, params: Vec<Val>, m: &mut Module) -> Func {
    let mut f = m.func("rt:type_id", params, vec![Val::I32; 4]);
    for text in [&parts.name, &parts.shown] {
        f.u32(m.static_bytes(text.as_bytes(), 1));
        f.u32(u32::try_from(text.len()).expect("type name"));
    }
    f
}

/// [`Helper::ListTruncate`].
fn list_truncate(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:list_truncate", vec![Val::I32, Val::I32], vec![Val::I32]);
    let (b, at, n) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.helper(Helper::ListPinned);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.get(1);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "collection-pinned: list length changed while pinned".into(),
    ));
    f.op(Instruction::End);
    f.get(b);
    f.helper(Helper::ListUnique { elem: elem.clone() });
    f.set(b);
    if let Some(walk) = &elem.walk {
        f.get(b);
        f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
        f.get(1);
        f.op(Instruction::I32Sub);
        f.set(n);
        f.get(b);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(1);
        f.u32(elem.size);
        f.op(Instruction::I32Mul);
        f.op(Instruction::I32Add);
        f.set(at);
        let ls = load_elem_locals(&mut f, &elem.leaves);
        each_elem(&mut f, at, n, elem.size, &|f| {
            fill_elem(f, at, &elem.leaves, &ls);
            walk_code(f, walk, &|f, k| f.get(ls[k as usize]), &|f| {
                f.helper(Helper::ReleaseRef);
            });
        });
    }
    f.get(b);
    f.get(1);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(b);
    f
}

/// [`Helper::DropElems`].
fn drop_elems(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:drop_elems", vec![Val::I32], vec![]);
    let (at, n) = (f.local(Val::I32), f.local(Val::I32));
    let Some(walk) = &elem.walk else {
        return f;
    };
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(0);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.set(at);
    let ls = load_elem_locals(&mut f, &elem.leaves);
    each_elem(&mut f, at, n, elem.size, &|f| {
        fill_elem(f, at, &elem.leaves, &ls);
        walk_code(f, walk, &|f, k| f.get(ls[k as usize]), &|f| {
            f.helper(Helper::ReleaseRef);
        });
    });
    f
}

/// Fresh locals for an element's leaves.
fn load_elem_locals(f: &mut Func, leaves: &[Leaf]) -> Vec<u32> {
    leaves.iter().map(|l| f.local(l.val)).collect()
}

/// Loads each leaf of the element at local `at` into its local of `ls`.
fn fill_elem(f: &mut Func, at: u32, leaves: &[Leaf], ls: &[u32]) {
    for (l, x) in leaves.iter().zip(ls) {
        f.get(at);
        f.op(l.load(0));
        f.set(*x);
    }
}

/// [`Helper::CopyElems`].
fn copy_elems(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:copy_elems", vec![Val::I32, Val::I32], vec![]);
    let ls = load_elem_locals(&mut f, &elem.leaves);
    let walk = elem.walk.as_ref().and_then(|w| w.without(&elem.colls));
    let retain = retain_call(m);
    each_elem(&mut f, 0, 1, elem.size, &|f| {
        fill_elem(f, 0, &elem.leaves, &ls);
        if let Some(w) = &walk {
            walk_code(f, w, &|f, k| f.get(ls[k as usize]), &retain);
        }
        for c in &elem.colls {
            let l = elem.leaves[*c as usize];
            f.get(0);
            f.get(ls[*c as usize]);
            f.helper(Helper::CollCopy);
            f.op(Instruction::I32Store(mem(HEAP, u64::from(l.offset), 2)));
        }
    });
    f
}

/// [`Helper::ListUnique`]: a shared buffer is copied at its length; its
/// elements take their own counts and the old buffer one holder less.
fn list_unique(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_unique", vec![Val::I32], vec![Val::I32]);
    let (b, n, nb) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(Instruction::I32Ne);
    // A buffer with an owner word has one holder; the other counts are
    // element references, which see the change in place.
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.set(nb);
    f.get(nb);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(nb);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    if elem.counts() {
        f.get(nb);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(n);
        f.helper(Helper::CopyElems { elem: elem.clone() });
    }
    f.get(b);
    f.helper(Helper::ReleaseKnown);
    f.get(nb);
    f.set(b);
    f.op(Instruction::End);
    f.get(b);
    f
}

/// [`Helper::ListGrow`]: a pinned list panics; a full buffer moves to
/// one of twice its length plus 4, and the old one, its elements moved and its length 0, loses
/// the count the list held: an outlined change's thunk holds one more
/// until it returns (`emit/erased.rs`).
fn list_grow(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_grow", vec![Val::I32], vec![Val::I32]);
    let (b, n, nb) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.helper(Helper::ListPinned);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "collection-pinned: list length changed while pinned".into(),
    ));
    f.op(Instruction::End);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(n);
    f.i32(2);
    f.op(Instruction::I32Mul);
    f.i32(4);
    f.op(Instruction::I32Add);
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.set(nb);
    f.get(nb);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(nb);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    // An exposed list stays unshared after its pin ends, so the word moves.
    f.get(nb);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.op(Instruction::I32Store(mem(HEAP, LIST_OWN, 2)));
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(b);
    f.helper(Helper::ReleaseKnown);
    f.get(nb);
    f.set(b);
    f.op(Instruction::End);
    f.get(b);
    f
}

/// [`Helper::StrFromBytes`]: no bytes is the static empty string.
fn str_from_bytes(m: &Module) -> Func {
    let mut f = m.func("rt:str_from_bytes", vec![Val::I32], vec![Val::I32; 2]);
    let (b, n, s) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.u32(EMPTY);
    f.set(s);
    f.get(n);
    f.op(Instruction::If(BlockType::Empty));
    f.get(n);
    f.helper(Helper::Str);
    f.set(s);
    f.get(s);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    f.op(Instruction::End);
    f.get(s);
    f.get(n);
    f
}

/// [`Helper::ConcatAll`]: one pass sums the lengths, a second copies;
/// no bytes is the static empty string.
fn concat_all(m: &Module) -> Func {
    let mut f = m.func("rt:concat_all", vec![Val::I32], vec![Val::I32; 2]);
    let (at, n, total, s, to) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    let start = |f: &mut Func| {
        f.get(0);
        f.set(at);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
        f.set(n);
        f.get(at);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.set(at);
    };
    start(&mut f);
    each_elem(&mut f, at, n, 8, &|f| {
        f.get(total);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::I32Add);
        f.set(total);
    });
    f.u32(EMPTY);
    f.set(s);
    f.get(total);
    f.op(Instruction::If(BlockType::Empty));
    f.get(total);
    f.helper(Helper::Str);
    f.set(s);
    f.get(s);
    f.set(to);
    start(&mut f);
    each_elem(&mut f, at, n, 8, &|f| {
        f.get(to);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::MemoryCopy {
            src_mem: HEAP,
            dst_mem: HEAP,
        });
        f.get(to);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::I32Add);
        f.set(to);
    });
    f.op(Instruction::End);
    f.get(s);
    f.get(total);
    f
}

// Representation views retain a source reference, not a snapshot. Their
// getter and setter exchange one owned erased value at a fixed ABI.
fn ref_vals(value: &RefLayout) -> Vec<Val> {
    value.leaves.iter().map(|l| l.val).collect()
}

fn ref_count(f: &mut Func, ls: &[u32], value: &RefLayout, retain: bool) {
    if value.known
        && let Some(Walk::Ref(k)) = &value.walk
    {
        f.get(ls[*k as usize]);
        f.helper(if retain {
            Helper::RetainKnown
        } else {
            Helper::ReleaseKnown
        });
    } else if let Some(walk) = &value.walk {
        for l in ls {
            f.get(*l);
        }
        let vals = ref_vals(value);
        f.helper(if retain {
            Helper::Retain {
                vals,
                walk: walk.clone(),
            }
        } else {
            Helper::Release {
                vals,
                walk: walk.clone(),
            }
        });
    }
}

fn ref_box(f: &mut Func, ls: &[u32], value: &RefLayout) {
    if value.leaves.is_empty() {
        f.i32(0);
        return;
    }
    if !value.boxed {
        f.get(ls[0]);
        return;
    }
    let at = f.local(Val::I32);
    f.u32(value.size);
    // The value's type is not known here, so a box that holds a counted
    // reference takes part in cycle collection.
    let glue = value.walk.as_ref().map(|walk| {
        Sym::Helper(Helper::DropFields {
            leaves: value.leaves.clone(),
            walk: walk.clone(),
        })
    });
    push_desc(f, glue, true);
    f.helper(Helper::New);
    f.set(at);
    for (leaf, l) in value.leaves.iter().zip(ls) {
        f.get(at);
        f.get(*l);
        f.op(leaf.store(0));
    }
    f.get(at);
}

fn ref_unbox(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    if !value.boxed {
        return vec![at];
    }
    let ls: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
    ref_unbox_into(f, at, value, &ls);
    ls
}

/// The value of the erased box in local `at`, whose count this takes, in
/// locals `ls` with counts of their own.
fn ref_unbox_into(f: &mut Func, at: u32, value: &RefLayout, ls: &[u32]) {
    if !value.boxed {
        f.get(at);
        f.set(ls[0]);
        return;
    }
    for (leaf, l) in value.leaves.iter().zip(ls) {
        f.get(at);
        f.op(leaf.load(0));
        f.set(*l);
    }
    ref_copy(f, ls, value);
    f.get(at);
    f.helper(Helper::ReleaseRef);
}

/// One more count on each counted reference of the value in locals `ls`,
/// loads no count keeps: a collection's is the copy glue's result, which
/// replaces it, a clone when the collection is pinned (`ref.elem.copy`).
fn ref_copy(f: &mut Func, ls: &[u32], value: &RefLayout) {
    for &c in &value.colls {
        f.get(ls[c as usize]);
        f.helper(Helper::CollCopy);
        f.set(ls[c as usize]);
    }
    let rest = RefLayout {
        walk: value.walk.as_ref().and_then(|w| w.without(&value.colls)),
        known: value.known && value.colls.is_empty(),
        ..value.clone()
    };
    ref_count(f, ls, &rest, true);
}

/// The value that the reference in local `at` names, in new locals with
/// counts of their own: a view's getter's box unboxed, or the leaves at
/// the address. Emitted inline, so a value of any width stays in locals
/// and never crosses a function's parameters or results.
pub(crate) fn ref_load(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    ref_load_inner(f, at, value, None)
}

pub(crate) fn ref_load_borrowed(f: &mut Func, at: u32, value: &RefLayout, owned: u32) -> Vec<u32> {
    ref_load_inner(f, at, value, Some(owned))
}

fn ref_load_inner(f: &mut Func, at: u32, value: &RefLayout, owned: Option<u32>) -> Vec<u32> {
    f.stack_values |= value.leaves.len() > crate::wide::WIDTH;
    if let Some(flag) = owned {
        f.zero(flag);
    }
    let ls: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
    if ls.is_empty() {
        return ls;
    }
    f.get(at);
    f.helper(Helper::RefIsView);
    f.op(Instruction::If(BlockType::Empty));
    if let Some(flag) = owned {
        f.i32(1);
        f.set(flag);
    }
    let boxed = f.local(Val::I32);
    f.get(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.call_indirect(vec![Val::I32], vec![Val::I32]);
    f.set(boxed);
    ref_unbox_into(f, boxed, value, &ls);
    f.op(Instruction::Else);
    for (leaf, l) in value.leaves.iter().zip(&ls) {
        f.get(at);
        f.op(leaf.load(0));
        f.set(*l);
    }
    if owned.is_none() {
        ref_copy(f, &ls, value);
    }
    f.op(Instruction::End);
    ls
}

/// Stores the value in locals `ls`, which keep their own counts, through
/// the reference in local `at`: a view's setter takes a new box of it, an
/// address its leaves, releasing the value it held. Emitted inline, as
/// [`ref_load`] is.
pub(crate) fn ref_store(f: &mut Func, at: u32, ls: &[u32], value: &RefLayout) {
    if ls.is_empty() {
        return;
    }
    ref_count(f, ls, value, true);
    f.get(at);
    f.helper(Helper::RefIsView);
    f.op(Instruction::If(BlockType::Empty));
    let boxed = f.local(Val::I32);
    ref_box(f, ls, value);
    f.set(boxed);
    f.get(at);
    f.get(boxed);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.call_indirect(vec![Val::I32; 2], vec![]);
    f.op(Instruction::Else);
    let old: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
    for (leaf, o) in value.leaves.iter().zip(&old) {
        f.get(at);
        f.op(leaf.load(0));
        f.set(*o);
    }
    for (leaf, l) in value.leaves.iter().zip(ls) {
        f.get(at);
        f.get(*l);
        f.op(leaf.store(0));
    }
    ref_count(f, &old, value, false);
    f.op(Instruction::End);
}

/// The source reference that the view in parameter 0 retains, in a new
/// local.
fn view_source(f: &mut Func) -> u32 {
    let src = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(src);
    src
}

fn ref_helper(h: &Helper, m: &Module) -> Func {
    match h {
        Helper::RefIsView => {
            let mut f = m.func("rt:ref_is_view", vec![Val::I32], vec![Val::I32]);
            let Some(r) = m.runtime else {
                f.i32(0);
                return f;
            };
            let block = f.local(Val::I32);
            f.get(0);
            f.call(Sym::Inst(r.block_of));
            f.set(block);
            f.get(block);
            f.op(Instruction::I32Eqz);
            f.op(Instruction::If(BlockType::Empty));
            f.i32(0);
            f.op(Instruction::Return);
            f.op(Instruction::End);
            f.get(block);
            f.u32(HEADER);
            f.op(Instruction::I32Add);
            f.get(0);
            f.op(Instruction::I32Ne);
            f.op(Instruction::If(BlockType::Empty));
            f.i32(0);
            f.op(Instruction::Return);
            f.op(Instruction::End);
            f.get(block);
            f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
            glue_of(&mut f);
            f.table_slot(Sym::Helper(Helper::RefViewDrop));
            f.i32(1);
            f.op(Instruction::I32Add);
            f.op(Instruction::I32Eq);
            f
        }
        Helper::RefViewDrop => {
            let mut f = m.func("rt:ref_view_drop", vec![Val::I32], vec![]);
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
            f.helper(Helper::ReleaseRef);
            f
        }
        Helper::RefAddress => {
            // Physical places use their address. Fields of an erased
            // word use (word, field offset), since adding the concrete
            // offset to that one word could alias an unrelated object.
            let mut f = m.func("rt:ref_serial", vec![Val::I32], vec![Val::I64]);
            let offset = f.local(Val::I32);
            f.op(Instruction::Block(BlockType::Empty));
            f.op(Instruction::Loop(BlockType::Empty));
            f.get(0);
            f.helper(Helper::RefIsView);
            f.op(Instruction::I32Eqz);
            f.op(Instruction::BrIf(1));
            f.get(offset);
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
            f.op(Instruction::I32Add);
            f.set(offset);
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, 16, 2)));
            f.op(Instruction::If(BlockType::Empty));
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
            f.op(Instruction::I64ExtendI32U);
            f.op(Instruction::I64Const(32));
            f.op(Instruction::I64Shl);
            f.get(offset);
            f.op(Instruction::I64ExtendI32U);
            f.op(Instruction::I64Or);
            f.op(Instruction::Return);
            f.op(Instruction::End);
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
            f.set(0);
            f.op(Instruction::Br(0));
            f.op(Instruction::End);
            f.op(Instruction::End);
            f.get(0);
            f.get(offset);
            f.op(Instruction::I32Add);
            f.op(Instruction::I64ExtendI32U);
            f
        }
        Helper::RefView { source } => {
            let mut f = m.func("rt:ref_view", vec![Val::I32], vec![Val::I32]);
            // A view already carries both representations and can cross back.
            f.get(0);
            f.helper(Helper::RefIsView);
            f.op(Instruction::If(BlockType::Empty));
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain));
            }
            f.get(0);
            f.op(Instruction::Return);
            f.op(Instruction::End);
            let at = f.local(Val::I32);
            f.i32(20);
            // A view holds its source, whose type is not known here, so
            // it takes part in cycle collection.
            push_desc(&mut f, Some(Sym::Helper(Helper::RefViewDrop)), true);
            f.helper(Helper::New);
            f.set(at);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain));
            }
            f.get(at);
            f.get(0);
            f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
            f.get(at);
            f.table_slot(Sym::Helper(Helper::RefGet {
                source: source.clone(),
            }));
            f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
            f.get(at);
            f.table_slot(Sym::Helper(Helper::RefSet {
                source: source.clone(),
            }));
            f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
            f.get(at);
            f.i32(0);
            f.op(Instruction::I32Store(mem(HEAP, 12, 2)));
            f.get(at);
            f.i32(i32::from(!source.boxed));
            f.op(Instruction::I32Store(mem(HEAP, 16, 2)));
            f.get(at);
            f
        }
        Helper::RefGet { source } => {
            let mut f = m.func("rt:ref_get", vec![Val::I32], vec![Val::I32]);
            let src = view_source(&mut f);
            let ls = ref_load(&mut f, src, source);
            ref_box(&mut f, &ls, source);
            f
        }
        Helper::RefSet { source } => {
            let mut f = m.func("rt:ref_set", vec![Val::I32; 2], vec![]);
            let ls = ref_unbox(&mut f, 1, source);
            let src = view_source(&mut f);
            ref_store(&mut f, src, &ls, source);
            ref_count(&mut f, &ls, source, false);
            f
        }
        _ => unreachable!("a reference helper"),
    }
}

fn ref_field_helper(h: &Helper, m: &Module) -> Func {
    let (parent, field, first, offset) = match h {
        Helper::RefProject {
            parent,
            field,
            first,
            offset,
            ..
        }
        | Helper::RefFieldGet {
            parent,
            field,
            first,
            offset,
        }
        | Helper::RefFieldSet {
            parent,
            field,
            first,
            offset,
        } => (parent, field, *first, *offset),
        _ => unreachable!("a field reference helper"),
    };
    match h {
        Helper::RefProject { .. } => {
            let mut f = m.func("rt:ref_project", vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.helper(Helper::RefIsView);
            f.op(Instruction::I32Eqz);
            f.op(Instruction::If(BlockType::Empty));
            f.get(0);
            f.u32(offset);
            f.op(Instruction::I32Add);
            f.set(0);
            if !matches!(h, Helper::RefProject { borrowed: true, .. })
                && let Some(r) = m.runtime
            {
                f.get(0);
                f.call(Sym::Inst(r.retain));
            }
            f.get(0);
            f.op(Instruction::Return);
            f.op(Instruction::End);
            let at = f.local(Val::I32);
            f.i32(20);
            // A view holds its source, whose type is not known here, so
            // it takes part in cycle collection.
            push_desc(&mut f, Some(Sym::Helper(Helper::RefViewDrop)), true);
            f.helper(Helper::New);
            f.set(at);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain));
            }
            f.get(at);
            f.get(0);
            f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
            for (slot, helper) in [
                (
                    4,
                    Helper::RefFieldGet {
                        parent: parent.clone(),
                        field: field.clone(),
                        first,
                        offset,
                    },
                ),
                (
                    8,
                    Helper::RefFieldSet {
                        parent: parent.clone(),
                        field: field.clone(),
                        first,
                        offset,
                    },
                ),
            ] {
                f.get(at);
                f.table_slot(Sym::Helper(helper));
                f.op(Instruction::I32Store(mem(HEAP, slot, 2)));
            }
            f.get(at);
            f.u32(offset);
            f.op(Instruction::I32Store(mem(HEAP, 12, 2)));
            f.get(at);
            f.i32(0);
            f.op(Instruction::I32Store(mem(HEAP, 16, 2)));
            f.get(at);
            f
        }
        Helper::RefFieldGet { .. } => {
            let mut f = m.func("rt:ref_field_get", vec![Val::I32], vec![Val::I32]);
            let src = view_source(&mut f);
            let ls = ref_load(&mut f, src, parent);
            let child = &ls[first as usize..first as usize + field.leaves.len()];
            ref_count(&mut f, child, field, true);
            ref_count(&mut f, &ls, parent, false);
            ref_box(&mut f, child, field);
            f
        }
        Helper::RefFieldSet { .. } => {
            let mut f = m.func("rt:ref_field_set", vec![Val::I32; 2], vec![]);
            let child = ref_unbox(&mut f, 1, field);
            let src = view_source(&mut f);
            let ls = ref_load(&mut f, src, parent);
            let into = &ls[first as usize..first as usize + field.leaves.len()];
            ref_count(&mut f, into, field, false);
            for (target, source) in into.iter().zip(child) {
                f.get(source);
                f.set(*target);
            }
            ref_store(&mut f, src, &ls, parent);
            ref_count(&mut f, &ls, parent, false);
            f
        }
        _ => unreachable!("a field reference helper"),
    }
}

/// Gives this allocation its checked descriptor kind. No instruction is
/// added in unchecked builds; the linker aliases the same release glue.
#[cfg(feature = "checked-report")]
pub(crate) fn checked_kind(f: &mut Func, m: &Module, kind: u8) {
    if m.runtime.is_none_or(|r| r.checked.is_none()) {
        return;
    }
    if let Some(i) = f.body.windows(3).rposition(|w| matches!(w, [Ins::TableSlot(_), Ins::Op(Instruction::I32Const(c)), Ins::Op(Instruction::I32Add)] if c.cast_unsigned() == 1 || c.cast_unsigned() == CYCLIC + 1)) && let Ins::TableSlot(s) = &f.body[i] {
        f.body[i] = Ins::Descriptor(*s, kind);
    }
}
