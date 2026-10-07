# New Compiler: Lowering Catalog

Part of the [compiler design](README.md).

Status: Design, 2026-10-07. It merges the two representation studies,
[representation-runtime.md](representation-runtime.md) (runtime side) and
[representation-compile.md](representation-compile.md) (compile side),
with the orchestrator's calls on their rankings. Items still pending a
measurement of spike 0c are written as "default X, decided by Y". The
layout rules themselves live in [wasm-layout.md](wasm-layout.md), the
emission rules in [codegen.md](codegen.md). This file gives one entry per
data type and per syntax form, with its cost on both axes.

**Tiers (owner, 2026-10-07; [tiering.md](tiering.md)).** "We need very
fast dev time compilation that crappy output is allowed, and blazingly
fast/lean artifact for release build, which can be slow." Optimizations
are composable single passes. This file fixes **representation**
(layouts, the literal pool, type-only numbering), which is the same in
both pipelines, and a **baseline emission** per entry: the code that one
unoptimized walk writes. Every optimization is a separately named pass
in [Optimization Passes](#optimization-passes), with its inputs, outputs
and cost. The **dev pipeline** (`hd run`, `hd test`, the REPL) runs the
baseline plus `const-fold`, `fold` and, if spike T4 keeps it,
`inline-trivial`. The **optimized pipeline** (`--release`) runs every
pass. Merging runs in both; Binaryen is decided after spike T2.

## Changes In This Pass

1. **Stable, type-only numbering** (compile study, rank 1). No code body
   holds a program-wide dense number except a direct call's function
   index, which wasmtime's per-function cache abstracts. Panic stubs take
   no site immediate: the host maps the stub's caller by code offset.
   Context key ids and `TypeId` values are content hashes. Lazily
   initialized globals are read through getter functions: one per fact,
   and one per module for that module's table of pooled literals
   ([Literals](#literals)). See [Numbering](#numbering) and codegen.md §13.10.
2. **One representation per type in every position.** A type has the same
   Wasm values in a local, a parameter, a field and an array element.
   Value layouts over the boxing bound (default 4 Wasm values, decided by
   E2) are one immutable box everywhere, fields and arrays included.
   Payload slots of different variants are shared by Wasm type
   (wasm-layout.md §15.1, §15.2).
3. **Erased storage for reference type arguments (A1)**, applied at
   collection: default adopted, decided by E1 and S1. A type-parameter
   slot at a reference layout is stored as `eqref`, and move-only generic
   bodies get one instance per representation class (codegen.md §13.2).
4. **Collection skips callees that are always inlined** (compile study,
   change 2), unless one is used as a value or in a vtable slot. It
   applies in any pipeline that runs the `inline-trivial` pass.
5. **Optimizations are named passes** ([Optimization Passes](#optimization-passes)):
   `inline-trivial`, `inline-bounded`, `escape`, `scalar-replace`,
   `devirt-vtable`, `devirt-closure`, `hoist-constants`, `cse-getters`,
   `const-fold`, `fold` and `leb-shrink`. Closure specialization is a
   schedule of them, repeated once per stage under a size cap per caller.
6. **Merging** uses a worklist for rounds after the first (codegen.md
   §13.7). It runs in both pipelines; the dev pipeline's Cranelift
   setting is decided by spike T1 (engines-and-test-runner.md §18.1,
   §18.6).
7. **Per-program packs** for code entries and Cranelift entries, read in
   one batch (cache.md §5.2, §5.4).
8. **`--filter` builds only the selected test cases**
   (engines-and-test-runner.md §19.1).
9. **A compact `hd.names` section** replaces the standard `name` section
   in release builds (wasm-layout.md §15.5).
10. **Constant strings:** one deduplicated passive data segment, a lazy
    literal table per module behind one getter per module, a host fast path that copies a literal
    from the segment into the exchange buffer with `memory.init`, and
    `array.new_fixed` only for literals of at most 4 bytes
    (default, decided by E10).
11. **Std collections over plain element storage:** `Heap` over
    `List[T]`, `Deque` over `Array[T]` with a count, `Set` over
    `Map[T, void]`, and a compact insertion-ordered map. This pass
    designs their representation and does not edit `lib/std`.
12. **The `Hasher` trait** gains `write_u8`, `write_u16`, `write_u32`,
    `write_u64` and `write_str` with default bodies that write today's
    bytes. That is a std change and a change to the trait shown in
    [Hashing](../../spec/lang/09-traits.md#hashing); this pass only notes
    it.
13. **The wasmtime GC heap is sized from the profile** (default 64 MiB
    initial for `hd run`, decided by E9; runtime-and-host.md §17.8).
14. **Size guards:** inliner budgets, a warning past 200 thunks for one
    method, and `hd build --size-report` (codegen.md §12.8).
15. **A "Size Versus Speed Policy"** section in codegen.md (§12.8): what
    each pass buys and costs, the budgets, and the size guards. The
    budgets apply in the optimized pipeline.
16. **Smaller fixes found in this pass:** trait-value payloads are
    `eqref`, not `anyref`, since `is` lowers to `ref.eq`; `Array[T]` slots
    past the count use the defaultable form; `Array[void]` has no Wasm
    array; a `dyn` value and a closure get a null niche for `T?`.

## Conventions

Each entry gives:

- **Wasm:** the Wasm GC types and the shape of the emitted code, as a
  small WAT-like snippet. `$X` names a canonical type; `dflt(L)` is the
  defaultable form of layout `L` (wasm-layout.md §15.2).
- **Speed:** allocations, indirect calls and casts on the hot path of
  the baseline emission. A figure that needs a pass names it ("with
  `scalar-replace`").
- **Size:** bytes per use or per instance, from the runtime study's pilot
  counts (Binaryen-encoded, unoptimized) where it has one; otherwise an
  estimate marked as one.
- **Engines:** what differs between wasmtime (Cranelift, copying
  collector, no indirect-call inlining) and V8 (speculative `call_ref`
  inlining, generational collector).
- **Boundary:** how the value crosses to the host or JS, where it can.
- **Type-only:** whether the layout depends only on the type's
  definition (and the definitions its fields name), never on uses or the
  program. Every entry must say yes, or say why the question does not
  apply. A cached instance is then valid in every program that reaches it.
- **Pending:** the deciding experiment of spike 0c, where one is open.

The **bound** below is the boxing bound of change 2: at most 4 Wasm
values after slot sharing, the tag included, by default. A boxed value
counts as one value toward an enclosing layout's bound.

### Pending Decisions

| Item | Default | Decided by |
| --- | --- | --- |
| A1, `eqref` storage for type-parameter slots at reference layouts, at collection | adopted; A2 (erased function types) not adopted | E1: adopt unless the wasmtime read penalty is over 1 ns per element or over 5 percent on the `sort` and `map` microbenchmarks with reference elements. S1 records the compile-side gain |
| the boxing bound | 4 Wasm values after slot sharing, tag included | E2: the largest payload count where parallel arrays win the read-only kernel by more than 10 percent on wasmtime and `push` stays under 256 bytes |
| string slicing | decided (owner, 2026-10-07): Go-style shared views, `(ref $bytes, i64 span)` ([`string`](#string)) | E6 measures the tokenizer and map cases against Node; no decision rests on it |
| dev Cranelift setting | `None` with the single-pass allocator | spike T1 of [tiering.md](tiering.md#72-spike-0c-additions): falls back to `None` with backtracking, then `Speed`, if it fails a runtime case, saves too little or breaks `dev-speed` |
| inliner caps | runtime study §9.2 budgets (codegen.md §12.8), as pass parameters | S6 sets the caller cap below the size where Cranelift time per byte doubles; E8 keeps the budgets unless the next level buys 5 percent speed for under 10 percent size |
| constant globals read directly | direct `global.get`, globals ordered by content key | S2: any perturbation under 95 percent hits moves that global kind behind getters |
| the `array.new_fixed` threshold and eager pool fill | 4 bytes; lazy per-module tables behind one getter per module ([Literals](#literals)) | E10; S2 says whether the getter may be inlined |
| the initial GC heap of `hd run` | 64 MiB | E9 |
| host byte copies | a Wasm loop with 8-byte loads | E4: host-side bulk fill only if 4x faster and the API exists |
| a shared `poll` per result layout | one `poll` per frame | E11: share if it saves over 2 KB at under 5 ns per poll |
| the `Hasher` change's gain | adopted (orchestrator) | E5 records it |

## Numbering

Every index space of the module, and what a code body holds for it:

| Index space | A body holds | Order at link | Stable under an edit elsewhere |
| --- | --- | --- | --- |
| defined functions | `call` index | imports, runtime helpers, then representatives by instance key | yes: the Cranelift cache abstracts call targets |
| `ref.func` targets | the function index | as above | measured by S2; if it misses, one immutable `funcref` global per closure code |
| Wasm types | type immediates in `struct.new`, `ref.cast`, `array.new`, block types | Kahn order, sorted ready set (§15.3) | measured by S2; fallback a per-type helper call |
| fixed globals: the runtime's | `global.get` | first, in a fixed order | yes |
| module literal tables | nothing; only the module's getter reads one | after the fixed globals, by module path | readers yes; a getter's body changes when a module is added before it |
| immutable constants: vtables, capture-free closures, witnesses, type ids, member handles, singletons, short literals | `global.get` | by kind, then content key | measured by S2 (default direct) |
| module storage | `global.get`, `global.set` | by module path, then binding index | shifts only when a top-level binding is added or removed |
| lazy globals: module literal tables, facts | a `call` of the getter; a literal use adds its module-local number | the getter holds the number | yes; a module-local number changes only with that module's literals |
| data segments | only the module literal getters, span functions and the fill helper name segment 0 | one deduplicated segment, then each module's index area | yes |
| panic sites | nothing | `hd.sites` by function and code offset | yes |
| context key ids, `TypeId` | `i64.const` of a content hash | none | yes |
| witness fields `f_I` | `Field` relocation | per program | no: a known exception, erased bodies only |

"Append-friendly order" for lazy globals would need a numbering kept from
earlier builds. That makes the bytes depend on build history, which
§15.8 forbids. So getters are the default, not a fallback.

Stable numbering serves the per-function Cranelift cache, so it matters
most to the dev pipeline. A release pipeline that runs `wasm-opt`
afterwards may renumber freely: its output is not what the cache keys.

## Baseline Emission

The baseline is what one walk over an instance's TIR writes with no
optimization pass: the representation of every value, and a direct
translation of every instruction.

- Every direct call stays a `call`; every `CallDyn` and `CallValue` is a
  `call_ref`.
- Every construction allocates: a closure, its cells, an `Iterator`, a
  box over the bound, a `Range` value.
- Every literal read calls its module's literal getter, and every fact
  read its fact's getter.
- Checks follow the spec's profile rules, which are semantics, not
  optimization: overflow checked in debug and test, wrapping in release.
- `for` over a range, a list or a map is a counted loop
  ([Loops](#loops)). This is part of the baseline: it is no harder to
  emit than an iterator call, and it allocates nothing.
- A1, slot sharing, the bound and the literal pool are representation, so
  they are part of the baseline too.

## Optimization Passes

Each pass is a single step with stated inputs and outputs, so the
pipelines of [tiering.md](tiering.md) compose them. "Compile cost" is
the pass's own time; "Cranelift" is its effect on the engine's compile
time through code size. Analyses and decisions write side tables that
steer the one emission walk (codegen.md §12.1); local rewrites are fused
into the walk; no pass materializes instance IR (tiering.md §6). Every
pass keeps a function's output a function of its own code key's inputs:
budgets are local to the caller.

| Pass | Input | Output | Compile cost | Speed it buys | Size effect |
| --- | --- | --- | --- | --- | --- |
| `const-fold` | the instance's TIR, constants | folded constants, dead branches removed | one walk; near zero | small | negative |
| `inline-trivial` | a call; the callee's TIR and `inline_summary` (at most 8 instructions, no loop, no suspension, no closure) | the callee's body in place | negative: fewer calls emitted; enables the collection skip, which removes 15 to 25 percent of instances | one call per small callee | none or negative |
| `inline-bounded` | a call; the callee's TIR; the caller's size so far; budgets | the callee's body in place | the inlined TIR is walked per site; Cranelift time can grow super-linearly in a large caller, hence the caller cap | a call, and the chance for the passes below | up to the caller cap (default 2x or 2 KB) |
| `escape` (analysis) | the instance's TIR after inlining | an escape bit per allocation; live sets | linear in instructions times loop depth | none by itself | none |
| `scalar-replace` | escape bits | non-escaping closures, cells, boxes, `Iterator` and other data values as locals | linear | an allocation each; 0 per chain with the schedule below | usually negative |
| `devirt-vtable` | a `CallDyn` whose vtable operand is a known constant global | a direct `call` | linear | one `call_ref`; on wasmtime, enables inlining (`mut dyn Hasher` above all) | none |
| `devirt-closure` | a `CallValue` whose closure is a known literal after `scalar-replace` | a direct call of the closure's body, its environment fields as locals | linear | one `call_ref` and one cast | none |
| `hoist-constants` | a box, enum or tuple construction whose payloads are constants | an immutable constant global | linear | an allocation per evaluation (`.Err(ParseError.Empty)`) | a global per constant |
| `cse-getters` | getter calls (literals, facts) in a body | one call per dominating path; pure literal getters hoisted out of loops | linear | a call per repeated read | negative |
| `fold` (link) | the program's code entries | classes of byte-identical functions, aliases in `hd.folds` | one hash per body, a worklist after round one; under 1 ms at 10k lines | none | 3 percent of functions under exact types, 15 to 25 percent with A1; Cranelift time saved in proportion |
| `leb-shrink` (link) | padded 5-byte LEB immediates | minimal LEBs, with offset maps for sites and lines | linear | none | negative, several percent |

**Closure specialization** is a schedule, not a pass:
`inline-bounded`, `escape`, `scalar-replace`, `devirt-closure` and
`devirt-vtable`, then `inline-bounded` again, one round per stage of an
iterator chain, at most 4 rounds and at most 512 bytes of growth per
chain site by default (codegen.md §12.6). It turns
`xs.iter().map(fn x: x + 1).sum()` from about 7 allocations per chain
and 3 `call_ref`s per element into a counted loop: about 0.3 ns per
element on V8, against 1 to 5 ns with real indirect calls. E3 says
whether the optimized pipeline needs it (yes if the unspecialized form
exceeds 3x Node's `for` loop on wasmtime).

**Hasher fast paths** are a std change plus passes, not a pass of their
own: the fixed-width `Hasher` writes remove the allocations in std's
`Hash` impls in every pipeline; `devirt-vtable` and `inline-bounded` then
remove the indirect calls and inline the mix where they run.

**Not designed here:** bounds-check hoisting and loop-invariant code
motion beyond `cse-getters`, which the engines do in part; whole-program
passes, which would break the caller-local rule; and `wasm-opt` in
release, which spike T2 decides.

## Data Types

### Scalars: Integers, Floats, `bool`

```wat
;; i8 i16 i32 u8 u16 u32 usize bool -> i32;  i64 u64 -> i64;  f32 f64 -> f32 f64
(struct (field $flag (mut i8)) (field $n (mut i16)) (field $x (mut i32)))   ;; packed in fields
(array (mut i8))                                                           ;; List[u8], List[bool]
(i32.add (local.get $a) (local.get $b))
(struct.get_s $T $n (local.get $t))   ;; i16 field, sign-extended; get_u for u16
```

- **Speed:** free. Packed reads cost a sign or zero extension.
- **Size:** none beyond the instruction. A sub-word store needs no mask:
  `struct.set` on an `i8` field truncates.
- **Engines:** both engines treat packed fields alike. The debug-only
  `i64` multiplication check divides back, about 20 cycles, the one real
  debug cost.
- **Boundary:** a scalar host parameter or result is a Wasm parameter or
  result. Floats cross as raw IEEE bits.
- **Erased** (`dyn`, `Any`, an open value): `bool`, `char` and integers of
  at most 16 bits are `i31ref`; wider ones are `$Box_i32`, `$Box_i64`,
  `$Box_f32` or `$Box_f64`, one allocation of 16 to 24 bytes.
- **Type-only:** yes. The class depends on the width alone.

### `char`

```wat
;; i32 holding a Unicode scalar value; (mut i32) in fields; i31ref when erased
```

- **Speed:** free. A conversion from an integer checks the scalar range
  and the surrogate gap.
- **Size:** none.
- **Boundary:** an `i32`; inside a string it is UTF-8 bytes.
- **Engines:** no difference.
- **Type-only:** yes.

### `void`, `()` And `never`

```wat
;; no Wasm values; a void result is an empty result list
;; Array[void] has no Wasm array: one shared empty constant struct
```

- **Speed, size:** nothing. A `List[void]` or `Map[K, void]` stores only
  its count, which makes `Set[T]` free of a value array.
- **Erased:** `ref.null any`.
- **Engines:** no difference.
- **Type-only:** yes.

### `string`

Go-style shared views (owner, 2026-10-07): a string is `(array, start,
len)`, and `slice` and `s[a..b]` are O(1) and share bytes
([`module.string.slice.shared`](../../spec/lang/10-modules.md#r-module.string.slice.shared)).
The layout and its table by position are in wasm-layout.md §15.2.

```wat
(type $bytes (array i8))                      ;; immutable, valid UTF-8 in every viewed range
;; a string: two values (ref $bytes) and i64 span = len << 32 | start
(i32.wrap_i64 (i64.shr_u (local.get $span) (i64.const 32)))     ;; len()
;; s[i]: i < len, else index-out-of-bounds; then bytes[start + i]
(array.get_u $bytes (local.get $b) (i32.add (i32.wrap_i64 (local.get $span)) (local.get $i)))
;; s[a..b]: a <= b <= len and both scalar boundaries, then a new span; no allocation
(i64.or (i64.shl (i64.extend_i32_u (i32.sub (local.get $hi) (local.get $lo))) (i64.const 32))
        (i64.extend_i32_u (i32.add (i32.wrap_i64 (local.get $span)) (local.get $lo))))
```

- **Speed:** a slice allocates nothing: a few ALU instructions and the
  checks. `len()` is two instructions; a byte read adds one `add` to the
  start, and a counted loop hoists the unpacking. Equality compares the
  lengths, returns early when both views share one array and one start
  (`ref.eq`), and otherwise compares bytes one per step. Every search
  reads one byte per step.
- **Size:** 12 bytes per string at rest in a field or a list element
  (a 4-byte reference plus the `i64`), with no header object. Each string
  argument or result is two Wasm values.
- **The candidates** (V8 figures from the runtime study §6.2; "alloc" is
  one GC allocation of a small struct, about 4 to 7 ns):

  | | `(ref, i64 span)` (chosen) | `(ref, i32, i32)` | one box `{bytes, start, len}` | values in locals, box in fields and arrays |
  | --- | --- | --- | --- | --- |
  | slice | free | free | one alloc, 3.6 to 6.8 ns | free in locals; one alloc per store into a field or list |
  | new string (concat, builder, host) | one array | one array | one array and one box | one array; one box per store |
  | `len()`, start | 2 and 1 ALU instructions | a local | a field load | as the chosen form in locals, a load in fields |
  | field, list element | 12 bytes | 12 bytes | 4 bytes plus a 16 to 24 byte box per distinct string | 4 bytes plus the box |
  | values toward the bound | 2 | 3 | 1 | 1 in fields, 2 in locals |
  | `enum Token: Num(i64) \| Ident(string) \| Op(char) \| End` | tag, `i64`, ref, `i32`: 4, unboxed | 5: boxed, one alloc per token | 4, unboxed | 4 |
  | `(string, string)` (a `Map[string, string]` entry) | 4, unboxed | 6: boxed | 2 | 2 |
  | erased (`dyn`, `Any`) | one box | one box | free upcast | free upcast |
  | under A1 | its own class | its own class | folds with references | mixed |
  | one representation per type | yes | yes | yes | no |

- **Engines:** both engines run 64-bit integer operations natively. The
  explicit length check replaces the engine's array bounds check, which
  still guards the array.
- **Boundary:** raw UTF-8 bytes of the viewed range through the exchange
  buffer, with no conversion
  ([`types.string.host-bytes`](../../spec/lang/04-type-system.md#r-types.string.host-bytes)),
  copied by a generated Wasm loop with 8-byte loads (default, E4). A
  string from the host is a new exact-size array viewed from start 0.
- **Retention:** a small slice keeps its whole backing array alive, as
  in Go. A known risk, watched by `long-run-memory`; no mitigation in the
  first release.
- **Type-only:** yes.
- **Pending:** E6 measures the tokenizer and map cases; E7 the per-byte
  search cost.

### Bytes: `List[u8]`

```wat
(type $Arr_u8 (array (mut i8)))
(type $List_u8 (struct (field $len (mut i32)) (field $data (mut (ref $Arr_u8)))))
```

- **Speed:** packed, one byte per element. A host read of 10 MB costs
  about 5 ms of copying on V8 (0.4 to 0.8 ns per byte, one byte per load;
  0.3 to 0.6 with wider loads). `array.copy` is no faster.
- **Size:** a `List` instance at a scalar class; folds with `List[i8]`.
- **Engines:** the browser cannot build a GC byte array from JS and does
  not need to: the Wasm loop copies from the exchange buffer. A
  wasmtime host-side bulk fill may not exist in the public API; E4 checks.
- **Boundary:** length then bytes (§17.4). Parsing in place from the
  exchange buffer is the later step for `serde-throughput`.
- **Type-only:** yes.
- **Pending:** E4.

### Tuples, Narrow And Wide

```wat
;; (i32, f64): two values in locals and results; two fields in a struct
(func $pair (result i32 f64) ...)
;; List[(i32, f64)]: a structure of arrays
(type $Arr_i32_f64 (struct (field $a0 (mut (ref $Arr_i32))) (field $a1 (mut (ref $Arr_f64)))))
;; over the bound, e.g. (i64, i64, i64, i64, i64): one immutable box everywhere
(type $Tup5 (struct (field i64) (field i64) (field i64) (field i64) (field i64)))
```

- **Speed:** within the bound, free: no allocation, values on the stack.
  Over it, one allocation per construction in the baseline; none with
  `hoist-constants` (constant payloads) or `scalar-replace` (no
  escape). Reading a boxed
  tuple from a list is a load, never an allocation, because the list
  stores the same box.
- **Size:** about 2 to 4 bytes per extra value per pass-through within
  the bound. A `List` method over a tuple of `k` values costs about
  `60 + 25k` bytes, one array per value.
- **Engines:** both support multi-value results. Wide multi-value
  returns cost register pressure on Cranelift; the bound keeps them small.
- **Boundary:** elements in order (§17.4).
- **Type-only:** yes. The layout reads the element layouts only.
- **Pending:** the bound, E2.

### `data` Types, Mutable And Readonly Views

```wat
(type $Point (struct (field $x (mut i32)) (field $y (mut i32))))
(struct.new $Point (local.get $x) (local.get $y))
(struct.get $Point $x (local.get $p))
(struct.set $Point $x (local.get $p) (local.get $v))     ;; needs a mut view; checked statically
;; data Empty: pass  -> one immutable constant global, its canonical identity
```

- **Speed:** one allocation per construction. A field read is one
  `struct.get`; on wasmtime, base plus index plus offset.
- **Views:** `mut T` and readonly `T` have one layout. A view is a
  checker fact, and its coercion emits nothing.
- **GC:** header plus fields; each reference field is one pointer the
  copier follows. No class-id field: hd dispatches through trait values,
  so it saves 4 bytes per object against dart2wasm or Kotlin/Wasm.
- **Size:** `struct.new` with its arguments; one type entry per distinct
  field shape, shared structurally (§15.3), so instances over two data
  types with equal fields fold.
- **Recursive data:** an optional self field is `(ref null $T)`; a field
  of type `T` itself must be reachable through an optional or a
  collection to be constructible.
- **Field defaults and copy-update:** a default is a body run per
  construction that omits it. A copy-update `p { x: 1 }` is one
  `struct.new` of the new field and the source's other fields.
- **Boundary:** boundary-safe data crosses as its fields in declaration
  order (§17.4).
- **Engines:** wasmtime's copying collector copies every live struct per collection, so allocation counts matter more there; V8's young generation copies survivors only. On wasmtime a reference is a 32-bit index plus the heap base.
- **Type-only:** yes. `layout_hash(T)` covers the fields' layouts.

### Embedded Parts

```wat
(type $Timestamps (struct (field $created_at (mut i64)) (field $updated_at (mut i64))))
(type $Post (struct (field $title (mut (ref $bytes))) (field $title_span (mut i64)) (field $Timestamps (ref $Timestamps))))
(struct.get $Timestamps $created_at (struct.get $Post $Timestamps (local.get $post)))
```

- **Speed:** one extra load per part access; one extra allocation per
  construction and per copy-update that copies the part.
- **Size:** none beyond the field.
- **Why not inline:** a part read yields the part itself, which a
  `mut self` method may store elsewhere, and Wasm GC has no interior
  references. An inline layout would copy on every escaping read and
  lose later writes through the copy. Inline parts are impossible on
  Wasm GC; a native backend with interior pointers may revisit them.
- **Engines:** the extra load is one more dependent load on both; V8 may hoist it in loops.
- **Type-only:** yes. The part's struct depends on the part's fields.

### Payloadless Enums

```wat
;; Ordering: i32 tag in locals; i8 in fields and arrays (i16 past 256 variants)
(br_table $less $equal $greater (local.get $tag))
```

- **Speed, size:** free; a `match` is one `br_table`.
- **Shared constructor data:** a per-variant constant read by tag from an
  immutable global table, never stored in the value.
- **Boundary:** the variant index.
- **Engines:** no difference.
- **Type-only:** yes.

### Enums With Payloads Within The Bound

Slot sharing first: payload fields of different variants with the same
Wasm type share a slot; reference fields share reference slots, typed
exactly when every variant's field in that slot has one reference type,
and `eqref` with a cast after the tag test otherwise.

```wat
;; enum Token: Num(i64) | Ident(string) | Op(char) | End
;; -> (i32 tag, i64 slot0, i32 slot1, (ref null $bytes) slot2): 4 values;
;;    slot0 holds Num's i64 or Ident's string span, slot1 Op's char
(func $next (result i32 i64 i32 (ref null $bytes)) ...)
;; match: br_table on the tag local; .Ident(name) is (ref.as_non_null slot2, slot0)
```

- **Speed:** no allocation. A `match` tests a local; a payload read is a
  local read, plus `ref.as_non_null` or a `ref.cast` on a shared `eqref`
  slot. In a list, parallel arrays: 2.8 to 4.6 ns per element to build
  and sum on V8, against 44 to 89 ns for one box per element.
- **Size:** a 3-value enum passed through: 28 bytes (35 boxed). A 3-arm
  `match`: 44 bytes (68 boxed). A list method: about `60 + 25k` bytes for
  `k` slots.
- **Constants:** a value whose payloads are constants is constants on
  the stack; it never allocates.
- **Boundary:** variant index, then the payload fields (§17.4).
- **Engines:** multi-value results and wide locals are native to both; Cranelift spills past its register budget, which the bound limits.
- **Type-only:** yes. Slot sharing reads the enum's own variants and the
  layouts of their payload types, transitively through value payloads; a
  data payload stops the chain.
- **Pending:** the bound, E2.

### Enums Over The Bound, And Self-Recursive Enums

```wat
;; flat box (payload fields at most 4 in total): tag plus the union of fields
(type $Shape (struct (field $tag i8) (field $f0 f64) (field $f1 f64) (field $f2 f64) (field $f3 f64)))
;; subtype box (more fields): a base with the tag, one final subtype per payload variant
(type $Ev (sub (struct (field $tag i8))))
(type $Ev_Click (sub final $Ev (struct (field $tag i8) (field $x i32) (field $y i32) ...)))
(ref.cast (ref $Ev_Click) (local.get $e))      ;; once per matching arm
```

- **Speed:** one immutable allocation per construction in the baseline.
  `hoist-constants` makes a box with constant payloads a constant
  global; `inline-bounded` then `scalar-replace` make
  `match parse(s): .Ok(v) => ...` allocate nothing. Reading
  one from a list is a load, never an allocation: the box is the value in
  every position.
- **Payloadless variants** of a subtype box are constant singleton
  globals.
- **Size:** about 130 bytes per `List` method, and under A1 the method
  folds with every other reference list.
- **Self-recursive** payloads (directly or through other enums) are
  always references, so the layout stays finite.
- **Engines:** an allocation costs more on wasmtime, whose collector copies the whole live set; a `ref.cast` to a final type is a header load and compare on both (wasmtime unmeasured).
- **Type-only:** yes. Flat or subtypes depends on the enum's own field
  count; crossing the bound depends on the enum's deep layout. A change
  across the bound cascades to every value type that holds the enum by
  value, as the compile study notes, which is why there is one bound.
- **Pending:** the bound, E2.

### `Option`

```wat
;; T? with T a non-null reference (data, List, string, a box, a closure): (ref null $T)
;; T? with T a scalar: (i32 tag, T)                  -> two arrays in a list
;; T? with T a dyn pair: (eqref, (ref null $VT))     -> None is a null vtable (mine)
;; T? with T a value layout: (i32 tag, dflt(T)...)   -> boxed past the bound
;; T??: an outer tag plus the inner layout
```

- **Speed:** free. `.Some(x)` of a reference is `x`; `?` is a null test or
  a tag test.
- **Size:** about 2 extra bytes per pass for a scalar `T?`.
- **Engines:** no difference; null tests are one compare on both.
- **Type-only:** yes. The case depends on `T`'s class.

### `Result`

```wat
;; Result[T, E] -> (i32 tag, dflt(T')..., dflt(E')...), with slot sharing
;; Result[i32, IoError] with IoError boxed -> (i32, i32, (ref null $IoError))
(br_if $err (local.get $tag))      ;; each ? is a tag test, then the exit ladder
```

- **Speed:** free while both payloads stay within the bound. A boxed
  error costs nothing on the success path, since errors are cold.
- **Size:** about 2 to 4 bytes per pass-through for the wider results.
- **Engines:** no difference.
- **Type-only:** yes. The bound counts the payloads' own layouts, a boxed
  enum as one value.

### `dyn Trait` And `dyn Any`

```wat
;; a pair: (eqref payload, (ref $VT_Trait)); two fields; two arrays in a list
(type $VT_Display (struct (field $to_string (ref $Fn_to_string)) (field $Debug (ref $VT_Debug))))
(global $vt_Point_Display (ref $VT_Display) (struct.new $VT_Display (ref.func $Point.to_string) ...))
;; call: x.to_string()
(call_ref $Fn_to_string (local.get $payload) (struct.get $VT_Display $to_string (local.get $vt)))
```

- **Speed:** a coercion allocates nothing for a reference payload (a
  scalar or value payload is erased as above). A call is one load and
  one `call_ref`. With `devirt-vtable`, a `CallDyn` whose vtable is a
  constant global (after inlining) calls the slot's function directly,
  and `inline-bounded` may then inline it. This matters most for
  `mut dyn Hasher`.
- **Size:** a 3-slot vtable is about 27 bytes beyond its functions; a call
  site about 8 bytes.
- **Engines:** V8 inlines a monomorphic `call_ref` (0.31 ns, the same as
  inlined); with 8 targets it costs 5.3 ns. wasmtime never inlines it.
- **`is`:** `ref.eq` on the payloads, which is why the payload is
  `eqref`, not `anyref`.
- **`dyn Any`, `Inspectable`:** the vtable holds a reference to the
  type's `TypeId` global. `downcast` compares ids, then casts or unboxes.
- **Boundary:** not boundary-safe.
- **Type-only:** yes. The vtable shape depends on the trait alone.

### Generic Methods Through `dyn`: Witnesses And Thunks

```wat
(type $Ops_I.m (struct (field (ref $Thunk_0)) (field (ref $Thunk_1))))
(type $W_m (struct (field (ref null $Ops_I1.m)) (field (ref null $Ops_I2.m))))
(call_ref $Thunk_k ... (struct.get $Ops_I.m k (local.get $ops)))   ;; per open instruction
```

- **Speed:** one `call_ref` per open instruction, plus a box per
  crossing for wide scalars and value layouts within the bound. A layout
  over the bound is already a box and crosses as itself. The owner
  accepted a slower `dyn`.
- **Size:** about 25 bytes per thunk; thunks are the product of impls,
  type arguments and open instructions (20 × 5 × 10 = 1,000 thunks, about
  25 KB before folding). The compiler warns past 200 thunks for one
  method, and `--size-report` lists the counts.
- **Engines:** V8 may inline a monomorphic thunk call; wasmtime never does.
- **Type-only:** the thunks are; the witness field index `f_I` is the one
  known exception to stable numbering, limited to erased bodies.

### Closures And Mutably Captured Variables

```wat
(type $Fn_i32_i32 (sub (struct (field $code (ref $Code_i32_i32)))))
(type $Env_k (sub final $Fn_i32_i32 (struct (field $code (ref $Code_i32_i32)) (field $count (ref $Cell_i32)))))
(type $Cell_i32 (struct (field $v (mut i32))))
;; call f(x): the code receives the closure and casts it to its own environment
(call_ref $Code_i32_i32 (local.get $f) (local.get $x) (ref.null $Ctx)
          (struct.get $Fn_i32_i32 $code (local.get $f)))
```

- **Speed:** per call, a load, a `call_ref`, and a cast in the callee
  (skipped when the closure has no capture). Per construction, one
  allocation, plus one per shared cell. A capture-free closure is a
  constant global.
- **With passes:** the closure specialization schedule removes the call,
  the environment and the cells together when a closure literal reaches
  a callee within budget ([Closure Calls](#closure-calls)).
- **Size:** about 78 bytes for a small closure: type, code, construction.
- **Engines:** V8 inlines a monomorphic call site; wasmtime pays the
  indirect call every time, and it blocks loop optimization across it.
- **Boundary:** no serializable closures in the first release.
- **`T?` of a closure:** `(ref null $Fn_sig)`, a null niche.
- **Type-only:** yes. The base depends on the signature (rows left out,
  codegen.md §12.4); the subtype on the capture shape.

### `List[T]`

```wat
(type $List_i32 (struct (field $len (mut i32)) (field $data (mut (ref $Arr_i32)))))
(type $List_eq  (struct (field $len (mut i32)) (field $data (mut (ref $Arr_eq)))))   ;; A1: every reference T
(type $Arr_eq (array (mut eqref)))
;; items[i] for List[Point]
(if (i32.ge_u (local.get $i) (struct.get $List_eq $len (local.get $l))) (then (call $panic_index)))
(ref.cast (ref $Point) (array.get $Arr_eq (struct.get $List_eq $data (local.get $l)) (local.get $i)))
```

- **Speed:** index: two loads, a compare, an array read, and under A1 a
  cast where the caller needs the exact type (+0.3 ns per read on V8;
  wasmtime is E1). Push: amortized one store; growth uses `array.copy`.
  Counted loops hoist `len` and `data`.
- **GC:** two objects per list; the array holds `capacity` slots. Slots
  past `len` hold `default(L)`, so the element form is `dflt(L)`.
- **Size:** push with growth 124 bytes at `i32`, 130 at a reference; an
  index with its check 59 bytes, 67 with the cast. Under A1, one instance
  of each move-only method per class: about 9 KB of list code instead of
  20 KB for 20 element types, 12 of them references.
- **Elements of a value layout** within the bound are a structure of
  arrays; over the bound, one box per element.
- **Boundary:** count, then elements.
- **Engines:** bounds checks stay unless the engine proves them; V8 hoists more often. The A1 cast is the open wasmtime number (E1).
- **Type-only:** yes. `List[Point]` is always `{len, (array eqref)}`
  under A1, whatever the program.
- **Pending:** A1, E1.

### `Map[K, V]`

A compact insertion-ordered map, as CPython's compact dictionary and
JavaScript's deterministic `Map`:

```wat
(type $Map_K_V (struct
  (field $index  (mut (ref $Arr_i32)))   ;; power of two; -1 empty, -2 deleted, else an entry number
  (field $hashes (mut (ref $Arr_i32)))   ;; per entry, insertion order
  (field $keys   (mut (ref $Arr_K)))     ;; per entry; eqref under A1; parallel arrays for a value K
  (field $values (mut (ref $Arr_V)))     ;; per entry; no Wasm array when V is void
  (field $used (mut i32)) (field $live (mut i32))))
```

- **Speed:** a lookup hashes the key, probes `index`, compares the stored
  hash, then calls `Eq`. Removal writes a hole and a tombstone; growth
  compacts. Iteration walks entries and skips holes, in insertion order.
  Today it costs 3 to 5 allocations and 2 indirect calls per lookup. The
  `Hasher` change removes the allocations in every pipeline; with
  `devirt-vtable` and `inline-bounded`, an `i64` key also hashes with no
  indirect call.
- **Size:** about 12 methods per `(K, V)`, folding by class under A1 on
  the `V` side and for move-only paths.
- **Bucketing:** an implementation detail since the owner's decision of
  2026-10-07 ([`std-hash.map.bucket-hash`](../../spec/std/hash.md#r-std-hash.map.bucket-hash));
  iteration order is deterministic but need not be insertion order, so
  this compact insertion-ordered table is one valid layout, not a
  requirement. See [decision 3](#3-the-maps-bucketing-hash).
- **Boundary:** count, then key and value pairs in iteration order;
  decoding calls the key's `Eq` and `Hash` in Wasm.
- **Engines:** the hash protocol's indirect calls are free on V8 when monomorphic and real on wasmtime; the `map` case is 5.3x Node today.
- **Type-only:** yes.
- **Pending:** E5 measures the hash protocol's gain.

### `Set[T]`

```wat
;; Set[T] = { map: Map[T, void] }: $Map_T_void has no values array (Array[void] has none)
```

- **Speed:** `insert` probes once through an internal
  insert-if-absent method of the map.
- **Size:** the map's methods at `V = void`; nothing per element beyond
  the key and its hash.
- **Engines:** as `Map`.
- **Type-only:** yes. The empty array follows from the `void` layout,
  with no special case.

### `Deque[T]`

```wat
(type $Deque_T (struct (field $data (mut (ref $Arr_T))) (field $head (mut i32)) (field $count (mut i32))))
;; a ring buffer over Array[T]; slots outside [head, head+count) hold default(L) and are never read
```

- **Speed:** one array, no tag test per access. Today's `List[T?]` costs
  two arrays and a `match` per access for a scalar `T`.
- **Size:** the ring arithmetic per method; about the size of the
  matching `List` method.
- **Compiler hook:** `Array[T]` with slots past a count that hold
  `default(L)`, which `List` already needs.
- **Engines:** no difference.
- **Type-only:** yes.

### `Heap[T]`

```wat
;; Heap[T] = { items: List[T] }: sift up and down over the list; pop shrinks it
```

- **Speed:** a comparison is two array reads and a call of the type's own
  `cmp` (inlined by `inline-bounded`); no `match` per read, no tag array.
- **Engines:** no difference.
- **Type-only:** yes.

### Iterators And Ranges

```wat
;; Iterator[T] = data { step: fn() -> T? }   -> (ref $Iterator_T) holding (ref $Fn_unit_OptT)
;; for i in a..b, a..=b, a..  -> a counted loop; no range value exists (codegen.md §12.5)
;; Range[T] as a value -> data { start, end, inclusive }; one allocation in the baseline
```

- **Speed:** in the baseline, `xs.iter().map(f).sum()` costs about 7
  allocations per chain (3 closures, 3 iterators, 1 cell) and 3
  `call_ref`s, 3 casts and 2 option tests per element. With the closure
  specialization schedule the chain is a counted loop: about 0.3 ns per
  element on V8, against 1 to 5 ns with real indirect calls.
- **Size:** about 30 bytes per call into shared adapters in the
  baseline; 50 to 150 bytes per inlined stage when specialized.
- **Ranges in slices:** `items[1..3]` builds a `Range` data value for the
  `Index` impl; `inline-bounded` then `scalar-replace` remove it.
- **Engines:** V8 hides one monomorphic `call_ref` per stage; wasmtime pays each one and loses loop optimizations across it, so the pass matters most there.
- **Type-only:** yes; specialization makes code, never layouts.
- **Pending:** E3 (the pass is required for release if the
  unspecialized form exceeds 3x Node's `for` loop on wasmtime); S6 and E8
  for the cap.

### Newtypes

```wat
;; type Mile(i32) -> i32;  type Order(Draft) -> (ref $Draft): no wrapper, no allocation
```

- **Speed, size:** free; construction and unwrapping emit nothing.
- **Folding:** instances over `Mile` and over `i32` fold.
- **`TypeId`:** distinct, by `canon(Mile)`.
- **Engines:** no difference.
- **Type-only:** yes.

### `TypeId`

```wat
(type $TypeId (struct (field $id i64) (field $name (ref $LitFn))))   ;; $LitFn = func () -> (ref $bytes) i64: the name as a string view
(global $tid_Point (ref $TypeId) (struct.new $TypeId (i64.const 0x9f3a...) (ref.func $lit_Point)))
;; ==: compare $id; Hash: write_u64($id); Display: call_ref the name getter
```

- **The id** is the first 64 bits of `H(canon(T))`, so it depends only on
  the type. Link checks the program's ids for collisions; on one, link
  rehashes every id with the next fixed salt, deterministically. The
  spec promises nothing across builds
  ([`trait.typeid.cross-build`](../../spec/lang/09-traits.md#r-trait.typeid.cross-build)),
  and a content hash keeps more than that.
- **Speed:** equality is one `i64.eq`; no allocation.
- **Size:** about 15 bytes per type that reaches a type-id read, plus its
  name literal's getter.
- **Engines:** no difference.
- **Type-only:** yes.

### Host Handles

```wat
;; i32: 20 bits of slot, 11 bits of generation, inside the provider's data value
;; debug builds add a closed flag beside it
```

- **Speed, size:** free.
- **Boundary:** the `i32` itself.
- **Engines:** no difference.
- **Type-only:** yes.

### Suspension Frames

```wat
(type $Suspend_L (sub (struct (field $state (mut i32)) (field $flags (mut i32))
                              (field $driver (mut anyref)) (field $vt (ref $SuspendVT_L)))))
(type $F_f (sub final $Suspend_L (struct ... saved dflt locals ... (field $child (mut (ref null $F_g))))))
;; f$body, f$cold, f$poll, f$cancel per suspending instance (suspension.md §14.1)
```

- **Speed:** the ready path is a call and a null test; the pending path
  allocates one frame per level, once.
- **Size:** four functions per suspending instance. `poll` and `cancel`
  fold only across equal frame layouts.
- **Engines:** no difference in shape; the browser drives `hd.poll` from the worker's event loop (§17.6).
- **Type-only:** yes.
- **Pending:** E11, a shared `poll` per result layout.

### Facts

```wat
(func $fact_k (result (ref $T))          ;; the getter; readers call it
  (block $hit (result (ref $T))
    (br_on_non_null $hit (global.get $fact_k_value))
    ... run the fact body inside the forbidden-context counter, store, return))
```

- **Speed:** per read, a direct call, a `global.get`, a test and a branch.
  `cse-getters` keeps one call per dominating path in a body.
- **Size:** one getter of 30 to 50 bytes per fact; about 3 bytes per read
  in release.
- **Flag:** a fact of one reference layout uses null as its flag; other
  layouts keep an `i32` flag global.
- **Engines:** no difference.
- **Type-only:** not a type question. The getter keeps the fact's global
  index out of every reader, which is what stable numbering asks.

### Literals

```wat
;; integers and floats: immediates.  Strings, per source module m:
(global $lits_m (ref $LitTab)                                    ;; one immutable table per module
  (struct.new $LitTab (array.new_default $Pool (i32.const N_m))  ;; N_m: m's pooled literals
                      (i32.const base_m)))                       ;; m's index area in segment 0
(func $lit_m (param $k i32) (result (ref $bytes))                ;; one getter per module
  (block $hit (result (ref $bytes))
    (br_on_non_null $hit
      (array.get $Pool (struct.get $LitTab 0 (global.get $lits_m)) (local.get $k)))
    (call $lit_fill (global.get $lits_m) (local.get $k))))       ;; shared: reads (offset, length) at base_m + 8k,
                                                                 ;; array.new_data from segment 0, stores the slot
;; a use of m's literal number k:      (call $lit_m (i32.const k)) (i64.const len_k << 32)
;; a literal is a view over its pooled array, from start 0; slicing it shares the pooled bytes
;; literals of at most 4 bytes: an immutable global of array.new_fixed, read the same way
```

**One table per module, not one getter per literal** (orchestrator,
2026-10-07, after the systems review). The lowering pass first gave each
literal its own getter. At 10k lines that is about 2,000 tiny functions,
and each one pays Cranelift's fixed cost per function and a member of the
Cranelift cache. wasmtime does not inline across functions, so nothing
removes them. Now each module's literals are slots of one table,
numbered locally:

- **Module-local numbers.** m's pooled literals are numbered in content
  order (by their bytes) within m. The list is part of m's TIR content
  hash. A number changes only when m's own set of literals changes.
- **What a body holds.** A use is `i32.const k` and a call of `$lit_m`.
  The call target is abstracted by wasmtime's per-function cache, and `k`
  depends only on m. So no body holds a program-wide number. The code key
  of an instance lists `(literal hash, module-local number)` for every
  literal it reads, its own or an inlined callee's (codegen.md §13.8).
- **What only the getter holds.** The index of `$lits_m` and, through
  the table, `base_m`, which depend on the program. They live in one
  function per module, emitted at link like the old getters.
- **Speed:** a direct call with one argument, an array read with a bounds
  check (the index is no longer a constant inside the getter) and a null
  test per use. `cse-getters` keeps one call per dominating path, and
  since a literal read cannot panic, it hoists the call out of a loop.
- **Size:** the bytes in one deduplicated passive segment, plus an index
  area of 8 bytes per module-local literal; about 40 bytes per module
  for its getter; about 5 bytes per use in release; and one shared fill
  helper (about 90 bytes). As `array.new_fixed`, a 16-byte literal is 65
  bytes of code.
- **Host fast path:** `println` of a literal calls its module's span
  function, `$span_m(k)`, which copies the bytes from segment 0 into the
  exchange buffer with `memory.init` and returns the length; the import
  then reads the buffer as usual. No array is built, the offset stays
  inside the span function, and hello world needs no pool.

**The comparison** (`ordinary-10k`: about 2,000 pooled literals, an
estimated 3,000 uses, 100 modules plus about 40 std modules reached;
Cranelift's fixed cost per function is an estimate of 10 to 30 µs until
S3 measures it):

| Cost | One getter per literal | One table per module (chosen) | Table read inline, no getter |
| --- | --- | --- | --- |
| functions added | about 2,000 | about 140 | none |
| code bytes | 2,000 × 16 + 3,000 × 3 ≈ 41 KB | 140 × 40 + 3,000 × 5 + the index area 16 KB ≈ 37 KB | 3,000 × 12 + 16 KB ≈ 52 KB |
| Cranelift, cold | 2,000 × 10 to 30 µs = 20 to 60 ms of CPU | 1.4 to 4 ms | none |
| Cranelift cache members | +2,000 | +140 | none |
| a literal added in module m | getters after it in program order renumber: about 1,000 misses, 10 to 30 ms | m's instances that read a renumbered literal re-emit and recompile: an estimated 10 to 25, 2 to 6 ms | as the table, plus every reader of `$lits_m` if globals shift |
| a literal added elsewhere | the same 1,000 misses | nothing in m | nothing, if S2 shows global indices stable |
| hit path | call, array read, null test | call, array read with bounds check, null test | array read, null test |

The inline variant puts the program-dense index of `$lits_m` into every
reader. That is the same exposure as the direct constant globals, which
S2 measures. If S2 shows immutable globals keep at least 95 percent of
functions hitting, the getter may be inlined later as a pass
(`inline-trivial` already fits it). E10 sets the `array.new_fixed`
threshold and eager filling; S2 and E10 give the final numbers.
- **List and map literals** build fresh values each evaluation, since
  lists and maps are mutable: `array.new_fixed` for a short list,
  `array.new_data` for a long constant scalar list.
- **Engines:** no difference in code; `array.new_data` is not a constant instruction, which is why long literals cannot be constant globals on either engine.
- **Type-only:** not a type question.
- **Pending:** E10 (the threshold and eager filling).

## Expressions And Statements

For a syntax form, "type-only" asks whether the emitted code depends only
on the instance's own TIR, its type arguments and the layouts and impls
it reads, which the code key lists (codegen.md §13.8). Every form below
keeps that. None reads a program-wide number (see [Numbering](#numbering)).

### Arithmetic And Overflow Checks

```wat
;; a + b on i32, debug and test: checked; release: i32.add alone
(local.set $r (i32.add (local.get $a) (local.get $b)))
(if (i32.lt_s (i32.and (i32.xor (local.get $a) (local.get $r))
                       (i32.xor (local.get $b) (local.get $r))) (i32.const 0))
    (then (call $panic_overflow)))          ;; no site immediate: the call's offset is the site
;; unsigned: a + b < a; i64 *: divide back; narrowing casts: a range compare
```

- **Speed:** release is the bare instruction. Debug adds a few ALU
  operations and a predictable branch; the `i64` multiplication check
  costs about 20 cycles. `check-cost` (the optimized pipeline, checked
  at most 1.3x unchecked) watches the total.
- **Size:** about 10 to 14 bytes per checked operation in debug. Each
  check keeps its own `call`, since its code offset names its site.
- **Engines:** no difference.
- **Type-only:** yes. The sequence depends on the operand type and the
  profile.

### Division, Remainder And Shifts

```wat
;; a / b on i32: a zero divisor traps in the engine; the trap maps to integer-division-by-zero
(if (i32.and (i32.eq (local.get $a) (i32.const 0x80000000)) (i32.eq (local.get $b) (i32.const -1)))
    (then (call $panic_overflow)))        ;; debug; release returns INT_MIN, since div_s would trap
;; x << n: n >= width panics invalid-shift in every profile; Wasm masks the count otherwise
(if (i32.ge_u (local.get $n) (i32.const 32)) (then (call $panic_shift)))
```

- **Speed:** one compare and branch per operation.
- **Size:** about 12 bytes per guarded division, 8 per guarded shift.
- **Engines:** both trap on a zero divisor; the host maps the trap by code
  offset.
- **Type-only:** yes.

### Comparison, Equality And Logical Operators

```wat
;; primitives: i32.eq, i64.lt_s, f64.eq ...; string ==: length compare, a ref.eq-and-start fast path, then a byte loop over both views (std)
;; data or enum ==: a direct call of the selected Eq impl, usually inlined
;; a and b: (if (result i32) (local.get $a) (then (local.get $b)) (else (i32.const 0)))
```

- **Speed:** primitives free; strings one byte per step (E7 measures it
  against Node); user `Eq` a direct call.
- **Size:** none beyond the instruction or the call.
- **Engines:** no difference.
- **Type-only:** yes.

### Indexing And Slicing

```wat
;; items[i]: the list's index method, inlined: compare with len, read, (A1) cast
;; m[k]: lookup; absent key panics index-out-of-bounds;  m.get(k) returns V?
;; s[i]: compare with the view's len (index-out-of-bounds), then array.get_u at start + i
;; s[a..b]: bounds and boundary checks, then a new span over the same array; no allocation
;; items[a..b]: a new list of the selected elements (a copy, by the spec)
```

- **Speed:** a list index is two loads, a compare and a read; a map index
  is a lookup. A list slice allocates a copy (by the spec); a string
  slice allocates nothing.
- **Size:** 59 bytes for a list index with its check, 67 with the A1
  cast, when inlined; a call is about 5 bytes when not.
- **Engines:** V8 hoists more bounds checks out of loops than Cranelift.
- **Type-only:** yes.
- **Pending:** A1 (E1).

### Path Mutation And Compound Assignment

```wat
;; order.items[i].qty += 1
(local.set $l (struct.get $Order $items (local.get $order)))     ;; evaluate the path once
... index check on $l, read element, cast (A1) to $Line ...
(struct.set $Line $qty (local.get $line) (i32.add (struct.get $Line $qty (local.get $line)) (i32.const 1)))
;; m[k] += v: one lookup for the read and the write through an internal entry slot
;; a part on the path: one more struct.get; the part is shared, so the write is visible through the outer value
```

- **Speed:** each path step is one load; the final step is one store. A
  value-layout element in a list (within the bound) is updated in place in
  its parallel arrays; a boxed element is replaced by a new box.
- **Size:** a few bytes per step.
- **Engines:** no difference.
- **Type-only:** yes.

### String Interpolation And Concatenation

```wat
;; "id=$id name=$name": each Display part writes into one builder through its resolved callee
(call $sb_new (i32.const 16))                   ;; capacity: the literal parts' total bytes
(call $sb_push_lit (... (call $lit_k)))         ;; literal parts: array.copy from the pooled literal
(call $i32_display (local.get $sb) (local.get $id))
(call $str_display (local.get $sb) (local.get $name_b) (local.get $name_span))  ;; copies the viewed bytes
(call $sb_finish (local.get $sb))               ;; one exact-size $bytes, viewed from start 0
;; a + b on strings: array.new of the summed length, two array.copy from the two views
```

- **Speed:** one builder, one final string; each `Display` part a direct
  call that writes in place rather than returning a string.
- **Size:** about 8 bytes per part plus the literal's getter call.
- **Engines:** no difference.
- **Boundary:** `println("...")` of a literal alone takes the host fast
  path (no array built).
- **Type-only:** yes. The callee per part is the impl `select` picks.

### Pipes, Binding Expressions And Ranges

```wat
;; x |> f(_, 2)  ==  f(x, 2): the piped value is a local, evaluated first; nothing at run time
;; (n := xs.len()) > 0: a local
;; a..b as a value: struct.new $Range; in for and in an inlined slice it is never built
```

- **Speed, size:** nothing beyond the desugared form.
- **Type-only:** yes.

### Propagation `?`

```wat
;; Result: test the tag local; on .Err convert the error (From impl, a direct call) and leave
(if (local.get $tag) (then
    (local.set $ret_err (call $From_FsError_SyncError (local.get $err)))
    (local.set $exit (i32.const 2)) (br $cleanup)))       ;; or br straight to the function's end with no defer
;; Option of a reference: (br_on_null $none (local.get $v))
```

- **Speed:** one tag or null test. A conversion runs only on the error
  path, before any deferred cleanup.
- **Size:** about 6 to 12 bytes per `?`; more when an error conversion is
  called.
- **Engines:** no difference.
- **Type-only:** yes.

### `if` And `match` As Values

```wat
(if (result i32 i64) (local.get $c) (then ...) (else ...))   ;; a multi-value block type uses a func type entry
;; match: br_table on a tag or a dense range; a binary search of ifs past 8 sparse cases;
;; strings: length, then bytes; each arm once, in nested blocks
(block $arm2 (block $arm1 (block $arm0 (br_table $arm0 $arm1 $arm2 (local.get $tag))) ...) ...)
```

- **Speed:** a `br_table` per enum match; a subtype box casts once per
  matching arm.
- **Size:** 44 bytes for a 3-arm match on a value enum, 68 on a boxed one.
- **Engines:** both lower `br_table` to a jump table.
- **Type-only:** yes. The block's result types come from the layout.

### Loops

```wat
;; for i in a..b: i = a; end = b; loop: if i >= end break; body; i = i + 1
;; for x in items: len captured; loop over the backing array; compare len each step (iterator-invalidated)
;; for (k, v) in m: loop over entries, skipping holes; the same length check
;; for x in it (an Iterator): call step until .None; specialized when it is a known chain
;; while c: (block $exit (loop $top (br_if $exit (i32.eqz c)) body (br $top)))
```

- **Speed:** counted loops allocate nothing in either pipeline; this is a
  lowering rule, not an optimization. A loop over a list under A1 casts
  each element only where the body needs its exact type.
- **Size:** about 20 to 30 bytes of loop scaffolding.
- **Engines:** Cranelift does not vectorize; V8 neither for Wasm GC
  arrays.
- **Type-only:** yes.

### `break` With A Value, `continue` And Loop `else`

hd has no loop labels: `break` and `continue` target the nearest loop
([`flow.break`](../../spec/lang/06-control-flow.md#r-flow.break)). A loop
with `else` produces a value.

```wat
(block $done (result i32)                 ;; the loop's value
  (block $exhausted
    (loop $top ... (br $done (local.get $v)) ... (br $top)))   ;; break v
  ... else suite ...)                      ;; normal exhaustion
;; continue: br to the increment (counted loops) or to $top
;; inside a Scope with defer: the exit ladder stores the value and an exit code first
```

- **Speed, size:** branches only.
- **Engines:** no difference.
- **Type-only:** yes.

### Comprehensions

```wat
;; [for x in xs if p(x) => f(x)]: the nested loops of the clauses, a push per result
(local.set $out (call $List_new_with_capacity (struct.get $List $len (local.get $xs))))   ;; no filter: exact capacity
;; {for u in users => u.id: u}: map inserts in order; a later duplicate replaces the value
```

- **Speed:** eager loops; one result list or map, grown as it goes, or
  sized once when the first clause is a list and there is no filter.
- **Size:** the loops plus a push call per result site.
- **Engines:** no difference.
- **Type-only:** yes.

### Method Calls: Static, Bound, `dyn`

```wat
(call $Cart.total (local.get $cart))                 ;; an inherent or Impl choice
(call $Point.Display.to_string (local.get $p))      ;; a Bound choice, selected at the instance's types
(call_ref $Fn_m (local.get $payload) ... (struct.get $VT $m (local.get $vt)))   ;; a dyn call
;; with devirt-vtable, a dyn call whose vtable is a known constant global: a direct call
```

- **Speed:** static and bound calls are direct, and `inline-trivial` or
  `inline-bounded` may inline them; a `dyn` call is a load and a
  `call_ref` in the baseline, a direct call with `devirt-vtable`.
- **Size:** a direct call is about 3 bytes in release; a `dyn` call about
  8.
- **Engines:** V8 inlines monomorphic `call_ref`s; wasmtime never does,
  so devirtualization is the compiler's job.
- **Type-only:** yes. The callee is what collection selected, recorded
  in the code key.

### Closure Calls

```wat
;; baseline: struct.get $code, call_ref with the closure first, cast in the callee
;; the closure specialization schedule (a closure literal reaching a callee within budget):
;;   inline-bounded the callee; escape and scalar-replace its Iterator and environment structs;
;;   devirt-closure: the call_ref target is a known literal, so call its body directly; inline it
;;   repeat per stage, at most 4 rounds, under the caller's size cap
```

- **Speed:** baseline, one `call_ref` and one cast per call; with the
  schedule, none, and no allocation for the chain.
- **Size:** 50 to 150 bytes per specialized stage at the call site; at
  most 512 bytes of growth per chain site by default.
- **Engines:** the pass matters on wasmtime; V8 already inlines
  monomorphic calls at run time.
- **Type-only:** yes; specialization makes code, never layouts, and its
  budget is local to the caller, so the caller's bytes depend only on its
  own code key's inputs.
- **Pending:** S6 and E8 (caps), E3 (whether release requires the pass).

### Providers: `$.with`, `$.use` And Rows

```wat
;; a concrete row: one parameter per key, in canon(K) order; a provider is a dyn pair
(call $load_user (local.get $id) (local.get $db_payload) (local.get $db_vt))
;; a row parameter $R: one context parameter, a linked list of (key id, provider)
(struct.new $Ctx (i64.const 0x51c2...) (local.get $p) (local.get $vt) (local.get $ctx))   ;; key id: a content hash
```

- **Speed:** a concrete row costs one argument pair per key and no
  allocation. Extension into row-polymorphic code allocates one node; a
  lookup walks the list.
- **Size:** two parameters per key; about 15 bytes per context
  extension.
- **Engines:** no difference.
- **Type-only:** yes. Key ids are `H(canon(K))`, so a new key elsewhere
  in the program changes no body.

### Bang Calls, `all!`, `race!` And `block_on`

```wat
;; x := load!(id): call the body with a null frame; Ready continues; Pending saves live locals and returns
(call $load$body (ref.null $F_load) (local.get $id) ...)   ;; -> (dflt(T), (ref null $F_load))
(br_on_non_null $pending ...)
;; all!(a, b): one intrinsic frame per tuple of child result types; race!: a generic intrinsic over T
```

- **Speed:** the ready path is a direct call and a null test; the
  pending path allocates one frame per level once. `all!` polls each
  child; `race!` cancels the losers synchronously.
- **Size:** the save and reload sequences per suspension point; four
  functions per suspending instance (suspension.md §14.1).
- **Engines:** `block_on` uses JSPI in the browser, else a synchronous
  same-origin XHR, else a `host-contract` panic (§17.6).
- **Boundary:** a waiting host call is `.start` and `.finish` (§17.2).
- **Type-only:** yes.
- **Pending:** E11.

### `defer`

```wat
;; one exit: the suites run there directly (the common case)
;; several exits: each stores its value and an exit code, then br $cleanup;
;; $cleanup runs the registered suites last in, first out, then br_table on the exit code
```

- **Speed:** a flag store per conditionally registered suite, and a
  `br_table` at a scope with several exits. A panic runs no suite.
- **Size:** one cleanup block per scope, plus a few bytes per exit.
- **Engines:** no difference.
- **Type-only:** yes.

### Let Patterns And Let-Else

```wat
;; let .Some(p) = find(id) else: return 0
(block $matched (result (ref $Point))
  (br_on_non_null $matched (call $find (local.get $id)))
  ... else block: diverges ...)
;; let (a, b) = pair: the pair's values into two locals; nothing allocated
```

- **Speed, size:** a match of one arm.
- **Engines:** no difference.
- **Type-only:** yes.

### `is`

```wat
(ref.eq (local.get $a) (local.get $b))     ;; data, lists, maps, an AnyRef type parameter's instance
(ref.eq (local.get $pa) (local.get $pb))   ;; dyn and Any: the eqref payloads
```

- **Speed, size:** one instruction. Value operands are a compile error,
  so there is nothing to emit for them.
- **Type-only:** yes.

### Default Arguments

```wat
;; f(a) where f(a, b = make()): a direct call of the default body's instance with a,
;; inside the forbidden-context bracket when the default body makes a call
(call $f (local.get $a) (call $f$default_b (local.get $a)))
;; with inline-trivial, a constant default (= 10) is inlined at the call
```

- **Speed:** a call per omitted argument, unless inlined.
- **Size:** about 5 bytes per call site; the default body once per
  instance.
- **Type-only:** yes.

### Module Initialization Across Folders

```wat
(func (export "hd.init")
  (call $init_group_std_text) (call $init_group_shop_model) (call $init_group_shop_main))   ;; D1's InitOrder
;; each group's init runs its statements in order and sets module storage globals
```

- **Speed:** once per program instance; per test case, since each case
  gets a fresh instance. Constant globals need no init call.
- **Size:** one init function per reachable group with statements.
- **Engines:** no Wasm `start` function on either, so the host can
  attribute an init trap and bound it by the case's time limit.
- **Type-only:** not a type question; the order is D1's, a function of
  the use graph and source order.

### Tests

```wat
;; one export per registered case; its TestCase body evaluates the registration's run-time
;; arguments and calls the std registration function, which drives the test body
(func (export "t3") ...)
;; --filter: the program's roots are the selected cases plus the module's init groups
```

- **Speed:** a fresh instance per case, through `InstancePre` and the
  pooling allocator.
- **Size:** a filtered program holds only the selected cases' code; it
  shares every code entry with the full program.
- **Type-only:** yes.

### Derives

```wat
;; @derive(Debug, Eq, Hash) on data Point: ordinary instances of the templates;
;; each member call (w.member(h, value)) is a direct call; member handles are constant globals
;; with inline-trivial, each member is inlined and collection emits no member instance
(call $hasher_write_u32 ...)   ;; with the Hasher change, a field hash writes without allocating
```

- **Speed:** a direct call per member in the baseline; straight-line
  code with `inline-trivial`. Derived hashing writes through the new
  fixed-width `Hasher` methods, so it allocates nothing in any pipeline.
- **Size:** derived code is about 27 percent of a 10k-line program's
  instances today; in a pipeline with `inline-trivial`, skipping
  always-inlined callees at collection removes the per-member instances.
- **Engines:** no difference.
- **Type-only:** yes.

### Explicit Panics And Unreachable Code

```wat
(call $panic_explicit (... message in the exchange buffer ...))   ;; stores the category, then unreachable
```

- **Speed:** a panic traps; the host reads the category global and the
  buffer, never calling into the poisoned instance.
- **Size:** one `call` per site; the stub once per category.
- **Engines:** wasmtime's `WasmBacktrace` and V8's `Error.stack` both give
  the caller frame's code offset, which `hd.sites` maps to the site.
- **Type-only:** yes.

## Inconsistencies Between The Two Studies

| Topic | Runtime study | Compile study | Resolution |
| --- | --- | --- | --- |
| debug Cranelift level | debug and release run the same optimizer | `Speed` unless S4 shows `None` saves 25 percent | both studies assumed one shared emission; the owner's tiering decision of 2026-10-07 gives a dev and an optimized pipeline, and spike T1 sets the dev Cranelift setting ([tiering.md](tiering.md)) |
| what an optimization is | budgets inside one shared pipeline | the same | each optimization is a named pass with inputs, outputs and cost ([Optimization Passes](#optimization-passes)); the optimized pipeline runs them all |
| literal access | an inlined fast path at the use site, the pool index in it | lazy globals through getters, or append order | one table and one getter per module, with module-local numbers (orchestrator, after the systems review): an inlined program-wide index is a dense number in every reader, append order needs build history, which §15.8 forbids, and a getter per literal adds about 2,000 functions at 10k lines |
| facts in loops | inline the fast path at reads inside loops | getters | getters, with reads common-subexpression-eliminated in a body |
| erased storage (A) | a layout rule for every type-parameter slot; a cast at the first exact use | A1 at collection by representation summaries, instance keys by class | both: the layout rule gives one type per class; the summary gives one instance per class for move-only bodies. Classes `REF` and `REF?` stay apart, since `T?` differs between them |
| `TypeId` hash | a link-time number; `Hash` writes the name bytes | a content hash | a content hash; `Hash` writes the 64-bit id, which is already independent of link order |
| the boxing bound | box over 4 values everywhere, after slot sharing | reuse the existing bound; one bound only | one bound, 4 by default, after slot sharing; E2 sets it |
| short literals and vtables | `array.new_fixed` for literals of at most 4 bytes | direct constants "ordered by content", which still shift on insertion | direct by default; S2 decides per global kind |
| heap sizing | `Config::gc_heap_initial_size` around 64 MiB from the profile | not covered | `hd run` only; `hd test` keeps small pooled heaps, since 64 concurrent instances at 64 MiB each would exhaust memory. E9 and `unit-test-perf` measure it |
| principle 3 and A1 | one representation per type in every position | `eqref` slots with casts | compatible: principle 3 forbids conversions that allocate or copy; an upcast to `eqref` and a cast back do neither |
| the `name` section | not covered | `hd.names` in release, `name` only in debug | adopted; release backtraces are symbolized from `hd.names` |
| map hashing | the map's internal hasher may use a faster mix | not covered | conflicts with `std-hash.default.map`; owner question 3 |
| `--size-report` | a first-release size guard | not covered | first release, by the orchestrator's call; goals.md still lists it as Later |

## Owner Decisions (2026-10-07)

The four questions this pass raised are answered.

### 1. String Slicing: S1 Or S2

**Decided: S2, Go-style shared slices.** A string is a view `(array,
start, len)`; `slice` and `s[a..b]` stay O(1) and share bytes, as the
spec already says
([`module.string.slice`](../../spec/lang/10-modules.md#r-module.string.slice),
[`module.string.slice.shared`](../../spec/lang/10-modules.md#r-module.string.slice.shared),
[`expr.index.slice.string.shared`](../../spec/lang/05-expressions.md#r-expr.index.slice.string.shared)).
This pass had recommended S1 (copying slices). The layout is in
[`string`](#string) and wasm-layout.md §15.2. The known cost: a small
slice keeps its whole backing array alive.

### 2. Rebasing The `dead-code` Target

**Decided:** the target is re-based after spike S7 measures real section
sizes. Every design in the compile study lands at 48 to 64 KB per 1,000
lines, 33 to 42 KB of it code; the toy compiler measures 58 B per line.

### 3. The Map's Bucketing Hash

**Decided:** the bucket hash is an implementation detail
([`std-hash.map.bucket-hash`](../../spec/std/hash.md#r-std-hash.map.bucket-hash)),
and iteration order is deterministic but no longer required to be
insertion order
([`types.map.order.deterministic`](../../spec/lang/04-type-system.md#r-types.map.order.deterministic)).
`hash_of` and `DefaultHasher` stay FNV-1a. After E5, the map may mix a
whole `u64` per write. The compact insertion-ordered table of
[`Map[K, V]`](#mapk-v) remains a valid layout.

### 4. No Standard `name` Section In Release Builds

**Decided:** release builds carry the compact `hd.names` instead of
`name`, which saves about 120 KB at 10k lines. hd's own backtraces stay
symbolized. External tools (browser devtools, `wasm-objdump`, native
profilers) show unnamed functions for a release module; a dev build keeps
`name` for them.
