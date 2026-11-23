# Std Arena Allocator

**Status:** proposal, 2026-11-23; domain: std memory and lifetimes
([#1410](https://github.com/HerringtonDarkholme/hd-lang/issues/1410)); not
accepted. It builds on spec chapter 13
([Local References](../../spec/lang/13-values-and-references.md#local-references),
[Dependent Results](../../spec/lang/13-values-and-references.md#dependent-results),
[Lifetime Structs](../../spec/lang/13-values-and-references.md#lifetime-structs)),
the [markers](../../spec/lang/10-modules.md#markers) and
[ownership.md](../compiler/ownership.md). The signatures and examples are
the checked package [arena/](arena/).

## The Proposal

Add `std.arena.Arena[T]`, a typed arena written in std over `Ptr` and
`std.rt.alloc`, as `std.sync.Arc` is. `alloc(&self, value)` moves a value
in and lends it back as `&T`, tied to the arena's borrow. Arena values
carry no header and no count. `reset` or the arena's drop frees them all
at once. Escapes, use after `reset` and races are compile errors under
today's lifetime and marker rules. End state: **0 compiler lines**, about
150 std lines, and 2.2x to 2.3x less work on node-churn programs.

```text
use std.arena.Arena

struct Tree['a]:
    left: (&'a Tree['a])?
    right: (&'a Tree['a])?

fn build['a](arena: &'a Arena[Tree['a]], depth: i32) -> &'a Tree['a]:
    if depth == 0:
        return arena.alloc(Tree { left: .None, right: .None })
    left := build(arena, depth - 1)
    right := build(arena, depth - 1)
    arena.alloc(Tree { left: .Some(left), right: .Some(right) })

fn nodes(t: &Tree) -> i64:
    match (t.left, t.right):
        (.Some(l), .Some(r)) => 1 + nodes(l) + nodes(r)
        _ => 1

fn rounds(depth: i32, n: usize) -> i64:
    let mut scratch = Arena::[Tree]::new()
    let total: i64 = 0
    for _ in 0..n:
        total += nodes(build(&scratch, depth))  # no count, no free per node
        scratch.reset()                         # one walk of the chunks
    total
```

## Open Questions

1. Ship `std.arena.Arena[T]` as above? **Recommended: yes.**
2. `alloc` lends `&T`, not Rust's `&mut T`: a `&mut` result locks the whole
   arena (`ref.excl.mut`), so a second `alloc` is `conflicting-access`.
   Parts that change are `*mut` fields. **Recommended: `&T`.**
3. Typed arenas only, no mixed-type `Bump`: a `Bump` needs a no-glue bound
   or a drop list per value. **Recommended: typed only.**
4. `reset` keeps the newest chunk, as bumpalo does, not all capacity, as
   Zig's `retain_capacity` does. **Recommended: newest chunk.**
5. Module name `std.arena`, apart from `std.collections`, as Rust's
   `typed_arena` and Zig's `std.heap` are. **Recommended: `std.arena`.**
6. A top-level `let` arena lends `&'static T` (an interner), and no thread
   can reach it. **Recommended: allow it, once bug 3 below is fixed.**
7. A count cycle through arena values is not collected: it leaks, as a
   cycle through `Arc` does today. Collecting it needs the collector's
   type walk to see `T`. **Recommended: leak, and say so in the spec.**

## Problems It Solves

Each program ran in a release build on V8's default tier, as the median
of 21 fresh processes (machine load 11 to 17). An empty program takes
88 ms, so the work is the wall time minus 88 ms. Counts come from the
checked runtime (`bench/counts/measure.ts`); arena allocations follow
from its growth rule.

| Program | Today | With `Arena` |
| --- | --- | --- |
| `bench/study/binary_trees.hd`, depth 17 | 293 ms; 29,971,806 allocations and frees, one per node; 29,971,824 releases | 178 ms (work 2.3x less); 24 allocations (chunks and state); 18 releases |
| #1407's `alloc` shape, depth 16, 60 rounds | 155 ms; 7,864,260 allocations; 7,864,262 releases | 118 ms (work 2.2x less); 69 allocations; 2 releases |
| `bench/runtime/progs/expr-eval.hd` | 357 ms; 3,650,726 releases | 367 ms, within noise; 1,259,246 releases |

**Trees built and dropped in rounds.** Today each node costs one
allocation, one release, one glue visit and one free (#932). The arena
costs a bump: two loads, a compare and a store, inlined. `reset` frees
chunks, not nodes. This matches #932's region ceiling (about 2.2x)
without a region runtime.

**A parser's syntax tree.** `expr-eval` keeps its tree in an arena held
by a lifetime struct `Parser['a]`, as rustc and swc do. The port gives
the same output and removes 2.39M of 3.65M releases, but saves no
measurable time. An arena pays only where allocation dominates.

**A frame rebuilt per request** ([example 3](arena/src/examples.hd)), as
dodrio renders into bumpalo. Values with counted parts (a `string`, a
`List`) are released by `reset`, so nothing leaks. A checked build of
5,300 such values made 15,904 releases and no leak.

## How It Works

| Item | What it is for |
| --- | --- |
| `Arena[T]` | values of one type that live and die together; `!Copy`; `Send` when `T` is; never `Sync` |
| `new()`, `with_capacity(n)` | an empty arena; the second sizes its first chunk |
| `alloc(&self, value: T) -> &T` | moves `value` in; lends it while the arena is borrowed |
| `len(&self)` | how many values it holds |
| `reset(&mut self)` | drops every value; keeps the newest chunk |

**Layout.** An `Arena` holds the address of a 16-byte state block: the
bump pointer, the chunk end, the newest chunk and its capacity. A chunk
is one `std.rt.alloc` block: the previous chunk, its used end, then the
values at `size_of::[T]()` apart. Chunks double from 4 KiB to 1 MiB.

**Counts.** A value in an arena is not an object: no header, no count.
Code reaches it only through `&'a T`, which counts nothing
(`ref.type.local-uncounted`). No `*T` can point into a chunk, since every
counted reference starts at a whole object (`ref.box.object`). A counted
part of a value, such as a `List` field, holds its usual count, which the
arena releases at `reset` or drop.

**Lifetimes.** `alloc`'s result depends on `self` (`ref.dep.elide`), so
`'a` is the arena's borrow. Values link to each other through `&'a`
fields of lifetime structs and enums. The checker already rejects every
misuse; the probes gave these diagnostics:

| Misuse | Rule | Error |
| --- | --- | --- |
| use a value after `reset` | `ref.excl.shared` | `conflicting-access` |
| move or drop the arena while a value is used | `ref.excl.move` | `conflicting-access` |
| return a value past the arena's function | `ref.dep.return` | `local-reference-escapes` |
| keep a value in a `*T`, a field without `'a`, or a `'static` bound | `ref.local.escape.*`, `ref.generic.static.error` | `local-reference-escapes` |
| capture a value in a closure that outlives the call | `ref.local.escape.capture-stored` | `local-reference-escapes` |
| `alloc` from another thread | `module.marker.table.shared-sync` | `unsatisfied-trait-bound` |

A `&T` crosses to a scoped thread when `T` is `Send` and `Sync`: a tree
of `&'a` links and numbers does, a value with a `List` does not.

**Drop.** `Arena` implements `std.rt.Drop`, as `UniqList` does. Its hook
reads every value out of each chunk, so each value's glue runs, then
frees the chunks. When `T` has no glue, the walk's loop folds away (the
measured build has no item loop), so `reset` costs one step per chunk.
Glue never reads through a local reference, so the order values die in
does not matter. An arena dropped inside a traced object's glue frees
its chunks through `hold_room`, as `Buffer`'s room does.

## Cost

| Component | Price |
| --- | --- |
| Compiler, end state | 0 lines added, 0 deleted: `Ptr`, `alloc`, `Drop`, explicit `Send` and the lifetime solver exist |
| Compiler bugs the probes hit | bug 2 blocks the inlined `alloc`; bug 3 blocks a top-level arena with threads; sizes found in phase 1 |
| `lib/std/arena.hd` | about 150 lines; the probe was 118 lines of code |
| `spec/std/arena.md` and fixtures | about 70 spec lines; about 12 conformance cases |
| Runtime, per call | `alloc` O(1) amortized; `reset` and drop O(values) when `T` has glue, else O(chunks); `len` O(chunks) |
| Runtime, measured | 2.3x and 2.2x less work above; no change on `expr-eval` |
| Memory | a 12-byte node takes 12 bytes against a 32-byte block today; at most half of the newest chunk unused; peak RSS 104 to 100 MiB on binary_trees |
| Wasm size | binary_trees 10,981 to 8,297 bytes; a program that names no `Arena` compiles none of it |
| Web compiler gzip | about +1.1 KB against the 2 MB cap |

**Diff against today's std.** Nothing changes; `std.arena` is new.

**Not supported, with reasons.** `&mut T` results (question 2). A mixed
`Bump` (question 3). Arena-backed `List`, `Map` or `string`: they are
counted compiler built-ins (owner, 2026-11-12), and an allocator
parameter would reach every collection's glue. A `Sync` arena shared by
threads: it needs an atomic bump; use one arena per thread. Rust's
`alloc_extend`, `into_vec` and `iter_mut`: no measured program needs them.

## Appendix A. Stress Test

The probe std module and programs are in scratch, not in this PR. Each
was run against the probe built into a local `hd`.

| Case | What happened | What the design does |
| --- | --- | --- |
| binary_trees (benchmarks game) | same output; 2.3x less work | the main case |
| #1407 `alloc` | same checksum; 2.2x less work | same |
| `expr-eval` parser with `Parser['a]` holding `&'a Arena[Expr['a]]` | same output, same time; 251,748 retains appear, all turned away as static data | arena only where allocation dominates; the retains are a lead for phase 2 |
| dodrio-style frame with `string` and `List` parts | checked build: no leak | `reset` runs glue |
| A first probe walked values at a rounded stride | the checked leak audit reported 7,950 leaked blocks | the stride is `size_of`, as `Ptr.offset` steps; fixtures run checked builds |
| typed-arena's "safe cycles": cities with `roads: *mut List[&'a City['a]]` | runs; edges count nothing, so the cycle is no count cycle | parts that change are `*mut` fields |
| the same with `@inline(.Always)` on `alloc` | a push through `x.roads` is lost (`len` 0); nested arenas report a wrong `len` | **bug 2**: fix before inlining `alloc`, which buys 223 to 186 ms on binary_trees |
| `alloc_mut(&self) -> &mut T` | the second call is `conflicting-access` | question 2 |
| `&Arena` captured by `s.spawn` | `unsatisfied-trait-bound` | not `Sync` |
| `&City` with a `*mut` field to `s.spawn` | `unsatisfied-trait-bound` | a counted part is not `Sync` |
| `&Tree` to two scoped threads | runs, 30 nodes | read-only sharing works |
| moving an `Arena` into `s.spawn` | `move-out-of-place` | the crossing-closure rule, not the arena |
| zero-sized `T`, 10,000 values | `len` 10,000, then 0 after `reset` | a zero-sized value takes one byte |
| `Arena[Arena[Entry]]` with `Arc` fields | counts right without forced inlining | drop order is free |
| a count cycle box -> `Arena` value -> box, 200 times | the checked audit reports 600 leaked blocks; the same cycle through `Arc` leaks 400 | question 7 |
| the same with an empty `Buffer[T]` field, so the type walk sees `T` | the audit reports 400 blocks freed twice | collecting needs a trace-safe hook; not in this design |
| a top-level `let` arena | `&'static T` values; a thread that reaches it gets `crossing-top-level-binding` | question 6 |
| the same in a program that starts a thread | init panics `integer-overflow` in `Arena.drop` | **bug 3**, not bisected |
| keeping a value in a `*Plain` result | `local-reference-escapes`, worded "escapes to a function value's result" | a diagnostic wording lead |

**Bug 1, found on the way, without the arena.** Through `Arc.get()`,
`println("${f.edges.len()}")` and then `e := f.edges; e.push(8)` fail
with "memory access out of bounds" in dev and release. Through a plain
`&Flat` it runs. It needs its own ticket.

## Appendix B. Prior Art

**Rust `typed-arena`.** "A fast (but limited) allocation arena for values
of a single type"; values are "destroyed all at once"; "you can safely
create cycles" ([docs](https://docs.rs/typed-arena/latest/typed_arena/)).
hd takes the typed shape and the drop of values; it lends `&T`, not
`&mut T`.

```rust
struct Node<'a> { left: Option<&'a Node<'a>>, right: Option<&'a Node<'a>> }
fn make<'a>(arena: &'a Arena<Node<'a>>, d: u32) -> &'a Node<'a> {
    if d == 0 { return arena.alloc(Node { left: None, right: None }); }
    let (l, r) = (make(arena, d - 1), make(arena, d - 1));
    arena.alloc(Node { left: Some(l), right: Some(r) })   // alloc(&self, T) -> &mut T
}
```

**Rust `bumpalo`.** `alloc<T>(&self, val: T) -> &mut T` for any `T`;
`reset(&mut self)`; `Send` and `!Sync`; on mass deallocation "allocated objects' Drop
implementations are not invoked"
([Bump](https://docs.rs/bumpalo/latest/bumpalo/struct.Bump.html),
[crate](https://docs.rs/bumpalo/latest/bumpalo/)). hd takes `reset`'s
policy and the markers. It rejects skipping drops: an hd value's counted
parts would leak.

```rust
let mut bump = Bump::new();
for frame in frames {
    let page = render(&bump, frame);   // allocations of any type
    send(page);
    bump.reset();                      // no Drop runs
}
```

**Rust `Allocator`.** Stabilized for Rust 1.100 ([PR
#156882](https://github.com/rust-lang/rust/pull/156882), tracking issue
[#32838](https://github.com/rust-lang/rust/issues/32838) closed
2026-09-23): `Vec<T, A>` and `Box<T, A>` take an allocator, such as
`&Bump`. hd rejects the parameter (appendix C).

```rust
let bump = Bump::new();
let mut v: Vec<u32, &Bump> = Vec::new_in(&bump);  // bumpalo's allocator_api feature
v.push(1);
```

**Zig `std.heap.ArenaAllocator`.** It "takes an existing allocator,
wraps it", frees all together, and `free` frees only the last allocation;
`reset` takes `free_all`, `retain_capacity` or `retain_with_limit`
([source](https://github.com/ziglang/zig/blob/master/lib/std/heap/arena_allocator.zig)).
#1407 measured it at 66 ms on btree against hd's 95. hd takes bulk
reset; it rejects a plain allocator, since hd values have glue.

```zig
var arena = std.heap.ArenaAllocator.init(allocator);
defer arena.deinit();
while (r < rounds) : (r += 1) {
    const root = try buildTree(arena.allocator(), depth);
    sum += @intCast(walkTree(root));
    _ = arena.reset(.retain_capacity);
}
```

**Go arenas.** `arena.NewArena()`, `arena.New[T](a)`, `a.Free()`; a
use-after-free is "typically" a fault, but the package "reserves the
right to not force a fault"
([arena.go](https://github.com/golang/go/blob/master/src/arena/arena.go)).
The proposal is "on hold indefinitely due to serious API concerns"
(2023-01-17, [#51317](https://github.com/golang/go/issues/51317)). The
later regions discussion says arenas "compose poorly", and every API
must take "the arena to allocate into"
([#70257](https://github.com/golang/go/issues/70257)). hd makes the
use-after-free a compile error, which Go cannot. The extra argument stays:
an hd caller passes `&'a Arena`.

```go
a := arena.NewArena()
defer a.Free()
n := arena.New[Node](a)   // a use after Free may fault, or may not
```

**Swift.** No std arena. `withUnsafeTemporaryAllocation` (SE-0322,
Swift 5.6) lends a buffer that "must not escape"; `~Escapable` (SE-0446,
Swift 6.2) ties a value to a scope
([SE-0322](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0322-temporary-buffers.md),
[SE-0446](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md)).
Swift's class nodes pay one retain and release each, as hd's boxes do.
hd's `&'a` already is the non-escaping form.

```swift
withUnsafeTemporaryAllocation(of: Node.self, capacity: 64) { buffer in
    // buffer is freed when the closure returns; it must not escape
}
```

## Appendix C. Alternatives Considered

- **A counted region** (#932's "region, full header"). Objects keep
  their header, so `*T` can point in. It needs a current-region provider
  across nesting and suspension, a region bit on every retain and release
  path, and an outside-reference check at region end with a fallback.
  Every program pays the bit test, and misuse fails at run time.
- **An allocator parameter on `List` and `Map`** (Rust's `Vec<T, A>`,
  Zig). Both are counted copy-on-write built-ins. A buffer that may be
  shared cannot be freed by a reset, so the parameter would need a second
  ownership mode in every collection's glue.
- **Go-style regions.** Implicit: memory "fades" out of a region when it
  escapes, which needs a write barrier. hd has counts, not a tracer.
- **Young-object deferral.** Rejected by the owner on #932.
- **Only a faster allocator.** hd's allocator already beats Zig's
  general-purpose one on `alloc` (44 against 61 ms, #1407, 2026-11-12).
  What remains is count and free work that no allocator removes.

## Appendix D. Today In hd

- **Runtime.** `std.rt.alloc` serves 16-byte to 32 KiB blocks from size
  classes, and larger ones as page runs. Each thread allocates from its own
  heap, and a block freed elsewhere returns to its owner's list
  (`module.rt.thread.alloc`). An arena's chunks come from the thread that
  grows the arena.
- **Per node today** (release WAT of the `alloc` shape): `build_tree`
  calls the size-class allocator for a 12-byte payload, which writes the
  8-byte header; at the last use, `release` reaches zero, the glue
  releases both children, and `free_class` lists the block. With the
  arena, `alloc` is inlined as `next = load(state)`, a compare with
  `load(state + 4)`, and `store(state, next + 12)`; nothing runs at the
  last use.
- **`Arc` as the model.** `std.sync.Arc` is std code over `Ptr`, `alloc`
  and `Drop`, with an explicit `Send` impl
  (`module.marker.explicit.only`). The arena uses the same parts.
- **Counts are never atomic** (`module.marker.send.counts`). The arena
  counts nothing, is `Send` only by a move, and is never `Sync`.

## Appendix E. Details

- **Typing.** No new rule; `Arena[Node['a]]` is a type argument that holds
  a local reference (`ref.lifetime.store.argument`).
- **Ownership and the inliner.** Values written by `Ptr.write` and lent by
  `Ptr.as_ref`, as in `Arc.get`; bug 2 is the inliner's.
- **Soundness.** A value lives while the arena is borrowed; `reset` and
  drop need the arena unborrowed. Glue never reads through `&'a`.
- **Collector.** Values are not objects. A value's count on an object
  is an outside reference to trial deletion, so the object is never
  freed early. Links between values count nothing and form no cycle. An
  `Arena` holds only an address, so the type walk never marks a type
  cycle-capable through it, as with `Arc` (question 7).
- **Threads.** `Send` by move; `&Arena` never crosses; `&T` crosses when
  `T` is `Send` and `Sync`. A moved arena frees on another thread through
  the owner heap's list.
- **Suspension.** A value held across a bang call is a local reference
  in the frame (`ref.dep.suspension`); the probe compiled.
- **Diagnostics.** None new.

**Phases**, each about one hour with its test:

1. Bisect and fix bug 2 with a stage case; file bugs 1 and 3 with
   known-failure fixtures.
2. `lib/std/arena.hd` and `spec/std/arena.md`; runtime fixtures in
   checked builds: glue values, zero-sized, nested, cycle, threads.
3. Invalid fixtures, one per row of the misuse table.
4. Bisect bug 3; rerun the #1407 race with an hd arena row.

**Checks.** In `docs/future-work/arena/`: `hd check` and
`hd fmt --check`. Parse log: every `text` block above parses
(`scripts/hd-conformance.sh parse`); parsing checks syntax only.
