# Rust's Memory Model For hd: Design Study

Status: decided (#163). hd does not adopt Rust's ownership. Objects live in
linear memory under reference counting with cycle detection
([Refcount Runtime](REFCOUNT_RUNTIME.md)), values copy by default, and
opt-in lifetimes are parked (#163).
The study below prices Rust's model against the Wasm GC design of
2026-10-18; its measurements stay as reference.

The study assumes that design: every composite value is a reference on the
Wasm GC heap, `mut` is an access permission that any number of aliases
may hold, and the engine's collector frees what nothing reaches.
The owner asked on 2026-10-18 whether hd should instead adopt Rust's
model: owned values, moves, borrows with lifetimes and destructors, no
tracing collector, with hd's requirement rows, providers, suspension,
derivations and facts on top. And if so, whether a `Gc[T]` type could
replace Rust's `Rc<RefCell<T>>` where programs need shared data.

This study is for the owner, to decide the memory model before more is
built on the current one. It prices the change by allocation, copies,
code size, compile time and checker complexity, and writes three
realistic programs three ways, counting what an agent would write and
the errors it would likely hit.

**The answer in short.** Do not adopt Rust's model. Rust's
model would buy three real things: shared-memory threads on Wasm today,
SIMD and zero-copy host I/O over bytes, and allocation-free records on
a stack. It would cost a second language's worth of spec, std and
compiler work, and every program would pay for it in annotations and
borrow errors, the part of Rust that its own users and a controlled
trial name as the hardest. Most of the three gains have cheaper routes
that are already proposed. If the owner adopts ownership anyway, the
`Gc[T]` that fits Wasm is one whose contents live on the Wasm GC heap
with today's semantics. That `Gc[T]` needs no `RefCell` and no borrow
flags, but it splits the language into two worlds.

This is the study for issue #163. Every spelling is a placeholder;
`text` blocks are proposed syntax that no compiler checks, and `hd`
blocks pass `hd check`. Line counts are from 2026-10-18; numbers marked
"estimate" are not measurements.

## 1. Why Ask Now

Jobs R1 to R3 removed the opt-in ownership prototype on 2026-10-18;
`^` remains only as a borrow mark on parameters and bindings. That
prototype layered ownership on the collector, so it bought resource
tracking, which `Disposable` and `using` now give at run time. Rust's
model removes the collector as the default and moves owned values off
the GC heap. Three pressures make it worth pricing:

- **Parallelism.** Wasm GC cannot share a reference between threads
  until shared-everything threads mature; that proposal is at phase 1.
  Shared linear memory with atomics has shipped. A language whose
  values live in linear memory can share them across threads today.
- **Speed.** No compiler pass removes an allocation today, and
  allocation per element is the largest open cost
  ([Performance Roadmap](PERFORMANCE_ROADMAP.md) §1). SIMD cannot load
  from a GC byte array, so scans stay a byte at a time
  ([SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md) §3).
- **Cleanup.** Use after close and a forgotten handle are runtime
  reports today, not compile errors.

hd is written mostly by coding agents
([Direction](README.md#direction)), so the cost that matters most is
what every program must say and how often the first attempt fails.

## 2. Rust's Model In hd

### 2.1 The Model On One Page

Under this model, every value has one owner. Passing or binding a
value moves it, and using it after the move is an error. Scalars and
types that implement `Copy` are copied instead. A second owner needs an
explicit `clone()`. A borrow `^T` reads a value it does not own, and
`^mut T` changes one; a value may have many `^T` borrows or one
`^mut T`, never both. Every borrow has a lifetime. A signature names
lifetimes where elision cannot supply them: a result borrows from the
only borrowed parameter, or from `^self`. When the owner's scope ends,
the value is dropped, and its type's `Drop` runs. Threads share only
`Sync` values, and only `Send` values cross to a thread.

```text
struct Order:
    id: u64
    lines: List[Line]                             # owned: the list dies with the order

fn total(o: ^Order) -> i64:                       # borrows; the caller keeps `o`
    let sum: i64 = 0
    for line in o.lines:                          # iterates through a borrow
        sum = sum + line.price
    sum

fn first_line[^a](o: ^{a} Order) -> ^{a} Line:    # elision would allow `-> ^Line`
    o.lines[0]

fn archive(o: Order) -> void: store(o)            # takes ownership

fn demo() -> void:
    o := load_order(7)
    n := total(o)                                 # lends `o`; no mark at the call
    archive(o)                                    # moves `o`
    m := total(o)                                 # error: use-after-move
```

hd's `^` spelling and its call-site rule carry over: lending marks
nothing at the call (the Lending section of the former
`spec/lang/13-ownership.md`, removed in #163), so `total(o)`
reads the same whether it borrows or not. The rest is new.

| Area | Today | Rust's model in hd |
| --- | --- | --- |
| Composite values | shared references; aliases are free | owned, moved, or borrowed |
| `mut` | an access permission on any alias | `^mut`: one exclusive borrow; the owner may mutate |
| Strings | immutable shared slices, Go's model | an owned buffer and a borrowed `^str` slice with a lifetime, or immutable reference-counted values |
| Closures | one `fn` type; captures are shared | Rust's three kinds (called once, called with exclusive access, called shared), each a type |
| Iterators | `Iterator[T]` holds a closure over shared state | an iterator borrows its collection: `ListIter[^a, T]` |
| Cleanup | `using` with `Disposable`; leaks are runtime reports | `Drop` at scope end; `using` becomes an explicit early drop |
| Threads | separate instances, copied messages | `Send` and `Sync`; scoped threads borrow |
| A graph | nodes point at nodes | indices into an arena, `Rc` with `Weak`, or `Gc[T]` (section 3) |

The closure row is the largest surface change. hd decided against a
mutable closure type because "hd has no unique borrows, so a mutation
capability on function types would protect nothing"
([Mutable Closures](../../spec/lang/07-functions.md#mutable-closures)).
Under exclusive borrows that reason reverses, and Rust's `FnOnce`,
`FnMut` and `Fn` distinction comes back.

### 2.2 Suspension And Lifetimes

Rust's async has four well-known pains. hd's design avoids two of
them at a price, inherits two, and adds one of its own.

**Pin: avoided, with eager frames.** A Rust future is a value nested
inside its parent's future, with no allocation per level. A future that
borrows its own local across an `.await` points into itself, so it must
not move after its first poll, which `Pin` enforces. hd never nests
frames by value: a frame is a heap object, allocated on the first real
wait, and `Suspend[T]` is a reference to it
([crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md) §14.3). A
frame never moves, so hd needs no `Pin` in its surface. The price is
in where locals live. On the ready path they are Wasm locals, copied
into the frame on the first wait, and a borrow taken before the wait
would point at the old place. So a body that holds a borrow of its own
local across a bang call allocates its frame on entry and keeps that
local there, giving up the ready path's zero allocation for those
bodies only.

**Send across a suspension point: avoided while tasks stay put.**
`tokio::spawn` needs a `Send + 'static` future, and holding an `Rc` or
a `RefCell` guard across an `.await` makes a future not `Send`, with an
error far from its cause. Each hd instance runs its tasks on one thread
and never migrates them
([Cooperative Scheduling](../../spec/lang/11-requirements-and-suspension.md#cooperative-scheduling)),
so frames never need `Send`. Only a stage C parallel child would, and
the rule would sit at that one combinator.

**Scoped children: inherited.** `all!(echo(a), echo(b))` may borrow
the parent's locals, since the children end inside the call, as with
Rust's `join!`. Two children may not both hold a `^mut` of one place,
which today's `map_all!` does with its `slots` and `cursor`
(`lib/std/task.hd`); under ownership it needs cells (section 3.3). The
scoped task trilemma (Without Boats, 2023) then applies: a sound API
gets at most two of concurrency with the parent, parallel children,
and borrowing without synchronization. `all!` gets concurrency and
borrowing. A stage C child that is parallel and borrows would make the
parent's thread block until the children stop, as `thread::scope`
does, stalling the instance's other tasks. It would also forbid
abandoning a started suspension without cancelling it, which today
skips its `defer` suites
([Discarding A Suspension](../../spec/lang/11-requirements-and-suspension.md#discarding-a-suspension)).

**Lending closures: inherited.** `retry!(3, attempt = fn!(): get!(url))`
calls a closure several times, and each call returns a suspension that
borrows the closure's capture. Rust could not express that until async
closures (RFC 3668, stable in 1.85). Every `fn!` parameter in std
(`retry!`, `timeout!`, `map_all!`, test bodies) has this shape, so hd
would need the lending rule on day one.

**Suspending drop: new to hd.** `Disposable.dispose!` may suspend, and
cancellation waits for it
([Using Bindings](../../spec/lang/06-control-flow.md#using-bindings)).
Rust has no stable async drop; its async initiative lists it as open
design work. If `Drop` replaces `Disposable`, every scope exit becomes
a possible suspension point, a value whose drop suspends can be owned
only in a `!` body, generic code needs a bound saying whether `T`'s
drop may suspend, and a synchronous `cancel()` cannot wait for it (the
suspending `cancel!()` of
[Cancellation](../../spec/lang/11-requirements-and-suspension.md#cancellation) waits for a dispose). The simpler choice keeps `Drop`
synchronous and memory-only, with `using` and `Disposable` for handles;
then ownership buys handles no compile-time cleanup either.

**Stored suspensions carry lifetimes.** A cold call that lends an
argument builds a suspension bounded by that borrow, so `race!`,
`block_on` and every stored suspension type write a lifetime.

### 2.3 Requirements And Providers

A provider is one value shared by a whole call tree, sibling tasks
included, and a mutable provider changes its own state
([Mutable Providers](../../spec/lang/11-requirements-and-suspension.md#mutable-providers)).
Ownership offers three readings:

| Provider is | What `$.use(K)` gives | What breaks |
| --- | --- | --- |
| borrowed for the scope (`$.with(K = ^p)`) | `^K`, bounded by the `$.with` block | `mut self` methods: sibling tasks would share one `^mut`, so providers take `^self` and keep their state in cells. A provider value cannot escape its scope, so the escape rules ([Provider Values](../../spec/lang/11-requirements-and-suspension.md#provider-values)) go. A cold suspension's providers bound its lifetime to the scope ([Construction-Time Requirement Binding](../../spec/lang/11-requirements-and-suspension.md#construction-time-requirement-binding)) |
| owned by the scope | `^K` again; the scope drops it at its end | the same, plus providers that live past the scope need cloning |
| a shared managed value (`Gc` semantics, section 3) | today's value, with today's permission | nothing: the provider keeps object semantics |

Only a design with `Gc[T]` has the third reading, which changes
nothing in chapter 11. Pure Rust gives the first, rewriting mutable
providers, escaping provider values and stored suspensions. Rows
themselves (`$ R`, row parameters, entailment) say which keys a call
needs, not how values are held, and carry over unchanged.

### 2.4 Derivations And Facts

These fit with small changes. A fact becomes a lazily initialized
static, read as `^{static}`; handles become `Copy`; `walk` and
`describe` take `^self`; `build` returns an owned value that `missing`
sources move members into. The `Structure` generator (`hd_structure`,
2.0k lines) must emit move-correct bodies. `Clone` becomes the
derivation nearly every data type carries, as `#[derive(Clone)]` is in
Rust.

### 2.5 What It Costs The Checker

Borrow checking is per body against written signatures, like the rest
of hd's checking, and lifetimes go in the interface hash.

| Dimension | Cost |
| --- | --- |
| Granularity | per body, after type checking |
| Contention | none: reads callee signatures only |
| Locality | a dataflow over each body's places and control flow |
| Allocation | a set of live loans per program point |
| Superlinear risk | loans times points in one body; a long body with many borrows is quadratic (estimate) |
| I/O | none |
| Incremental | lifetimes are in the interface hash; a body edit rechecks that body |

The size is the cost. The removed prototype checked marks, lending,
moves and drops but never built loans; its removal commits deleted
about 8,100 lines of code, spec and fixtures (`a04c3497c`, `8a77d0273`,
`101344d4c`). A complete checker adds loans, two-phase borrows,
reborrows, closure capture analysis, drop elaboration with drop flags,
and Rust's years of diagnostics work. Estimate: 10k to 15k new lines in
`hd_check`, against its 31.2k today.

## 3. Shared Data Without Rc And RefCell

### 3.1 The Four Shapes

Ownership is a tree. A graph, a cache that hands out entries, an
observer list and a tree with parent links are not. Rust writes them
with indices into an arena (`petgraph`, `slotmap`), or with
`Rc<RefCell<T>>` and `Weak` back edges. `RefCell` checks the
exclusive-borrow rule at run time and panics on a conflict; holding
one across an `.await` is a hazard Clippy's `await_holding_refcell_ref`
lint flags.

### 3.2 The Options

| Option | Graph and cache | Observers and parent links | Mutation | Costs |
| --- | --- | --- | --- | --- |
| Arena and indices (Rust, `slotmap`) | nodes in a `List`, edges as `usize` or generational keys | indices; a removed node leaves a stale key that a generational key detects | through `^mut` of the arena, one node at a time | no runtime cost; every access passes the arena; a stale index is a logic error, not a crash |
| `Rc<RefCell<T>>` with `Weak` (Rust) | `Rc` per node | `Weak` back edges, upgraded on use | `borrow_mut()` with a runtime flag | a count per copy and drop; a flag per access; a missed `Weak` leaks a cycle |
| Tracing `Gc<T>` in linear memory (`rust-gc`, Alloy) | `Gc` per node; cycles collected | plain `Gc` back edges | `GcCell`, a `RefCell` that tracks roots; Alloy can reuse destructors as finalizers | root tracking on every handle outside the heap; a collector in every program that uses it; `rust-gc` is single-threaded |
| Reference counting with cycle collection (Samsara, `bacon-rajan-cc`, Nim's ORC) | counted pointer; a trial-deletion pass finds cycles | plain back edges | Samsara: closure-based `get`/`set` with an owner token from `qcell`, or blocking locks | a count per copy; the cycle pass costs memory per candidate; Nim lets a type be marked `acyclic` to skip it |
| Swift ARC | class references | `weak` or `unowned`, written by hand | any reference mutates; exclusive access checked at run time for class properties and globals | a count per copy; cycles leak unless a back edge is `weak` |
| Koka Perceus | immutable data; graphs as maps of ids | no back edges | functional update, reused in place when the count is one | cycles through mutable references are not collected |
| Vale generational references | an owning reference plus non-owning ones | non-owning references, checked against a generation on use | through any reference | a generation per object and per reference; a stale use halts at run time |
| Hylo mutable value semantics | values only; graphs as arrays and indices | indices | `inout` parameters; no stored references | no lifetimes and no counts at the language level; copy-on-write needs a uniqueness test |

Every option that keeps pointers pays per access or per copy: a flag,
a count, a generation or a root. Only arenas and Hylo pay nothing, by
turning pointers into indices. The counting options leak cycles unless
someone writes `weak` or runs a cycle pass. Only a tracing collector
makes shared mutable data look like today's hd.

### 3.3 Does `Gc[T]` Need Interior Mutability?

`RefCell` protects one thing: a borrow into storage that a mutation
through another path frees or moves. `v.push(x)` reallocates a list's
buffer, and a `^T` borrow of `v[0]` taken through a second alias would
then dangle. Whether `Gc[T]` needs the flag depends on what its
contents hold:

- **Contents on a collected heap.** If every part of a `Gc` object is
  itself collected, nothing is freed while anything reaches it. A push
  that grows the buffer leaves the old buffer alive for whoever still
  reads it. Aliased mutation is then memory-safe, and that is exactly
  today's hd, whose iterators iterate a snapshot
  ([Iterator Snapshots](../../spec/lang/06-control-flow.md#iterator-snapshots)).
  So `Gc[mut T]` can give today's `mut` permission semantics with no
  flag and no `borrow_mut()`.
- **Contents that hold owned values.** If a `Gc` object can own a
  linear-memory list, two aliases can free each other's borrowed
  storage. Then `Gc[T]` needs `RefCell`'s flags, or a static token in
  the style of GhostCell, as Samsara uses through `qcell`.

hd's scheduling adds one option, since only bang calls yield. A cell
accessed through a non-suspending closure,
`stats.update(fn(s: ^mut Stats): s.record(host))`, cannot be
interrupted by a sibling task, so `RefCell`'s async failure cannot
happen; the flag still catches a re-entrant `update`. Call it
`Shared[T]`: the shared mutable state of pure Rust-model hd.

### 3.4 `Gc[T]` On Wasm GC

The cheapest `Gc[T]` uses the engine's collector: owned values in
linear memory, and `Gc[T]` values as Wasm GC references. One rule of
Wasm decides its shape. In the reference-types proposal's words,
references "cannot be stored into linear memory. Tables are used as
the equivalent." It follows that:

- **An owned value cannot hold a `Gc[T]` field directly.** The field
  becomes an index into a table of references. Each slot is a root,
  allocated when the handle is stored and freed by the owner's drop.
  That costs a table write per store and an indirection per read, and
  owned code still never sees the address.
- **A `Gc` object cannot own a linear-memory value.** Freeing that
  value when the object dies needs a finalizer, which hd has ruled out. So a `Gc` object's
  contents must be managed through and through: scalars, managed
  strings, managed lists and maps, and other `Gc` references.
- **So there are two worlds.** Owned types live in linear memory under
  Rust's rules. Managed types live on the GC heap under today's rules.
  A type used in both worlds compiles twice, or a declaration says
  which world it belongs to. `List[T]` and `Map[K, V]` need a managed
  family beside the owned one.
- **No cross-world cycles.** A managed object cannot point to an owned
  value, so a cycle never runs through a table slot. Nothing leaks.

The other implementations avoid the split at a price. A tracing
`Gc[T]` in linear memory, as `rust-gc` builds it, can hold owned values,
but Wasm has no stack scanning, so every `Gc` handle in a local must be
registered as a root, every such program carries a collector (estimate:
2k to 3k lines, estimate), and its
contents need `GcCell` flags. Counting with cycle collection pays a
count per copy, and a drop that runs when the count hits zero is a
finalizer whenever it does more than free memory.

### 3.5 Finding

Yes, `Gc[T]` can replace `Rc<RefCell<T>>`, and on Wasm GC it does so
with no runtime flag, no count and no `clone()` ceremony. Copying a
traced reference is free, so `Gc[T]` can be `Copy`. Its price is that
its contents are managed types with today's semantics, which makes
ownership-plus-`Gc` a two-world language: Rust's rules for owned data,
hd's current rules inside `Gc`. That is the shape of the removed
prototype, with the default flipped. Programs whose data is
shared, such as graphs, caches, providers and UI state, would write
`Gc` at every type that holds it.

## 4. The Backend

### 4.1 Owned Values In Linear Memory

Under Rust's model, owned values move from Wasm GC structs to linear
memory. That changes most of `hd_wasm`:

| Piece | Today on Wasm GC | Owned values in linear memory |
| --- | --- | --- |
| Allocation | `struct.new`, the engine's allocator and collector | an allocator emitted into the module (estimate: 1k to 3k bytes for a size-class allocator) |
| Freeing | the engine's collector | drop glue per type, drop flags where a move is conditional |
| Stack | none: Wasm GC cannot place an object on the stack | a shadow stack in linear memory for every borrowed local; records passed by value in locals |
| Embedded parts | a separate struct per part, since there are no interior pointers | inline, with `^` as an address into the outer value |
| Erased generics | `eqref` storage and a `ref.cast` per read | monomorphized layouts, no casts |
| Bounds | the engine checks each array access | linear memory is one sandbox; slices check their own bounds, as Rust does |
| SIMD over bytes | impossible: no wide load from a GC array | `v128.load` directly; the probe measured 22x to 41x on 1 MiB scans |
| Host copies | element by element: Wasm GC has no bulk copy to memory | the host reads and writes memory directly; the probe measured 6x to 10x on a 1 MiB copy |
| Threads | none until shared-everything threads | Wasm threads with shared memory, shipped; owned trees move to a worker in constant time |
| Memory return | the engine's collector may return memory | linear memory never shrinks; freed pages stay reserved |

The probe numbers are hand-written kernels
([SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md) §10), not hd output.

### 4.2 Binary Size

The reference hello build is 2,935 bytes, and the tiny-program goal is
2,048 ([Performance Roadmap](PERFORMANCE_ROADMAP.md) §2.2). A program
that allocates on the linear heap carries an allocator and drop glue,
which is emitted per type and multiplied by monomorphization. A
program that also uses `Gc[T]` carries both runtimes plus a table.
Estimate: a small allocating program grows by 2 to 5 KB, and the
tiny-program goal holds only for a program that never touches the
linear heap.

### 4.3 JS Interop And The Playground

Browsers run both kinds of Wasm, so the playground keeps working. Its
programs reach JavaScript only through the host boundary codecs, which
get cheaper: the host reads linear memory as typed arrays instead of
calling element getters. GC objects stay opaque to JavaScript either
way. Two costs: browser threads need a cross-origin-isolated page
(COOP and COEP headers), and the host code that walks GC arrays
(`host/*.mjs`, 5.2k lines; about 2.5k of codecs in `hd_run_wasmtime`)
is rewritten.

### 4.4 Native Later

A native backend is not planned (#188 closed).

## 5. Three Programs, Three Ways

Each program is first written in today's hd and checked. Then it is
written in Rust-model hd, and in Rust-model hd with `Gc[T]`. Every
signature of std is assumed to have changed sensibly: `get!` takes
`url: ^str`, `retry!`'s `when` takes `fn(^E) -> bool`, and `fn!`
parameters accept lending closures.

### 5.1 HTTP Fetch And Parse With Retry

Fetch a repository's releases with three tries inside a ten-second
limit, and return the first published one.

```hd
use std.http.{Http, HttpError, get}
use std.serde.{Deserialize, Serialize}
use std.task.{retry, timeout}
use std.time.{Clock, s}

@derive(Deserialize, Serialize)
struct Release:
    tag_name: string
    draft: bool

fn transient(error: HttpError) -> bool:
    match error:
        .Timeout => true
        .Connect(_) => true
        .Status(status, _) => status >= 500
        _ => false

fn latest!(repo: string) -> Result[Release, HttpError] $ Http + Clock:
    url := "https://api.example.com/repos/$repo/releases"
    fetched := timeout!(s(10)):
        retry!(3, when=transient):
            get!(url)?.ok()
    response := match fetched:
        .Ok(result) => result?
        .Err(_) => return .Err(.Timeout)
    for release in response.json::[List[Release]]()?:
        if !release.draft:
            return .Ok(release)
    .Err(.Other("no published release"))
```

Rust-model hd changes two lines: `transient(error: ^HttpError)`, since
`retry!`'s `when` now borrows the error, and `latest!(repo: ^str)`.
`get!(url)` reads the same and now lends `url`; the closure lends its
capture to each try; `for` consumes the decoded list.

With `Gc[T]`, nothing changes: no data is shared. This is the easy
case for ownership: data flows down, and call-site lending keeps the
calls unchanged. Had `get!` taken `url: string` by value, the closure
would move `url` out on its first try (`cannot-move-out-of-capture`).

### 5.2 A Worker Pool

Fetch a list of URLs four at a time, count successes per host and
collect failures, in one stats record that every task updates.

```hd
use std.http.{Http, get}
use std.task.map_all

struct Stats:
    done: mut Map[string, usize]
    failed: mut List[string]

impl Stats:
    fn record(mut self, host: string) -> void:
        self.done[host] = self.done.get(host).unwrap_or(0) + 1

struct Job:
    url: string
    host: string

struct Outcome:
    url: string
    bytes: usize

fn run_job!(job: Job, stats: mut Stats) -> Outcome? $ Http:
    match get!(job.url):
        .Ok(response) =>
            stats.record(job.host)
            .Some(Outcome { url: job.url, bytes: response.body.len() })
        .Err(_) =>
            stats.failed.push(job.url)
            .None

fn crawl!(jobs: List[Job]) -> (List[Outcome], Stats) $ Http:
    let stats: mut Stats = Stats { done: {}, failed: [] }
    results := map_all!(jobs, 4, fn!(job: Job) -> Outcome? $ Http: run_job!(job, stats))
    let kept: mut List[Outcome] = []
    for r in results:
        match r:
            .Some(o) => kept.push(o)
            .None => pass
    (kept, stats)
```

In Rust-model hd, four tasks cannot hold one `^mut Stats`. The stats
move into a cell, and each change becomes a non-suspending closure:

```text
fn run_job!(job: Job, stats: ^Shared[Stats]) -> Outcome? $ Http:
    match get!(job.url):                                          # lends `job.url`
        .Ok(response) =>
            stats.update(fn(s: ^mut Stats): s.record(job.host))   # moves `job.host` into the closure
            .Some(Outcome { url: job.url, bytes: response.body.len() })
        .Err(_) =>
            stats.update(fn(s: ^mut Stats): s.failed.push(job.url))
            .None

fn crawl!(jobs: List[Job]) -> (List[Outcome], Stats) $ Http:
    stats := Shared::new(Stats { done: {}, failed: [] })
    results := map_all!(jobs, 4, fn!(job: Job) -> Outcome? $ Http: run_job!(job, stats))
    ...                                                           # `kept` as today; it consumes `results`
    (kept, stats.into_inner())                                    # the borrow ended with `map_all!`
```

`record` takes `^mut self`; the fields lose their `mut`, since the
owner may mutate. std's `map_all!` needs the same rewrite.

With `Gc[T]`, the program is today's plus three changes: `run_job!`
takes `stats: Gc[mut Stats]`, `crawl!` builds `stats := Gc::new(...)`,
which each task copies for free, and returns `Gc[Stats]`. `Stats`'s map
and list must be managed collections, so `Stats` belongs to the managed
world, and a caller that wants it owned copies it out.

### 5.3 An In-Memory Graph

A build graph: targets depend on targets, touching a target marks it
and everything that depends on it dirty, and building runs the dirty
targets in dependency order.

```hd
struct Target:
    name: string
    deps: mut List[mut Target]
    dependents: mut List[mut Target]
    dirty: bool

struct Graph:
    nodes: mut Map[string, mut Target]

impl Graph:
    fn node(mut self, name: string) -> mut Target:
        match self.nodes.get(name):
            .Some(found) => found
            .None =>
                let fresh: mut Target = Target { name: name, deps: [], dependents: [], dirty: true }
                self.nodes[name] = fresh
                fresh

    fn depend(mut self, from: string, to: string) -> void:
        let a: mut Target = self.node(from)
        let b: mut Target = self.node(to)
        a.deps.push(b)
        b.dependents.push(a)

fn touch(t: mut Target) -> void:
    if t.dirty:
        return
    t.dirty = true
    for up in t.dependents:
        touch(up)

fn build(t: mut Target, log: mut List[string]) -> void:
    if !t.dirty:
        return
    for d in t.deps:
        build(d, log)
    log.push(t.name)
    t.dirty = false
```

An agent's first Rust-model attempt keeps this shape and meets four
errors: a `^mut Target` stored in a list has no lifetime
(`misplaced-borrow`); `depend` holds the first `node` result while
calling `node` again (`borrow-conflict`); `touch` iterates `dependents`
while recursing mutably (`borrow-conflict`); and `name` moves into both
the target and the map (`use-after-move`). None has a local fix. The
idiomatic answer is an arena with indices:

```text
struct Target:
    name: string
    deps: List[usize]                             # indices into Graph.targets
    dependents: List[usize]
    dirty: bool

struct Graph:
    targets: List[Target]
    index: Map[string, usize]

impl Graph:
    fn node(^mut self, name: ^str) -> usize:
        match self.index.get(name):
            .Some(i) => i                         # usize copies
            .None =>
                self.targets.push(Target { name: name.to_string(), deps: [], dependents: [], dirty: true })
                self.index[name.to_string()] = self.targets.len() - 1
                self.targets.len() - 1

    fn depend(^mut self, from: ^str, to: ^str) -> void:
        a := self.node(from)
        b := self.node(to)
        self.targets[a].deps.push(b)
        self.targets[b].dependents.push(a)

    fn touch(^mut self, t: usize) -> void:
        if self.targets[t].dirty: return
        self.targets[t].dirty = true
        for up in self.targets[t].dependents.clone():     # the loop would borrow what `touch` mutates
            self.touch(up)

    fn build(^mut self, t: usize, log: ^mut List[string]) -> void:
        if !self.targets[t].dirty: return
        for d in self.targets[t].deps.clone():
            self.build(d, log)
        log.push(self.targets[t].name.clone())
        self.targets[t].dirty = false
```

With `Gc[T]`, the original shape comes back:

```text
struct Target:                                      # a managed type
    name: string
    deps: List[Gc[mut Target]]
    dependents: List[Gc[mut Target]]
    dirty: bool

struct Graph:
    nodes: Map[string, Gc[mut Target]]            # a table slot per entry if `Graph` is owned

impl Graph:
    fn node(^mut self, name: string) -> Gc[mut Target]:
        match self.nodes.get(name):
            .Some(found) => found                 # Gc copies
            .None =>
                fresh := Gc::new(Target { name: name.clone(), deps: [], dependents: [], dirty: true })
                self.nodes[name] = fresh
                fresh

    fn depend(^mut self, from: string, to: string) -> void:   # body as today

fn touch(t: Gc[mut Target]) -> void:              # body as today
fn build(t: Gc[mut Target], log: ^mut List[string]) -> void:   # body as today
```

### 5.4 The Count

Memory marks are words written only to say how a value is held or
shared: `mut` on types today; `^`, `^mut`, `clone()`, `to_string()`,
cells and lifetimes under ownership; `Gc` and its constructor. Errors
are those an agent writing today's shape first would likely hit.

| Program | Today: marks | Rust model: marks | Rust model: likely first errors | With `Gc[T]`: marks | With `Gc[T]`: likely first errors |
| --- | --- | --- | --- | --- | --- |
| Fetch with retry | 0 | 2 | 1: `transient` keeps a by-value parameter (`type-mismatch` against `fn(^E) -> bool`) | 2 | 1, the same |
| Worker pool | 6 | 8 (`^Shared`, `Shared::new`, two `update` calls, two `^mut Stats`, `^mut self`, `into_inner`) | 2: two tasks capture one `^mut Stats` (`borrow-conflict`), and the work closure is not callable concurrently (`closure-kind`) | 6 (`Gc` types twice, `Gc::new`, `mut self`, two `mut` fields) | 1: returning the managed `Stats` where an owned one is expected (`type-mismatch`) |
| Build graph | 15 | 13 (`^mut self` four times, `^str` three times, `^mut List`, three `clone()`, two `to_string()`), after a redesign to indices | 4, none locally fixable | 12 | 1 to 2: an owned `Graph` holding `Gc` values, or `name` moving twice |

The fetch program costs nothing extra, like most request-and-response
code. The pool and the graph cost a redesign, and the errors come
first. An agent fixes `use-after-move` locally with a `clone()`; it
cannot fix "four tasks share one counter" or "a node points at its
parent" locally, because the program must change shape.

### 5.5 Published Evidence

No published rate of borrow errors for agents exists, so this study
gives none. What has been published:

- A randomized controlled trial with 428 students compared Rust with
  and without Bronze, a library garbage collector. On a task that
  needed complex aliasing, Bronze users were more likely to finish in
  the time available, and those who finished took about a third of the
  time (4 hours against 12). Surveys named "ownership, borrowing, and
  lifetimes" as the primary causes of difficulty (Coblenz et al., 2022).
- The Rust project's 2023 survey: "The areas of Rust that Rustaceans
  seem to struggle with the most seem to be asynchronous Rust, the
  traits and generics system and also the borrow checker." Of former
  users, 24% cited difficulty as the main reason they stopped.
- Google's 2022 internal survey of its Rust developers listed
  "ownership and borrowing" among the top three challenges, with
  macros and async programming. A third of respondents were as
  productive in Rust as in other languages within two months, and over
  half within four.
- RustAssistant fixed real Rust compilation errors with an LLM at a
  peak accuracy of about 74% (Deligiannis et al., 2023). Its abstract
  does not break that down by error kind.

The pattern matches section 5.4: straight-line code is cheap, and
aliasing is where the time goes. The services and tools agents write in
hd keep shared state in caches, providers and handlers routinely.

## 6. Rewrite Scope

Lines counted on 2026-10-18; the share that changes is an estimate.

| Area | Lines today | Changes (estimate) |
| --- | --- | --- |
| spec/lang 04, type system | 2,494 | views, access permission, fresh values, mutable paths, variance and the implementation model: about half |
| spec/lang 07, functions | 1,083 | parameter modes, captures, three closure kinds: about 40% |
| spec/lang 11, requirements and suspension | 1,609 | providers, frames, cancellation, combinators: about 30% |
| spec/lang 06, control flow | 1,185 | `for` over owned and borrowed values, moves in `match`, `using` against `Drop`: about 30% |
| spec/lang 05, 08, 09, 10, 14 | 10,267 | moves in expressions, fields, receivers, `Copy`, `Clone`, `Drop`, `Send`, `Sync`, statics, handles: 10% to 25% each |
| spec/lang 13, ownership | 307 | replaced by a full chapter: 1.5k to 2.5k new lines |
| spec/std | 8,835 | every signature gains modes; iterators and task combinators redesigned: 30% to 50% |
| lib/std | 15,922 hd | every signature; the closure-backed `Iterator`, `map_all!`, `all_list!`, collections and `Disposable` users: 40% to 60% |
| `hd_check` | 31,227 Rust | a borrow checker, move checking, drop elaboration, closure kinds: 10k to 15k new |
| `hd_types` | 8,925 | lifetimes in the type pool, variance over them, `Copy` and `Send` solving: 2k to 4k new |
| `hd_tir`, `hd_mono`, `hd_structure` | 9,644 | places and moves, drop glue, move-correct derivations: 3k to 5k changed |
| `hd_wasm` (the Wasm GC backend) | 19,629 | linear-memory layouts, allocator, shadow stack, drop glue, and with `Gc` the table slots and two layouts: 12k to 18k new or rewritten |
| Host codecs (`host/*.mjs`, `hd_run_wasmtime`) | 5,178 and about 2,500 | read and write linear memory instead of GC arrays: 3k to 5k changed |
| Conformance fixtures | 3,744 files, 64,834 lines | 687 files use `mut`; 1,291 runtime fixtures depend on aliasing or cleanup order: a third to a half edited |
| Guide, tour, playground examples | 2,861 | rewritten |

In total, an estimated 40k to 60k lines of new or changed spec, std
and compiler, before diagnostics tuning. That is about the size of
`hd_check` and `hd_wasm` together, and it lands on work that is in
flight today: the stdlib audit, the performance steps and the
serializable closures.

## 7. What hd Offers Over Rust

Under Rust's model, hd would compete with Rust directly. It would still
offer requirement rows and providers (checked dependency injection,
fakes swapped with `$.with`, host capabilities in `main`'s signature;
Rust threads context through generics or globals), simpler async (one
runtime, frames that never move, tasks that never migrate, structured
cancellation that waits for disposal), typed derivation in hd itself
instead of procedural macros, and the platform: a sandbox, a browser
playground, deterministic output and replay, and diagnostics designed
for agents.

None of these needs ownership. A collector is the main reason hd can
ask agents for Go-level effort, and a user who wants no collector
already has Rust, with its ecosystem, an LLVM backend and ten years of
borrow-checker refinement. Under the collector, hd's position is Go's
or TypeScript's ease with checked capabilities and structured effects.
Under ownership, it becomes Rust with dependency injection and simpler
async: a narrower niche, with Rust's hardest part inherited.

## 8. What Rust Gives, Measured

The sections above price Rust's model on paper. This one measures what
Rust's speed is made of today, on the same Wasm engines hd targets, so
the owner can see how much of it comes from allocating less and how
much from a better optimizer. Wasm GC cannot place an object on a
stack, and hd removes no allocation yet (section 1), so the suite has
two groups: five programs that compute in fixed arrays with almost no
allocation, and six that allocate in their hot loop.

Each program is one algorithm written idiomatically in JavaScript (on
Node), Python (CPython), hd and Rust, with the same input and the same
printed checksum. hd is built with `hd build --release` and runs on V8
through the repository's Node host and on wasmtime through
`hd run --release --engine wasmtime`. Rust is built for
`wasm32-unknown-unknown` with `--release`, exports `run(n)`, and runs on
V8 from a small Node driver and on wasmtime 49.0.2 from a scratch
embedding configured as hd's engine is (Cranelift at speed, epoch
interruption on). Every time is the median of five runs after one
warm-up, as process wall time in milliseconds, so it includes start-up:
the last row of each table is an empty program. The machine was never
quiet: the one-minute load stayed between 5.6 and 15.4 on 14 cores
(mostly Spotlight indexing), which adds noise but no bias to one
language. Every checksum matched across all languages and engines. The
programs, versions and commands are in the appendix.

### 8.1 Computation

| Program | Node | Python | hd, V8 | hd, wasmtime | Rust, V8 | Rust, wasmtime | hd/Rust, V8 | hd/Rust, wasmtime |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| spectral-norm (n 2500) | 528 | 20,886 | 317 | 822 | 227 | 482 | 1.4 | 1.7 |
| fannkuch-redux (n 10) | 208 | 3,331 | 753 | 414 | 279 | 228 | 2.7 | 1.8 |
| mandelbrot (1600², 50 iterations) | 141 | 4,511 | 430 | 224 | 243 | 172 | 1.8 | 1.3 |
| nbody (5M steps) | 286 | 15,575 | 262 | 339 | 408 | 225 | 0.6 | 1.5 |
| sieve (30M) | 130 | 1,508 | 624 | 314 | 273 | 144 | 2.3 | 2.2 |
| empty program | 30 | 24 | 36 | 18 | 32 | 6 | | |

Rust allocates at most three times per run here, and hd allocates
nothing beyond start-up except in the sieve. Its 30 million flags cost
268 MB of V8 allocation against Rust's one 30 MB vector, which is about
four bytes per flag plus the copies a growing list makes.

hd is 1.3x to 2.7x slower than Rust in nine of ten cells, and faster
than Rust on V8 in nbody. With no allocation in either, the gap is code
quality. Rust compiled at `opt-level=1` is within 20% of `opt-level=3`
in every cell of this group, while `opt-level=0` is 1.2x to 5.6x
slower. hd's release build lands between Rust's levels 0 and 1 in most
cells, and at or behind level 0 for mandelbrot and the sieve on V8.
hd's own optimizer buys little so far: on V8 its release build is at
most 1.14x faster than its debug build, except spectral-norm at 2.25x.

### 8.2 Allocation

| Program | Node | Python | hd, V8 | hd, wasmtime | Rust, V8 | Rust, wasmtime | hd/Rust, V8 | hd/Rust, wasmtime |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| binary-trees (depth 20) | 2,825 | 19,483 | 1,934 | 2,913 | 3,958 | 11,973 | 0.5 | 0.2 |
| points (1M records, 10 rounds) | 673 | 3,317 | 399 | 251 | 81 | 43 | 4.9 | 5.9 |
| closures (1M items, 50 rounds) | 889 | 4,131 | 272 | 1,632 | 160 | 117 | 1.7 | 13.9 |
| map-count (2M words) | 370 | 348 | 497 | 450 | 144 | 162 | 3.4 | 2.8 |
| string-build (3M pieces) | 240 | 262 | 366 | 351 | 104 | 112 | 3.5 | 3.1 |
| json (400k records) | 241 | 781 | 1,007 | 2,427 | 234 | 323 | 4.3 | 7.5 |
| empty program | 30 | 24 | 36 | 18 | 32 | 6 | | |

What each run allocates: Rust counted by a counting global allocator,
hd and Node by V8 (heap in use at exit plus what every collection
freed, the method of `bench/metrics`), less the empty program.

| Program | Rust: allocations, bytes | Rust in hd's shape: allocations, bytes | hd on V8: bytes | Node: bytes |
| --- | --- | --- | ---: | ---: |
| binary-trees | 307M, 2,455 MB | the same program | 9,821 MB | 12,276 MB |
| points | 11, 176 MB | 11.0M, 220 MB | 537 MB | 1,504 MB |
| closures | 1, 4 MB | 201, 4 MB | 8 MB | 1,271 MB |
| map-count | 38, 36 MB | 6.0M, 123 MB | 447 MB | 390 MB |
| string-build | 23, 67 MB | 3.0M, 148 MB | 531 MB | 350 MB |
| json | 3.6M, 164 MB | 6.4M, 384 MB | 2,132 MB | 212 MB |

"Rust in hd's shape" is the same Rust allocating the way hd does:
`Vec<Box<Point>>` for the records, boxed iterator stages and `Box<dyn
Fn>` closures, a `String` per word and per piece with owned map keys,
and JSON decoded through a `serde_json::Value` tree first, as hd's
`decode` parses to a `Json` value.

Three readings stand out.

- **Allocating an object is cheaper on Wasm GC than in Rust's linear
  memory.** Binary-trees allocates one object per node in both
  languages, and hd is 2x faster than Rust on V8 and 4x on wasmtime:
  the engine's bump allocation and generational collection beat
  `dlmalloc`'s `malloc` and `free` per node. Boxing the points in Rust
  makes it slower than hd on wasmtime. Rust wins binary-trees only by
  not allocating per node: with one vector per tree it takes 925 ms.
- **Rust's lead in the record, string and map programs is mostly
  layout.** Records inline in a `Vec`, words borrowed as `&str` slices
  of the text, and one growing `String` are what Rust gives. Put back
  into hd's shape, Rust loses 2.1x to 8.7x in points, map-count and
  string-build, half or more of its lead, and 1.7x of JSON's 7.6x on
  wasmtime. Laying hd's points out by hand as two lists of numbers
  makes hd 1.7x faster on both engines.
- **The closure chain allocates nothing in hd.** hd's iterator runs
  map, filter and fold with no allocation per element, and Rust's
  boxed, dynamically dispatched version is as fast as its inlined one.
  The 13.9x gap on wasmtime is the engine: the same hd module runs 6.9x
  slower on wasmtime than on V8, while Rust's runs faster there.

### 8.3 Attribution

For each cell where Rust leads by more than 1.5x, the gap splits into
three measured factors and a residual. The times here have each
runner's empty-program time subtracted, so start-up does not dilute
the ratios. The engine factor is how much larger the gap is on this
engine than on the other one, so it is measured. Allocation and layout
is Rust in hd's shape against idiomatic Rust, also measured. The
residual is what remains, attributed to code generation, an estimate.
The last column bounds that estimate: what dropping from
`opt-level=3` to `opt-level=0` costs Rust.

| Program | Engine | hd/Rust | Engine factor | Allocation and layout | Residual: code generation (estimate) | Bound: Rust O0/O3 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| spectral-norm | wasmtime | 1.69 | 1.18 | none to measure | 1.43 | 3.99 |
| fannkuch-redux | V8 | 2.90 | 1.62 | none to measure | 1.78 | 3.88 |
| fannkuch-redux | wasmtime | 1.78 | 1.00 | none to measure | 1.78 | 5.52 |
| mandelbrot | V8 | 1.86 | 1.50 | none to measure | 1.24 | 1.71 |
| sieve | V8 | 2.44 | 1.13 | not measured | 2.16 | 2.00 |
| sieve | wasmtime | 2.15 | 1.00 | not measured | 2.15 | 4.36 |
| points | V8 | 7.34 | 1.15 | 4.30 | 1.49 | 1.97 |
| points | wasmtime | 6.38 | 1.00 | 6.38 (8.67 alone) | 1.00 | 3.84 |
| closures | V8 | 1.83 | 1.00 | 1.00 (0.84 alone) | 1.83 | 1.68 |
| closures | wasmtime | 14.50 | 7.91 | 1.08 | 1.70 | 5.18 |
| map-count | V8 | 4.09 | 1.47 | 2.15 | 1.29 | 4.14 |
| map-count | wasmtime | 2.78 | 1.00 | 2.08 | 1.34 | 3.38 |
| string-build | V8 | 4.59 | 1.47 | 2.11 | 1.49 | 2.52 |
| string-build | wasmtime | 3.13 | 1.00 | 2.19 | 1.43 | 1.92 |
| json | V8 | 4.81 | 1.00 | 4.81 (5.05 alone) | 1.00 | 5.07 |
| json | wasmtime | 7.59 | 1.58 | 1.66 | 2.90 | 5.13 |

The factors multiply to the gap. A factor that would exceed what
remains, or fall below 1, is clamped, with its own value in
parentheses. Three cells are less certain than the rest. Rust's
JSON-through-a-tree variant runs 2.0x slower on V8 than on wasmtime,
so its 4.81 on V8 likely includes V8's cost of growing a 384 MB linear
memory, and the wasmtime 1.66 is the safer figure. The sieve has no
Rust variant with four-byte flags, so its layout share is folded into
the residual, which then exceeds the bound. The closure chain on V8
also exceeds it, so part of that residual is the cost of hd's closure
calls rather than optimization.

Two engine costs apply to every row. Epoch interruption, which hd's
wasmtime engine turns on to enforce time limits, slows Rust's modules
on wasmtime by up to 2.4x (spectral-norm; binary-trees 2.0x; most
programs 1.2x to 1.5x), and hd pays it in every loop. And hd runs
integer loops faster on wasmtime than on V8 (fannkuch, mandelbrot,
sieve and points at 0.50x to 0.64x of V8's time), but closures, JSON
and spectral-norm 2.5x to 6.9x slower.

Read together: in the computation group, Rust's speed is code
generation. No allocation is involved, the gap of 1.2x to 2.9x is at
most about what dropping to `opt-level=0` costs Rust, and
`opt-level=1` already gives nearly all of it. In the allocation group,
Rust's speed is layout, not ownership: values stored inline or
borrowed instead of allocated as separate objects explain half or more
of the gap in three of the four programs that have a variant. Where
Rust allocates object by object as hd does, Wasm GC is faster. What
remains after layout is at most 1.5x of code generation, except the
closure chain at 1.7x to 1.8x and JSON on wasmtime at 2.9x, whose
cause was not isolated. The single largest factor in the suite is not
a language property at all: hd's closure calls on wasmtime.

### 8.4 What This Means For The Recommendation

The measurements confirm the recommendation in section 9 and narrow
its third point. Rust's model would not make hd's allocation-heavy
programs fast by itself. Owned values in linear memory, allocated one
by one, measured slower than Wasm GC objects here. Rust's advantage
comes from storing small values inline and borrowing slices, which a
collected language can also do: flat records in lists (the structure
of arrays that [`hd_wasm`'s design](../../crates/hd_wasm_linear/DESIGN.md)
§15.2 plans for `Array[T]`), substrings and map keys that need no
object of their own, and scalar replacement after inlining. hd's
hand-flattened points measure that route at 1.7x. The
computation gap is optimizer work that no memory model changes. The
measurements also rank three costs that this study had not priced:
closure calls on wasmtime, decoding JSON through a tree (2.1 GB
allocated where Rust allocates 164 MB), and the epoch checks in every
loop on wasmtime.

## 9. Recommendation

**Keep the collector as the default, and take the three gains by
narrower routes.**

1. **Threads.** Stage A of
   [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md) gives worker
   pools and parallel maps on Wasm today: separate instances, copied
   messages and `SharedBytes` in shared linear memory. The pool of
   section 5.2 needs no threads at all, since its tasks wait on I/O.
   Shared mutable GC data between threads waits for shared-everything
   threads. That wait is the real price of keeping the collector, and
   it is the one gain ownership would bring sooner.
2. **SIMD and host copies.** Put large byte data in linear memory
   behind handles, as `SharedBytes` already proposes, and give scans
   the approved primitives
   ([SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md)). The probe's 22x
   to 41x applies to bytes in linear memory, whoever owns them.
3. **Allocation.** Inlining, escape analysis and scalar replacement in
   the optimized pipeline remove per-element allocation with no
   language mark ([Performance Roadmap](PERFORMANCE_ROADMAP.md),
   steps 20 to 24).
4. **Cleanup** stays `Disposable` and `using`. Ownership would not
   improve it without a suspending drop, which Rust has not designed.

Ownership is parked (#163 closed). The tier 2 study keeps the opt-in
lifetime design.

## 10. Questions For The Owner

1. **Default model.** Keep the collector and today's object semantics
   as hd's default, rather than adopting owned values, moves, borrows
   with lifetimes and `Drop` for every program? *Recommendation: keep
   the collector.* The cost lands on shared state (section 5), and the
   rewrite is 40k to 60k lines (section 6).
2. **Parallelism as the reason.** Is shared mutable data between
   threads on Wasm, before shared-everything threads ship, worth a new
   memory model? *Recommendation: no.* Stage A covers worker pools,
   parallel maps and large bytes today; revisit when stage A's copies
   are measured as too slow for a real program.
3. **If ownership is adopted: the sharing type.** Use a `Gc[T]` backed
   by Wasm GC whose contents are managed types with today's
   semantics, so no `RefCell` and no borrow flags, accepting two worlds
   and a managed collection family? *Recommendation: yes.* It is the
   only option without a per-access or per-copy cost, and the Wasm
   constraint forces the split anyway.
4. **If ownership is adopted: providers.** Keep providers as shared
   managed values, so mutable providers, escaping provider values and
   stored suspensions keep their current rules? *Recommendation: yes.*
   Siblings share providers by design, so borrowing them buys nothing.
5. **If ownership is adopted: cleanup.** Keep `Disposable` and `using`
   for handles, with a synchronous `Drop` that frees memory only?
   *Recommendation: yes.* A suspending `Drop` turns every scope exit
   into a possible suspension point and inherits Rust's unsolved async
   drop.

## Sources

- Rust: [`std::pin`](https://doc.rust-lang.org/std/pin/index.html);
  [`tokio::spawn`](https://docs.rs/tokio/latest/tokio/task/fn.spawn.html);
  [RFC 3668](https://rust-lang.github.io/rfcs/3668-async-closures.html) and
  [Rust 1.85.0](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/) (async closures);
  [async drop](https://rust-lang.github.io/async-fundamentals-initiative/roadmap/async_drop.html);
  [`await_holding_refcell_ref`](https://rust-lang.github.io/rust-clippy/master/index.html#await_holding_refcell_ref);
  [The Scoped Task Trilemma](https://without.boats/blog/the-scoped-task-trilemma/);
  [`slotmap`](https://docs.rs/slotmap/latest/slotmap/).
- Collectors and counting for Rust: [rust-gc](https://github.com/Manishearth/rust-gc);
  Hughes and Tratt, [Garbage Collection for Rust: The Finalizer Frontier](https://arxiv.org/abs/2504.01841) (Alloy);
  [Samsara](https://github.com/chc4/samsara); [bacon-rajan-cc](https://github.com/fitzgen/bacon-rajan-cc);
  [GhostCell](https://plv.mpi-sws.org/rustbelt/ghostcell/).
- Other models: [Swift ARC](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/);
  [Nim ORC](https://nim-lang.org/blog/2020/12/08/introducing-orc.html);
  [Perceus](https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/) (PLDI 2021);
  [Vale generational references](https://verdagon.dev/blog/generational-references);
  [Hylo](https://hylo-lang.org/) and [Implementation Strategies for Mutable Value Semantics](https://www.jot.fm/issues/issue_2022_02/article2.pdf) (JOT 2022).
- Wasm: [reference types](https://github.com/WebAssembly/reference-types/blob/master/proposals/reference-types/Overview.md);
  [GC](https://github.com/WebAssembly/gc/blob/main/proposals/gc/Overview.md);
  [shared-everything threads](https://github.com/WebAssembly/shared-everything-threads).
- Evidence: Coblenz, Mazurek and Hicks, [Garbage Collection Makes Rust Easier to Use: A Randomized Controlled Trial of the Bronze Garbage Collector](https://arxiv.org/abs/2110.01098) (ICSE 2022);
  [Rust 2023 survey results](https://blog.rust-lang.org/2024/02/19/2023-Rust-Annual-Survey-2023-results/);
  [Rust fact vs. fiction](https://opensource.googleblog.com/2023/06/rust-fact-vs-fiction-5-insights-from-googles-rust-journey-2022.html) (Google, 2023);
  Deligiannis et al., [Fixing Rust Compilation Errors using LLMs](https://arxiv.org/abs/2308.05177) (2023).
- This repository: the studies linked above, [crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)
  §14 and §15, and the deleted ownership design (`git show 73c0e0478^:docs/future-work/OWNERSHIP_DESIGN.md`).

## Appendix: Benchmark Data

The data behind section 8, measured on 2026-10-18 at commit `e3d78deeb`.

**Machine and versions.** Apple M3 Max, 14 cores, 36 GB, macOS 26.6.2
(Darwin 25.6.0). Node v24.19.0 (V8 13.6.233.17-node.51); Python 3.14.7;
rustc 1.97.1 with target `wasm32-unknown-unknown`; wasmtime 49.0.2 (in
`hd` and in the scratch runner); serde 1.0.229 and serde_json 1.0.154.
The one-minute load before each measured command was 5.6 to 15.4
(main runs) and 5.6 to 13.9 (variant runs); the brief's wait for a load
under 4 ran its 20 minutes and timing started at 15.4. Of 154 measured
commands longer than 50 ms, 25 had a spread (max minus min over five
runs) above 10% of the median.

**Programs.** The sources are scratch files under `target/bench4/`
(`js/`, `py/`, `hd/<name>/src/main.hd`, `rust/examples/`), not in the
repository. All programs compute their checksums with integer
arithmetic or truncate a float result times 10⁹, so every language
prints the same number. Rust uses `Vec<f64>`, `Vec<usize>` and
`Vec<bool>` with bounds checks, `Option<Box<Node>>` trees,
`HashMap<&str, i64>`, `write!` into a `String`, and serde derive. hd
uses `List` with `while` loops, `data` records, `List.map`, the
`Iterator` chain, `Map[string, i32]`, `StringBuilder`, and
`std.json.encode` and `decode`. JavaScript uses typed arrays for the
computation group and arrays of objects, `Map`, `join` and `JSON` for
the rest. Python uses lists, `bytearray`, tuples for trees, a
`dataclass(slots=True)` for points, `functools.reduce`, a `dict`, `join`
and the `json` module.

**Commands.** hd: `cargo build --release -p hd_cli --features wasmtime`;
in each package `HD_CACHE=target/bench4/cache target/release/hd build
--release --engine node` (and `hd build --engine node` for the debug
pipeline), then `node host/run.mjs NAME.wasm`; on wasmtime
`hd run --release --engine wasmtime` (debug: `hd run --engine
wasmtime`, which also turns Cranelift's optimizer off). Rust:
`cargo build --release --examples --target wasm32-unknown-unknown`
with profiles `o1` and `o0` that inherit `release` and set only
`opt-level`; on V8 `node rust-v8.mjs NAME.wasm N`; on wasmtime
`wasmtime-runner NAME.wasm N`, a small embedding with Cranelift at
`Speed`, epoch interruption on and a deadline it never reaches
(`--no-epoch` turns it off). Node: `node NAME.js`. Python:
`python3 -I NAME.py`. A Python harness ran each command once to warm
up, then five times under `/usr/bin/time -l`, keeping the median wall
time and the peak RSS.

**Allocation counts.** Rust: the same examples built with a counting
`#[global_allocator]` over the system allocator, `dlmalloc` on this
target. hd and Node: V8's heap at exit plus the bytes every collection
freed under `--trace-gc-nvp`, as the `allocations` metric in
`bench/metrics` measures; the empty program's 6.1 MB (hd) and 4.8 MB
(Node) are subtracted. wasmtime has no allocation counter, so hd's
counts are from V8 only.

**Module size and instructions.** Release builds; instructions counted
with the `wasmparser` crate. hd's release modules carry no name
section, so its per-function counts are not shown; Rust's `run`
includes everything LLVM inlined into it.

| Program | hd: bytes | hd: instructions | Rust: bytes | Rust: instructions | Rust: `run` |
| --- | ---: | ---: | ---: | ---: | ---: |
| spectral-norm | 5,487 | 1,320 | 23,053 | 7,314 | 480 |
| fannkuch-redux | 5,849 | 1,497 | 23,175 | 7,384 | 551 |
| mandelbrot | 3,640 | 894 | 744 | 167 | 167 |
| nbody | 7,973 | 1,904 | 2,720 | 899 | 487 |
| sieve | 3,458 | 834 | 21,875 | 6,864 | 184 |
| binary-trees | 3,845 | 911 | 20,195 | 6,265 | 104 |
| points | 5,152 | 1,183 | 20,338 | 6,461 | 408 |
| closures | 5,989 | 1,304 | 19,848 | 6,256 | 203 |
| map-count | 19,188 | 5,753 | 30,594 | 9,932 | 1,123 |
| string-build | 6,459 | 1,513 | 23,143 | 7,069 | 85 |
| json | 118,455 | 29,555 | 138,788 | 39,119 | 1,195 |

A Rust module that allocates or can panic carries `dlmalloc` (2,100
instructions in `malloc` alone) and the panic formatter, about 20 KB.

**Variant runs** (median wall ms). "Variant" is Rust in hd's shape:
`binarytrees_arena` for binary-trees (a vector per tree, the one
variant that allocates less than hd), `points_boxed`,
`closures_boxed`, `mapcount_owned`, `stringbuild_parts`, `json_tree`.
hd's points as two lists of numbers took 231 ms on V8 and 144 ms on
wasmtime.

| Program | hd debug, V8 | hd debug, wasmtime | Rust O1, V8 | Rust O1, wasmtime | Rust O0, V8 | Rust O0, wasmtime | Rust, wasmtime, no epoch | Variant, V8 | Variant, wasmtime |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| spectral-norm | 668 | 1,491 | 232 | 556 | 1,083 | 1,903 | 204 | | |
| fannkuch-redux | 857 | 793 | 281 | 199 | 993 | 1,231 | 206 | | |
| mandelbrot | 440 | 217 | 226 | 162 | 394 | 208 | 120 | | |
| nbody | 254 | 781 | 418 | 207 | 819 | 1,228 | 195 | | |
| sieve | 685 | 407 | 297 | 170 | 513 | 609 | 99 | | |
| binary-trees | 1,986 | 2,949 | 4,033 | 12,501 | 6,202 | 12,080 | 6,036 | 925 | 1,293 |
| points | 412 | 376 | 65 | 46 | 129 | 146 | 47 | 244 | 323 |
| closures | 421 | 2,856 | 221 | 145 | 248 | 582 | 115 | 139 | 126 |
| map-count | 540 | 575 | 152 | 155 | 498 | 532 | 121 | 274 | 330 |
| string-build | 386 | 356 | 109 | 112 | 213 | 210 | 93 | 183 | 239 |
| json | 1,079 | 2,702 | 324 | 432 | 1,056 | 1,634 | 238 | 1,052 | 532 |
| empty program | 35 | 16 | 30 | 6 | 30 | 6 | 6 | | |

**Peak RSS** (MB, the largest of the five runs).

| Program | Node | Python | hd, V8 | hd, wasmtime | Rust, V8 | Rust, wasmtime |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| spectral-norm | 53 | 15 | 55 | 26 | 50 | 11 |
| fannkuch-redux | 51 | 15 | 54 | 26 | 50 | 11 |
| mandelbrot | 53 | 15 | 54 | 26 | 50 | 7 |
| nbody | 53 | 15 | 54 | 26 | 50 | 8 |
| sieve | 79 | 43 | 258 | 282 | 78 | 40 |
| binary-trees | 748 | 143 | 1,349 | 538 | 116 | 75 |
| points | 756 | 427 | 630 | 282 | 81 | 42 |
| closures | 253 | 55 | 64 | 34 | 54 | 15 |
| map-count | 365 | 246 | 398 | 278 | 80 | 41 |
| string-build | 370 | 230 | 479 | 505 | 105 | 65 |
| json | 374 | 398 | 963 | 1,069 | 187 | 147 |
| empty program | 45 | 15 | 52 | 26 | 47 | 6 |
