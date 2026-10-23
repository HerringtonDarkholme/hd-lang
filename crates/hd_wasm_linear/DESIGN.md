# Wasm Emission And Runtime

The compiler has one code generator, `hd_wasm_linear`. It emits scalar
Wasm functions over an exchange memory and a reference-counted heap.
Node, browsers and Wasmtime execute this same representation. There is
no backend selector or module-by-module session linker. A REPL build
contains its whole input history and replays recorded host results.

## 15. Shared Module Contracts

### 15.1 Layout

Values flatten to scalar locals; aggregate memory layouts follow §23.3.

### 15.2 References

The checker records reference reads, writes and identity in TIR. Emission
uses addresses, pinned owners and the typed views described in §23.20.

### 15.3 Recursive Types

Layout walks recursive groups iteratively. Reference edges are addresses;
value edges contribute inline scalar leaves.

### 15.4 Globals And Module Initialization

`hd.init` runs initializers in dependency order; `hd.poll` drives the root.
Tests export numbered entry points. REPL histories export `hd.init.N` and
`hd.poll.N` for each input. Each module owns its two memories, a third for the shadow stack when
a signature is wide (§23.29), and its globals.

### 15.5 Panic Sites And Backtraces

A panic preserves its category, message and source sites across engines.
The module maps function indices and call offsets to TIR source anchors.

### 15.6 Tiny Programs

Unreachable functions and helpers do not enter the final module.

### 15.7 Emission

Collection supplies concrete bodies and call targets; emission writes
symbolic scalar functions; linking assigns function and table indices.

### 15.8 Deterministic Bytes

Instances, helpers, imports and static data have deterministic content order.

### 15.9 The Line Table

`hd.lines` maps each function's anchored call byte offsets to TIR instruction
numbers. `hd.names` preserves source names in release builds.

## 14. Suspension Lowering

### 14.1 Functions Per Suspending Body

A cold constructor creates a frame. Body, poll, cancel and release glue
operate on that frame's linear-memory address.

### 14.2 The State Machine

Pending points save live values and a program counter; resumption reloads them.

### 14.3 Lazy Frame Materialization

A body need not materialize a saved frame until it actually waits.

### 14.4 Wakers And The Entry Driver

The host drives pending roots through poll and wake entry points.

Runtime-created ownership points down the active task graph; waiter links
back toward enclosing work are weak. User values may still create cycles
through explicit references, captures or providers (§23.28).

| Link | Ownership | Lifetime |
| --- | --- | --- |
| root driver -> root frame | owned count | released when ready or cancelled |
| frame -> active child/cleanup, saved arguments/captures/providers/context | owned counts | stable saved slots are released on exit/drop |
| all frame -> children, results and waker node | owned counts | frame owns these until completion/cancellation |
| race frame -> task list and kept winner | owned counts | list owns child values; result stays until cleanup settles |
| using/cancel wrapper -> cleanup child and saved values | owned counts | a shared cleanup can outlive an individual awaiter |
| current Waker global -> active enclosing node | borrowed address | the active polling frame owns it; restored after the child poll |
| node/host registration -> waiter edge head; edge -> next edge | owned storage | release storage on completion, abort or node drop |
| waiter edge -> enclosing node | weak slot + generation | resolve through registry before reading the target |
| registry row -> node | weak address | cleared before node release drops edge storage |

A node is still 12 payload bytes: edge head at 0, registry slot at 4,
ready bits at 8. A waiter edge is 16 bytes: slot, generation, owned next
edge, bits. Registry rows are 12 raw bytes: address, generation, free-list
index. Reusing a slot changes its generation. At `u32::MAX` the slot is
retired, never wrapped. A stale target is ignored, or its edge storage is
reused when registering another waiter; distinct live awaiters stay in
the list. Only edge-list storage owns a count, never its target.

The registry stays valid throughout teardown and collection. Checked
shutdown frees it afterward, before allocation audit. In normal runs its
reusable slots track peak concurrent nodes, plus any exhausted slots.
The linker derives the waker feature from reachable helpers, including
cached functions. A module with no node constructor links no registry or
waiter traversal; ordinary host completions still use their own handle
generations and wake table. The direct weak-handle test in `task.rs`
exercises stale-slot reuse, multiple awaiters, registry growth and
exhausted generations.

### 14.5 `all!` And `race!`

Generated task frames own their children and retain the results they need.

### 14.6 Cancellation And `defer`

Cancellation runs cleanup ladders and releases owned frame values.

### 14.7 Hook Points

Task entry points carry the host's current execution context.

### 14.8 Deadlock Detection

A pending root with no host operation that can wake it reports deadlock.

### 14.9 `block_on`

The synchronous driver polls a suspension to completion in allowed contexts.

## 23. Linear-Memory Layout

Objects live in Wasm linear memory and are freed by reference counting,
with a cycle collector for the types that can form a cycle. Every
aggregate is a value stored inline; sharing comes only from references,
which are plain addresses. The runtime (allocator, release, collector) is
ordinary hd in std over a few memory intrinsics; the backend emits only
what the spec names as intrinsic or what only it can build.

### 23.1 Where The Pieces Live

| Piece | Where |
| --- | --- |
| this record | `crates/hd_wasm_linear/DESIGN.md` §23 |
| the backend: layouts, emission from TIR, the module's link, the runtime pieces it generates | the crate `crates/hd_wasm_linear` |
| `layout.rs` | a type's linear layout: its Wasm values, its size and alignment in memory, its field offsets (§23.3) |
| `emit.rs` | one instance's TIR body to one Wasm function |
| `rt.rs` | generated runtime pieces: the entry wrapper, panic stubs, the exchange-buffer copies, the host provider stubs, object headers and iterative release glue |
| `task.rs` | frame constructors, waker nodes, polling and cancellation kernels (§23.21) |
| `lib.rs` | `build`: the whole program's module from collection's output |
| code generation | `hd_wasm_linear` is the sole code generator; commands have no backend-selection flag |
| conformance | both tiers build every case with the linear backend; `HD_CONFORMANCE_CHECKED=1` builds them in checked mode |
| memory intrinsics: load, store, grow, copy, fill | the privileged std module `std.rt.memory`: plain hd declarations whose bodies are `@intrinsic` |
| the allocator | hd in `std.rt.alloc`, over `std.rt.memory` |
| counts | `retain` and `release` in `std.rt.alloc`, called by emitted code, with release glue that `rt.rs` generates per layout |
| lists | `emit/list.rs` for the list kernels, and `rt.rs` for the header, the buffer, its release glue and the change that makes a buffer a list's own (§23.17) |
| maps | `emit/map.rs` for the map kernels, and `map.rs` for the table: its probe, put, growth, release glue and the change that makes a table a map's own (§23.18) |
| closures and `dyn` | `emit/closure.rs` for environments, cells and calls of function values, `emit/dyn_value.rs` for trait values and their vtables, `emit/iter.rs` for collection iterators (§23.19) |
| references, cleanup scopes, releases at last use | `emit/refs.rs` for references as addresses, the ownership passes' `Drop`s for each value's and local's last use, `emit/scope.rs` for the `defer` exit ladder, `emit/list.rs` for element references' pins (§23.20) |
| host codecs | `boundary.rs`: each boundary type's encoder and decoder over the exchange buffer, and the host method slots that call them (§23.23) |
| checked mode | `std.rt.checked`, the counts and the allocator with their checks, and `rt.rs`'s audit at an entry's end, with checked allocation/count auditing enabled (§23.24) |
| iterative release | `rt.rs::release_ref`: nested releases enqueue through `ReleaseHead`, and the outer release drains the queue |
| the cycle collector | hd in `std.rt.cycle`, called by emitted code, which runs each object's release glue as its trace (§23.28) |

`std.rt` modules are privileged: only std modules may import them, so the
runtime can change its representation without breaking a program.

### 23.2 Memories And The Address Space

| Memory | Holds | Exported as |
| --- | --- | --- |
| 0 | the exchange buffer (crates/hd_run_wasmtime/DESIGN.md §17.3) | `hd.x` |
| 1 | the heap: static data, the page table, every counted object | not exported: the host reads and writes only memory 0 (§23.23) |
| 2 | the shadow stack: the frames of wide calls, declared only when a signature is wide (§23.29) | not exported |

- **Why two memories.** The exchange buffer grows from address 0 as the
  host's results need, so it cannot share a memory with a heap that must
  never move. A second memory (Wasm multi-memory, on by default in V8
  and wasmtime) keeps the host contract unchanged, so both hosts run the same
  compiled module: the module's own codecs move values between its
  layouts and the buffer's bytes (§23.23). A string crosses with one
  `memory.copy` from memory 1 to memory 0.
- **Addresses** are `i32`, unsigned (`usize` on wasm32). Address 0 is
  null: `.None` of `Option[&T]`. The first 16 bytes are never used.
- **Static data** starts at address 16 in page 0: string literals, the
  one empty string, immortal constants, vtables and type descriptors,
  all placed by link in one active data segment. Its pages are marked
  static in the page table, so a count update through an address in them
  does nothing.
- **Pages** are Wasm's 64 KiB pages. The heap grows with `memory.grow`
  on memory 1 and never shrinks: a program's footprint is its high-water
  mark (REFCOUNT_RUNTIME.md §6).

### 23.3 Values

A layout is a list of Wasm values in locals, parameters and results, and
a size, an alignment and field offsets in memory. One representation per
type in every position; aggregate values are inline in memory.

| hd value | In locals and results | In memory |
| --- | --- | --- |
| integers, `bool`, `char`, floats | the scalar | packed: 1 byte for `bool`, `i8`, `u8`; 2 for 16-bit integers; 4 or 8 otherwise, aligned to its size |
| struct, tuple | its fields' values, flattened, in declaration order (Wasm multi-value) | its fields inline, each at its alignment; the struct aligned to its widest field |
| enum | an `i32` tag, then the payload slots: variants' fields of one Wasm value type share a slot (§15.1's slot sharing, with no bound) | the tag (1, 2 or 4 bytes by variant count), then the slots in their order, each 4 or 8 bytes at its alignment: the same values as in locals, so a load or a store needs no branch on the tag |
| self-recursive enum (today's language) | the address of a counted cell holding the enum value | the address |
| `T?` of a reference `T` | the address; 0 is `.None` | 4 bytes |
| `T?` of another `T` | as an enum with one payload | as an enum |
| `string` | `(address, length)`: two `i32` | 8 bytes |
| `&T`, `&mut T` | the address of the `T`, possibly inside an object (§23.5); of a trait value, that trait value (§23.20) | 4 bytes; 8 for a trait value |
| a compiler-supplied data type | the address of a counted object holding the fields | 4 bytes |
| `List[T]`, `Map[K, V]` | the address of a counted header over a counted buffer (§23.7) | 4 bytes |
| closure | `(code, environment)`: a function-table index and the address of a counted environment, 0 when nothing is captured | 8 bytes |
| `dyn Trait`, `Any` | `(data, vtable)`: the value's address and a static vtable's (§23.8) | 8 bytes |
| suspension | the address of a counted frame owned by its task | 4 bytes |
| `void`, `()`, `never` | none | 0 bytes |

- **Struct values inline.** A struct value is inline, so a field
  write is one store. A program declares no `data` type; only a
  compiler-supplied data type is still a counted object with identity.
- **No bound.** In
  linear memory a wide value costs only its moves, as in Go and C#. A
  large struct passed by value is the price OBJECT_MODEL_TIER1.md §3
  accepts. A signature too wide for Wasm values passes through the
  shadow stack (§23.29).
- **Type-only.** Every rule reads the type's definition and the
  definitions its fields name, never a use, so a laid-out instance is
  valid in every program that reaches it.

### 23.4 Counted Objects

Every counted object is one allocator slot (§23.6), and its header is at
the slot's start:

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | the count: 32-bit, non-atomic, with no overflow check on wasm32 (REFCOUNT_RUNTIME.md §4.4) |
| 4 | 4 | the descriptor word: bits 0 to 24 the release glue's function-table slot plus one (0 for none), bits 25 to 29 the allocator class; bit 30 set while the object is a cycle candidate, bit 31 when its type is cycle-capable (§23.28) |
| 8 | the payload's size | the payload: the fields, or a buffer's header and elements |
| the block's last 4 bytes | 4 | the collector word, for a cycle-capable type only: the color and the frame flag (REFCOUNT_RUNTIME.md §7.2, §23.28) |

- A reference to a whole object is the payload's address, slot + 8, so
  its fields read at constant offsets with no lookup; the header is at
  `address - 8`.
- **The descriptor word** identifies shared release glue, not a nominal
  type record. Its low 25 bits are the glue's table slot plus one; five
  bits carry an allocator class, and the top two are collector flags.
  The allocator supplies the block size. An interior reference finds its
  block through the page table, then runs that block's glue. Types with
  identical layouts may share the descriptor and have no stored type name.
- **Immortal objects.** A literal or constant object starts at count
  2^31, and lives in a static page anyway; a program never releases more
  references than it made, so it never reaches zero.
- **Release glue** is one function per type layout, shared by types with
  the same layout, iterative for a recursive type (REFCOUNT_RUNTIME.md
  §4.5); trace glue exists only for cycle-capable types.

### 23.5 Interior Addresses And The Page Table

A reference is one `i32` address, which may point at a field or an
element (the thin form of REFCOUNT_RUNTIME.md §5). Reading through it
needs only the address; a count update finds the object's slot through
the page table:

1. `page = address >> 16`, `entry = page_table[page]`.
2. A small page: `slot = page_start + 16 + ((address - page_start - 16) /
   size) * size`, the division by the class size done as a multiply by
   its reciprocal from a static table.
3. A large run: the entry names the run's first page, whose start is the
   slot.
4. A static or free page: no count; static pages are immortal, and a live
   reference never points into a free page.

The page table is one 4-byte entry per heap page, an array in memory 1
whose base and length are allocator globals, after 256 bytes that hold
each class's list of pages with room and the size/reciprocal tables.
It starts at one page (16,320
entries, almost 1 GiB of heap) and moves to a larger run when the heap
outgrows it, so its cost follows the heap: 256 KiB for a full 4 GiB
memory.

| Entry, top 2 bits | Low 30 bits | Page |
| --- | --- | --- |
| `00` | 0 | free: in the page pool, or never allocated |
| `01` | the size class, 1 to 20; 0 for a static page | a small page of one class, or static data |
| `10` | the run's page count | the first page of a large run |
| `11` | the run's first page | a later page of a large run |

### 23.6 The Allocator's Geometry

The allocator is hd in `std.rt.alloc`, REFCOUNT_RUNTIME.md §6's
`sc` grown into a real allocator:

- **Size classes**, 20 of them, sizes in bytes including the header: 16
  to 128 in steps of 16, then 160, 208, 256, 320, 400, 512, 640, 800,
  1024, 1280, 1600 and 2048, about 25% apart. Every size is a multiple
  of 16, so every payload at slot + 8 is 8-aligned. No object stores its
  size: its descriptor knows the class.
- **Pages of one class.** A small page starts with a 16-byte page header
  (the free-list head, the next and previous pages of its class's list,
  the live count and the bump offset), then its slots; its class is in
  its page-table entry. A fresh page is bump-allocated; freed slots go on
  the page's free list, popped and pushed in constant time; a page whose
  live count falls to zero returns to the page pool, unless it heads its
  class's list.
- **Large blocks** (over 2 KiB) are runs of whole pages, first fit from
  the pool's runs, coalesced with free neighbors on free; every page of a
  run points at its first (§23.5).
- **No zeroing.** Every object is initialized by its constructor before
  anything reads it, and only the collector scans, and only initialized
  objects.
- **Reachable code only.** A program that never allocates links none of
  the allocator, so hello pays nothing for it (REFCOUNT_RUNTIME.md §10.1).

### 23.7 Strings And Collection Buffers

- **Strings** are views `(address, length)` of valid UTF-8 inside a
  counted byte buffer, or inside static data for a literal. A slice is a
  new view with no allocation; it counts the buffer through the page
  table only when stored (REFCOUNT_RUNTIME.md §5). **Every empty string
  is one static value**: its address is one fixed static location and its
  length 0, never counted, so a view one past a buffer's end can never
  reach the next object's count. Slicing to length zero yields it.
- **A list** is the address of a counted header, which holds its buffer's
  address and its owner word. The buffer is a counted object holding the
  capacity, the length and then the elements inline at their layout's
  size. While the owner word is 0, the buffer's count is its owner count,
  for copy-on-write: a change first checks it is 1 and copies the buffer
  otherwise (REFCOUNT_RUNTIME.md §9.1). Once an element reference pins
  the list, the owner word is set and the header is the buffer's one
  owner, so its other counts are pins, the second count of
  OBJECT_MODEL_TIER1.md §6.3 (§23.20).
- **A map** is the same kind of header over its table, one counted block
  that holds the entries in the order they were added and a probing
  index over them (§23.18).

| Header payload offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | the buffer's address |
| 4 | 4 | the owner word: clone-glue table slot plus one once pinned (0 while shareable), with bit 31 recording transitive exposure |

| Buffer payload offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | capacity, in elements |
| 4 | 4 | length |
| 8 | capacity times the element size | the elements, inline |

Why a header, and not the buffer's address as the value: the TIR changes
a list through its value, not through the place that holds it (`push`,
and an element store take the list as an operand), and
the checker (on the ownership passes' path, insertion's rule 7) lets
places share one list value where nothing can change it in place
(`ValueCopy`, §15.2, "Collection values"). So the change must
reach every holder of that value,
while the buffer underneath may move when it grows or is copied, except
while pinned: an element reference is an address in the buffer, so a
pinned buffer never moves, and a copy of a pinned list copies it at
once. A TIR that wrote changes back to their place would let the value
be the buffer's address and save one object per list.

### 23.8 Closures, `dyn` And Vtables

- **The function table.** Every function called indirectly (a vtable
  slot, a closure's code, release and trace glue) has an entry in one
  Wasm table, filled by an active element segment; a call is
  `call_indirect` through the entry's index.
- **A closure** is `(code, environment)`. The environment is a counted
  object holding the captures inline, absent (0) when nothing is captured;
  each closure literal's environment layout has its own descriptor, whose
  release and trace know the captures (OBJECT_MODEL_TIER1.md §9.3).
- **A `dyn Trait` value** is `(data, vtable)`. The data is always one
  counted address (the value itself, or a box of it, §23.19), so dropping
  a `dyn` releases its data with no knowledge of the type: the release
  and, later, the trace are the data's own descriptor's (§23.4). A
  vtable is a static record of the method slots in the trait's declared
  order, then each direct supertrait's vtable address. `Inspectable`'s
  `runtime_type` slot answers the type's identity (§15.2's `TypeId`).
- **Providers** are trait values, passed as `(data, vtable)` pairs in key
  order (crates/hd_mono/DESIGN.md §13.18).

### 23.9 Globals, The Entry Wrapper And The Host

- **Module storage** (top-level bindings) is one Wasm global per Wasm
  value of the binding's layout, set by its group's init function (§15.4).
- **Runtime state** (the panic flag, the woken flag, the
  forbidden-context counter) are `i32` globals; the wake table and the
  candidate buffer are counted buffers in memory 1.
- **Exports and imports** are `hd.init`, `hd.poll`, `hd.wake`,
  `hd.x` (memory 0), `hd.cancel` for a scenario, and the `hd:rt`,
  `hd:prim` and capability imports of crates/hd_run_wasmtime/DESIGN.md
  §17.2.
- **Panics** call a stub that writes the report through `hd:rt` and
  traps (§15.5); a message literal is copied straight from static data to
  the exchange buffer.
- **Sections.** `hd.lines`, `hd.positions` and `name`
  (§15.5, §15.9) map panic frames to source locations.

### 23.10 Runtime Component Boundaries

The implementation units are listed in §23.1. Conformance fixtures live
in `spec/conformance/`, with runtime-unit fixtures in
`crates/hd_driver/tests/std_rt/`. The root `KNOWN_FAILURES.tsv` records
issue-backed exceptions; there is no per-backend rollout pass list.

### 23.11 Scalar Instruction Emission

- **Values in locals.** Each TIR instruction's value gets fresh Wasm
  locals of its layout, a TIR local owns the locals of its own, and a
  constant is an immediate. Wasm's structured control does the rest: a
  `Match` is one block per arm around its decision tree, whose `ToArm`
  branches out to its arm.
- **Integer code** follows §13.15 of
  crates/hd_mono/DESIGN.md: the same checks, the same wrapping in a
  release build, the same panic messages.

### 23.12 Memory Intrinsics And Runtime Tests

- **`std.rt.memory`.** spec/lang/10-modules.md "Memory Intrinsics": plain
  hd declarations in `lib/std/rt/memory.hd` whose bodies are
  `@intrinsic`, with the `mem_` keys of `hd_host_abi::INTRINSICS`. The
  linear backend lowers each to one instruction on memory 1: a
  little-endian load or store at any address, `memory.size`,
  `memory.grow` with its `-1` read as `.None`, `memory.copy`, which
  allows overlap, and `memory.fill`. Only std can reach one.
- **Typed memory.** spec/lang/10-modules.md "Typed Memory": `Ptr[T]` is
  a plain struct of one address, and a `T` at it is laid out as a list
  element of `T` is (`layout::mem`). `size_of` is that layout's size and
  `offset` one multiply and add (`Em::ptr`). `read` and `write` reach the
  emitter as a `Read` and a `Store` of the place `[ptr %p]`
  (`Em::ptr_place`): a read loads the leaves, and a `move` keeps the
  count they held; a store writes its value as an element store does
  (`slot_src`, `retain_src`) and releases nothing. A `Buffer[T]`
  (`StdKind::Buffer`) is the address of a list's buffer, §23.7's
  capacity and length words and the elements, made by `ListBuf` with
  its element glue, counted and dropped as any counted address is; a
  string's bytes are its address, and a new string is a `Str` block, or
  the static empty string for no bytes.
- **Runtime modules are std's.** `hd_project::UseRoots` refuses a use of
  `std.rt` or a module under it from any package but std, as
  `std-only-use` (`module.rt.std-only.error`).
- **The folder `std.rt`.** With `lib/std/rt/`, the virtual `rt.hd` is its
  parent file (`module.folder.parent-file`), so `std.rt` and its child
  modules form folder `std.rt`, with an edge to `std`. The driver reaches
  that folder for every program, since it roots `std.rt`'s result
  functions whatever the program uses. No module of folder `std` may use
  a runtime module, or the two folders would form a cycle; emitted code
  calls the runtime by name instead.
- **Test programs.** Tests export `hd.init.N` and `hd.test.N`; each
  case uses its own entry poll and the suspension protocol (§23.21).
- **The hd tests.** `crates/hd_driver/tests/std_rt/` holds std's own hd
  tests of its runtime modules. The conformance runner adds each file to
  std's sources and runs its cases on the linear backend, on the engines
  of its mode (`STD_RT_TESTS`).

### 23.13 Allocator Integration

- **`std.rt.alloc`.** `lib/std/rt/alloc.hd` is the allocator of §23.5 and
  §23.6 in hd over `std.rt.memory`: `alloc(size)`, `free(address)`,
  `block_of(address)`, the page-table lookup of an interior address, and
  `block_size(size)`. Its state is four module bindings: the table's
  address and capacity, how many pages the table knows, and the lowest
  page that may be free. The first `alloc` grows a page for the table and
  marks every page before it static.
- **Pages it does not own.** The heap can grow under it through `grow`.
  Each time it grows, it marks pages grown since as static and never
  hands them out.
- **Runs.** A run is taken first fit from the lowest page that may be
  free, by scanning the table for free entries; a freed run's entries
  become free, so neighbors coalesce with no merge step. The scan costs
  time proportional to the pages before the first fit; a free-run index is
  a later speed-up.
- **Division.** Issue #643 replaces the class-size division with the
  reciprocal table of §23.5; see §23.30.
- **Tests.** `crates/hd_driver/tests/std_rt/alloc_tests.hd` exercises
  allocation, free, reuse, interior lookup, large-run coalescing, and
  pages another part of the program grew, on both engines.

### 23.14 Inline Aggregate Layouts

Aggregate layouts come from `layout.rs` (§23.3); `emit/value.rs`
lowers their instructions.

- **One walk, two forms.** `layout::mem` gives a type's values as leaves,
  in the order its locals hold them, each with its offset and width in
  memory, and its size and alignment. Locals and memory never disagree, so
  moving a value between them is one load or store per leaf.
- **Structs and tuples** are their fields' values: a literal pushes them,
  a field read picks some, and a copy-update rebuilds the value from the
  source's values and the replacements. The checker writes a struct's
  field as a copy-update of the place (§15.2), so nothing stores into a
  struct value.
- **Enums.** The tag, then slots in the order `i32`, `f32`, `i64`, `f64`:
  the `k`th value of one Wasm type in a variant's payload is that type's
  `k`th slot, so a variant fills the slots it needs and leaves zeros in
  the rest. In memory each slot is a whole Wasm value at its alignment, not
  the variant's fields at their packed offsets as the table first said:
  that way a load or a store of an enum needs no branch on its tag. A
  narrow payload field costs its slot's 4 bytes.

### 23.15 String Emission And Host Primitives

String operations are emitted by `emit/text.rs` and runtime helpers
(§23.7).

- **Three kinds of view.** A literal's bytes are static data. A built
  string's bytes are a string buffer, `rt:str`: an 8-byte header, which
  reference counting uses, then the bytes; the view's address is the
  payload's. Every empty string is the one static empty value, address
  `EMPTY` (the first static address) and length 0: an interpolation or a
  concatenation with no bytes, an empty slice and an empty host result all
  give it, so no empty buffer is ever allocated.
- **Kernels.** An interpolation or a concatenation sums its parts'
  lengths, allocates once and copies each part with `memory.copy`; one
  part is itself. `s[i]` and `bytes_at` check the index and load a byte.
  `bytes_slice` checks `start <= end <= len` and makes a view of the same
  bytes, with no copy (`module.string.slice.shared`). `cmp`, `partial_cmp`
  and `<` compare bytes in order, a prefix first (`rt:str_cmp`). Integer
  text is written backward into a 20-byte string buffer and is its tail.
  `char_scalar`, `char_from_scalar` and the float bit casts are inline.
- **Host primitives.** Float text, `to_fixed`, `lower`, `upper`, float
  parsing and the snapshot hook are `hd:prim` imports. A string argument goes into the exchange buffer at 0, its
  length the parameter; a string result comes back there in the host
  codec, its LEB128 length then its bytes, and the import answers the
  encoding's length, so the bytes are what follows the prefix. One
  primitive takes at most one string, since the buffer holds one.

### 23.16 Counted Values And Cleanup

The ownership passes (`hd_tir::own`, docs/compiler/tir-v2.md, "Stage 2")
decide every count of a body; `emit/count.rs` lowers their decisions to
count glue, with release glue in `rt.rs`, and decides none itself.

- **Runtime roots.** The driver collects the runtime functions and runs
  their initializers before allocation. Linking retains only reachable
  code. `rt:new` initializes the counted header and payload (§23.4).
- **The header.** The count and descriptor word follow §23.4. Release
  glue is shared by layout; allocator and collector flags occupy the
  remaining descriptor bits.
- **Counts through any address.** `retain` and `release` find the block
  through the page table (§23.5), so a string's view and a slice count
  their buffer, and static data, which is in no block, is never counted:
  literals, the empty string and fieldless data objects need no check.
  `release` hands back the block at its last count; `rt:release_ref` then
  calls the descriptor's glue on the payload and frees the block.
- **Which values count.** A type's `layout::walk` identifies counted
  addresses through inline structs, tuples and enum payloads. Glue
  retains or releases those fields. Closure environments, owning trait
  values and suspension frames follow the same ownership model.
- **Every body is the passes' output.** The driver runs the passes over
  each body, the ones collection generates included (a function
  reference's call body, `hd_mono::adapter_call_body`, among them); the
  backend runs none. A body with a `use` read, which no pass decided, is
  refused as unsupported.
- **Reads, drops, stores.** A `Read` copies (a count of its own), moves
  (the place's count, and zeros left in what it read) or borrows (no
  count) as its mode says. A `Drop` releases a value or a local and
  zeroes it, so a later drop of it releases nothing. A value moves into
  the one use that consumes it (`hd_tir::own::consumers`, `Em::take`,
  `Em::take_from`), which zeroes it once read (`Em::settle`). A `Store`
  takes its new value (moved in, or counted) before it releases the old
  one.
- **One exit.** A function that holds a counted value runs its body in a
  block: a return stores its result, with a count for the caller, and
  branches out; after the block, the function releases its sub-body's
  exit set (`Body::exit_drops`), the locals that may still hold a value
  there. A place never stored, or already dropped, holds zeros, which
  release as nothing.
- **Glue for what the generic body cannot see.** Emission still adds
  counts where the lowering or the instance differs from what the passes
  saw: a consumer whose lowering cannot move a count in takes one and the
  value is released after it (`release_untaken`), unless the consumer
  never completes (a `Return`, a `Break`, a call that panics), after which
  no code runs; a list push and a map store move their value in, as a
  `Store` does (the passes count them as consumers); an `@intrinsic` call
  is lowered in place and lends every argument, so the passes pass them
  lent (`OwnCx::pass`); a lent argument a callee instance owns takes a
  count (`push_args`); a parameter its caller lends takes one on entry
  when the body owns it (its sub-body's exit set lists it), while one the
  borrow pass left unowned takes none, and an instance that receives it
  owned (a type parameter's) releases it at the exit
  (`Em::unowned_params`); and a cancelled suspension point, an edge the
  TIR does not have, releases the values the known-null pass lists for it
  (`Body::unwind_drops`; from a cleanup scope's ladder, those of every
  point inside the scope) and then leaves through the exit, whose exit
  set holds every local that may hold a value at a suspension point
  (`release_unwound`).
- **Uniqueness and pins.** `IsUnique` lowers to `rt:list_unique` or
  `rt:map_unique`, and `Pin` to the header's owner word (§23.20),
  of the header at their place: a local's or a value's, then struct
  fields, elements, bounds-checked, and dereferences, a view's included
  (`Em::place_header`, which walks the place as a rooted one, below).
  Since the passes put an `IsUnique` before every change of a collection
  place, a body's intrinsics and its thunks'
  check no uniqueness themselves: an element store, `MapSet` and
  `MapRemove` skip it, and `ListPush` only moves a full
  buffer (`rt:list_grow`). One check stays: std's `list_truncate` (`pop`,
  `insert`, ...) changes the list behind its reference, where no pass
  sees the change. A hoisted `IsUnique` (docs/compiler/tir-v2.md,
  "Hoisting") waits for the loop that follows it (`Em::hoisted`) and runs
  once before it under the loop's entry test: the statements before the
  test, which the pass found pure, run once more, then the test
  (`Em::hoisted_uniques`); in a suspending body, or where a default
  argument's specialization folded the test away, it runs unguarded,
  which costs only a copy an empty loop did not need. One behind a
  dereference reads the reference as its referent's address
  (`Em::deref_physical`, `place_header`) and runs only where no reference
  view is reachable (`RefViewsAbsent`); there the `COVERED` ones in the
  loop are skipped, in the shape the link folds away, and elsewhere they
  run. The loop caches (`Em::own_list_caches`) are the hoisting pass's
  stable list places (`Body::stable_lists`): a local's list or the list a
  reference local refers to, with no counted leaf in its elements, keeps
  its header, buffer and length in locals for the loop where no view is
  reachable, with no check of its own. The checker writes element
  references as plain `Ref`s, which the passes pin, and element reads as
  plain `Read`s (docs/compiler/tir-v2.md, stage 2 commit 12b); TIR has
  no pin or element-write intrinsic. Its header copies
  (`ValueCopy`, lowered as below, §23.17) are insertion's (`hd_tir::own`
  rule 7, stage 2 commit 13a).
- **Moves of parts, taking references, borrows.** A `move` read of a
  part takes the part laid out in its root's locals (`Em::part_slots`:
  struct, tuple and newtype fields and enum payload fields), whose locals
  a `Drop` of that part releases and zeroes too (`Em::own_drop_part`). A
  part at no such place at this instance, a boxed enum's payload or an
  open value's part, is read as a copy; the move pass then keeps the
  root's whole `Drop`, which releases it. A `Ref` that takes its local's
  count (move's rule 4) is the cell's address with that count, and zeroes
  the local; where the reference is no cell's address (a trait value's,
  an open cell's view) it counts as any reference and the local is
  released after it. A `borrow` read of any place takes no count
  (`Em::prepare_own_loans`: it joins `Em::loans`) and lowers as a copy's
  read would; one an erased body's thunk makes is that thunk's owned
  value, which the read's kept `Drop` releases, and one through a thin
  reference has a flag (`Em::view_loans`) that a view's getter sets for
  the fresh value it makes and that the read's kept `Drop` tests. A local
  a borrow is stored into (a binding local) holds no count
  (`Em::loan_locals`): its stores count and release nothing.
- **Thunks.** An erased body's thunk (§23.27) runs one instruction of an
  owned body outside the passes' view, as glue of its own (`Em::owned`
  is false there): each operand and capture arrives with a count, the
  instruction's reads copy, its result leaves with a count, and its exit
  releases every place it holds (`Em::release_all`).
- **What an instance decides** (docs/compiler/tir-v2.md,
  stage 2 commit 13b). The passes decide per generic body; three choices
  are left to the instance, where the layout is known.
  - *A type parameter's value parameter* is owned by the passes
    (`OwnCx::pass`): an instance that makes it a value moves the argument
    in at its last use; one that makes it a reference borrows it, and the
    value the call consumed is released after it (`release_untaken`).
    `Em::instance_loans` takes back what that costs where it can: a
    function parameter an instance receives borrowed, which the body never
    stores, addresses or captures, and whose every read of all of it lends
    or is an argument a direct callee borrows at this instance too, takes
    no count on entry and none at those reads (`hash_of(a)` for `a: &T`,
    or `notify(c)` forwarding `c: C` at `C = &Channel`); the reads join
    `Em::loans` and the exit set skips the parameter.
  - *A whole `Drop` after part moves* (`Em::own_drop_rest`): the move pass
    keeps a base's drop whole when its type names a type parameter, or
    when more than one counted field stays beside the moved part. Where
    the instance lays the value out in its locals and the moves or part
    drops before it in the same block took fields of one variant, the drop
    releases the one counted field they left, or nothing; with more left,
    it is one call of the whole value's glue, the taken parts being zero.
    An open value's parts were read as copies, so its whole release runs.
  - *A lent reference* (`ref_word::LENT`, `[*$p .f]` into what a lent
    parameter refers to) joins `Em::loans` with a view flag
    (`Em::view_loans`), as §23.31's projection loans: it takes no count
    where the referent is physical, and a view's projection makes one that
    its kept `Drop` releases under the flag.
- **Places rooted at the local** (docs/compiler/tir-v2.md, stage 2 commit
  12a). The checker names a place from its local through every step
  (`Read [*$0 [%5] .x]`, `Store [$0 [%1]] v`, `Read [*$0 .grid [%1] [%2]]`),
  not each base read into a value first. A place rooted at a local with
  an element or a variant
  among its steps, or a dereference after its first step
  (`Em::rooted_at`), goes through one walk (`Em::walk_place`,
  `emit/refs.rs`) from the local's value. A place rooted at a top-level
  binding takes it too when it has a step other than a field, or is read
  through fields, starting at the binding's static slot or at its Wasm
  globals read into locals; a store into or a reference to a binding's
  fields keeps the old lowering. A field picks its part (in
  locals, or an offset in memory), a variant its payload field (in a
  boxed enum's cell when the enum is boxed), a dereference of a data
  value the object's field, an element its slot in the buffer,
  bounds-checked. The first element of a local's list, or of the list
  the reference it holds refers to, takes the loop's cached header where
  the list is one of `Body::stable_lists` and its index unchecked where
  the loop's range proof covers the read (`list_loop_safe` and
  `range_indices` read such places as `Em::rooted_elem`). A dereference
  of a thin reference goes to its referent's address; where a view may be
  reached, a view's getter makes the referent fresh instead, the walk
  reads on in that value to the value the next element or dereference
  needs (which the view's source holds too), and the fresh value is
  released, under a flag, once the instruction is done
  (`Em::release_fresh`). A `Read` then loads only the leaves the place
  names (copy or borrow as its mode says; a `move` reaches the walk only
  for a part at no place of its root's locals, read as a copy); a
  `Store` stores them, counting the new value before releasing the old;
  a `Ref` is the address of the last element's slot or the last
  dereference's referent, then the fields after it, as a `Ref` of that
  head lowers (`Em::ref_from`); an `IsUnique` or a `Pin` takes the
  header the place names (`Em::place_header`). A place that ends with a
  dereference of a reference followed by fields only reads, writes or
  addresses the referent as `[*%r .x]` does (`Em::read_part`,
  `deref_field_write_at`), views included. So `bodies[i].x` loads one
  field (§23.31's "Read the selected field" is the meaning of the place).
- **Bounded memory.** `crates/hd_driver/tests/std_rt/count_tests.hd`
  repeatedly builds strings, chains of nodes and enum payloads, checking
  that the heap stays bounded.

### 23.17 List Operations

List kernels live in `emit/list.rs`, with buffer/header glue in
`rt.rs`; changes preserve collection value semantics (§23.7).

- **Literals.** A literal sizes its buffer exactly for its items and its
  spreads (`expr.list.spread.insert`), stores each item, copies each
  spread's elements with one `memory.copy`, then makes a header over it
  (`rt:list_header`). Every empty literal shares one static empty
  buffer, whose count word is 2, so a change always copies it first. A
  tuple's rest list is its last member.
- **Copies.** `ValueCopy` and `SnapBegin` make a new header over the same
  buffer, which counts one more owner (`rt:list_copy`); a struct's copy
  copies each list among its fields at any depth (`layout::coll_leaves`,
  after `hd_mono::holds_coll`). An operand stored into an element, a
  field, a payload or a tuple is copied the same way, unless its own
  instruction made it. Such a copy's own
  count belongs to a temporary that the function's exit releases.
- **Changes.** The passes' `IsUnique` before a `push` or an element
  store makes the buffer the list's own (`rt:list_unique`). A shared
  buffer is copied: its elements take their own counts, the lists among
  them become copies in turn, so a nested write never reaches another
  list, and the old buffer loses one owner. A `push` into a full buffer
  moves it to one of twice the length plus 4 (`rt:list_grow`), and frees
  the old block without releasing its elements, which moved. A store
  counts the new element before it releases the old one.
- **Release.** A header's release glue releases its buffer; a buffer's
  (`rt:drop_elems`) releases each element's counted references up to the
  length. A list of strings or of structs holding lists frees everything
  at its last release.
- **Loops.** `SnapEnd` gives the snapshot's buffer back at once, so after
  the loop the list's next change copies only when something else still
  shares the buffer.
- **Pins.** The passes' `Pin` gives the header its owner word while
  element references hold counts of the buffer, and a `push` into a
  pinned list panics `collection-pinned`. Element
  references use the typed views of §23.20, preserving the header and
  the referenced slot when a buffer is copied.
- **Indexing.** An index is checked against the length before it is
  narrowed: a 64-bit index past 4 GiB is `index-out-of-bounds`, never
  wrapped.
- **Strings over lists.** `string_from_bytes` and `concat_all` copy a
  list's bytes, or its strings' bytes, into one new string buffer.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/list_tests.hd`
  builds lists of strings and lists of structs holding lists, and copies
  and changes a list, each in a loop, and checks that the heap does not
  grow after a warm-up.

### 23.18 Map Operations

Map kernels live in `emit/map.rs`, with table glue in `map.rs` (§23.7).
Each operation implements collection value semantics (§15.2;
`crates/hd_mono/DESIGN.md` §13.12), using the table's count for copy-on-write.

- **The table.** A map is a list's kind of header over one counted
  block: the live and written entry counts, the rebuild count, the
  capacity, then an index of entry numbers plus one (0 empty) twice the
  capacity long, each entry's hash (-1 once removed), and the entries
  in the order they were added, each a `(K, V)` record laid out as a
  tuple. Probing is linear from the hash's low bits; the index is at
  most half full, so a probe always meets an empty slot.
- **Keys.** A key hashes and compares through the operations collection
  chose: an integer, `bool` or `char` through `int_bucket_hash` over its
  bits zero-extended and `i32.eq` or `i64.eq`, a string through
  `string_bucket_hash` and a byte comparison, any other key through its
  `key_bucket_hash` and `Eq.eq` instances. `map_hash_seed` is a runtime
  global filled from the host's `hd:rt.hash_seed` on first use.
- **Literals and puts.** A literal makes a table with room for its
  entries and puts each in source order (`expr.map.order`). A put of an
  equal key replaces the value in the entry's place
  (`expr.map.duplicate.last`) and releases the key it was given; a new
  key appends an entry. The table takes a count of each key and value it
  keeps. Every empty literal shares the static empty buffer of §23.17,
  now 16 zero bytes, which is an empty table too.
- **Growth.** A put into a full table moves the live entries in order
  into a new table, of twice the capacity (at least 4) when at least half
  are live and of the same capacity otherwise, rebuilds the index from
  the kept hashes, and frees the old block without release, since its
  entries moved.
- **Removal.** `remove` marks the entry's hash -1 and lowers the live
  count: probes pass it, iteration skips it, and the survivors keep
  their order. The result takes its own count of the value before the
  table releases the entry's key and value.
- **Copies and changes.** `ValueCopy`, `SnapBegin` and `SnapEnd` are the
  list ones (§23.17), since the header is the same. `m[k] = v`,
  `remove` and a change through a value place first make the table the
  map's own (`rt:map_unique`): a shared table is copied whole, each live entry
  takes its own counts, and the collections among the values become
  copies, so a write through `m[k].push(v)` reaches only this map.
- **Iteration.** A counted `for` iterates the snapshot through
  `MapSlots`, `MapSlotNext` and `MapSlotEntry`, which read the entries
  in place.
- **Release.** A table's glue (`rt:drop_entries`) releases each live
  entry's key and value.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/map_tests.hd`
  builds maps of strings and maps of lists, removes in a loop, and
  copies and changes a map, and checks that the heap does not grow after
  a warm-up.
- **Why glue, not hd.** The table's code depends on the entry's layout,
  which only the backend knows: std has no size or layout of a type
  parameter over `std.rt.memory`, as for list buffers.

### 23.19 Closure And Trait Values

Closure environments and function-value calls are in `emit/closure.rs`.
Trait values and vtables use `emit/dyn_value.rs`; map cursor closures use
`emit/iter.rs` (§23.8).

- **Function values.** `(code, environment)`: the function-table slot of
  the code instance and the address of a counted environment, 0 when
  nothing is captured. A code instance takes the environment first, then
  its parameters, then the providers of its type's row in key order, so a
  call of a value pushes them and makes one `call_indirect`, or a direct
  call when collection knows the closure (`KnownClosure`). A function
  reference (`double` as a value) is an adapter instance that ignores its
  environment and calls its target.
- **Environments.** One counted object per closure value, holding each
  capture inline (OBJECT_MODEL_TIER1.md §9.1): an immutable capture's
  value, with a count of its own, or a shared local's cell address. Its
  descriptor's glue releases the captures, so each closure literal's
  layout has its own release, as §23.8 says. A closure copied counts its
  environment once more; the environment goes at its last copy.
- **Cells.** A local that a closure shares (`CaptureMode::Shared`, one
  assigned after it is captured) lives in a cell: a counted object
  holding its value, whose address is the local's one Wasm value. The
  local's declaration makes the cell (a new one per loop iteration, so
  closures made in a loop each see their own, `flow.for.binding.fresh`);
  every later store writes the cell, counting the new value before it
  releases the old; a read copies the value out with a count. A shared
  `mut` parameter moves into a cell on entry. A capture reached before
  its declaration (a recursive reassignable local function's own name)
  makes a cell of zeros, which the declaration fills.
- **Immutable self-recursion.** A recursive `:=` local function's own
  binding is not an environment field. Its code reconstructs that value
  from its current function-table slot and hidden environment parameter.
  Entry liveness owns that value like a copied capture; a suspending cold
  constructor saves it in the initial frame. A nested closure copies the
  reconstructed value. Thus value-only captures form no cell/environment
  back-link. A reassignable `let` keeps its shared cell, as does explicit
  address-taking of the binding, whose address must remain stable.
- **Trait values.** `(data, vtable)`. The data is the value itself when
  its layout is one counted address (a data value, a list, a map, a
  reference), 0 when it has no Wasm values, and otherwise a new counted
  box of the value's layout, whose descriptor releases its fields. So the
  data is always one counted address, and copying or dropping a trait
  value counts its data with no knowledge of the type; this is why a
  vtable carries no release word. A vtable is static data: one
  function-table slot per method of the trait, in declared order, each an
  adapter that loads the payload from the box (or passes the data itself)
  and calls the method's instance, then the addresses of the direct
  supertraits' vtables. Widening to a supertrait loads those addresses
  along the path; a supertrait's method dispatches the same way.
- **Reference payloads.** A `&S` converted to a trait value
  (`RefToTraitValue`) keeps the `Ref` object as its data. A slot whose
  method takes the value reads it through the `Ref`'s `get` closure
  (`SlotRecv::ReadThrough`); a `&self` or `&mut self` slot gets the `Ref`
  itself. A trait value reference installed as a provider
  (`RefAsProvider`) is the `Ref` with the vtable that its `get` returns,
  and `$.use(K)` (`ProviderAsRef`) reads the `Ref` back, so two uses are
  one reference (`req.ref.same-place`).
- **Inspection.** `TypeId` is a struct of two static strings, the key and
  the printable name, never counted. `TypeId::of`, a concrete value's
  `runtime_type` and the vtable's `runtime_type` slot answer them with no
  allocation; a downcast compares the key the payload's vtable reports
  with the target's and unboxes on a match.
- **Identity.** `is` compares the first counted address: a data value's,
  a list's or a map's header, a trait value's data; two references
  compare their addresses (§23.20). A trait value that boxed a value
  compares its boxes (`ref.identity.same-place`).
- **Providers.** `$.with(K: v) body` makes `v` the provider of `K` in the
  body, borrowing `v`'s locals, whose count lasts to the function's
  exit. (`Guard`, a match guard's condition block and its two branches,
  lowers here too.)
- **Iterators.** `List.iter()` uses std's direct cursor: a reference to
  the source list, its length at creation, and an independent position.
  Its std `next` checks invalidation and reads the indexed element;
  neither constructing nor stepping it needs a closure. `Map.iter()`'s
  `MapEntries` still uses `map_steps`: its step closure keeps the map's
  header and creation counts, while the cursor keeps its own position.
  `rt:map_step` checks those counts on every step, including exhaustion.
  Both report `iterator-invalidated` when the collection changes
  (`flow.for.invalidate.panic`). `list_truncate`, used by `pop`, `insert`,
  `remove_at` and `clear`, reads through the list reference and truncates
  it in place.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/closure_tests.hd`
  calls function references and closures, shares a local between a scope
  and its closures, dispatches through boxed and widened trait values,
  and makes closures and trait values in a loop, checking that the heap
  does not grow after a warm-up.

### 23.20 Reference Addresses And Typed Views

References use addresses and typed views; cleanup scopes and element
references preserve their owners and pins (OBJECT_MODEL_TIER1.md §6.3).

- **References as addresses** (issue #587). The checker names each
  reference's place in the TIR (`Ref` of a place: a local, a top-level
  binding, a place through another reference, a data object's field or a
  list element, then a struct field path; `docs/compiler/tir-v2.md`)
  and each read, write and identity through one (a `Read` or `Store` of
  a place through it, `RefIdentity`), with no closure form (stage 1 of
  TIR v2 removed them).
  Here `std.core.Ref[T]` is one address (`layout::Kind::Ref`,
  `emit/refs.rs`), the thin form of §23.5:
  - a local some reference refers to lives in a cell
    (`closure::cell_locals`), one per execution of its declaration, and
    the reference is the cell's payload address plus the field path's
    offset, so `&mut x` in a loop over a local declared outside it
    allocates nothing;
  - a top-level binding some reference refers to lives in a static slot
    of its layout instead of Wasm globals (`Module::addressed`), whose
    address no count changes;
  - a place through a reference is that address plus the path's offset;
  - a field of a compiler-supplied data object is its payload address
    plus the field's offset;
  - a list element is its address in the buffer (below).
  A read loads the target's values from the address and a write stores
  them, counting the new value before it releases the old. A struct field
  assignment through a reference is one `Store [*r .f]` of that field,
  without rebuilding its parent.
  The source reference evaluates once before the RHS. A compound assignment
  reads the old field before its RHS; both kinds store after the RHS,
  preserving changes it makes to neighboring fields through aliases. A reference
  is counted through the page table at any address in its object, so a
  reference to a field keeps the whole object alive. `is` compares
  addresses, and `identity_of` is no base, the address as the serial and
  no key. A reference to a trait value is that trait value, the data
  address and the vtable, as Rust's `&dyn Tr` is: `RefDyn`'s other block
  makes it, `&mut held` of a trait value is the trait value in `held`,
  and a provider installed behind a reference is that trait value
  (`RefAsProvider`). A local copy that a closure captured and a
  suspending body's parameter, whose cell the frame cannot hold yet, are
  no places in memory: a read-only reference to one is to a new object
  holding a copy, and `&mut` to one stays refused.
  hello (`println(42)`) went from 6,007 to 5,040 bytes in the dev
  profile and in release; 16 of them are the count of blocks
  `std.rt.alloc.allocated` reports, which `ref_tests.hd` reads to see
  that a loop writing through `&mut` to a local allocates nothing.
- **Cells made before use** (issue #678). A use of a cell reads its local
  directly when the cell certainly exists there (`Em::cell_made`): the
  prologue made or received it (a parameter moved into its cell, a
  closure's or a thunk's shared capture), or known-null found the local
  holding its value there (`Body::full_cells`, rule 5 of
  docs/compiler/tir-v2.md, "Known-null"). The pass gives each local a
  `Ref` of its own storage or a shared capture puts in a cell a bit that
  is set where it may hold no value: before its first store, after a
  `Drop` of it, a `move` read of it and a `Ref` that takes its count;
  states join by union at the ends of branches and arms and at a loop's
  head. A `Read`, `Store`, `Ref`, `IsUnique` or `Pin` of a place rooted
  at the local, a `Closure` that captures it, and a `Loop` (whose stable
  places and hoisted checks are read before it) get a row where the bit
  is clear on every path. Every store makes the cell (its declaration,
  or `Em::cell_of` for any other), and only a `Drop`, a move or a taking
  `Ref` zeroes the local, so the local holds the cell's address there.
  Any other use tests the local and makes a cell of zeros when it is 0: a
  use some path reaches before a store, one inside a cleanup suite (which
  also runs at jumps the walk does not follow there), and every use in a
  suspending body, whose resume reloads the locals from the frame and
  enters blocks in the middle (§23.21).
- **A reference at its local's last access** (issue #701). `&x`, the
  address of cell local `x`'s cell with no field path, takes the local's
  count of the cell where move's rule 4 marks it (`Ref take`,
  `ref_word::TAKES`, docs/compiler/tir-v2.md, "Move"): the reference is
  stored as owned and the local is zeroed, and the pass deleted the
  local's `Drop`. Where the reference is no cell's address at this
  instance (a trait value's, an open cell's view), it takes a count of
  its own and the local is released after it. The pass keeps the count
  for a cell a closure shares, every cell of a suspending sub-body (its
  frame saves the local, and a resume enters blocks in the middle), and
  `&x.field`, whose address `rt:ref_project` makes with a count of its
  own. `&Node { ... }`, which the checker binds to a hidden local, makes
  its node with no count change (`allocation/new-reference`);
  `hd_driver/tests/cases/counts/reference-*` hold the boundaries.
- **Cleanup scopes.** A `Scope` with `defer` suites uses an
  exit ladder (`emit/scope.rs`, crates/hd_mono/DESIGN.md
  §12.2): each suite has a flag that its `Defer` sets; a `break`,
  `continue` or `return` that leaves the body stores its exit's code and
  branches to the scope's end, which runs the registered suites last in,
  first out, as a forbidden context, then takes the exit again through
  the next scope out. A suite that suspends (`using`'s `dispose!`) uses the
  cleanup state machine (§23.21).
- **Releases at last use.** A reference is released after its last use,
  not at the end of its block (§6.3 rule 5), so a counted value never
  waits for the function's exit (§23.16). Insertion places the `Drop`s
  (docs/compiler/tir-v2.md, "Insertion" rules 2 to 4): a value's after the
  statement of its home block that holds its last use, a local's after
  the statement of the innermost block holding all its accesses that
  holds the last one, and before each jump the drops of what it leaves
  that may still hold a count, up to the innermost cleanup scope it
  crosses, whose suites may still read the scope's locals. The emitter
  lowers each `Drop` where it stands.
- **Element references own their pin.** An element reference is the
  element's address in its list's buffer, and its count is a count of
  the buffer. Insertion puts an `IsUnique` and a `Pin` of each list on
  the reference's path before it, outermost first (docs/compiler/tir-v2.md,
  "Insertion" rule 6). `IsUnique` makes the buffer the list's own, and
  `Pin` gives the header its owner word (§23.7), with no count: the
  function-table slot of the list's clone glue plus one. A buffer whose
  header has an owner word has that header as its one owner, so every
  count past the first is a reference's: the list is pinned while the
  buffer's count is over 1 (`rt:list_pinned`), and a `push` or a
  truncation then panics `collection-pinned` (`ref.elem.release.copies`).
  The reference's own count of the innermost buffer is the pin, so it
  ends with the reference's `Drop`; an ancestor list is exposed and never
  pinned. A change through the list writes the buffer in place, which
  every element reference sees. A copy of a pinned list copies its buffer
  at once through the owner word's clone glue (`rt:list_copy`), and a
  copy of an unpinned one clears the word, since the buffer is shared
  again. When the element layout contains collection values, `Pin` also
  sets bit 31 of the owner word. This exposure survives the pin: an
  escaped reference can still reach a descendant after that pin ends.
  `ListCopy` clones exposed buffers regardless of the direct pin count,
  masking off bit 31 before calling the clone glue. Recursive collection
  copies isolate pinned descendants; the new header starts unexposed.
  Fresh containment uses the same collection-copy glue, so it cannot hide
  an exposed child behind a shared outer buffer. Lists of scalar or
  reference elements gain no exposure bit: their ordinary pin/count fast
  path is unchanged. Each copy of an element reference holds its own
  count, so a reference stored in a field, returned, or captured by a
  closure called later keeps the list pinned until that copy's last use.
  Two references to one slot share its address (`ref.elem.identity`). A
  trait value of a value that is one counted address is that address,
  never a copy.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/ref_tests.hd` writes
  through references to locals and elements, checks that a pin ends with
  its reference's last use, and takes references to locals and elements
  in a loop, checking that the heap does not grow after a warm-up.
- **Host providers behind references.** A key whose trait has a `&self`
  or `&mut self` method, as `Console` has since `write_line!` takes
  `&mut self`, gets its host provider behind a reference (`req.ref.host.reference-values`):
  the entry's poll builds the host trait value, no data beside the
  vtable of host stubs, and calls the driver's `std.rt.host_provider`
  instance at `dyn K`, whose result it installs as the provider. So
  `println` reaches the host through a `&mut dyn Console`. The reference is the host trait
  value itself (a reference to a trait value has that representation), and
  `host_provider` allocates nothing.

### 23.21 Suspension Frames And Wakers

Suspending code uses §14's state machine, waker, entry driver
and cancellation (docs/future-work/CANCELLATION_AND_CLEANUP.md) over
counted frames in memory 1 (`emit/susp.rs`, `task.rs`).

- **The frame.** A frame is a counted object. Its payload starts with
  the task prefix: the poll slot (`F_POLL`, a table slot), the cancel
  slot (`F_CANCEL`), the state (`F_STATE`), the flags (`F_FLAGS`:
  `ACTIVE`, `DONE`, `CANCELLED`, `UNWINDING`, `ABANDONED`, and `EXITED`
  once the body has left) and the child (`F_CHILD`). The save area
  follows at `F_SAVED`: one slot for every Wasm local of the body but
  the frame and `pc`. A point's suspend stores them all and a resume
  loads them all (`Ins::SaveAll` and `Ins::LoadAll`, expanded once the
  body's locals are known). That is simpler than §14.3's live set, and
  costs frame bytes, not code.
- **The parts.** The instance's own function
  is the cold constructor and the body, poll and cancel are its parts
  (`Part`). The frame's release glue is a fourth part. Until the body has
  left, the glue releases each place the body owns that the save area
  holds, the results an `all!` kept, the providers and the child. The
  body is `(frame, args..., providers...) -> (results..., frame)`. A frame
  of 0 starts fresh, and the frame is made at the first Pending (§14.3).
  The cold constructor stores the arguments and providers in a fresh
  frame at state 0, with a count of each. A reloaded local holds the
  count its frame saved, so only a fresh start takes counts of its
  parameters. A frame retains its providers, since the scope that
  installed them may end before the frame does.
- **The state machine.** States are numbered in structural order, so a
  subtree's states are contiguous (`susp::plan`). A block whose
  statements hold states is entered by one `br_table` on `pc`. An `if`,
  `match`, `switch`, `guard` or `&&`/`||` that holds the resumed state
  takes the branch that holds it, whatever its condition was. A direct
  `g!()` steps `g`'s body with the saved child. A stored suspension, a
  trait-value bang call or a suspending function value is polled through
  its poll slot.
- **Suspending function values.** A suspending closure's code and a
  function reference's adapter are cold constructors. They return the
  frame that the call's `mut Suspend[T]` holds. The constructor copies
  the captures from the environment into the frame, with a count of
  each. The body runs only over that frame. A call whose callee's row
  parameter carries a key is still refused (the context value of
  crates/hd_mono/DESIGN.md §12.4). The old check let one through when its
  keys were also declared, and then lost the closure's providers.
- **Cancellation.** Each point's cancellation branch cancels the child,
  waits while the child unwinds, then leaves through the cleanup scopes'
  exit ladders by `Exit::Cancel` (§23.20). A counting body takes its one
  exit with a `cancelled` flag. The frame's cancel marks a cold frame,
  or steps a started one with `UNWINDING` set. `CancelEnter` and
  `CancelLeave` bracket a cancel for `req.abandon.carry`.
  `task_cleanup_frame`, `task_pending_frame` and `task_abandon` are
  runtime helpers, as in §14.6.
- **Waiting suites.** A `using` binding's `dispose!` that suspends is a
  rung a resume can enter. Its points are shielded: they have no
  cancellation branch. Under `abandon()`, or when the awaited child was
  cancelled elsewhere, they kill the child instead of waiting. A frame
  cancelled while such a suite waited leaves by `Cancel` once the ladder
  has run.
- **`all!` and `race!`.** `all!` keeps a done mask, each child's result
  and a counted waker node in the save area. Its first poll visits every
  child; later polls visit only unfinished children whose ready bit is
  set (#630). While a child is polled, host leaves register its node and
  bit. A host completion sets that bit and follows the node's enclosing
  links, so nested `all!` points wake the appropriate outer child.
  Shared cancellation cleanups keep every distinct live waiter, merging
  bits for the same node generation. Registrations compare full host
  handle generations; waiter targets separately use the weak node slots
  of §14.4. Completion/abort releases edge storage, without retaining or
  releasing enclosing target nodes. A stale wake never reads a freed node. Completing or cancelling an `all!` releases
  its node, and frame drop/trace glue handles a node saved while pending.
  Dev builds export the `i32` global `hd.all_polls`: 30 separately woken
  children take exactly 60 child polls. `race!` is §14.5's frame over the
  list (`task::race_*`). The results of done children are released by the
  cancellation branch and by the release glue.
- **The waker and the entry driver.** `hd.wake(n)` marks completed host
  handles in a growable wake table (`WakeTab`). A host leaf frame (the
  handle at `F_SAVED`, then its arguments) takes its handle's mark. The
  root driver (`task::root_poll`) keeps the root frame and its owner in
  globals. It panics for a competing driver, a reentrant poll and a poll
  after completion. It continues a cancelled root's cleanup and answers
  -1 while Pending. A cancelled root answers -2, or -3 when its cleanup
  was cut short. The module exports `hd.cancel` and `hd.abandon` for a
  `main!` that suspends. `block_on` drives with the same checks, and
  hands an abandon's rewake to the outer driver.
- **Scenarios.** `Entry::Scenario` is `main`'s entry with
  `hd.scenario.cleanup`. Its held call answers every poll Pending, and
  flags the frame active for `reentrant-poll`. A direct step of the held
  call builds the frame through the cold constructor. A runtime profile's
  own host trait gets a vtable of imports from `hd:<Trait>`. Only methods
  with no arguments and a `void` result are built (`Gate.wait!`).
- **Bounded memory.** `crates/hd_driver/tests/std_rt/task_tests.hd` makes
  cold frames, `all!`, `race!`, `cancel!` and `block_on` frames in a
  loop, and checks that the heap does not grow after a warm-up.

### 23.22 Code Size: The Local Cleanup And The Allocator

Issue #581 brings the allocator toward §10.1's size budget of
REFCOUNT_RUNTIME.md (`opt.rs`, `lib/std/rt/alloc.hd`).

- **The local cleanup.** Emission gives every TIR value a fresh local, and
  checks every integer operation. Before encoding, `opt::locals` rewrites
  each function by rules that keep what every path computes:
  - a set and get of a local used once is the value left on the stack,
    and another adjacent set and get is a `local.tee`;
  - a store to a local nothing reads is a `drop`;
  - a copy of a local, a global or a constant moves to its one read in
    the same straight-line run;
  - a `while` test's `if … else br` is a `br_if`;
  - an `if` whose arms each end by setting one local, read once right
    after it, is a typed `if`;
  - two values computed into locals only to be pushed in order stay on
    the stack;
  - constants fold: a zero test of a nonzero constant, `== 0`, a
    power-of-two multiply, divide or remainder, and a constant added to
    a load or store address, which becomes its offset;
  - unused locals are not declared.
  Across functions, `opt::constants` reads a top-level binding as its
  constant when an init stores that constant and nothing else writes
  it, as the allocator's `PAGE` and tag constants.
- **Arithmetic in `std.rt`.** The runtime modules' integer arithmetic
  wraps, as the release profile's does (`types.arith.release`): their
  values stay in range by construction, and only std may use them
  (`module.rt.std-only`). An unsigned quotient or remainder needs only
  the zero check, and none for a nonzero literal divisor.
- **The allocator's source.** `class_bytes` computes the size classes
  instead of matching on them. A new small page's header is zeroed with
  `fill`, as is a freed run's table entries. The wake table grows and
  frees through the std allocator, so a program that counts no longer
  links the bump allocator as well.
- **Literals.** A string literal is a view into static data (§23.7), so
  it is never counted: `block_of` finds no block on a static page, and
  retain and release change nothing. A program that allocates nothing
  links no allocator: a `main` that only computes and panics with a
  literal is 656 bytes.
- **Remaining allocation.** hello still links the allocator. Its literal is no
  longer the reason, nor, since references are addresses (§23.20), the
  `&mut dyn Console` that `println` reaches the Console through, which
  is now the host trait value itself. The host call's frame is a counted
  object (§23.21), and `"$value"` builds a counted string. The counting
  code both need links `retain`, `release` and `free`. Making hello link
  nothing would take formatting into the exchange buffer. That is a
  design question for the owner.

### 23.23 Host Value Codecs

Host value codecs implement the ABI table, test-runner traits, host
handles and runtime-profile traits, so programs that use `fs`, `process`,
`host`, `sys`, `net`, `http`, `time` and `random` run on this backend
(`boundary.rs`).

- **One wire format, generated in the module.** The host contract stays
  crates/hd_run_wasmtime/DESIGN.md §17: the exchange buffer is memory 0,
  `hd.x`, and every structured argument and result crosses in it in the
  §17.4 encoding. This backend generates the encoders and decoders over
  its own layouts, so `host/*.mjs` and the wasmtime host share one
  exchange-buffer contract, and the
  ordinary boundary methods never read the heap (memory 1). Checked
  reports inspect its exported counters separately (§23.24). The alternative,
  host codecs that read the heap's layouts, would need the host to know each
  type's offsets and slot sharing (§23.3) and to allocate counted
  objects for its results, which means calling back into the module
  during an import (§17.3 forbids nested calls); it would also tie the
  host to this backend's layouts.
- **Boundary types.** `boundary::of` describes a type as an `LTy` from
  its linear layout: an integer (`bool` one bit, `char` 21), a float, a
  string, `List[u8]`, any other list with its element's layout, a
  struct or a tuple as a record of its fields' values, an enum or an
  optional as its tag and shared slots (each payload field's value
  indices from `layout::enum_mem`), a host handle with its vtable, and,
  at a profile's boundary, a private-field type as its JSON text through
  the codecs collection gathered (`module.boundary.serialize.exact`). A
  newtype is its field, so `Path` is its string with its length; only a
  lone `string` argument crosses as its bytes alone. A map, a function, a
  data object, a recursive enum or another trait value is refused at
  Link, and nesting past 32 levels is too.
- **Encoders** are `rt:enc` helpers, `(off, values...) -> off'`, one per
  `LTy`, each growing the buffer (`rt:fit`) before it writes. A string
  and a byte list are one `memory.copy` from the heap to the buffer; a
  list's elements are loaded from its buffer at their offsets. A value
  is borrowed: an encoder takes no count.
- **Decoders** are `rt:dec` helpers, `(off) -> (values..., off')`. They
  check every LEB128 byte and length against the buffer's size, a
  variant index against the variant count, and a narrow integer against
  its range (`host-contract`). A string of bytes is a new string buffer
  filled by one `memory.copy` from the buffer, and no bytes is the one
  static empty string (§23.7). A byte list is a new list buffer filled
  the same way; any other list decodes each element into its slot and
  counts it in the buffer's length once stored, so the glue never
  releases an unwritten one. Every value a decoder returns arrives with
  its count, as a call's result does (§23.16).
- **Host calls that never wait** (`Helper::HostCall`): the structured
  arguments are encoded one after another from offset 0, the scalars are
  the import's parameters, then the length; a scalar result is the
  import's result, a structured one is decoded from offset 0.
- **Host leaves** (`Helper::HostCold`, `HostPoll`): the cold frame keeps
  the arguments as one record after the handle (`LEAF_ARGS`), each with a
  count of its own, a list as its copy (`rt:copy_elems`); its glue
  releases them. The first poll encodes them for `.start`. Any result
  but an enum of tags decodes from offset 0 after `.start` or `.finish`;
  `Result[void, E]` with a payload-free `E` keeps the two-byte read, so
  hello's code is unchanged.
- **Host handles.** A handle is a trait value whose data is a counted
  box of its number and whose vtable's slots (`Helper::HandleSlot`) load
  the number and call the inner host call or leaf with it first; a
  supertrait's vtable, as `Disposable`'s, follows the slots (§23.8). A
  `&mut dyn TcpStream` result calls the `std.rt.host_provider` instance
  collection recorded (`handle_ref`), which returns the trait value
  itself, and releases the box's first count. A handle never crosses to
  the host. A profile's own handle trait is imported from `hd:<Trait>`
  by method name, as `disposed-file`'s `FileHandle`.
- **Runtime profiles.** Every argument and result of a profile trait's
  method crosses in the buffer, so a `u8` result of 300 is the
  `host-contract` panic (`module.profile.host-result.contract-panic`)
  and a NaN, an infinity or `-0.0` keeps its bits.
- **Checked by** `tests::every_runtime_helper_validates`, which builds
  a host call and a handle's leaf over a record of every kind of boundary
  type and validates the module, and by the runtime fixtures of the
  profiles, the property and snapshot runners, `Random.fill` and
  `ConsoleInput.read_line` in `LINEAR_PASSES.tsv`. Programs over the
  real `fs`, `process`, `env`, `args`, `sys`, `clock`, `random`, `net`
  (loopback TCP and UDP) and `http` (a local server) hosts print the
  same output on both engines.

### 23.24 Checked Builds

Checked builds audit the backend's allocation and reference counting.
`Packages::checked` selects it internally;
`HD_CONFORMANCE_CHECKED=1` selects it in conformance runs. A normal build
is unchanged.

- **The checked runtime.** The driver roots `std.rt.checked`
  (`lib/std/rt/checked.hd`) in place of `std.rt.alloc`'s counts, so
  emitted code calls its `allocate`, `free`, `retain` and `release` by
  key, as it calls the plain ones (§23.16). Optional host-side reporting
  also adds the descriptor metadata described below.
- **Poison, and no reuse.** A new block is filled with `0xa5` bytes, so
  a read before its constructor's store finds no zeros. A freed block is
  filled with `0xde` bytes and never goes back to the allocator, so a
  stale reference reads poison, never a newer object. A free of an
  address that is not a block's start, or of a poisoned block, fails.
- **Counts audited.** `retain` and `release` fail on a block whose count
  word is poison, a freed object, or 0 or fresh, a block with no count.
- **Leaks at the end.** `std.rt.checked` counts the blocks given and not
  freed. The entry's poll (`Helper::CheckedPoll`) audits once it answers
  anything but Pending. It frees the wake table, which is the runtime's
  own, and releases and zeroes every top-level binding's counted
  references (`Helper::Teardown`, from `Module::stored`, which emission
  fills). Then no block may be live. A test case's instance serves the
  next case, so the poll then runs every module's init again, but
  `std.rt`'s: the next case starts from the bindings a fresh instance
  has, as `flow.panic.isolation` describes. A scenario's host reads its
  bindings through `hd.scenario.cleanup` after the root ends, so a
  scenario has no audit.
- **The leak report** names the lowest live block: its address, count,
  descriptor word and size, found by scanning the heap, since a freed
  block holds poison and an unwritten one fresh bytes or zeros.
- **Failures** panic `count-audit` with what failed, the block's address
  or the number of blocks leaked. It is not a stable panic category
  (`flow.panic.known-categories`): only a bug of this backend or its
  runtime can reach it.
- **The collector** (§23.28) runs after the teardown and before the
  audit, so a cycle the bindings held is freed first.
- **Collected-cycle totals (#714).** The CLI enables the `checked-report`
  Cargo feature of the driver and backend. In checked builds only,
  `CycleFree` calls `std.rt.checked_report.collected`, which increments a
  raw `u64` array by descriptor ID. `cycle.freeing_cycles()` distinguishes
  actual white-block frees from the initial zero-count sweep. The ordinary
  collector algorithm is unchanged. The array grows by doubling and holds
  no counted links; like collector work arrays, its raw storage is outside
  the leak audit.
- **Host-side kinds.** A checked descriptor gets a distinct table slot for
  each release-glue/kind pair, pointing at the same release function.
  Closures, shared local cells, frames, waker/all/race nodes, list/map
  buffers and headers stay distinct even when their layouts share glue.
  Reference roots represent objects, not capture cells. Other user values
  fall back to objects because shared layout glue retains no nominal name.
  The `hd.cycles` custom section is version byte 1 followed by records
  `(descriptor ID: little-endian u32, kind: u8)`, where kinds 0 through 6
  mean object, closure, cell, frame, node, buffer and header. Descriptor IDs
  are table slots plus one. The host reads the counter array through
  `hd.cycles.pointer` and `hd.cycles.length` and the exported `hd.heap`,
  aggregates it using that metadata, and prints one `cycles freed:` line
  on stderr when a drive ends. No kind names or formatting code enter the
  guest. Totals are cumulative within an instance; fresh instances start
  at zero. Invalid report metadata is ignored by both hosts.
- **The feature boundary.** Without `checked-report`, compiler-side
  descriptor aliases, metadata and report exports are absent. The web
  embedding excludes both checked runtime source modules, since the
  playground compiles only unchecked programs. Unchecked modules preserve
  their existing descriptors and bytes even with the CLI feature enabled.
- **The sweep.** `HD_CONFORMANCE_CHECKED=1` builds every
  case of the conformance runner in checked mode, so each must pass with
  its counts audited.
  `crates/hd_driver/tests/std_rt/checked_tests.hd` tests the runtime's
  poison, its reuse rule and its counts.

### 23.25 Count Elision

The ownership passes decide which counts a body elides
(docs/compiler/tir-v2.md, "What each emitter rule becomes", maps each
rule to its pass); the emitter lowers their decisions and decides none
(§23.16). The backend keeps the parameter convention, which `OwnCx::pass`
reads, and the counts of the code it generates.

- **Parameters by their type** (`layout::Pass`). A plain function, one
  that does not suspend, takes a value parameter `T` as its own copy:
  the argument arrives with a count of each counted reference in it, and
  the callee releases it as it releases any place it owns. A `&T`
  parameter is borrowed: the caller keeps the argument alive for the
  call, and neither side counts it. A borrowed parameter the body owns
  (its exit set lists it: the body writes it, or a cell holds it) is
  retained on entry and owned from then on. Only the parameter's type
  decides, so every caller and the callee agree without reading each
  other's bodies (§4.3's header rule). A caller reads the types from the
  callee instance's own parameter locals (`emit::callee_params`), which
  the callee itself counts by. A suspending function's cold constructor
  counts each argument into its frame, so its callers lend their
  arguments.
- **Callers.** A direct call, a default argument's call and a call of a
  function value give each owned argument its count: by a move when the
  call consumes the argument (`Em::take_from`), else by one retain.
  Arguments spread from a tuple do not line up with the parameters one
  to one, so each owned parameter's values take a count wherever they
  come from. A function reference's adapter passes its arguments on
  unchanged. Code the backend generates lends what it passes: a vtable
  slot (`Helper::DynSlot`, `DynRead`), the map glue's `key_bucket_hash`
  and `eq` calls (`KeyOps`), and the boundary's JSON codecs and
  `host_provider` count a copy of each argument the target owns
  (`rt::own_arg`). So a dynamic call lends its arguments, as host stubs
  expect.
- **Moves.** The move pass moves a value's count into its one consumer
  and a local's into its last read (docs/compiler/tir-v2.md, "Move",
  rules 1 to 4). The emitter zeroes a moved place once its consumer has
  read it (`Em::settle`), so the one exit, which releases the exit set,
  releases it as nothing. Construction consumes its parts
  (`Em::count_parts`): a part that moves gives its count, any other
  takes one.
- **Borrows.** The borrow pass lends a read where the place's own count
  keeps the value alive over the read's span: an immutable read, a loop
  element of the checker's private snapshot, a reference parameter's
  read (§23.31) and a local's read over its span (§23.36). A borrowed
  read takes no count, and its drops release nothing (`Em::loans`).
- **Balanced pairs.** Pair cancellation deletes a copy and a drop of the
  same value in one straight-line run (docs/compiler/tir-v2.md, "Pair
  cancellation"). In generated code, `opt.rs` cancels a retain and its
  matching release over a short straight-line span on unchanged locals,
  after local cleanup; a freeing call, a memory operation, an
  operand-local write or a control-flow edge ends the span.
- **Counts.** `std.rt.checked` totals retains and releases; a checked
  entry exports them as `hd.retained` and `hd.released`.
- **Checked by** `crates/hd_driver/tests/std_rt/count_tests.hd`, whose
  cases pass strings through owned and borrowed parameters into a
  struct in a loop, check that the heap does not grow, and check that an
  argument used again after a call keeps its value; and the `counts`
  baselines under `crates/hd_driver/tests/cases/counts/`.

### 23.26 Requirement Rows And Context Values

Issue #613 lowers requirement rows with row parameters, function values
whose row changes and `$.Context` values on the linear backend
(`emit/ctx.rs`), after crates/hd_mono/DESIGN.md §12.4.

- **A context** is a counted list buffer (§23.7) whose elements are
  16-byte entries: a key id (`i64`, the first 64 bits of the key's
  content hash) and a provider, the trait value's data and
  vtable. Each entry holds a count of its provider's data, so the
  buffer's release glue is the list glue's (`rt::ctx_elem`). The empty
  context is 0. `rt:ctx_find` looks a key id up and takes the first
  entry that has it; `rt:ctx_new(n, base)` makes a context of `n`
  entries for the caller to fill, followed by the entries of `base`,
  which take counts of their own.
- **One callable ABI.** A closure's code and a function reference's
  adapter take the environment, their parameters, then one context,
  whatever the value's row. The code looks its own row's keys up on
  entry, after a suspending body has reloaded its locals; a row that
  lists an open row keeps the context for the keys it does not name. A
  caller passes a context of the value's row, built from its providers
  in scope, so row subsumption (`fn $ A` to `fn $ A + B`) is no
  instruction. An empty row passes 0, and a row that is only the
  caller's open row passes the caller's context unchanged. The iterator
  step codes take the context too and ignore it.
- **Row parameters.** An instance of an item with row parameters is
  compiled once with each row parameter the open row (`hd_mono::rowless`).
  It takes one provider per key its row lists itself, then the context
  of its row parameters' keys. A key no provider in scope covers is read
  from that context. A caller builds the context from the keys of its
  row arguments, in front of its own context when an argument lists its
  open row, with the keys of its open `$.with` scopes in front too, so
  the nearer provider wins (`req.with.nearest.row-parameter`).
- **Counts.** A caller releases a context it built once the call has
  returned. A callee borrows the context, as it borrows its providers;
  a suspending callee's frame takes a count of it, as of its providers,
  since the call's scope may end first.
- **Context values.** `$.Context[$ R]` is one counted address of a
  context whose entries are in key order (`req.context.order`).
  `ContextNew` builds it from the last pair that binds each key, a trait
  value or an entry of a spread context. A spread in `$.with` reads each
  entry into provider locals, borrowed from the value.
- **Dynamic calls.** A vtable slot of a method whose row lists a key, or
  that has row parameters, takes one context last: the caller cannot know
  the implementation's row, which may list fewer keys. The slot looks up
  the implementation's own keys and passes the context on when the
  target has row parameters. A method with row parameters of its own
  has an erased body like any generic method (§23.27): a call at its open
  row is an open instruction.

### 23.27 Erased Generic Methods

Issue #615 lowers generic methods called through a trait value on the
linear backend (`emit/erased.rs`), with the erased ABI
(crates/hd_mono/DESIGN.md §13.5.1): one erased body per implementation,
a witness per call's type arguments, and one thunk per open instruction
at the caller's types. Collection builds the
same erased bodies, thunks and witness fields.

- **Open values.** In an erased body, a value of an open type (one that
  mentions a method type parameter) is one `i32`, a counted address that
  owns one count, as a trait value's data is (§23.19): the caller's value
  itself when its layout is one counted address (a list, a map, a data
  value, a reference), 0 when it has no Wasm values, and otherwise a new
  counted box of its layout, whose descriptor releases its fields. So the
  erased body lays out, moves, stores and counts every open value alike,
  and its locals, cells and suspension frames need nothing new. A copy of
  an open collection is no `ListCopy` in the erased body; the thunks that
  know the type copy it.
- **The slot.** A generic method's vtable slot takes the receiver, the
  arguments with open ones as open values, the witness address, then the
  context when the method has a row (§23.26). A caller boxes each open
  argument into an open value it releases after the call, passes the
  witness of the call's type arguments, and unboxes an open result,
  which arrives with its count. An awaited suspending method's poll
  returns the open value, which the await unboxes once it is Ready.
- **Witnesses** are static data, as vtables are (§23.8): one word per
  erased body of the method's family, in link's field order, holding the
  address of that body's thunk table, one function-table slot per open
  instruction. The erased body reads its own table from its field once on
  entry, after a resumed body has reloaded its locals, since the witness
  is a parameter and stays in the frame.
- **Open instructions.** Instruction `k` loads slot `k` of the table and
  makes one `call_indirect`: its operands and captures, each with a count
  the thunk owns (a shared capture as its cell, counted once more), the
  providers of the keys the body's row lists, and, with row parameters, a
  context of every other key in scope in front of the open one. A tag
  switch outlines only its tag read. The thunk is the instruction emitted
  at the caller's types: it unboxes its open operands, taking counts of
  the fields and releasing the boxes, emits the instruction, boxes an
  open result with its own count and releases what it holds at its exit.
- **Cells.** A local that closures share in a generic method of a trait
  or an implementation keeps its erased view in every instance:
  the erased body's thunks build the closures, and they are the
  concrete instances' own. So a concrete instance's cell holds an open
  value: a read unboxes it, a write boxes the new value.
- **References.** A reference whose target is open is the address of an
  open value. When the concrete target is one counted address, its place
  holds the same word, so the reference crosses as it is. Any other
  reference crosses as a counted view of its original place (issue
  #619), still one word, so a reference keeps one ABI on both sides. A
  view is a 20-byte object whose descriptor is `rt:ref_view_drop`: the
  source reference it retains (an address, or another view), a getter
  slot that answers the value as an owned erased box, a setter slot that
  takes one, a field offset, and whether the source is an erased word.
  A field reference into a view (`&mut alias.text`) is a view whose
  getter and setter read and write the whole parent through the parent
  view. Every concrete read and write through a reference is emitted
  inline: when std's `block_of` finds the address is the payload of a
  block whose descriptor is the view's, the access goes through the
  getter or setter and unboxes or boxes the value; otherwise it loads or
  stores the leaves at the address as before. Inline, a value of any
  width stays in locals and never crosses a function's parameters or
  results. Since a view forwards each access, aliases see later writes
  from either side, after the call has returned and from a cold call.
  Identity (`is`) follows views to their source: an address plus the
  field offset, or for an erased word the pair of the word and the
  offset, since a concrete offset added to a one-word cell could name an
  unrelated object. A synchronous call releases the views it made for
  its arguments when it returns; a result, a capture, a field or a cold
  call's argument keeps its view counted like any other value.
- **Kept suspensions.** A plain call of a suspending generic method
  through a trait value keeps the erased frame, whose poll answers an
  open value, as a `Suspend[T]` that is polled where only `T` is known:
  by `!`, `race!`, `block_on` or a callee. So unless a `T` value is its
  open value, the caller wraps it in a frame of its own
  (`Helper::OpenFrame`, issue #617): `cancel!`'s cleanup frame with a
  poll that polls the child and, once it is Ready, unboxes its result
  with counts of its own and releases the box; a reference result
  becomes a view of the open word instead. Its cancel cancels the
  child and unwinds while the child does, as the cleanup frame's does,
  and its glue releases the child.
- **Rows.** A method's own row parameters are open too, so a method with
  only row parameters is erased the same way, and a call at its open row
  is a thunk at the caller's row. A dynamic call reads the keys of its
  rows from its arguments as the instance has them, so a thunk's row,
  which is the caller's own, lists its keys.
- **Checked by** the five conformance cases of the issue, in normal and
  checked mode, whose count audit covers every box and cell, and by
  `erased-method-boxed-reference-argument` and
  `dyn-generic-suspension-kept` (issue #617), and the seven
  `erased-reference-*` cases (issue #619).

### 23.28 The Cycle Collector

The backend reclaims reference cycles by synchronous trial deletion
(Bacon and Rajan, ECOOP 2001), as
docs/future-work/REFCOUNT_RUNTIME.md §7 plans. The collector is hd in
`std.rt.cycle` (`lib/std/rt/cycle.hd`); the backend adds what only it can
build (`rt.rs`'s `Helper::Collect`, `CycleTrace`, `CycleRelease`,
`CycleFree` and `Opaque`, and `cycle.rs`'s per-type analysis).

- **What takes part.** An object whose type can be on a cycle, which
  the backend decides when it emits the allocation (`cycle.rs`,
  issue #642) and records as bit 31 (`CYCLIC`) of the descriptor word
  (§23.4). Strings and other objects without glue hold nothing and never
  take part. A frame is tagged (`Helper::Opaque`, from
  `task.rs`'s `new_zeroed`) so the collector checks its task flags. While
  `ACTIVE` or `RUNNING` is set, its saved slots may be stale: what it
  holds counts as held from outside, and its candidate stays buffered
  until an idle collection. An idle frame participates like any other object.
- **The collector word** sits in the last 4 bytes of the block; the
  allocator supplies its size. `rt:new` asks for 4 more
  bytes when the descriptor has `CYCLIC` and has std's `track` zero the
  word; any other object has no collector word. Its low 2 bits are the
  color (black, gray, white) and bit 2 marks a frame. Only a collection
  reads it: no release or free needs the block's size.
- **Candidates.** Std's `release` (`std.rt.alloc`'s and
  `std.rt.checked`'s) buffers a `CYCLIC` object when it leaves a count
  above zero (`suspect`). Bit 30 of the descriptor word (`BUFFERED`) says
  whether the object is in the buffer, which makes it purple outside a
  collection, so `suspect` returns at once for one already there. The
  release of any other object is the decrement alone. A retain does not
  blacken a purple object: a needless candidate costs a walk, not
  correctness.
- **A candidate's last count** (#670). Its last release runs its glue at
  once, as any object's does, but the collector frees its block: std's
  `free` asks `keep`, which leaves a `BUFFERED` block in the buffer with a
  count of 0, and the next collection frees it before it marks anything
  (Bacon and Rajan's release of a buffered object). The buffer is
  therefore a plain list of addresses that only a collection removes
  from, and no object records its place in it. Before #670 the last
  release found the entry through the collector word and cleared it,
  reading the block's size from the page table twice on the way in and
  twice on the way out; binary-trees makes each node a candidate and then
  frees it, so this was a sixth of its run. `free` takes counted objects
  only (`BUFFERED` is a header bit); the wake table, a raw block, goes
  to the free without the collector test.
- **Tracing is the release glue.** `CycleTrace` runs the descriptor's
  glue on the payload with the `Tracing` global set, and `rt:release_ref`
  hands each reference to the collector's `visit` instead of counting it
  while it is set. So no type needs trace glue of its own: every glue
  releases its references through `rt:release_ref` and does nothing else.
  Outside a collection a release reads the global and never calls
  `visit`. Frame glue walks the saved ownership slots:
  moved and dead places are zeroed before `SaveAll`, and each `all!`
  result is guarded by its done bit. `EXITED` skips released body slots;
  providers and the child retain their separate ownership. The suspension
  plan selects the resume state; these saved zeros encode that state's
  live ownership, so tracing and final release cannot disagree.
- **Running frames.** `task::guard_frame` sets `RUNNING` around body,
  helper poll and helper cancel execution, restoring its previous bit on
  every return. Direct resumed calls go straight to `Part::Body`, so the
  body itself is guarded, including a fresh body's materialized frame.
  This bit is separate from `ACTIVE`, preserving the suspension protocol's
  reentrancy checks. A collection never runs user code and therefore cannot
  change a frame's running state during its three walks.
- **A collection** (`collect(trace, release, free)`, called with three
  function values whose code is `CycleTrace`, `CycleRelease` and
  `CycleFree`) marks gray from each purple candidate, taking one count off
  each object a gray one references; scans, so a gray object with a count
  left, which something outside holds, and all it reaches turn black and
  get their counts back, while the rest turn white; then walks the white
  objects from the candidates. Each walk keeps an explicit work list, so
  a long ring needs no deep stack. The white objects are freed only once
  the walk is done, and then a white object's references to objects that
  do not take part are released as usual, which may free more and make
  new candidates for the next collection.
- **When.** `rt:new` collects first when std's `due` says the buffer holds
  at least the threshold's live candidates (those with a count): 10,000 at first, halved back
  toward 10,000 after a collection that frees at least half of what it
  marked gray, doubled after one that frees less (REFCOUNT_RUNTIME.md
  §7.4). An allocation is never reached from release glue, so a
  collection never interrupts one. The entry's poll collects whenever it
  answers (`Helper::CollectPoll`): at idle when the answer is Pending, and
  at the end. Checked mode's poll collects after the teardown releases the
  bindings and before the leak audit, so a handle leaked inside a cycle
  is still reported. `due` also answers when at least 64 candidates have
  no count left and they are at least half the buffer; that collection
  only frees them (a sweep). A sweep walks the buffer once, so waiting
  for as many dead candidates as live ones makes each cost a bounded
  share, and the blocks waiting for it number no more than the live
  candidates, or 64.
- **Memory.** The buffer grows with the live candidates and the dead
  ones a sweep waits for, at most as many again. The
  buffer, the work list and the lists of white and released objects are
  blocks of `std.rt.alloc`, which checked mode does not count as live.
  `count_tests.hd`'s heap bound goes from 4 pages to 12: the buffer's
  size classes and runs.
- **Checked by** both sweeps: `linear-checked` passes all 1,480
  `LINEAR_PASSES.tsv` cases with counts audited, including the eight
  cycles of §23.24 (`deep-equal-rings`, `deep-equal-visits-shared`,
  `local-closure-calls-own-name`, `local-function-recurses`,
  `named-local-functions`, `named-local-suspending-function`,
  `recursive-local-closure`, `reference-cycles-are-ordinary-data`). With
  the threshold fixed at one, so that every allocation after a release
  collects, the `linear-checked` sweep passes too. The conformance case
  `dropped-cycles-are-reclaimed` drops 50,000 rings and 20,000 recursive
  local closures; `crates/hd_driver/tests/std_rt/cycle_tests.hd` drops
  200,000 of each and checks the heap: the rings take 23 pages with the
  collector and 430 without.
- **Immutable recursive closures (#713).** The std runtime tests also
  drop 200,000 recursive `:=` functions, with and without value captures,
  and check that `pending()` is unchanged. Their environments contain no
  self-binding cell; no collection is needed to reclaim them (§23.19).
- **Frame cycles (#637).** `dropped-suspension-cycles` drops cold frames
  retaining arguments and closure captures, then resumes a nested direct
  call through a pending host write and collects while saved strings are
  stale. Checked mode audits the final heap on both engines.
  `std_rt/cycle_tests.hd` drops 400,000 argument/capture frame cycles and
  checks reuse in the normal allocator; checked mode quarantines freed
  blocks and checks their counts instead.
- **Cycle capability** (issue #642; REFCOUNT_RUNTIME.md §7.1).
  An object can be on a cycle only if what it holds reaches, through
  counted references, a value that can hold the object again.
  `cycle::object` (a list's header and buffers, a map's table, a data
  object, a self-recursive enum's cell) and `cycle::holding` (a closure's
  environment, a local's cell, a `dyn` or open value's box, an iterator's
  environment, which only open values hold) walk the types that the
  object's contents hold, through tuples, options, structs, value enums,
  list and map elements, data fields and boxed variants. The object takes
  part when the walk meets a reference (`&T` or `&mut T`: it may be a view
  into any object with a place of its target's type, so the type graph
  cannot say what it reaches), an open value (a function value, a trait
  value, a context, a suspension, an erased body's open type, or a type
  the walk does not know). A path through values alone is acyclic, even
  when it revisits its own type. Construction holds older values; a write
  first copies a shared collection buffer or value cell. That copy keeps
  the old object's edges unchanged, so the write cannot close a cycle
  through value edges. The visited-type set therefore ends a repeated
  value path without making it capable.
  A closure literal's environment is decided from its actual captures:
  a shared captured cell or a captured reference keeps it capable; copied
  or moved values use the same value walk. A type with an ordinary `fn`
  field remains open and capable. No callee or whole-program summary is
  involved. A suspending body's frame, a child's or a race's frame
  with glue (#637), a reference view and a boxed reference value
  (`rt:ref_view_drop`, `ref_box`, whose types `rt.rs` does not know)
  always take part. A host call's leaf frame takes part as `holding` says
  of its arguments' types. A list copy (`rt:list_copy`) takes part as its
  original does, read from the original's descriptor at run time.
- **Coverage.** `crates/hd_driver/tests/std_rt/cycle_tests.hd` checks that
  sharing and dropping value-only lists and closures creates no cycle
  candidate, while a dropped ring of references does.
- **No collector when nothing takes part** (issue #649). The driver roots
  both sets of the runtime's frees and releases: with collector hooks
  (`free`, `release`, `release_known`) and without (`std.rt.alloc`'s
  `free_block`, `release_acyclic`, `release_known_acyclic`;
  `std.rt.checked`'s `free_acyclic`, `release_acyclic`,
  `release_known_acyclic`). `Module::finish` looks through what the exports
  reach for a descriptor that `rt::push_desc` pushed with `CYCLIC`. When
  there is none, it sets `Module::acyclic`, points the runtime at the
  releases without hooks, and builds every helper again. `rt:new` then
  asks for no collector word and never collects, `rt:release_ref` has no
  `Tracing` check, and neither the entry's poll nor checked mode's
  shutdown collects. The init of `std.rt.cycle` is skipped too, unless
  code still calls the collector directly, as std's own tests do.
  `CYCLE_FNS` are still rooted, since
  collection runs before any allocation is emitted; only the link drops
  them. `tests/all/cycle_linking.rs` checks that a program of lists and
  closures links no `std.rt.cycle` and calls `release_acyclic`, while a
  ring of references links the collector.

### 23.29 Wide Signatures And The Shadow Stack

Issue #629. A value's Wasm values are its flattened fields (§23.3), so a
function that takes or returns a struct of a thousand fields would have a
thousand parameters or results. V8 refuses a function type of more than
1000 parameters or 1000 results at instantiation, and wasmparser, which
wasmtime validates with, has the same limits. `wide.rs` lowers such
signatures before encoding, as clang's and Rust's wasm32 `byval` and
`sret` do:

- **The bound** is 64 Wasm values (`wide::WIDTH`). Any bound under 1000
  is correct; 64 keeps every ordinary signature, a struct of some thirty
  strings included, as Wasm values, and moves only rare wide ones. The
  parameters and the results are bounded apart.
- **Wide parameters** become one `i32`: the address of a frame that holds
  each argument in an 8-byte slot, in order. The callee loads every slot
  into a local before anything else runs, so a suspending callee keeps
  its arguments in its own frame (§23.21) from then on.
- **Wide results** become none, and the function takes the frame's
  address as its last parameter. It stores each result in the frame's
  slots just before each `return` and at its end. Arguments and results
  share the slots, since the callee reads every argument before it
  writes a result.
- **The caller** reserves the frame below the stack pointer
  (`Global::Stack`), stores the arguments, calls, loads the results and
  moves the pointer back. A function under the bound never touches the
  stack pointer.
- **Uniform.** The pass rewrites every function, `call` and
  `call_indirect` of a wide signature by the signature alone, so
  instances, closures, adapters, vtable slots, erased thunks, suspension
  parts and runtime helpers that take a whole value (retain and release
  glue, codecs, copies) agree without knowing of it. A wide import or
  export is a compiler bug: the host calls those by their Wasm signature.
- **Counts.** The moves copy bits: a value's counts move through the
  frame as through Wasm values, and the frame is never allocated or
  counted, so `linear-checked` audits the same counts.
- **The stack** is memory 2, 16 pages that the module declares only when
  some signature or body needs it. It grows down from its end; a frame below
  address 0 wraps to an address past the end, so a too-deep chain of wide
  calls traps on its first store. A trap skips its frames' release,
  which only shrinks the room a later entry in the same instance has.
- **Wide bodies** (#640) put their locals in a zeroed shadow frame after
  signature and call lowering. An emitted aggregate wider than 64 values,
  or a wide signature, marks the body. Narrow bodies keep Wasm locals.
  The rewrite includes scratch storage, so copies cannot recreate the
  local-count limit. Four typed scratch locals, a branch selector and
  the frame address suffice; unused declarations are removed afterward.
- **Exits** restore the frame on fallthrough, explicit return and a branch
  to the function label, including conditional and table branches. Calls
  reserve their argument/result frames below this frame. A resumed
  suspension restores its saved values into its fresh shadow frame.
- **Projection** reads only a sole field consumer at the original
  plain local read (`Read [$l]`) or whole `Read [*r]` position; a read through a reference
  with a field path, `Read [*r .f]`, loads only that field by itself,
  and a match over it borrows it as such a consumer does. It retains
  the selected field before the original owner may die, without copying
  or retaining other fields.
  A boxed reference view reads that field from the getter's box and
  releases the box, without materializing the parent in value locals.

### 23.30 Known Block Starts

Issue #643 avoids finding an object's block when its representation
already supplies the payload start, eight bytes after its header.
Interior pointers keep the page-table path, including string slices and
physical or erased references without a block-start proof.

- **Known counts.** The runtime's `retain_known` and `release_known`
  entries use `address - 8`. Null and static zero-count objects stay
  uncounted. Final releases use the existing allocation-free release
  worklist. Both release paths visit only while `Tracing` is set, buffer
  only `CYCLIC` objects; a `BUFFERED` block's free waits for the collector. Collection
  header copies inherit their original cycle eligibility. The emitter uses
  this path for cells, nominal heap objects, closure environments,
  suspension and collection headers, and collection buffers. Untyped
  aggregate walks conservatively keep general counts.
- **Checked proofs.** Checked mode verifies that every claimed payload
  start agrees with `block_of`, then performs the existing audited count
  update. An incorrect proof fails even when the ordinary count would
  have found the enclosing block. `RefLayout` carries the same proof into
  generated reads and writes through references.
- **Class tables.** Two 20-entry memory tables give each class's size
  and `ceil(2^32 / size)`. The allocator initializes them in its own
  metadata prefix, beside the class heads. Growing the page table copies
  that prefix, and neither table needs a counted allocation. A small-page
  offset is below `2^16`; multiplying it by the reciprocal and shifting
  right 32 gives the exact quotient for every class. No variable division
  remains in that lookup. The allocator fixture checks every offset in
  all 20 classes against integer division.
- **Physical references.** After reachability, a module with no
  `RefView` constructor cannot contain a representation view. Its
  `ref_is_view` calls become false, removing their page-table queries.
  Field projection cannot originate a view without a parent view. A
  module that can construct erased representation views keeps the full
  discriminator; the #619 erased-reference fixtures audit that path.
- **Inline fast paths** (issue #644, `opt::inline_counts`), for the
  wasmtime target only: its pipelines carry the link pass
  `inline_counts` (`crates/hd_opt/DESIGN.md` §3.1), which sets
  `Input::inline_counts`. V8 inlines the helpers itself and ran the inline
  form slower, so Node and browser builds keep the calls. After
  `Module::finish` has cancelled balanced pairs, every `RetainKnown` and
  `ReleaseKnown` call becomes inline code over `address - 8`. A retain
  adds one unless the address is null or the count is 0 (static data). A
  release takes one off when the count is above 1, the object is not
  `CYCLIC` and `Tracing` is clear. Otherwise it calls `ReleaseKnown`,
  which handles the last count, a cycle-capable object's `suspect`, and
  `visit` while the collector traces. That call is also made for a count
  of 0 while tracing, since a gray object's count can reach 0 in a mark.
  An acyclic module (§23.28) drops both collector tests. Interior and
  erased addresses keep their calls, and checked mode keeps every count a
  call so that its runtime audits them. With inlining, hello (release)
  grows by 212 bytes, from 7,183 to 7,395 for `println(42)`. On wasmtime, at
  fce872a45: nbody 4,861 ms to 4,411; closures 6,159 to 4,512; json
  35,299 to 33,888; binary-trees 83,770 to 78,729. On V8: binary-trees
  29,215 to 24,206 and json 18,514 to 18,063, but nbody (about 2,920 to
  3,030) and closures (about 1,590 to 1,690) were 4 to 6 per cent slower
  in repeated runs, hence the wasmtime-only rule.

### 23.31 Parameter And Projection Loans

The borrow pass lends a read through a `&T` or `&mut T` parameter from
the caller's ownership: the caller keeps the referent alive for the
call, so a read with no call that may write in its span takes no count
(docs/compiler/tir-v2.md, "Borrow", rule 3). The hoisting pass keeps a
changed list's uniqueness check out of its loop. What stays the
backend's is below.

- **Views keep temporaries.** A borrowed read through a reference takes
  no count on a physical place. An erased view's getter makes a fresh
  owned value instead, so that branch sets an ownership flag
  (`Em::view_loans`, set up by `Em::prepare_own_loans`). The read's
  drop, the exit and the next iteration release the temporary only when
  the flag is set. A projected view still owns its newly allocated
  forwarding reference. Open thunks keep their ordinary counts.
- **Read the selected field.** A place reads only its field:
  `Read [*$0 .x]` and `Read [*$0 [%i] .x]` load that field's leaves
  (`Em::read_part`). A boxed view loads the field from the getter's box,
  retains it and releases the box. An unboxed view keeps its scalar
  fallback.
- **Uniqueness before a loop.** A hoisted `IsUnique` runs once before
  its loop, under the loop's entry test, so an empty loop copies
  nothing (`Em::hoisted_uniques`; §23.16, "Uniqueness and pins").
- **Loop caches.** The hoisting pass lists each loop's stable list
  places in `Body::stable_lists`: a local's list, or the list a
  reference local refers to, that nothing in the loop replaces. One
  with no counted leaf in its elements keeps its header, buffer and
  length in locals for the loop (`Em::own_list_caches`), and its element
  reads and writes and its length use them. They are read only where no
  reference view is reachable (`RefViewsAbsent`), a fact the link folds;
  a view module keeps its ordinary reads and writes. The loop's exit
  drops the cache. Bounds checks still use the cached length.
- **Checked by** fixtures covering invalidating alias calls, erased
  getter temporaries, early returns, initial copy-on-write sharing and
  snapshots created by callbacks between writes.

### 23.32 Element Field Stores And Constant Branches

Issue #655 makes `bodies[i].vx = e`, on a list of value structs, one
store, and removes the scaffolding that a folded view test left.

- **One field store.** A write into a field path of a list element
  (`xs[i].p.x = v`) is one `Store [%xs [%i] .p .x] v` (TIR v2 stage 1;
  #655 first made it an intrinsic). It used to read the element out,
  rebuild each struct on the path and store the whole element back. The emitter (`elem_store`) checks the index, after the
  passes' `IsUnique` made the buffer unique, and stores only the field's
  values at their offsets. A field with counted references releases its
  old value after the new one is counted. Reads already load only the
  projected field (§23.31). A field of a local, a top-level binding, a
  data object or a referenced value is stored in place the same way
  (`emit/refs.rs`).
- **Constant branches** (`opt::fold_constant_branches`, after `opt::constants`).
  An `if` whose condition is an `i32` constant becomes a `block` of the
  arm that runs, which keeps every branch depth inside it. An `if` with
  no `else` and no result that never runs goes. An `i32` local whose every
  write stores 0 is read as 0. In a module with no reference views,
  `ref_is_view` is already `i32.const 0`, so the view's `call_indirect`
  arm and its release flag fold away at every read through a reference.
- **Cost.** nbody's `advance` (release, V8 target) goes from 63
  `f64.store`s and 146 `f64.load`s to 9 and 83, from 154 calls to 47, and
  from 3,211 to 1,508 lines of text. The module shrinks from 20,446 bytes
  to 14,955. hd on V8 (ms, two alternating runs of three): nbody 2,904 and
  2,900 to 1,303 and 1,318; spectral-norm 409 and 414 to 321 and 336;
  fannkuch-redux 3,412 and 3,492 to 3,431 and 3,475; sieve 558 and 573 to
  549 and 549.
- **Not done.** Each access still loads the list's header, buffer and
  length, and checks the index. A cache of the buffer and length in
  locals, with a flag that any possibly-changing instruction clears, ran
  slower on V8 (nbody 1,300 to 1,410, sieve 540 to 650), so it was
  dropped. Removing the bounds check for `0..n` over the length needs a
  proof that `n` is the list's length and the list cannot shrink.
- **Checked by** `runtime/valid/list-element-places.hd`, which writes a
  string field and a nested struct field of an element, and
  `runtime/valid/list-reads-see-every-change.hd`. The second reads lists
  after pushes, element writes, calls that change the list through a
  reference, and copies.

### 23.33 Instruction Origins For Development Views

Issue #661 keeps source attribution outside the instruction stream.
`build_with_origins` returns the module bytes and an off-band function map;
ordinary `build` keeps returning only the same bytes. Mapping adds no
instruction, anchor or Wasm section. The existing call-only `hd.lines`
anchors and panic reporting keep their behavior.

- **Origins.** An emitted body's parallel table names its TIR instruction
  at each instruction index. The item path identifies the source body;
  the driver or harness resolves its syntax node to a source span. A
  source-less generated helper has no item or origin. Frame setup and
  function exit use the body's scope origin, except the exit's releases:
  each uses where its place began, so a statement's excerpt shows the
  exit releases of what it bound (#673). A TIR local's release uses its
  first positioned binding (a `Store` into the whole local), an
  instruction value's its own
  instruction; a parameter's stays at the body's. An early exit's
  releases use the scope they end, including an exit through several
  scopes. Last-use cleanup follows the statement containing that last
  use instead.
- **Rewrites.** Unchanged and moved instructions keep their own origins.
  A merge takes the first available origin among the instructions it
  combines. An expansion copies the replaced instruction's origin to
  every resulting instruction. Deleted code has no resulting entries.
  Constant propagation from another function keeps the read's use site.
  Each optimizer, wide-value and suspension rewrite maintains the table
  with its instruction changes; the metadata never affects matching.
- **Final offsets.** A mapped function records its pre-optimization origin
  table and each final encoded instruction's body-relative byte offset,
  including local declarations in that offset and the implicit final
  `end`. Zero-width call anchors have no encoded instruction entry. Raw
  compiler-generated helper bodies have no instruction-origin table.
  Mapping does not require dense anchors and cannot interrupt a peephole.
- **Names (#662).** A mapped function also names what its code refers to
  by index, for the stage fixtures' `wat` view. Each Wasm local is named
  by its TIR local (`$N`; `$N.PATH` for each value of a local of several,
  where `layout::value_paths` gives a struct field by name, `$N.pos.x`,
  and any other value by its index, `$N.1`), by the role of a parameter that is no TIR local (`frame`,
  `env`, `ctx`, `witness`, `providerN`, `providerN.vtable`, and `args`
  for a wide signature's frame address), or not at all for a temporary.
  Local renumbering (rule 11 of `opt.rs`) and wide-signature lowering
  carry the names to the final indices. Each static data address a body
  pushes is listed with what it holds: bytes, a vtable's slot functions,
  or a name (`empty-object`, `empty-buffer`, `""`, a top-level binding's
  slot). Each global the body names comes with its binding's path, or
  the runtime's name for it. An instance lists its type arguments by
  their printed names (`FunctionOrigins::type_args`). The tables change
  no byte of the module.

### 23.34 Development Fixture Retention

`Entry::Fixture` gives the linker private instance roots. Every collected
instance defined in the case module is retained, including closure bodies,
named generic instantiations and generated suspension parts, even if a
constant branch or cross-function fold removes its last caller. Their
normal dependencies remain reachable by the ordinary helper walk.

These roots are separate from exports: fixture functions keep their
declared ABI, including wide signatures lowered through the shadow stack.
No executable entry, Termination conversion, host providers or callable
exports are synthesized. Ordinary builds still prune from their exports.
The driver exposes this only through the development `Goal::Fixture`
API, with sorted notes for generic definitions lacking an instantiation;
there is no CLI flag or language rule (#663).

### 23.35 Layout And Count Development Views

The fixture driver exposes concrete struct declarations through
`views::inspect_layout`. It uses emission's `mem`, field parts, counted
walk and cycle analysis. Field names come from `ProgramEnv::data_field_names`;
counted-leaf paths and count labels reuse #673's `layout::value_paths`. Field paths are source names; nested offsets and
counted-leaf offsets are bytes from the value's base. Conditional leaves
mark enum/option payloads; the cycle bit uses `cycle::object` for that type.
Generic declarations without a concrete declared type are not layout roots.

Mapped functions also expose final count/free operations. Typed counting
sequences carry a type label in their source-map metadata. Internally,
extra origin tokens follow the body's real TIR instruction range and map
back to that instruction and label. Existing rewrites copy those opaque
tokens, so the view needs no new optimizer instruction or Wasm anchor.
Public pre-optimization and final maps always resolve back to real TIR
instructions; the tokens never enter Wasm or source-span consumers.

Only surviving count/free calls are reported. An inlined count fast path
and its fallback share one operation label, rather than appearing twice.
Direct runtime calls without a typed counting sequence use a runtime
object label (`block` or `counted reference`). Labels describe the counted
value's type, not dynamic invocation counts or whether a nullable/static
pointer takes the helper's no-op branch.

`hd_testkit::stages` adds exact `layout` and `counts` views. Layout filters
source declarations; counts filters final operations with #661's source
spans, excluding std/helper bodies and context outside the subject. Topic
files in `hd_driver/tests/stages` cover padding, nested/List fields,
conditional counted leaves, cycle capability, parameters, returns, loops,
field writes. A backend encoding regression covers direct frees. The WAT
view remains a separate topic/tool (#664).

### 23.36 Span Loans

The borrow pass lends a local's read for the span in which its value is
used (docs/compiler/tir-v2.md, "Borrow", rules 1 to 4). The read takes
no count and its last use releases nothing; the local's own count keeps
the value alive.

- **The span.** The statements from the read to its value's last use,
  with every block they run.
- **The rule.** The read borrows when nothing in the span can drop or
  replace the local:
  1. Every use lends: no use needs a count of its own (an owned
     argument, a store, a return).
  2. No `Store`, `Drop`, `move` read, `IsUnique` or mutable `Ref` of an
     overlapping place stands in the span.
  3. No `Ref` starts from the local, no closure captures it, and it
     lives in no cell. Then no call, global or write through a
     reference reaches it; only the body's own instructions change it.
     A call that receives `&mut l` therefore keeps the read's count.
  4. The span holds no suspension point.
- **Why it is safe.** A jump out of the span leaves the read's remaining
  uses behind. A change made in place through the value, a list push or
  element store, keeps the list's header, which is the whole value read
  (§23.7); copy-on-write counts the buffer, not the header, so a missing
  header count changes no copy decision.
- **Emission.** A borrowed read takes no count (`Em::loans`), and a
  local the passes store a borrow into holds none (`Em::loan_locals`):
  its stores count nothing and release nothing.
- **Fixtures.** `hd_driver/tests/cases/counts/`: the element read and
  write of a local list, a method call on a local reference, a
  reference argument and a push to a list field take no count.
  `local-replaced-inside-the-span`, `local-moved-inside-the-span` and
  `local-lent-by-mut-reference` each keep the count, by rules 2, 2 and 3.

### 23.37 Range Bounds And Exact List Indices

Issue #711 removes the second comparison when a range advances: the
checker reuses the iteration's less-than result. A literal exclusive
range uses that one exit test and increments directly, since an entered
iteration is strictly below the endpoint. Dynamic and inclusive ranges
keep their endpoint handling, including the largest integer value.
Advancing still precedes the user body, preserving `continue`.

The list emitter recognizes an exact `usize` induction variable from
zero to an observed list length. Its local definition and increment must
match the loop test, with no cell or captured assignment; the index read
must precede the increment and dominate the access. Scalar aliases and
fields of fresh range values may carry the bound. It applies the existing
within-body alias/resize proof both to the loop and to the statements
between the length observation and loop entry. Thus a pop in that
prelude invalidates the fact too. Unknown calls and possibly aliased
mutations keep the normal check.

Proven element read (`Read [%xs [%i]]`) instruction IDs live only in emitter state; TIR tags,
operands and arities do not change. Their physical reads calculate the
element address directly. Typed views use the ordinary checked path,
selected by the same RefViewsAbsent guard as cached list projections;
the linker folds it in programs without views. Other index expressions,
wide indices, offset indices and unproved paths retain their checks.
Only element reads in list_index use this fact; element writes keep
their existing checks and lowering for the parallel TIR rewrite.

The focused WAT cases show the eliminated loop comparison and exact
index check, and the retained checks for a pop in the body, a pop in the
prelude, an aliased pop and an offset index. Checked runtime cases cover
those panics, an addressable induction variable, typed views, empty and
nested loops, continue and full-width inclusive endpoints.
