# Parallelism And Locality: Design Study

This is a design study for issue #163, after the owner's decision of
2026-10-17 (late) that resources use `Disposable` and `using`, not
ownership (see the note at the top of
[Ownership And Borrows](OWNERSHIP_DESIGN.md)). Nothing in it is decided.
Every spelling, std name and diagnostic name is a placeholder. Hd code in
`text` blocks is proposed syntax; no compiler checks it.

- [1. Motivation](#1-motivation)
- [2. The Model On One Page](#2-the-model-on-one-page)
- [3. Local Values](#3-local-values)
- [4. Sharing Data Between Threads](#4-sharing-data-between-threads)
- [5. A Tour](#5-a-tour)
- [6. The Rules In Plain Words](#6-the-rules-in-plain-words)
- [7. The Ownership Prototype: Keep Or Remove](#7-the-ownership-prototype-keep-or-remove)
- [8. Re-Plan](#8-re-plan)
- [9. Questions For The Owner](#9-questions-for-the-owner)
- [Sources](#sources)

**Owner decisions (2026-10-17, night).** Keep the `^T` / `^mut T` spelling for the local mode ("stays in this call"). `^` appears only on parameters, receivers, locals and function types, never on results, fields or type arguments, and parts read through `^` are plain. A closure parameter is plain unless written `^fn`. Real parallelism starts with stage A (separate instances, copied boundary-safe messages, `SharedBytes`, worker code named by #166's portable closures, the boundary-safe rule as the only "Sendable" rule); stage C is parked as a P4 issue until shared-everything threads reach phase 3. The prototype re-plan (question 6) is approved: remove the owned mode, moves, drops, `OwnOnly`/`DropOwn` and lifetimes; keep `^` lending, the loan check and the exit ladder; run jobs R1-R3 and L1-L8 in place of slices 12-23. No marker for closures that change captured state (`fn~` was proposed and rejected: "without ownership and lifetime we don't need this"); plain `fn` stays as today.

## 1. Motivation

Ownership and lifetimes were chosen to make resources safe. The owner has
now moved resources to `Disposable` and `using`: a handle is a plain
value, use after dispose is the runtime `Closed` error, and a leak is a
runtime report. That removes the main reason for moves, drops and
lifetimes. It does not remove three needs that ownership also served,
which the owner named:

1. **Modular escape analysis.** Whether a value can outlive a call should
   be checkable one function at a time, from signatures only, so bodies
   check in parallel and a body edit never rechecks its callers. Hd
   already writes every visible signature for this reason
   ([Functions](../../spec/lang/07-functions.md)).
2. **How threads share data.** The owner wants real parallelism. Today
   one instance runs on one thread, and instances exchange only
   boundary-safe values ([Instances And Threads](../../spec/lang/10-modules.md#instances-and-threads)).
   The owner noted that scriptc seems to have this right.
3. **Local closures.** A closure passed to `map`, `sorted_by`, `retry!` or
   `timeout!` is called during the call and then dropped. It should be
   inlined or kept out of the heap, and the language should guarantee it.

A history note. An earlier design (NonEscapable, deleted on 2026-10-16)
had a `^` locality mark and lifetimes, and the owner held then that a
design without lifetimes could not work. The decisive cases were
handles: `open.push(c)` of a connection whose cleanup ran at block end.
Those cases now have runtime answers by the owner's own decision
(`Closed`, the leak report), so the argument for lifetimes lapses with
them. This study checks whether what is left can be served without
lifetimes. It finds that need 1 and need 3 can, and that need 2 mostly
needs something else: isolation now, and deep immutability later.

## 2. The Model On One Page

**Local values.** A parameter, receiver or local written `^T` holds a
value that cannot outlive the current call. It may be read and passed to
other `^` parameters. It cannot be returned, stored in a field, a
collection or a top-level binding, passed to a plain parameter, or
captured by a closure that could outlive the call. `^mut T` is the same,
with a `mut` view, and is exclusive: while it is lent, the place it came
from is not used. A closure that captures a `^` value is itself local.
There are no lifetime names, no `^` results, no `^` fields and no `^`
type arguments. Every value is still plain and garbage-collected;
`^` changes only where a value may go. The mode is checked per function
body against signatures, in one linear pass.

**Parallelism, in three stages.**

- **Stage A, Wasm today.** A worker is a separate Wasm instance of the
  same build: a Web Worker in a browser, a thread with its own `Store`
  under wasmtime. Messages are boundary-safe values, copied through the
  existing `Serialize`/`Deserialize` boundary codecs. Large byte data
  lives in shared linear memory (the Wasm threads proposal, shipped), is
  passed by handle, and is synchronized with atomics or handed over by
  transfer. The "Sendable" rule is the existing boundary-safe rule; no
  new trait is needed.
- **Stage B, now, single thread.** `^` lets cooperative children of
  `all!`, `map_all!`, `retry!` and `timeout!` borrow their parent's
  locals safely, and gives the optimizer guaranteed closure inlining.
- **Stage C, when engines share GC objects.** Structured parallel
  children may read the parent's data without copying when it is
  `Frozen` (deeply immutable), and write only what they allocate. This
  waits for the shared-everything-threads proposal, which is at phase 1.

The idea in one sentence: **`^` says a value stays in this call; copies
and frozen data say how it crosses threads.**

## 3. Local Values

### 3.1 What `^` Means

`^` keeps the landed spelling and the landed lending rules: any value of
type `T` may be passed where `^T` is expected, and `^mut T` needs a `mut`
view (the `mutable-upgrade` rule). No mark appears at the call site. What
changes is the meaning: a `^` value has no lifetime that travels, only
the promise that it stays inside the call.

```text
data Order:
    id: u64
    total: i64
    lines: List[Line]

fn subtotal(o: ^Order) -> i64:
    let sum: i64 = 0
    for line in o.lines:              # the list read through `^` is a plain readonly view
        sum = sum + line.price
    sum

fn audit(o: ^Order, log: mut List[Order]) -> void:
    log.push(o)                       # error: local-escapes: `o` is local to this call
```

A field read through `^` gives a plain readonly value, and through
`^mut` a plain `mut` view of a `mut` field. This is shallow on purpose:
the collector keeps every part alive, so a part can escape safely, and a
getter such as `fn address(u: ^User) -> Address` keeps working. OxCaml
chose the other default, where parts of a local record are local unless a
field is marked `global_`, because it allocates parts on the stack. Hd's
first gain is the outer object and the closure, so shallow costs nothing
it needs (question 2).

### 3.2 Local Closures

A closure literal that captures a `^` value is local. So is a closure
passed where a `^fn` is expected. A local closure can be called any
number of times and passed to other `^fn` parameters; it cannot be stored
or returned.

```text
pub fn sorted_by(self, compare: ^fn(T, T) -> Ordering) -> List[T]
pub fn retry![T, E, $R](times: usize, when: ^fn(E) -> bool, attempt: ^mut fn!() -> Result[T, E] $ R) -> Result[T, E] $ R
pub fn map_all![T, U, $R](items: List[T], limit: usize, work: ^fn!(T) -> U $ R) -> List[U] $ R
```

There are two closure modes. They follow the owner's decision of
2026-10-17 that a closure capturing `^mut` is called any number of
times; the streaming block rule that applied it lapsed with ownership,
and `^` would bring it back:

- `^fn` may capture plain values and `^` values. Several calls may be in
  flight at once, so `map_all!` takes it.
- `^mut fn` may also capture a `^mut` value. While the closure lives, the
  place it borrowed is lent exclusively, and its calls run one at a time,
  so `retry!` and `timeout!` take it. A `^fn` is accepted where a
  `^mut fn` is expected, and a plain `fn` is accepted for either.

```text
fn sync!(conn: ^mut Conn, rows: List[Row]) -> Result[void, NetError]:
    retry!(3, attempt = fn!() -> Result[void, NetError]: conn.send!(encode(rows)))
    # the closure captures `conn` mutably: it is a `^mut fn!`, which `retry!` accepts

fn fan!(conn: ^mut Conn, ids: List[u64]) -> List[Row]:
    map_all!(ids, 4, fn!(id: u64) -> Row: conn.get!(id))   # error: local-escapes: a `^mut` capture where `^fn!` is expected
```

### 3.3 What It Buys

**Checking (need 1).** The body of `subtotal` is checked against its own
signature only. A caller checks against `subtotal`'s signature only.
Nothing is inferred across functions, so every body checks in parallel,
and a body edit rechecks only that body. Go shows the alternative: its
compiler infers escape facts per function and exports them as tags, so
callers in other packages use them without annotations. That is modular
too, but inferred: a callee's body edit changes its tags and invalidates
its callers' code. Hd's interface hash covers written signatures only,
so a written mark fits the cache and an inferred tag does not.

**The Wasm GC backend (need 3).** Wasm GC has no stack allocation:
every `struct.new` is a heap object. The gains are therefore of two
kinds.

- *Closure literals at `^fn` parameters.* `hd_opt` already specializes a
  callee for a known closure literal (`closure_spec`), but it must still
  build the closure's environment when the callee might store the
  closure. With `^fn`, the callee cannot store it, so the specialized
  instance takes the captures as extra parameters and no environment is
  built. `xs.map(fn(x): x * k)` allocates the result list and nothing
  else. A `fn!` closure still runs inside a heap frame, since every
  suspending body has one; `^` saves only its environment.
- *Scalar replacement after inlining.* A data value built in a body and
  passed only to `^` parameters never escapes. When those callees are
  inlined, its fields become Wasm locals and the `struct.new` disappears.
  Without inlining, Wasm GC still needs a reference to pass, so the
  object stays. `hd_opt` declares `escape` and `scalar_replace` and has
  not built them; `^` gives them a fact they would otherwise have to
  infer.

**A future native backend.** A value passed only to `^` positions can
live on the stack, and a local closure's environment too. This is
OxCaml's main use of `local`.

**Concurrency in one thread (stage B).** A child of `all!` or
`map_all!` may borrow its parent's locals, because a child finishes or
is cancelled before the combinator returns. Two children cannot both
hold a `^mut` of one place. This is slice 11 of the prototype plan
(loans inside a body), limited to one body. It covers the place that was
lent, not other plain paths to the same object, so it checks the code in
view and promises nothing about the heap.

### 3.4 What It Costs

**Checker cost.** One forward pass per body marks each value local or
not and rejects a local value at a return, a store, a plain argument or
a capture by a non-local closure. The `^mut` exclusivity check is the
loan pass of the prototype plan's slice 11, but a loan now lives only
for one call or for the live range of one local closure, so a body has
few loans. Priced with the cost model:

| Dimension | Cost |
| --- | --- |
| Granularity | per body, inline with today's body check |
| Contention | none: reads only callee signatures |
| Locality | one walk over the body's TIR |
| Allocation | one bit per value, one small loan set per local |
| Superlinear risk | loans times instructions in bitsets; few loans per body; body fuel bounds it |
| I/O | none |
| Incremental | `^` is in the interface hash; a body edit rechecks only that body |

The lifetime slices that remain in the prototype plan (lifetimes across
calls, lifetime parameters on data, drop check, mode substitution in
generics) all disappear.

**What programs can no longer write.**

- *A borrowed view inside a struct.* Rust's `Parser<'a>` holding `&'a
  str`, or a `Cursor` holding `^mut Buffer`, has no form. The struct
  holds a plain field instead, which the collector keeps alive, and loses
  the promise that the view does not escape. Or each method takes the
  document as a `^` parameter.
- *A function returning a borrow.* `fn first(xs: ^List[T]) -> ^T` has no
  form. Since parts read through `^` are plain, `-> T` works and costs
  nothing at run time.
- *A reader owning its stream.* Rust's `BufReader` owns its inner reader
  by value. In hd a `LineReader` holds a plain stream field and implements
  `Disposable` by disposing it. This is the same shape as Rust's, with a
  runtime `Closed` in place of a move error if the stream is used after
  the reader takes it. Nothing is lost that the owner's decision did not
  already give up.
- *A lazy iterator over a local closure.* `xs.iter().map(f)` stores `f`
  inside the iterator, so `Iterator.map` keeps a plain `fn` and its
  closure is allocated. Rust removes that cost by monomorphizing and
  inlining, not by escape modes; hd's optimizer can do the same later.
- *std's own `map_all!`.* Its body captures `work` in closures that it
  pushes onto a `List` before calling `all_list!`. With `work: ^fn!`,
  that list holds local closures, which the rules forbid. `map_all!`
  needs a different body, for example a fixed number of worker calls
  passed straight to an `all!`-style intrinsic that accepts local
  children. Every std function that takes `^fn` must be checked for this.

**Migration.** `^` is opt-in, so no user program changes. A std
signature that moves from `fn` to `^fn` breaks no caller, because every
`fn` value is accepted. It breaks only a std body that stores the
closure, as `map_all!` does. The prototype's `own`, move and drop
fixtures are removed (section 7).

### 3.5 Prior Art

| Language | Mechanism | What hd takes |
| --- | --- | --- |
| OxCaml | `local` mode on parameters and values; checked per function; inferred inside a file, written in `.mli`; local values go on the stack; local closures too | the whole model, minus deep locality of parts and minus `exclave_` (local results) |
| Swift | closure parameters are non-escaping by default, `@escaping` to store; `borrowing`/`consuming` parameter modifiers; `~Escapable` types with lifetime dependence | non-escaping closure parameters; not the default (question 3); not `~Escapable`, which needs lifetimes |
| Kotlin | `inline` functions inline their lambda arguments; a lambda that is stored must be `noinline` | the guarantee that a literal at such a parameter is inlined, without making the callee itself inline |
| Hylo (Val) | second-class references: a `let` or `inout` parameter cannot be stored or returned, so no lifetimes are needed | the core claim: projections that never escape need no lifetime names |
| Go | escape analysis inferred per package, exported as tags; no annotations | the reason to write the mark: inferred facts change with bodies |

## 4. Sharing Data Between Threads

### 4.1 What scriptc Does

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
It relies on a native heap where any object can become shared in place.
Hd on Wasm does not have that heap, which is why the stages below exist.

### 4.2 Stage A: Hd On Wasm Today

| scriptc | Hd on Wasm today | Cost |
| --- | --- | --- |
| a worker thread with its own heap | a separate instance of the same module: a Web Worker in a browser, a thread with its own `Store` in wasmtime | an instance per worker, with its own GC heap and globals; start-up is instantiation plus module init |
| a structured clone per message | a boundary-safe value, encoded by its `Serialize` and decoded by its `Deserialize` through the host boundary codecs | one encode, one copy, one decode per message: linear in size |
| a transfer list | a `SharedBytes` handle that the send marks closed in the sender; later use is the runtime `Closed` error | constant |
| `SharedArrayBuffer` + `Atomics` | a block of shared linear memory (Wasm threads: `memory.atomic.*`, `wait`, `notify`), imported by every instance and reached through a `SharedBytes` handle | no copy; plain loads and stores, atomic ones where asked |
| `publish` of an immutable graph | none without copying: GC objects cannot cross instances | a copy per worker, or an encoded image in shared memory (example 5.3) |

The message rule is the boundary-safe rule that the spec already has:
primitives, strings, tuples, lists, maps, data and enums whose parts are
boundary-safe; never mutable types, closures, trait values or live
handles. Issue #166's `Boundary` bound states it in a generic signature,
and its `Portable[fn]` closures, whose captures are boundary-safe, are
how a worker learns what code to run: a worker is the same build, so the
closure travels as a content identity plus encoded captures. A
`SharedBytes` handle is the one new boundary-safe value: an offset, a
length and a generation into shared memory.

Wasm threads are at phase 4 and shipped in every major browser, which
enable shared memory only on cross-origin-isolated pages; wasmtime
supports them at tier 2. A module can use Wasm GC for its
managed values and a shared linear memory beside it, so stage A needs no
new engine feature. It needs host work (spawning instances, queues) and
a std module, both of which need the owner's approval before a spec
pass.

### 4.3 What Waits For Shared-Everything Threads

Sharing a GC object between threads needs the shared-everything-threads
proposal. It is at phase 1, its overview warns of frequent and
significant changes, and wasmtime lists it at tier 3 with nothing
implemented (tracking issue 9466). It adds `shared` struct, array and
function types, thread-local globals, atomic access to shared GC fields
and managed waiter queues.

One rule in it shapes hd's design: shared and unshared types are never
subtypes of one another, and a shared object can point only to shared
objects. So hd cannot freeze an ordinary object in place and hand it to
another thread, as scriptc's `publish` does. A value becomes shareable
only by being allocated as `shared` from the start, or by one deep copy
into `shared` types. Either way the compiler must know the type at
allocation, which favors an explicit `freeze` that copies.

### 4.4 Stage C: Structured Parallel Children

The later step is a parallel combinator whose children finish before the
parent continues, as Rust's `thread::scope` and OxCaml's `fork_join`
do. It is tempting to say that `^` makes this safe, since a child cannot
keep a borrow past the call. On a garbage-collected heap that is not the
hazard. A child that keeps a reference keeps the object alive; nothing
dangles. The hazard is a data race, and it comes from aliasing:

- a `^List[Loan]` lent to four children is safe only if nobody writes
  the list or a loan while they run. Plain values alias freely, so some
  other plain path may hold a `mut` view of the same loan.
- the parent's own thread keeps running its other cooperative tasks while
  the parent waits, and those tasks may hold such a path.
- two `^mut` chunks of one list are disjoint slots, but two slots may
  point to the same object.

`^` and `^mut` cannot see any of this, because plain aliases are
untracked by design. So the stage C rule is about mutation, as OxCaml's
`contended` mode is: a parallel child may read the parent's data only
through `Frozen` values (deeply immutable), may write only data it
allocates, and returns its results. `^` still matters for a native
backend, where a child may read the parent's stack, and for the
exclusive `^mut fn` closure, which stage C never sends to a parallel
child.

One more problem waits for stage C: top-level mutable `let` bindings.
Today each instance has its own, so stage A has no issue. With one
shared heap, a child that calls a function writing a top-level `let`
races with its siblings. The choices are thread-local globals (as
scriptc's per-thread module state; a child's write is then invisible to
the parent) or a rule that a child's code never writes a global, which
needs a fact about whole call graphs. That choice is left for when the
proposal matures.

### 4.5 A Sendable Rule In Hd

- **For copying (stage A):** the boundary-safe rule that exists, spelled
  `Boundary` in a generic bound (#166). It is Swift's `Sendable` for
  values that are copied, and Erlang's rule that every message is a copy.
- **For sharing without copying (stage C):** a `Frozen[T]` value, built
  by `freeze(x)`, which deep-copies a boundary-safe `x` into objects that
  no `mut` view can ever reach. The parked deep-immutability idea in
  [Open Issues](OPEN_ISSUES.md) describes the property. It is scriptc's
  `publish`, checked statically, and freed by the collector instead of
  being immortal.
- **No `Sync`-style trait for mutable shared state.** Shared mutable
  state is bytes with atomics, or a host service. A mutex-protected GC
  object waits for stage C and a separate study.

### 4.6 How Others Share Data

| System | Isolation | Sharing without copying | Static rule |
| --- | --- | --- | --- |
| scriptc | heap per thread; messages cloned | `publish`ed immutable graphs, never freed; `SharedArrayBuffer` | none for messages; `Immutable<T>` checks writes it can see |
| JS workers | heap per worker; structured clone | `SharedArrayBuffer` + `Atomics`; transfer moves buffers and ports | none |
| Erlang | heap per process; messages copied | binaries over 64 bytes are reference-counted off-heap and shared | none needed: data is immutable |
| Go | none: one shared heap | everything | none; a race detector at run time |
| Swift | actors own their state | `Sendable` values; `sending` moves a disconnected region | `Sendable` conformance; region analysis per function |
| OxCaml | none: one shared heap | `portable` functions, `contended` values; immutable data crosses both modes | modes checked per function; `fork_join` needs portable children |
| Hd, stage A | instance per worker; messages copied | `SharedBytes` + atomics | boundary-safe messages |
| Hd, stage C | one shared heap | `Frozen` values; children allocate their own writes | `Frozen` for shared reads; `^` only bounds where values go |

## 5. A Tour

Each example gives its cost on today's Wasm (stage A) and, where it
changes, under stage C. `Workers`, `Sender`, `Receiver`, `SharedBytes`,
`SharedSlot`, `Frozen` and `freeze` are placeholders.

### 5.1 A Parallel Map Over A Large List

A bank scores a million loans against one risk model. Scoring is pure
and takes about a microsecond per loan.

```text
use std.remote.{Portable, RemoteError, portable}

pub data Loan:
    pub id: u64
    pub principal: i64
    pub rate_bp: i32

pub data RiskModel:
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

### 5.2 A Worker Pool Processing Jobs

An image service resizes uploads. Each job carries its bytes and a
target size; workers keep a decoder cache of their own.

```text
pub data Job:
    pub id: u64
    pub image: SharedBytes
    pub width: u32

pub data Done:
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

### 5.3 A Shared Read-Mostly Cache

A pricing service looks up exchange rates on every request, from four
workers. Rates change once a minute.

Stage A gives three options:

| Option | How | Cost |
| --- | --- | --- |
| a copy per worker | each worker builds or receives the table; a refresh is a message to each | memory times four; a refresh copies the table four times; lookups are local and free |
| a host service | a `RateCache` capability whose host keeps one table behind a lock or sharded map | one boundary crossing and a copy-out per lookup; one copy of the data |
| an encoded image in shared memory | the writer encodes the table into a `SharedBytes` block and publishes its handle; readers decode the entry they need | lookups read bytes in place and decode one entry; a refresh writes a new block and swaps the handle atomically; freeing the old block needs a reader count or an epoch |

```text
fn refresh!(rates: Map[string, f64], current: ^mut SharedSlot) -> void:
    image := SharedBytes::encode(rates)       # one encode, into shared memory
    current.swap(image)                       # an atomic store of the handle

fn price(amount: i64, currency: string, current: ^SharedSlot) -> f64:
    rate := current.load().lookup::[f64](currency).unwrap_or(1.0)   # decodes one entry
    f64(amount) * rate
```

The image option is how scriptc's `InternTable` would look without a
shared GC heap: bytes are shared, values are decoded on read. Under
stage C, the table becomes `Frozen[Map[string, f64]]` behind an atomic
reference, and lookups read it directly.

### 5.4 A Large Byte Buffer Handed Between Threads

A log shipper reads 64 MB chunks, and a compressor on another worker
compresses them.

```text
fn ship!(source: ^mut FileReader, to: Sender[SharedBytes]) -> Result[void, FsError] $ FsRead:
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
  in either GC heap.
- **Synchronization.** One queue operation per chunk. After the send,
  the sender's handle is closed, so a later use is the runtime `Closed`
  error, as for a disposed handle. Two workers that both write one block
  need atomics; that is a different API (`SharedBytes::atomic_*`).
- **Cleanup.** `using` disposes the sender's handle, which does nothing
  after a transfer. The receiver disposes the block when done; a block
  left undisposed is a leak report, as for any handle.

## 6. The Rules In Plain Words

**Local values**

1. `^T` and `^mut T` may be written on a parameter, a receiver, a local
   binding and a function type. They may not be written on a result, a
   field, a type argument or a top-level binding.
2. Any value of type `T` may be passed where `^T` is expected. `^mut T`
   needs a `mut` view.
3. A local value may be read, called, matched, iterated, and passed to a
   `^` parameter.
4. A local value may not be returned, stored into a field, a collection,
   a top-level binding or a binding with a plain type, or passed to a
   plain parameter. A `:=` binding of a local value is local.
5. A part read through `^T` is a plain readonly value; through `^mut T`,
   a `mut` field gives a plain `mut` view.
6. A closure literal that captures a local value is local. A local
   closure may be called any number of times.
7. A closure that captures a `^mut` value is a `^mut fn`. It is accepted
   only where a `^mut fn` is expected, and its calls run one at a time.
8. While a `^mut` value is lent, to a call or to a live local closure,
   the place it came from is not read, written or lent again.
9. A plain call of a suspending function with a local argument builds a
   local suspension, which may only be awaited or passed to a combinator
   that accepts local children.
10. A local value is not boundary-safe and is never a provider.

**Threads, stage A**

11. Each worker is a separate instance; instances share no GC values.
12. A message is a boundary-safe value, copied.
13. A `SharedBytes` handle is boundary-safe; sending it with transfer
    closes the sender's handle.
14. Shared memory is reached only through `SharedBytes`, with atomic
    operations where they are asked for.

## 7. The Ownership Prototype: Keep Or Remove

Slices 1 to 10 of [Ownership Prototype Plan](OWNERSHIP_PROTOTYPE.md) are
committed. Slice 11 (loans inside a body) is not; a draft was set aside
when ownership work went on hold.

| Part | Where | Verdict | Why |
| --- | --- | --- | --- |
| `^T`, `^mut T`, `^self`, `^mut self` syntax | `hd_syntax` types and items | keep | the local mode's spelling |
| `own T`, `let own`, `own self`, `own fn` syntax | `hd_syntax`, `hd_fmt` | remove | no owned mode |
| lifetime syntax `^{x}`, `data P[^a]`, `[^]` | `hd_syntax`, `hd_resolve` lower | remove | no lifetimes |
| `T < own Any` bounds | `hd_syntax`, `hd_types` | remove | no owned type arguments |
| `TyData::Borrow` | `hd_types` pool | keep, without its lifetime list | the local mode's type form |
| `TyData::Own` | `hd_types` pool, wire | remove | |
| header rules: `own-on-value`, `own-on-type-parameter`, `missing-lifetime`, `ambiguous-lifetime`, `misplaced-lifetime` | `hd_check` header, own.rs | remove | |
| header rule for where `^` may appear | `hd_check` header | rewrite | rule 1 of section 6; replaces `not-in-ownership-prototype` for top-level and result borrows |
| `OwnOnly`, `DropOwn`, `drop` | `lib/std/ownership.hd`, `hd_resolve` known names | remove | `Disposable` replaces them |
| ownership by structure, the classifier | `hd_check` own.rs (`Classify`) | remove | |
| freshness, `own-upgrade`, `own-to-plain` | `hd_check` modes.rs | remove | |
| lending, `mutable-upgrade`, field reads through a borrow | `hd_check` modes.rs | keep; field reads become plain (section 3.1) | |
| `Lend`, `LendMut` coercions | `hd_tir` | keep | they mark where a value is lent |
| move checking, `use-after-move`, `partial-move`, the `moves` column | `hd_check` moves.rs, `hd_tir` | remove | no moves |
| `DropLocal` in the scope list and the exit ladder | `hd_tir`, `hd_wasm` emit/drops.rs | keep the ladder for `using`; remove flags and `drop_mode` | `using` disposes at scope end in reverse order, which is the same registration |
| `DropTemp`, temporaries | `hd_check` temps.rs, `hd_tir` | remove | `using` names its value |
| drop glue | `hd_mono` drops.rs, `hd_structure` drop.rs, `hd_wasm` glue bodies | remove | `dispose` is one ordinary method call |
| loans inside a body | slice 11, not committed | build, limited to one call or one closure's live range (job L3) | `^mut` exclusivity |
| `provider-not-plain`, `not-boundary-safe` for borrowed types | `hd_check` | keep the borrowed half | rule 10 |
| prototype limits (`not-in-ownership-prototype`) | `hd_check` init.rs, modes.rs | remove; the cold-call limit becomes rule 9 | |
| spec chapter | `spec/lang/13-ownership.md` | rewrite as a short chapter on local values | |
| conformance fixtures | `spec/conformance/*/ownership/` | keep the lending and loan cases; remove owned, move, drop and lifetime cases; turn the drop-order runtime cases into `using` cases when `using` lands | |

## 8. Re-Plan

Slices 12 to 23 of the prototype plan map as follows.

| Old slice | Under this model |
| --- | --- |
| 12 Lifetimes across calls | removed: no `^` results, so no loan crosses a call |
| 13 Two-phase receivers | kept, as job L6, if std adopts `^mut self` receivers |
| 14 Lifetime parameters on data | removed: no `^` fields |
| 15 Drop check | removed: no drops |
| 16 Patterns and guards | removed with moves; matching a local value is part of L2 |
| 17 Owned type arguments | removed |
| 18 Generic bodies for every mode | removed: no `^` type arguments, so a type parameter is always plain |
| 19 Closures | replaced by L3 |
| 20 `own dyn` | removed; `^dyn K` parameters already work |
| 21 `Option.take` | removed |
| 22 Suspending bodies | replaced by L4 |
| 23 Tour and cost report | replaced by L8 |

The jobs, each about an hour, one feature each, in order. Each ships its
spec rules and fixtures in the same job, by the owner's standing order,
and runs the footprint guard where it touches codegen. None starts before
the owner approves the model.

| Job | Work | Done check |
| --- | --- | --- |
| R1 | Remove the owned mode: `own` syntax, `TyData::Own`, freshness, `own-upgrade`, `own-to-plain`, moves and their fixtures and spec sections | no `own` in syntax, types or spec; hello unchanged |
| R2 | Remove drops: `OwnOnly`, `DropOwn`, `drop`, the classifier, drop glue, `DropTemp`, drop flags; keep the `DropLocal` ladder unused | std interface hash changes once; tiny-program footprint byte-identical |
| R3 | Remove lifetimes: `^{x}`, lifetime parameters and arguments, their header rules | lifetime fixtures gone; parse snapshots updated |
| L1 | Placement: `^` only on parameters, receivers, locals and function types (rule 1) | one invalid fixture per forbidden place |
| L2 | The escape check: `local-escapes` at return, store, plain argument and capture; shallow part reads; matching | fixtures for each escape and for the getter that returns a part |
| L3 | Local closures and loans: `^fn` and `^mut fn` types, capture rules, repeat calls; loans of `^mut` arguments and of live local closures | `sync!` accepted, `fan!` rejected, as in section 3.2 |
| L4 | Suspending bodies: local arguments of cold calls, local children of `all!` and `race!`, two `^mut` children of one place rejected | fixtures from the old slice 22 without owned values |
| L5 | std: `^fn` on eager callbacks (`List.map`, `filter`, `sorted_by`, `sorted_by_key`, `retry!`, `retry_with!`, `timeout!`, `map_all!`), with `map_all!`'s body rewritten | std golden TIR updated once; every std test passes |
| L6 | Two-phase receivers, if L5 adds `^mut self` receivers | `xs.push(xs.len())` accepted |
| L7 | Optimizer: closure literals at `^fn` parameters build no environment | a test counts zero closure `struct.new` in `xs.map(fn(x): x * k)`; footprint and timing reported |
| L8 | Tour and cost report: the section 3 examples as fixtures; check time of the 10k-line package before and after | plain check time within noise |

`using` and `Disposable` are a separate design job, already decided by
the owner; R2 leaves the exit ladder for it. Stage A (workers, messages,
`SharedBytes`) is std and host work that needs its own proposal and the
owner's approval; it does not depend on the L jobs. Stage C is a parked
issue until shared-everything threads reach phase 3.

## 9. Questions For The Owner

1. **Spelling.** Keep `^T` and `^mut T` for the local mode, with the new
   meaning "stays in this call", or use a word such as `local T`?
   Recommended: keep `^`. It is landed, short, already means "not plain"
   to readers of the prototype, and the owner's `^mut` closure decision
   carries over unchanged.
2. **Where `^` may appear, and how deep.** `^` only on parameters,
   receivers, locals and function types, never on results, fields or type
   arguments, and parts read through `^` are plain. Recommended: yes.
   Results, fields and type arguments are what bring lifetimes back, and
   deep locality only pays for stack-allocating parts, which Wasm GC
   cannot do.
3. **Closure parameters by default.** Should a closure parameter be local
   unless marked, as in Swift, or plain unless written `^fn`?
   Recommended: written `^fn`. It matches "if a thing is local, always
   show it in source", and changes no existing program.
4. **Stage A first.** Should real parallelism start with separate
   instances, copied boundary-safe messages, and `SharedBytes` over shared
   linear memory, using #166's `Portable` closures to name worker code,
   with the boundary-safe rule as the only "Sendable" rule?
   Recommended: yes. It runs on engines that ship today, and every later
   stage keeps it.
5. **Stage C parked.** Should sharing GC data between threads (`Frozen`,
   structured parallel children, globals under threads) wait as a P4
   issue until shared-everything threads reach phase 3? Recommended: yes.
   The proposal is at phase 1 and its type rules decide hd's `freeze`.
6. **The prototype.** Remove the owned mode, moves, drops, `OwnOnly`,
   `DropOwn` and lifetimes; keep `^` lending and the loan check; keep the
   exit ladder for `using`; run jobs R1 to L8 in place of slices 12 to 23?
   Recommended: yes. The removed parts served resources, which
   `Disposable` now serves.

## Sources

- scriptc documentation index: <https://scriptc.dev/llms.txt>; sharing
  data between workers: <https://scriptc.dev/docs/threads>
- OxCaml stack allocation (locality): <https://oxcaml.org/documentation/stack-allocation/intro/>
- OxCaml parallelism tutorial (portable, contended, `fork_join2`):
  <https://oxcaml.org/documentation/tutorials/intro-to-parallelism-part-1/>
- Swift closures, escaping closures:
  <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/closures/>
- Swift SE-0377, `borrowing` and `consuming`:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md>
- Swift SE-0446, non-escapable types:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md>
- Swift SE-0302, `Sendable`:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md>
- Swift SE-0414, region-based isolation:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0414-region-based-isolation.md>;
  SE-0430, `sending`:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md>
- Kotlin inline functions: <https://kotlinlang.org/docs/inline-functions.html>
- Hylo: <https://hylo-lang.org/>; Racordon et al., "Implementation
  Strategies for Mutable Value Semantics", JOT 2022:
  <https://www.jot.fm/issues/issue_2022_02/article2.pdf>
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
