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

| Piece | Where | Job (REFCOUNT_RUNTIME.md §10.2) |
| --- | --- | --- |
| this record | `crates/hd_wasm_linear/DESIGN.md` §23 | 1 |
| the backend: layouts, emission from TIR, the module's link, the runtime pieces it generates | the crate `crates/hd_wasm_linear` | 2, 3 and on |
| `layout.rs` | a type's linear layout: its Wasm values, its size and alignment in memory, its field offsets (§23.3) | 3, 6 |
| `emit.rs` | one instance's TIR body to one Wasm function | 3 and on |
| `rt.rs` | generated runtime pieces: the entry wrapper, panic stubs, the exchange-buffer copies, the host provider stubs, object headers and release glue, and the bump allocation that host frames still use; the wake table later | 3 and on |
| `lib.rs` | `build`: the whole program's module from collection's output, with no per-instance code cache yet | 2, 3 |
| code generation | `hd_wasm_linear` is the sole code generator; commands have no backend-selection flag | 2 |
| conformance | both tiers build every case with the linear backend; `HD_CONFORMANCE_CHECKED=1` builds them in checked mode | 2 |
| memory intrinsics: load, store, grow, copy, fill | the privileged std module `std.rt.memory`: plain hd declarations whose bodies are `@intrinsic` | 4 |
| the allocator | hd in `std.rt.alloc`, over `std.rt.memory` | 5 |
| counts | `retain` and `release` in `std.rt.alloc`, called by emitted code, with release glue that `rt.rs` generates per layout | 8 |
| lists | `emit/list.rs` for the list kernels, and `rt.rs` for the header, the buffer, its release glue and the change that makes a buffer a list's own (§23.17) | 9 |
| maps | `emit/map.rs` for the map kernels, and `map.rs` for the table: its probe, put, growth, release glue and the change that makes a table a map's own (§23.18) | 10 |
| closures and `dyn` | `emit/closure.rs` for environments, cells and calls of function values, `emit/dyn_value.rs` for trait values and their vtables, `emit/iter.rs` for collection iterators (§23.19) | 11 |
| references, cleanup scopes, releases at last use | `emit/refs.rs` for references as addresses, `emit/live.rs` for each value's and local's last use, `emit/scope.rs` for the `defer` exit ladder, `emit/list.rs` for element references' pins (§23.20) | 12 |
| host codecs | `boundary.rs`: each boundary type's encoder and decoder over the exchange buffer, and the host method slots that call them (§23.23) | 14 |
| checked mode | `std.rt.checked`, the counts and the allocator with their checks, and `rt.rs`'s audit at an entry's end, with checked allocation/count auditing enabled (§23.24) | 16 |
| iterative release | hd in a further `std.rt` module, called by emitted code | 19 |
| the cycle collector | hd in `std.rt.cycle`, called by emitted code, which runs each object's release glue as its trace (§23.28) | 21 |

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
| 4 | 4 | the descriptor word: bits 0 to 29 the release glue's function-table slot plus one (0 for none); bit 30 set while the object is a cycle candidate, bit 31 when its type is cycle-capable (§23.28) |
| 8 | the payload's size | the payload: the fields, or a buffer's header and elements |
| the block's last 4 bytes | 4 | the collector word, for a cycle-capable type only: the color, the frame flag and the candidate-buffer index (REFCOUNT_RUNTIME.md §7.2, §23.28) |

- A reference to a whole object is the payload's address, slot + 8, so
  its fields read at constant offsets with no lookup; the header is at
  `address - 8`.
- **The type descriptor** is a static record that the descriptor number
  indexes: the release function's and the trace function's function-table
  indices (0 for none), the payload size and the slot's size class, and
  flags (cycle-capable, buffer). It is what lets the last reference be an
  interior one: when `&mut packet.status` dies last, the page table finds
  the slot and the descriptor says to release a whole `Packet`
  (REFCOUNT_RUNTIME.md §2).
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

The allocator is hd in `std.rt.alloc` (job 5), REFCOUNT_RUNTIME.md §6's
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
| 4 | 4 | the owner word: 0, or the function-table slot of the list's clone glue plus one once pinned |

| Buffer payload offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | capacity, in elements |
| 4 | 4 | length |
| 8 | capacity times the element size | the elements, inline |

Why a header, and not the buffer's address as the value: the TIR changes
a list through its value, not through the place that holds it (`push`,
an element store and `ListIndexMut` take the list as an operand), and
the checker lets places share one list value where nothing can change it
in place (`ValueCopy`, §15.2, "Collection values"). So the change must
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

### 23.10 What The Jobs Build On This

| Job | Adds |
| --- | --- |
| 2 | the switch, an empty module that runs, the linear conformance mode and its pass list |
| 3 | both memories, the static data segment, globals, the entry wrapper, integer code and panics: hello and integer programs run |
| 4 | `std.rt.memory`'s intrinsics, with spec rules and hd tests on the linear backend |
| 5 | `std.rt.alloc`: size classes, pages, the page table, large runs, with hd tests |
| 6 to 15 | values inline, strings, counts and release glue, lists, maps, closures and `dyn`, references, frames, the host codecs, each adding rows to `LINEAR_PASSES.tsv` |

### 23.11 What Job 3 Lowers

Job 3 runs hello and the integer programs. What it builds, and where it
stops short of the layout above:

- **Values in locals.** Each TIR instruction's value gets fresh Wasm
  locals of its layout, a TIR local owns the locals of its own, and a
  constant is an immediate. Wasm's structured control does the rest: a
  `Match` is one block per arm around its decision tree, whose `ToArm`
  branches out to its arm.
- **Layouts so far.** Scalars, `string` as `(address, length)`, a trait
  value as `(data, vtable)` and a suspension as its frame's address. An
  enum or a `T?` is its tag, then every variant's payload values in
  declaration order: no slot sharing yet, which job 6 adds with the
  layouts in memory. Data, tuples, lists, maps and function values are
  refused, naming their job.
- **Integer code** follows §13.15 of
  crates/hd_mono/DESIGN.md: the same checks, the same wrapping in a
  release build, the same panic messages.
- **Bump allocation.** A string that interpolation or integer text
  builds is allocated in memory 1 by the helper `rt:alloc` from the
  global `HeapTop`, 8-aligned after static data, and never freed.
  Emitted code moves to the std allocator of job 5 when its blocks are
  counted and freed (jobs 7 and 8).
- **The host's providers.** The entry passes each row key's host
  provider as data 0 and a static vtable of host stubs. A stub's cold
  call is a 20-byte frame: its poll slot, its state, the host handle and
  the text argument. `block_on` polls the frame through its slot and
  waits in `hd:rt.block` while it is pending. While only one host call
  is pending at a time, the completion the host reports is that call's;
  the wake table comes with the frames of job 13. Only host methods that
  take one string and answer tags (`Console.write_line!`) are built.
- **Not yet.** No `hd.lines` or `hd.positions` section, so a panic
  report names no frames; no `hd.wake` or `hd.cancel`; no test or
  scenario entry.

### 23.12 What Job 4 Adds

Job 4 gives std the heap: the memory intrinsics, and the test programs
that run their hd tests.

- **`std.rt.memory`.** spec/lang/10-modules.md "Memory Intrinsics": plain
  hd declarations in `lib/std/rt/memory.hd` whose bodies are
  `@intrinsic`, with the `mem_` keys of `hd_host_abi::INTRINSICS`. The
  linear backend lowers each to one instruction on memory 1: a
  little-endian load or store at any address, `memory.size`,
  `memory.grow` with its `-1` read as `.None`, `memory.copy`, which
  allows overlap, and `memory.fill`. Only std can reach one.
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
- **Bump allocation beside `grow`.** `rt:alloc` keeps the end of its own
  pages in the global `HeapEnd`. An allocation that does not fit grows
  fresh pages and starts on them, so it never writes a page that `grow`
  gave std (`module.rt.memory.grow.owner`).
- **Test programs.** The linear backend builds `hd.init.N` and
  `hd.test.N` as §15.4 describes, each case's poll an entry poll of its
  own. A test case is a suspending function; a root that only the entry
  calls, `main` or a case, is built as a plain function, since until job
  13 any suspension point in it is refused, and a body that never
  suspends ends on its first poll.
- **The hd tests.** `crates/hd_driver/tests/std_rt/` holds std's own hd
  tests of its runtime modules. The conformance runner adds each file to
  std's sources and runs its cases on the linear backend, on the engines
  of its mode (`STD_RT_TESTS`). The wasmtime test pool allows two
  memories per module for it.

### 23.13 What Job 5 Adds

- **`std.rt.alloc`.** `lib/std/rt/alloc.hd` is the allocator of §23.5 and
  §23.6 in hd over `std.rt.memory`: `alloc(size)`, `free(address)`,
  `block_of(address)`, the page-table lookup of an interior address, and
  `block_size(size)`. Its state is four module bindings: the table's
  address and capacity, how many pages the table knows, and the lowest
  page that may be free. The first `alloc` grows a page for the table and
  marks every page before it static.
- **Pages it does not own.** The heap can grow under it, through `grow`
  or the bump allocation of §23.11, so each time it grows it marks the
  pages grown since as static, and it never hands them out.
- **Runs.** A run is taken first fit from the lowest page that may be
  free, by scanning the table for free entries; a freed run's entries
  become free, so neighbors coalesce with no merge step. The scan costs
  time proportional to the pages before the first fit; a free-run index is
  a later speed-up.
- **Division.** Issue #643 replaces the class-size division with the
  reciprocal table of §23.5; see §23.30.
- **Not wired in yet.** Emitted code still allocates through `rt:alloc`,
  which never frees, until blocks are counted (jobs 7 and 8). So no
  program links `std.rt.alloc` today, and hello is unchanged.
- **Tests and throughput.** `crates/hd_driver/tests/std_rt/alloc_tests.hd`
  drives allocation, free, reuse, interior lookup, large runs and their
  coalescing, and pages another part of the program grew, on both
  engines. A loop of 2 million `alloc(32)` and `free` pairs, and 2,000
  chains of 1,000 blocks of 8 classes freed in reverse, cost about 30 ns
  per pair on Node and 30 to 40 ns on wasmtime, in the dev and the
  optimized pipelines alike, with the checked arithmetic of a test
  build. The study's Rust `sc` spends about 7 ns per binary-trees node,
  allocation, initialization, traversal and free together
  (REFCOUNT_RUNTIME.md §3.2), so hd's allocator is about 4 times slower
  for now. The table move past 16,320 pages, almost 1 GiB of heap, has no
  test: it needs a heap that large.

### 23.14 What Job 6 Adds

Job 6 lays out structs, tuples, enums and data values (§23.3), in
`layout.rs`, and lowers their instructions in `emit/value.rs`.

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
- **Data values** are the address of an object: an 8-byte header, which
  counting fills (job 8), then the payload, the fields at their offsets.
  A literal allocates it with `rt:alloc`, a field read loads, a field
  write stores, a copy-update allocates a copy, and `is` compares
  addresses. A data type without fields is one static object.
- **Self-recursive enums** are the address of a cell holding the enum's
  value; the checker refuses recursion by value today
  (`recursive-value-type`), so only an enum with a projection in a value
  position reaches it (`hd_mono::layout::recursive_enums`).
- **Shared generic instances.** Collection gives one instance to every
  unbounded argument in the
  class `REF` (A1, crates/hd_mono/DESIGN.md §13.2). That class is
  one address, which is right for data values and boxed enums. Structs,
  enums and tuples with inline layouts cannot use that address-only
  representation, so a call that would pass one to
  such an instance, or get one back, is refused
  (`layout::shares_as_address`). Collection keyed by this backend's
  layouts lifts that; it is a change to `hd_mono`, not made here.
- **Not yet.** `T?` of a data value is still an enum, its tag and the
  address, not the address with 0 as `.None`. Integer `**`, float `**`
  and `%`, and float text are still refused.

### 23.15 What Job 7 Adds

Job 7 lowers strings (§23.7) in `emit/text.rs`, over runtime helpers in
`rt.rs`.

- **Three kinds of view.** A literal's bytes are static data. A built
  string's bytes are a string buffer, `rt:str`: an 8-byte header, which
  counting fills (job 8), then the bytes; the view's address is the
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
- **Leaks.** Until counting (job 8) no buffer is freed: every built
  string lives until the program exits, as every data object does.
- **Not yet.** `char` text builds a `List[u8]` (std's `scalar_text`), and
  `StringBuilder` is a reference to lists, so both wait for lists (job 9,
  §23.17) and references (job 12). So does every `assert_equal`, whose report is
  `debug`'s, over a `&mut DebugWriter` of lists and `dyn Any`: most string
  fixtures check with it, so the job's fixture prints instead.

### 23.16 What Job 8 Adds

Job 8 gives every object its header (§23.4) and counts naively: one more
count at every copy into a place, one less at every place's end. Correct
before fast; elision is jobs 17 and 18. The counting is in
`emit/count.rs`, the glue in `rt.rs`.

- **Std's allocator, linked.** For this backend the driver roots
  `std.rt.alloc`'s `alloc`, `free`, `retain` and `release`, and runs that
  module's inits first in every init group, so its top-level bindings are
  set before any allocation. The link keeps only what the exports reach,
  so a program that never allocates or counts links none of them.
  `rt:new` takes a block for a header and a payload and sets the count to
  1. Host call frames still come from the bump pointer (`rt:alloc`), so
  they are never freed until frames are counted (job 13).
- **The header.** The count, then the descriptor word: the table slot of
  the payload's release glue plus one, or 0 when the payload holds no
  counted reference. That is the release function of §23.4's descriptor
  record without the record, since nothing reads its other fields yet;
  it becomes the record's number when tracing (job 20) or the collector
  needs one.
- **Counts through any address.** `retain` and `release` find the block
  through the page table (§23.5), so a string's view and a slice count
  their buffer, and static data, which is in no block, is never counted:
  literals, the empty string and fieldless data objects need no check.
  `release` hands back the block at its last count; `rt:release_ref` then
  calls the descriptor's glue on the payload and frees the block.
- **Which values count.** A type's walk (`layout::walk`) lists the values
  that are counted addresses: a string's address, a data value, a boxed
  enum's cell and a shared instance's reference, through structs, tuples
  and enum payloads, an enum's by its tag. Retain and release glue
  (`rt:retain`, `rt:release`) follow the walk, one function per layout;
  `rt:drop` loads an object's payload and releases its fields. A
  suspension and a trait value are not counted yet (jobs 11 and 13).
- **Places.** Each instruction's value owns one count: its store retains
  a copy, while a call's result, a new object and a new string arrive
  with theirs. A TIR local, a global and a data field retain what is
  stored and release what it held. A new object retains the fields its
  construction copied in. A parameter is borrowed from the caller, so the
  callee retains it on entry. A store inside a loop releases the value
  the place held from the last iteration, after counting the new one.
- **One exit.** A function that holds a counted value runs its body in a
  block: a return stores its result, with a count for the caller, and
  branches out; after the block, every place it owns is released. A
  place never stored holds zeros, which release as nothing.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/count_tests.hd`
  builds a million short strings, 300 chains of 1,000 data nodes with a
  string each, and 300,000 enum payloads, each in a loop, and checks the
  heap's size: 4 pages each time, the static page, the page table and one
  page for each of two size classes. Without the frees the first case
  needs about 50 MB.
- **Size.** hello (`println(42)`, whose text is a counted string) links
  the allocator and the counts: 1,777 to 10,607 bytes in the dev profile
  and 7,417 in release. The allocator's hd is about 8.5 KB of that in the
  dev profile, far over §10.1's estimate of 0.8 to 1.2 KB: this backend
  emits each TIR value into fresh locals and checks every integer
  operation. That is where to cut first.
- **Not yet.** Release is recursive, so a very long chain can exhaust the
  stack until iterative release (job 19). Lists, maps, closures and
  references count when they land (jobs 9 to 12).

### 23.17 What Job 9 Adds

Job 9 lowers lists (§23.7), in `emit/list.rs` with their glue in
`rt.rs`. Each change implements collection value and element-reference
semantics (§15.2), using the buffer's count for copy-on-write.

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
- **Changes.** `push`, an element store, `ListIndexMut` and `ListPin`
  first make the buffer the list's own (`rt:list_unique`). A shared
  buffer is copied: its elements take their own counts, the lists among
  them become copies in turn, so a nested write never reaches another
  list, and the old buffer loses one owner. A `push` into a full buffer
  of the list's own moves it to one of twice the length plus 4, and frees
  the old block without releasing its elements, which moved. A store
  counts the new element before it releases the old one.
- **Release.** A header's release glue releases its buffer; a buffer's
  (`rt:drop_elems`) releases each element's counted references up to the
  length. A list of strings or of structs holding lists frees everything
  at its last release.
- **Loops.** `SnapEnd` gives the snapshot's buffer back at once, so after
  the loop the list's next change copies only when something else still
  shares the buffer.
- **Pins.** `ListPin` and `ListUnpin` count the header's pins, and a
  `push` into a pinned list panics `collection-pinned`. Element
  references are closures over the header and the index, so no fixture
  reaches them before closures (job 11) and references (job 12); a
  pinned buffer may be copied under its references, which reach their
  slot through the header.
- **Indexing.** An index is checked against the length before it is
  narrowed: a 64-bit index past 4 GiB is `index-out-of-bounds`, never
  wrapped.
- **Strings over lists.** `string_from_bytes` and `concat_all` copy a
  list's bytes, or its strings' bytes, into one new string buffer.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/list_tests.hd`
  builds lists of strings and lists of structs holding lists, and copies
  and changes a list, each in a loop, and checks that the heap does not
  grow after a warm-up.
- **Size.** hello holds no list: 10,607 bytes in the dev profile, as
  after job 8. A program that uses lists links the list glue, one
  function per element layout for its buffer, release, copy and change.
  The web compiler uses this same code generator.

- **Not yet.** List iterators (`iter()`) hold a closure, and std's
  shrinking primitive `list_truncate`, so `pop`, `insert`, `remove_at`
  and `clear`, takes `&mut List[T]`: they wait for closures (job 11) and
  references (job 12), as do `assert_equal` and `debug`, which write
  through `&mut DebugWriter`. Job 11 lowers all of them (§23.19).

### 23.18 What Job 10 Adds

Job 10 lowers maps (§23.7), in `emit/map.rs` with their glue in
`map.rs`. Each operation implements collection value semantics (§15.2;
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
  `remove` and `MapIndexMut` first make the table the map's own
  (`rt:map_unique`): a shared table is copied whole, each live entry
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
- **Not yet.** `Map.iter()` and the methods built on it (`keys`,
  `values`, `entries`, printing) hold a closure, and wait for job 11.

### 23.19 What Job 11 Adds

Job 11 lowers closures, function values and trait values (§23.8): the
environments in `emit/closure.rs`, trait values and their vtables in
`emit/dyn_value.rs`, and the collection iterators that hold a closure in
`emit/iter.rs`.

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
  its declaration (a recursive local function's own name) makes a cell of
  zeros, which the declaration fills.
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
  compare their identity fields (§15.2). A trait value that boxed a value
  compares its boxes (`ref.identity.same-place`). `ref_serial` is a
  global counter.
- **Providers.** `$.with(K: v) body` makes `v` the provider of `K` in the
  body, borrowing `v`'s locals, whose count lasts to the function's
  exit. (`Guard`, a match guard's condition block and its two branches,
  lowers here too.)
- **Iterators.** std's `list_steps` and `map_steps` intrinsics, under
  `List.iter()`'s `ListIter` and `Map.iter()`'s `MapEntries`, build a
  step closure whose environment holds the collection's header, with a
  count of its own, and what the collection looked like at creation; the
  cursor keeps its position and passes it in, so a copy advances on its
  own. `rt:list_step` and `rt:map_step` panic `iterator-invalidated`
  when the collection changed (`flow.for.invalidate.panic`).
  `list_truncate`, and so `pop`, `insert`, `remove_at` and `clear`, reads
  the list through its reference and truncates it in place.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/closure_tests.hd`
  calls function references and closures, shares a local between a scope
  and its closures, dispatches through boxed and widened trait values,
  and makes closures and trait values in a loop, checking that the heap
  does not grow after a warm-up.
- **Trace.** No glue traces yet: the collector (jobs 20 and 21) adds a
  trace entry to the same descriptors that release, so neither an
  environment nor a box needs a second word for it.
- **Size.** hello holds no closure and no trait value beyond its host
  provider: 10,635 bytes in the dev profile (10,607 after job 8) and
  7,445 in release.
- **Not yet.** A generic method called through a trait value needs the
  erased method ABI (crates/hd_mono/DESIGN.md §13.5.1), which §23.27
  adds. A function with row parameters, which takes its rows' providers
  as one context value (crates/hd_mono/DESIGN.md §12.4), and so
  `$.Context` values and a
  `$.with` of one, are refused, as is a coercion of a function value
  whose row changes, which needs an adapter that passes the providers
  its code takes. Suspending function values and closures wait for job
  13.

### 23.20 What Job 12 Adds

Job 12 lowers references in today's language, cleanup scopes, and
element references whose copies own their list's pin (OBJECT_MODEL_TIER1.md
§6.3; issue #556).

- **References as addresses** (issue #587). The checker names each
  reference's place in the TIR (`RefPlace`: a local, a top-level
  binding, a place through another reference, a data object's field or a
  list element, then a struct field path) and each read, write and
  identity through one (`RefRead`, `RefWrite`, `RefIdentity`), each
  owning the block of its checker-generated closure form (§15.2,
  "References"). The driver strips those blocks from the bodies of a
  linear build before collection (`hd_tir::ir::strip_closure_forms`), so
  their closures, cells and `ref_serial` calls never reach this backend.
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
  them, counting the new value before it releases the old. A reference
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
- **Cleanup scopes.** A `Scope` with `defer` suites uses an
  exit ladder (`emit/scope.rs`, crates/hd_mono/DESIGN.md
  §12.2): each suite has a flag that its `Defer` sets; a `break`,
  `continue` or `return` that leaves the body stores its exit's code and
  branches to the scope's end, which runs the registered suites last in,
  first out, as a forbidden context, then takes the exit again through
  the next scope out. A suite that suspends (`using`'s `dispose!`) waits
  for job 13.
- **Releases at last use.** A reference is released after its last use,
  not at the end of its block (§6.3 rule 5), so a counted value can no
  longer wait for the function's exit (§23.16). `emit/live.rs` finds,
  within one body and at statement granularity, the statement of a
  value's own block that holds its last use, and for a TIR local the
  innermost block holding all its uses and its last statement there; a
  parameter and a closure's capture live in the root block. After that
  statement the place is released and zeroed, so the exit, which still
  releases every place, releases it as nothing. A `break` or `continue`
  releases what the blocks it leaves hold, up to the innermost cleanup
  scope it crosses, whose suites may still read the scope's locals. A
  value used where no statement of its block holds the use (a block's
  result, a match decision's value read in an arm) keeps its count to the
  exit. Uses are TIR's `value_operands`, with a call's provider pairs;
  any other instruction's value words are over-approximated, which only
  delays a release.
- **Element references own their pin.** An element reference is the
  element's address in its list's buffer, and its count is a count of
  the buffer. `ListPin` makes the buffer the list's own, as before, and
  gives the header its owner word (§23.7): the function-table slot of
  the list's clone glue plus one. A buffer whose header has an owner
  word has that header as its one owner, so every count past the first
  is a reference's or a `ListPin`'s: the list is pinned while the
  buffer's count is over 1 (`rt:list_pinned`), and a `push` or a
  truncation then panics `collection-pinned` (`ref.elem.release.copies`).
  A change through the list writes the buffer in place, which every
  element reference sees. A copy of a pinned list copies its buffer at
  once through the owner word's clone glue (`rt:list_copy`), and a copy
  of an unpinned one clears the word, since the buffer is shared again.
  `ListPin`'s own count lasts until the checker's `ListUnpin`; each
  copy of an element reference holds its own, so a reference stored in
  a field, returned, or captured by a closure called later keeps the
  list pinned until that copy's last use. Two references to one slot
  share its address (`ref.elem.identity`). A trait value of a value
  that is one counted address is that address, never a copy.
- **Bounded memory.** `crates/hd_driver/tests/std_rt/ref_tests.hd` writes
  through references to locals and elements, checks that a pin ends with
  its reference's last use, and takes references to locals and elements
  in a loop, checking that the heap does not grow after a warm-up.
- **Size.** hello: 10,719 bytes in the dev profile (10,635 after job 11)
  and 7,529 in release, the releases at last use in the code it links,
  before references became addresses.
- **Host providers behind references.** A key whose trait has a `&self`
  or `&mut self` method, as `Console` has since `write_line!` takes
  `&mut self`, gets its host provider behind a reference (`req.ref.host.reference-values`):
  the entry's poll builds the host trait value, no data beside the
  vtable of host stubs, and calls the driver's `std.rt.host_provider`
  instance at `dyn K`, whose result it installs as the provider. So
  `println` reaches the host through a `&mut dyn Console`. With the
  reference's closures and identity this cost hello 12,014 bytes in the
  dev profile and 8,824 in release. Since references are addresses, the result is the host trait
  value itself (a reference to a trait value is that trait value), and
  `host_provider` allocates nothing.
- **Not yet.** Issue #556's three escape cases pass here. The
  release points are statement-granular and per body: a reference passed
  to a call is held by the caller's place until the statement ends.

### 23.21 What Job 13 Adds

Job 13 lowers suspending code: §14's state machine, waker, entry driver
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
  Shared cancellation cleanups retain every distinct waiter, merging
  bits for a shared node. Registrations compare full handle generations
  and release their counted links on completion or abort; stale wakes
  cannot follow a freed link. Completing or cancelling an `all!` releases
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
- **Size.** hello: 11,463 bytes in the dev profile and 8,273 in release.
  A `main` that never suspends links no task runtime.
- **Not yet.** Calls whose row parameters carry keys. Erased generic
  methods (job 11). Host methods with structured arguments or results,
  and a profile trait's methods with arguments, came with job 14
  (§23.23).

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
- **Size.** `std.rt.alloc` linked: 7,861 bytes of code before, 1,587
  after. hello (`println(42)`): 12,758 bytes in the dev profile before
  and 9,568 in release; 6,010 in both after. `println("hello")` is 5,988 bytes.
- **Not yet.** hello still links the allocator. Its literal is no
  longer the reason, nor, since references are addresses (§23.20), the
  `&mut dyn Console` that `println` reaches the Console through, which
  is now the host trait value itself. The host call's frame is a counted
  object (§23.21), and `"$value"` builds a counted string. The counting
  code both need links `retain`, `release` and `free`. Making hello link
  nothing would take formatting into the exchange buffer. That is a
  design question for the owner.

### 23.23 What Job 14 Adds

Job 14 (issue #595) lowers every host method of the ABI table, the test
runner's host traits, host handles and a runtime profile's own traits,
so programs that use `fs`, `process`, `host`, `sys`, `net`, `http`,
`time` and `random` run on this backend (`boundary.rs`).

- **One wire format, generated in the module.** The host contract stays
  crates/hd_run_wasmtime/DESIGN.md §17: the exchange buffer is memory 0,
  `hd.x`, and every structured argument and result crosses in it in the
  §17.4 encoding. This backend generates the encoders and decoders over
  its own layouts, so `host/*.mjs` and the wasmtime host share one
  exchange-buffer contract, and the
  heap (memory 1) stays private to the module. The alternative, host
  codecs that read the heap's layouts, would need the host to know each
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
- **Size.** hello (`println(42)`, dev): 5,046 bytes here before and
  after. A program links only the codecs
  of the types its host methods cross.
- **Checked by** `tests::every_runtime_helper_validates`, which builds
  a host call and a handle's leaf over a record of every kind of boundary
  type and validates the module, and by the runtime fixtures of the
  profiles, the property and snapshot runners, `Random.fill` and
  `ConsoleInput.read_line` in `LINEAR_PASSES.tsv`. Programs over the
  real `fs`, `process`, `env`, `args`, `sys`, `clock`, `random`, `net`
  (loopback TCP and UDP) and `http` (a local server) hosts print the
  same output on both backends.
- **Not yet.** These modules are untested on the wasmtime host while it
  is parked (job 15). It reads and writes the same wire format, so job
  15 should need no codecs of its own.

### 23.24 What Job 16 Adds

Job 16 (issue #594) is checked mode: the linear backend built to find
its own counting bugs. `Packages::checked` selects it internally;
`HD_CONFORMANCE_CHECKED=1` selects it in conformance runs. A normal build
is unchanged.

- **The checked runtime.** The driver roots `std.rt.checked`
  (`lib/std/rt/checked.hd`) in place of `std.rt.alloc`'s counts, so
  emitted code calls its `allocate`, `free`, `retain` and `release` by
  key, as it calls the plain ones (§23.16). Nothing else in the module
  changes.
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
- **The sweep.** `HD_CONFORMANCE_CHECKED=1` builds every
  case of the conformance runner in checked mode, so each must pass with
  its counts audited.
  `crates/hd_driver/tests/std_rt/checked_tests.hd` tests the runtime's
  poison, its reuse rule and its counts.
- **Size.** hello (`println(42)`, dev): 5,463 bytes on `linear` before
  and after, and 7,297 with the checked runtime.
- **What the first sweep found.** A point's cancellation branch kept
  its cancelled child in the frame's child slot, and a `using` binding's
  `dispose!` that waited overwrote that slot, so the child frame leaked
  (15 cases, most with `race!` or `timeout`). The branch now lets the
  child go before the exit ladders. Eight listed cases still leak, all
  reference cycles, which wait for the collector (job 21): rings of
  `&mut` nodes, and a recursive local function, whose environment holds
  the cell that holds the function. §23.28 collects them now.

### 23.25 What Job 17 Adds

Job 17 (issue #596) is the first round of count elision
(docs/future-work/REFCOUNT_RUNTIME.md §4.3): a parameter is passed as its
type says, and a count moves instead of being copied where a value is
consumed at its only use. A program's behaviour does not change; only
where its counts are taken does (`emit/count.rs`, `emit/live.rs`).

- **Parameters by their type** (`layout::Pass`). A plain function, one
  that does not suspend, takes a value parameter `T` as its own copy:
  the argument arrives with a count of each counted reference in it, and
  the callee releases it as it releases any place it owns. A `&T`
  parameter is borrowed: the caller keeps the argument alive for the
  call, and neither side counts it. The callee's prologue no longer
  retains its parameters. A borrowed parameter that the body writes, or
  that a cell holds, is retained on entry and owned from then on. Only
  the parameter's type decides, so every caller and the callee agree
  without reading each other's bodies (§4.3's header rule). A caller
  reads the types from the callee instance's own parameter locals
  (`emit::callee_params`), which the callee itself counts by. A
  suspending function's cold constructor still counts each argument into
  its frame, so its callers lend their arguments.
- **Callers.** A direct call, a default argument's call and a call of a
  function value give each owned argument its count: by a move when the
  argument is the operand's only use, else by one retain. Arguments
  spread from a tuple do not line up with the parameters one to one, so
  each owned parameter's values take a count wherever they come from. A
  function reference's adapter passes its arguments on unchanged. Code
  the backend generates still lends what it passes: a vtable slot
  (`Helper::DynSlot`, `DynRead`), the map glue's `key_bucket_hash` and
  `eq` calls (`KeyOps`), and the boundary's JSON codecs and
  `host_provider` count a copy of each argument the target owns
  (`rt::own_arg`). So a dynamic call lends its arguments, as host stubs
  expect.
- **Moves at last use** (`live::Moves`). An instruction value whose only
  use is a later statement of its own block moves its count into that
  statement when the statement consumes it: an owned argument, a local's
  or a global's or a cell's store, a field write, a write through a
  reference, a list or map store, a return, a `break` value
  (`Em::take`). A block's tail used only as the block's value moves into
  it (`Em::take_tail`). A `LocalGet` that is its local's last access, the
  read after which the local was to be released, takes the local's count
  instead of one of its own (`Em::take_local`). A borrowed parameter has
  no count to give, and a cell is shared, so neither moves.
- **Construction consumes.** A struct, a tuple, a data object, a variant,
  a newtype, a copy-update and a list literal count each part they take
  (`Em::count_parts`) rather than the whole new value: a part at its only
  use moves its count in, any other takes one.
- **Why it is sound.** A moved place is zeroed once its consumer has read
  it, so the function's one exit, which still releases every place,
  releases it as nothing. The release planned after the consuming
  statement is skipped there, and only there: a jump out of the block
  before the move still releases the place (`Em::drop_exited`). A value
  with one use in its own block runs that use at most once per value
  computed, and nothing reads it afterwards. A value that only `with`
  lends, or that two operands of one call read, has more than one use and
  keeps its count. A borrowed argument is an instruction value that keeps
  its own count until its statement has ended, so the referent outlives
  the call. `HD_CONFORMANCE_CHECKED=1` passes every
  case in the job's recorded linear suite but the eight cycles that wait for the
  collector (§23.24), with every count audited and no freed block
  touched.
- **Counts.** `std.rt.checked` now totals the counts it adds and takes,
  and a checked entry exports them as `hd.retained` and `hd.released`.
  Over the 21 single-file tour programs of `website/tour/`, the retains
  executed went from 1,203 to 477 and the releases from 1,446 to 720.
  Over hello, the four `examples/dogfood` programs, three bench and
  example programs and ten conformance programs, retains went from 15,465
  to 9,940 and releases from 16,890 to 11,365. `calc.hd`: 3,084 retains
  to 2,072; `textstats.hd`: 6,803 to 4,623; hello: 5 to 2. Every program
  printed the same output before and after.
- **Size.** hello (`println(42)`, dev): 5,457 bytes on `linear` before,
  5,418 after; 7,350 with the checked runtime,
  whose count totals cost 132 bytes. Programs shrink by about 1 to 10 per
  cent, as most entry retains and many copies go.
- **Round two** (job 18, issue #641). An immutable field or local read
  borrows its representation when its unaliased, unchanged source's
  existing lifetime strictly contains every use. List and map loop
  elements borrow from the checker's private snapshot, including when
  user code changes the original collection. A store, capture, return or
  owned argument still takes a count. Neither analysis extends a source's
  last use, which could otherwise keep an element reference pinned.
  Borrowed instruction and local slots take no count and are omitted
  from function/frame release and trace glue; the source owns them.
- **Balanced pairs.** After local cleanup, a short straight-line span can
  lose a retain and its matching release on unchanged locals. A freeing
  call, memory operation, operand-local write or control-flow edge ends
  that proof. Retains alone cannot free the original owner.
- **Round-two counts.** The checked builds of all 25 current tour
  programs execute 522 retains and 776 releases before this round,
  365 and 619 after (30.1% and 20.2% fewer). Four benchmark programs
  execute 521,033 retains and 522,061 releases before, 441,031 and
  442,059 after. These are `bench/math`, `bench/tokenizer`, the runtime
  StringBuilder benchmark at its original 40,000 iterations, and the
  runtime string-concatenation benchmark scaled from 40,000 to 1,000
  iterations to bound checked quarantine memory. StringBuilder alone
  falls from 520,016/520,026 to 440,014/440,024; the other three are
  unchanged. Legacy benchmark sources that no longer type-check are
  excluded. Every measured program has identical output and status.
- **Conservative cases.** General alias bindings, mutable data/reference
  projections, copied collection headers and erased reads that can make
  a fresh box keep their counts. A sub-body with deferred cleanup keeps
  its counts because registration order is not execution order. A
  dynamic call still lends its arguments, so its slot counts what the
  target owns.
- **Checked by** `crates/hd_driver/tests/std_rt/count_tests.hd`, whose
  new cases pass strings through owned and borrowed parameters into a
  struct in a loop, check that the heap does not grow, and check that an
  argument used again after a call keeps its value.

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
- **Size.** hello (`println("hello")`, release): 4,977 bytes on
  `linear` before and after; a program that calls no function value and
  no function with row parameters links neither context helper.
- **Not yet.** A method with type parameters of its own called through
  a trait value needs the erased method ABI, which §23.27 adds.

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

Issue #618 (job 21) reclaims reference cycles on the linear backend by
synchronous trial deletion (Bacon and Rajan, ECOOP 2001), as
docs/future-work/REFCOUNT_RUNTIME.md §7 plans. The collector is hd in
`std.rt.cycle` (`lib/std/rt/cycle.hd`); the backend adds what only it can
build (`rt.rs`'s `Helper::Collect`, `CycleTrace`, `CycleRelease`,
`CycleFree` and `Opaque`, and `cycle.rs`'s per-type analysis).

- **What takes part.** An object whose type can be on a cycle, which
  the backend decides when it emits the allocation (`cycle.rs`, job 20,
  issue #642) and records as bit 31 (`CYCLIC`) of the descriptor word
  (§23.4). Strings and other objects without glue hold nothing and never
  take part. A frame is tagged (`Helper::Opaque`, from
  `task.rs`'s `new_zeroed`) so the collector checks its task flags. While
  `ACTIVE` or `RUNNING` is set, its saved slots may be stale: what it
  holds counts as held from outside, and its candidate stays buffered
  until an idle collection. An idle frame participates like any other object.
- **The collector word** sits in the last 4 bytes of the block, not right
  after the payload as §23.4 first planned: nothing records a payload's
  size, while the allocator knows each block's. `rt:new` asks for 4 more
  bytes when the descriptor has `CYCLIC` and has std's `track` zero the
  word; any other object has no collector word. Its low 2 bits are the color (black, gray, white, purple), bit 2
  marks a frame, and the rest is the object's place in the
  candidate buffer plus one, 0 for none.
- **Candidates.** Std's `release` (`std.rt.alloc`'s and
  `std.rt.checked`'s) makes a `CYCLIC` object purple and buffers it when
  it leaves a count above zero (`suspect`), and takes it out of the
  buffer at its last count (`forget`), before its glue runs, so a
  collection never sees an object half released. Bit 30 of the
  descriptor word (`BUFFERED`) says whether the object is in the buffer:
  a buffered object is purple outside a collection, so `suspect` returns
  at once for one, and `forget` returns at once for one that is not, and
  neither reads the collector word, whose place needs the block's size.
  The release of any other object is the decrement alone. `free` forgets
  too, for the blocks the backend frees directly, such as a list's old
  buffer. `forget` checks that the buffer's entry names the block, so a
  block that is no counted object, whose words are data, is left alone. A
  retain does not blacken a purple object: a needless candidate costs a
  walk, not correctness.
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
  at least the threshold's candidates: 10,000 at first, halved back
  toward 10,000 after a collection that frees at least half of what it
  marked gray, doubled after one that frees less (REFCOUNT_RUNTIME.md
  §7.4). An allocation is never reached from release glue, so a
  collection never interrupts one. The entry's poll collects whenever it
  answers (`Helper::CollectPoll`): at idle when the answer is Pending, and
  at the end. Checked mode's poll collects after the teardown releases the
  bindings and before the leak audit, so a handle leaked inside a cycle
  is still reported.
- **Memory.** A full buffer first closes the gaps that forgotten
  candidates left, so it grows with the live candidates only. The
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
- **Frame cycles (#637).** `dropped-suspension-cycles` drops cold frames
  retaining arguments and closure captures, then resumes a nested direct
  call through a pending host write and collects while saved strings are
  stale. Checked mode audits the final heap on both engines.
  `std_rt/cycle_tests.hd` drops 400,000 argument/capture frame cycles and
  checks reuse in the normal allocator; checked mode quarantines freed
  blocks and checks their counts instead.
- **Cycle capability** (job 20, issue #642; REFCOUNT_RUNTIME.md §7.1).
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
  the walk does not know), or, for `object`, the object's own type. The
  walk does not ask whether a slot can be written after construction
  (§7.1's mutable edge): a recursive immutable enum takes part, which is
  conservative. A suspending body's frame, a child's or a race's frame
  with glue (#637), a reference view and a boxed reference value
  (`rt:ref_view_drop`, `ref_box`, whose types `rt.rs` does not know)
  always take part. A host call's leaf frame takes part as `holding` says
  of its arguments' types. A list copy (`rt:list_copy`) takes part as its
  original does, read from the original's descriptor at run time.
- **What it saves** (`bench/study/run.mjs`, hd on V8 only, same machine,
  load 9 to 13 before and 5 to 9 after, so the ratios are approximate):
  nbody 60,803 ms to 33,322; binary-trees 65,452 to 44,953; closures
  16,673 to 11,713; json 89,904 to 56,779. `List[f64]`, `List[Body]`,
  iterator environments and closures that capture values no longer touch
  the collector; binary-trees' `&Node` cells still take part, as a
  reference may close a cycle, but a release of one already in the
  buffer returns without reading its collector word. hello (`println(42)`,
  release) grows from 9,882 bytes to 9,985: `visit` is now called from
  `rt:release_ref`, behind the `Tracing` check. `std_rt/cycle_tests.hd`
  checks that sharing and dropping lists of `f64` and of a struct and a
  closure leaves no candidate, while a dropped ring does.
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
  code still calls the collector directly, as std's own tests do. So
  `std.rt.cycle` is not linked: hello (release) goes from 11,346 bytes to
  7,183 for `println(42)` and from 11,247 to 7,084 for `println("hello")`.
  Before #618 (and at #618 itself) these were 3,725 and 3,569, because
  hello then allocated nothing. Its host call's leaf frame now needs the
  allocator and the wake table. `CYCLE_FNS` are still rooted, since
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
  some signature is wide. It grows down from its end; a frame below
  address 0 wraps to an address past the end, so a too-deep chain of wide
  calls traps on its first store. A trap skips its frames' release,
  which only shrinks the room a later entry in the same instance has.
- **Not yet.** A function that handles a wide value keeps it in locals:
  each copy of a 1650-value struct is 1650 more locals, and V8 refuses a
  function of more than 50,000 locals. A body that reads many fields of
  such a value by value can reach that limit.


### 23.30 Known Block Starts

Issue #643 avoids finding an object's block when its representation
already supplies the payload start, eight bytes after its header.
Interior pointers keep the page-table path, including string slices and
physical or erased references without a block-start proof.

- **Known counts.** The runtime's `retain_known` and `release_known`
  entries use `address - 8`. Null and static zero-count objects stay
  uncounted. Final releases use the existing allocation-free release
  worklist. Both release paths visit only while `Tracing` is set, buffer
  only `CYCLIC` objects and forget only `BUFFERED` candidates. Collection
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
