# Parallelism And Locality: Design Study

Status: Stage A is proposed and needs the owner's approval before a spec
pass. Stage C is parked (#163 closed). Resources are handled separately,
by `Disposable` and `using`
([Using Bindings](../../spec/lang/06-control-flow.md#using-bindings)).

This study asks how hd gets real parallelism. Stage A runs on engines that
ship today: separate instances, copied boundary-safe messages and shared
byte buffers, with the existing boundary-safe rule as the only rule for
what may cross (section 3). Stage C, structured parallel children that
read frozen data in place, waits for the language design of what may be
shared between threads.

There is no local `^` mode and no ownership mode. Escape analysis and
scalar replacement are optimizer work after inlining, with no language
mark ([Performance Roadmap](PERFORMANCE_ROADMAP.md)).

hd code in `text` blocks is proposed syntax that no compiler checks yet,
and names are placeholders.

- [1. Motivation](#1-motivation)
- [2. The Model On One Page](#2-the-model-on-one-page)
- [3. Sharing Data Between Threads](#3-sharing-data-between-threads)
- [4. A Tour](#4-a-tour)
- [5. The Rules In Plain Words](#5-the-rules-in-plain-words)
- [Sources](#sources)

## 1. Motivation

Today one instance runs on one thread, and instances exchange only
boundary-safe values
([Instances And Threads](../../spec/lang/10-modules.md#instances-and-threads)).
Real parallelism needs a rule for what threads may share. Isolation covers
it now; deep immutability would cover it later.

## 2. The Model On One Page

**Parallelism, in two stages.**

- **Stage A, Wasm today.** A worker is a separate Wasm instance of the
  same build: a Web Worker in a browser, a thread with its own `Store`
  under wasmtime. Messages are boundary-safe values, copied through the
  existing `Serialize`/`Deserialize` boundary codecs. Large byte data
  lives in shared linear memory (the Wasm threads proposal, shipped), is
  passed by handle, and is synchronized with atomics or handed over by
  transfer. The "Sendable" rule is the existing boundary-safe rule; no
  new trait is needed.
- **Stage C, parked.** Structured parallel children may read the parent's
  data without copying when it is `Frozen` (deeply immutable), and write
  only what they allocate.

The idea in one sentence: **copies and frozen data say how a value crosses
threads.**

## 3. Sharing Data Between Threads

### 3.1 What scriptc Does

scriptc compiles TypeScript to native code with one reference-counted
heap per thread. Its model, from its documentation:

- `node:worker_threads` runs each worker as a native thread with its own
  module state, event loop and heap.
- `postMessage` clones primitives, plain objects, arrays, errors and
  byte buffers, keeping cycles and aliases. Class instances arrive
  without their methods, as in Node. A `MessagePort` moves only when the
  transfer list names it, and the sender's port is unusable after.
- `SharedArrayBuffer` shares fixed-length storage; `Atomics` works on 8,
  16 and 32-bit integer arrays, with `wait`/`notify` on `Int32Array`.
- `publish(value)` from `@scriptc/threads` makes a graph deeply
  immutable and then passes it to other threads by pointer, in constant
  time, keeping classes and identity. A write into a published object
  throws, checked by one extra load and branch on stores into types the
  program publishes. Published objects are never freed. `publish` refuses
  functions, dates, regexes, promises and typed arrays.
- An `InternTable` is a named table of published values that every thread
  shares, insert-only, with lock-free lookups.

What scriptc gets right is the split: mutable data is copied, immutable
data is shared, bytes are shared with atomics, and nothing else crosses.
It relies on a heap where any object can become shared in place. Hd on
Wasm does not have that heap, which is why the stages below exist.

### 3.2 Stage A: Hd On Wasm Today

| scriptc | Hd on Wasm today | Cost |
| --- | --- | --- |
| a worker thread with its own heap | a separate instance of the same module: a Web Worker in a browser, a thread with its own `Store` in wasmtime | an instance per worker, with its own heap and globals; start-up is instantiation plus module init |
| a structured clone per message | a boundary-safe value, encoded by its `Serialize` and decoded by its `Deserialize` through the host boundary codecs | one encode, one copy, one decode per message: linear in size |
| a transfer list | a `SharedBytes` handle that the send marks closed in the sender; later use is the runtime `Closed` error | constant |
| `SharedArrayBuffer` + `Atomics` | a block of shared linear memory (Wasm threads: `memory.atomic.*`, `wait`, `notify`), imported by every instance and reached through a `SharedBytes` handle | no copy; plain loads and stores, atomic ones where asked |
| `publish` of an immutable graph | none without copying: objects cannot cross instances | a copy per worker, or an encoded image in shared memory (example 4.3) |

The message rule is the boundary-safe rule that the spec already has:
primitives, strings, tuples, lists, maps, structs and enums whose parts are
boundary-safe; never mutable types, closures, trait values or live
handles. Issue #166's `Boundary` bound states it in a generic signature,
and its `Portable[fn]` closures, whose captures are boundary-safe, are
how a worker learns what code to run: a worker is the same build, so the
closure travels as a content identity plus encoded captures. A
`SharedBytes` handle is the one new boundary-safe value: an offset, a
length and a generation into shared memory.

Wasm threads are at phase 4 and shipped in every major browser, which
enable shared memory only on cross-origin-isolated pages; wasmtime
supports them at tier 2. A module can use a shared linear memory beside
its own, so stage A needs no new engine feature. It needs host work (spawning instances, queues) and
a std module, both of which need the owner's approval before a spec
pass.

### 3.3 Sharing Objects Between Threads

Objects live in counted linear memory, so stage A instances share none and
every count is non-atomic. Sharing an object between threads would put it
in a shared region, where only those objects need atomic counts. A frozen
graph published once could be made immortal for the life of its region, so
readers do no count updates
([Refcount Runtime](REFCOUNT_RUNTIME.md) §9.5). Wasm threads have shipped,
so this does not wait for an engine feature. It waits for the language
design of what may be shared, and is parked.

### 3.4 Stage C: Structured Parallel Children

The later step is a parallel combinator whose children finish before the
parent continues, as Rust's `thread::scope` and OxCaml's `fork_join`
do. The hazard is a data race, and it comes from aliasing:

- a `List[Loan]` lent to four children is safe only if nobody writes the
  list or a loan while they run. Plain values alias freely, so some other
  path may hold a `mut` view of the same loan.
- the parent's own thread keeps running its other cooperative tasks while
  the parent waits, and those tasks may hold such a path.
- two chunks of one list are disjoint slots, but two slots may point to
  the same object.

So the stage C rule is about mutation, as OxCaml's `contended` mode is: a
parallel child may read the parent's data only through `Frozen` values
(deeply immutable), may write only data it allocates, and returns its
results.

One more problem waits for stage C: top-level mutable `let` bindings.
Today each instance has its own, so stage A has no issue. With one
shared region, a child that calls a function writing a top-level `let`
races with its siblings. The choices are thread-local globals (as
scriptc's per-thread module state; a child's write is then invisible to
the parent) or a rule that a child's code never writes a global, which
needs a fact about whole call graphs. That choice is left for when the
proposal matures.

### 3.5 A Sendable Rule In Hd

- **For copying (stage A):** the boundary-safe rule that exists, spelled
  `Boundary` in a generic bound (#166). It is Swift's `Sendable` for
  values that are copied, and Erlang's rule that every message is a copy.
- **For sharing without copying (stage C):** a `Frozen[T]` value, built
  by `freeze(x)`, which deep-copies a boundary-safe `x` into objects that
  no `&mut` can reach. Since read-only `&` is shallow, this is hd's only
  deep immutability. The structural “reaches no `&mut`” check will be
  designed with stages B/C, not here. It walks every field and every type
  argument at any depth, and never trusts a read-only view: in the old
  `mut T` model, a read-only `Reader { hole: Box[mut Reader] }` still
  reached a writable `Reader`. The parked deep-immutability idea in
  [Open Issues](OPEN_ISSUES.md) describes the property. It is scriptc's
  `publish`, checked statically, and freed by the collector instead of
  being immortal.
- **No `Sync`-style trait for mutable shared state.** Shared mutable
  state is bytes with atomics, or a host service. A mutex-protected object waits for stage C and a separate study.

### 3.6 How Others Share Data

| System | Isolation | Sharing without copying | Static rule |
| --- | --- | --- | --- |
| scriptc | heap per thread; messages cloned | `publish`ed immutable graphs, never freed; `SharedArrayBuffer` | none for messages; `Immutable<T>` checks writes it can see |
| JS workers | heap per worker; structured clone | `SharedArrayBuffer` + `Atomics`; transfer moves buffers and ports | none |
| Erlang | heap per process; messages copied | binaries over 64 bytes are reference-counted off-heap and shared | none needed: data is immutable |
| Go | none: one shared heap | everything | none; a race detector at run time |
| Swift | actors own their state | `Sendable` values; `sending` moves a disconnected region | `Sendable` conformance; region analysis per function |
| OxCaml | none: one shared heap | `portable` functions, `contended` values; immutable data crosses both modes | modes checked per function; `fork_join` needs portable children |
| Hd, stage A | instance per worker; messages copied | `SharedBytes` + atomics | boundary-safe messages |
| Hd, stage C | one shared region | `Frozen` values; children allocate their own writes | `Frozen` for shared reads |

## 4. A Tour

Each example gives its cost on today's Wasm (stage A) and, where it
changes, under stage C. `Workers`, `Sender`, `Receiver`, `SharedBytes`,
`SharedSlot`, `Frozen` and `freeze` are placeholders.

### 4.1 A Parallel Map Over A Large List

A bank scores a million loans against one risk model. Scoring is pure
and takes about a microsecond per loan.

```text
use std.remote.{Portable, RemoteError, portable}

pub struct Loan:
    pub id: u64
    pub principal: i64
    pub rate_bp: i32

pub struct RiskModel:
    pub weights: List[f64]

pub fn score(loan: Loan, model: RiskModel) -> f64:
    f64(loan.principal) * model.weights[0] + f64(loan.rate_bp) * model.weights[1]

fn total_risk!(loans: List[Loan], model: RiskModel) -> Result[f64, RemoteError] $ Workers:
    scores := $.use(Workers).map!(loans, portable(fn(l: Loan) -> f64: score(l, model)))?
    .Ok(scores.sum())
```

`map!` splits the list into one chunk per worker, encodes each chunk
once, and sends it with the encoded `model`. Each worker decodes its
chunk, scores it and sends back a list of `f64`.

- **Copies.** Each loan is encoded, copied and decoded once (about 20
  bytes); each score comes back the same way (8 bytes). `model` is copied
  once per worker. All linear in the input.
- **Allocations.** Each worker allocates its decoded chunk and its
  result list; the parent allocates the joined result.
- **Synchronization.** One message out and one back per chunk.
- **When it pays.** The work per item must exceed the encode and decode
  cost per item, tens of nanoseconds for a small record. At a microsecond
  per loan and four workers, it does.

Under stage C, `loans` and `model` would be `Frozen`; the children read
them in place and only the scores are allocated. The encode, copy and
decode disappear; `freeze` costs one copy, once, which a long-lived
data set pays back over many runs.

### 4.2 A Worker Pool Processing Jobs

An image service resizes uploads. Each job carries its bytes and a
target size; workers keep a decoder cache of their own.

```text
pub struct Job:
    pub id: u64
    pub image: SharedBytes
    pub width: u32

pub struct Done:
    pub id: u64
    pub image: SharedBytes

fn serve!(jobs: Receiver[Job], done: Sender[Done]) -> void $ Workers:
    pool := $.use(Workers).pool(4, portable(fn!(job: Job) -> Done: resize!(job)))
    while true:
        job := match jobs.recv!():
            .Some(next) => next
            .None => break
        pool.submit!(job)          # copies `id` and `width`; the image is a handle
    pool.drain!(done)
```

- **Copies.** `Job` is boundary-safe: its id, width and a 12-byte handle
  are encoded per job. The image bytes are not copied.
- **Allocations.** Each worker decodes a small `Job`; the resized image
  is a new `SharedBytes` block that the worker fills.
- **Synchronization.** A queue per worker, a shared memory allocator
  (one atomic per allocation and free), and the handle transfer.
- **State.** Each worker's decoder cache lives in its own instance, so it
  needs no lock and is not shared. This is Erlang's process model.

### 4.3 A Shared Read-Mostly Cache

A pricing service looks up exchange rates on every request, from four
workers. Rates change once a minute.

Stage A gives three options:

| Option | How | Cost |
| --- | --- | --- |
| a copy per worker | each worker builds or receives the table; a refresh is a message to each | memory times four; a refresh copies the table four times; lookups are local and free |
| a host service | a `RateCache` capability whose host keeps one table behind a lock or sharded map | one boundary crossing and a copy-out per lookup; one copy of the data |
| an encoded image in shared memory | the writer encodes the table into a `SharedBytes` block and publishes its handle; readers decode the entry they need | lookups read bytes in place and decode one entry; a refresh writes a new block and swaps the handle atomically; freeing the old block needs a reader count or an epoch |

```text
fn refresh!(rates: Map[string, f64], current: &mut SharedSlot) -> void:
    image := SharedBytes::encode(rates)       # one encode, into shared memory
    current.swap(image)                       # an atomic store of the handle

fn price(amount: i64, currency: string, current: &SharedSlot) -> f64:
    rate := current.load().lookup::[f64](currency).unwrap_or(1.0)   # decodes one entry
    f64(amount) * rate
```

The image option is how scriptc's `InternTable` would look without a
shared heap: bytes are shared, values are decoded on read. Under
stage C, the table becomes `Frozen[Map[string, f64]]` behind an atomic
reference, and lookups read it directly.

### 4.4 A Large Byte Buffer Handed Between Threads

A log shipper reads 64 MB chunks, and a compressor on another worker
compresses them.

```text
fn ship!(source: &mut FileReader, to: Sender[SharedBytes]) -> Result[void, FsError] $ FsRead:
    while true:
        using chunk = SharedBytes::alloc(64 * 1024 * 1024)
        n := source.read_into!(chunk)?
        if n == 0:
            break
        to.send!(chunk.take(n))       # transfer: `chunk` is closed here
    .Ok(())
```

- **Copies.** The read fills shared memory directly from the host. The
  send copies a 12-byte handle. The compressor reads the same bytes.
- **Allocations.** One block per chunk from the shared allocator; none
  in either instance heap.
- **Synchronization.** One queue operation per chunk. After the send,
  the sender's handle is closed, so a later use is the runtime `Closed`
  error, as for a disposed handle. Two workers that both write one block
  need atomics; that is a different API (`SharedBytes::atomic_*`).
- **Cleanup.** `using` disposes the sender's handle, which does nothing
  after a transfer. The receiver disposes the block when done; a block
  left undisposed is a leak report, as for any handle.

## 5. The Rules In Plain Words

1. Each worker is a separate instance; instances share no objects.
2. A message is a boundary-safe value, copied.
3. A `SharedBytes` handle is boundary-safe; sending it with transfer
   closes the sender's handle.
4. Shared memory is reached only through `SharedBytes`, with atomic
   operations where they are asked for.

## Sources

- scriptc documentation index: <https://scriptc.dev/llms.txt>; sharing
  data between workers: <https://scriptc.dev/docs/threads>
- OxCaml parallelism tutorial (portable, contended, `fork_join2`):
  <https://oxcaml.org/documentation/tutorials/intro-to-parallelism-part-1/>
- Swift SE-0302, `Sendable`:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md>
- Swift SE-0414, region-based isolation:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0414-region-based-isolation.md>;
  SE-0430, `sending`:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md>
- Go, sharing by communicating: <https://go.dev/doc/effective_go>
- Erlang binary handling: <https://www.erlang.org/doc/system/binaryhandling.html>
- Rust scoped threads: <https://doc.rust-lang.org/std/thread/fn.scope.html>
- MDN, transferable objects:
  <https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects>
- WebAssembly proposals and phases: <https://github.com/WebAssembly/proposals>;
  threads: <https://github.com/WebAssembly/threads>; shared-everything
  threads: <https://github.com/WebAssembly/shared-everything-threads>
- Wasmtime proposal support tiers: <https://docs.wasmtime.dev/stability-wasm-proposals.html>;
  `SharedMemory`: <https://docs.wasmtime.dev/api/wasmtime/struct.SharedMemory.html>
