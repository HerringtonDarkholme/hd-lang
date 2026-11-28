//! Runtime pieces the compiler generates (`crates/hd_wasm/DESIGN.md`
//! §23.9): entry exports, panics, exchange-buffer copies, the bump-allocation
//! fallback, integer text and host provider stubs. Each is a [`Helper`],
//! built once on its first request.

use crate::op::Op as Instruction;
use hd_base::{DefId, Hash128, HashSet, StageResult};
use hd_types::AdtId;
use hd_types::{Ty, TyList};
use wasm_encoder::BlockType;

use crate::layout::{Addr, Leaf, LeafOps as _};
use crate::map::{self, KeyOps};
#[cfg(feature = "checked-report")]
use crate::module::DescriptorKey;
use crate::module::{
    BUF, CYCLIC, EMPTY, Func, GLUE, Global, HEADER, HEAP, Import, Ins, LOAN_EXCL, LOAN_SHARED,
    LOAN_STATE, Module, Sym, Val, WasmVal as _, mem,
};
use crate::{Entry, Input};

pub(crate) mod row;
pub use row::{RowAdapter, RowEntry};
/// A test program's unit graph in static memory ([`test_graph`]), read
/// by [`Helper::TestInitWalk`]. `at` holds `u32` words: the group offsets
/// (`groups + 1` words from word 0), each group's root units, the unit
/// offsets (`units + 1` words from word `unit_offsets`), and each unit's
/// direct dependencies; every offset is a word index into `at`.
/// `scratch` is zeroed and written: the stack, the reached units, then a
/// mark byte per unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TestGraph {
    pub at: u32,
    pub unit_offsets: u32,
    pub units: u32,
    pub scratch: u32,
}

/// Places a test program's unit graph `words` ([`Entry::Tests`]) in
/// static data, with scratch for its units: a [`TestGraph`]. Static words
/// linear in the groups, units and edges, and scratch linear in the
/// units.
fn test_graph(m: &mut Module, words: &[u32], unit_offsets: u32) -> TestGraph {
    // The first unit offset points just past the `units + 1` offsets.
    let units = words
        .get(unit_offsets as usize)
        .map_or(0, |first| first - unit_offsets - 1);
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let at = m.static_bytes(&bytes, 4);
    let scratch = m.static_slot(units * 9, 4);
    TestGraph {
        at,
        unit_offsets,
        units,
        scratch,
    }
}

/// The canonical descriptor behind a `TypeId` (`emit/dyn_value.rs`):
/// its identity key and printable name, both in static storage.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeIdParts {
    pub name: String,
    pub shown: String,
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

/// The leaves at a reference's address and their copy/release operations.
/// Direct loads and stores keep those leaves in locals; dynamic value
/// receivers and handed-in arguments use the same counts and Drop hooks.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefLayout {
    pub size: u32,
    pub leaves: Vec<Leaf>,
    /// How the value counts: its type's shims, which run its `Drop` hooks
    /// first (`hd_layout::shims`).
    pub counted: Counted,
}

/// A generated function, by what it does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    MemoryCopy {
        source: u32,
        dest: u32,
    },
    MemoryFill(u32),
    /// A link-time fact: some reachable operation can cancel a task.
    CanCancel,
    /// Marks an explicit cancellation operation; erased before optimization.
    CancellationRoot,
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
    /// block is left.
    CheckedPoll {
        poll: Box<Helper>,
    },
    /// `(ctx) -> status`: `poll`'s answer, after a cycle collection
    /// (§23.28): the entry is idle when the answer is Pending, and done
    /// otherwise. Checked mode collects in [`Helper::CheckedPoll`] instead.
    CollectPoll {
        poll: Box<Helper>,
    },
    /// `hd.test(case, ctx) -> status`: a test program's one case entry.
    /// One `br_table` (Binaryen's and LLVM's switch lowering: a block per
    /// arm) picks `polls[case]`, an [`Helper::EntryPoll`] written in its
    /// arm, or called when it suspends; then [`Helper::CheckedPoll`]'s
    /// end, or [`Helper::CollectPoll`]'s. A checked case's instance may
    /// serve the next body (a property's), so that end then runs
    /// `reinit`, every module's init again, which gives it the bindings a
    /// fresh instance has. A case number past the arms traps. An instance's exports and functions, which
    /// V8 builds per instantiate, then grow by one function per case: its
    /// body (#1902). Over [`DISPATCH_ARMS`] cases the arms call
    /// [`Helper::TestPollPart`]s instead, a tree of bounded functions.
    TestPoll {
        polls: Vec<Helper>,
        reinit: Vec<Hash128>,
    },
    /// `(unit)`: a test program's unit dispatch, which runs the inits
    /// `inits[unit]` as [`Helper::EntryInit`] runs them, in arms as
    /// [`Helper::TestPoll`] has; [`Helper::TestInitWalk`] calls it.
    TestInit {
        inits: Vec<Vec<(Hash128, bool)>>,
    },
    /// `(case, ctx) -> status`: the cases `polls` of a [`Helper::TestPoll`]
    /// too many for one function ([`DISPATCH_ARMS`]), dispatched the same
    /// way by `case` relative to the first, with no end of their own.
    TestPollPart {
        polls: Vec<Helper>,
    },
    /// `hd.test.init(group)`: marks the units `graph` says the group
    /// reaches, by an explicit stack in static memory, sorts their numbers
    /// in place (heapsort) and runs each through [`Helper::TestInit`] over
    /// `inits`, one list per unit. A call costs `r log r` for the `r`
    /// units it reaches, and no call nests: a chain of modules is a loop,
    /// not a recursion.
    TestInitWalk {
        inits: Vec<Vec<(Hash128, bool)>>,
        graph: TestGraph,
    },
    /// `(unit)`: the units `inits` of a [`Helper::TestInit`] too many
    /// for one function, as [`Helper::TestPollPart`] holds cases.
    TestInitPart {
        inits: Vec<Vec<(Hash128, bool)>>,
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
    /// descriptor word, the table slot of its release glue plus
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
    /// `(frame)`: frees the frame at payload start `frame`, which its
    /// invocation made and still holds alone, at a Ready exit (#1707): one
    /// count, never shared, never a cycle candidate, and its release glue
    /// would release nothing, so it takes no count, no drain and no glue.
    FreeFrame,
    /// `(len) -> address`: a new string buffer (§23.7) of `len` bytes, a
    /// counted object with nothing to release in it.
    Str,
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
    /// The last count of a buffer this body allocated and nothing else
    /// holds (`drop_word::FRESH`): straight to the free path.
    ReleaseFresh,
    /// `(coll)`: one more count on a list's or a map's buffer
    /// (`types.coll.value`), out of line: every copy of a collection
    /// shares this one call, which tests null and static data inside,
    /// where another known retain takes the inline test (about 36 bytes a
    /// site, `count_tests`).
    RetainColl,
    /// `(payload)`: one more count on each part of a new object's
    /// payload, laid out as `leaves`, that its construction copied in:
    /// each part's first leaf and how its type counts.
    RetainFields {
        leaves: Vec<Leaf>,
        parts: Vec<(u32, Counted)>,
    },
    /// `(values)`: a value in locals whose release runs a hook in place:
    /// stores it in a frame slot laid out as `leaves`, `size` bytes, in
    /// heap form, then calls its in-place release shim `shim` there.
    ReleaseSpilled {
        size: u32,
        leaves: Vec<Leaf>,
        shim: Hash128,
    },
    /// `(payload)`: the release glue of an object whose payload is laid
    /// out as `leaves`: one count less on each part, by its first leaf
    /// and how its type counts. A part whose type holds a `std.rt.Drop`
    /// hook runs it in its release shim. The cycle collector's drain
    /// reruns this same glue.
    DropParts {
        leaves: Vec<Leaf>,
        parts: Vec<(u32, Counted)>,
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
    /// `(ctx, key) -> entry`: the entry of key id `key` in context `ctx`
    /// ([`ctx_elem`]), `LIST_ELEMS` before it. Checking covers every key a
    /// body looks up, so a miss is internal and traps.
    CtxFind,
    /// `(n, base) -> ctx`: a new context of `n` entries, which the caller
    /// fills, then the entries of context `base` (0 for none), which take
    /// counts of their own.
    CtxNew,
    /// A checked function-row evidence wrapper, using the function-value ABI.
    RowAdapter(RowAdapter),
    /// `(at, n)`: the `n` elements at `at`, just copied from a buffer that
    /// keeps them too, take their own counts: a collection's is one more
    /// count on its buffer (`types.coll.value`).
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
    /// `() -> u64`: the next deterministic per-map seed for this run.
    MapSeed,
    /// `(capacity, seed) -> table`: a new, empty map table (`map.rs`) with room
    /// for `capacity` entries laid out as `entry`, its index cleared.
    MapTable {
        entry: map::Entry,
    },
    /// `(payload)`: the release glue of a map table: one count less on
    /// each counted reference of each live entry.
    DropEntries {
        entry: map::Entry,
    },
    /// `(key..., seed) -> i32`: a key's bucket hash, made non-negative.
    MapHash {
        key: Vec<Val>,
        ops: KeyOps,
    },
    /// `(table, hash, key) -> entry`: the number of the key's entry, or -1.
    MapFind {
        entry: map::Entry,
        ops: KeyOps,
    },
    /// `(table, unused, key...) -> entry`: builtin lookup; with `probe`,
    /// computes the original hashed probe and additionally returns its hash.
    MapLookup {
        entry: map::Entry,
        ops: KeyOps,
        probe: bool,
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
    /// table it shares is copied, its entries taking their own counts.
    /// The reserved word stays 0.
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
    /// it after the call, including its Drop hooks. With `body`, it calls
    /// `target`'s body on its fresh path instead of its cold constructor,
    /// for a target that completes at once (#2419): `results` are then the
    /// method's, and the body's frame, always 0, is dropped.
    DynSlot {
        target: Hash128,
        body: bool,
        retained_result: Counted,
        leaves: Option<Vec<Leaf>>,
        params: Vec<Val>,
        results: Vec<Val>,
        own: Vec<(u32, Counted)>,
        given: Vec<(u32, RefLayout)>,
        ctx: Option<SlotCtx>,
    },
    /// `(ref, params..) -> results`: a vtable slot whose payload is a
    /// reference and whose target takes the value: loads its leaves from
    /// that address, calls `target`, then releases the value read.
    DynRead {
        target: Hash128,
        retained_result: Counted,
        read: RefRead,
        params: Vec<Val>,
        results: Vec<Val>,
        own: Vec<(u32, Counted)>,
        given: Vec<(u32, RefLayout)>,
        ctx: Option<SlotCtx>,
    },
    /// `(env, at, ctx) -> (usize, (K, V))?`: the step closure code of
    /// std's `map_steps` (`map.rs`).
    MapStep {
        entry: map::Entry,
        item: Box<crate::layout::Flat>,
    },
    /// `(list, n) -> list`: std's `list_truncate`: make the buffer the
    /// list's own, release the elements from `n` on and set the length to
    /// `n`. The reserved word stays 0.
    ListTruncate {
        elem: Elem,
    },
    /// `(list, extra) -> list`: std's `list_reserve`, and the reserve
    /// of [`Helper::ListExtend`]: room for
    /// `extra` (more than 0, which each caller tests) more elements
    /// without another growth: a shared buffer is copied, its elements
    /// taking their own counts; a buffer without the room moves to one of the needed length or twice the
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
    /// `(at, len, from) -> index` for a constant set, else `(at, len, from,
    /// w0, w1, w2, w3) -> index`: std's `bytes_index_set`, the first byte at
    /// or after `from` in the 256-bit set, or `len` (#428).
    BytesIndexSet(Option<crate::scan::ByteSet>),
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
    /// shorter length (#428). [`Helper::StrEq`] calls it.
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
        counts: Box<HostCounts>,
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
    /// a frame; its terminal callback releases transferred argument counts.
    HostDrive {
        blocking: bool,
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        counts: Box<HostCounts>,
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
        owned: Option<Elem>,
    },
    /// `(data, params, ctx?) -> results`: a host handle's vtable slot,
    /// which calls `inner`, a host call or a cold leaf, with the handle
    /// number from the box at `data` first (`boundary.rs`). With `ctx`, it
    /// takes the context its method's slot shape has last, and ignores it.
    HandleSlot {
        inner: Box<Helper>,
        params: Vec<Val>,
        ctx: bool,
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
    /// (frame, scalar-base, own, handoff) -> (frame, own).
    FrameEnsure(Hash128),
    /// `(frame) -> i32`: a poll's runtime checks; 1 answers Pending.
    PollCheck,
    /// `(frame) -> i32`: a cancel's runtime checks; 0 leaves the frame.
    CancelCheck,
    /// `() -> i32`: starts a cancel's `ABANDONED` carry.
    CancelEnter,
    /// `(frame, seen)`: ends it.
    CancelLeave,
    /// `(frame) -> i32`: a point's cancellation of the child its frame
    /// holds, bracketed as a cancel; 1 when the child is left unwinding.
    CancelChild,
    /// `(n)`: `hd.wake(n)`, the completed handles into the wake table.
    WakeMark,
    /// `(h) -> i32`: whether handle `h` completed, which it takes.
    WakeTake,
    /// `(handle) -> address`: grows the 16-byte wake registrations.
    WakeSlot,
    /// `(handle)`: registers the current waker, or removes it.
    WakeRegister,
    WakeRemove,
    /// `(node, bit)`: marks the child and each enclosing join!.
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
    /// `(code, env)`: installs std's idle hook (`task.rs`, std's
    /// `ManualClock`).
    IdleHook,
    /// `()`: runs the idle hook, if one is installed; the drivers call it
    /// where they would wait for the host.
    IdleWake,
    /// `hd.thread(block, stopped)`, the export a thread's instance runs
    /// (`module.rt.thread.spawn`); see [`thread_start`].
    ThreadStart,
    /// `() -> completions`: `block_on`'s wait: [`Helper::IdleWake`] first,
    /// and no host completion when the hook readied something, else the
    /// host's `block`.
    IdleBlock,
    /// `race!`'s frame over a list of tasks of results `results`: part 0
    /// `(list) -> frame`, 1 its poll, 2 its cancel.
    Race {
        part: u8,
        results: Vec<Val>,
        counted: Counted,
    },
    /// `(payload)`: a race frame's glue.
    RaceDrop {
        results: Vec<Val>,
        counted: Counted,
    },
    /// A flat homogeneous all frame: constructor, poll, cancel, release
    /// glue, ready-tree traversal, and first-poll tree initialization.
    All {
        part: u8,
        elem: Elem,
    },
}

/// A list's element as the list glue sees it: its stride, its values in
/// memory, how they count, and whether the list's header and buffers take
/// part in cycle collection (`crate::cycle`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Elem {
    pub size: u32,
    pub leaves: Vec<Leaf>,
    /// How an element counts: its type's shims (`hd_layout::shims`).
    pub counted: Counted,
    /// A host argument area's arguments, each by its first leaf and how
    /// its type counts, hooks in its shims; none for a list's element.
    pub parts: Vec<(u32, Counted)>,
    pub cyclic: bool,
}

/// Count ownership of a host slot's argument record. Value indices refer
/// to the shared area layout, so the slot stores that layout only once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostCounts {
    pub borrowed: Vec<(u32, Counted)>,
    pub owned: Vec<(u32, Counted)>,
}

/// How a value of a layout record's type counts (`hd_layout::shims`):
/// nothing, one address among its values, a collection's buffer, an
/// optional's one address, null when it holds nothing, or its retain and
/// release shims (their keys, the value's width, and whether the release
/// runs a `Drop` hook).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Counted {
    #[default]
    None,
    Address {
        index: u32,
        known: bool,
    },
    /// A list's or a map's buffer, or a struct's whose one counted value
    /// is one (`hd_mono::holds_coll`): a known address, retained out of
    /// line ([`Helper::RetainColl`]).
    Coll {
        index: u32,
    },
    /// A sum whose one counted address says its variant: null for the
    /// empty one. It counts only when not null, as the variant's test
    /// would, so an empty optional makes no count call.
    Optional {
        index: u32,
        known: bool,
    },
    Shims(Box<ShimSyms>),
}

/// A type's retain and release shim keys (`Module::shim_key`), the width
/// of its values, and whether its release runs a `Drop` hook.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShimSyms {
    pub retain: Hash128,
    pub release: Hash128,
    /// The by-value forms of a struct or tuple kept by address, for a
    /// value in Wasm locals (the in-place forms' own keys otherwise).
    pub retain_value: Hash128,
    pub release_value: Hash128,
    pub width: u32,
    pub hooked: bool,
    /// Which shims take the value by address and count it where it lives
    /// (`hd_layout::shims::in_place`): the release, the retain; with the
    /// value's layout, its leaves and size, for the address of its first
    /// leaf and for the frame slot a hooked release in locals spills to.
    pub release_at: bool,
    pub retain_at: bool,
    pub in_place: Option<Box<(u32, Vec<Leaf>)>>,
    /// A struct or tuple of at most [`GLUE_ADDRESSES`] counted addresses,
    /// with no hook and no switch: each one's count, which runtime glue
    /// states inline ([`Counted::emit_glue`]).
    pub small: Option<Box<[Counted]>>,
}

impl Counted {
    /// Whether a value of the type counts anything.
    #[must_use]
    pub fn counts(&self) -> bool {
        *self != Counted::None
    }

    /// One count more (`retain`) or less on the value whose `k`th value
    /// `get` pushes: the address's count call, or a call of the shim.
    pub(crate) fn emit(
        &self,
        f: &mut Func,
        m: &Module,
        get: &dyn Fn(&mut Func, u32),
        retain: bool,
    ) {
        match *self {
            Counted::None => {}
            Counted::Coll { index } if retain => {
                get(f, index);
                f.helper(Helper::RetainColl);
            }
            Counted::Coll { index } => {
                get(f, index);
                release_call(f, Addr::Known);
            }
            Counted::Address { index, known } => {
                get(f, index);
                let addr = if known { Addr::Known } else { Addr::Ref };
                if retain {
                    retain_call(m)(f, addr);
                } else {
                    release_call(f, addr);
                }
            }
            Counted::Optional { index, known } => {
                get(f, index);
                f.op(Instruction::If(BlockType::Empty));
                Counted::Address { index, known }.emit(f, m, get, retain);
                f.op(Instruction::End);
            }
            Counted::Shims(ref s) => {
                let ShimSyms {
                    retain_value,
                    release,
                    release_value,
                    width,
                    hooked,
                    release_at,
                    ref in_place,
                    ..
                } = **s;
                for k in 0..width {
                    get(f, k);
                }
                match in_place {
                    // Only a release that runs a hook spills: the hook runs
                    // on the value where it lives.
                    Some(layout) if !retain && release_at && hooked => {
                        f.helper(Helper::ReleaseSpilled {
                            size: layout.0,
                            leaves: layout.1.clone(),
                            shim: release,
                        });
                    }
                    _ if retain => f.call(Sym::Shim(retain_value, true, width, false)),
                    _ => f.call(Sym::Shim(release_value, false, width, hooked)),
                }
            }
        }
    }

    /// [`Counted::emit`] in runtime glue, which runs once per object or
    /// element: a small struct's counts inline, with no call of its shim.
    pub(crate) fn emit_glue(
        &self,
        f: &mut Func,
        m: &Module,
        get: &dyn Fn(&mut Func, u32),
        retain: bool,
    ) {
        match self {
            Counted::Shims(s) if let Some(each) = &s.small => {
                for c in each {
                    c.emit(f, m, get, retain);
                }
            }
            _ => self.emit(f, m, get, retain),
        }
    }

    /// [`Counted::emit`] of a value kept in memory: a release that runs
    /// in place gets the address `addr` pushes, the payload address of
    /// the value's first leaf at `first` bytes; any other count, its
    /// values, as `emit` does.
    pub(crate) fn emit_at(
        &self,
        f: &mut Func,
        m: &Module,
        addr: &dyn Fn(&mut Func),
        get: &dyn Fn(&mut Func, u32),
        retain: bool,
    ) {
        match self {
            Counted::Shims(s) if retain && s.retain_at => {
                addr(f);
                f.call(Sym::Shim(s.retain, true, 1, false));
            }
            Counted::Shims(s) if !retain && s.release_at => {
                addr(f);
                f.call(Sym::Shim(s.release, false, 1, s.hooked));
            }
            _ => self.emit(f, m, get, retain),
        }
    }

    /// The byte offset of the value's first leaf in its own layout, for
    /// an address found from where that leaf is stored.
    pub(crate) fn first_offset(&self) -> u32 {
        match self {
            Counted::Shims(s) => s
                .in_place
                .as_ref()
                .and_then(|l| l.1.first())
                .map_or(0, |l| l.offset),
            _ => 0,
        }
    }
}

/// How a value of `t` counts, its shims queued for the drain when it has
/// them (`Module::shim_key`): both kinds, since the glue that calls them
/// is built at link, after the drain; one no glue reaches is not linked.
pub(crate) fn counted_of(input: &Input<'_>, m: &mut Module, t: Ty) -> StageResult<Counted> {
    counted_of_portion(input, m, t, hd_layout::shims::Portion::All)
}

/// [`counted_of`] of `portion` of the value's counted parts.
pub(crate) fn counted_of_portion(
    input: &Input<'_>,
    m: &mut Module,
    t: Ty,
    portion: hd_layout::shims::Portion,
) -> StageResult<Counted> {
    use hd_layout::shims::{Count, Shim};
    let (count, shape, _) = (input.shim)(t, Shim::Release, portion, false)?;
    Ok(match count {
        Count::None => Counted::None,
        // An optional whose empty variant is its address null
        // (`shims::null_niche`) counts under that test.
        Count::Address { index, known }
            if crate::layout::variants(input.pool, input.env, t).is_some() =>
        {
            Counted::Optional { index, known }
        }
        Count::Address { index, .. } if hd_mono::holds_coll(input.pool, input.env, t) => {
            Counted::Coll { index }
        }
        Count::Address { index, known } => Counted::Address { index, known },
        Count::Parts | Count::Call => {
            use hd_layout::shims::{Shim, in_place};
            let release_at = in_place(input.pool, input.env, t, Shim::Release, portion);
            let retain_at = in_place(input.pool, input.env, t, Shim::Retain, portion);
            let layout = if release_at || retain_at {
                let mem = crate::layout::mem(input.pool, input.env, t)?;
                Some(Box::new((mem.size, mem.leaves)))
            } else {
                None
            };
            let hooked = hd_layout::shims::hooked(input.pool, input.env, t, Shim::Release, portion);
            let retain = m.shim_key(t, shape, Shim::Retain, portion, false);
            let release = m.shim_key(t, shape, Shim::Release, portion, false);
            // A value in locals takes the by-value forms, linked only where
            // one is used; a release that runs a hook stays in place.
            let retain_value = if retain_at {
                m.shim_key(t, shape, Shim::Retain, portion, true)
            } else {
                retain
            };
            let release_value = if release_at && !hooked {
                m.shim_key(t, shape, Shim::Release, portion, true)
            } else {
                release
            };
            Counted::Shims(Box::new(ShimSyms {
                retain,
                release,
                retain_value,
                release_value,
                width: u32::try_from(crate::layout::vals(input.pool, input.env, t)?.len())
                    .unwrap_or(u32::MAX),
                hooked,
                release_at,
                retain_at,
                in_place: layout,
                small: {
                    let mut each = Vec::new();
                    addresses(input, t, portion, 0, &mut each)?.then(|| each.into())
                },
            }))
        }
    })
}

/// The symbol of `t`'s in-place retain (`retain`) or release shim, when a
/// count of a `t` where it lives calls it: queued alone. A shim counting
/// a part in place (`hd_layout::shims`, `IN_PLACE`) calls only this, so
/// asking [`counted_of_portion`] there would queue the part type's other
/// forms too, each O(its values) for a nest of by-address tuples (#2402),
/// as rustc's `drop_in_place::<(String, (String, ..))>` glue calls the
/// inner type's glue and nothing else. `None` when the count is not in
/// place.
pub(crate) fn in_place_shim(
    input: &Input<'_>,
    m: &mut Module,
    t: Ty,
    portion: hd_layout::shims::Portion,
    retain: bool,
) -> StageResult<Option<Sym>> {
    use hd_layout::shims::{Count, Shim, hooked, in_place};
    let kind = if retain { Shim::Retain } else { Shim::Release };
    if !in_place(input.pool, input.env, t, kind, portion) {
        return Ok(None);
    }
    let (count, shape, _) = (input.shim)(t, Shim::Release, portion, false)?;
    if !matches!(count, Count::Parts | Count::Call) {
        return Ok(None);
    }
    let hooked = !retain && hooked(input.pool, input.env, t, Shim::Release, portion);
    let key = m.shim_key(t, shape, kind, portion, false);
    Ok(Some(Sym::Shim(key, retain, 1, hooked)))
}

/// The most counted addresses of a struct or tuple whose counts runtime
/// glue states inline ([`ShimSyms::small`]), measured on the release
/// bench: past 5, no module changes.
pub const GLUE_ADDRESSES: usize = 5;

/// Whether a `t` value counts as at most [`GLUE_ADDRESSES`] addresses,
/// optionals' among them, with no hook and no switch: pushes each one's
/// count to `out`, its index `first` on. One question of each part's
/// count, each memoized; it stops past the bound.
fn addresses(
    input: &Input<'_>,
    t: Ty,
    portion: hd_layout::shims::Portion,
    first: u32,
    out: &mut Vec<Counted>,
) -> StageResult<bool> {
    use hd_layout::shims::{Count, Shim};
    let (pool, env) = (input.pool, input.env);
    let sum = crate::layout::variants(pool, env, t).is_some();
    match (input.shim)(t, Shim::Release, portion, false)?.0 {
        Count::None => return Ok(true),
        Count::Address { index, known } => {
            let index = first + index;
            out.push(if sum {
                Counted::Optional { index, known }
            } else if hd_mono::holds_coll(pool, env, t) {
                Counted::Coll { index }
            } else {
                Counted::Address { index, known }
            });
            return Ok(out.len() <= GLUE_ADDRESSES);
        }
        Count::Parts | Count::Call if sum || crate::layout::holds_drop(pool, env, t) => {
            return Ok(false);
        }
        Count::Parts | Count::Call => {}
    }
    let s = crate::layout::strip_newtype(pool, env, t);
    let parts = match pool.get(s) {
        hd_types::TyData::Adt(AdtId::Product { .. }, _) => crate::layout::tuple_members(pool, s),
        _ => crate::layout::value_parts(pool, env, s),
    };
    let (_, at) = crate::layout::payload_mem(pool, env, &parts)?;
    for (k, &p) in parts.iter().enumerate() {
        let start = first + u32::try_from(at[k].first).unwrap_or(u32::MAX);
        if !addresses(input, p, portion, start, out)? {
            return Ok(false);
        }
    }
    Ok(true)
}

impl Elem {
    /// Whether an element copied out of a shared buffer needs more than
    /// its bytes: counts.
    #[must_use]
    pub fn counts(&self) -> bool {
        self.counted.counts()
    }
}

/// A list buffer's payload: the capacity in elements, the length, four
/// bytes no code reads or writes, which keep the elements 8-aligned, then
/// the elements inline from [`LIST_ELEMS`].
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
        counted: Counted::Address {
            index: 1,
            known: false,
        },
        parts: Vec::new(),
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
    // A new thread's instance runs `hd.thread` (`module.rt.thread.spawn`).
    if m.threads {
        m.export("hd.thread", Sym::Helper(Helper::ThreadStart));
    }
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
            worker,
        } => {
            // A worker instance runs `hd.init`, then this root
            // (`std-remote.workers.serve`).
            if let Some((serve, k)) = worker {
                let serve = poll(input, m, *serve, *k, None, Vec::new())?;
                m.export("hd.worker", Sym::Helper(collecting(serve)));
            }
            (*main, *key, inits.clone(), Vec::new(), *report)
        }
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
            units,
            graph,
            unit_offsets,
            runtime_inits,
            ..
        } => {
            // The module under test is not an entry module
            // (`module.init.tests.no-entry`), so every init is forbidden.
            // One export each dispatches by number, `hd.test.init(j)` and
            // `hd.test(i, ctx)`: an instance's exports, which every
            // instantiate builds, stay constant in the case and module
            // counts (#1902). `hd.test.init(j)` walks the unit graph from
            // group j's roots and runs the units it reaches in order
            // ([`Helper::TestInitWalk`]), so each init is named once.
            let inits = units
                .iter()
                .map(|ks| ks.iter().map(|k| (*k, true)).collect())
                .collect();
            let graph = test_graph(m, graph, *unit_offsets);
            m.export(
                "hd.test.init",
                Sym::Helper(Helper::TestInitWalk { inits, graph }),
            );
            let mut bang = false;
            // Each init once, in unit order; a set keeps the pass linear
            // in the inits the units name.
            let runtime: HashSet<Hash128> = runtime_inits.iter().copied().collect();
            let mut seen: HashSet<Hash128> = HashSet::default();
            let reinit: Vec<Hash128> = units
                .iter()
                .flatten()
                .copied()
                .filter(|k| !runtime.contains(k) && seen.insert(*k))
                .collect();
            let mut polls = Vec::with_capacity(cases.len());
            for (def, key, report) in cases {
                polls.push(poll(input, m, *def, *key, *report, Vec::new())?);
                bang |= m.frames.contains_key(key);
            }
            m.export("hd.test", Sym::Helper(Helper::TestPoll { polls, reinit }));
            if bang {
                m.export("hd.wake", Sym::Helper(Helper::WakeMark));
            }
            if let Some(c) = m.runtime.report() {
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
                m.export(format!("hd.poll.{n}"), Sym::Helper(collecting(poll)));
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
    };
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: before }));
    let poll = poll(input, m, main, key, report, after)?;
    // A scenario's host reads its bindings through
    // `hd.scenario.cleanup` after the root has ended, so it keeps them
    // and has no audit.
    let poll = if matches!(input.entry, Entry::Scenario { .. }) {
        collecting(poll)
    } else {
        checked(m, poll)
    };
    m.export("hd.poll", Sym::Helper(poll));
    // Checked mode's count totals, for measuring elision (§23.24).
    if let Some(c) = m.runtime.checked {
        m.export("hd.retained", Sym::Inst(c.retained));
        m.export("hd.released", Sym::Inst(c.released));
        m.export("hd.uniques", Sym::Inst(c.unique_checks));
        m.export("hd.no_ops", Sym::Inst(c.no_op_counts));
        m.export("hd.skipped", Sym::Inst(c.skipped_tests));
        if let Some(r) = c.report {
            m.export("hd.cycles.pointer", Sym::Inst(r.pointer));
            m.export("hd.cycles.length", Sym::Inst(r.length));
            m.export("hd.count.pairs.pointer", Sym::Inst(r.count_audit.pointer));
            m.export("hd.count.pairs.length", Sym::Inst(r.count_audit.length));
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

/// [`Helper::ThreadStart`]: `hd.thread(block, stopped)`, which the host
/// calls in a new thread's instance with `stopped` 0. The thread's frames
/// take the region whose top and limit are the block's first two words;
/// the runtime modules' inits give the instance its own runtime storage
/// (`fn.crossing.global.runtime`); then it calls the function value in the
/// next two words, `thread_spawn`'s `run`, on the block, which ends the
/// thread in the block's fifth word, its state (`std.rt.thread`).
/// When the instance stopped instead (a panic or a trap), the host calls
/// it again with `stopped` 2, which only sets the state's ended and
/// stopped bits and wakes its waiters; the stopped thread keeps its owner
/// (`module.rt.thread.wait`). Cost: O(1) besides `run`.
fn thread_start(m: &Module) -> Func {
    let mut f = m.func("rt:thread_start", vec![Val::I32, Val::I32], vec![]);
    f.get(1);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    for (offset, g) in [(0, Global::Stack), (4, Global::StackLimit)] {
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, offset, 2)));
        f.global_set(g);
    }
    // The runtime's inits, each a forbidden context as in `hd.init`.
    let inits: Vec<Hash128> = (m.init_insts.iter())
        .filter(|k| m.rt_insts.contains(k) && !m.quiet_inits.contains(k))
        .filter(|k| !(m.acyclic && m.cycle_insts.contains(k)))
        .copied()
        .collect();
    if !inits.is_empty() {
        forbid_by(&mut f, 1);
        for k in inits {
            f.call(Sym::Inst(k));
        }
        forbid_by(&mut f, -1);
    }
    // A function value's call: its environment, the block, the context of
    // its empty row, then its code's table slot (`emit/closure.rs`).
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
    f.get(0);
    f.i32(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.call_indirect(vec![Val::I32, Val::I32, Val::I32], vec![]);
    f.op(Instruction::Else);
    // A thread that stopped never ended, so its two low bits are clear.
    f.get(0);
    f.i32(3);
    f.op(Instruction::I32AtomicRmwAdd(mem(HEAP, 16, 2)));
    f.op(Instruction::Drop);
    f.get(0);
    f.i32(-1);
    f.op(Instruction::MemoryAtomicNotify(mem(HEAP, 16, 2)));
    f.op(Instruction::Drop);
    f.op(Instruction::End);
    f
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
) -> StageResult<Helper> {
    let mut providers = Vec::new();
    for k in input.env.row_keys(main, TyList::EMPTY) {
        let by_ref = input.host_refs.iter().find(|(t, _)| *t == k).map(|r| r.1);
        let wire = input.env.host_bridge(k).map_or(k, |bridge| bridge.wire);
        providers.push((host_vtable(input, m, wire)?, by_ref));
    }
    let ret = input.env.ret(main).unwrap_or(Ty::VOID);
    let rvals = crate::layout::vals(input.pool, input.env, ret)?;
    let bang = m.frames.contains_key(&key).then(|| rvals.clone());
    Ok(Helper::EntryPoll {
        main: key,
        providers,
        results: u32::try_from(rvals.len()).expect("results"),
        report,
        then,
        bang,
    })
}

/// In checked mode, `poll` behind the audit at its end
/// ([`Helper::CheckedPoll`]); else `poll` behind a cycle collection
/// ([`Helper::CollectPoll`]).
fn checked(m: &Module, poll: Helper) -> Helper {
    if m.runtime.checked.is_some() {
        Helper::CheckedPoll {
            poll: Box::new(poll),
        }
    } else {
        collecting(poll)
    }
}

/// `poll` behind a cycle collection ([`Helper::CollectPoll`]).
fn collecting(poll: Helper) -> Helper {
    Helper::CollectPoll {
        poll: Box::new(poll),
    }
}

/// The most arms one dispatch function holds. A test program's
/// dispatchers ([`Helper::TestPoll`], [`Helper::TestInit`]) over more
/// split into a tree of functions of at most this many arms each, as
/// LLVM's switch lowering splits a large switch into a tree of tables: an
/// engine's optimizing compile of one function is superlinear in its
/// arms (V8's Turboshaft ran out of memory at 20,000 on Node 24, #2049).
/// A power of two, so a level's choice is a shift and the rest a mask.
pub const DISPATCH_ARMS: usize = 256;
const _: () = assert!(DISPATCH_ARMS.is_power_of_two() && DISPATCH_ARMS > 1);

/// The cases each arm of a dispatch over `n` covers: 1 when the `n` arms
/// fit one function, else the smallest power of [`DISPATCH_ARMS`] that
/// leaves at most [`DISPATCH_ARMS`] arms. The tree is then
/// `log_DISPATCH_ARMS(n)` levels deep, so a case's dispatch costs that many
/// calls, and its functions hold `n` arms in all plus fewer than
/// `n / (DISPATCH_ARMS - 1)` for the levels above the cases.
fn dispatch_span(n: usize) -> usize {
    let mut span = 1;
    while n.div_ceil(span) > DISPATCH_ARMS {
        span *= DISPATCH_ARMS;
    }
    span
}

/// The parts the dispatch over `items` calls, `items` in runs of
/// [`dispatch_span`], when they do not fit one function: none when they do.
pub(crate) fn dispatch_parts<T: Clone>(
    items: &[T],
    part: impl Fn(Vec<T>) -> Helper,
) -> Vec<Helper> {
    let span = dispatch_span(items.len());
    if span == 1 {
        return Vec::new();
    }
    items.chunks(span).map(|c| part(c.to_vec())).collect()
}

/// Opens the blocks of a dispatch on parameter 0 over `n` arms of `span`
/// cases each and branches: `n + 1` nested blocks, the default's outermost
/// and arm 0's innermost, and one `br_table` into them on parameter 0
/// shifted down by `log2(span)`. Arm `i`'s code follows the `i`th
/// [`Instruction::End`] after; [`dispatch_end`] closes the default.
/// Constant work per call, and code linear in the arms.
/// Answers the arm count, the default's label.
fn dispatch(f: &mut Func, arms: usize, span: usize) -> u32 {
    let labels: Vec<u32> = (0u32..).take(arms).collect();
    let n = labels.last().map_or(0, |last| last + 1);
    for _ in 0..=n {
        f.op(Instruction::Block(BlockType::Empty));
    }
    f.get(0);
    if span > 1 {
        // At most 64: exact as an i32.
        f.i32(span.trailing_zeros().cast_signed());
        f.op(Instruction::I32ShrU);
    }
    f.br_table(labels, n);
    n
}

/// An arm's case number within its part of `span` cases: parameter 0's
/// low bits.
fn part_case(f: &mut Func, span: usize) {
    f.get(0);
    // A span past i32 keeps every bit: case numbers are i32s.
    f.i32(i32::try_from(span - 1).unwrap_or(-1));
    f.op(Instruction::I32And);
}

/// The default arm of a [`dispatch`]: a number past the arms traps.
fn dispatch_end(f: &mut Func) {
    f.op(Instruction::End);
    f.op(Instruction::Unreachable);
}

/// [`Helper::TestInit`] and [`Helper::TestInitPart`], `name`: an arm per
/// unit of `groups`, or per part of them ([`dispatch_parts`]).
fn test_init(m: &Module, name: &str, groups: &[Vec<(Hash128, bool)>]) -> Func {
    let mut f = m.func(name, vec![Val::I32], vec![]);
    let span = dispatch_span(groups.len());
    let parts = dispatch_parts(groups, |inits| Helper::TestInitPart { inits });
    if parts.is_empty() {
        dispatch(&mut f, groups.len(), 1);
        for inits in groups {
            f.op(Instruction::End);
            entry_init(&mut f, m, inits);
            f.op(Instruction::Return);
        }
    } else {
        dispatch(&mut f, parts.len(), span);
        for part in parts {
            f.op(Instruction::End);
            part_case(&mut f, span);
            f.helper(part);
            f.op(Instruction::Return);
        }
    }
    dispatch_end(&mut f);
    f
}

/// Pushes the word at word index `idx` (a local) of the array at static
/// address `at`.
fn load_word(f: &mut Func, at: u32, idx: u32) {
    word_addr(f, at, idx);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
}

/// Pushes the address of word `idx` (a local) of the array at `at`.
fn word_addr(f: &mut Func, at: u32, idx: u32) {
    f.static_addr(at);
    f.get(idx);
    f.i32(2);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32Add);
}

/// Stores local `value` at word `idx` (a local) of the array at `at`.
fn store_word(f: &mut Func, at: u32, idx: u32, value: u32) {
    word_addr(f, at, idx);
    f.get(value);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
}

/// `local += by`.
fn bump(f: &mut Func, local: u32, by: i32) {
    f.get(local);
    f.i32(by);
    f.op(Instruction::I32Add);
    f.set(local);
}

/// Marks unit `u` (a local) and pushes it on the stack at `stack`, depth
/// `sp`, unless its mark byte at `marks` is already set.
fn test_visit(f: &mut Func, marks: u32, stack: u32, sp: u32, u: u32) {
    f.static_addr(marks);
    f.get(u);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.static_addr(marks);
    f.get(u);
    f.op(Instruction::I32Add);
    f.i32(1);
    f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
    store_word(f, stack, sp, u);
    bump(f, sp, 1);
    f.op(Instruction::End);
}

/// Sifts the word at index `r` (a local, changed) of the max-heap of
/// `size` (a local) words at `at` down to its place, as heapsort does:
/// a loop, `log size` steps. `c` and `t` are scratch locals.
fn test_sift(f: &mut Func, at: u32, r: u32, size: u32, c: u32, t: u32) {
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    // c = 2r + 1; stop at the heap's end.
    f.get(r);
    f.i32(1);
    f.op(Instruction::I32Shl);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(c);
    f.get(c);
    f.get(size);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    // The larger child.
    f.get(c);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.get(size);
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
    load_word(f, at, c);
    f.get(c);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(t);
    load_word(f, at, t);
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
    f.get(t);
    f.set(c);
    f.op(Instruction::End);
    f.op(Instruction::End);
    // Stop once the parent is no smaller.
    load_word(f, at, r);
    load_word(f, at, c);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    test_swap(f, at, r, c, t);
    f.get(c);
    f.set(r);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// Swaps words `a` and `b` (locals) of the array at `at`, through `t`.
fn test_swap(f: &mut Func, at: u32, a: u32, b: u32, t: u32) {
    load_word(f, at, a);
    f.set(t);
    word_addr(f, at, a);
    load_word(f, at, b);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    store_word(f, at, b, t);
}

/// [`Helper::TestInitWalk`]: group (parameter 0)'s roots onto the stack,
/// then a depth-first walk of the units, each marked once and recorded;
/// a heapsort of the recorded unit numbers, whose order is the run
/// order; then each unit's inits through [`Helper::TestInit`]. Every
/// step is a loop over static memory, so no input nests a call.
fn test_init_walk(m: &Module, inits: &[Vec<(Hash128, bool)>], g: TestGraph) -> Func {
    let mut f = m.func("rt:test_init_walk", vec![Val::I32], vec![]);
    let (stack, out, marks) = (g.scratch, g.scratch + 4 * g.units, g.scratch + 8 * g.units);
    let [i, end, sp, n, u, c, t, r] = [(); 8].map(|()| f.local(Val::I32));
    // The group's roots: words [at[g], at[g + 1]).
    load_word(&mut f, g.at, 0);
    f.set(i);
    f.get(0);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(t);
    load_word(&mut f, g.at, t);
    f.set(end);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    f.get(end);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    load_word(&mut f, g.at, i);
    f.set(u);
    test_visit(&mut f, marks, stack, sp, u);
    bump(&mut f, i, 1);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    // The walk: pop a unit, record it, push its unmarked dependencies.
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(sp);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    bump(&mut f, sp, -1);
    load_word(&mut f, stack, sp);
    f.set(u);
    store_word(&mut f, out, n, u);
    bump(&mut f, n, 1);
    f.get(u);
    f.u32(g.unit_offsets);
    f.op(Instruction::I32Add);
    f.set(t);
    load_word(&mut f, g.at, t);
    f.set(i);
    bump(&mut f, t, 1);
    load_word(&mut f, g.at, t);
    f.set(end);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    f.get(end);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    load_word(&mut f, g.at, i);
    f.set(c);
    test_visit(&mut f, marks, stack, sp, c);
    bump(&mut f, i, 1);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    // Heapsort the n recorded units: build the heap from n / 2 down, then
    // move the largest to the end, n - 1 times.
    f.get(n);
    f.i32(1);
    f.op(Instruction::I32ShrU);
    f.set(i);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    bump(&mut f, i, -1);
    f.get(i);
    f.set(r);
    test_sift(&mut f, out, r, n, c, t);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(n);
    f.set(end);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(end);
    f.i32(1);
    f.op(Instruction::I32LeU);
    f.op(Instruction::BrIf(1));
    bump(&mut f, end, -1);
    f.i32(0);
    f.set(r);
    test_swap(&mut f, out, r, end, t);
    test_sift(&mut f, out, r, end, c, t);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    // Run the units in order.
    f.i32(0);
    f.set(i);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    f.get(n);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    load_word(&mut f, out, i);
    f.helper(Helper::TestInit {
        inits: inits.to_vec(),
    });
    bump(&mut f, i, 1);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f
}

/// [`Helper::TestPoll`] (`reinit` some) and [`Helper::TestPollPart`]: the
/// dispatch, an arm per case of `polls` or per part of them
/// ([`dispatch_parts`]), in a block whose end takes the case's status;
/// then, at the root, a checked poll's end, or a collecting poll's.
fn test_poll(m: &Module, polls: &[Helper], reinit: Option<&[Hash128]>) -> Func {
    let name = if reinit.is_some() {
        "rt:test_poll"
    } else {
        "rt:test_poll_part"
    };
    let mut f = m.func(name, vec![Val::I32, Val::I32], vec![Val::I32]);
    let status = f.local(Val::I32);
    f.op(Instruction::Block(BlockType::Empty));
    let span = dispatch_span(polls.len());
    let parts = dispatch_parts(polls, |polls| Helper::TestPollPart { polls });
    let arms = if parts.is_empty() {
        polls.len()
    } else {
        parts.len()
    };
    let n = dispatch(&mut f, arms, span);
    for (i, at) in (0..n).zip(0usize..) {
        f.op(Instruction::End);
        if let Some(part) = parts.get(at) {
            part_case(&mut f, span);
            f.get(1);
            f.helper(part.clone());
        } else {
            match &polls[at] {
                Helper::EntryPoll {
                    main,
                    providers,
                    results,
                    report,
                    then,
                    bang: None,
                } => entry_poll(&mut f, m, *main, providers, *results, *report, then),
                // A suspending root's poll keeps its frame state machine
                // (`task::root_poll`) in a function of its own.
                suspending => {
                    f.get(1);
                    f.helper(suspending.clone());
                }
            }
        }
        f.set(status);
        // Out past the arms still open, the default's block and into
        // the end: `n - i` labels out.
        f.op(Instruction::Br(n - i));
    }
    dispatch_end(&mut f);
    f.op(Instruction::End);
    if let Some(reinit) = reinit {
        if m.runtime.checked.is_some() {
            checked_end(&mut f, m, status, reinit);
        } else if !m.acyclic {
            f.helper(Helper::Collect);
        }
    }
    f.get(status);
    f
}

/// [`Helper::EntryInit`]'s code: the inits not known quiet, in order.
fn entry_init(f: &mut Func, m: &Module, inits: &[(Hash128, bool)]) {
    for (k, forbid) in inits.iter().filter(|(k, _)| !m.quiet_inits.contains(k)) {
        // A program that links no collector needs no collector state.
        if !(m.acyclic && m.cycle_insts.contains(k)) {
            call_init(f, *k, *forbid);
        }
    }
}

/// [`Helper::EntryPoll`]'s code for a root that does not suspend, leaving
/// its status: `main` with its host providers, its result to `report` or
/// dropped, then `then`.
fn entry_poll(
    f: &mut Func,
    m: &Module,
    main: Hash128,
    providers: &[(usize, Option<Hash128>)],
    results: u32,
    report: Option<Hash128>,
    then: &[(Hash128, bool)],
) {
    host_providers(f, providers);
    f.call(Sym::Inst(main));
    if let Some(k) = report {
        f.call(Sym::Inst(k));
    } else {
        for _ in 0..results {
            f.op(Instruction::Drop);
        }
        // A script's later inits: its own bodies, which share the
        // first's providers and result, and the others, as forbidden
        // contexts.
        for (k, forbid) in then.iter().filter(|(k, _)| !m.quiet_inits.contains(k)) {
            if !forbid {
                host_providers(f, providers);
            }
            call_init(f, *k, *forbid);
            if !forbid {
                for _ in 0..results {
                    f.op(Instruction::Drop);
                }
            }
        }
        f.i32(0);
    }
}

/// [`Helper::CheckedPoll`].
fn checked_poll(poll: &Helper, m: &Module) -> Func {
    let mut f = m.func("rt:checked_poll", vec![Val::I32], vec![Val::I32]);
    let status = f.local(Val::I32);
    f.get(0);
    f.helper(poll.clone());
    f.set(status);
    checked_end(&mut f, m, status, &[]);
    f.get(status);
    f
}

/// [`Helper::CheckedPoll`]'s end, after the poll's answer is in local
/// `status`: once the root is not Pending, the teardown, the audit and
/// `reinit` ([`Helper::TestPoll`]'s); else a collection.
fn checked_end(f: &mut Func, m: &Module, status: u32, reinit: &[Hash128]) {
    let r = m.runtime;
    let c = r.checked.unwrap();
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
    for k in reinit.iter().filter(|k| !m.quiet_inits.contains(k)) {
        call_init(f, *k, true);
    }
    f.op(Instruction::Else);
    if !m.acyclic {
        f.helper(Helper::Collect);
    }
    f.op(Instruction::End);
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
        s.counted
            .emit(&mut f, m, &|f, k| f.get(ls[k as usize]), false);
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

/// The vtable of the host's provider of capability `key`, one slot per
/// method that keeps one (`hd_mono::Collected::slots`), in the trait's
/// method order: each a host call or a host leaf over the exchange
/// buffer's codecs (`boundary.rs`). A closed program keeps no slot for a
/// method that no reachable code calls; a REPL session's table, which
/// keeps every slot, gets a trap with no import for it.
fn host_vtable(input: &Input<'_>, m: &mut Module, key: Ty) -> StageResult<usize> {
    // ruled out: a requirement row lists traits (`req.row.syntax.signature`;
    // hd_resolve lowers each key with `Lower::trait_value`), an entry
    // point's row is closed, and a bridged key's wire trait
    // (`ProgramEnv::host_bridge`) is a trait value too.
    let (def, args) = input.pool.trait_value_parts(key);
    // ruled out: module.host.package.not-generic. The `@host` marker check
    // (hd_resolve lower/decorators.rs) and hd_check header::boundary, for
    // a runtime profile's host trait, reject a host trait with type
    // parameters, so a provider key names no instance.
    debug_assert!(
        args == TyList::EMPTY,
        "a generic host trait reached linking"
    );
    let p = (input.path)(def).replace('/', ".");
    let Some(host) = hd_host_abi::host_trait(&p) else {
        return profile_vtable(input, m, def, &p);
    };
    let mut slots = Vec::new();
    for method in input.collected.slots(input.env, def) {
        let mp = (input.path)(method);
        let name = mp.rsplit(['.', '/']).next().unwrap_or("");
        let hm = host.methods.iter().find(|x| x.name == name);
        // ruled out: hd_host_abi's table_mirrors_std_capability_traits test
        // checks that the ABI table has a row for each method a std host
        // trait declares.
        debug_assert!(hm.is_some(), "the std host method `{name}` has no ABI row");
        // A method no reachable code calls links no host method.
        let (true, Some(hm)) = (input.collected.calls_dyn(method), hm) else {
            slots.push(crate::boundary::uncalled_slot(input, method)?);
            continue;
        };
        let row = crate::boundary::Row::of(hm);
        slots.push(crate::boundary::host_slot(
            input, m, host.key, &row, method, false, None,
        )?);
    }
    crate::drive::vtable(input, m, def, slots, Vec::new())
}

/// The vtable of a host trait that a runtime profile declares itself
/// (`spec/conformance/README.md`, Runtime Profiles), which the ABI table
/// does not list: each method is imported from `hd:<Trait>` by its name
/// and waits when it suspends; every argument and result crosses in the
/// exchange buffer, so a result gets each check of its declared type
/// (`module.profile.host-result.*`), and a private-field type crosses as
/// its host codec's string (`module.boundary.serialize.exact`).
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
        package: package.is_some(),
        arguments: None,
    };
    let mut methods = Vec::new();
    let c_table = package.as_ref().and_then(|_| (input.package_c)(def));
    let vtable = match &c_table {
        Some(table) => c_vtable(input, m, def, key, at, table, &mut methods)?,
        None => profile_tree(input, m, def, def, key, at, &mut methods)?,
    };
    if let Some(table) = c_table {
        m.edit(crate::module::Edit::CTable(table));
    }
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
        );
    }
    Ok(vtable)
}

/// The vtable of a package host trait bound to a C provider: one slot per
/// method that keeps one, each a C call ([`crate::boundary::c_slot`]),
/// and its contract methods, the called ones in source order. Such a
/// trait has no supertrait (`hd_check::header::c_trait`), so it has no
/// parent vtable.
///
/// Cost: one pass over the trait's slots, with one table lookup each.
fn c_vtable(
    input: &Input<'_>,
    m: &mut Module,
    def: hd_base::DefId,
    key: &str,
    at: crate::boundary::AtProfile<'_>,
    table: &hd_host_abi::c::Table,
    methods: &mut Vec<hd_host_abi::schema::Method>,
) -> StageResult<usize> {
    let rows: hd_base::HashMap<&str, &hd_host_abi::c::Method> = table
        .methods
        .iter()
        .map(|cm| (cm.name.as_str(), cm))
        .collect();
    let mut slots = Vec::new();
    for method in input.collected.slots(input.env, def) {
        let mp = (input.path)(method);
        let member = mp.rsplit(['.', '/']).next().unwrap_or("");
        let row = rows.get(member).copied();
        // ruled out: the driver builds the table from every method of this
        // trait (`package_c_table`).
        debug_assert!(row.is_some(), "the C method `{member}` has no table row");
        let (true, Some(row)) = (input.collected.calls_dyn(method), row) else {
            slots.push(crate::boundary::uncalled_slot(input, method)?);
            continue;
        };
        slots.push(crate::boundary::c_slot(
            input, m, key, row, method, at, false,
        )?);
        crate::boundary::add_package_method(
            methods,
            crate::boundary::package_method(input, method, def, def, None)?,
        );
    }
    crate::drive::vtable(input, m, def, slots, Vec::new())
}

// ruled out: hd_resolve checks trait.super.acyclic before collection.
// Shared ancestors in a diamond need neither a cycle check nor a visited set.
fn profile_tree(
    input: &Input<'_>,
    m: &mut Module,
    def: hd_base::DefId,
    target: hd_base::DefId,
    key: &str,
    at: crate::boundary::AtProfile<'_>,
    methods: &mut Vec<hd_host_abi::schema::Method>,
) -> StageResult<usize> {
    use hd_host_abi::{Codec, Wait};
    let mut slots = Vec::new();
    for method in input.collected.slots(input.env, def) {
        if !input.collected.calls_dyn(method) {
            slots.push(crate::boundary::uncalled_slot_at(
                input,
                method,
                at.arguments,
            )?);
            continue;
        }
        let mp = (input.path)(method);
        let member = mp.rsplit(['.', '/']).next().unwrap_or("");
        let name = if at.package {
            crate::boundary::package_wire_name(input, target, def, member)
        } else {
            member.to_owned()
        };
        let (params, ret) = crate::boundary::signature(input.pool, input.env, method, at.arguments);
        let codecs = vec![Codec::Buffer(""); params.len()];
        let row = crate::boundary::Row {
            name: &name,
            params: &codecs,
            result: crate::boundary::result_codec(input, ret)?,
            wait: if input.env.suspends(method) {
                Wait::May
            } else {
                Wait::Never
            },
        };
        slots.push(crate::boundary::host_slot(
            input,
            m,
            key,
            &row,
            method,
            false,
            Some(at),
        )?);
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
            );
        }
    }
    let mut parents = Vec::new();
    // Both package and embedding profiles retain their supertrait pointers.
    let supertraits = input.env.decls().supertraits(def);
    if !supertraits.is_empty() {
        for &sup in supertraits {
            let full = at
                .arguments
                .unwrap_or_else(|| input.pool.list(&[hd_mono::class_ref(input.pool)]));
            let sup = hd_mono::subst(input.pool, input.env, def, full, sup);
            // ruled out: hd_resolve's supertrait lowering (`Lower::supers`,
            // through `Lower::trait_value`) makes each supertrait a trait
            // value over `Self`, and `subst` keeps the constructor.
            let (parent, args) = input.pool.trait_value_parts(sup);
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
            )?);
        }
    }
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
    retained_result: &Counted,
    read: &RefRead,
    params: &[Val],
    results: &[Val],
    own: &[(u32, Counted)],
    given: &[(u32, RefLayout)],
    ctx: Option<&SlotCtx>,
    m: &Module,
) -> Func {
    let mut all = vec![Val::I32];
    all.extend(params.iter().copied());
    let mut f = m.func("rt:dyn_read", all, results.to_vec());
    let held = ref_load(&mut f, m, 0, &read.value);
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
    let recv_owned = owned
        .first_mut()
        .is_some_and(|r| std::mem::take(&mut r.1).counts());
    own_lent(&mut f, m, &rvals, &recv, params, &owned);
    recv(&mut f);
    push_slot_params(&mut f, params.len() - usize::from(ctx.is_some()), ctx);
    f.call(Sym::Inst(target));
    retain_slot_result(&mut f, m, results, retained_result);
    if !recv_owned {
        ref_count(&mut f, m, &held, &read.value, false);
    }
    release_given(&mut f, m, own, given);
    f
}

/// Dynamic slots preserve their owned-result ABI even when the concrete
/// function forwards a guaranteed argument. Take the count before ending
/// any argument or receiver owner.
pub(crate) fn retain_slot_result(f: &mut Func, m: &Module, results: &[Val], counted: &Counted) {
    if !counted.counts() {
        return;
    }
    let locals: Vec<_> = results.iter().map(|&v| f.local(v)).collect();
    for &local in locals.iter().rev() {
        f.set(local);
    }
    counted.emit(f, m, &|f, k| f.get(locals[k as usize]), true);
    for &local in &locals {
        f.get(local);
    }
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

/// Logical call shape, available before a helper's body is reached.
/// Fixed/value-vector shapes are O(1); structured host arguments visit
/// their inline record metadata once, without generating code or codecs.
#[must_use]
pub fn arity(helper: &Helper) -> (usize, usize) {
    use Helper as H;
    let arguments = |args: &[crate::task::HostArg]| {
        args.iter()
            .map(|arg| match arg {
                crate::task::HostArg::Str => 2,
                crate::task::HostArg::Scalar(_) | crate::task::HostArg::List(_) => 1,
                crate::task::HostArg::Buf(ty) => ty.value_count(),
            })
            .sum::<usize>()
    };
    let host_result = |result: &crate::task::HostRes| match result {
        crate::task::HostRes::Void => 0,
        crate::task::HostRes::Tags => 2,
        crate::task::HostRes::Buf(ty) => ty.value_count(),
    };
    match helper {
        H::MemoryCopy { .. }
        | H::MemoryFill(_)
        | H::CycleTrace
        | H::CycleRelease
        | H::CycleFree
        | H::FreeClassLoaded
        | H::WakeLink => (3, 0),
        H::CancelChild => (1, 1),
        H::CanCancel
        | H::MapSeed
        | H::EntryCall { .. }
        | H::AllocClass { .. }
        | H::Frame(_)
        | H::CancelEnter
        | H::PendingCold { .. }
        | H::WaitCell
        | H::IdleBlock
        | H::TypeIdOf(_)
        | H::TypeInfo(_) => (0, 1),
        H::CancellationRoot
        | H::IdleWake
        | H::EntryInit { .. }
        | H::EntryReset
        | H::Teardown
        | H::Collect
        | H::Panic(_)
        | H::ForbidCheck
        | H::EntryCancel { .. } => (0, 0),
        H::EntryPoll { .. }
        | H::CheckedPoll { .. }
        | H::CollectPoll { .. }
        | H::Alloc
        | H::Str
        | H::ListBuf { .. }
        | H::ListUnique { .. }
        | H::ListCopy { .. }
        | H::ListGrow { .. }
        | H::MapGrow { .. }
        | H::MapUnique { .. }
        | H::Utf8ValidLen
        | H::PollCheck
        | H::CancelCheck
        | H::WakeTake
        | H::WakeSlot
        | H::WakerNew
        | H::WakerSlot
        | H::CleanupCold
        | H::WaitCold
        | H::WaitPoll
        | H::RuntimeType(_)
        | H::Race { part: 0, .. }
        | H::All { part: 0, .. } => (1, 1),
        H::Opaque
        | H::LoanShare
        | H::LoanUnshare
        | H::FreeClass
        | H::FreeFrame
        | H::ReleaseRef
        | H::ReleaseDrain
        | H::RetainKnown
        | H::ReleaseKnown
        | H::ReleaseFresh
        | H::RetainColl
        | H::DropParts { .. }
        | H::RetainFields { .. }
        | H::DropElems { .. }
        | H::DropEntries { .. }
        | H::PackageHandleRelease { .. }
        | H::Fit
        | H::WakeMark
        | H::WakeRegister
        | H::WakeRemove
        | H::WakerParent
        | H::WakerDrop
        | H::WakeNotify
        | H::HostCancel
        | H::ChildDrop
        | H::CleanupCancel
        | H::PendingCancel
        | H::WaitSignal
        | H::RetainAny
        | H::RaceDrop { .. }
        | H::TestInit { .. }
        | H::TestInitWalk { .. }
        | H::TestInitPart { .. } => (1, 0),
        H::PanicStr
        | H::CopyElems { .. }
        | H::CancelLeave
        | H::IdleHook
        | H::ThreadStart
        | H::WakeSignal
        | H::All { part: 4, .. } => (2, 0),
        H::PanicWith => (4, 0),
        H::ToBuf
        | H::New
        | H::NewFixed { .. }
        | H::CtxFind
        | H::CtxNew
        | H::ListTruncate { .. }
        | H::ListReserve { .. }
        | H::ListExtend { .. }
        | H::StrToBytes { .. }
        | H::LebPut
        | H::WakerLive
        | H::TestPoll { .. }
        | H::TestPollPart { .. } => (2, 1),
        H::RowAdapter(row) => (row.params.len(), row.results.len()),
        H::ReleaseSpilled { leaves, .. } => (leaves.len(), 0),
        H::MapTable { .. } => (2, 1),
        H::MapHash { key, .. } => (key.len() + 1, 1),
        H::MapFind { entry, .. } => (2 + entry.keys, 1),
        H::MapLookup { entry, probe, .. } => (2 + entry.keys, if *probe { 2 } else { 1 }),
        H::MapPut { entry, .. } => (3 + entry.leaves.len(), 1),
        H::MapSlotNext => (3, 1),
        H::DynSlot {
            params, results, ..
        }
        | H::DynRead {
            params, results, ..
        } => (1 + params.len(), results.len()),
        H::MapStep { item, .. } => (3, item.mem.leaves.len()),
        H::DynTrap { params, results } => (params.len(), results.len()),
        H::StrFromBytes | H::LebGet | H::CleanupPoll | H::All { part: 1, .. } => (1, 2),
        H::StrAppend | H::FrameEnsure(_) => (4, 2),
        H::FmtInt { .. } => (2, 2),
        H::StrEq | H::BytesIndex | H::BytesMismatch => (4, 1),
        H::BytesIndexSet(set) => (if set.is_some() { 3 } else { 7 }, 1),
        H::BytesIndexPair => (6, 1),
        H::HostPrim { args, result, .. } => (
            args.iter()
                .map(|arg| match arg {
                    PrimArg::Str => 2,
                    PrimArg::Scalar(_) => 1,
                })
                .sum(),
            if result.is_some() { 1 } else { 2 },
        ),
        H::HostCold { args, .. } => (1 + arguments(args), 1),
        H::HostPoll { res, .. } => (1, 1 + host_result(res)),
        H::HostDrive {
            blocking,
            args,
            res,
            ..
        } => (
            1 + arguments(args) + usize::from(*blocking),
            1 + host_result(res),
        ),
        H::Drive {
            blocking,
            params,
            results,
            ..
        } => (params.len() + usize::from(*blocking), 1 + results.len()),
        H::HostCall { args, result, .. } => (
            1 + arguments(args),
            match result {
                crate::boundary::CallRes::Void => 0,
                crate::boundary::CallRes::Scalar(_) => 1,
                crate::boundary::CallRes::Buf(ty) => ty.value_count(),
                crate::boundary::CallRes::Handle(_) => 2,
            },
        ),
        H::HandleSlot {
            params,
            ctx,
            results,
            ..
        } => (1 + params.len() + usize::from(*ctx), results.len()),
        H::Enc(ty) => (1 + ty.value_count(), 1),
        H::Dec(ty) => (1, 1 + ty.value_count()),
        H::PendingPoll { results } => (1, 1 + results.len()),
        H::Race {
            part: 1, results, ..
        } => (1, 1 + results.len()),
        H::Race { .. } | H::All { .. } => (1, 0),
    }
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
        Helper::ThreadStart => thread_start(m),
        Helper::Frame(_)
        | Helper::FrameEnsure(_)
        | Helper::PollCheck
        | Helper::CancelCheck
        | Helper::CancelEnter
        | Helper::CancelLeave
        | Helper::CancelChild
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
        | Helper::IdleHook
        | Helper::IdleWake
        | Helper::IdleBlock
        | Helper::Race { .. }
        | Helper::RaceDrop { .. }
        | Helper::All { .. } => crate::task::helper(h, m),
        Helper::HandleSlot {
            inner,
            params,
            ctx,
            results,
        } => crate::boundary::handle_slot(m, inner, params, *ctx, results),
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
            entry_init(&mut f, m, inits);
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
            entry_poll(&mut f, m, *main, providers, *results, *report, then);
            f
        }
        Helper::CheckedPoll { poll } => checked_poll(poll, m),
        Helper::TestPoll { polls, reinit } => test_poll(m, polls, Some(reinit)),
        Helper::TestPollPart { polls } => test_poll(m, polls, None),
        Helper::TestInit { inits } => test_init(m, "rt:test_init", inits),
        Helper::TestInitWalk { inits, graph } => test_init_walk(m, inits, *graph),
        Helper::TestInitPart { inits } => test_init(m, "rt:test_init_part", inits),
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
            {
                let r = m.runtime;
                f.get(0);
                f.call(Sym::Inst(r.cycle.opaque));
            }
            f
        }
        Helper::CycleFree => {
            let mut f = m.func("rt:cycle_free", vec![Val::I32; 3], vec![]);
            if let Some(r) = m.runtime.report() {
                f.get(1);
                f.call(Sym::Inst(r.collected));
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
        Helper::FreeFrame => {
            let mut f = m.func("rt:free_frame", vec![Val::I32], vec![]);
            let block = f.local(Val::I32);
            f.get(0);
            f.u32(HEADER);
            f.op(Instruction::I32Sub);
            f.set(block);
            f.get(block);
            if m.fixed_allocations && m.runtime.checked.is_none() {
                f.helper(Helper::FreeClass);
            } else {
                f.call(Sym::Inst(m.runtime.free));
            }
            f
        }
        Helper::Str => {
            let mut f = m.func("rt:str", vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.i32(0);
            f.helper(Helper::New);
            f
        }
        Helper::ReleaseRef | Helper::ReleaseKnown | Helper::ReleaseFresh => release_ref(m, h),
        Helper::ReleaseDrain => release_drain(m),
        Helper::RetainKnown | Helper::RetainColl => {
            let name = if *h == Helper::RetainColl {
                "rt:retain_coll"
            } else {
                "rt:retain_known"
            };
            let mut f = m.func(name, vec![Val::I32], vec![]);
            f.get(0);
            f.call(Sym::Inst(m.runtime.retain_known));
            f
        }
        Helper::RetainFields { leaves, parts } => {
            parts_glue(m, "rt:retain_fields", leaves, parts, true)
        }
        Helper::DropParts { leaves, parts } => parts_glue(m, "rt:drop", leaves, parts, false),
        Helper::ReleaseSpilled { size, leaves, shim } => {
            release_spilled(m, *size, leaves, Sym::Shim(*shim, false, 1, true))
        }
        Helper::ListBuf { elem } => list_buf(m, elem),
        Helper::DropElems { elem } => drop_elems(m, elem),
        Helper::RowAdapter(row) => row::build(m, row),
        Helper::CtxFind => ctx_find(m),
        Helper::CtxNew => ctx_new(m),
        Helper::CopyElems { elem } => copy_elems(elem, m),
        Helper::ListUnique { elem } => list_unique(elem, m),
        Helper::ListCopy { elem } => list_copy(elem, m),
        Helper::ListGrow { elem } => list_grow(elem, m),
        Helper::MapSeed => map::seed(m),
        Helper::MapTable { entry } => map::table(m, entry),
        Helper::DropEntries { entry } => map::drop_entries(m, entry),
        Helper::MapHash { key, ops } => map::hash(m, key, ops),
        Helper::MapFind { entry, ops } => map::find(m, entry, ops),
        Helper::MapLookup { entry, ops, probe } => map::lookup(m, entry, ops, *probe),
        Helper::MapPut { entry, ops } => map::put(m, entry, ops),
        Helper::MapGrow { entry } => map::grow(entry, m),
        Helper::MapUnique { entry } => map::unique(entry, m),
        Helper::MapSlotNext => map::slot_next(m),
        Helper::DynSlot {
            target,
            body,
            retained_result,
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
            own_lent(&mut f, m, &rvals, &recv, params, own);
            recv(&mut f);
            push_slot_params(
                &mut f,
                params.len() - usize::from(ctx.is_some()),
                ctx.as_ref(),
            );
            if *body {
                f.call(Sym::Part(*target, crate::module::Part::Body));
            } else {
                f.call(Sym::Inst(*target));
            }
            retain_slot_result(&mut f, m, results, retained_result);
            release_given(&mut f, m, own, given);
            f
        }
        Helper::DynRead {
            target,
            retained_result,
            read,
            params,
            results,
            own,
            given,
            ctx,
        } => dyn_read(
            *target,
            retained_result,
            read,
            params,
            results,
            own,
            given,
            ctx.as_ref(),
            m,
        ),
        Helper::ListTruncate { elem } => list_truncate(m, elem),
        Helper::ListReserve { elem } => list_reserve(elem, m),
        Helper::ListExtend { elem } => list_extend(elem, m),
        Helper::TypeIdOf(parts) => type_id(parts, vec![], m),
        Helper::TypeInfo(parts) => type_info(parts, m),
        Helper::RuntimeType(parts) => type_id(parts, vec![Val::I32], m),
        Helper::MapStep { entry, item } => map::step(m, entry, item),
        Helper::DynTrap { params, results } => {
            let mut f = m.func("rt:dyn_trap", params.clone(), results.clone());
            f.op(Instruction::Unreachable);
            f
        }
        Helper::StrFromBytes => str_from_bytes(m),
        Helper::StrToBytes { elem } => str_to_bytes(m, elem),
        Helper::StrAppend => str_append(m),
        Helper::BytesIndexSet(set) => {
            let params = if set.is_some() {
                vec![Val::I32; 3]
            } else {
                vec![
                    Val::I32,
                    Val::I32,
                    Val::I32,
                    Val::I64,
                    Val::I64,
                    Val::I64,
                    Val::I64,
                ]
            };
            let mut f = m.func("rt:bytes_index_set", params, vec![Val::I32]);
            f.raw = Some(crate::scan::set_index_code(*set));
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
            {
                let r = m.runtime;
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
    // The declaration must track its body constructor before wide lowering.
    // This verifies metadata consistency rather than inferring a signature
    // by generating a helper during stack analysis.
    debug_assert_eq!(
        arity(h),
        (f.params.len(), f.results.len()),
        "helper signature {}",
        f.name
    );
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

/// A vtable slot's counts for its target (`crates/hd_wasm/DESIGN.md`
/// §23.25): a dynamic call lends the receiver, pushed by `recv` with
/// layout `rvals`, and the arguments, in the slot's parameters from 1 on,
/// laid out as `params`. `own` has, for each of the target's parameters,
/// the receiver first, how many values it has and its walk when the target
/// owns it; each of those takes a count of its own here.
fn own_lent(
    f: &mut Func,
    m: &Module,
    rvals: &[Val],
    recv: &dyn Fn(&mut Func),
    params: &[Val],
    own: &[(u32, Counted)],
) {
    let ((_, r), rest) = own.split_first().unwrap();
    own_arg(f, m, rvals, r, recv);
    let mut at = 1u32;
    for (n, w) in rest {
        let first = at;
        let vals = &params[(first - 1) as usize..(first - 1 + n) as usize];
        own_arg(f, m, vals, w, &|f| {
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
fn release_given(f: &mut Func, m: &Module, own: &[(u32, Counted)], given: &[(u32, RefLayout)]) {
    for (k, value) in given {
        let first: u32 = 1 + own[1..*k as usize].iter().map(|o| o.0).sum::<u32>();
        let n = own[*k as usize].0;
        let locals: Vec<u32> = (first..first + n).collect();
        ref_count(f, m, &locals, value, false);
    }
}

/// One more count on a value that a helper lends to a plain function that
/// owns its parameter (`layout::Pass::Owned`, `crates/hd_wasm/DESIGN.md`
/// §23.25): `get` pushes the value, of layout `vals`; `walk` is the
/// parameter's when the callee owns it.
pub(crate) fn own_arg(
    f: &mut Func,
    m: &Module,
    vals: &[Val],
    counted: &Counted,
    get: &dyn Fn(&mut Func),
) {
    if !counted.counts() {
        return;
    }
    get(f);
    let ls: Vec<u32> = vals.iter().map(|&v| f.local(v)).collect();
    for &l in ls.iter().rev() {
        f.set(l);
    }
    counted.emit(f, m, &|f, k| f.get(ls[k as usize]), true);
}

/// What retains the address on the stack, given whether it is a known
/// payload start: std's `retain`, `retain_known` for a known start, or
/// using the required std runtime.
pub(crate) fn retain_call(m: &Module) -> impl Fn(&mut Func, Addr) {
    let retain = m.runtime.retain;
    move |f: &mut Func, addr: Addr| match addr {
        Addr::Known => f.helper(Helper::RetainKnown),
        Addr::Ref => f.call(Sym::Inst(retain)),
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

/// [`Helper::DropParts`] and [`Helper::RetainFields`]: loads the
/// payload's leaves, then counts each part through its type's count.
fn parts_glue(
    m: &Module,
    name: &str,
    leaves: &[Leaf],
    parts: &[(u32, Counted)],
    retain: bool,
) -> Func {
    let mut f = m.func(name, vec![Val::I32], vec![]);
    // Each leaf a count reads is loaded where it is first read, into a
    // local of its own: a part released in place reads only its address,
    // so the glue is O(the leaves its counts read), not O(the payload's).
    // Parts own disjoint leaves, and a part's count reads each of its
    // leaves first outside any test, so the memo is the part's own.
    for (first, counted) in parts {
        let ls: std::cell::RefCell<hd_base::HashMap<usize, u32>> = std::cell::RefCell::default();
        let get = |f: &mut Func, k: u32| {
            let at = (first + k) as usize;
            if let Some(&l) = ls.borrow().get(&at) {
                f.get(l);
                return;
            }
            let l = f.local(leaves[at].val);
            f.get(0);
            f.op(leaves[at].load(0));
            f.op(Instruction::LocalTee(l));
            ls.borrow_mut().insert(at, l);
        };
        // A small struct's counts are inline, on the loaded values.
        if let Counted::Shims(s) = counted
            && s.small.is_some()
        {
            counted.emit_glue(&mut f, m, &get, retain);
            continue;
        }
        let off = leaves[*first as usize].offset - counted.first_offset();
        counted.emit_at(
            &mut f,
            m,
            &|f| {
                f.get(0);
                if off != 0 {
                    f.u32(off);
                    f.op(Instruction::I32Add);
                }
            },
            &get,
            retain,
        );
    }
    f
}

/// [`Helper::ReleaseSpilled`]: the value moves into a frame slot in heap
/// form, after a zeroed frame header, and its in-place release runs
/// there.
fn release_spilled(m: &Module, size: u32, leaves: &[Leaf], shim: Sym) -> Func {
    let mut f = m.func(
        "rt:release_spilled",
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
    f.store_locals(at, leaves, 0..);
    f.get(at);
    f.call(shim);
    crate::wide::locals_frame(
        &mut f,
        base,
        (crate::module::HEADER + size).next_multiple_of(8),
    );
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
            f.glue(g.clone());
            f.u32(if cyclic { CYCLIC + 1 } else { 1 });
            f.op(Instruction::I32Add);
            if cyclic || recursive {
                f.drain_glue(g);
            }
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
const FREED: u32 = 640;

/// Pop the class's freed list, else a free slot, or bump within the head
/// page when it remains non-full. The last available slot, a missing
/// page/table, and page-list maintenance stay in std, which is given the
/// class's bytes as the size. There is no call between reading metadata
/// and updating it. The page fast paths share one tail, so that V8 inlines
/// this into each allocation.
fn alloc_class(m: &Module, class: u32, bytes: u32) -> Func {
    let mut f = m.func(format!("rt:alloc_class/{class}"), vec![], vec![Val::I32]);
    let r = m.runtime;
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
    if m.acyclic {
        return;
    }
    let r = m.runtime;
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
    let r = m.runtime;
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
    if m.threads {
        // A thread's instance has no table before its first allocation
        // (`module.rt.thread.alloc`): `free` gives it its heap first.
        f.call(Sym::Inst(r.small.metadata));
        f.op(Instruction::I32Eqz);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::I32And);
    }
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
    let r = m.runtime;
    f.get(block);
    if m.fixed_allocations && r.checked.is_none() {
        f.helper(Helper::FreeClass);
    } else {
        f.call(Sym::Inst(r.free));
    }
}

/// [`Helper::New`]: a block of the header and the payload from std's
/// `alloc`.
///
/// With the runtime, a collection runs first when one is due (at a known
/// class, only on the class's slow path). An object whose descriptor has
/// [`CYCLIC`] takes part in collections; the descriptor written here has
/// its collector bits clear, black and no frame (§23.28).
fn new_object(m: &Module, fixed: Option<u32>) -> Func {
    let small = fixed.and_then(|size| {
        if m.runtime.checked.is_some() {
            return None;
        }
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
    if let Some((class, bytes)) = small {
        // The class's block holds the header and the payload.
        f.helper(Helper::AllocClass { class, bytes });
    } else {
        f.get(0);
        f.u32(HEADER);
        f.op(Instruction::I32Add);
        f.call(Sym::Inst(m.runtime.alloc));
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
    if m.acyclic {
        return f;
    }
    let r = m.runtime;
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
/// `h` says what the address is: any address ([`Helper::ReleaseRef`]), a
/// known payload start ([`Helper::ReleaseKnown`]), or a fresh buffer's
/// last count, which no collector sees ([`Helper::ReleaseFresh`]).
fn release_ref(m: &Module, h: &Helper) -> Func {
    let fresh = *h == Helper::ReleaseFresh;
    let name = if fresh {
        "rt:release_fresh"
    } else {
        "rt:release_ref"
    };
    let mut f = m.func(name, vec![Val::I32], vec![]);
    let r = m.runtime;
    let block = if fresh { 0 } else { f.local(Val::I32) };
    let known = *h == Helper::ReleaseKnown;
    if fresh {
        f.get(0);
        f.u32(HEADER);
        f.op(Instruction::I32Sub);
        f.set(block);
    } else if !m.acyclic {
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
    if !fresh {
        f.get(0);
        f.call(Sym::Inst(if known { r.release_known } else { r.release }));
        f.set(block);
        f.get(block);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        f.op(Instruction::Return);
        f.op(Instruction::End);
    }

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
    glue_of(&mut f);
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
    let r = m.runtime;
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
    let glue =
        (elem.counted.counts()).then(|| Sym::Helper(Helper::DropElems { elem: elem.clone() }));
    push_desc(&mut f, glue, elem.cyclic);
    f.helper(Helper::New);
    f.set(b);
    f.get(b);
    if elem.size == 0 {
        f.u32(u32::MAX);
    } else {
        f.get(b);
        f.u32(HEADER);
        f.op(Instruction::I32Sub);
        f.call(Sym::Inst(m.runtime.block_bytes));
        f.u32(HEADER + LIST_ELEMS);
        f.op(Instruction::I32Sub);
        f.u32(elem.size);
        f.op(Instruction::I32DivU);
    }
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
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
    if elem.counted.counts() {
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
            elem.counted
                .emit(f, m, &|f, k| f.get(ls[k as usize]), false);
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
        elem.counted
            .emit_glue(f, m, &|f, k| f.get(ls[k as usize]), false);
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
    each_elem(&mut f, 0, 1, elem.size, &|f| {
        fill_elem(f, 0, &elem.leaves, &ls);
        elem.counted
            .emit_glue(f, m, &|f, k| f.get(ls[k as usize]), true);
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

/// With a capacity on the stack: a new buffer for `elem` in local `nb`,
/// holding a bitwise copy of the `n` elements of the buffer in local `b`,
/// at length `n`. The caller settles the elements' counts.
fn new_buffer_copy(f: &mut Func, elem: &Elem, b: u32, n: u32, nb: u32) {
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
    new_buffer_copy(&mut f, elem, b, n, nb);
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
/// one requesting twice its length plus 4 (clamped to the largest fitting
/// capacity), and the old one, its elements moved and its length 0, loses
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
    let most = (u32::MAX - LIST_ELEMS - HEADER)
        .checked_div(elem.size)
        .unwrap_or(u32::MAX);
    f.get(n);
    f.u32(most);
    f.op(Instruction::I32GeU);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "heap-exhausted: a list does not fit the heap".into(),
    ));
    f.op(Instruction::End);
    // Clamp the geometric request before multiplying, including element
    // layouts whose largest possible capacity is less than four.
    f.get(n);
    f.u32(most.saturating_sub(4).div_ceil(2));
    f.op(Instruction::I32GeU);
    f.op(Instruction::If(BlockType::Result(Val::I32.wasm())));
    f.u32(most);
    f.op(Instruction::Else);
    f.get(n);
    f.i32(2);
    f.op(Instruction::I32Mul);
    f.i32(4);
    f.op(Instruction::I32Add);
    f.op(Instruction::End);
    new_buffer_copy(&mut f, elem, b, n, nb);
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
    if let Some(c) = m.runtime.checked {
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
    new_buffer_copy(&mut f, elem, b, n, nb);
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
/// is its bytes in place. A shared one is copied into a string
/// buffer and loses the count. No bytes is the static empty string.
fn str_from_bytes(m: &Module) -> Func {
    let mut f = m.func("rt:str_from_bytes", vec![Val::I32], vec![Val::I32; 2]);
    let (b, n, s) = (0, f.local(Val::I32), f.local(Val::I32));
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.op(Instruction::If(BlockType::Empty));
    {
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
    {
        let r = m.runtime;
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

fn ref_vals(value: &RefLayout) -> Vec<Val> {
    value.leaves.iter().map(|l| l.val).collect()
}

fn ref_count(f: &mut Func, m: &Module, ls: &[u32], value: &RefLayout, retain: bool) {
    value
        .counted
        .emit(f, m, &|f, k| f.get(ls[k as usize]), retain);
}

/// The value that the reference in local `at` names, in new locals with
/// counts of their own: the leaves at the address. Emitted inline, so a
/// value of any width stays in locals and never crosses a function's
/// parameters or results.
pub(crate) fn ref_load(f: &mut Func, m: &Module, at: u32, value: &RefLayout) -> Vec<u32> {
    let ls = ref_lend(f, at, &value.leaves);
    ref_count(f, m, &ls, value, true);
    ls
}

/// The leaves of the concrete value that the reference in local `at`
/// names, lent with no count of their own: the place's own. Never a copy,
/// so it reads a value that may not copy.
pub(crate) fn ref_lend(f: &mut Func, at: u32, leaves: &[Leaf]) -> Vec<u32> {
    f.stack_values |= leaves.len() > crate::wide::WIDTH;
    f.load_locals(at, leaves)
}

/// Moves the value that the reference in local `at` names out of its
/// place into new locals, which take its counts, and leaves zeros there.
/// [`ref_store`] puts a value back, releasing the zeros.
pub(crate) fn ref_take(f: &mut Func, at: u32, value: &RefLayout) -> Vec<u32> {
    f.stack_values |= value.leaves.len() > crate::wide::WIDTH;
    let ls: Vec<u32> = value.leaves.iter().map(|l| f.local(l.val)).collect();
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
    ls
}

/// Stores the value in locals `ls` through the reference in local `at`,
/// releasing the value it held. An `owned` value's counts move into the
/// place, so its locals keep none; any other keeps its own, and the place
/// takes a count of its own. Emitted inline, as [`ref_load`] is.
pub(crate) fn ref_store(
    f: &mut Func,
    m: &Module,
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
        ref_count(f, m, ls, value, true);
    }
    let old = f.load_locals(at, &value.leaves);
    f.store_locals(at, &value.leaves, ls.iter().copied());
    if release {
        let was = f.reason.replace(hd_tir::own::Reason::Overwritten);
        ref_count(f, m, &old, value, false);
        f.reason = was;
    }
}

/// Gives this allocation its checked descriptor kind. No instruction is
/// added in unchecked builds; the linker aliases the same release glue.
#[cfg(feature = "checked-report")]
pub(crate) fn checked_kind(f: &mut Func, m: &Module, kind: u8) {
    if m.runtime.report().is_none() || !m.allocation_kind_reporting {
        return;
    }
    if let Some(i) = f.body.windows(3).rposition(|w| matches!(w, [Ins::Glue(_) | Ins::Descriptor(_, _), Ins::Op(Instruction::I32Const(c)), Ins::Op(Instruction::I32Add)] if c.cast_unsigned() == 1 || c.cast_unsigned() == CYCLIC + 1)) {
        match f.body[i] {
            Ins::Glue(s) => {
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
    f.call(Sym::Inst(m.runtime.alloc));
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
    {
        let r = m.runtime;
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
/// `docs/compiler/ownership.md#the-mechanism-in-detail-formerly-e2`). A begin adds to the object's
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
    f.call(Sym::Inst(m.runtime.alloc));
}

/// Frees the raw block whose address is on the stack: the free that skips
/// the collector's candidate test.
fn spill_free(f: &mut Func, m: &Module) {
    f.call(Sym::Inst(m.runtime.acyclic.free));
}
