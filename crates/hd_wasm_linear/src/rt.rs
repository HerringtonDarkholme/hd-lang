//! Runtime pieces the compiler generates (`crates/hd_wasm_linear/DESIGN.md`
//! §23.9): the entry exports, panics, the exchange-buffer copy, the bump
//! allocation that serves until the std allocator (job 5), integer text
//! and the host provider stubs. Each is a [`Helper`], built once on its
//! first request.

use crate::op::Op as Instruction;
use hd_base::{DefId, Hash128, StageResult};
use hd_types::{Ty, TyData, TyList};
use wasm_encoder::BlockType;

use crate::layout::{Addr, Leaf, Walk};
use crate::map::{self, KeyOps};
use crate::module::{
    BUF, CYCLIC, DescriptorKey, EMPTY, Func, GLUE, Global, HEADER, HEAP, Import, Ins, LOAN_EXCL,
    LOAN_SHARED, LOAN_STATE, Module, Sym, Val, WEAK, WEAK_GLUE, mem,
};
use crate::{Entry, Input, unsupported};

/// The canonical descriptor behind a `TypeId` (`emit/dyn_value.rs`):
/// its identity key and printable name, both in static storage.
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
    /// The `std.rt.Drop` implementations a release of an inline value
    /// runs before its fields (`module.rt.drop.glue`).
    pub drops: Box<[DropCall]>,
    /// The leaves a copy takes through the copy glue (`rt:coll_copy`).
    pub colls: Box<[u32]>,
    pub boxed: bool,
    pub known: bool,
    /// Exact referent type alias when a weak constructor can observe this box.
    pub weak: Option<Hash128>,
}

/// A generated function, by what it does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    MemoryCopy {
        source: u32,
        dest: u32,
    },
    MemoryFill(u32),
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
    /// A reference to an enum's payload field whose leaves lie at no one
    /// offset of the enum's shared slots (`picks`, by leaf index): a view
    /// over the reference to the enum, always.
    RefPayload {
        parent: RefLayout,
        field: RefLayout,
        picks: Vec<u32>,
        offset: u32,
    },
    RefPayloadGet {
        parent: RefLayout,
        field: RefLayout,
        picks: Vec<u32>,
    },
    RefPayloadSet {
        parent: RefLayout,
        field: RefLayout,
        picks: Vec<u32>,
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
    /// `hd.init`: the inits in order; a flagged one belongs to a module
    /// other than the entry's and runs as a forbidden context
    /// (`req.drive.block-on.forbidden-contexts.v2`).
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
    /// `(header)`: a shared loan's begin on the object whose descriptor
    /// word is at `header`, spilling past 31 ([`loan_share`]).
    LoanShare,
    /// `(header)`: a shared loan's end, from the spill table first.
    LoanUnshare,
    /// `(category, message)`: std's `panic_with`, whose report shows the
    /// message after the category when it is not empty
    /// (`flow.panic.report`).
    PanicWith,
    /// `(text) -> len`: copies a string into the exchange buffer at 0,
    /// growing the buffer to fit, and returns its length.
    ToBuf,
    /// `(size) -> address`: `size` bytes of the heap, 8-aligned, from the
    /// bump pointer; the heap grows to fit, and a refused growth is
    /// `heap-exhausted`.
    Alloc,
    /// `(size, descriptor) -> address`: a new counted object (§23.4) of
    /// `size` payload bytes, from std's allocator: its count 1 and its
    /// descriptor word, the table slot of its [`Helper::DropFields`] plus
    /// one, or 0 when its payload holds no counted reference and has no
    /// `std.rt.Drop` hook. The address is its payload's.
    New,
    /// The same ABI as New, with a statically known payload size.
    NewFixed {
        size: u32,
    },
    /// `() -> address`: a block of the known small class `class`, of
    /// `bytes` bytes, with page-list maintenance on the std slow path.
    AllocClass {
        class: u32,
        bytes: u32,
    },
    /// Free an object using its descriptor class when available.
    FreeClass,
    /// Free an object using its already loaded descriptor and decoded class.
    FreeClassLoaded,
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
    /// `(block)`: a block whose last count [`Helper::ReleaseRef`] took
    /// while no release runs: releases its payload's references through its
    /// descriptor, frees it, then does the same for each block the releases
    /// queued.
    ReleaseDrain,
    /// Count a statically known payload start without a block lookup.
    RetainKnown,
    ReleaseKnown,
    /// `(box) -> moved`: takes the value out of the counted box at
    /// payload start `box`, whose fields the caller has loaded. A box
    /// with one holder is freed without its glue, so its fields move out
    /// (1); a shared one loses a count, and the caller counts its copy
    /// (0).
    TakeBox,
    /// `(payload)`: the release glue of an object whose payload is laid
    /// out as `leaves`: its `std.rt.Drop` hooks, then one count less on
    /// each counted reference in it.
    DropFields {
        leaves: Vec<Leaf>,
        walk: Walk,
        /// The `std.rt.Drop.drop` instances to call before the fields.
        drops: Vec<DropCall>,
    },
    /// `(values)`: moves an inline value into a local-stack frame, runs
    /// its Drop hooks, then releases its fields. No counted box is made.
    DropValue {
        size: u32,
        leaves: Vec<Leaf>,
        walk: Walk,
        drops: Vec<DropCall>,
    },
    /// `(payload)`: a type-keyed descriptor alias for a weak-target type
    /// whose payload has no counted fields.
    WeakNoopDrop,
    /// `(serial: i64, expected_descriptor: i32) -> entry`: creates or reuses
    /// a side entry when the reference names an exact whole allocation.
    WeakNew,
    /// `(entry)`: removes the raw side-table row and clears the target's
    /// descriptor bit when the last weak value releases its entry.
    WeakEntryDrop,
    /// `()`: registers this module's descriptor aliases with the shared weak
    /// runtime before any user initializer runs.
    WeakTypeInit,
    /// `(payload)`: one more count on each counted reference in a new
    /// object's payload, which its construction copied in.
    RetainFields {
        leaves: Vec<Leaf>,
        walk: Walk,
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
    /// a buffer it shares is copied ([`Helper::ListCopy`]), and the
    /// place's count goes to the copy.
    ListUnique {
        elem: Elem,
    },
    /// `(list) -> list`: [`Helper::ListUnique`]'s slow path, out of line:
    /// a copy of the shared buffer at its length, whose elements take
    /// their own counts; the old buffer loses one holder.
    ListCopy {
        elem: Elem,
    },
    /// `(list) -> list`: a `push`'s slow path: the list's buffer, its own already
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
    /// `(map, pos, end) -> slot`: the first live entry in `pos..end`, or
    /// `end`.
    MapSlotNext,
    /// `(data, params..) -> results`: a vtable slot (`emit/dyn_value.rs`):
    /// calls `target` with the payload, loaded from the box at `data` as
    /// `leaves`, or `data` itself without them, then the parameters.
    /// The dynamic call hands in open values and synchronous inline Drop
    /// values, and lends the others. `own` has the walk of each lent
    /// parameter, the receiver first, that `target` owns; the slot counts
    /// a copy of each. `given` has each handed-in value, by its index in
    /// `own`, that `target` only borrows, and its layout: the slot releases
    /// it after the call, including its Drop hooks.
    DynSlot {
        target: Hash128,
        leaves: Option<Vec<Leaf>>,
        params: Vec<Val>,
        results: Vec<Val>,
        own: Vec<(u32, Option<Walk>)>,
        given: Vec<(u32, RefLayout)>,
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
        given: Vec<(u32, RefLayout)>,
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
    /// `(list, extra) -> list`: std's `list_reserve`, and the reserve
    /// of [`Helper::ListExtend`]: room for
    /// `extra` (more than 0, which each caller tests) more elements
    /// without another growth: a pinned list panics; a shared
    /// buffer is copied, its elements taking their own counts; a buffer
    /// without the room moves to one of the needed length or twice the
    /// length plus 4, whichever is more. At most one new buffer.
    ListReserve {
        elem: Elem,
    },
    /// `(list, other) -> list`: std's `list_extend`: `other`'s elements
    /// appended with one [`Helper::ListReserve`] and one `memory.copy`,
    /// each taking its own counts. `other` may be the list itself.
    ListExtend {
        elem: Elem,
    },
    /// `() -> TypeId`: a `TypeId` addressing its static descriptor.
    TypeIdOf(TypeIdParts),
    /// `() -> address`: the canonical static descriptor of a type.
    TypeInfo(TypeIdParts),
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
    /// `(address, length) -> List[u8]`: a new list of exactly the `length`
    /// bytes at `address`, one sized buffer and one copy.
    StrToBytes {
        elem: Elem,
    },
    /// `(left, left_len, right, right_len) -> (address, length)`: takes the
    /// left string's count. An unshared buffer with tail room is extended in
    /// place; otherwise its bytes move to a buffer with doubled capacity.
    StrAppend,
    /// `(value: i64, destination: i32) -> (start, length)`: writes decimal
    /// bytes into the tail of twenty bytes of caller-owned room.
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
    /// `(at, len, from, byte) -> index`: std's `bytes_index`, the first
    /// byte at or after `from` equal to `byte`, or `len` (#428).
    BytesIndex,
    /// `(at, len, from, first, last, gap) -> index`: std's
    /// `bytes_index_pair`, the first offset `i` at or after `from` with
    /// `i + gap < len`, byte `i` equal to `first` and byte `i + gap` equal
    /// to `last`, or `len` (#428).
    BytesIndexPair,
    /// `(left_at, left_len, right_at, right_len) -> index`: std's
    /// `bytes_mismatch`, the offset of the first differing byte, else the
    /// shorter length (#428). [`Helper::StrEq`] and [`Helper::StrCmp`] call
    /// it.
    BytesMismatch,
    /// `(list) -> len`: std's `utf8_valid_len`, the length of the longest
    /// well-formed UTF-8 prefix of a `List[u8]` that ends at a scalar
    /// boundary (#428).
    Utf8ValidLen,
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
    /// (`req.drive.block-on.forbidden-contexts.v2`).
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
        blocking: bool,
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        res: crate::task::HostRes,
    },
    /// `(frame)`: a leaf's cancel, which aborts a waiting call.
    HostCancel,
    /// A host provider's drive slot for a method that may wait (§23.39):
    /// `(data, arguments) -> (frame, results)`. It calls `.start` at once;
    /// only a call the host left pending gets a leaf frame and zero results.
    /// A blocking slot takes a final caller-local buffer instead of allocating
    /// a frame; the caller explicitly drops that buffer's retained arguments.
    HostDrive {
        blocking: bool,
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        res: crate::task::HostRes,
    },
    /// The drive slot beside cold slot `cold` (§23.39): `(params) ->
    /// (frame, results)`. It calls `cold` and polls its frame once; a
    /// Ready frame is released and 0 returned with the results, a Pending
    /// one returned with zero results. Blocking slots take and ignore an
    /// additional caller buffer: an hd-provider's frame stays owned.
    Drive {
        blocking: bool,
        cold: Box<Helper>,
        params: Vec<Val>,
        results: Vec<Val>,
    },
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
    /// `() -> cell`: a wait cell, `{waiter edges, epoch}`, that hd code
    /// waits on and signals (`task.rs`, std's `Channel`).
    WaitCell,
    /// `(cell) -> frame`: a frame that completes once `cell` is signalled
    /// after the frame was made.
    WaitCold,
    /// `(frame) -> (ready)`.
    WaitPoll,
    /// `(cell)`: wakes every frame waiting on `cell`.
    WaitSignal,
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
    pub drops: Vec<DropCall>,
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

    #[must_use]
    pub fn needs_drop(&self) -> bool {
        self.walk.is_some() || !self.drops.is_empty()
    }
}

/// One `std.rt.Drop.drop` call in structural release glue. Every guard is
/// an enclosing enum tag and the variant that contains the value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DropCall {
    pub offset: u32,
    pub target: Hash128,
    pub guards: Vec<(Leaf, u32)>,
}

/// A reserved word of a list buffer and of a map table (§23.7), at the
/// same payload offset of both, which stays 0. It held the owner word of a
/// pinned collection until element references stopped counting their
/// buffer (#1336 phase 10); the elements still start after it.
pub const LIST_OWN: u64 = 8;

/// A list buffer's payload: the capacity in elements, the length, a
/// reserved word ([`LIST_OWN`]), then the elements inline from
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
        drops: Vec::new(),
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
                m.export("hd.count.pairs.pointer", Sym::Inst(c.count_audit.pointer));
                m.export("hd.count.pairs.length", Sym::Inst(c.count_audit.length));
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
        m.export("hd.uniques", Sym::Inst(c.unique_checks));
        #[cfg(feature = "checked-report")]
        {
            m.export("hd.cycles.pointer", Sym::Inst(c.pointer));
            m.export("hd.cycles.length", Sym::Inst(c.length));
            m.export("hd.count.pairs.pointer", Sym::Inst(c.count_audit.pointer));
            m.export("hd.count.pairs.length", Sym::Inst(c.count_audit.length));
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
        let wire = input.env.host_bridge(k).map_or(k, |bridge| bridge.wire);
        providers.push((host_vtable(input, m, wire)?, by_ref));
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
                f.address(*at);
                f.op(leaves[k].load(0));
            } else {
                f.global_get(g);
            }
            f.set(*l);
        }
        if let (Some(k), Some(addr)) = (s.walk.single(), s.walk.addr()) {
            f.get(ls[k as usize]);
            release_call(&mut f, addr);
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
                f.address(*at);
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
    crate::drive::vtable(input, m, def, slots, Vec::new())
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
    crate::drive::vtable(input, m, def, slots, parents)
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
#[allow(clippy::too_many_arguments)]
fn dyn_read(
    target: Hash128,
    read: &RefRead,
    params: &[Val],
    results: &[Val],
    own: &[(u32, Option<Walk>)],
    given: &[(u32, RefLayout)],
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
    // borrows it. Other arguments follow the slot's handoff rules.
    let mut owned = own.to_vec();
    let recv_owned = owned.first_mut().is_some_and(|r| r.1.take().is_some());
    own_lent(&mut f, &rvals, &recv, params, &owned, m);
    recv(&mut f);
    push_slot_params(&mut f, params.len() - usize::from(ctx.is_some()), ctx);
    f.call(Sym::Inst(target));
    if !recv_owned {
        ref_count(&mut f, &held, &read.value, false);
    }
    release_given(&mut f, own, given, m);
    f
}

/// Writes the literal `text` to standard error.
fn write_lit(f: &mut Func, m: &mut Module, text: &str) {
    let at = m.static_bytes(text.as_bytes(), 1);
    f.static_addr(at);
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
        Helper::MemoryCopy { source, dest } => crate::bulk::helper(m, *source, *dest, true),
        Helper::MemoryFill(memory) => crate::bulk::helper(m, *memory, *memory, false),
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
        Helper::RefPayload { .. } | Helper::RefPayloadGet { .. } | Helper::RefPayloadSet { .. } => {
            ref_payload_helper(h, m)
        }
        Helper::RefIsView | Helper::RefViewDrop | Helper::RefAddress => ref_helper(h, m),
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
        | Helper::HostDrive { .. }
        | Helper::Drive { .. }
        | Helper::HostCall { .. }
        | Helper::EntryCancel { .. }
        | Helper::ChildDrop
        | Helper::CleanupCold
        | Helper::CleanupPoll
        | Helper::CleanupCancel
        | Helper::PendingCold { .. }
        | Helper::PendingPoll { .. }
        | Helper::PendingCancel
        | Helper::WaitCell
        | Helper::WaitCold
        | Helper::WaitPoll
        | Helper::WaitSignal
        | Helper::Race { .. }
        | Helper::RaceDrop { .. }
        | Helper::All { .. } => crate::task::helper(h, m),
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
                Global::BlockingWakes,
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
            if m.uses_weak || m.open_world {
                for (k, forbid) in inits.iter().filter(|(k, _)| m.runtime_insts.contains(k)) {
                    if !(m.acyclic && m.cycle_insts.contains(k)) {
                        call_init(&mut f, *k, *forbid);
                    }
                }
                f.push(Ins::WeakTypeTable);
                f.push(Ins::WeakTypeCount);
                f.helper(Helper::WeakTypeInit);
                for (k, forbid) in inits.iter().filter(|(k, _)| !m.runtime_insts.contains(k)) {
                    call_init(&mut f, *k, *forbid);
                }
            } else {
                for (k, forbid) in inits {
                    // A program that links no collector needs no collector state.
                    if !(m.acyclic && m.cycle_insts.contains(k)) {
                        call_init(&mut f, *k, *forbid);
                    }
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
            glue_of(&mut f, m);
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
        // `panic_with(msg, "")`, whose report is `msg` alone, through the
        // table: V8 inlines a small direct callee by size when it has no
        // call counts (`--no-liftoff`), so every panic site would copy the
        // code that writes the report, `rt:to_buf` with it. An inlined
        // panic helper is a few nodes.
        Helper::Panic(msg) => {
            let mut f = m.func("rt:panic", vec![], vec![]);
            let at = m.static_bytes(msg.as_bytes(), 1);
            f.static_addr(at);
            f.u32(u32::try_from(msg.len()).expect("message"));
            f.i32(0);
            f.i32(0);
            f.cold_helper(Helper::PanicWith, vec![Val::I32; 4], vec![]);
            f.op(Instruction::Unreachable);
            f
        }
        Helper::LoanShare => loan_share(m, true),
        Helper::LoanUnshare => loan_share(m, false),
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
            // A cold call: the panic writes its message through here, and
            // a cycle of direct calls is one an engine's inliner unrolls.
            f.cold_helper(
                Helper::Panic("heap-exhausted: the exchange buffer cannot grow".into()),
                vec![],
                vec![],
            );
            f.op(Instruction::Unreachable);
            f.op(Instruction::End);
            f.op(Instruction::End);
            f.i32(0);
            f.get(0);
            f.get(1);
            f.push(crate::bulk::copy(HEAP, BUF));
            f.get(1);
            f
        }
        Helper::Alloc => alloc(m),
        Helper::StrEq => str_eq(m),
        Helper::New => new_object(m, None),
        Helper::NewFixed { size } => new_object(m, Some(*size)),
        Helper::AllocClass { class, bytes } => alloc_class(m, *class, *bytes),
        Helper::FreeClass => free_class(m),
        Helper::FreeClassLoaded => free_class_loaded(m),
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
            walk_code(&mut f, walk, &|f, k| f.get(k), &release_call);
            f
        }
        Helper::ReleaseRef => release_ref(m, Addr::Ref),
        Helper::ReleaseKnown => release_ref(m, Addr::Known),
        Helper::ReleaseDrain => release_drain(m),
        Helper::TakeBox => take_box(m),
        Helper::RetainKnown => {
            let mut f = m.func("rt:retain_known", vec![Val::I32], vec![]);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain_known));
            }
            f
        }
        Helper::DropFields {
            leaves,
            walk,
            drops,
        } => fields_glue(m, "rt:drop", leaves, walk, drops, &release_call),
        Helper::DropValue {
            size,
            leaves,
            walk,
            drops,
        } => drop_value_glue(m, *size, leaves, walk, drops),
        Helper::WeakNoopDrop => m.func("rt:weak_noop_drop", vec![Val::I32], vec![]),
        Helper::WeakNew => weak_new(m),
        Helper::WeakEntryDrop => weak_entry_drop(m),
        Helper::WeakTypeInit => weak_type_init(m),
        Helper::RetainFields { leaves, walk } => {
            fields_glue(m, "rt:retain_fields", leaves, walk, &[], &retain_call(m))
        }
        Helper::ListBuf { elem } => list_buf(m, elem),
        Helper::DropElems { elem } => drop_elems(m, elem),
        Helper::CollCopy => coll_copy(m),
        Helper::CtxFind => ctx_find(m),
        Helper::CtxNew => ctx_new(m),
        Helper::CopyElems { elem } => copy_elems(elem, m),
        Helper::ListUnique { elem } => list_unique(elem, m),
        Helper::ListCopy { elem } => list_copy(elem, m),
        Helper::ListGrow { elem } => list_grow(elem, m),
        Helper::MapTable { entry } => map::table(m, entry),
        Helper::DropEntries { entry } => map::drop_entries(m, entry),
        Helper::MapHash { key, ops } => map::hash(m, key, ops),
        Helper::MapFind { entry, ops } => map::find(m, entry, ops),
        Helper::MapPut { entry, ops } => map::put(m, entry, ops),
        Helper::MapGrow { entry } => map::grow(entry, m),
        Helper::MapUnique { entry } => map::unique(entry, m),
        Helper::MapSlotNext => map::slot_next(m),
        Helper::DynSlot {
            target,
            leaves,
            params,
            results,
            own,
            given,
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
            release_given(&mut f, own, given, m);
            f
        }
        Helper::DynRead {
            target,
            read,
            params,
            results,
            own,
            given,
            ctx,
        } => dyn_read(*target, read, params, results, own, given, ctx.as_ref(), m),
        Helper::ListTruncate { elem } => list_truncate(m, elem),
        Helper::ListReserve { elem } => list_reserve(elem, m),
        Helper::ListExtend { elem } => list_extend(elem, m),
        Helper::TypeIdOf(parts) => type_id(parts, vec![], m),
        Helper::TypeInfo(parts) => type_info(parts, m),
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
        Helper::StrToBytes { elem } => str_to_bytes(m, elem),
        Helper::StrAppend => str_append(m),
        Helper::StrCmp => str_cmp(m),
        Helper::BytesIndexJson => {
            let mut f = m.func("rt:bytes_index_json", vec![Val::I32; 3], vec![Val::I32]);
            f.raw = Some(crate::scan::json_index_code());
            f
        }
        Helper::BytesIndexPair => {
            let mut f = m.func("rt:bytes_index_pair", vec![Val::I32; 6], vec![Val::I32]);
            f.raw = Some(crate::scan::index_pair_code());
            f
        }
        Helper::BytesIndex => {
            let mut f = m.func("rt:bytes_index", vec![Val::I32; 4], vec![Val::I32]);
            f.raw = Some(crate::scan::index_code());
            f
        }
        Helper::BytesMismatch => {
            let mut f = m.func("rt:bytes_mismatch", vec![Val::I32; 4], vec![Val::I32]);
            f.raw = Some(crate::scan::mismatch_code());
            f
        }
        Helper::Utf8ValidLen => {
            let mut f = m.func("rt:utf8_valid_len", vec![Val::I32], vec![Val::I32]);
            f.raw = Some(crate::scan::utf8_valid_len_code());
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
        Helper::ListBuf { .. } | Helper::MapTable { .. } => {
            checked_kind(&mut f, m, 5);
        }
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
/// the most a `u64` or a signed `i64` needs. The remaining prefix is zeroed,
/// so all twenty bytes are initialized; the caller may expose only the
/// written tail. The kernel neither allocates nor copies its digits.
fn fmt_int(m: &Module, signed: bool) -> Func {
    let mut f = m.func(
        if signed { "rt:fmt_i64" } else { "rt:fmt_u64" },
        vec![Val::I64, Val::I32],
        vec![Val::I32, Val::I32],
    );
    let (buf, at, neg, v) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I64),
    );
    f.get(1);
    f.set(buf);
    // Fixed-size stores keep initialization local to this tiny kernel;
    // a variable-length memory.fill needs a runtime builtin on V8.
    f.get(buf);
    f.op(Instruction::I64Const(0));
    f.op(Instruction::I64Store(mem(HEAP, 0, 0)));
    f.get(buf);
    f.op(Instruction::I64Const(0));
    f.op(Instruction::I64Store(mem(HEAP, 8, 0)));
    f.get(buf);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 16, 0)));
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
    f.op(Instruction::I32Sub);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.get(at);
    f.op(Instruction::I32Sub);
    f
}

/// [`Helper::StrEq`]: equal lengths, then no mismatch before the end.
fn str_eq(m: &Module) -> Func {
    let mut f = m.func("rt:str_eq", vec![Val::I32; 4], vec![Val::I32]);
    f.get(0);
    f.get(2);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(1);
    f.get(3);
    f.op(Instruction::I32Eq);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    for l in 0..4 {
        f.get(l);
    }
    f.helper(Helper::BytesMismatch);
    f.get(1);
    f.op(Instruction::I32Eq);
    f
}

/// [`Helper::StrCmp`]: the first differing byte decides; when the shorter
/// is a prefix of the longer, the lengths do.
fn str_cmp(m: &Module) -> Func {
    let mut f = m.func("rt:str_cmp", vec![Val::I32; 4], vec![Val::I32]);
    let (k, x, y) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
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
    for l in 0..4 {
        f.get(l);
    }
    f.helper(Helper::BytesMismatch);
    f.set(k);
    // A mismatch before the shorter length: the bytes there decide.
    f.get(k);
    f.get(1);
    f.get(3);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32LtU);
    f.op(Instruction::Select);
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
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
    sign(&mut f, x, y);
    f.op(Instruction::Return);
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
    f.push(crate::bulk::copy(BUF, HEAP));
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

/// After a slot's call: releases each value parameter in `given`
/// (an index in `own`, whose entries give each parameter's value count,
/// and its layout), whose ownership the dynamic call handed in and the target
/// only borrowed. The target's results stay on the stack.
fn release_given(
    f: &mut Func,
    own: &[(u32, Option<Walk>)],
    given: &[(u32, RefLayout)],
    m: &Module,
) {
    if m.runtime.is_none() {
        return;
    }
    for (k, value) in given {
        let first: u32 = 1 + own[1..*k as usize].iter().map(|o| o.0).sum::<u32>();
        let n = own[*k as usize].0;
        let locals: Vec<u32> = (first..first + n).collect();
        ref_count(f, &locals, value, false);
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

/// What retains the address on the stack, given whether it is a known
/// payload start: std's `retain`, `retain_known` for a known start, or
/// nothing when the program counts nothing.
pub(crate) fn retain_call(m: &Module) -> impl Fn(&mut Func, Addr) {
    let retain = m.runtime.map(|r| r.retain);
    move |f: &mut Func, addr: Addr| match retain {
        Some(_) if addr != Addr::Ref => f.helper(Helper::RetainKnown),
        Some(k) => f.call(Sym::Inst(k)),
        None => f.op(Instruction::Drop),
    }
}

/// The release of one counted address on the stack, of leaf `addr`:
/// without finding its block when it is a known payload start.
pub(crate) fn release_call(f: &mut Func, addr: Addr) {
    f.helper(match addr {
        Addr::Ref => Helper::ReleaseRef,
        Addr::Known => Helper::ReleaseKnown,
    });
}

/// Emits `walk` over the values that `get(k)` pushes, running `on` with
/// each counted address on the stack and the leaf that holds it: an
/// enum's arms by its tag.
pub(crate) fn walk_code(
    f: &mut Func,
    walk: &Walk,
    get: &dyn Fn(&mut Func, u32),
    on: &dyn Fn(&mut Func, Addr),
) {
    match walk {
        Walk::Ref(k) => {
            get(f, *k);
            on(f, Addr::Ref);
        }
        Walk::Known(k) => {
            get(f, *k);
            on(f, Addr::Known);
        }
        // A plain reference: only a representation view, a payload start.
        Walk::View(k) => {
            get(f, *k);
            f.helper(Helper::RefIsView);
            f.op(Instruction::If(BlockType::Empty));
            get(f, *k);
            on(f, Addr::Known);
            f.op(Instruction::End);
        }
        Walk::Glue => {}
        Walk::Seq(ws) => {
            for w in ws {
                walk_code(f, w, get, on);
            }
        }
        Walk::Nullable(w) => {
            if let Some(k) = w.single() {
                get(f, k);
                f.op(Instruction::If(BlockType::Empty));
                walk_code(f, w, get, on);
                f.op(Instruction::End);
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
    drops: &[DropCall],
    on: &dyn Fn(&mut Func, Addr),
) -> Func {
    let mut f = m.func(name, vec![Val::I32], vec![]);
    call_drops(&mut f, 0, drops);
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
    push_desc_recursive(f, glue, cyclic, false);
}

/// [`push_desc`], with a code-only marker for a release glue that can call
/// itself recursively even when the object is not cycle-capable.
pub(crate) fn push_desc_recursive(f: &mut Func, glue: Option<Sym>, cyclic: bool, recursive: bool) {
    match glue {
        Some(g) => {
            f.table_slot(g.clone());
            f.u32(if cyclic { CYCLIC + 1 } else { 1 });
            f.op(Instruction::I32Add);
            if cyclic || recursive {
                f.drain_glue(g);
            }
        }
        None => f.i32(0),
    }
}

/// [`push_desc_recursive`] with an exact weak-target type alias. A type with
/// no ordinary release glue uses one shared no-op function; the alias slot,
/// not the function body, carries its type identity.
pub(crate) fn push_typed_desc_recursive(
    f: &mut Func,
    glue: Option<Sym>,
    cyclic: bool,
    recursive: bool,
    key: Hash128,
) {
    let glue = glue.unwrap_or(Sym::Helper(Helper::WeakNoopDrop));
    f.descriptor(glue.clone(), DescriptorKey::weak(key));
    f.u32(if cyclic { CYCLIC + 1 } else { 1 });
    f.op(Instruction::I32Add);
    if cyclic || recursive {
        f.drain_glue(glue);
    }
}

/// Pushes the release glue's table slot plus one from the descriptor word
/// on the stack, without the collector and allocation-class bits.
pub(crate) fn glue_of(f: &mut Func, m: &Module) {
    f.u32(if m.uses_weak || m.open_world {
        WEAK_GLUE
    } else {
        GLUE
    });
    f.op(Instruction::I32And);
}

/// The allocator's twenty small classes, including the object's header.
/// Zero/large objects keep the ordinary path.
fn small_class(size: u32) -> Option<(u32, u32)> {
    const BYTES: [u32; 20] = [
        16, 32, 48, 64, 80, 96, 112, 128, 160, 208, 256, 320, 400, 512, 640, 800, 1024, 1280, 1600,
        2048,
    ];
    BYTES.into_iter().enumerate().find_map(|(i, bytes)| {
        (size <= bytes).then(|| (u32::try_from(i + 1).expect("size class"), bytes))
    })
}

/// The offset of each class's freed list in std's allocator metadata
/// (`std.rt.alloc`'s `FREED`): class `c`'s is at `FREED + 4 * c`.
const FREED: u32 = 256;

/// Pop the class's freed list, else a free slot, or bump within the head
/// page when it remains non-full. The last available slot, a missing
/// page/table, and page-list maintenance stay in std, which is given the
/// class's bytes as the size. There is no call between reading metadata
/// and updating it. The page fast paths share one tail, so that V8 inlines
/// this into each allocation.
fn alloc_class(m: &Module, class: u32, bytes: u32) -> Func {
    let mut f = m.func(format!("rt:alloc_class/{class}"), vec![], vec![Val::I32]);
    let Some(r) = m.runtime else {
        f.u32(bytes);
        f.helper(Helper::Alloc);
        return f;
    };
    if r.checked.is_some() {
        f.u32(bytes);
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
    f.op(Instruction::Block(BlockType::Empty));
    f.call(Sym::Inst(r.small.metadata));
    f.op(Instruction::LocalTee(table));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(0));
    // A block on the class's freed list, which its page still counts.
    let freed = u64::from(FREED + class * 4);
    f.get(table);
    f.op(Instruction::I32Load(mem(HEAP, freed, 2)));
    f.op(Instruction::LocalTee(slot));
    f.op(Instruction::If(BlockType::Empty));
    f.get(table);
    f.get(slot);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Store(mem(HEAP, freed, 2)));
    f.get(slot);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(table);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(class * 4), 2)));
    f.op(Instruction::LocalTee(page));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(0));
    f.get(page);
    f.op(Instruction::I32Load16U(mem(HEAP, 14, 1)));
    f.i32(4);
    f.op(Instruction::I32Shl);
    f.set(bump);
    f.get(page);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::LocalTee(slot));
    f.op(Instruction::If(BlockType::Empty));
    // Pop the free list unless that leaves the page full.
    f.get(slot);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::LocalTee(next));
    f.get(bump);
    f.u32(65536 - bytes);
    f.op(Instruction::I32LeU);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(page);
    f.get(next);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.op(Instruction::Else);
    // Bump unless that leaves the page full.
    f.get(bump);
    f.u32(65536 - 2 * bytes);
    f.op(Instruction::I32GtU);
    f.op(Instruction::BrIf(1));
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
    f.op(Instruction::End);
    f.get(page);
    f.get(page);
    f.op(Instruction::I32Load16U(mem(HEAP, 12, 1)));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store16(mem(HEAP, 12, 1)));
    f.get(slot);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    // The slow path: only here does a known class's allocation ask whether
    // a collection is due. A collection becomes due only as candidates
    // gain blocks that nothing frees, so their class's pages fill and reach
    // this path within a page of allocations (§23.28).
    collect_if_due(&mut f, m);
    f.u32(bytes);
    f.u32(class);
    f.call(Sym::Inst(r.small.alloc));
    f
}

/// Runs a collection when the collector says one is due.
fn collect_if_due(f: &mut Func, m: &Module) {
    let Some(r) = m.runtime.filter(|_| !m.acyclic) else {
        return;
    };
    f.call(Sym::Inst(r.cycle.due));
    f.op(Instruction::If(BlockType::Empty));
    // A cold call (§23.17): through the table, V8 does not inline the
    // collection into each allocation and spend its budget there.
    f.cold_helper(Helper::Collect, vec![], vec![]);
    f.op(Instruction::End);
}

/// A live block of a known class goes on its class's freed list, which
/// its page keeps counting until std's `reclaim`. Buffered candidates keep
/// the collector's free.
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
    let (desc, class) = (f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(desc);
    f.get(desc);
    f.u32(crate::module::CLASS_MASK);
    f.op(Instruction::I32And);
    f.u32(crate::module::CLASS_SHIFT);
    f.op(Instruction::I32ShrU);
    f.set(class);
    f.get(0);
    f.get(desc);
    f.get(class);
    f.helper(Helper::FreeClassLoaded);
    f
}

/// The fixed-class free after its caller has loaded the descriptor and decoded
/// its class. Keeping this body out of the drain preserves its compact hot loop
/// while avoiding the old helper's descriptor reload and class decode.
fn free_class_loaded(m: &Module) -> Func {
    let mut f = m.func("rt:free_class_loaded", vec![Val::I32; 3], vec![]);
    free_class_body(&mut f, m, 0, 1, 2);
    f
}

/// Emits the fixed-class free after the caller has loaded the descriptor and
/// decoded its class. A release drain already needs the descriptor for glue
/// dispatch, so its generic path can reuse both values instead of loading and
/// decoding the word again in [`Helper::FreeClass`].
fn free_class_body(f: &mut Func, m: &Module, block: u32, desc: u32, class: u32) {
    let Some(r) = m.runtime else {
        return;
    };
    let list = f.local(Val::I32);
    f.op(Instruction::Block(BlockType::Empty));
    f.get(class);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32Eqz);
    f.get(desc);
    f.u32(crate::module::BUFFERED);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.call(Sym::Inst(r.small.metadata));
    f.get(class);
    f.i32(2);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32Add);
    f.set(list);
    f.get(block);
    f.get(list);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(FREED), 2)));
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(list);
    f.get(block);
    f.op(Instruction::I32Store(mem(HEAP, u64::from(FREED), 2)));
    f.op(Instruction::Br(1));
    f.op(Instruction::End);
    f.get(block);
    f.call(Sym::Inst(r.free));
    f.op(Instruction::End);
}

fn free_object(f: &mut Func, m: &Module, block: u32) {
    let Some(r) = m.runtime else {
        return;
    };
    clear_weak(f, m, block);
    f.get(block);
    if m.fixed_allocations && r.checked.is_none() {
        f.helper(Helper::FreeClass);
    } else {
        f.call(Sym::Inst(r.free));
    }
}

/// Clears a target's side entry at logical death, before cycle buffering or
/// allocator reuse. Session helpers are weak-capable from their base module,
/// because a later input may introduce the first weak constructor.
fn clear_weak(f: &mut Func, m: &Module, block: u32) {
    if !(m.uses_weak || m.open_world) {
        return;
    }
    let Some(r) = m.runtime else {
        return;
    };
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.u32(WEAK);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f.call(Sym::Inst(r.weak.clear));
    f.op(Instruction::End);
}

/// The common lowering of `weak` and `weak_mut`.
fn weak_type_init(m: &Module) -> Func {
    let mut f = m.func("rt:weak_type_init", vec![Val::I32, Val::I32], vec![]);
    let Some(r) = m.runtime else {
        return f;
    };
    let (row, at) = (f.local(Val::I32), f.local(Val::I32));
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(row);
    f.get(1);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.get(row);
    f.i32(3);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32Add);
    f.op(Instruction::LocalTee(at));
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.call(Sym::Inst(r.weak.type_bind));
    f.get(row);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(row);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f
}

/// The common lowering of `weak` and `weak_mut`.
fn weak_new(m: &Module) -> Func {
    let mut f = m.func(
        "rt:weak_new",
        vec![Val::I64, Val::I32, Val::I32, Val::I32],
        vec![Val::I32],
    );
    let Some(r) = m.runtime else {
        f.i32(0);
        return f;
    };
    let (target, block, entry, actual, row, valid) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    // Erased-field serials carry their base in the high word; only a plain
    // physical address can denote a whole allocation.
    f.get(0);
    f.op(Instruction::I64Const(32));
    f.op(Instruction::I64ShrU);
    f.op(Instruction::I64Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.op(Instruction::I32WrapI64);
    f.set(target);
    f.get(target);
    f.call(Sym::Inst(r.block_of));
    f.op(Instruction::LocalTee(block));
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f.get(target);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.u32(WEAK_GLUE);
    f.op(Instruction::I32And);
    f.op(Instruction::LocalTee(actual));
    f.call(Sym::Inst(r.weak.type_of));
    f.get(1);
    f.op(Instruction::I32Eq);
    f.set(valid);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(valid);
    f.op(Instruction::BrIf(1));
    f.get(row);
    f.get(3);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(2);
    f.get(row);
    f.i32(3);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32Add);
    f.op(Instruction::LocalTee(entry));
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(actual);
    f.op(Instruction::I32Eq);
    f.get(entry);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::I32And);
    f.set(valid);
    f.get(row);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(row);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(valid);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(target);
    f.call(Sym::Inst(r.weak.lookup));
    f.op(Instruction::LocalTee(entry));
    f.op(Instruction::If(BlockType::Empty));
    f.get(entry);
    f.call(Sym::Inst(r.retain_known));
    f.get(entry);
    f.op(Instruction::Return);
    f.op(Instruction::End);

    f.u32(8);
    push_desc(&mut f, Some(Sym::Helper(Helper::WeakEntryDrop)), false);
    f.helper(Helper::New);
    f.op(Instruction::LocalTee(entry));
    f.get(target);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(target);
    f.get(entry);
    f.call(Sym::Inst(r.weak.bind));
    f.get(block);
    // Allocating the side entry may collect and therefore change the target's
    // color or BUFFERED bits. Preserve that current descriptor word.
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.u32(WEAK);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.get(entry);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(0);
    f
}

fn weak_entry_drop(m: &Module) -> Func {
    let mut f = m.func("rt:weak_entry_drop", vec![Val::I32], vec![]);
    let Some(r) = m.runtime else {
        return f;
    };
    let target = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::LocalTee(target));
    f.op(Instruction::If(BlockType::Empty));
    f.get(target);
    f.get(0);
    f.call(Sym::Inst(r.weak.forget));
    f.get(target);
    f.u32(HEADER - 4);
    f.op(Instruction::I32Sub);
    f.get(target);
    f.u32(HEADER - 4);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.u32(!WEAK);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.op(Instruction::End);
    f
}

/// [`Helper::New`]: a block of the header and the payload from std's
/// `alloc`, or from the bump pointer when the program counts nothing.
///
/// With the runtime, a collection runs first when one is due (at a known
/// class, only on the class's slow path). An object whose descriptor has
/// [`CYCLIC`] takes part in collections; the descriptor written here has
/// its collector bits clear, black and no frame (§23.28).
fn new_object(m: &Module, fixed: Option<u32>) -> Func {
    let small = fixed.and_then(|size| {
        m.runtime.filter(|r| r.checked.is_none())?;
        small_class(size.checked_add(HEADER)?)
    });
    let name = small.map_or_else(
        || "rt:new".into(),
        |(class, _)| format!("rt:new/class-{class}"),
    );
    let mut f = m.func(name, vec![Val::I32, Val::I32], vec![Val::I32]);
    let block = f.local(Val::I32);
    // A known class asks on its slow path only ([`alloc_class`]).
    if small.is_none() {
        collect_if_due(&mut f, m);
    }
    match (m.runtime, small) {
        // The class's block holds the header and the payload.
        (Some(_), Some((class, bytes))) => f.helper(Helper::AllocClass { class, bytes }),
        (Some(r), None) => {
            f.get(0);
            f.u32(HEADER);
            f.op(Instruction::I32Add);
            f.call(Sym::Inst(r.alloc));
        }
        (None, _) => {
            f.get(0);
            f.u32(HEADER);
            f.op(Instruction::I32Add);
            f.helper(Helper::Alloc);
        }
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
/// native stack; the outermost one goes to [`Helper::ReleaseDrain`] at
/// once, which keeps this small enough for V8 to inline. While the
/// collector traces an object (`Tracing`), the reference goes to its
/// `visit` instead, checked here so that `visit` is called only during a
/// collection (§23.28).
///
/// `addr` says what the address is: any address ([`Helper::ReleaseRef`])
/// or a known payload start ([`Helper::ReleaseKnown`]).
fn release_ref(m: &Module, addr: Addr) -> Func {
    let mut f = m.func("rt:release_ref", vec![Val::I32], vec![]);
    let Some(r) = m.runtime else {
        return f;
    };
    let block = f.local(Val::I32);
    let known = addr != Addr::Ref;
    if !m.acyclic {
        // A drain cannot overlap collector tracing. Nested releases therefore
        // bypass the collector test; only the outer release performs it.
        f.op(Instruction::Block(BlockType::Empty));
        f.global_get(Global::Releasing);
        f.op(Instruction::BrIf(0));
        f.global_get(Global::Tracing);
        f.op(Instruction::If(BlockType::Empty));
        f.get(0);
        // Through the table, as `collect` is: V8 would otherwise inline
        // the collector's walk into every release.
        f.table_slot(Sym::Inst(r.cycle.visit));
        f.call_indirect(vec![Val::I32], vec![Val::I32]);
        f.op(Instruction::If(BlockType::Empty));
        f.op(Instruction::Return);
        f.op(Instruction::End);
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
    clear_weak(&mut f, m, block);

    // The final release already removed the block from cycle candidates.
    f.global_get(Global::Releasing);
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.global_get(Global::ReleaseHead);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(block);
    f.global_set(Global::ReleaseHead);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(block);
    if m.acyclic && known && !m.drain_glues.is_empty() {
        // Keep the recursive drain across a call boundary in optimizing
        // engines. This indirect call runs once for the outer structure;
        // the drain still dispatches its per-object recursive glue directly.
        f.table_slot(Sym::Helper(Helper::ReleaseDrain));
        f.call_indirect(vec![Val::I32], vec![]);
    } else {
        f.helper(Helper::ReleaseDrain);
    }
    f
}

/// [`Helper::TakeBox`].
fn take_box(m: &Module) -> Func {
    let mut f = m.func("rt:take_box", vec![Val::I32], vec![Val::I32]);
    if m.runtime.is_none() {
        f.i32(1);
        return f;
    }
    let (block, desc) = (f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::LocalTee(block));
    unique_count(&mut f, m);
    f.i32(1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    clear_weak(&mut f, m, block);
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(desc);
    free_loaded_object(&mut f, m, block, desc);
    f.i32(1);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.helper(Helper::ReleaseKnown);
    f.i32(0);
    f
}

/// [`Helper::ReleaseDrain`]. A recursive type's glue
/// ([`Module::drain_glues`]) is called directly when the descriptor names
/// it, so that V8 can inline it here; any other glue goes through the
/// table.
fn release_drain(m: &Module) -> Func {
    let mut f = m.func("rt:release_drain", vec![Val::I32], vec![]);
    let (block, slot, desc) = (0, f.local(Val::I32), f.local(Val::I32));
    f.i32(1);
    f.global_set(Global::Releasing);
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.op(Instruction::LocalTee(desc));
    glue_of(&mut f, m);
    f.op(Instruction::LocalTee(slot));
    f.op(Instruction::If(BlockType::Empty));
    if m.drain_glues.is_empty() {
        f.get(block);
        f.u32(HEADER);
        f.op(Instruction::I32Add);
        f.get(slot);
        f.i32(1);
        f.op(Instruction::I32Sub);
        f.call_indirect(vec![Val::I32], vec![]);
    } else {
        dispatch_glue(&mut f, &m.drain_glues, block, slot);
    }
    f.op(Instruction::End);
    free_loaded_object(&mut f, m, block, desc);
    f.global_get(Global::ReleaseHead);
    f.op(Instruction::LocalTee(block));
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.global_set(Global::ReleaseHead);
    f.op(Instruction::Br(1));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(0);
    f.global_set(Global::Releasing);
    f
}

/// Frees a block whose descriptor is already in `desc`. The generic drain
/// has loaded it for glue dispatch, so a fixed allocation can decode its
/// class once and enter the same free body without a helper reload.
fn free_loaded_object(f: &mut Func, m: &Module, block: u32, desc: u32) {
    let Some(r) = m.runtime else {
        return;
    };
    if m.fixed_allocations && r.checked.is_none() {
        let class = f.local(Val::I32);
        f.get(desc);
        f.u32(crate::module::CLASS_MASK);
        f.op(Instruction::I32And);
        f.u32(crate::module::CLASS_SHIFT);
        f.op(Instruction::I32ShrU);
        f.set(class);
        f.get(block);
        f.get(desc);
        f.get(class);
        f.helper(Helper::FreeClassLoaded);
    } else {
        f.get(block);
        f.call(Sym::Inst(r.free));
    }
}

/// Runs the glue at table slot `slot` plus one over the payload of
/// `block`: one of `glues` by a direct call, any other through the table.
fn dispatch_glue(f: &mut Func, glues: &[Sym], block: u32, slot: u32) {
    f.get(slot);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.set(slot);
    f.op(Instruction::Block(BlockType::Empty));
    for glue in glues {
        f.get(slot);
        f.table_slot(glue.clone());
        f.op(Instruction::I32Eq);
        f.op(Instruction::If(BlockType::Empty));
        f.get(block);
        f.u32(HEADER);
        f.op(Instruction::I32Add);
        f.call(glue.clone());
        f.op(Instruction::Br(1));
        f.op(Instruction::End);
    }
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f.get(slot);
    f.call_indirect(vec![Val::I32], vec![]);
    f.op(Instruction::End);
}

/// [`Helper::CollCopy`]: one more count on the buffer of a list or a map,
/// which counts its holders; a change through either holder copies it
/// first (`rt:list_unique`). A local reference into it counts nothing, so
/// its count is its holders' alone (`ref.count.local`).
fn coll_copy(m: &Module) -> Func {
    let mut f = m.func("rt:coll_copy", vec![Val::I32], vec![Val::I32]);
    let b = 0;
    f.get(b);
    f.helper(Helper::RetainKnown);
    f.get(b);
    f
}

/// [`Helper::ListBuf`]: a capacity whose bytes pass 4 GiB is
/// `heap-exhausted`.
fn list_buf(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:list_buf", vec![Val::I32], vec![Val::I32]);
    let b = f.local(Val::I32);
    if let Some(most) = (u32::MAX - LIST_ELEMS - HEADER).checked_div(elem.size) {
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
        .needs_drop()
        .then(|| Sym::Helper(Helper::DropElems { elem: elem.clone() }));
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
    f.push(crate::bulk::copy(HEAP, HEAP));
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

/// A function of `params`, which it ignores, that returns the canonical
/// descriptor's address. Both entry forms use the same helper symbol,
/// including when the session linker reuses an earlier module's helper.
fn type_id(parts: &TypeIdParts, params: Vec<Val>, m: &mut Module) -> Func {
    let mut f = m.func("rt:type_id", params, vec![Val::I32]);
    f.helper(Helper::TypeInfo(parts.clone()));
    f
}

/// Initialize the descriptor through relocatable instructions rather
/// than embedding absolute string addresses in a linked data segment.
/// Its first word doubles as the initialized flag; type keys are nonempty.
fn type_info(parts: &TypeIdParts, m: &mut Module) -> Func {
    let mut f = m.func("rt:type_info", vec![], vec![Val::I32]);
    let at = m.static_slot(16, 4);
    m.label_static(at, &format!("type descriptor {}", parts.name));
    let fields = [&parts.name, &parts.shown].map(|text| {
        (
            m.static_bytes(text.as_bytes(), 1),
            u32::try_from(text.len()).expect("type name"),
        )
    });
    f.static_addr(at);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    // Publish the first word last, after the complete descriptor is ready.
    for (index, (text, len)) in fields.into_iter().enumerate().rev() {
        let offset = (index * 8) as u64;
        f.static_addr(at);
        f.u32(len);
        f.op(Instruction::I32Store(mem(HEAP, offset + 4, 2)));
        f.static_addr(at);
        f.static_addr(text);
        f.op(Instruction::I32Store(mem(HEAP, offset, 2)));
    }
    f.op(Instruction::End);
    f.static_addr(at);
    f
}

/// [`Helper::ListTruncate`].
fn list_truncate(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:list_truncate", vec![Val::I32, Val::I32], vec![Val::I32]);
    let (b, at, n) = (0, f.local(Val::I32), f.local(Val::I32));
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
            walk_code(f, walk, &|f, k| f.get(ls[k as usize]), &release_call);
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
    if !elem.needs_drop() {
        return f;
    }
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(0);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.set(at);
    let ls = load_elem_locals(&mut f, &elem.leaves);
    each_elem(&mut f, at, n, elem.size, &|f| {
        call_drops(f, at, &elem.drops);
        fill_elem(f, at, &elem.leaves, &ls);
        if let Some(walk) = &elem.walk {
            walk_code(f, walk, &|f, k| f.get(ls[k as usize]), &release_call);
        }
    });
    f
}

/// Calls each `std.rt.Drop` implementation of `drops` on the value at
/// `base`. A trace runs them in every pass, so their releases visit what
/// the value holds (`module.rt.drop.trace`).
fn call_drops(f: &mut Func, base: u32, drops: &[DropCall]) {
    for drop in drops {
        for &(tag, variant) in &drop.guards {
            f.get(base);
            f.op(tag.load(0));
            f.u32(variant);
            f.op(Instruction::I32Eq);
            f.op(Instruction::If(BlockType::Empty));
        }
        f.get(base);
        if drop.offset != 0 {
            f.u32(drop.offset);
            f.op(Instruction::I32Add);
        }
        f.call(Sym::Inst(drop.target));
        for _ in &drop.guards {
            f.op(Instruction::End);
        }
    }
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

/// [`Helper::ListUnique`]: a buffer with more than one holder and no owner
/// word is copied by [`Helper::ListCopy`], a cold call (`Func::cold_helper`).
/// What a hot caller's engine inlines is then a few loads and a compare,
/// not the copy loop and the allocator behind it.
fn list_unique(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_unique", vec![Val::I32], vec![Val::I32]);
    let b = 0;
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    unique_count(&mut f, m);
    f.i32(1);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.cold_helper(
        Helper::ListCopy { elem: elem.clone() },
        vec![Val::I32],
        vec![Val::I32],
    );
    f.set(b);
    f.op(Instruction::End);
    f.get(b);
    f
}

/// [`Helper::ListCopy`]: a shared buffer is copied at its length; its
/// elements take their own counts and the old buffer one holder less.
fn list_copy(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_copy", vec![Val::I32], vec![Val::I32]);
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
    f.push(crate::bulk::copy(HEAP, HEAP));
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
    f
}

/// [`Helper::ListGrow`]: a full buffer moves to
/// one of twice its length plus 4, and the old one, its elements moved and its length 0, loses
/// the count the list held.
fn list_grow(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_grow", vec![Val::I32], vec![Val::I32]);
    let (b, n, nb) = (0, f.local(Val::I32), f.local(Val::I32));
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
    f.push(crate::bulk::copy(HEAP, HEAP));
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

/// Read a count for a sole-owner decision. Unchecked builds emit only
/// the original load; checked builds count the executed test in std.
pub(crate) fn unique_count(f: &mut Func, m: &Module) {
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    if let Some(c) = m.runtime.and_then(|r| r.checked) {
        f.call(Sym::Inst(c.observe_unique));
    }
}

/// Pushes whether the buffer in local `b` is the list's own: one holder.
fn buffer_own(f: &mut Func, b: u32, m: &Module) {
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    unique_count(f, m);
    f.i32(1);
    f.op(Instruction::I32Eq);
}

/// [`Helper::ListReserve`].
fn list_reserve(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_reserve", vec![Val::I32, Val::I32], vec![Val::I32]);
    let (b, extra) = (0, 1);
    let (n, need, own, nb) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.get(extra);
    f.op(Instruction::I32Add);
    f.set(need);
    buffer_own(&mut f, b, m);
    f.set(own);
    f.get(own);
    f.op(Instruction::I32Eqz);
    f.get(need);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32GtU);
    f.op(Instruction::I32Or);
    f.op(Instruction::If(BlockType::Empty));
    // The capacity: the needed length or twice the length plus 4, when
    // the buffer has no room; else the buffer's (a shared one's copy).
    f.get(need);
    f.get(n);
    f.i32(1);
    f.op(Instruction::I32Shl);
    f.i32(4);
    f.op(Instruction::I32Add);
    f.op(Instruction::LocalTee(nb));
    f.get(need);
    f.get(nb);
    f.op(Instruction::I32GtU);
    f.op(Instruction::Select);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(need);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32GtU);
    f.op(Instruction::Select);
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
    f.push(crate::bulk::copy(HEAP, HEAP));
    f.get(own);
    f.op(Instruction::If(BlockType::Empty));
    // The list's own buffer: its elements move, and the old block is
    // freed.
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    if elem.counts() {
        f.op(Instruction::Else);
        // A shared buffer keeps its elements: the copies take counts.
        f.get(nb);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(n);
        f.helper(Helper::CopyElems { elem: elem.clone() });
    }
    f.op(Instruction::End);
    f.get(b);
    f.helper(Helper::ReleaseKnown);
    f.get(nb);
    f.set(b);
    f.op(Instruction::End);
    f.get(b);
    f
}

/// [`Helper::ListExtend`]: `other` the list's own buffer itself is read
/// from wherever the reserve put it; its length was read first, so the
/// copy is the elements before the call.
fn list_extend(elem: &Elem, m: &Module) -> Func {
    let mut f = m.func("rt:list_extend", vec![Val::I32, Val::I32], vec![Val::I32]);
    let (b, src) = (0, 1);
    let (count, alias, n, at) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(src);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.op(Instruction::LocalTee(count));
    f.op(Instruction::If(BlockType::Empty));
    // A buffer that is the list's own and also `other` moves or stays
    // with the list; a shared one stays where it is for `other`.
    f.get(src);
    f.get(b);
    f.op(Instruction::I32Eq);
    buffer_own(&mut f, b, m);
    f.op(Instruction::I32And);
    f.set(alias);
    reserve_room(&mut f, elem, b, count, m);
    f.get(alias);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.set(src);
    f.op(Instruction::End);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(at);
    f.get(src);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(count);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.push(crate::bulk::copy(HEAP, HEAP));
    if elem.counts() {
        f.get(at);
        f.get(count);
        f.helper(Helper::CopyElems { elem: elem.clone() });
    }
    f.get(b);
    f.get(n);
    f.get(count);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.op(Instruction::End);
    f.get(b);
    f
}

/// Room for local `extra` more elements in the list in local `b`: the
/// inline test of a buffer the list owns alone with room left, else
/// [`Helper::ListReserve`].
fn reserve_room(f: &mut Func, elem: &Elem, b: u32, extra: u32, m: &Module) {
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    unique_count(f, m);
    f.i32(1);
    f.op(Instruction::I32Ne);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.get(extra);
    f.op(Instruction::I32Add);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32GtU);
    f.op(Instruction::I32Or);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.get(extra);
    f.helper(Helper::ListReserve { elem: elem.clone() });
    f.set(b);
    f.op(Instruction::End);
}

/// [`Helper::StrFromBytes`]: takes a count of the list's buffer. An
/// unshared buffer (count 1) becomes the string's: the view
/// is its bytes in place. A shared or pinned one is copied into a string
/// buffer and loses the count. No bytes is the static empty string.
fn str_from_bytes(m: &Module) -> Func {
    let mut f = m.func("rt:str_from_bytes", vec![Val::I32], vec![Val::I32; 2]);
    let (b, n, s) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.op(Instruction::If(BlockType::Empty));
    // A module that counts nothing has no holder count to read.
    if m.runtime.is_some() {
        f.get(b);
        f.u32(HEADER);
        f.op(Instruction::I32Sub);
        unique_count(&mut f, m);
        f.i32(1);
        f.op(Instruction::I32Eq);
        f.op(Instruction::If(BlockType::Empty));
        f.get(b);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(n);
        f.op(Instruction::Return);
        f.op(Instruction::End);
    }
    f.get(n);
    f.helper(Helper::Str);
    f.set(s);
    f.get(s);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.push(crate::bulk::copy(HEAP, HEAP));
    f.op(Instruction::Else);
    f.u32(EMPTY);
    f.set(s);
    f.op(Instruction::End);
    f.get(b);
    f.helper(Helper::ReleaseKnown);
    f.get(s);
    f.get(n);
    f
}

/// [`Helper::StrToBytes`]: the buffer has exactly the string's length,
/// and no bytes is an empty list.
fn str_to_bytes(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:str_to_bytes", vec![Val::I32; 2], vec![Val::I32]);
    let b = f.local(Val::I32);
    f.get(1);
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.set(b);
    f.get(b);
    f.get(1);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(0);
    f.get(1);
    f.push(crate::bulk::copy(HEAP, HEAP));
    f.get(b);
    f
}

/// [`Helper::StrAppend`]: an empty suffix keeps the moved left count. A
/// uniquely held heap buffer with enough bytes after the view grows in place;
/// otherwise the old bytes move to a buffer with room for twice their length.
fn str_append(m: &Module) -> Func {
    let mut f = m.func("rt:str_append", vec![Val::I32; 4], vec![Val::I32; 2]);
    let (left, left_len, right, right_len) = (0, 1, 2, 3);
    let (block, total, capacity, at) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(left_len);
    f.get(right_len);
    f.op(Instruction::I32Add);
    f.set(total);
    f.get(right_len);
    f.op(Instruction::If(BlockType::Empty));
    if let Some(r) = m.runtime {
        f.get(left);
        f.call(Sym::Inst(r.block_of));
        f.set(block);
        f.get(block);
        f.op(Instruction::If(BlockType::Empty));
        f.get(block);
        unique_count(&mut f, m);
        f.i32(1);
        f.op(Instruction::I32Eq);
        f.op(Instruction::If(BlockType::Empty));
        f.get(block);
        f.call(Sym::Inst(r.block_bytes));
        f.get(left);
        f.get(block);
        f.op(Instruction::I32Sub);
        f.op(Instruction::I32Sub);
        f.set(capacity);
        f.get(total);
        f.get(capacity);
        f.op(Instruction::I32LeU);
        f.op(Instruction::If(BlockType::Empty));
        f.get(left);
        f.get(left_len);
        f.op(Instruction::I32Add);
        f.get(right);
        f.get(right_len);
        f.push(crate::bulk::copy(HEAP, HEAP));
        f.get(left);
        f.get(total);
        f.op(Instruction::Return);
        f.op(Instruction::End);
        f.op(Instruction::End);
        f.op(Instruction::End);
    }
    f.get(left_len);
    f.i32(1);
    f.op(Instruction::I32Shl);
    f.set(capacity);
    f.get(total);
    f.get(capacity);
    f.get(total);
    f.get(capacity);
    f.op(Instruction::I32GtU);
    f.op(Instruction::Select);
    f.helper(Helper::Str);
    f.set(at);
    f.get(at);
    f.get(left);
    f.get(left_len);
    f.push(crate::bulk::copy(HEAP, HEAP));
    f.get(at);
    f.get(left_len);
    f.op(Instruction::I32Add);
    f.get(right);
    f.get(right_len);
    f.push(crate::bulk::copy(HEAP, HEAP));
    f.get(left);
    f.helper(Helper::ReleaseRef);
    f.get(at);
    f.get(total);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(left);
    f.get(total);
    f
}

// Representation views retain a source reference, not a snapshot. Their
// getter moves the value out of its place into one erased box, leaving
// zeros there, and their setter moves a box's value in, releasing what the
// place held: a fixed ABI that never copies the value.
fn ref_vals(value: &RefLayout) -> Vec<Val> {
    value.leaves.iter().map(|l| l.val).collect()
}

fn ref_count(f: &mut Func, ls: &[u32], value: &RefLayout, retain: bool) {
    if !retain && !value.drops.is_empty() {
        drop_value(f, ls, value, &value.drops);
        return;
    }
    if let Some((k, addr)) = value
        .walk
        .as_ref()
        .filter(|w| w.known())
        .and_then(|w| Some((w.single()?, w.addr()?)))
    {
        f.get(ls[k as usize]);
        if retain {
            f.helper(Helper::RetainKnown);
        } else {
            release_call(f, addr);
        }
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

/// Releases an inline value through the same local-stack helper as
/// `Em::drop_inline`, including values moved out of representation views.
fn drop_value(f: &mut Func, ls: &[u32], value: &RefLayout, drops: &[DropCall]) {
    for &l in ls {
        f.get(l);
    }
    f.helper(Helper::DropValue {
        size: value.size,
        leaves: value.leaves.clone(),
        walk: value.walk.clone().unwrap_or(Walk::Glue),
        drops: drops.to_vec(),
    });
}

/// Each invocation has its own frame, so nested Drop calls cannot
/// overwrite a parent's payload. The zero header gives references to the
/// frame the same uncounted representation as every other local slot.
fn drop_value_glue(
    m: &Module,
    size: u32,
    leaves: &[Leaf],
    walk: &Walk,
    drops: &[DropCall],
) -> Func {
    let mut f = m.func(
        "rt:drop_value",
        leaves.iter().map(|l| l.val).collect(),
        vec![],
    );
    let base = f.local(Val::I32);
    let at = f.local(Val::I32);
    f.get(base);
    f.op(Instruction::I64Const(0));
    f.op(Instruction::I64Store(mem(HEAP, 0, 3)));
    f.get(base);
    f.u32(crate::module::HEADER);
    f.op(Instruction::I32Add);
    f.set(at);
    for (i, leaf) in leaves.iter().enumerate() {
        f.get(at);
        f.get(u32::try_from(i).expect("leaves"));
        f.op(leaf.store(0));
    }
    call_drops(&mut f, at, drops);
    let ls = load_elem_locals(&mut f, leaves);
    fill_elem(&mut f, at, leaves, &ls);
    walk_code(&mut f, walk, &|f, k| f.get(ls[k as usize]), &release_call);
    crate::wide::locals_frame(
        &mut f,
        base,
        (crate::module::HEADER + size).next_multiple_of(8),
    );
    f
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
            drops: value.drops.to_vec(),
        })
    });
    if let Some(key) = value.weak {
        push_typed_desc_recursive(f, glue, true, false, key);
    } else {
        push_desc(f, glue, true);
    }
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
/// locals `ls` with counts of their own. The value moves out of a box
/// with one holder, which goes without its glue ([`Helper::TakeBox`]);
/// only a shared box leaves a copy, which takes counts of its own.
fn ref_unbox_into(f: &mut Func, at: u32, value: &RefLayout, ls: &[u32]) {
    if !value.boxed {
        f.get(at);
        f.set(ls[0]);
        return;
    }
    if ls.is_empty() {
        return;
    }
    for (leaf, l) in value.leaves.iter().zip(ls) {
        f.get(at);
        f.op(leaf.load(0));
        f.set(*l);
    }
    f.get(at);
    f.helper(Helper::TakeBox);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    ref_copy(f, ls, value);
    f.op(Instruction::End);
}

/// One more count on each counted reference of the value in locals `ls`,
/// loads no count keeps: a collection's is the copy glue's result, which
/// replaces it, a clone when the collection is pinned (the pin's eager copy, until phase 10 of `docs/future-work/POINTER_LOANS.md`).
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

/// How [`ref_read`] reads the value a reference names.
#[derive(Clone, Copy)]
enum Access {
    /// A copy, with counts of its own.
    Copy,
    /// A loan with no count of its own where the place is physical; a
    /// copy through a view, which sets the flag in the local.
    CopyOnView(u32),
    /// A loan with no count of its own, of the leaves the place holds.
    Lend,
}

/// The value that the reference in local `at` names, in new locals with
/// counts of their own: the leaves at the address, or, through a view,
/// the value its getter moves out, copied before its setter moves it back.
/// Emitted inline, so a value of any width stays in locals and never
/// crosses a function's parameters or results.
pub(crate) fn ref_load(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    ref_read(f, at, value, Access::Copy)
}

/// [`ref_load`] as a loan: no count where the place is physical, and a
/// copy through a view, which sets the flag in local `owned`. An erased
/// body reads an open value this way, since the box a view's getter makes
/// over a concrete place goes when its setter moves the value back.
pub(crate) fn ref_load_borrowed(f: &mut Func, at: u32, value: &RefLayout, owned: u32) -> Vec<u32> {
    ref_read(f, at, value, Access::CopyOnView(owned))
}

/// The leaves of the concrete value that the reference in local `at`
/// names, lent with no count of their own: through a view, its getter
/// moves the value out and its setter moves it back, so the leaves are
/// the place's own. Never a copy, so it reads a value that may not copy.
pub(crate) fn ref_lend(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    ref_read(f, at, value, Access::Lend)
}

/// [`ref_lend`] where local `at` is known to hold a view.
pub(crate) fn view_lend(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    f.stack_values |= value.leaves.len() > crate::wide::WIDTH;
    let ls: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
    if !ls.is_empty() {
        view_read(f, at, value, Access::Lend, &ls);
    }
    ls
}

/// A read through the view in local `at` into locals `ls`: the getter
/// moves the value out, the leaves are read from its box, copied unless
/// `access` lends them, and the setter moves the value back.
fn view_read(f: &mut Func, at: u32, value: &RefLayout, access: Access, ls: &[u32]) {
    let boxed = view_get(f, at);
    load_boxed(f, boxed, value, ls);
    if !matches!(access, Access::Lend) {
        ref_copy(f, ls, value);
    }
    if let Access::CopyOnView(flag) = access {
        f.i32(1);
        f.set(flag);
    }
    view_put(f, at, boxed);
}

fn ref_read(f: &mut Func, at: u32, value: &RefLayout, access: Access) -> Vec<u32> {
    f.stack_values |= value.leaves.len() > crate::wide::WIDTH;
    if let Access::CopyOnView(flag) = access {
        f.zero(flag);
    }
    let ls: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
    if ls.is_empty() {
        return ls;
    }
    f.get(at);
    f.helper(Helper::RefIsView);
    f.op(Instruction::If(BlockType::Empty));
    view_read(f, at, value, access, &ls);
    f.op(Instruction::Else);
    for (leaf, l) in value.leaves.iter().zip(&ls) {
        f.get(at);
        f.op(leaf.load(0));
        f.set(*l);
    }
    if matches!(access, Access::Copy) {
        ref_copy(f, &ls, value);
    }
    f.op(Instruction::End);
    ls
}

/// Moves the value that the reference in local `at` names out of its
/// place into new locals, which take its counts, and leaves zeros there:
/// through a view, the value its getter moves out. Its setter, or
/// [`ref_store`], puts a value back, releasing the zeros.
pub(crate) fn ref_take(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    f.stack_values |= value.leaves.len() > crate::wide::WIDTH;
    let ls: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
    if ls.is_empty() {
        return ls;
    }
    f.get(at);
    f.helper(Helper::RefIsView);
    f.op(Instruction::If(BlockType::Empty));
    let boxed = view_get(f, at);
    ref_unbox_into(f, boxed, value, &ls);
    f.op(Instruction::Else);
    for (leaf, l) in value.leaves.iter().zip(&ls) {
        f.get(at);
        f.op(leaf.load(0));
        f.set(*l);
        f.get(at);
        f.op(match leaf.val {
            Val::I32 => Instruction::I32Const(0),
            Val::I64 => Instruction::I64Const(0),
            Val::F32 => Instruction::F32Const(0.0.into()),
            Val::F64 => Instruction::F64Const(0.0.into()),
        });
        f.op(leaf.store(0));
    }
    f.op(Instruction::End);
    ls
}

/// Calls the getter of the view in local `at`, which moves its value out
/// of its place into an erased box: a new local holding the box.
fn view_get(f: &mut Func, at: u32) -> u32 {
    let boxed = f.local(Val::I32);
    f.get(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.call_indirect(vec![Val::I32], vec![Val::I32]);
    f.set(boxed);
    boxed
}

/// Calls the setter of the view in local `at` with the erased box in
/// local `boxed`, which moves the box's value into the view's place.
fn view_put(f: &mut Func, at: u32, boxed: u32) {
    f.get(at);
    f.get(boxed);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.call_indirect(vec![Val::I32; 2], vec![]);
}

/// The leaves of the erased box in local `boxed` into locals `ls`, with
/// no count: the box keeps the value.
fn load_boxed(f: &mut Func, boxed: u32, value: &RefLayout, ls: &[u32]) {
    if !value.boxed {
        f.get(boxed);
        f.set(ls[0]);
        return;
    }
    for (leaf, l) in value.leaves.iter().zip(ls) {
        f.get(boxed);
        f.op(leaf.load(0));
        f.set(*l);
    }
}

/// Stores the value in locals `ls` through the reference in local `at`: a
/// view's setter takes a new box of it, an address its leaves, releasing
/// the value it held. An `owned` value's counts move into the place, so
/// its locals keep none; any other keeps its own, and the place takes a
/// count of its own. Emitted inline, as [`ref_load`] is.
pub(crate) fn ref_store(
    f: &mut Func,
    at: u32,
    ls: &[u32],
    value: &RefLayout,
    owned: bool,
    release: bool,
) {
    if ls.is_empty() {
        return;
    }
    if !owned {
        ref_count(f, ls, value, true);
    }
    f.get(at);
    f.helper(Helper::RefIsView);
    f.op(Instruction::If(BlockType::Empty));
    let boxed = f.local(Val::I32);
    ref_box(f, ls, value);
    f.set(boxed);
    view_put(f, at, boxed);
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
    if release {
        let was = f.reason.replace(hd_tir::own::Reason::Overwritten);
        ref_count(f, &old, value, false);
        f.reason = was;
    }
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

fn leaf_count(value: &RefLayout) -> u32 {
    u32::try_from(value.leaves.len()).expect("leaves")
}

/// The getter of a view of a part of its source's value, the leaves
/// `picks` of the `parent` value: takes the parent out of its place,
/// moves the part out of it into a box, and puts the rest back.
fn part_get(f: &mut Func, parent: &RefLayout, field: &RefLayout, picks: &[u32]) {
    let src = view_source(f);
    let ls = ref_take(f, src, parent);
    let part: Vec<u32> = picks
        .iter()
        .map(|&p| {
            let x = f.local(field_val(parent, p));
            f.get(ls[p as usize]);
            f.set(x);
            f.zero(ls[p as usize]);
            x
        })
        .collect();
    ref_store(f, src, &ls, parent, true, true);
    ref_box(f, &part, field);
}

/// The setter of a view of a part of its source's value, the leaves
/// `picks` of the `parent` value: moves the new part out of its box,
/// takes the parent out of its place, puts it back with the new part,
/// and releases the old one.
fn part_set(f: &mut Func, parent: &RefLayout, field: &RefLayout, picks: &[u32]) {
    let part = ref_unbox(f, 1, field);
    let src = view_source(f);
    let ls = ref_take(f, src, parent);
    let old: Vec<u32> = picks
        .iter()
        .zip(&part)
        .map(|(&p, &new)| {
            let x = f.local(field_val(parent, p));
            f.get(ls[p as usize]);
            f.set(x);
            f.get(new);
            f.set(ls[p as usize]);
            x
        })
        .collect();
    ref_store(f, src, &ls, parent, true, true);
    ref_count(f, &old, field, false);
}

fn field_val(parent: &RefLayout, p: u32) -> Val {
    parent.leaves[p as usize].val
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
            glue_of(&mut f, m);
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
            let picks: Vec<u32> = (first..first + leaf_count(field)).collect();
            part_get(&mut f, parent, field, &picks);
            f
        }
        Helper::RefFieldSet { .. } => {
            let mut f = m.func("rt:ref_field_set", vec![Val::I32; 2], vec![]);
            let picks: Vec<u32> = (first..first + leaf_count(field)).collect();
            part_set(&mut f, parent, field, &picks);
            f
        }
        _ => unreachable!("a field reference helper"),
    }
}

/// [`Helper::RefPayload`] and its getter and setter: a view whose source
/// is the reference to the enum. The getter and setter move the leaves
/// `picks` names out of the enum and back in ([`part_get`], [`part_set`]).
fn ref_payload_helper(h: &Helper, m: &Module) -> Func {
    match h {
        Helper::RefPayload {
            parent,
            field,
            picks,
            offset,
        } => {
            let mut f = m.func("rt:ref_payload", vec![Val::I32], vec![Val::I32]);
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
                    Helper::RefPayloadGet {
                        parent: parent.clone(),
                        field: field.clone(),
                        picks: picks.clone(),
                    },
                ),
                (
                    8,
                    Helper::RefPayloadSet {
                        parent: parent.clone(),
                        field: field.clone(),
                        picks: picks.clone(),
                    },
                ),
            ] {
                f.get(at);
                f.table_slot(Sym::Helper(helper));
                f.op(Instruction::I32Store(mem(HEAP, slot, 2)));
            }
            f.get(at);
            f.u32(*offset);
            f.op(Instruction::I32Store(mem(HEAP, 12, 2)));
            f.get(at);
            f.i32(0);
            f.op(Instruction::I32Store(mem(HEAP, 16, 2)));
            f.get(at);
            f
        }
        Helper::RefPayloadGet {
            parent,
            field,
            picks,
        } => {
            let mut f = m.func("rt:ref_payload_get", vec![Val::I32], vec![Val::I32]);
            part_get(&mut f, parent, field, picks);
            f
        }
        Helper::RefPayloadSet {
            parent,
            field,
            picks,
        } => {
            let mut f = m.func("rt:ref_payload_set", vec![Val::I32; 2], vec![]);
            part_set(&mut f, parent, field, picks);
            f
        }
        _ => unreachable!("a payload reference helper"),
    }
}

/// Gives this allocation its checked descriptor kind. No instruction is
/// added in unchecked builds; the linker aliases the same release glue.
#[cfg(feature = "checked-report")]
pub(crate) fn checked_kind(f: &mut Func, m: &Module, kind: u8) {
    if m.runtime.is_none_or(|r| r.checked.is_none()) {
        return;
    }
    if let Some(i) = f.body.windows(3).rposition(|w| matches!(w, [Ins::TableSlot(_) | Ins::Descriptor(_, _), Ins::Op(Instruction::I32Const(c)), Ins::Op(Instruction::I32Add)] if c.cast_unsigned() == 1 || c.cast_unsigned() == CYCLIC + 1)) {
        match f.body[i] {
            Ins::TableSlot(s) => {
                f.body[i] = Ins::Descriptor(s, DescriptorKey::checked(kind));
            }
            Ins::Descriptor(s, mut key) => {
                key.kind = kind;
                f.body[i] = Ins::Descriptor(s, key);
            }
            _ => unreachable!(),
        }
    }
}

/// Grows the raw table whose address and capacity globals are `globals`
/// to `cap` (a local) entries of `stride` bytes: a zeroed block that takes
/// the old entries, whose block is freed.
pub(crate) fn grow_table(m: &Module, f: &mut Func, [tab, len]: [Global; 2], cap: u32, stride: i32) {
    let fresh = f.local(Val::I32);
    f.get(cap);
    f.i32(stride);
    f.op(Instruction::I32Mul);
    match m.runtime {
        Some(r) => f.call(Sym::Inst(r.alloc)),
        None => f.helper(Helper::Alloc),
    }
    f.set(fresh);
    f.get(fresh);
    f.i32(0);
    f.get(cap);
    f.i32(stride);
    f.op(Instruction::I32Mul);
    f.push(crate::bulk::fill(HEAP));
    f.global_get(tab);
    f.op(Instruction::If(BlockType::Empty));
    f.get(fresh);
    f.global_get(tab);
    f.global_get(len);
    f.i32(stride);
    f.op(Instruction::I32Mul);
    f.push(crate::bulk::copy(HEAP, HEAP));
    if let Some(r) = m.runtime {
        f.global_get(tab);
        f.call(Sym::Inst(r.acyclic.free));
    }
    f.op(Instruction::End);
    f.get(fresh);
    f.global_set(tab);
    f.get(cap);
    f.global_set(len);
}

/// [`Helper::LoanShare`] and [`Helper::LoanUnshare`]: a shared loan's
/// begin or end on the object whose descriptor word is at address
/// parameter 0, at a count of 31 (model A,
/// `docs/future-work/POINTER_LOANS.md` E.2). A begin adds to the object's
/// entry in the spill map instead, leaving the count at 31 so the inline
/// checks still see a shared loan; an end takes from the entry first. The
/// map is an open-addressed table of `(address, count)` pairs, linearly
/// probed, in a raw block it allocates on first use, doubles at three
/// quarters full and frees when its last entry goes (`SpillTab`,
/// `SpillCap` entries, `SpillLen` used), so only memory limits it.
fn loan_share(m: &Module, begin: bool) -> Func {
    let name = if begin {
        "rt:loan_share"
    } else {
        "rt:loan_unshare"
    };
    let mut f = m.func(name, vec![Val::I32], vec![]);
    let (s, i, e) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(s);
    // Below 31 (an end whose object has no entry too): the count itself.
    let plain = |f: &mut Func| {
        f.get(0);
        f.get(s);
        f.u32(LOAN_SHARED);
        f.op(if begin {
            Instruction::I32Add
        } else {
            Instruction::I32Sub
        });
        f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    };
    f.get(s);
    f.u32(LOAN_STATE - LOAN_EXCL);
    f.op(Instruction::I32And);
    f.u32(LOAN_STATE - LOAN_EXCL);
    f.op(Instruction::I32Ne);
    if !begin {
        f.global_get(Global::SpillTab);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::I32Or);
    }
    f.op(Instruction::If(BlockType::Empty));
    plain(&mut f);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    if begin {
        // Room for one more at three quarters full.
        f.global_get(Global::SpillLen);
        f.i32(1);
        f.op(Instruction::I32Add);
        f.i32(4);
        f.op(Instruction::I32Mul);
        f.global_get(Global::SpillCap);
        f.i32(3);
        f.op(Instruction::I32Mul);
        f.op(Instruction::I32GtU);
        f.op(Instruction::If(BlockType::Empty));
        spill_grow(&mut f, m);
        f.op(Instruction::End);
    }
    // The object's entry, or the empty slot that ends its probe.
    spill_find(&mut f, 0, i, e);
    f.get(e);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    if begin {
        f.get(e);
        f.get(0);
        f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
        f.get(e);
        f.i32(1);
        f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
        f.global_get(Global::SpillLen);
        f.i32(1);
        f.op(Instruction::I32Add);
        f.global_set(Global::SpillLen);
    } else {
        plain(&mut f);
    }
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(e);
    f.get(e);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.i32(if begin { 1 } else { -1 });
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    if !begin {
        f.get(e);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        spill_remove(&mut f, m, i, e);
        f.op(Instruction::End);
    }
    f
}

/// Pushes the home slot of address `key`'s entry in the spill map.
fn spill_home(f: &mut Func, key: u32) {
    f.get(key);
    f.i32(4);
    f.op(Instruction::I32ShrU);
    f.get(key);
    f.i32(12);
    f.op(Instruction::I32ShrU);
    f.op(Instruction::I32Xor);
    f.global_get(Global::SpillCap);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32And);
}

/// Sets `i` and `e` to the slot and address of `key`'s entry in the spill
/// map, or of the empty slot where its probe ends.
fn spill_find(f: &mut Func, key: u32, i: u32, e: u32) {
    spill_home(f, key);
    f.set(i);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.global_get(Global::SpillTab);
    f.get(i);
    f.i32(8);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.set(e);
    f.get(e);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Eqz);
    f.get(e);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(key);
    f.op(Instruction::I32Eq);
    f.op(Instruction::I32Or);
    f.op(Instruction::BrIf(1));
    f.get(i);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.global_get(Global::SpillCap);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32And);
    f.set(i);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// Stores the entry `(key, count)`, whose key the map lacks, at the empty
/// slot its probe ends at.
fn spill_put(f: &mut Func, key: u32, count: u32) {
    let (i, e) = (f.local(Val::I32), f.local(Val::I32));
    spill_find(f, key, i, e);
    f.get(e);
    f.get(key);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(e);
    f.get(count);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
}

/// Doubles the spill map (16 entries at first), moving its entries.
fn spill_grow(f: &mut Func, m: &Module) {
    let (old, cap, at, key, count) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.global_get(Global::SpillTab);
    f.set(old);
    f.global_get(Global::SpillCap);
    f.set(cap);
    f.get(cap);
    f.i32(2);
    f.op(Instruction::I32Mul);
    f.i32(16);
    f.get(cap);
    f.op(Instruction::Select);
    f.global_set(Global::SpillCap);
    f.global_get(Global::SpillCap);
    f.i32(8);
    f.op(Instruction::I32Mul);
    spill_alloc(f, m);
    f.global_set(Global::SpillTab);
    f.global_get(Global::SpillTab);
    f.i32(0);
    f.global_get(Global::SpillCap);
    f.i32(8);
    f.op(Instruction::I32Mul);
    f.push(crate::bulk::fill(HEAP));
    f.get(old);
    f.op(Instruction::If(BlockType::Empty));
    f.get(old);
    f.set(at);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.get(old);
    f.get(cap);
    f.i32(8);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Eq);
    f.op(Instruction::BrIf(1));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(key);
    f.get(key);
    f.op(Instruction::If(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(count);
    spill_put(f, key, count);
    f.op(Instruction::End);
    f.get(at);
    f.i32(8);
    f.op(Instruction::I32Add);
    f.set(at);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(old);
    spill_free(f, m);
    f.op(Instruction::End);
}

/// Empties slot `i` at address `e` of the spill map and moves back the
/// entries of its probe run after it, so no probe stops short; frees the
/// map when it holds no entry.
fn spill_remove(f: &mut Func, m: &Module, i: u32, e: u32) {
    let (key, count) = (f.local(Val::I32), f.local(Val::I32));
    f.get(e);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.global_get(Global::SpillLen);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.global_set(Global::SpillLen);
    f.global_get(Global::SpillLen);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.global_get(Global::SpillTab);
    spill_free(f, m);
    f.i32(0);
    f.global_set(Global::SpillTab);
    f.i32(0);
    f.global_set(Global::SpillCap);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    // Each later entry of the run, taken out and put back.
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.global_get(Global::SpillCap);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32And);
    f.set(i);
    f.global_get(Global::SpillTab);
    f.get(i);
    f.i32(8);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.set(e);
    f.get(e);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(key);
    f.get(key);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(e);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(count);
    f.get(e);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    spill_put(f, key, count);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// Allocates a raw block of the size on the stack, leaving its address.
fn spill_alloc(f: &mut Func, m: &Module) {
    match m.runtime {
        Some(r) => f.call(Sym::Inst(r.alloc)),
        None => f.helper(Helper::Alloc),
    }
}

/// Frees the raw block whose address is on the stack: the free that skips
/// the collector's candidate test.
fn spill_free(f: &mut Func, m: &Module) {
    match m.runtime {
        Some(r) => f.call(Sym::Inst(r.acyclic.free)),
        None => f.op(Instruction::Drop),
    }
}
