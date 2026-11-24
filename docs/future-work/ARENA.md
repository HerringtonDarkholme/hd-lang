# Std Arena Allocator

**Status:** proposal, 2026-11-23, rewritten the same day for the owner's
API (untyped, `&mut T` results); domain: std memory and lifetimes
([#1410](https://github.com/HerringtonDarkholme/hd-lang/issues/1410)); not
accepted. The signatures and examples are the package [arena/](arena/);
the stress test is [ARENA_STRESS_TEST.md](ARENA_STRESS_TEST.md).

## The Proposal

The owner's API, bumpalo's `Bump`: one untyped `std.arena.Arena`, and
`alloc[T](&self, val: T) -> &mut T`. Results are disjoint and live at
once, none outlives the arena, and `reset` is rejected while any is live.
**hd cannot express it today**: the checker makes each `&mut` result a
`&mut` loan of the arena, so the second `alloc` is `conflicting-access`.
It needs one missing primitive and two missing intrinsics:

- **`@interior`**, a std-only marker on `Arena`'s declaration: a `&` of
  such a value stays a shared loan when a `&mut` result holds it, as
  Rust's borrow checker treats `&'a Bump -> &'a mut T`. It is hd's name
  for the `UnsafeCell` that bumpalo and typed-arena build on. About 35
  checker lines.
- **`needs_drop[T]()`** and **`align_of[T]()`** in `std.rt.memory`, as
  Rust's `core::mem` has them. About 30 backend lines.

```text
use std.arena.Arena

struct Account:
    owner: string
    balance: i64

fn open(bank: &Arena, owner: string, balance: i64) -> &mut Account:
    bank.alloc(Account { owner: owner, balance: balance })

fn settle(bank: &Arena) -> i64:
    alice := open(bank, "alice", 100)
    bob := open(bank, "bob", 5)        # `alice` is still live
    alice.balance -= 30                # both written at once
    bob.balance += 30
    note := bank.alloc("moved 30")     # another type, same arena
    alice.balance + bob.balance + i64(note.len())
```

## Open Questions

1. Add `@interior` (std-only, in `std.rt.memory`) to close the gap?
   **Recommended: yes**; the other primitives are compared below.
2. Add `needs_drop[T]()` and `align_of[T]()` to `std.rt.memory`?
   **Recommended: yes**; without `needs_drop` a glue-free value pays a drop
   entry, which gave back all of `binary_trees`' gain (297 to 296 ms).
3. `Arena` neither `Send` nor `Sync`? Its values may hold non-atomic
   counts of any type. **Recommended: neither**; one arena per thread.
4. `reset` keeps the newest chunk, as bumpalo does. **Recommended: yes.**
5. Drop order: newest first (`reset` and drop). **Recommended: yes.**

## The Gap, Exactly

[`ref.excl.call`](../../spec/lang/13-values-and-references.md#r-ref.excl.call)
ends: "A result of type `&mut T` holds that reference as a `&mut` one
after the call, whatever the receiver or parameter kind". The checker does
it in `mut_result`
([`exclusive.rs:386`](../../crates/hd_check/src/dep_results/constraints/exclusive.rs#L386)),
which turns the call's shared receiver loan exclusive (`:793`). So
`a := arena.alloc(x)` holds `&mut arena`, and `arena.alloc(y)` while `a`
is used is `conflicting-access` ([`ref.excl.mut`](../../spec/lang/13-values-and-references.md#r-ref.excl.mut)).
Writing the lifetime changes nothing: `alloc['s, T](&'s self, v: T) ->
&'s mut T` gives the same error. A free `'a` (`-> &'a mut T`) checks, but
is unsound: nothing ties the result to the arena, so it outlives `reset`
and drop. `Ptr.as_mut` inside `alloc` does not help; the caller reads only
the signature. A user package cannot try any of this: `std.rt.memory` is
`std-only-use`.

The sentence is there for a reason: hd lets a `&self` method reborrow a
`&mut` field, `fn get(&self) -> &mut i32: &mut *self.r` with
`r: &'a mut i32` checks today, and two such results would alias with no
run-time loan to catch it. So the fix cannot delete it.

## The Primitives Compared

Each was checked on the five stress cases (tree linked after allocation,
graph with cycles, parser patching nodes, interner, `binary_trees`) with
a prototype where one exists. "Counts back" means the case had to give
values a count, the cost an arena exists to remove.

| Primitive | Sound because | Checker price | Stress cases | Counts back |
| --- | --- | --- | --- | --- |
| **`@interior` on `Arena`, an `UnsafeCell`-like type marker** (recommended) | every `&self -> &mut` method of a marked std type hands out fresh bytes (`module.rt.ptr.as-ref`); std-only by `module.rt.std-only`; the result still holds `&arena`, so `reset`, a move and a drop wait | about 35 lines (estimate): a `TypeAttrs` flag and its wire bit, the inert prototype's test in `mut_result` | as the inert prototype: 5 of 5 check; the graph keeps a `*mut` edge list and shared bindings, as Rust needs `Cell` | graph edge lists only |
| Inert receiver (no name: a type of only scalars keeps the `&` shared) | an inert value reaches no `&mut`; std-only `Ptr` is the only other source | 29 lines, prototyped; passed `lang/references` and `lang/functions` (3,024 cases, three modes) | 5 of 5, as above; the package checks | same |
| `@disjoint` on the `alloc` method | as the marker, for one method's results | 32 lines, prototyped (`CallAttrs` flag, wire bit, decorator, `mut_result`); passed the same 3,024 cases | 4 of 5: the parser's `parse(arena, ..) -> &'a mut Expr` is no `@disjoint` call, so its result locks the arena again; the package's `open(bank, ..)` helper does too | same, where it checks |
| Region or provider-scope lifetime | results tied to a scope, not to a borrow | a new lifetime kind through `dep_results`; hundreds of lines; not pursued: the arena is an explicit parameter (owner, 2026-11-24) | parse only | none |
| Counted handles (`alloc` returns `*mut T`) | today's rules | 0 | 5 of 5 after every type becomes `*mut` | all: `binary_trees` stays at today's 297 ms |
| (first draft) `&T` results | today's rules | 0 | 2 of 5; tree, graph and parser need `*mut` fields | every field that changes |

The method attribute fails where the owner's API is most useful: a
helper that returns what it allocated (`fn open(bank: &Arena, ..) -> &mut
Account`) is an ordinary function, so its result locks `bank` again.
Rust has the same signature, and its borrow checker holds `&'a Arena`
shared for any function, which a type-level rule reproduces. The marker
wins over the inert rule because it is explicit and cannot widen by
accident: a future scalar-only `Cell` type would turn the inert rule
unsound. For `Arena`, whose one field is `state: usize`, the marker and
the inert rule give every caller the same loan, so the inert prototype's
results stand for the marker's; the marker itself is not prototyped.

## Problems It Solves

`bench/study/binary_trees.hd` at depth 17, release build, V8 default
tier, median of 21 fresh processes (machine load about 9); an empty
program takes 87 ms, so the work is the wall time minus 87 ms.

| Build | Wall | Work | Wasm |
| --- | --- | --- | --- |
| today | 297 ms | 210 ms | 10,981 bytes |
| untyped arena, `needs_drop` false for `Node` | 197 ms | 110 ms (1.9x less) | 8,407 bytes |
| untyped arena, a drop entry per value | 296 ms | 209 ms | — |

The untyped arena gives the same output. Its `alloc` and the bump are not
force-inlined (bug 2 of the first draft still blocks `@inline(.Always)` on
`alloc`); the first draft's typed arena measured 178 ms with it. The probe
fixed `needs_drop` per build, since the intrinsic does not exist yet.

**Trees, parsers and frames.** The stress test's tree linked after
allocation, its parser patching every node after the parse, and the
package's frame builder all write through `&mut` results while others are
live; with `&T` each needed a `*mut` field, a count per value.

## How It Works

| Item | What it is for |
| --- | --- |
| `Arena` | values of any types that live and die together; `!Copy`; neither `Send` nor `Sync` |
| `new()`, `with_capacity(bytes)` | an empty arena; the second sizes its first chunk |
| `alloc[T](&self, val: T) -> &mut T` | moves `val` in at `T`'s alignment; lends it writable while the arena is borrowed |
| `allocated_bytes(&self)` | bytes of values and padding, as bumpalo's |
| `reset(&mut self)` | drops every value, newest first; keeps the newest chunk |

**Layout.** A 16-byte state block: the bump pointer, the chunk end, the
newest chunk, and the drop list's head. A chunk is one `std.rt.alloc`
block: the previous chunk, its byte size, then values. Chunks double from
4 KiB to 1 MiB, or fit one larger value.

**Alignment.** `align_of` is missing from `std.rt.memory`. The probe
derives it from `size_of`: hd lays a type out at a multiple of its
alignment, which is at most 8 (`hd_layout`), so the lowest set bit of the
size, capped at 8, always suffices. It over-aligns some types (a
`(u32, u32)` at 8), so the intrinsic is proposed.

**Drop.** A value whose type has drop work (`needs_drop[T]()`, a
compile-time constant) gets a 12-byte entry in the arena: the previous
entry, its address, and `drop_at::[T]`, which reads the value out so its
glue runs. `reset` and drop walk the list newest first. A glue-free type
writes nothing, as the folded branch above shows. The checked count audit
of a probe with `string` and `List` values in three rounds of 100 is clean
with the list, and reports 600 leaked blocks without it.

**What stays rejected** (probes on the prototype):

| Misuse | Rule | Error |
| --- | --- | --- |
| use a value after `reset` | `ref.excl.shared` | `conflicting-access` |
| move or drop the arena while a value is used | `ref.excl.move` | `conflicting-access` |
| return a value past the arena's function | `ref.dep.return` | `local-reference-escapes` |
| store a value in a field without `'a`, or a `'static` bound | `ref.local.escape.*` | `local-reference-escapes` |
| capture a value in a closure that outlives the call | `ref.local.escape.capture-stored` | `local-reference-escapes` |
| `alloc` from another thread | `&Arena` is not `Send` | `unsatisfied-trait-bound` |
| write a value from a scoped thread | `&mut T` is not `Send` | `unsatisfied-trait-bound` |
| two `&mut` from a `&self` method of an unmarked type | `ref.excl.call`, unchanged | `conflicting-access` |

## Cost

| Component | Price |
| --- | --- |
| Checker | about 35 lines: `@interior` in `TypeAttrs` and its wire bit, read by `mut_result`; nothing deleted |
| Backend | about 30 lines: `ptr_needs_drop` and `ptr_align_of` beside `ptr_size_of` in `hd_wasm`'s `ptr` |
| `lib/std/arena.hd` | about 140 lines; the probe is 120 |
| Spec | one sentence in `ref.excl.call`, three `module.rt` rules, `spec/std/arena.md` (about 70 lines) |
| Compile time | none on a program that names no `Arena`; one flag test per `&mut` result |
| Runtime, per call | `alloc` O(1) amortized, plus one 12-byte entry for a value with drop work; `reset` O(values with drop work + chunks) |
| Wasm size | binary_trees 10,981 to 8,407 bytes |

## What The First Version Got Wrong

It recommended `&T` results and said "0 compiler lines", calling the
`&mut` lock a reason to change the API. That moved the cost to users:
every part that changes after allocation became a `*mut` field, a count
per value, in the tree, graph and parser cases alike. It was typed only,
so a parser needed one arena per node type. And it ran no stress test of
code that links or patches values after allocation, which is most arena
code.

## Appendix A. Stress Test

In [ARENA_STRESS_TEST.md](ARENA_STRESS_TEST.md): five cases, each checked
against `&mut` results with and without the rule, `&T` results, and
counted handles.

## Appendix B. Probes

The probe std module, the prototype patch and the programs are in
scratch, not in this PR.

| Probe | Result |
| --- | --- |
| two live `alloc` results on main, in std (`&'s self -> &'s mut T`) | `conflicting-access` at the second |
| the same with a free `'a` result | checks; unsound, it survives `reset` |
| untyped `alloc` of `Node`, `i64`, `u8` and a `Label` with a `string` and `List`, three rounds with `reset` | right output, 128 bytes per round |
| checked count audit of 300 values with counted parts | clean with the drop list; 600 leaked blocks without |
| `binary_trees` | same output as today |
| bug 1 (first draft, no arena) | through `Arc.get()`, `println("${f.edges.len()}")` then `e := f.edges; e.push(8)` fails "memory access out of bounds" |
| bug 2 (first draft) | `@inline(.Always)` on an `alloc` of `Ptr.write` then `as_ref` loses a later push through a field |
| bug 3 (first draft) | a top-level `let` arena in a program that starts a thread panics `integer-overflow` in the arena's drop |

## Appendix C. Prior Art

**Rust `typed-arena`.** "A fast (but limited) allocation arena for values
of a single type"; values are "destroyed all at once"; "you can safely
create cycles" ([docs](https://docs.rs/typed-arena/latest/typed_arena/)).
Its `alloc(&self, value: T) -> &mut T` takes `self.chunks.borrow_mut()`
(a `RefCell<ChunkList<T>>`), pushes only below capacity so no value
moves, and returns `unsafe { &mut *chunks.current.as_mut_ptr().add(len) }`
([source](https://github.com/thomcc/rust-typed-arena/blob/master/src/lib.rs)).
hd takes its drop of values; `@interior` is hd's name for that `RefCell`
and `unsafe`.

```rust
struct Node<'a> { left: Option<&'a Node<'a>>, right: Option<&'a Node<'a>> }
fn make<'a>(arena: &'a Arena<Node<'a>>, d: u32) -> &'a Node<'a> {
    if d == 0 { return arena.alloc(Node { left: None, right: None }); }
    let (l, r) = (make(arena, d - 1), make(arena, d - 1));
    arena.alloc(Node { left: Some(l), right: Some(r) })   // alloc(&self, T) -> &mut T
}
```

**Rust `bumpalo`.** `alloc<T>(&self, val: T) -> &mut T` for any `T`;
its fields are `Cell`s (`current_chunk_footer: Cell<NonNull<ChunkFooter>>`),
`alloc`'s body is `self.emplace().write(val)`, and `reset(&mut self)`
needs "self must be unique and there can't be any borrows active";
`unsafe impl Send`, no `Sync`; "Objects that are bump-allocated will
never have their Drop implementation called"
([source](https://github.com/fitzgen/bumpalo/blob/main/src/lib.rs)). It
is sound because each `alloc` hands out bytes no earlier result covers,
and Rust's borrow checker holds `&'a self` shared while each `&'a mut T`
lives, so `reset` waits. hd takes the API, `reset`'s policy and that
argument. It rejects skipping drops: an hd value's counted parts would
leak (the probe below leaks 600 blocks without the drop list).

```rust
let mut bump = Bump::new();
for frame in frames {
    let page = render(&bump, frame);   // allocations of any type
    send(page);
    bump.reset();                      // no Drop runs
}
```

**Rust `UnsafeCell`.** "The core primitive for interior mutability in
Rust"; "`UnsafeCell<T>` opts-out of the immutability guarantee for
`&T`"; and "if you create a `&mut T` reference, then you must not access
the data within the `UnsafeCell` with any other pointer/reference until
that reference expires"
([docs](https://doc.rust-lang.org/std/cell/struct.UnsafeCell.html)).
Rust's checker never sees disjointness: `unsafe` code promises it. hd has
no `unsafe`, so the promise needs a std-only name the checker reads.

```rust
pub struct Cell<T: ?Sized> { value: UnsafeCell<T> }
impl<T> Cell<T> {
    pub fn get_mut(&mut self) -> &mut T { self.value.get_mut() }
    // `&self` methods write through `self.value.get()`, a `*mut T`
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

## Appendix D. Alternatives Considered

- **Delete the last sentence of `ref.excl.call`.** Unsound, by the
  `&mut *self.r` example above.
- **A written lifetime meaning a shared hold.** A written lifetime would
  mean something its elided form does not, and the hole reopens for
  anyone who writes one.
- **A typed arena with `&T` results** (the first draft): see above.
- **An allocator parameter on `List` and `Map`**, **a counted region**,
  **Go-style regions**: as in the first draft; each adds work to every
  program or every collection.

## Appendix E. Today In hd

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

## Appendix F. Details

- **Typing.** No new type rule; `alloc[T]` is an ordinary generic method.
- **Ownership.** `@interior` changes only `mut_result`: a `&` of a
  marked value stays shared; everything else of `ref.excl` holds.
- **Soundness.** Each `alloc` writes bytes no earlier result covers; std
  keeps that (`module.rt.ptr.as-ref`). Results hold `&arena`, so `reset`
  (`&mut self`), a move and a drop wait for them.
- **Threads.** Neither `Send` nor `Sync` (question 3).
- **Diagnostics.** None new.

**Phases**, each about one hour with its test:

1. `@interior` in spec 13 and `hd_check`, with a valid stage case (two
   live results, and a helper returning one) and an invalid one (the
   `&mut *self.r` getter of an unmarked type).
2. `needs_drop` and `align_of` intrinsics with conformance cases.
3. `lib/std/arena.hd`, `spec/std/arena.md`, runtime fixtures in checked
   builds: mixed types, glue values, zero-sized, nested arenas.
4. Invalid fixtures, one per misuse row.
5. Bugs 1 to 3 of the first draft: bisect and fix.

**Checks.** In `docs/future-work/arena/`: `hd fmt --check` passes;
`hd check` passes with phase 1's rule (verified on the prototype) and on
today's checker reports the five `conflicting-access` errors the rule
removes.
