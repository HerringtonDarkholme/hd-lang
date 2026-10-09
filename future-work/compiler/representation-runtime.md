# New Compiler: Runtime Data Representation, Runtime Side

Part of the [compiler design](README.md).

Status: Study, not decided, 2026-10-07 (runtime side).

A second study prices the same representations from the compiler-cost
side. The lowering design pass merges both into
[wasm-layout.md](wasm-layout.md) and [codegen.md](codegen.md). This file
changes no decision. Every "recommend" below is a proposal.

The owner's guidance for this study (2026-10-07): runtime performance has
two axes, code speed and code size. Each section prices both.

## Summary

**The cost model.** Runtime speed scales with allocations, indirect
calls, casts and pointers per live object. Code size scales with
distinct instances, inlined copies and constant-building code. The two
engines differ in ways that matter:

- wasmtime compiles ahead of time with Cranelift. It never inlines an
  indirect call, its collector is a semi-space copier with no
  generations, and a reference is a 32-bit index into the GC heap.
- V8 inlines `call_ref` speculatively and has a generational collector.
  A V8 number never stands in for wasmtime.

**Pilot numbers (V8 only, Node 24.19, a heavily loaded machine).** These
calibrate the spike; they don't replace it.

| Question | Measured on V8 |
| --- | --- |
| `ref.cast` per read of an `eqref` array element (proposal A) | +0.3 ns per read (0.59 to 0.86 ns) |
| `call_ref` with one target, per element | 0.31 ns, same as inlined: V8 inlined it |
| `call_ref` with 8 targets, per element | 5.3 ns, against 0.36 ns without the call |
| linear memory to `(array i8)`, one byte per step | 0.4 to 0.8 ns per byte; `array.copy` between GC arrays costs the same |
| enum of 4 scalars in a list: parallel arrays vs one box per element, build and sum | 2.8 to 4.6 ns vs 44 to 89 ns per element |
| 16-byte substring: copy vs a view struct | 13 ns vs 3.6 to 6.8 ns |
| FNV-1a over an `(array i8)` | about 0.45 ns per byte |

**What the orchestrator's analysis missed or got wrong.**

1. **`string.slice` must not copy.** The spec says `slice` runs in
   constant time and shares the bytes
   ([`module.string.slice`](../../spec/lang/10-modules.md#r-module.string.slice),
   [`module.string.slice.shared`](../../spec/lang/10-modules.md#r-module.string.slice.shared)).
   The current layout, an `(array i8)` whose substrings copy, breaks
   that rule. Either the representation changes or the spec does. That
   is open question 1, and it decides every string row.
2. **The `Hash` protocol, not the string layout, sets map speed.**
   `Hash.hash` takes `mut dyn Hasher`, and `Hasher.write` takes a
   `List[u8]`. std's `i64` hash builds a fresh list of 8 bytes, and
   `string.hash` copies the whole string with `to_utf8()`. On a map
   lookup that is 3 to 5 allocations and 2 or more indirect calls before
   one key is compared. A cached string hash can't help, since the map
   never sees a hash value, only bytes written to a hasher. Fix the
   protocol (rank 1).
3. **A layout must be the same in locals and in storage.** Today a
   `multi` value over 4 Wasm values is boxed in locals but stays unboxed
   in fields and arrays. Then every read of such an element from a list
   allocates a box. The size rule of proposal B must hold everywhere, or
   it costs an allocation per read.
4. **Merging (A) matters more for size than for speed.** Without it, std
   collection methods alone can exceed the `dead-code` budget of
   10 KB per 1,000 lines in a program with 20 element types.
5. **Closure specialization (C) needs one more step.** An adapter stores
   its closure in a field of a mutable `Iterator` struct. So the inliner
   must scalar-replace that struct before the `call_ref` target is known.
   The loop is inline, scalar-replace, devirtualize, repeated per stage.
6. **Host bytes (D) are cheap on V8.** Copying one byte per step from the
   exchange buffer costs about as much as `array.copy`. The browser has
   no bulk path, and doesn't need one for 10 MB. Whether wasmtime matches
   is the open number. A wasmtime host-side bulk fill may not exist in
   the public API (unverified); the spike checks it.
7. **The wasmtime collector has a heap-growth pathology.** With a
   moderate long-lived set, it re-copies the set on every collection and
   never grows the heap. One report measured 337 collections and a 8.8x
   slowdown, fixed by `Config::gc_heap_initial_size`. That is a host
   setting for `long-run-memory` and `runtime`, not a layout.
8. **Inline embedded parts are impossible on Wasm GC.** A part read
   yields the part itself, which may be stored elsewhere, and Wasm GC
   has no interior references. Drop the "Later" idea for Wasm.
9. **`Deque`, `Heap` and `Set` are written in a way that doubles their
   arrays.** `Deque` and `Heap` store `List[T?]`, which for a scalar `T`
   is two arrays plus a tag test per access. `Set` is `Map[T, bool]`, one
   wasted value array.
10. **Short literals as `array.new_fixed` cost about 3 bytes per ASCII
    byte.** A 16-byte literal is 65 bytes of code. A literal pool over
    one data segment is smaller for any literal over about 5 bytes.

**Ranked changes** (details in [Ranked Changes](#ranked-changes)): fix the
`Hasher` protocol; one layout per type in every position; settle string
slicing; erased storage for reference type parameters; closure
specialization with budgets; std collections over raw arrays; a literal
pool; size budgets for the inliner and the thunks; wasmtime heap sizing.

## 1. The Cost Model

### 1.1 What A Hot Path Pays

| Cost | wasmtime (Cranelift, copying GC) | V8 (TurboFan, generational GC) |
| --- | --- | --- |
| allocation | bump pointer in a semi-space; whether it is inline or a libcall per `struct.new` is unverified, spike E0 | inline bump allocation in the young generation |
| collection | copies every live object on each collection; no barriers; cost grows with the live set | young-generation scavenge copies survivors only; old objects are not copied on a minor GC |
| reference | a 32-bit index; every access adds the heap base | a compressed pointer |
| `call_ref`, `call_indirect` | an indirect call, never inlined ([Fitzgerald](https://fitzgen.com/2025/11/19/inliner.html)) | speculatively inlined from feedback, up to a few targets ([V8](https://v8.dev/blog/wasm-speculative-optimizations)) |
| `ref.cast` to a final type | a null check, an i31 check for `eqref`, a header load and a compare (expected, not measured) | measured +0.3 ns per read |
| array bounds | checked on every access unless Cranelift proves it | checked; often hoisted |

Two consequences shape every section below:

- **An allocation costs more on wasmtime than its bump suggests.** Each
  collection copies the whole live set. A program with 50 MB live and a
  short-lived allocation per element pays a full copy every few
  megabytes of allocation, unless the heap is sized up (§5.1).
- **Every indirect call stays indirect on wasmtime.** V8 hides a
  monomorphic `call_ref` completely (0.31 ns against 0.28 ns inlined, in
  the pilot). Only the compiler's own inliner removes it on wasmtime.

### 1.2 What Code Size Pays

Size feeds four things: browser download, instantiation, engine compile
time (Cranelift and TurboFan both compile per function), and the
`size-startup-heap` and `dead-code` targets (tiny ≤ 2 KB;
≤ 10 KB per 1,000 lines). Pilot byte counts, from Binaryen-encoded
modules without optimization, with the type entries they add:

| Lowering | Bytes |
| --- | ---: |
| `List[i32].push` with growth | 124 |
| `List[ref].push` with growth | 130 |
| list index with a bounds check | 59 |
| the same, reading `eqref` and casting at the caller's type | 67 |
| one `ref.cast` at a use site | 4 |
| a closure: environment type, code function, construction site | 78 |
| a vtable of 3 slots (global only, beyond its function) | 27 |
| a thunk that casts, unboxes an `i31ref`, calls and boxes | 25 |
| a 3-value enum passed through (parameters and results) | 28 |
| the same enum boxed, passed through | 35 |
| a 3-arm `match` on a `multi` enum | 44 |
| the same `match` on a boxed enum | 68 |
| a 5-byte literal as `array.new_fixed` in a global | 32 |
| a 16-byte literal the same way | 65 |
| a 32-byte literal the same way | 113 |
| a literal-pool helper over `array.new_data` (once per program) | 79 |
| one literal use through the pool | about 18 |

The instance count multiplies these. A std method used at 20 element
types is 20 copies unless the copies fold (§3.1, proposal A).

### 1.3 Two Principles

1. **A layout depends only on the type's definition.** It may read the
   definitions its fields name, recursively, which `layout_hash`
   already covers ([codegen.md §13.8](codegen.md#138-code-entries)). It
   never reads uses, call sites, or the program. Every recommendation
   below keeps this; each section says so in its last row.
2. **Specialization makes code, never layouts.** Inlining and closure
   specialization may duplicate code per call site. They never invent a
   Wasm type that another instance would need to agree on. So a cached
   instance is valid in every program that reaches it.

A third principle follows from finding 3 of the summary:

3. **One representation per type in every position.** A type has the
   same Wasm values in a local, a parameter, a field and an array
   element, apart from packing scalars (`i8` for `bool`). Converting
   between two forms of one type at every load allocates or copies.

## 2. Scalars

| Item | Current design | Speed | Size |
| --- | --- | --- | --- |
| integers, `bool`, `char`, floats | the Wasm scalar; packed `i8`/`i16` in fields and arrays | free | free |
| `i64` multiplication overflow check (debug) | divide back, about 20 cycles | the one real debug cost | small |
| erased scalar (`Any`, `dyn`, open values) | `i31ref` up to 16 bits, and for any integer whose value fits in 31 signed bits; else a box struct | a box is one allocation of 16 to 24 bytes, only for large integers and floats | a few bytes per site |

**Prior art.** Scala.js on Wasm stores integers in the `i31` range as
`i31ref` and boxes larger numbers into structs
([Scala.js 1.19](https://www.scala-js.org/news/2025/04/21/announcing-scalajs-1.19.0/)).
wasm_of_ocaml uses `i31ref` for OCaml's tagged integers
([Vouillon](https://cambium.inria.fr/seminaires/transparents/20231213.Jerome.Vouillon.pdf)).
dart2wasm boxes `int` in generic positions (unverified detail).

**Integers that fit (orchestrator, 2026-10-07).** This study first
dropped the owner's "i32 uses `i31ref` when it fits", reasoning that the
fast path adds a `ref.test` to every unbox. It doesn't: a box-only
unbox from `eqref` already needs a `ref.cast`, whose own i31 check comes
first. So every integer width uses `i31ref` when the value fits in 31
signed bits and falls back to a box, as Scala.js does.

**Recommendation (high).** Keep, with the fast path. Layout is type-only;
only the runtime form depends on the value.

## 3. Data, Parts And Containers

### 3.1 `data T`

**Current.** One struct per type, one mutable field per declared field,
`(ref $T)`. Types are structural, so two data types with the same fields
share a Wasm type.

| Axis | Cost |
| --- | --- |
| speed | one allocation per construction; field read is one `struct.get` (on wasmtime, base plus index plus offset) |
| GC | header plus fields; every reference field is one more pointer the copier follows |
| size | `struct.new` with its arguments; a type entry per distinct field shape |

**Prior art.** dart2wasm, J2Wasm and Kotlin/Wasm put a class id or vtable
field in every object, because they dispatch on the object
([V8](https://v8.dev/blog/wasm-gc-porting)). hd dispatches through trait
values, so it needs no per-object header field. That saves 4 bytes per
object against those languages.

**Generic data fields: proposal A.** A field of declared type `T`, a type
parameter, today has the exact layout of its argument. So
`List[Point].push` and `List[User].push` have different element types and
never fold. Proposal A stores a type-parameter slot whose argument has a
reference layout as `eqref` (nullable), and casts on read.

| | Exact element types (today) | `eqref` storage (A) |
| --- | --- | --- |
| read | `array.get` | `array.get` plus `ref.cast`: +0.3 ns on V8; wasmtime unmeasured |
| write | `array.set` | same instruction; one V8 run showed 2.4x, a second showed no difference, so treat it as noise until E1 |
| instances | one per element type | one per Wasm layout class for code that only moves `T` |
| size per extra reference element type | about 1 KB for 10 list methods | about 0 for moving methods; trait-calling methods stay per type |
| layout depends only on the type | yes | yes: `List[Point]` is always `{len, (array eqref)}` |

**What A must also say.**

- It applies to every generic slot, not only `Array[T]`: a user's
  `data Box[T]: value: T` too. Otherwise `Box[Point].get` and
  `Box[User].get` still differ. One rule: a slot whose declared type is a
  type parameter, instantiated at a reference layout, is `eqref`.
- A cast lands at the first use that needs the exact type, not at the
  read. Passing an element on to another erased slot needs no cast.
- `string` is a reference only under representation S1 or S3 of §6.
  Under S2 a string is not a reference layout and doesn't share.
- Values whose layout is not one reference (scalars, `multi` enums,
  tuples) keep exact storage. Their instances fold among equal layouts
  as today.

**Size estimate.** A program with 20 element types, 12 of them
references, and 10 list methods per type: about 20 KB of list code
without A, about 9 KB with A. Against a budget of 10 KB per 1,000 lines,
a 2,000-line program spends most of its budget on list methods without
A.

**Prior art.** .NET shares one body for all reference instantiations
(`__Canon`) and specializes value types
([BOTR](https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/shared-generics.md)).
Go shares per GC shape. Kotlin/Wasm, J2Wasm and Scala.js erase all
generics by language semantics and remove casts with Binaryen. That is
what A does for references only.

**Recommendation (medium; E1 decides).** Adopt A for all generic slots at
reference layouts. Reject A if wasmtime's cast costs more than 1 ns per
read in E1 and a list-heavy benchmark loses more than 5 percent.

### 3.2 Embedded Parts

**Current.** A part is a separate struct, referenced by an immutable
field of its outer struct. A read of `post.Timestamps.created_at` is two
`struct.get`s; construction is two allocations.

**Inline layout is not possible on Wasm GC.** A part read yields the part
itself, with its container's access, and a `mut self` method may store it
elsewhere ([`data.part.access`](../../spec/lang/08-data-and-enums.md#r-data.part.access),
[`data.part.aliases-untracked`](../../spec/lang/08-data-and-enums.md#r-data.part.aliases-untracked)).
So a part must be addressable as an object, and Wasm GC has no interior
references. Inline fields would need a copy on every read that escapes,
which then makes later changes through the copy invisible to the outer
value. That breaks the spec.

| Axis | Separate struct |
| --- | --- |
| speed | one extra load per part access; one extra allocation per construction and per copy-update |
| GC | one extra object and one extra pointer per part |
| size | none beyond the field |

**Recommendation (high).** Keep the separate struct, and drop the "inline
layout later" idea for Wasm. A native backend with interior pointers may
revisit it. Type-only: yes.

## 4. Enums, `Option`, `Result` And Tuples

All of these are identity-free values (S1c). A value may be copied,
shared, hoisted or boxed without being observed.

### 4.1 Shapes And Current Layout

| Shape | Current layout | Allocation |
| --- | --- | --- |
| payloadless (`Ordering`) | `i32`; `i8`/`i16` packed | none |
| one reference payload, one empty variant (`User?`) | `(ref null $T)` | none |
| tag plus at most 4 Wasm values (after slot sharing) | `multi` in locals; several fields; parallel arrays in a list | none |
| over 4 Wasm values | boxed in locals; **unboxed** in fields and arrays | one per crossing between the two |
| self-recursive | flat or subtype structs | one per node |

The fourth row breaks principle 3. Reading element `i` of a
`List[Big]` gives five arrays' worth of values; passing it to a function
then boxes it. A loop over such a list allocates per element.

### 4.2 Parallel Arrays Against Boxes (proposal B)

The pilot built and summed a list of 1M enums of 4 scalars on V8:

| Form | ns per element | Allocations per element |
| --- | ---: | ---: |
| parallel arrays (tag `i8`, 3 `i32` arrays) | 2.8 to 4.6 | 0 (amortized array growth) |
| one immutable struct per element | 44 to 89 | 1 |

A loop that only reads would narrow the gap: boxes then cost a load and
a cache miss per element, not an allocation. The pilot didn't measure
read-only loops; E2 does.

Memory and code per element, for an enum with `k` Wasm payload values:

| | Parallel arrays | Boxed |
| --- | --- | --- |
| bytes per element | tag plus every slot, sized for the largest variant: about `1 + 4k` to `1 + 8k` | one reference (4 bytes), plus header and the actual variant's payload |
| pointers the copier follows per element | one per reference slot | one, plus the box's reference fields |
| `push` code size | about `60 + 25k` bytes; one array per slot to grow | about 130 bytes, and it folds with every other reference list under A |
| layout depends only on the type | yes | yes |

At `k = 12`, a parallel-array push is about 360 bytes per enum type and a
`List[E]` element costs 49 to 97 bytes whatever the variant. The boxed
list costs a 4-byte reference plus a box only as large as the variant.

**Prior art.** Swift boxes `indirect` cases and stores the rest inline;
Rust stores every enum inline at the size of its largest variant, and
Clippy warns on large size differences (`large_enum_variant`). Kotlin,
Scala and Java allocate every payload case. wasm_of_ocaml represents
blocks as `eqref` arrays with the tag in field 0
([Vouillon](https://cambium.inria.fr/seminaires/transparents/20231213.Jerome.Vouillon.pdf)).

**Recommendation (medium-high; E2 tunes the bound).**

1. **Slot sharing first.** Payload fields of different variants with the
   same Wasm type share a slot; all reference fields share one `anyref`
   slot, cast by tag. This is research-enum-values.md §5.1 and should
   become the layout rule.
2. **One bound, one representation.** At most 4 Wasm values after
   sharing, tag included: `multi` everywhere, parallel arrays in lists.
   Over 4: one immutable box struct everywhere, locals included. No
   conversion between forms at a load.
3. **Constant values are hoisted.** A boxed enum with constant payloads is
   one immutable global (`.Err(ParseError.Empty)` never allocates).
4. **Scalar replacement removes boxes** that don't escape a function
   after inlining: `match parse(s): .Ok(v) => ...` allocates nothing.
5. **The box is flat when the payload fields number at most 4 in total,
   subtypes otherwise,** as today.

The bound of 4 is a guess at the break-even. E2 measures 1, 4 and 12
scalars both ways, reading and building, and sets the bound.

### 4.3 `Option`

| Case | Layout | Speed | Size |
| --- | --- | --- | --- |
| `T?`, `T` a non-null reference | `(ref null $T)` | free | free |
| `T?`, `T` a scalar | `(i32 tag, T)`; two arrays in a list | free; one extra array per list | about 2 extra bytes per pass |
| `T?`, `T` a `multi` | tag plus the inner values | free up to the bound, then boxed | as above |
| `T??` | an outer tag plus the inner layout | free | small |

A scalar `T?` in a list costs a second array. `Deque[i32]` and
`Heap[i32]` pay that today, because std stores `List[T?]` (§7.4).

**Prior art.** Rust's niche layout makes `Option<Box<T>>` one pointer;
Kotlin's nullable references are the same. dart2wasm and Kotlin/Wasm use
nullable references for nullable types.

**Recommendation (high).** Keep. Type-only: yes.

### 4.4 `Result`

**Current.** `multi`: `(i32 tag, dflt(T'), dflt(E'))`. Every `?` is a tag
test on a local.

| Axis | Cost |
| --- | --- |
| speed | free while the two payloads stay within the bound; a tag test per `?` |
| size | every function returning `Result` has wider results: about 2 to 4 bytes per pass-through |

A common case breaks the bound: `Result[T, IoError]` where the error
enum carries a path string and a kind. Under §4.2 the error is boxed,
so the `Result` is `(i32, T', (ref null $IoError))`, which fits. Errors
are cold, so boxing them costs nothing on the success path.

**Recommendation (high).** Keep `multi`, with the bound counted after
the payloads' own layouts, a boxed enum counting as one value.
Type-only: yes.

### 4.5 Tuples

Same as enums with no tag: `multi` at most 4 values, else one immutable
box everywhere. `List[(K, V)]` from `Map.iter` is two arrays, or one box
array past the bound. Type-only: yes.

## 5. Trait Values, Closures And The Heap

### 5.1 The Heap Size On wasmtime

Not a layout, but it outweighs most layouts. wasmtime's copying collector
decides growth against the whole heap while allocating in one
semi-space. A program with a long-lived set near the semi-space size
re-copies it on every collection and never grows. One report on
wasmtime 49 measured 337 collections and 1.14 s, against 23 collections
and 0.13 s with a 64 MiB initial heap
([issue](https://github.com/voces/vl/issues/3075)).

**Recommendation (medium-high).** `hd run` sets
`Config::gc_heap_initial_size` from the profile, with a default around
64 MiB, and `--max-heap` caps it. Slice 0a adds a test with a 6 MB live
set. E9 measures the curve.

### 5.2 `dyn Trait` And `Any`

**Current.** A pair `(anyref payload, (ref $VT))`. Two fields in a
struct; two arrays in a list. A call is `struct.get` of the slot, then
`call_ref`. Vtables are constant globals per `(type, trait)`.

| Axis | Cost |
| --- | --- |
| speed | per call: one load and one indirect call; V8 may inline it, wasmtime never. A scalar payload is an `i31ref` or a box |
| GC | two pointers per stored trait value |
| size | per vtable about `6 + 3 × slots` bytes plus its functions; per call site about 8 bytes |

**Alternatives.**

| Option | Speed | Size | Note |
| --- | --- | --- | --- |
| pair (today) | no allocation per coercion | two values per stored value | fine |
| one box `{payload, vt}` | one allocation per coercion | one value | worse on wasmtime |
| class id in every data object, global dispatch table (dart2wasm) | `call_indirect` by id plus selector | one table | needs a header field in every object, and scalars still need boxes |

**Known-vtable devirtualization.** After inlining, a `CallDyn` on a value
whose vtable is a constant global has a known target. Emission can call
it directly and inline it. This matters most for `mut dyn Hasher` (§7.2).
It reads no program-wide data, so cached code stays valid.

**Recommendation (high).** Keep the pair. Add known-vtable
devirtualization to the bounded inliner. Type-only: yes.

### 5.3 Generic Methods Through `dyn` (Thunks)

The erased ABI (codegen.md §13.5.1) costs one `call_ref` per open
instruction plus a box per crossing for wide scalars and `multi` values.

| Axis | Cost |
| --- | --- |
| speed | owner accepted a slower `dyn`; not on targets |
| size | about 25 bytes per thunk; the product of impls, type arguments and open instructions. 20 × 5 × 10 = 1,000 thunks = about 25 KB before folding |

**Recommendation (medium).** Add a size guard: the compiler reports the
thunk count per method in `--size-report` and warns past 200 thunks for
one method. Outlining a whole basic block of open instructions per thunk
(the "later" item) cuts both thunks and crossings; make it the first
size fix if the guard fires in std or examples.

### 5.4 Closures And Captured Mutable Variables

**Current.** A base struct per signature holding a typed function
reference; one final subtype per capture shape. A call: `struct.get` of
the code, then `call_ref` with the closure as the first argument; the
callee casts the closure to its own environment type. A captured `mut`
that the closure shares is a cell, a one-field struct.

| Axis | Cost |
| --- | --- |
| speed | per call: a load, an indirect call, a cast in the callee. Per construction: one allocation, plus one per shared cell |
| GC | the environment plus each cell |
| size | about 78 bytes for a small closure: type, code, construction |

The callee's cast is not avoidable while different capture shapes share
one signature: the code receives the base type and must cast to read its
captures. It costs one cast per call, and a capture-free closure skips
it. Only specialization (§8.1) removes it.

**Prior art.** Every Wasm GC toolchain uses a struct with a function
reference (Kotlin/Wasm, dart2wasm, Hoot). wasm_of_ocaml uses closures as
structs with the code pointer in field 0 (unverified detail).

**Recommendation (high).** Keep. Add closure specialization (§8.1), which
removes the call, the environment and the cells together. Type-only: yes.

## 6. Strings

### 6.1 What The Spec Requires

| Rule | Requirement |
| --- | --- |
| [`types.string.len-bytes`](../../spec/lang/04-type-system.md#r-types.string.len-bytes) | `len()` in bytes, constant time |
| [`types.string.host-bytes`](../../spec/lang/04-type-system.md#r-types.string.host-bytes) | UTF-8 bytes at every host boundary, no conversion |
| [`module.string.slice`](../../spec/lang/10-modules.md#r-module.string.slice) | `slice(start, end)` in constant time |
| [`module.string.slice.shared`](../../spec/lang/10-modules.md#r-module.string.slice.shared) | the result shares the original's bytes |
| [`types.sealed.anyval-values`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-values) | `string` is `AnyVal`: no identity |

The current layout, `(array i8)` with copying substrings, breaks the
two slice rules. research.md's Q13 chose copying after Java 7u6, without
noting the rule.

### 6.2 Candidates

| | S1: `(array i8)`, slices copy | S2: three values `(ref $bytes, i32 start, i32 len)` | S3: one struct `{bytes, start, len}` |
| --- | --- | --- | --- |
| spec | needs a spec change to the slice rules | meets it | meets it |
| slice | allocate and copy: 13 ns for 16 bytes on V8 | free | one small allocation: 3.6 to 6.8 ns on V8 |
| new string (concat, builder) | one allocation | one allocation | two allocations, or one if a whole-array string may be the array itself (needs a test on every access) |
| byte access in a loop | `array.get_u` | `array.get_u` at `start + i` | one extra load, hoisted out of loops |
| `len()` | `array.len` | a local | a field |
| equality | length test, then a byte loop | same | same |
| locals, parameters, results | one reference | three values | one reference |
| field | one | three | one |
| `List[string]` | one reference array | three arrays | one reference array |
| into `Any` or `dyn` | free upcast | one box allocation | free upcast |
| folds with other references under A | yes | no | yes |
| long-run memory | a slice never pins its source | a small slice pins a large source (Go's known issue) | same as S2 |
| size | smallest | about 4 extra bytes per pass and per field access | small |
| type-only | yes | yes | yes |

**Prior art.**

- Go's string is a two-word value, pointer and length, and slicing shares
  ([Go blog](https://go.dev/blog/strings)). That is S2 without a separate
  start.
- Java copied substrings from 7u6 on to stop small slices pinning large
  arrays. Swift's `Substring` shares and is a separate type.
- dart2wasm uses two array strings, `OneByteString` (`i8`) and
  `TwoByteString` (`i16`) ([WebAssembly/gc #259](https://github.com/WebAssembly/gc/issues/259)).
- Kotlin/Wasm started with a plain char array, then added a lazy
  concatenation link that folds on demand
  ([Deleuze](https://seb.deleuze.fr/introducing-kotlin-wasm/)).
- wasm_of_ocaml uses `(array i8)` for OCaml's byte strings
  ([Vouillon](https://cambium.inria.fr/seminaires/transparents/20231213.Jerome.Vouillon.pdf)).
- MoonBit, Scala.js and J2Wasm use JS strings through the string builtins
  in the browser ([MoonBit](https://www.moonbitlang.com/blog/js-string-builtins),
  [Scala.js #4994](https://github.com/scala-js/scala-js/issues/4994)).
  They are UTF-16, so they don't fit hd's byte semantics.
- Hoot lowered `stringref` to WTF-8 arrays
  ([Wingo](https://wingolog.org/archives/2023/10/19/requiem-for-a-stringref)).

**Recommendation (medium; owner decision).** S1, with a spec change:
`slice` copies the slice's bytes, in time linear in the slice's length,
not the source's. Reasons:

- It keeps strings one reference: one array per `List[string]`, free
  erasure, folding under A, no pinning.
- Typical slices are short: tokens, fields, lines. A parser touches
  each byte anyway; a copy at about 0.5 ns per byte adds little.
- Code that wants a window has `ListView` for lists. A text view type is
  a std question for later, not this decision.

If the owner keeps the slice rules, choose **S2**. It meets the rules
with no allocation, at the cost of three values per string and boxes in
erased positions. S3 is dominated: it allocates per string creation and
still pins.

> **Superseded (owner, 2026-10-07).** The owner chose S2, Go-style shared
> slices, and kept the slice rules. The layout packs `start` and `len`
> into one `i64`, so a string is two values, `(ref $bytes, i64 span)`,
> and counts 2 toward the boxing bound, not 3. The decided layout and its
> costs are in [wasm-layout.md §15.2](wasm-layout.md#152-values) and
> [lowering-catalog.md](lowering-catalog.md#string). The pinning risk is
> accepted. This section stays as the study's record.

### 6.3 Hashing, Equality And A Cached Hash

A cached hash (Java's `String.hash` field) needs a header object, which
S1 and S2 don't have. And it would never be read: hd's maps hash through
`Hash.hash(state: mut dyn Hasher)`, which writes bytes into a hasher.
The cache could only serve a map that special-cases `string` keys.

The real string-key cost today is the protocol:

```text
impl Hash for string:
    fn hash(self, state: mut dyn Hasher) -> void:
        bytes := self.to_utf8()          # copies the whole string, one push per byte
        u64(bytes.len()).hash(state)     # builds an 8-byte List[u8]
        state.write(bytes)               # indirect call
```

That is two allocations plus one per list growth, about `n` pushes, and
two indirect calls per key. FNV-1a itself costs about 0.45 ns per byte
on V8. The fix is in §7.2.

**Recommendation (high).** No cached hash. Fix the protocol.

### 6.4 Text Throughput

`(array i8)` has no wide loads: every search, compare and hash reads one
byte per step. V8 ran about 0.3 to 0.8 ns per byte in the pilot's copy
loops. Node's own string search uses vectorized native code. A
`split` or `find` over 1 MB costs about 0.5 ms here. The
`text-throughput` target (≥ 0.5x Node's MB/s) holds only if the
workload's per-piece costs dominate the per-byte scan. E7 measures it.

Packing bytes into `(array i64)` would give wide reads but make every
byte access a shift and a mask. Not recommended without a measurement.

### 6.5 Literals

**Current.** Literals of 16 bytes or less are constant globals built by
`array.new_fixed`. Longer ones are mutable nullable globals, filled on
first use by `array.new_data`.

| Option | Size per literal | Speed per use | Startup |
| --- | --- | --- | --- |
| `array.new_fixed` global | about `10 + 3 × len` bytes for ASCII (each `i32.const` of a value ≥ 64 is 3 bytes) | `global.get` | built at instantiation |
| one lazy global plus getter per literal | about 49 bytes | `global.get`, null test, branch | none |
| **pool**: one `(array (mut (ref null $str)))`, one data segment, a shared fill helper | bytes in the data segment plus about 18 bytes per use | `array.get`, null test, branch; hoisted in loops | none |
| pool filled eagerly in `hd.init` by one loop over an offset table | data plus 8 bytes per literal; about 8 bytes per use | `array.get` and `ref.as_non_null` | about 20 ns per literal |

**Recommendation (medium-high).** The pool, deduplicated, one passive
segment, filled lazily through one helper whose fast path is inlined at
the use site. Literals of 4 bytes or less may stay `array.new_fixed`.
Measure the eager variant in E10; it wins if instantiation stays under
5 ms for 10,000 literals. A host fast path that hands the segment offset
to an import (`println` of a literal) skips the array altogether.
Type-only: not a type question.

### 6.6 Append: From Quadratic `+` To A Rope (Proposal, Not Accepted)

Q26 measured `out = out + "abcdefgh"` at 40k appends: ~6.4 GB copied
for a 320 KB result, 84x vs Node, scaling 1 : 3.8 : 15.3 at
10k/20k/40k. Every `+` allocates one flat array and copies both sides,
so any loop over `+` is O(n²) in total bytes moved. The spec fixes the
observables — immutable values, valid UTF-8, O(1) `len`, byte equality,
flat UTF-8 bytes at the host boundary — but not the heap shape, so the
fix can be invisible to the language. One hd-specific constraint shapes
every option: S2 slices share their source's bytes, and strings have no
identity (`AnyVal`), so reusing a buffer in place is unsound wherever
a slice may alias it. Only a uniqueness proof (which hd has no machinery
for) or a shape that shares safely can fix `+` itself.

| | V8 cons-string (lazy tree, flatten on demand) | Java/Go builder (explicit type, amortized buffer) | Rust `String` (move reuses the left buffer) | Swift/Koka COW (in-place append on unique reference) |
| --- | --- | --- | --- | --- |
| how the append avoids O(n²) | `+` returns a node, no copy; flatten walks or copies on demand | appends go to a spare-capacity buffer; `build` copies once | `a + b` moves `a`, pushes into its spare capacity when present | append mutates the buffer iff the reference is unique, else copies |
| heap under Wasm GC | two string shapes: flat `(ref, span)` plus nodes of two child strings and a total length | one growable byte array beside the existing strings | over-allocated arrays plus a length word; same as a builder buffer | a uniqueness word on every string plus the buffer when unique |
| ops that must flatten | host boundary (flat UTF-8 bytes, no conversion allowed); equality and hashing either walk the tree or flatten first | none: the buffer is flat by construction; `build` copies once | none, same as a builder | none when unique; the copy path is flat |
| hashing / equality cost | same byte loops once flat; an unflattened tree costs a walk per compare unless flattened eagerly | unchanged from today | unchanged from today | unchanged from today |
| host boundary | flatten to one array first (one copy, amortized over the appends that built the tree) | already flat | already flat | already flat when unique |
| hello-world code size | flatten + node-shape branches ship in every binary that touches strings; measure with `wasm-size.mjs` on a hello-string before/after | nothing: `StringBuilder` already exists in `std.text` | the growth policy + spare-capacity branches in string code | the uniqueness word, checks, and both paths in string code |
| language needs | none visible: same `string`, same `+`, same observables (`len` stays O(1) from the stored total) | none new: `StringBuilder` exists; only programs that opt in improve | move semantics hd does not have; proving the left buffer unaliased through S2 slices needs new analysis | a uniqueness notion hd does not have; Wasm GC provides no refcounts |
| what stays quadratic | deep trees degrade walks until flattened; needs a depth cap with eager flatten | every bare `+` loop; `join` is O(n log n) halves today | n/a (needs the missing uniqueness proof) | n/a (needs the missing uniqueness word) |

Notes on the table:

- The std `StringBuilder` (parts list + `build` via divide-and-conquer
  `join`) is already sub-quadratic, but only for opt-in code, and
  `join` copies O(n log n). A single-pass `join` (measure total
  length, copy once) would make it O(n) in `lib/std` alone.
- Rust-style reuse is unsound here without more: `out = out + x` cannot
  reuse `out`'s array because an S2 slice may share those bytes, and hd
  has no move to prove otherwise. Same for naive COW: there is nothing
  to count.
- A rope shares safely for the same reason slices do: nodes are
  immutable, so aliasing is unobservable. Slices of ropes stay free
  (a node is already a view), and pinning is the accepted S2 risk.

**Recommendation (medium; owner decision).** A rope: `+` returns a lazy
concat node, flattening happens at the host boundary and inside
equality/hashing, everything else in the spec reads unchanged. It is
the only option that fixes bare `+` loops — including unchangeable user
code — with no new names and no spec change.

**Smallest first step.** Single-pass `join` in `lib/std`: measure total
length, allocate once, copy once. No compiler, spec, or representation
change; it makes today's `StringBuilder` O(n) and the builder-vs-`+`
benchmark then sizes the rope payoff. No owner questions: no new
user-visible names, no observable change.

**Owner decision, 2026-10-08: builder now, rope later.** Strings stay
flat, as Go and Rust ship them. Now: make `join` and `std.text`'s
`StringBuilder` single-pass O(n), so programs that build large strings
have a linear path. The rope above is revisited in phase 3 ("make it
wonderful") with measurements: hello-world size, and real programs
against the builder-vs-`+` benchmark.

### Single-Pass `join`: The `concat_all` Intrinsic (Proposal, Not Accepted)

`join` joins by halves (O(n log n) byte copies) because hd cannot write
the single-pass version: measuring the total needs `bytes_len` per
part (fine), but allocating a string of a computed length and filling
it byte by byte needs a byte buffer, and hd has none — the primitives
table says exactly this of `bytes_concat`
([Standard Library Primitives](../../spec/std/README.md#standard-library-primitives)).
`List[u8]` plus `string_from_bytes` does not close it: without a
reserve operation every push may grow and copy, and the growth policy
is representation, not library code. So single-pass needs one new
primitive. Candidates:

| | A1: `concat_all` over a list | A2: `join` with the separator built in | B: alloc + copy + finish trio |
| --- | --- | --- | --- |
| hd declaration | `@intrinsic("string_concat_all")` `fn concat_all(parts: List[string]) -> string: panic("intrinsic")` | `@intrinsic("string_join")` `fn join_parts(parts: List[string], sep: string) -> string: panic("intrinsic")` | a buffer type plus three functions, e.g. `StringBuf`, `buf_copy`, `buf_finish` |
| `join` on top | interleave the separator into a `2n-1` list, one call | one call | fill a buffer part by part, finish once |
| `StringBuilder.build` on top | `concat_all(self.parts)` — no intermediate at all | `join_parts(self.parts, "")` | same buffer loop with `""` never copied |
| Wasm the emitter produces | `array.new` of the measured total, then one `array.copy` per part at a running offset; the S2 span covers the whole array | same, separators copied inline between parts | same, split across three calls with the buffer (length + fill cursor) on the heap |
| allocations | 1 string; plus one `2n-1` pointer list for `join` with a separator | 1 string; none | 1 string + 1 buffer object |
| copies per byte | exactly 1, plus one O(1) length read per part | exactly 1 | exactly 1 |
| new visible names | one private function | one private function with join semantics baked in | a public-ish buffer type and three functions |
| spec naming | one row in the Representation primitives table (needs the owner's approval, as the table requires) | same, but the row bakes in separator semantics | three rows plus the buffer type |

The hd code on top of A1:

```text
pub fn join(parts: List[string], separator: string) -> string:
    if parts.len() == 0:
        return ""
    let flat: mut List[string] = []
    flat.push(parts[0])
    let i: usize = 1
    while i < parts.len():
        flat.push(separator)
        flat.push(parts[i])
        i = i + 1
    concat_all(flat)
```

No UTF-8 validation is needed anywhere in this path: parts and
separator are valid by construction, and concatenation preserves
validity (`types.string.concat-bytes`). Empty parts and an empty
separator fall out of the same code (an empty part copies zero bytes).

**Recommendation (medium; owner decision).** A1. One private function,
no new types, reusable for both `join` and `build`; the interleaved
list is pointer pushes, one pass, freed with the call. A2 saves that
list at the cost of baking separator semantics into the runtime; B
exposes a whole buffer protocol for the same bytes. If the owner
approves the primitive row, `StringBuilder` goes O(n) with no other
change, and the 40k-append benchmark should read ~320 KB copied once
instead of ~6.4 GB.

## 7. Collections

### 7.1 `List[T]`

**Current.** `{len: mut i32, data: mut (ref $Arr_T)}`, element layout per
§15.2 of wasm-layout.md, parallel arrays for `multi` elements. Index
checks the length, since the array is longer than the list.

| Axis | Cost |
| --- | --- |
| speed | index: two loads, a compare, an array read; push: amortized one store, growth copies with `array.copy` |
| GC | two objects per list; the array holds `capacity` slots |
| size | about 60 to 130 bytes per method per element layout |

**Recommendation (high).** Keep, with A for reference elements. Hoist
`len` and `data` out of counted loops; `ForList` already captures the
length. Type-only: yes.

### 7.2 `Map[K, V]` And Hashing

> **Update (owner, 2026-10-07).** Iteration order is no longer required
> to be insertion order. It stays deterministic
> ([`types.map.order.deterministic`](../../spec/lang/04-type-system.md#r-types.map.order.deterministic)),
> and the bucket hash is an implementation detail
> ([`std-hash.map.bucket-hash`](../../spec/std/hash.md#r-std-hash.map.bucket-hash)).
> The compact insertion-ordered layout below is still a valid choice.

**Spec constraints (as studied).** Insertion order, including "remove then reinsert
goes last"; expected amortized O(1); lookups use `Eq` and `Hash`; hash
values stay outside map semantics
(`types.map.order`, since retired,
[`module.map.complexity`](../../spec/lang/10-modules.md#r-module.map.complexity),
[`types.map.semantics`](../../spec/lang/04-type-system.md#r-types.map.semantics)).
So the map may use any hash function internally; only `hash_of` and
`DefaultHasher` have fixed results.

**Table layout.** Insertion order plus O(1) is the compact dictionary of
CPython 3.6 ([Hettinger](https://mail.python.org/pipermail/python-dev/2012-December/123028.html))
and the deterministic hash table that JavaScript's `Map` uses
([Orendorff](https://wiki.mozilla.org/User:Jorend/Deterministic_hash_tables)).
Node's map benchmark runs against exactly that structure.

```text
$Map_K_V = struct {
  index:   mut (ref (array (mut i32)))   ;; power of two; -1 empty, -2 deleted; entry number otherwise
  hashes:  mut (ref (array (mut i32)))   ;; per entry, in insertion order
  keys:    mut (ref $Arr_K)              ;; per entry; eqref under A, parallel arrays for multi K
  values:  mut (ref $Arr_V)              ;; per entry; absent when V is void
  used:    mut i32                       ;; entries written, holes included
  live:    mut i32                       ;; entries present
}
```

Lookup is hash, probe `index`, compare the stored hash, then `Eq`. Removal
writes a hole and a tombstone; growth compacts. Iteration walks entries
and skips holes.

**The `Hash` protocol is the real cost.** Today, per lookup of an `i64`
key:

| Step | Cost |
| --- | --- |
| a `DefaultHasher` for the call (if the map uses it) | one allocation |
| `signed_bytes(self, 8)`: a new `List[u8]`, 8 pushes | 2 to 3 allocations |
| `state.write(bytes)` through `dyn Hasher` | one indirect call |
| FNV per byte, written with 32-bit halves | about 10 operations per byte |

So about 4 allocations and 100 to 200 instructions before the first key
compare. Node's `Map` hashes a small integer in a few instructions. The
`map` microbenchmark is 5.3x Node today; this protocol alone could keep
it above 3x on wasmtime.

**Options.**

| Option | Speed | Size | Spec |
| --- | --- | --- | --- |
| H1: `Hasher` gains `write_u8`, `write_u16`, `write_u32`, `write_u64` and `write_str`, with default bodies that write the same bytes as today | integer keys: no allocation; string keys: no copy | a few small default bodies | changes the language-tier trait (spec/lang/09); the bytes of `std-hash.bytes.*` are unchanged |
| H2: `Hash.hash[H < Hasher](self, state: mut H)` instead of `dyn` | direct calls, inlinable | one instance per `(impl, hasher)`: about 2x hash code | changes the trait; `dyn Hash` then uses the erased ABI |
| H3: keep the trait; devirtualize known vtables and scalar-replace short `List[u8]` literals | depends on the inliner catching every case; a list is a mutable reference, so its scalar replacement is new work | none | none |
| H4: special-case primitive keys in `Map` | fast | small | a special case; the owner rejects these |

With H1 and known-vtable devirtualization (§5.2), an `i64` lookup is
one inlined mix of 8 bytes, no allocation, then the probe. The map's
internal hasher can then mix a whole `u64` per `write_u64` (a multiply
and a rotate) instead of FNV per byte, since the map's hash is not
observable. `DefaultHasher` keeps FNV-1a, so `hash_of` is unchanged.

**Recommendation (high for the problem, medium for H1).** H1 plus
known-vtable devirtualization; open question 2. Expected gain: the
`map` case from about 5x to within the 1.5x geomean target on V8, and
to about 2x on wasmtime, by estimate. E5 measures it. Type-only: yes.

### 7.3 `Set[T]`

**Current.** `Set[T]` wraps `Map[T, bool]`: a values array of `i8` that
says nothing, and every `insert` does a lookup, then a second lookup to
insert.

**Recommendation (high; std, my call).** `Set[T]` over `Map[T, void]`.
A `void` layout has no Wasm values, so the values array vanishes by the
layout rule, with no special case. Give the map an
`insert_if_absent`-style internal method so `insert` probes once.
Type-only: yes.

### 7.4 `Deque[T]` And `Heap[T]`

**Current.** Both store `List[T?]` to mark empty slots. The comment in
`lib/std/collections.hd` says `Heap` did so because the prototype had no
`List.pop` yet.

| | Today | Proposed |
| --- | --- | --- |
| `Heap[i32]` | a tag array and a value array; a `match` per comparison | `List[T]`, with `pop` to shrink; one array, no tag |
| `Heap[User]` | one nullable reference array | the same array |
| `Deque[i32]` | two arrays; a tag test per access | the intrinsic `Array[T]` with the same rule `List` uses: slots past the count hold `default(L)`, and std never reads them |
| `Deque[User]` | one nullable array | same |

`Heap.less` calls `self.element(i).cmp(...)` with a `match` per read and
an `Ordering` result. With `List[T]` and inlining, a comparison is two
array reads and the type's own compare.

**Recommendation (high; std).** Rewrite both over plain element storage.
This is std work, not a layout. It wants one compiler hook:
`Array[T]` with unreadable slots past a count, which `List` already
needs internally.

### 7.5 Bytes: `List[u8]` And The Host

**Current.** `List[u8]` is a packed `(array (mut i8))`. Host bytes cross
through the exchange buffer, copied by a generated Wasm loop.

Pilot on V8:

| Path | ns per byte |
| --- | ---: |
| buffer to `(array i8)`, one `i32.load8_u` per byte | 0.4 to 0.8 |
| the same, one `i32.load` per 4 bytes | 0.3 to 0.6 |
| `array.copy` between two GC arrays | 0.4 to 0.8 |
| `(array i8)` to the buffer | 0.3 to 0.4 |
| JS `TypedArray.set` (a `memcpy`) | 0.02 |

So a 10 MB read costs about 5 ms of copying on V8, and `array.copy`
would not be faster. The browser cannot build an `i8` GC array from JS,
and doesn't need to.

**Candidates.**

| Option | Where | Note |
| --- | --- | --- |
| the Wasm loop with wide loads (today, improved) | both engines | 4 or 8 bytes per load |
| host-side bulk fill | wasmtime | the public `ArrayRef` API builds arrays from `Val`s; a byte-slice fill may not exist (unverified); E4 checks |
| parse in place | both | a std reader decodes JSON or text straight from the exchange buffer with linear-memory loads, and copies only the strings it keeps |
| `(array i16)` with JS string builtins | browser | breaks UTF-8 semantics; rejected |

**Recommendation (medium).** The Wasm loop with 8-byte loads on both
engines. Parse in place is the stronger idea for `serde-throughput`: it
reads with wide loads and skips one full copy. It is std work for later,
after E4. Type-only: yes.

## 8. Iterators, Ranges And Closure Specialization

### 8.1 Iterator Chains

**Current std.** `Iterator[T]` is a data type holding `step: fn() -> T?`.
Each adapter (`map`, `filter`, ...) builds a new closure that captures
the previous iterator and the user's closure, plus a new `Iterator`.

For `xs.iter().map(fn x: x + 1).sum()` without specialization:

| Per chain | Per element |
| --- | --- |
| 3 closures, 3 `Iterator` structs, 1 cell for the index: about 7 allocations | 3 `call_ref`s (`sum` to `map`'s step, to the list's step, to the user's closure), 3 casts in callees, 2 `Option` tag tests; 0 allocations |

So the new layouts already remove the prototype's 237 bytes per element.
The `call_ref`s remain. On V8 one monomorphic `call_ref` costs nothing
(the pilot); an 8-target one costs 5 ns. On wasmtime every one is a real
indirect call, and it blocks every loop optimization across it.

**Closure specialization (proposal C), made precise.** The bounded
inliner already lists "closures passed to a known callee". For a chain it
needs four steps, repeated per stage:

1. **Inline** the adapter call (`map`): its body builds a closure and an
   `Iterator` struct.
2. **Scalar-replace** the `Iterator` struct. It is a mutable data value,
   but it does not escape, so its identity is unobservable. Its `step`
   field becomes a local holding a known closure literal.
3. **Devirtualize**: a `call_ref` whose target is a known closure literal
   becomes a direct call of that closure's body, with the environment
   fields as locals.
4. **Inline** that body, then scalar-replace its environment and cells.

Each stage needs one round. A three-stage chain needs three rounds, so
the pass iterates to a fixed point under a round limit.

**Speed.** The chain becomes a counted loop: about 0.3 ns per element on
V8 in the pilot, against 1 to 5 ns with real indirect calls. The `sum`
microbenchmark is 11x Node today; this and counted loops are what bring
it near 1x.

**Size.** Each specialized chain inlines its adapters' bodies at the call
site: about 50 to 150 bytes per stage, so 150 to 450 bytes for three
stages, against about 30 bytes for a call into shared adapters.

**Prior art.** Rust gets the same result from monomorphized iterator
structs plus LLVM inlining. Kotlin's `inline fun` with lambda arguments
does it at the source level, without a budget. MLton's whole-program
defunctionalization is the extreme case. V8 gets it at run time from
speculative inlining, which wasmtime lacks.

**Recommendation (high for the pass; budgets in §9).** Specialize when a
closure literal, or a chain of adapter calls ending in one, reaches a
callee within budget. Stop at 4 rounds or the size budget. Without a
literal, keep the indirect call. Specialization is code-only, so
type-only holds.

### 8.2 Ranges

`for` over an integer range is a counted loop by a lowering rule, with
no range value, as decided. `(0..n).map(f).sum()` goes through the
`Iterable` path and is handled by §8.1. **Recommendation (high).** Keep.

### 8.3 `for` Over A List

`ForList` loops over the backing array with the length captured. With A,
a reference element costs a cast per iteration only where the body uses
the element's exact type. **Recommendation (high).** Keep.

## 9. Size Versus Speed

### 9.1 Policy

1. **Layouts are the same in debug and release.** A layout is part of
   every instance's code; two layouts would double the cache and break
   `release-check-cost` comparisons.
2. **Speed where it is hot, size everywhere else.** Specialize and
   inline only inside loops or for closure literals; everything else is
   a shared, folded call.
3. **Fold always.** Folding costs no speed. Proposal A makes it work for
   references.
4. **Budgets are per function and per program**, so one hot spot cannot
   blow up a module.
5. **Debug and release run the same optimizer** with the same budgets.
   Only checks differ, so `release-check-cost` (debug ≤ 1.3x release)
   holds.

> **Superseded by [tiering.md](tiering.md) (2026-10-07).** Policy 5 no
> longer holds. Layouts stay the same in both pipelines (policy 1), but
> the optimizers differ: the dev pipeline runs almost no passes, and the
> optimized pipeline runs every pass with these budgets.
> `release-check-cost` became `check-cost` and `dev-speed`.

### 9.2 Per-Feature Budgets (proposed, to be tuned by spike 0c)

| Feature | Speed it buys | Size it costs | Budget |
| --- | --- | --- | --- |
| trivial inlining | a call per small callee | none or negative | callee ≤ 8 TIR instructions, no loop |
| bounded inlining | a call, plus later folding | callee size per site | callee ≤ 40 TIR instructions inside a loop, ≤ 15 outside; a function grows at most 2x or 2 KB, whichever is smaller |
| closure specialization | 1 to 5 `call_ref`s per element on wasmtime | 50 to 150 bytes per stage | ≤ 4 rounds; ≤ 512 bytes growth per chain site |
| known-vtable devirtualization | one `call_ref` | none; enables inlining | always |
| scalar replacement | an allocation | usually smaller | always, when the value does not escape |
| proposal A | folds reference instances | negative | always |
| parallel arrays for `multi` lists | an allocation per element | about 25 bytes per slot per list method | only up to the bound of 4 values |
| literal pool | none | negative above 5 bytes per literal | always |
| thunks of the erased ABI | owner accepted slow `dyn` | about 25 bytes each | warn past 200 per method |

**What "size" includes.** E8 reports, per configuration: Wasm bytes,
Cranelift compile time at both optimization levels, V8 Liftoff and
TurboFan compile time, and instantiation time. A budget that buys less
than 5 percent speed on the runtime suite for more than 10 percent size
on the `dead-code` packages is cut.

### 9.3 Effect On The Targets

| Target | Without these changes | With them (estimate) |
| --- | --- | --- |
| `runtime` ≤ 1.5x Node, no case > 3x | `map` at risk above 3x on wasmtime from the hash protocol; chains at risk from `call_ref` | `map` near 2x on wasmtime; chains near 1x; the heap-size pathology removed |
| `allocations` 0 per counted loop, ≤ 1 per chain | chains allocate per chain, not per element; map lookups 3 to 5 per lookup | 0 for both |
| `size-startup-heap` tiny ≤ 2 KB, ≤ 5 ms, heap ≤ 2x Node | short literals cost 3 bytes per byte | pool; hello world needs no pool (a host fast path) |
| `dead-code` ≤ 10 KB per 1,000 lines | list code per reference type | folded by A |
| `serde-throughput`, `text-throughput` ≥ 0.5x Node | byte-at-a-time loops | wide loads at the boundary; parse in place later |
| `long-run-memory` flat | S2 or S3 slices pin sources; heap growth pathology | S1 never pins; heap sized |
| `host-call-overhead` | one copy per byte crossing | unchanged; 0.5 ms per MB on V8 |

## 10. Suspension, Facts, Type Ids And Handles

### 10.1 Suspension Frames

**Current.** One final frame struct per suspending instance, under a
base per result layout, materialized only on the first real wait.

| Axis | Cost |
| --- | --- |
| speed | ready path: a call and a null test; pending path: one allocation per level, once |
| GC | saved locals; dead references cleared at each point |
| size | four functions per suspending instance (`body`, `cold`, `poll`, `cancel`); `poll` and `cancel` are small and fold across equal frame layouts |

**What it misses.** `poll` and `cancel` fold only when frame layouts are
equal, which is rare. A shared `poll` per result layout that calls
`body` through a function reference in the frame's vtable would fold
them all. It costs one `call_ref` per poll, which is cold. E11 measures
the bytes.

**Recommendation (medium-high).** Keep the frames; consider one shared
`poll` per result layout. Type-only: yes.

### 10.2 Facts

**Current.** A mutable global plus an "initialized" flag, filled on first
read.

| Axis | Cost |
| --- | --- |
| speed | per read: `global.get`, a test, a branch; predictable |
| size | one getter (about 30 to 50 bytes) per fact; about 5 bytes per call |

**Recommendation (high).** Inline the fast path at reads inside loops,
and use the null of a nullable reference as the flag when the fact's
layout is one reference, which saves a global. Type-only: not a type
question.

### 10.3 `TypeId`

`TypeId` is opaque data with `Eq`, `Hash` and `Display`
([`trait.typeid.opaque`](../../spec/lang/09-traits.md#r-trait.typeid.opaque)).
**Recommendation (medium).** One immutable constant global per type that
reaches a type-id read, holding a link-time number and the type's name
as a pool literal. Equality compares the numbers; `Hash` writes the name
bytes, so it does not depend on link order. A vtable holds the
reference. Cost: about 15 bytes per type id; zero allocation.

### 10.4 Host Handles

A handle is an `i32` (20 bits of slot, 11 of generation) inside the
provider's data value; debug builds add a closed flag. Free on both axes.
**Recommendation (high).** Keep.

## Ranked Changes

Ranked by expected runtime gain over risk. "Size" is the effect on Wasm
bytes.

| Rank | Change | Speed gain | Size | Risk | Confidence |
| --- | --- | --- | --- | --- | --- |
| 1 | `Hasher` gets fixed-width and string writes (H1), plus known-vtable devirtualization | map lookups lose 3 to 5 allocations and 2 indirect calls; `map` case from about 5x to about 1.5 to 2x Node | about the same | a language-tier trait change (owner) | high for the cause, medium for the gain |
| 2 | one representation per type in every position; box `multi` layouts over 4 values everywhere | removes an allocation per read of a large enum or tuple from a list | smaller for large enums | low | high |
| 3 | settle string slicing: S1 with a spec change, or S2 | S1: fastest everyday strings, no pinning; S2: free slices | S1 smallest | a spec change, or three values per string | medium |
| 4 | closure specialization as a fixed-point inline, scalar-replace, devirtualize pass | chains from 1 to 5 ns per element per stage to about 0.3 ns on wasmtime | +50 to 150 bytes per stage per site | inliner complexity; bounded by budgets | high for the gain |
| 5 | wasmtime GC heap sized from the profile | up to 8.8x on programs with a moderate live set | none | low | medium-high |
| 6 | proposal A: `eqref` storage for type-parameter slots at reference layouts | small loss: a cast per read | large saving: about 1 KB per reference element type | wasmtime cast cost unknown | medium |
| 7 | std collections over raw element storage: `Heap`, `Deque` without `T?`, `Set` over `Map[T, void]`, a compact ordered map | removes a tag array and tag tests; one probe per insert | slightly smaller | std work only | high |
| 8 | literal pool with one data segment | neutral | 3x smaller literals | low | medium-high |
| 9 | size guards: inliner budgets, thunk warning, `--size-report` | none | caps growth | low | high |
| 10 | 8-byte loads at the host boundary; parse in place later | 1.5 to 2x on the copy loop | small | low | medium |

## Spike 0c: Representation Benchmarks

**Setup.** Hand-written Wasm (WAT encoded by `wasm-tools` or Binaryen
without optimization), run on the pinned wasmtime (build-order.md §22.2)
with Cranelift at `Speed` and at the debug level, and on V8 through
Node and Chrome stable. Each case times its kernel only: 3 warm-ups, then
15 runs, reporting median, p10 and p90. On V8 run each case twice: as
is, and with an 8-target `call_ref` variant where the case has a call,
so V8's speculative inlining does not hide wasmtime's cost. Record the
machine, load and versions in every report.

**For every experiment, also report size:** module bytes, code bytes per
function, Cranelift compile time (both levels), V8 Liftoff and TurboFan
compile time (`--liftoff-only` and `--no-liftoff`), and instantiation
time.

**E0. Allocation and collection cost (prerequisite).**
- Program: allocate 10M structs of 2, 4 and 8 `i32` fields in a loop,
  keeping none; then again with a live set of 1, 10 and 50 MB of linked
  structs.
- Metric: ns per allocation; collections; total time. On wasmtime, also
  whether `struct.new` is inline or a libcall (from the compiled code).
- Decision: the allocation cost used to set the bounds in E2 and E3.

**E1. Exact element types against `eqref` plus casts.**
- Program: arrays of 1M references to `struct {i32, i32}`, typed
  `(array (mut (ref null $P)))` and `(array (mut eqref))`. Kernels: sum a
  field (read and cast), overwrite every slot (write), and `push` 1M
  elements into a list with growth.
- Metric: ns per element; module bytes for 10 list methods at 12
  reference element types, both ways.
- Decision: adopt A when the wasmtime read penalty is ≤ 1 ns per
  element and ≤ 5 percent on the `sort` and `map` microbenchmarks
  rewritten with reference elements.

**E2. Enums in a list: parallel arrays against boxes.**
- Program: enums of 1, 4 and 12 `i32` payload values, two variants, as
  parallel arrays and as one immutable struct per element. Kernels: build
  1M with `push`; sum read-only; filter into a new list.
- Metric: ns per element per kernel; peak heap; code bytes of `push`.
- Decision: the bound is the largest payload count where parallel arrays
  win the read-only kernel by more than 10 percent on wasmtime and
  their `push` stays under 256 bytes. Default 4 if the data is unclear.

**E3. `map(fn x: x + 1).sum()` with and without closure specialization.**
- Program, unspecialized: the std shape. An `Iterator` struct per stage
  holding a closure; `sum` calls `step` through `call_ref`; `map`'s step
  calls the previous step and the user closure. Program, specialized:
  the counted loop that the pass produces. Both over 0..10M and over a
  `List[i32]` of 10M.
- Metric: ns per element; allocations per chain; code bytes of both.
- Decision: the specialization pass is required for release if the
  unspecialized form exceeds 3x Node's `for` loop on wasmtime. The size
  budget of §9.2 is confirmed if the specialized form is within 512
  bytes of growth.

**E4. Host bytes into `(array i8)`.**
- Program: 1 MB and 10 MB in the exchange buffer, copied into a new
  `(array i8)`: one byte per load; 4 and 8 bytes per load. On wasmtime,
  also a host-side fill through the public API, if one exists, and the
  reverse direction. In the browser, the same Wasm loop after JS
  `TypedArray.set` into the buffer.
- Metric: ns per byte; total ms per MB.
- Decision: host-side bulk only if it is 4x faster than the 8-byte loop
  on wasmtime and the API exists. Otherwise the 8-byte loop on both.

**E5. Map lookup with today's hash protocol against H1.**
- Program: a compact ordered map of 100k `i64` keys and of 100k
  16-byte string keys. Hashing as std does today (a `List[u8]` per
  write, `dyn Hasher` through `call_ref`, FNV in 32-bit halves), against
  H1 (`write_u64`, `write_str`, direct calls, one multiply-rotate mix).
  Also the string-key case with a cached hash in a header, to price it.
- Metric: ns per insert and per lookup; allocations per lookup; Node's
  `Map` on the same keys.
- Decision: take H1 to the owner with the numbers. A cached hash is
  dropped unless it wins more than 20 percent on string lookups with
  fresh keys.

**E6. String slicing: S1, S2, S3.**
- Program: tokenize a 1 MB ASCII text into words, keeping every token in
  a list; then look each token up in a map. Three representations.
- Metric: ns per token; allocations; peak heap; retained heap after the
  source text is dropped.
- Decision: input to open question 1. S2 wins only if it is more than
  20 percent faster on the tokenizer and not slower on the map.

**E7. Byte-at-a-time text loops.**
- Program: `find` of a byte, `split` on a byte, and equality of 64-byte
  strings, over `(array i8)` at 1 MB.
- Metric: MB/s against Node's `indexOf` and `split`.
- Decision: if below 0.5x Node, open a study of wide-read string helpers.

**E8. Inlining and specialization budgets, size against speed.**
- Program: the runtime microbenchmarks rewritten by hand at three
  inlining levels: trivial only, the §9.2 budgets, and no limit.
- Metric: run time, Wasm bytes, compile times, instantiation time.
- Decision: keep the §9.2 budgets unless the next level up buys 5
  percent speed for under 10 percent size.

**E9. wasmtime heap sizing.**
- Program: E0's live-set program with initial heaps of 0, 16, 64 and
  256 MiB.
- Metric: collections, total time, peak RSS.
- Decision: the default initial heap for `hd run`.

**E10. Literals.**
- Program: 10, 1,000 and 10,000 distinct literals of 8, 32 and 200 bytes,
  each used once in a loop of 1M: `array.new_fixed` globals, the lazy
  pool, and the eager pool.
- Metric: module bytes, instantiation time, ns per use.
- Decision: the pool form, and the size below which `array.new_fixed`
  stays.

**E11. Suspension `poll` sharing.**
- Program: 50 suspending functions with distinct frames, with a `poll`
  per frame, and with one shared `poll` per result layout.
- Metric: code bytes; ns per poll.
- Decision: share `poll` if it saves more than 2 KB at under 5 ns per
  poll.

## Open Questions For The Owner

1. **String slicing.** The spec says `slice` takes constant time and
   shares bytes. The layout copies. Recommendation: change the spec so
   `slice` copies the slice's bytes (S1), as Java does, and keep `string`
   one array. Alternative: keep the rule and make `string` three values,
   as Go does (S2). E6 gives the numbers.
2. **The `Hasher` trait.** `Hasher.write(List[u8])` forces an allocation
   per integer hashed and a full copy per string hashed.
   Recommendation: add `write_u8`, `write_u16`, `write_u32`,
   `write_u64` and `write_str` with default bodies that write exactly the
   bytes of today's table, so `hash_of` results do not change. Keep
   `mut dyn Hasher`, and let the compiler devirtualize known vtables.
3. **The value bound.** Enums, `Result` and tuples over 4 Wasm values
   are boxed in every position, not only in locals. Recommendation: yes,
   with E2 setting the number.
4. **Erased storage (A).** Store type-parameter slots at reference
   layouts as `eqref`. Recommendation: yes if E1 passes; it is the main
   size lever for generic code.

Questions I settled myself, per the standing rules: `Set` over
`Map[T, void]`, `Heap` and `Deque` without `T?` (std calls); the literal
pool and the heap default (compiler and host settings).

## Sources

- Bytecode Alliance, GC and exceptions in Wasmtime: <https://bytecodealliance.org/articles/wasmtime-gc>
- wasmtime copying collector heap growth, a downstream report: <https://github.com/voces/vl/issues/3075>
- V8, porting GC languages to WasmGC (J2Wasm 1.9x from `wasm-opt`, 30 percent from speculative inlining): <https://v8.dev/blog/wasm-gc-porting>
- V8, speculative optimizations for Wasm: <https://v8.dev/blog/wasm-speculative-optimizations>
- Nick Fitzgerald, the Cranelift inliner: <https://fitzgen.com/2025/11/19/inliner.html>
- Jérôme Vouillon, wasm_of_ocaml: <https://cambium.inria.fr/seminaires/transparents/20231213.Jerome.Vouillon.pdf>
- Scala.js 1.19.0: <https://www.scala-js.org/news/2025/04/21/announcing-scalajs-1.19.0/>
- Scala.js, JS string builtins option: <https://github.com/scala-js/scala-js/issues/4994>
- dart2wasm string arrays, WebAssembly/gc #259: <https://github.com/WebAssembly/gc/issues/259>
- Sébastien Deleuze, Kotlin/Wasm strings: <https://seb.deleuze.fr/introducing-kotlin-wasm/>
- MoonBit, JS string builtins: <https://www.moonbitlang.com/blog/js-string-builtins>
- Andy Wingo, requiem for a stringref: <https://wingolog.org/archives/2023/10/19/requiem-for-a-stringref>
- .NET shared generics: <https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/shared-generics.md>
- Go strings: <https://go.dev/blog/strings>
- Raymond Hettinger, compact dictionaries: <https://mail.python.org/pipermail/python-dev/2012-December/123028.html>
- Jason Orendorff, deterministic hash tables: <https://wiki.mozilla.org/User:Jorend/Deterministic_hash_tables>
- Pilot measurements: hand-written Wasm encoded by Binaryen 132 and run
  on Node 24.19 (V8), 2026-10-07, on a machine with a load average of 26
  to 52. Not committed; spike 0c replaces them.
