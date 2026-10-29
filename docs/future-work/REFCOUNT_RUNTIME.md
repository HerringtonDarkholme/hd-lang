# Refcount Runtime: Design Study

Status: design record (#426). The linear-memory runtime with reference
counting is the only backend (#626, #627). Sections 2 to 9 stay as the
measured reference.

## 1. The Problem

Objects live in Wasm linear memory and are freed by reference counting,
with a cycle detector for the garbage that counting cannot see. That is
decided. This study prices the runtime, in run time and in module size,
and plans how to build it so the cost stays small.

The decision comes with a new object model, taken from Go. Every
aggregate is a struct, a value stored inline: in a local, in its parent,
in a list's buffer. Sharing comes only from references (`&` and
pointers), and a reference may point into the middle of an object, at a
field or a list element. `data` is gone. Go manages this model with a
tracing collector; hd manages it with counts, escape analysis, and
opt-in lifetime marks that make "this reference does not escape" a
checked fact. Two separate studies design the language side: tier 1,
Go-style references that may go anywhere
(the tier-1 design, `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md`), and tier 2,
Rust-style lifetimes on request. This study prices the runtime under
both, and settles the runtime assumptions the tier 1 study marks "to be
confirmed" (section 10).

Three earlier measurements set the bar.

- **Allocating one object at a time is cheaper on Wasm GC than in
  linear memory.** In the Rust model study (`git show b3f5eed1a:docs/future-work/RUST_MODEL_STUDY.md`
  section 8), binary-trees allocates one object per node in both
  languages, and hd on Wasm GC ran 1,934 ms on V8 against Rust's 3,958
  with `Box` and `dlmalloc`, and 2,913 against 11,973 on wasmtime. The
  engine's bump nursery beats a general-purpose `malloc` and `free`.
- **Rust's lead elsewhere is layout, not ownership.** Records inline in
  a vector, words borrowed as slices, one growing buffer. Put back into
  hd's shape (an object per record, a string per word), Rust loses half
  or more of its lead in points, map-count and string-build.
- **hello.wasm is 2,935 bytes.** A program that never allocates must
  stay there.

So a counting runtime wins only if it allocates rarely (structs inline,
which Go's model gives), allocates fast where it must (a better
allocator than `dlmalloc`), and counts rarely (moves, borrows,
immutability). This study measures each of those on proxies written in
Rust and compiled to Wasm, then proposes the runtime.

The linear-memory layout is in
[crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md) §23.

## 2. The Runtime On One Page

| hd value | Representation in linear memory | Counted |
| --- | --- | --- |
| scalars, `bool`, `char` | Wasm values; packed in memory | no |
| struct | its fields inline: several Wasm values in locals, parameters and results; fields inline in its parent; elements inline in a list buffer | no; its reference fields are counted one by one |
| enum | tag and payload inline; recursion goes through a written `&`, a counted object | the referenced object |
| `string` | a view: address and length of bytes inside a counted buffer, or inside the data segment for a literal; every empty string is one shared static value | the buffer |
| `List[T]` | a value: a small header referring to a counted buffer of inline elements; copy-on-write; element references pin the buffer | the buffer: owners and pins, two counts |
| `Map[K, V]` | a value: a counted table of inline entries, copy-on-write | the table |
| `&T` | the address of a `T` inside a counted cell, a list element or a field | the enclosing object |
| closure | code index and a counted environment; no environment when nothing is captured | the environment |
| `dyn Trait` | data reference and a vtable (methods, release, trace) | the data |
| suspension frame | a counted object owned by its task | yes |

A counted object has an 8-byte header before it: a 32-bit count, and a
32-bit word holding its release-and-trace descriptor together with the
tier 2 borrow-flag bits (a collection buffer adds a second count for
pins, as the tier 1 study specifies). The descriptor is needed because
the last reference to an object may be an interior one: when the
`&mut i32` that `&mut packet.status` produced dies, the release must
destroy a whole `Packet`, and the reference's type says only `i32`. The
page table finds the header; the descriptor says what to release. Around that sit a size-class allocator (section 6), counting
rules decided from declarations and one body at a time (section 4),
interior references as plain addresses (section 5), and a cycle
collector that only cycle-capable types ever reach (section 7). Section
10 sums it up in ten lines.

## 3. Measurements

### 3.1 Method

The proxies are Rust programs built for `wasm32-unknown-unknown` with
`--release` (rustc 1.97.1), one module per program with one export per
variant, run on V8 through a small Node driver (Node v24.19.0). Each
program prints the same checksum in every variant and with every
allocator. Every time is the median of five runs after one warm-up
(`hyperfine`), as process wall time, so it includes start-up, as the
Rust study's times do: an empty run takes 27 ms.

**No wasmtime numbers.** `target/release/hd` was removed by a
`cargo clean`, and no wasmtime CLI is installed, so this study ran on V8
only. In the Rust study, binary-trees with `Box` and `dlmalloc` ran 3x
slower on wasmtime than on V8 (2x with epoch checks off), so the
allocator results below likely understate the gain on wasmtime.

**Machine.** Apple M3 Max, 14 cores, macOS 26.6.2. The machine was
busy at first: the wait for a one-minute load under 4 ran 17 minutes
and ended at 3.60; during the timed runs the load stayed between 1.6
and 2.8. Of 75 timed commands, 3 had a spread (slowest minus fastest of
five) above 10% of the median.

The variants model hd's future runtime:

| Variant | What it models |
| --- | --- |
| inline, arena, borrowed keys, direct | Go's model where it needs no object: structs in a list, nodes in one vector, words as views, one growing buffer |
| box, owned | an object per record or per string, with no count (the "Rust in hd's shape" of the earlier study) |
| rc naive | one 32-bit count per object; every copy into a local, field or argument increments and every end of a local decrements |
| rc borrow | the same objects with the elisions of section 4: moves at last use, borrowed parameters, borrowed reads |
| cc | the `bacon_rajan_cc` crate (Bacon and Rajan trial deletion) instead of plain counts |
| gcmodule | the `gcmodule` crate (CPython's scheme: every tracked object on a list) |
| cow | a counted buffer that checks it is unique before every write |

The counted variants use a hand-written `HRc<T>`: a four-byte count, no
weak count, increments and decrements as volatile loads and stores so
LLVM cannot cancel them (an hd backend emitting Wasm would not get that
from the engine either). Strings are a counted header `{count, len}`
followed by the bytes, and a view is `(base, start, len)`.

Three allocators: Rust's default `dlmalloc`, `talc` 5.1.1 (a small,
fast allocator for Wasm), and `sc`, a 150-line segregated size-class
allocator written for this study as a model of section 6. Bytes
allocated come from a counting wrapper around the allocator, as in the
Rust study. The sources are scratch files under `target/rc/` (`bench/`,
`size/`, `csize/`), not in the repository.

### 3.2 Speed

Median wall time in milliseconds on V8, start-up (27 ms) included. The
last column is hd today on Wasm GC, from the Rust study (start-up 36 ms).
Binary-trees is the benchmarks-game program, as in the study; the other
three follow the study's descriptions with their own inputs, so their hd
column is a reference point, not a same-input comparison.

| Program | Variant | `dlmalloc` | `talc` | `sc` | hd on Wasm GC |
| --- | --- | ---: | ---: | ---: | ---: |
| binary-trees, depth 20 | arena: one vector per tree | 891 | 899 | 898 | |
| | box | 4,079 | 4,795 | 2,221 | 1,934 |
| | rc naive | 4,342 | 4,953 | 2,429 | |
| | rc borrow | 3,851 | 4,640 | 2,059 | |
| | cc naive | 8,428 | 9,448 | 6,850 | |
| | cc borrow | 4,586 | 6,164 | 3,588 | |
| | gcmodule, borrow | 4,424 | 4,886 | 2,605 | |
| points, 1M records, 10 rounds | inline structs | 57 | 58 | 57 | 399 |
| | box | 201 | 246 | 138 | |
| | rc naive | 258 | 265 | 171 | |
| | rc borrow | 229 | 242 | 144 | |
| map-count, 2M words | borrowed keys | 127 | 123 | 121 | 497 |
| | owned: a string per word | 174 | 175 | 169 | |
| | counted views, naive | 166 | 161 | 162 | |
| | counted views, borrow | 161 | 160 | 161 | |
| string-build, 3M pieces | one buffer, no piece objects | 102 | 100 | 102 | 366 |
| | owned: an object per piece | 140 | 151 | 130 | |
| | rc naive | 144 | 153 | 132 | |
| | rc borrow | 141 | 149 | 128 | |
| | cow: unique check per append | 103 | 102 | 102 | |
| cyclic: trees with parent references, 2M nodes | arena | 46 | 47 | 48 | |
| | weak parent references, no collector | 66 | 70 | 52 | |
| | Bacon-Rajan, collect per tree | 219 | 226 | 206 | |
| | gcmodule, collect per tree | 137 | 143 | 111 | |
| | Bacon-Rajan, 262k-node live heap | 275 | 289 | 250 | |
| | gcmodule, 262k-node live heap | 939 | 972 | 897 | |

What each run allocates, counted at the allocator (requests and bytes,
with growth of a buffer counted as a request), against V8's count for hd
on Wasm GC:

| Program | Variant | Requests | Bytes | hd on V8 |
| --- | --- | ---: | ---: | ---: |
| binary-trees | box | 307M | 2,455 MB | 9,821 MB |
| | rc (12-byte nodes) | 307M | 3,683 MB | |
| points | inline | 11 | 176 MB | 537 MB |
| | box | 11.0M | 220 MB | |
| | rc | 11.0M | 308 MB | |
| map-count | borrowed keys | 17 | 12 MB | 447 MB |
| | owned | 2.0M | 71 MB | |
| | counted views | 38 | 72 MB | |
| string-build | one buffer, cow | 23 | 67 MB | 531 MB |
| | owned | 3.0M | 90 MB | |
| | rc | 3.0M | 114 MB | |

Two smaller probes, each the median of seven runs at a load of 1.6 to
1.7:

| Release of the last reference | `dlmalloc` | `sc` |
| --- | ---: | ---: |
| a tree of 2M counted nodes, recursive | 16.7 ms | 6.6 ms |
| a linked list of 1M nodes, iterative | 8.7 ms | 4.8 ms |
| a vector of 1M counted 16-byte objects | 7.7 ms | 3.0 ms |

### 3.3 What the numbers say

- **The allocator is the largest lever.** The size-class allocator
  halves `dlmalloc`'s time on binary-trees (4,079 to 2,221 ms) and
  frees 1.8x to 2.6x faster. With counts and the elisions of section 4
  it runs binary-trees in 2,059 ms, within 1.07x of hd on Wasm GC (1,934), where
  `Box` and `dlmalloc` were 2.1x behind. `talc` was slower than
  `dlmalloc` on every allocating program here, or tied.
- **Elided counting is nearly free; naive counting is not.** With moves
  and borrows, counted objects ran within 4% of uncounted ones (tree
  2,059 against 2,221, points 144 against 138, string-build 128 against
  130). Counting at every copy cost 18% on the tree (2,429) and 19% on
  points (171). One increment and decrement pair measured 2.1 ns
  (section 5).
- **Layout still matters most.** Inline structs run points in 57 ms
  against 144 counted and 399 for hd today: 30 ms of work after
  start-up against 363. Words as views (161) and borrowed keys (121)
  beat a string per word (169). One vector per tree (891) beats every
  object-per-node variant. Go's model takes these gains by default.
- **A collector must stay off acyclic data.** The same acyclic tree
  under the `bacon_rajan_cc` crate took 1.74x the plain-count time with
  borrows (3,588) and 3.3x without (6,850), because every decrement to
  a nonzero count buffers a candidate. `gcmodule`'s per-object list
  links cost 1.27x. Section 7.1's analysis removes this.
- **The unique check is free.** A counted buffer that checks it is
  unique before every append ran as fast as a plain buffer (102 against
  102 ms).
- **Counts are not where memory goes.** Every counted variant
  allocated 16% to 57% of the bytes hd allocates on Wasm GC. The
  four-byte header grows 8-byte nodes to 12 (16 after rounding), so the
  tree requests 1.5x the bytes of `Box`.
- **The cost of `sc`'s simplicity** shows in peak memory: with no
  coalescing, map-count's owned variant peaked at 94 MB of linear
  memory against 72 with `dlmalloc`. The proposed allocator coalesces
  page runs and returns empty pages to a pool (section 6).

### 3.4 Size

Each module below is `no_std`, `opt-level = "s"`, LTO, `panic =
abort`, stripped, so the numbers are runtime code and not Rust's panic
formatter. "Bump" never frees; it is the floor.

| Module (bytes) | `dlmalloc` | `talc` | `sc` | `lol_alloc` | bump |
| --- | ---: | ---: | ---: | ---: | ---: |
| allocates nothing (an allocator declared) | 65 | 65 | 65 | 65 | 65 |
| one `Box` and a growing `Vec` | 7,488 | 2,818 | 1,751 | 1,388 | 925 |
| plus one counted tree type | 7,827 | 3,157 | 2,086 | 1,727 | 1,256 |
| plus nine more counted types | 9,834 | 5,038 | 4,102 | 3,608 | 3,026 |

- **Allocator alone** (over bump): `dlmalloc` 6.6 KB, `talc` 1.9 KB,
  `sc` 0.8 KB, `lol_alloc` 0.5 KB.
- **Counting glue:** about 335 bytes for the first counted type (new,
  increment, decrement, release), then about 210 bytes per type, of
  which the release glue is 100 bytes (one function per type in the
  name section).
- **Collector:** with std, `sc` and the same flags, a program with one
  cyclic type grew by 5,965 bytes with `bacon_rajan_cc` and 4,620 with
  `gcmodule` over the same program with plain counts.
- **Tree shaking works as hoped.** A module whose code never allocates
  carries no allocator at all, whichever one is declared.

## 4. Where Counting Costs, And How To Cut It

### 4.1 The costs

| Cost | When | What it looks like in Wasm |
| --- | --- | --- |
| increment | a reference is copied into a place that outlives the source | load, add, store: 13 bytes inline, 4 as a call to a shared helper |
| decrement and zero test | a reference's place ends or is overwritten | load, subtract, store, test, call the type's release on zero: about 26 bytes inline, 4 as a call to the type's helper |
| cascading free | the last reference to a large structure goes | a walk over every object it owns, at that moment |
| cache traffic | a count update writes the object's header even when the program only reads the object | one dirty cache line per object touched |
| allocation | every new object | a size-class free-list pop, against Wasm GC's bump pointer |
| atomics | only for objects shared between threads | a read-modify-write per update |

### 4.2 What proven systems do

- **Moves at last use** transfer the reference instead of copying it
  (Rust, Swift's OSSA, Nim's ARC).
- **Borrowed parameters.** The caller keeps the argument alive, so the
  callee neither increments nor decrements. Swift borrows every
  parameter except those of initializers and setters, which consume,
  "since we have found this to be more efficient in most situations"
  ([SE-0377](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md)).
  The convention is part of the declaration and of the ABI; SE-0377
  added `borrowing` and `consuming` to override it. Objective-C ARC
  writes the exceptions in the header (`ns_consumed`,
  `ns_returns_retained`, and the `alloc`/`new`/`copy` method families;
  [Clang ARC](https://clang.llvm.org/docs/AutomaticReferenceCounting.html)).
- **Precise counting with reuse (Perceus).** Koka and Lean insert counts
  on a clean intermediate form, drop each value as soon as it is dead,
  and reuse a freed cell for the next allocation of the same size, so a
  functional update of a unique value happens in place
  ([Perceus](https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/),
  PLDI 2021; [Counting Immutable Beans](https://arxiv.org/abs/1908.05647),
  IFL 2019). Lean also *infers* which parameters are borrowed from the
  callee's body.
- **Nim ARC/ORC:** `sink` parameters, `lent` results and uncounted
  `{.cursor.}` fields ([Nim destructors](https://nim-lang.org/docs/destructors.html)).
- **Deferred and coalesced counting.** The RC Immix work skips counts
  for stack references (which then must be scanned) and coalesces field
  updates, and found the free-list heap, not counting, the main
  remaining gap to tracing ([Down for the count?](https://doi.org/10.1145/2258996.2259008),
  ISMM 2012; [RC Immix](https://doi.org/10.1145/2509136.2509527), OOPSLA 2013).
- **Non-atomic counts** where objects cannot be seen by another thread.
  Swift's counts are always atomic, which is what biased reference
  counting attacks ([Choi, Shull and Torrellas](https://doi.org/10.1145/3243176.3243195),
  PACT 2018); Roc counts atomically only values the host can see
  ([Roc: reference counting](https://github.com/roc-lang/roc/blob/main/docs/langref/expressions.md#reference-counting)).

### 4.3 What hd takes

hd's rule: a caller may rely only on what a declaration's header says,
never on a callee's body. So an elision may never make a function's
calling convention depend on its body across modules. Elision inside
one body is free to use anything that body shows.

| Technique | hd | Needs a mark |
| --- | --- | --- |
| moves at last use | yes: liveness inside one body | no |
| parameter passing | fixed by the parameter's type: a value parameter `T` is an owned copy, moved in at the caller's last use and otherwise passed with one increment, so the callee may change it in place when it is the only holder; a `&T` parameter is a borrowed read with no count | no: the type already says it |
| consuming parameters for construction | yes: a struct literal, an enum constructor and a list literal take their parts; a stored borrowed argument costs one increment in the callee | no |
| a `consume` parameter mark (Swift's `consuming`, Nim's `sink`) | not needed: a value parameter already owns its copy | no |
| borrowed reads of immutable fields | yes: a field that is not `mut`, read from a value the body holds, stays alive as long as its parent | no |
| borrowed reads of mutable places | only when no call or store between the read and the last use can overwrite the place; otherwise one increment | no |
| borrowed loop elements | yes when the loop body cannot write the list; otherwise one increment per element | no |
| reuse of a freed cell, drop specialization | yes, inside one body | no |
| borrow *inference* of parameters (Lean) | no: it makes the convention depend on the body | would need a header mark instead |
| borrowed results (Nim's `lent`, Rust's `&`) | only with tier 2 lifetimes, which state the borrow in the header | yes |
| objects and address-taken locals on the stack | tier 1: in a shadow-stack frame that holds one count, when no reference leaves the body or reaches a call that is not inlined; tier 2 headers extend this across calls | across calls, yes |
| deferred counting of stack references | no: Wasm cannot scan its own stack, so it needs a shadow stack of every reference, the very cost AssemblyScript met (section 8.1) | no |
| coalesced field updates | no: a write log and a collection phase for a gain the elisions above mostly take | no |
| non-atomic counts | yes: stage A instances share no objects ([PARALLELISM_AND_LOCALITY.md](PARALLELISM_AND_LOCALITY.md) §3.2) | no |

One rule needs care. A `&T` argument must stay alive for the call, so
when the caller passes a reference to a value it just read from a
mutable place (`f(&node.next)` where `next` is mutable), and the callee
could overwrite that place, the caller increments first. Swift does the
same. A `&T` to a local, a parameter or an immutable field passes with
no count. A value argument is moved at the caller's last use and
otherwise costs one increment.

### 4.4 Do values need counts at all?

Most hd values are never counted:

- **Scalars and structs** are copied, never shared. A struct inline in
  its parent has no header; only the references it contains are
  counted, each as its own field.
- **Immutable fields** are borrowed when read (section 4.3), so a read
  of `order.customer.name` touches no count.
- **Counts never overflow on wasm32**: every reference occupies at
  least four bytes of a 4 GiB memory, so the increment has no overflow
  check (memory64 would need one).
- **Literals and constants are immortal.** A constant object reached by
  its base has a count that starts at 2^31; a program never releases
  more references than it made, so it never reaches zero and needs no
  test. A string literal is a view into the data segment, whose pages
  the page table marks static; a count update through a view reads the
  page entry anyway (section 5) and skips static pages.

What remains counted is what Go would put on its heap: strings built at
run time, lists, maps, closures with captures, `dyn` values, the nodes
of recursive types behind `&`, frames, and the cells that escaping references point
into.

### 4.5 Cascading frees

Releasing the last reference to a tree releases every node at that
moment: 6.6 ms for a 2M-node tree with `sc` and 16.7 ms with
`dlmalloc`, about 3 to 8 ns per object. A recursive release also
overflows the Wasm stack on a long linked list. So release glue for a recursive type is
iterative, as CPython's "trashcan" and the list variant measured here
are: it takes the tail while it holds the only reference and loops.

Deferring that work is not a free bound on pauses, for three reasons.
Freeing a fixed number of objects per allocation does not bound memory
(Boehm, POPL 2004): pacing has to be by bytes, and then allocation pays
in proportion to size. In hd, counts are observable: a late decrement
keeps a list pinned past the last use of an element reference (`ref.elem.access.pin`),
so a later `push` panics `collection-pinned`, and a count left high
makes `rt:list_unique` copy a buffer that was in fact unique. And a
deferred free saves no work; it only moves it.

A helper thread could free a dead structure's unshared subgraph (count
1, referenced only from the dead object) while the program keeps its
non-atomic counts, as Redis's lazy free does for large values. It needs
shared memory, a settle step before every point the runtime reads a
count (pins, copy-on-write, `memory.grow`, the leak audit), and it
excludes cycle-capable types and any type a `Weak[T]` can name, since a
weak upgrade reaches an object whose count the helper has not yet
lowered. The measured pause (6.6 ms for 2M nodes) does not ask for any
of this; a server benchmark of p99 latency on other connections while
a big value drops, and while a big map grows, would be the evidence to
reopen it.

## 5. Interior References

Go lets a pointer point at a field or an element, and so will hd. Under
counting, an interior reference must keep its whole object alive, so
every count update through it must find the object's header. Reading
through it needs only the address. Three representations:

| Representation | Size | Count update | Read |
| --- | --- | --- | --- |
| base only (no interior references) | 4 bytes | at the address | address plus a constant |
| fat: `(base, offset)` | 8 bytes | at `base` | `base + offset` |
| thin: the field's address; the header from the page table | 4 bytes | page table entry, a multiply by the slot size's reciprocal, the header | the address |

The thin form is Go's: the allocator keeps one entry per page with its
size class, and `findObject` turns any address into the object's base
([mbitmap.go](https://github.com/golang/go/blob/master/src/runtime/mbitmap.go),
[mheap.go](https://github.com/golang/go/blob/master/src/runtime/mheap.go)
`spanOf`). For hd it requires that every counted object live in a page
of one size class (small objects) or a page run whose entries point at
its start (large ones), which the allocator of section 6 does anyway.

Measured on 1M objects of 36 bytes in 64 KiB single-class pages, with a
list of references to one 16-byte field of each, 200 passes, nanoseconds
per reference after setup is subtracted:

| Representation | Copy (increment and decrement) and read | Read only | Reference size |
| --- | ---: | ---: | ---: |
| base only | 3.44 | 1.30 | 4 bytes |
| fat | 3.41 | 1.82 | 8 bytes |
| thin, page-table lookup | 3.43 | 1.29 | 4 bytes |

An increment and decrement pair costs 2.1 ns (3.43 against 1.29). The
page-table lookup did not show: its loads hit the cache and the CPU
overlaps them with the count update. The fat form's doubled
reference size cost 40% on the read-only pass, which is the common
case. The objects here are visited in address order; in a scattered
heap the lookup adds one load from a table of 4 bytes per 64 KiB page
(3 KB for a 48 MB heap), which stays in cache. hd uses the thin form,
which keeps a reference one Wasm value.

Strings are the most common interior reference: a view into a buffer.
In the thin form a string is `(address, length)`, 8 bytes, against 12
for today's `(ref, i64)` in a field, and a substring costs no
allocation and no lookup until it is stored.

An empty view needs one rule. `text.slice(text.len(), text.len())` has
the address one past its buffer, and when the buffer exactly fills its
slot that address lies inside the next object, whose count the page
table would then find. So every empty view, and every empty string, is
one shared static empty value that is never counted; slicing to length
zero drops the reference to the source.

With tier 2 lifetimes, a non-escaping reference needs no count at all,
because its lifetime already keeps the object alive, and a local it
points at can live in a shadow-stack frame instead of a heap cell. That
is where optional borrows now pay in a way they never could on Wasm GC:
they remove count traffic, not just a check.

## 6. The Allocator

| Allocator | Wasm bytes (allocator alone, approx.) | Speed in section 3 | Notes |
| --- | ---: | --- | --- |
| `dlmalloc` (Rust's default) | 6.6 KB | the baseline | boundary tags, coalescing, bins |
| `talc` | 1.9 KB | 1.0x to 1.3x slower than `dlmalloc` | a two-level bitfield of bins, coalescing |
| `sc` (this study) | 0.8 KB | 1.0x to 1.8x faster than `dlmalloc`; frees 1.8x to 2.6x faster | size classes, free lists, bump in fresh chunks, no coalescing of small blocks |
| `lol_alloc` | 0.5 KB | not measured | a single free list; linear-time allocation |
| bump, never frees | 0.1 KB | not measured | AssemblyScript's `stub` runtime |

The proposal is `sc` grown into a real allocator:

- **Size classes** of 16 bytes up to 128, then about 25% steps up to
  2 KiB (16 to 20 classes). No per-object size: the release glue knows
  the type, so it knows the class.
- **Pages of one class.** A 64 KiB page holds one class; a page table
  (one entry per page, 4 bytes each, 256 KiB for a full 4 GiB memory,
  grown with the memory) records it. This is what thin interior
  references need, and it keeps objects of one size together.
- **Free lists per page**, popped and pushed in constant time; a fresh
  page is bump-allocated. A page that becomes empty returns to the page
  pool. mimalloc's sharded free lists are the model.
- **Large blocks** (over 2 KiB) are runs of pages, first fit, coalesced
  on free; every page of a run points at the run's start.
- **Memory never shrinks.** Wasm cannot return pages to the engine, so a
  program's footprint is its high-water mark. Wasm GC could give memory
  back; this runtime cannot.
- **No zeroing.** Every object is initialized by its constructor before
  anything can read it, and nothing scans the heap except the collector,
  which only visits initialized objects.

## 7. Cycle Detection

### 7.1 Which types can be part of a cycle

A cycle is closed by storing a reference into an object that already
exists and that the stored value reaches. Values built bottom-up and
never mutated cannot form one: Roc has no way to express a cycle at
all, "so none of these solutions are necessary"
([Roc](https://github.com/roc-lang/roc/blob/main/docs/langref/expressions.md#reference-cycles)).

The compiler decides, per concrete type after monomorphization, from the
type's definition and the definitions its fields name (the same inputs
as today's layout rules, so cached instances stay valid):

1. Draw the type graph: an edge from `A` to `B` for every reference to
   `B` that an `A` can hold, through fields, list and map elements,
   options and enum payloads. Mark an edge mutable when the slot can be
   written after construction.
2. A type is **cycle-capable** when it lies on a cycle of that graph
   that has a mutable edge, or when it reaches a mutable slot whose type
   is open: `dyn Trait`, `Any`, or a closure.
3. Each closure literal and each concrete type behind a `dyn` carries
   its own release function, so a closure that captures only immutable
   values, or a `dyn` value whose concrete type is acyclic, records no
   candidate.

Scalars, strings, immutable structs and enums, and every type with no
path back to itself through a mutable slot are acyclic. They never
touch the collector: their release is a plain decrement and free, with
no buffer check. This is Nim's rule: its compiler "analyses the
involved types and only if it is potentially cyclic, code is produced
that calls into the cycle collector", and `{.acyclic.}` can assert it
([Introducing ORC](https://nim-lang.org/blog/2020/12/08/introducing-orc.html)).
`gcmodule`'s `is_type_tracked` is the same idea in Rust.

**In real programs.** The 19 example packages under `examples/`, the 19
playground programs and the 22 tour chapters declare 54 types. None is
cycle-capable. The only recursive type is the calculator's `Expr`, an
immutable enum built bottom-up. Every mutable field holds a list or map
of strings or integers (`log: mut List[string]`,
`counts: mut Map[string, i32]`). The closures passed to `map`, `filter`
and `fold` capture immutable values. So none of these programs would
link the collector, and none would ever record a candidate. In std's
147 types, the open mutable slots are few: format's
`active: mut List[dyn Any]` (the stack of values being printed, which
could hold identities instead of references), and generic writers
whose instance decides. Cycle-capable types appear where programs build
graphs with back edges: a tree with parent references, an observer list,
a cache whose entries point at their owner.

### 7.2 The algorithm

Synchronous trial deletion (Bacon and Rajan, "Concurrent Cycle
Collection in Reference Counted Systems", ECOOP 2001):

1. When a cycle-capable object is decremented to a count above zero, it
   becomes a candidate root: a flag in its second header word and an
   entry in the candidate buffer. Freeing it removes the entry in
   constant time (ORC stores the buffer index in the object).
2. A collection marks gray from each candidate, subtracting internal
   references; scans, restoring objects still reachable from outside;
   and frees what stays white. It visits only what the candidates reach,
   never the whole heap.
3. Trace glue (one function per cycle-capable type, visiting its
   reference fields) drives the walk; acyclic children are not entered.

### 7.3 What a pause costs

From the cyclic program (`sc` allocator): 64 trees of 32,767 nodes,
each node pointing at its parent, collected after each tree is dropped.

| Collector | Total pause | Longest pause | Pause with a 262k-node live heap (total, longest) |
| --- | ---: | ---: | ---: |
| Bacon-Rajan (`bacon_rajan_cc`) | 157 ms | 3.2 ms | 164 ms, 3.6 ms |
| CPython-style (`gcmodule`) | 67 ms | 2.0 ms | 829 ms, 16.7 ms |
| weak parent references, no collector | none | none | none |

Trial deletion's pause depends on the garbage it examines, not on the
heap: a live cyclic heap eight times larger left it unchanged. The
CPython-style scan was faster on pure garbage but scaled with every
tracked object, which is why CPython needs generations and scans its
oldest one only when a quarter of it is new
([CPython GC design](https://github.com/python/cpython/blob/main/InternalDocs/garbage_collector.md)). Collecting cost about 75 ns per collected node in
the crate, against 52 ms for the whole program when the programmer
breaks the cycles with weak references. A cycle-capable program pays
for its cycles, and only for them.

### 7.4 When it runs

- When the candidate buffer passes a threshold that adapts as ORC's
  does: start at 10,000 candidates (PHP's fixed buffer size,
  [PHP manual](https://www.php.net/manual/en/features.gc.collecting-cycles.php)), double it when a collection frees
  little, halve it back when one frees most of what it visited.
- When the instance goes idle: the entry driver's `hd.poll` finds no
  runnable task. A collection there pauses nothing the user waits for.
- At orderly instance shutdown, before the final leak report, so a
  handle leaked inside a cycle is still reported, by `hd test` too.
- Never on a timer, and never from a host call in the middle of a
  computation.

There is no public API to force a collection; tests get a hook in debug
builds.

## 8. Prior Art

### 8.1 AssemblyScript: counting, then tracing

AssemblyScript shipped automatic reference counting with a Bacon-Rajan
cycle collector (`--runtime full`) and replaced it in v0.18.0
(January 2021) with a mark-and-sweep collector, now incremental by
default ([v0.18.0](https://github.com/AssemblyScript/assemblyscript/releases/tag/v0.18.0),
[#1559](https://github.com/AssemblyScript/assemblyscript/pull/1559),
[runtime](https://www.assemblyscript.org/runtime.html)). Its reasons,
from the pull request and the issue before it
([#1534](https://github.com/AssemblyScript/assemblyscript/issues/1534)):

| Their reason | Applies to hd? |
| --- | --- |
| The insertion of retain and release in the compiler was "error-prone" and "haunting us": autorelease rules, `WILL_RETAIN` constraints, and special cases for skipped and delayed releases. An attempt at deferred counting needed five more intrinsics and "documentation for all this stuff would be half a book". | **Partly.** hd inserts counts on its typed IR, one body at a time, with moves explicit, as Koka and Lean do. It still needs a checked mode and a fuzzer (section 10.2, job 16). |
| JavaScript had to call `__retain` and `__release` on every object it kept. | **No.** hd values cross to the host through codecs that copy; JavaScript never holds a raw hd pointer. Handles for a future embedding API use a table and `FinalizationRegistry` (section 9.4). |
| Generated code became unrecognizable, and the only runtime exports they wanted were `__new` and `__collect`. | **Yes, for size.** Count updates are code. Section 3.4 prices them, and the elisions keep them off the common paths. |
| A tracing collector was about 270 lines, and it prepared for Wasm GC. | **No.** hd leaves Wasm GC; and their tracing collector then needed a shadow stack for precise roots and zeroed allocations, the two costs counting avoids on Wasm. |

So AssemblyScript left counting for engineering reasons in an AST-level
compiler and for JavaScript interop, not because counting measured
slow. The first reason is the real risk for hd too.

### 8.2 Others

| System | Counts | Cycles | Elision | Lesson for hd |
| --- | --- | --- | --- | --- |
| Swift ARC | atomic, in the object header (with a side table for weak references) | none: `weak` and `unowned` by hand | borrowed parameters by default, OSSA, `borrowing`/`consuming` | a fixed declaration rule for conventions works; atomic counts cost |
| Objective-C ARC | atomic | none | conventions in headers and method families | conventions belong in declarations |
| CPython | non-atomic (atomic in the free-threaded build), immortal objects | generational scan of all containers | none in the interpreter | scanning every container makes old-generation collections cost the whole heap |
| PHP | non-atomic | Bacon-Rajan, 10,000-root buffer | none | trial deletion is enough for a long-lived production runtime |
| Nim ORC | non-atomic | trial deletion, only for cycle-capable types | `sink`, `lent`, cursors, last-use moves | type analysis keeps the collector off acyclic data |
| Koka, Lean (Perceus) | precise, atomic only for shared values | none: Lean's data is immutable; Koka leaks cycles through mutable references | owned by default, borrow inference (Lean), reuse | reuse turns functional updates into in-place writes |
| Roc | atomic only for host-visible values | impossible by design | in-place updates of unique values | immutable data needs no collector |

## 9. What Linear Memory Changes Beyond Counting

### 9.1 Copy-on-write collections

With a count, a collection can be a value: a write checks that the
buffer is unique and copies it first if not, as Swift's
`isKnownUniquelyReferenced` and Roc's opportunistic mutation do. The
check is a load and a compare before a write; the string-build proxy
measured it as free (102 ms with it, 102 without). The tier 1 study
makes lists, maps and strings values on exactly this check.

### 9.2 Structs stored inline

A list of points is one buffer of 16-byte elements, not a buffer of
references to a million objects. The points proxy measures this: 57 ms
against 144 for an object per record with counts, and 399 for hd today.
Elements with references inside are still inline; the list's release
walks them and decrements each reference.

### 9.3 Deterministic release and `Disposable`

Counting frees memory at the moment the last reference goes. hd keeps
cleanup in `Disposable` and `using`, because closing can suspend, and a
release cannot. What counting adds is a cheap, exact leak report: when a
`Disposable` handle's count reaches zero and it was never disposed, the
runtime knows the site at once, instead of the host's descriptor table
noticing at exit. A handle leaked inside a cycle never reaches zero by
itself; it is reported when the collector reclaims the cycle, and at the
latest at shutdown (section 7.4). Neither report disposes the handle.

### 9.4 The host and JavaScript

- **Strings and bytes out** are one copy from linear memory
  (`TextDecoder` over a view of the memory buffer). The Rust study
  measured host copies 6x to 10x faster from linear memory than element
  by element from a GC array.
- **The host codecs** read and write memory directly; the 5.2k lines of
  `host/*.mjs` and about 2.5k lines of codecs in `hd_run_wasmtime` that
  walk GC arrays are rewritten.
- **JavaScript holding an hd object**, if an embedding API ever allows
  it, gets a handle: an index into a table in the instance that holds
  one count. A `FinalizationRegistry` callback releases it when the
  JavaScript wrapper dies. That callback may come late or never, so a
  handle that matters is released explicitly.

### 9.5 Threads

Stage A copies messages between instances, so every count is
non-atomic. Real shared-memory parallelism (stage C) puts objects in a
shared region: those, and only those, need atomic counts. A frozen graph
published once to other threads can be made immortal for the life of
its region, as scriptc does, so readers do no count updates at all.
Wasm threads have shipped, so this no longer waits for
shared-everything threads; it waits for the language design of what
may be shared.

### 9.6 The backend rewrite

Moving from Wasm GC to linear memory changes every row of the layout table: memory and data
segments, the runtime in the module, count updates and release glue,
a shadow stack for borrowed locals, monomorphized layouts with no
`eqref` casts, new host codecs. In exchange, SIMD over bytes becomes
possible ([Performance Roadmap 3.9](PERFORMANCE_ROADMAP.md#39-simd-byte-scans-428)). A native backend
is not planned (#188 closed).

## 10. Recommendation And Plan

The runtime, in ten lines:

1. Counted objects in linear memory with an 8-byte header: a 32-bit,
   non-atomic count and a word for the release-and-trace descriptor and
   the borrow-flag bits. A cycle-capable type's collector state is three
   bits of that word.
2. No overflow check on wasm32; literals and constants are immortal
   (a count starting at 2^31, or a static page).
3. A size-class allocator: classes up to 2 KiB in 64 KiB single-class
   pages, free lists and bump fill, a page table, coalesced page runs
   for large blocks.
4. References are plain addresses; a count update through an interior
   reference finds the header through the page table.
5. Value parameters owned (moved at the caller's last use, else one
   increment), `&T` parameters borrowed, results owned, construction
   consumes, moves at last use, immutable fields read without a count:
   all decided from declarations and one body, with no new syntax.
6. Release glue per type, iterative for recursive types, shared between
   types with the same layout.
7. Trial deletion only for cycle-capable types, run when the candidate
   buffer passes an adaptive threshold and when the instance is idle.
8. Structs inline everywhere; strings as `(address, length)` views;
   lists and maps as counted buffers with inline elements.
9. All of it ordinary reachable code: a program that never allocates
   links none of it.
10. The linear backend is the only backend; Wasm GC emission is gone.

For the tier 1 study's open runtime assumptions: the thin address is
confirmed (section 5), and the header is 8 bytes, the count plus the
descriptor word (section 2); the copy-on-write test
measured free (section 9.1); and a buffer with an owner count and a pin
count costs one more word per buffer, not per element.

The "rc borrow" column measured every parameter at +0. Under the settled
convention a value parameter costs one increment when the caller keeps
using the value and nothing at its last use, while a read-only callee
written with `&T` stays at +0; so that column is the cost of code whose
read-only helpers take `&T`, and value parameters that the caller keeps
add one 2.1 ns pair each.

Tier 2 lifetimes, when they come, remove counts from every non-escaping
reference and let borrowed locals live in a shadow stack.

### 10.1 Size budget

| Component | Bytes | Linked when |
| --- | ---: | --- |
| nothing (hello) | 2,935, as today | never: a literal is a static view and `println` copies it to the exchange buffer |
| allocator | about 0.8 KB measured; 1.2 KB with coalesced page runs and the page pool (estimate) | any allocation is reachable |
| increment and decrement helpers | about 100 (estimate) | any counted type |
| release glue | about 100 per counted type, shared by types with the same layout | that type is released |
| trace glue | about 100 per cycle-capable type (estimate) | that type exists |
| collector | 1.5 to 2.5 KB written for hd (estimate); 4.6 to 6.0 KB as Rust crates, measured | a cycle-capable type exists |
| copy-on-write | about 10 per write site, a copy helper per element layout (estimate) | a value collection is written |

Estimate: a small program that allocates grows by 1.5 to 3 KB over what
it costs on Wasm GC today, less the Wasm GC type section entries it no
longer needs. A program with no cycle-capable type, which is every
example and tour program today, carries no collector.

### 10.2 Jobs

Each job is about an hour and leaves `main` green. Jobs marked (2) are
likely two jobs. Jobs 2 and 25 are dropped: there is no backend switch,
because the linear backend is the only backend.

1. Layout record: `crates/hd_wasm_linear/DESIGN.md` gains a linear-memory
   layout section from this study (docs only).
2. (Dropped.)
3. Module skeleton: memory, data segments, globals, entry wrapper;
   hello and integer-only programs pass; hello stays at 2,935 bytes.
4. Memory intrinsics (load, store, grow, copy, fill) in a privileged
   std module, with their spec rules.
5. Allocator in std: size classes, pages, page table, large runs; hd
   tests drive it.
6. Structs, tuples and enums inline: locals, parameters, multi-value
   results, fields.
7. Strings: literal views into data, counted buffers, concatenation,
   the builder.
8. Header and naive counting: increment at every copy, decrement at
   every end, release glue per type. Correct before fast.
9. Lists: counted buffers with inline elements, growth, element release.
10. Maps on linear memory (2).
11. Closures and `dyn`: counted environments, vtables with release and
    trace.
12. References and interior references: cells, page-table lookup,
    escape of a local's address to a cell.
13. Suspension frames as counted objects; the waker and entry driver.
14. Host codecs over linear memory in `host/*.mjs` (2).
15. Host codecs in `hd_run_wasmtime` (2).
16. Checked mode: poison freed memory, audit counts, report leaks at
    exit, collect at every allocation; a count fuzzer over conformance.
17. Elision 1: moves at last use, owned value parameters and borrowed
    `&T` parameters, construction consumes.
18. Elision 2: immutable-field borrows, loop-element borrows, cancelling
    increment and decrement pairs.
19. Iterative release for recursive types, with a pause test.
20. Cycle-capability analysis per concrete type, shown by `hd inspect`.
21. Trial-deletion collector, candidate buffer, threshold and idle
    triggers.
22. The `Disposable` leak report at count zero, and the collection at shutdown before the final report.
23. Conformance parity: the linear known-failures list to zero (several
    jobs).
24. Benchmarks: the Rust study's suite and this study's on the linear
    backend, with the footprint guard.
25. (Dropped.)

## Sources

- Counting and elision: [Perceus](https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/)
  (Reinking, Xie, de Moura, Leijen, PLDI 2021);
  [Counting Immutable Beans](https://arxiv.org/abs/1908.05647) (Ullrich
  and de Moura, IFL 2019); [SE-0377](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md)
  (Swift `borrowing` and `consuming`);
  [Swift ARC](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/);
  [Clang Objective-C ARC](https://clang.llvm.org/docs/AutomaticReferenceCounting.html);
  [Nim destructors and move semantics](https://nim-lang.org/docs/destructors.html);
  [Roc reference counting and cycles](https://github.com/roc-lang/roc/blob/main/docs/langref/expressions.md#reference-counting).
- Counting performance: Shahriyar, Blackburn and Frampton,
  [Down for the count?](https://doi.org/10.1145/2258996.2259008)
  (ISMM 2012); Shahriyar, Blackburn, Yang and McKinley,
  [Taking off the gloves with reference counting Immix](https://doi.org/10.1145/2509136.2509527)
  (OOPSLA 2013); Choi, Shull and Torrellas,
  [Biased reference counting](https://doi.org/10.1145/3243176.3243195)
  (PACT 2018).
- Cycles: Bacon and Rajan, "Concurrent Cycle Collection in Reference
  Counted Systems" (ECOOP 2001);
  [Introducing ORC](https://nim-lang.org/blog/2020/12/08/introducing-orc.html);
  [CPython garbage collector design](https://github.com/python/cpython/blob/main/InternalDocs/garbage_collector.md);
  [PEP 683](https://peps.python.org/pep-0683/) (immortal objects);
  [PHP: collecting cycles](https://www.php.net/manual/en/features.gc.collecting-cycles.php);
  [bacon_rajan_cc](https://github.com/fitzgen/bacon-rajan-cc) 0.4.0;
  [gcmodule](https://github.com/quark-zju/gcmodule) 0.3.3.
- AssemblyScript: [v0.18.0 release](https://github.com/AssemblyScript/assemblyscript/releases/tag/v0.18.0);
  [#1559](https://github.com/AssemblyScript/assemblyscript/pull/1559)
  (rewrite runtime, switch to tracing GC);
  [#1534](https://github.com/AssemblyScript/assemblyscript/issues/1534)
  (deferred reference counting);
  [runtime documentation](https://www.assemblyscript.org/runtime.html).
- Allocators and interior pointers: Go's
  [`findObject`](https://github.com/golang/go/blob/master/src/runtime/mbitmap.go)
  and [`spanOf`](https://github.com/golang/go/blob/master/src/runtime/mheap.go);
  [talc](https://github.com/SFBdragon/talc) 5.1.1 and its
  [Wasm benchmarks](https://github.com/SFBdragon/talc/blob/master/BENCHMARKS_WASM.md);
  [dlmalloc](https://github.com/alexcrichton/dlmalloc-rs);
  [lol_alloc](https://github.com/Craig-Macomber/lol_alloc).
- This repository: the Rust model study (`git show b3f5eed1a:docs/future-work/RUST_MODEL_STUDY.md`) section 8
  and its appendix (hd's Wasm GC numbers),
  [PARALLELISM_AND_LOCALITY.md](PARALLELISM_AND_LOCALITY.md),
  [crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md) §14 and §15.
