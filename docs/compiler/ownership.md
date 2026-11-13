# Ownership: Counts, Loans And Storage

Status: design record; may lag the code. The code and the spec win.
Part of the [compiler design](ARCHITECTURE.md). It merges four records
that were accepted and built: Ownership Per Instance (accepted
2026-11-04, migration #1399 closed 2026-11-08), Pointer Loans (model A,
accepted 2026-11-03, built under #1336), Raw Buffers (accepted
2026-11-03) and the Refcount Runtime study (#426, #626, #627). Their full
texts, with the illustrative problems, stress tests, prior art,
alternatives, cost estimates and migration plans, are in git history at
7ccbb4ea8 `docs/future-work/OWNERSHIP_ARCHITECTURE.md`,
7ccbb4ea8 `docs/future-work/POINTER_LOANS.md`, 7ccbb4ea8 `docs/future-work/RAW_BUFFER.md` and
7ccbb4ea8 `docs/future-work/REFCOUNT_RUNTIME.md`.

The layouts, the runtime (allocator, release, cycle collector) and the
emission of counts and loans are
[crates/hd_wasm/DESIGN.md §23](../../crates/hd_wasm/DESIGN.md#23-linear-memory-layout).
The ownership passes over TIR are [tir-v2.md](tir-v2.md#the-ownership-passes)
and `crates/hd_tir/src/own/`. The language rules are spec chapter 13.

## Decisions On Record

- **Counting is the only backend.** Objects live in linear memory, freed
  by reference counting with a cycle collector for cycle-capable types
  (#626, #627; the Wasm GC backend is deleted).
- **Every count is decided per instance** (owner, 2026-11-04: "we still
  need a solid foundation, this is the one we need to do"). This lifted
  the earlier fixed constraint "work is done once per generic body where
  possible, not per instance". Per-instance work measured 1.03 times the
  per-body instructions in release, 1.04 in dev, on 25 programs. The
  owner allowed whole-program and cross-function analysis on 2026-11-12
  (docs/audit/review-2026-11-12.md §13, "Owner Answers").
- **Aliasing through `*T` is model A, loans** (owner, 2026-11-03): a
  garbage-collected language iterates `for a in arr`, a copy, so the
  friction cases are written as value loops over copy-on-write lists and
  take no loan. Rejected: Go-style keep-alive (model B); a unique
  `*mut T` with Rust's `Box`/`Rc`/`RefCell` split ("definitely a no go");
  call-scoped references only.
- **Raw buffers** (owner, 2026-11-03): `Buffer` is `!Copy`, owns its
  memory and frees it when dropped, as Rust's `RawVec`; a list drops its
  elements through `std.rt.Drop`, which only std implements; no value of
  a non-copyable type is ever copied, and a verifier checks it in every
  build. The counted `List` and `Map` stay compiler built-ins (owner,
  2026-11-12; #1219 and #1220 closed).

## Ownership Per Instance

Decide every count once, in one TIR pass, per monomorphized instance.
Before it, never-addressed locals become values and a storage pass picks
where each remaining place lives. The pass writes every retain, release,
uniqueness check and loan as a TIR statement with a reason. A checked
build verifies each reason; the emitter lowers the statements one to one
and decides nothing.


**As built (7ccbb4ea8).** The passes run per instance, write each count
with a reason (`hd_tir/src/own/reason.rs`), and a checked build verifies
the result (`verify_owned`). Two parts differ from the design below.
Pointer loans are a whole-program link pass (`hd_tir/src/own/pointer.rs`),
a result table rather than `LoanBegin` and `LoanEnd` statements in TIR.
The emitter still decides parameter entry and exit counts, cancelled-await
releases, every pointer loan and drop flags, and the peephole deletes
releases by dataflow (#1420, #1781, #1792; docs/audit/review-2026-11-12.md
§13).

### Pipeline

```ir
check ─► decode ─► per instance:
           substitute types, rechoose impls (instance_body, today)
           inline (release)                     hd_opt::inline
           values: never-addressed locals → SSA values
           storage: value | frame slot | heap cell, per place, env, frame
           ownership: one derivation → Retain/Release/IsUnique/Loan* with reasons
           verify (checked builds and CI)
         ─► emit: lowers each statement one to one ─► Wasm cleanup (no count rules)
```

Each stage is a TIR-to-TIR pass in `hd_tir`, as today's passes are. There
is no second IR. The per-instance path exists: `instance_body` already
substitutes, rechooses impls, inlines and runs the passes for one
instance, but only when a declaration-fact query needs it.

**Values.** A local is a value when no `Ref` of it survives inlining, no
closure shares it through a cell, and no cleanup suite reads it. Its reads
are the stored value; stores more than once get join values by Braun et
al.'s construction on structured control flow. Measured over the
bodies 25 programs reach (release, after inlining): 98.1% of 14,902
locals qualify, 88.1% have one definition, 10.0% need joins, 1.9% stay
places.

**Storage.** For each place local, closure environment, `using` binding,
default argument and suspension frame: a Wasm value when nothing takes
its address after inlining; a frame slot when its addresses do not
outlive the function; a heap cell otherwise. Whether an address outlives
the function comes from def-use on values: a value derived from a `Ref`
extends the place's life to its own last use, as Swift's
`mark_dependence` records it. The checker has already proven that
such references stay in scope, so no new rule is needed. This replaces
`frame.rs`'s per-local bit sets and their 64-candidate cap.

### The One Place Each Count Is Decided

The ownership pass decides from four facts per value: its source is
owned or lent; each use consumes or lends; its last use on each path;
whether its source may change while it lives. It writes a statement only
when a reason holds:

| Statement | Reason | It holds when |
| --- | --- | --- |
| `Retain v` | stored | a use consumes `v` (a store, a return, an owned argument, a capture) and `v` is lent or used again later |
| `Retain v` | second owner | `v` is read from a place that a write may change while `v` lives (including through a call or a `*T` alias) |
| `Release v` | last use | `v` is owned and its last use on this path lends, or it has none |
| `Release p` | overwritten | a store replaces an owned value in a place not known to be empty |
| `Release v` | cancelled | a suspension point is cancelled while `v` is live across it |
| `IsUnique b` | may be shared | `b` is not fresh, and since its last check a retain, a consuming use or a keeping call may have shared it |
| `LoanBegin`, `LoanEnd` | pointer loan | a reference through a `*T` outlives its expression, and elision found a possible conflicting access |
| any | by design | a `*dyn`'s data count, a suspension's counted reference (#1355) |
| any | conservative | a fact was not computed (a size cap); reported, and its budget may only fall |

Read modes follow: a read is `move` at the place's last access, `borrow`
when the value only lends and its source does not change while it lives,
and `copy` only with a `Retain` beside it. This is Perceus's rule on
owned and borrowed values, with Swift's +0 convention for
parameters.

**Parameters.** A value parameter is lent in every build, as Swift's
normal parameters are; the callee retains what it consumes. In release,
the existing per-SCC summaries mark a parameter owned when the callee
consumes it on every path, as Lean's borrow inference does in reverse.
Interprocedural facts stay release-only. The cost: in dev, a callee
that stores a parameter takes one retain where today's by-type
convention moved it. Dev is not a performance target.

### Proving No Redundant Count Remains

1. **One producer.** Only the ownership pass writes count statements.
   The emitter lowers each to one helper call or inline sequence. A
   checked build asserts every emitted retain, release and uniqueness
   call traces to one TIR statement.
2. **A reason is a proof obligation.** The verifier recomputes the four
   facts by a slow, uncapped method and checks each statement's reason.
   A `Retain` whose value is never consumed, a `Release` before a later
   use, an `IsUnique` on a fresh buffer: each fails the checked build.
3. **Minimal relative to named facts.** Every remaining count traces to
   one fact. The facts are imprecise in three known places: a call not
   inlined and not summarized (dev) may write or keep its arguments; a
   write through a `*T` may reach any object of its type. (A third, a
   reference to a scattered payload field, went with views: #1422, #1448.) The `own` view prints the fact behind
   each count, not the pattern that missed.
4. **Two backstops.** A CI report lists every count by reason over the
   bench programs and std, and "conservative" has a budget that may only
   fall. #1361's runtime detector keeps running on the bench.

**Against #1361 and #1362.** #1362 tags each operation with its reason
in today's layers. Each layer labels its own output, and nothing checks
the label; the glue's "conservative" cannot point to a fact. Here the
tag is the statement's reason field, and the verifier checks it. #1361 is
a runtime detector: it flags a retain and release pair that nothing
observed, but only on paths the corpus runs. It stays as a dynamic check;
with this design it should flag only sites whose reason is one of the
three imprecise facts. Step 1 below builds #1362's tags on today's layers
for a baseline; the verifier makes them checked.

### With Ownership And Lifetimes

- **Model A loans** ([Conflicts Through Aliases](../../spec/lang/13-values-and-references.md#conflicts-through-aliases),
  #1336). A loan is live until the last use of every holder. The pass
  places `LoanBegin` and `LoanEnd` at that range; a `Drop` no longer ends
  it. Elision keeps model A's per-reference rule over release write
  summaries, moved from `emit/elide.rs` into a TIR pass. Same-root
  conflicts stay compile errors in the checker.
- **Views.** None remain: #1448 deleted representation views on
  2026-11-08. A local reference is a plain address that keeps nothing
  alive (`ref.count.local`); a counted reference keeps its object alive
  (`ref.box.lifetime`).
- **`Drop`, `Disposable` and `using`.** `std.rt.Drop` runs inside release
  glue ([Runtime Drop Hook](../../spec/lang/10-modules.md#runtime-drop-hook));
  the design moves no release later or earlier than today's last-use rule
  (`crates/hd_wasm/DESIGN.md` §23.20). `dispose` is a call in a cleanup suite
  ([Using Bindings](../../spec/lang/06-control-flow.md#using-bindings)),
  not a count, and keeps its lexical place. A cleanup suite's reads keep
  their locals as places.
- **Closures and `fn!`.** Captured immutable bindings are copied and
  variables are shared through a cell
  ([Copied And Shared Captures](../../spec/lang/07-functions.md#copied-and-shared-captures)).
  Where the cell lives is unobservable, so a cell no escaping closure
  shares is a frame slot or a value. A suspending closure's environment
  lives as long as the frame that holds it, so it escapes when the frame
  does.
- **Suspension frames**
  ([Suspending Functions](../../spec/lang/11-requirements-and-suspension.md#suspending-functions)).
  A frame holds the live values at each point. By-value arguments move
  in. A suspension that retains a counted reference its call lends
  ([Dependent Results](../../spec/lang/13-values-and-references.md#dependent-results))
  keeps that retain, with a reason.
- **`List`/`Map` against `UniqList`/`UniqMap`**
  ([Collection Values](../../spec/lang/04-type-system.md#collection-values),
  [UniqList](../../spec/std/collections.md#uniqlist)). Copy-on-write
  buffers get `IsUnique` only with "may be shared". A `UniqList` is not
  `Copy`: it is moved, never retained, and never checked. The verifier
  rejects any `Retain` of a non-`Copy` value, as checked builds already
  trap on a retained buffer (b8382fa46).
- **`&dyn` and `&mut dyn`**
  ([Trait Values And `Any`](../../spec/lang/04-type-system.md#trait-values-and-any)).
  A trait reference lends like any reference and counts nothing (#1354).
  One that takes the count of a value made for it, a `*dyn` call result
  or a copy, owns that trait value, as a local reference owns the new
  object it made.
- **Tier 2 lifetimes.** The lifetime solver proves where each local
  reference points and that it stays in scope; a local reference keeps
  nothing alive ([Local References](../../spec/lang/13-values-and-references.md#local-references)).
  The design relies on that proof and adds no check. It needs only the
  liveness extension above, which def-use on values gives.


## Pointer Loans (Model A)

Aliasing through a `*T`: two names that hold the same pointer. Paths with
no `*T` are checked statically at no cost (#1320). Every counted object
carries a borrow state, as a Rust `RefCell` does: an exclusive bit and a
shared count in six spare descriptor bits, so no object grows. A
reference through a `*T` takes a **loan** on each object it crosses until
its last use, unless nothing in its live range can write those types
through a pointer (**per-reference elision**). An alias's conflicting
access panics `borrow-conflict`. Every conflict the checker can see stays
a compile error.

```text
struct Team:
    lead: *mut User
    log: List[string]

struct User:
    name: string
    age: i32

fn rename(t: *mut Team, alias: *mut Team) -> string:
    n := &t.lead.name           # shared loans on the Team and the User
    t.log.push("renamed")       # runs: t's own path, a field the checker proves apart
    alias.lead.age += 1         # panics borrow-conflict
    *n

fn same_root(t: *mut Team) -> string:
    n := &t.lead.name
    t.lead = box(User { name: "b", age: +1 })  # conflicting-access at compile time
    *n

fn greet(u: *User) -> void $ Console:
    n := &u.name                # no loan: nothing in range writes a User
    println(*n)
```

The seven design questions were settled as recommended, and each is
built: (1) model A; (2) per-reference elision by per-type release write
summaries; (3) object granularity: a write to another field of a borrowed
object through an alias panics, as `RefCell` does; (4) own loans: a write
through the loan's own pointer, to a field the checker proves apart, skips
the function's own loans, so `for x in &p.items: p.count += 1` runs;
(5) element references: no copy of a collection while a `&mut` element
reference lives, and no write while a `&` one lives, in place of the pin;
(6) a variable a closure shares is behind a `*mut`, its cell; (7) past 31
shared loans the count spills to a side table.

### How It Works

**Static first.** Every rule of
[Writes While Borrowed](../../spec/lang/13-values-and-references.md#writes-while-borrowed)
applies through pointers, since `ref.excl.place` covers fields and
dereferences of one root, and two more cases are compile errors:

- A pointer argument is an access to its whole pointee for the call's
  other arguments: `*mut T` a write, `*T` a read.
  `mutate(boxed, &boxed.field)` and `inspect(view, &mut view.field)` are
  `conflicting-access`; `inspect(view, &view.field)` is two reads. At
  run time passing a pointer is no access.
- A call result that depends on a `*T` argument is a local reference to
  that argument's root (`ref.dep.call`): `r := borrow(boxed)` then
  `boxed.change()` conflicts, and `r` cannot outlive the local `boxed`.

Only a second name for the same pointer (`alias := a`, a field, a call
result, a parameter) reaches the run-time model.

**State.** Descriptor bits 16 to 21 (§23.4 of
`crates/hd_wasm/DESIGN.md`): bit 21 exclusive, bits 16 to 20 the
shared count. The release glue's slot keeps bits 0 to 15; the largest
bench program, `json`, had 48 table entries when this was designed.

**Loans.** A reference that crosses `*T` objects `o1 … on` takes a
shared loan on `o1 … o(n-1)` and, on `on`, a shared loan for `&` or an
exclusive one for `&mut`. An argument's loans run from its evaluation to
the return; a `&mut self` receiver is shared until the call starts. A
loan ends after the last use of every value holding the reference.

**Checks.** Taking a shared loan checks the exclusive bit, an
exclusive one the whole state. A write through a `*T` checks the whole
state, a read the exclusive bit, each against the function's own loans
through the same pointer local.

**Per-reference elision.** A reference takes no loan, and needs no
matching check, when nothing between taking it and its last use can
write any type it crosses through a pointer: no write through a `*T` of
that type, and every call has a release write summary that writes none
(for `&mut`, neither reads nor writes). A function value, a trait call
whose implementations are not all known, a suspension point and an
erased body count as writing everything. A type none of whose loans
survive needs no check anywhere.

**Element references.** With no pin, only a copy can detach a
reference from its slot, so a copy while a `&mut` element reference
lives and a write while a `&` one lives are `conflicting-access`.

**Roots.** The local or parameter holding the first `*T` of a live
reference's path lives until the reference's last use; a suspending
body's frame owns its pointer parameters.

| Operation (`o` the payload address) | Code |
| --- | --- |
| shared loan begin / end | `s = load(o-4)`; `if s & X: panic`; `store(o-4, s + 1<<16)` / subtract |
| exclusive loan begin / end | `if load(o-4) & M != own: panic`; set `X` / clear it |
| write through `*T` | `if load(o-4) & M != own: panic`, then the store |
| read through `*T` | `if load(o-4) & X != own: panic`, then the load |

### `*T` Where `&T` Is Expected

```text
struct Shape:
    w: f64
    h: f64

impl Shape:
    fn area(&self) -> f64: self.w * self.h

fn total(p: *Shape) -> f64:
    p.area()                    # the receiver is &*p
```

`p.area()` lends `&*p` with a shared call loan for the call, and
elision removes it because `area` writes nothing through a pointer.

### The Mechanism In Detail (formerly E.2)

- **Header.** Descriptor bits 16 to 21: bit 21 exclusive, bits 16 to 20
  the shared count. The release glue's slot narrows to bits 0 to 15; the
  collector's colors and flags keep bits 22 to 31. Every loan update is a
  read-modify-write that keeps the other bits, as a count update is. A
  static object has the same header. A static cell (`crates/hd_wasm/DESIGN.md` §23.20) has only a
  count word, so one that a closure shares gains a descriptor word: 4
  bytes of static data, no heap object.
- **Spill.** A shared begin at count 31 calls `rt:loan_spill`, which adds
  to a side table keyed by address and leaves the count at 31, so the
  inline check still sees a shared loan; the matching end consults the
  table first.
- **Own loans.** The checker counts, per pointer local and program point,
  the loans its references hold, and the emitted check compares the state
  with that constant. Where the count is not one constant (a loan taken
  on one branch), a local counter holds it.
- **Suspension.** A loan held across a bang call stays set; the frame
  keeps the object's address for the end. Abandoning a task runs its
  loan ends, as it runs its drops.
- **Closures.** A closure passed directly to a parameter may hold a
  reference; its loan lasts the call that receives it. A write through a
  `*T` the closure captured is checked like any other, against the
  creator's own loans through the same pointer.
- **Erased generics and trait values.** The state is in the header, so an
  erased body or a vtable method takes loans and checks with no witness.
  A loan is taken where the reference crosses the pointer, usually the
  concrete caller; a `*mut dyn` receiver takes a call loan.
- **Views (historical).** A representation view stood for an open
  reference argument inside the caller's call loan; #1320 gave it an
  explicit owner. #1448 removed those objects on 2026-11-08.
- **Cycles.** A borrowed object is reachable from the live root of its
  loan, so the collector never frees it; trace glue reads fields without
  checks.
- **Elision (release).** A type `T` drops its loans and checks when no
  reference through a `*T` to `T` outlives its expression and no call that
  lends one can reach a `T` through another pointer, or when no write
  through a `*T` reaches a `T`. Erased bodies count against every type
  they are instantiated at.

### References Count Nothing (formerly phase 10)

Every local reference is a plain address that counts nothing, and the
ownership passes keep its root alive until its last use
(`ref.count.local`, `ref.box.root`). The new object `&e` makes belongs to
the reference that made it: a hidden local the passes keep alive, a
static cell in a top-level binding's initializer, or, for a default
argument's `&mut []`, an object the caller releases after the call. The
pin is gone: no `Pin` statement, no eager copy in `coll_copy`, and no
owner-word test in `rt:list_unique`, `rt:map_unique` or a push. A
`Buffer`'s room keeps no count (#1321).

A borrowed `&dyn` reference counts nothing too (#1354); an owned trait
value still releases the counted value it owns, as described above.
A suspending function's frame can keep reference parameters alive: its
cold constructor takes the required object counts and releases them when
that frame ends or is dropped. These keepalive counts do not make an
ordinary local reference or a `Buffer` room reference counted.

## Raw Buffers

The two collection families stay as decided: `List[T]` and `Map` are
`Copy` with copy-on-write and are counted; `UniqList[T]` and `UniqMap`
are `!Copy`, have one owner, and take no count and no copy-on-write
check. What changes is the storage under them:

- `std.rt.memory.Buffer[T]` is raw memory: an address and a capacity.
  It is `!Copy`, has one owner, and dropping it frees the memory and
  nothing else. It knows nothing about elements: no length, no element
  references, no element drops.
- `UniqList[T]` is a buffer plus a length. It moves elements in and out
  through `at(i)`, gives element references, and drops its first `len`
  elements before its buffer frees the memory. `UniqMap` is the same over
  a buffer of slots. `List[T]` and `Map` stay compiler built-ins with
  their own counted storage (owner, 2026-11-12).
- One invariant holds in every build: **no value of a non-copyable type
  is ever copied**. The ownership pass never chooses a copy for one, and
  a verifier checks it after the pass.

```text
# hypothetical syntax; std.rt.memory
pub struct Buffer[T < ?Copy]:           # raw memory; dropping it frees the room, never an element
    address: usize
    capacity: usize

impl[T < ?Copy] !Copy for Buffer[T]

# std.collections
pub struct UniqList[T < ?Copy]:
    buffer: Buffer[T]
    len: usize

impl[T < ?Copy] !Copy for UniqList[T]

impl[T < ?Copy] Drop for UniqList[T]:   # std.rt.Drop: only std implements it
    fn drop(&mut self) -> void:
        let index: usize = 0
        while index < self.len:
            _ := self.buffer.at(index).read()   # moves the element out; its glue runs
            index += 1
        # then the buffer field's own drop frees the room
```

- **Buffer.** `Buffer::new(capacity)` allocates room for `capacity`
  values, `at(i)` is a `Ptr[T]`, and `grow(capacity)` moves the bytes to
  a larger room. Its glue frees the room and drops nothing; it is never
  counted.
- **Drop.** `std.rt.Drop` has one method, `fn drop(&mut self)`. A type
  that implements it gets glue that calls `drop`, then releases its
  fields. Only std may implement it; user code keeps `Disposable` and
  `using` for resources.
- **The rule.** The ownership pass decides each read's mode
  (`ReadMode` in `crates/hd_tir/src/ir.rs`). A read of a `!Copy` value starts as the checker's decision: a
  move at a move site ([`ref.move.sites`](../../spec/lang/13-values-and-references.md#r-ref.move.sites)),
  else a borrow. A use the checker accepted but that needs a copy is a
  compiler bug, reported as an internal error, never emitted.
- **The verifier.** After the ownership passes, no `Read` of a `!Copy` type has mode
  `Copy`, and no retain names a `!Copy` type. It runs on every body.
- **The collector.** A free during a trace is deferred to the `owned`
  list, which keeps today's free order: a white holder's room is freed
  after `later` is released.
- **The test matrix.** One generated conformance family covers every
  erased call shape, reference kind (`&`, `&mut`, value), receiver
  (`&self`, `&mut self`, `self`) and element kind (`Copy`, counted,
  `!Copy`), each run in checked builds.

