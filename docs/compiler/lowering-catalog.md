# New Compiler: Lowering Catalog

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](ARCHITECTURE.md). One entry per data type
and per syntax form: its Wasm GC representation and emitted code, with
its cost in speed and size. The layout rules themselves live in
[crates/hd_wasm/DESIGN.md](../../crates/hd_wasm/DESIGN.md) (§15.1,
§15.2; `hd_wasm::layout`), the emission rules in
[crates/hd_mono/DESIGN.md](../../crates/hd_mono/DESIGN.md).

Representation (layouts, the literal pool, numbering) is the same in both
pipelines of [docs/compiler/tiering.md](tiering.md). Each entry gives the
**baseline emission**, the code that one walk writes with no optimization
pass. Speed and size figures come from the representation studies' pilot
counts on V8, or are estimates. Planned, not built: the spike 0c
measurements those studies named (E1 to E11, S1 to S7), which would settle
the bound, the A1 read cost on wasmtime and the inliner caps.

## Conventions

Each entry gives:

- **Wasm:** the Wasm GC types and the shape of the emitted code, as a
  small WAT-like snippet. `$X` names a canonical type; `dflt(L)` is the
  defaultable form of layout `L` (crates/hd_wasm/DESIGN.md §15.2).
- **Speed:** allocations, indirect calls and casts on the hot path of the
  baseline emission. A figure that needs a pass names it.
- **Size:** bytes per use or per instance, an estimate where no pilot
  count exists.
- **Engines:** what differs between wasmtime (Cranelift, copying
  collector, no indirect-call inlining) and V8 (speculative `call_ref`
  inlining, generational collector).
- **Boundary:** how the value crosses to the host or JS, where it can.
- **Type-only:** whether the layout depends only on the type's definition
  (and the definitions its fields name), never on uses or the program. A
  cached instance is then valid in every program that reaches it.

The **bound** (`hd_wasm::layout::BOUND`) is at most 4 Wasm values after
slot sharing, the tag included. A value layout over it is one immutable
box everywhere, fields and arrays included, and counts as one value
toward an enclosing layout's bound. A type has the same Wasm values in a
local, a parameter, a field and an array element. A type-parameter slot
at a reference layout is stored as `eqref` (A1, `layout::storage`), so
move-only generic bodies get one instance per representation class.

## Numbering

Every index space of the module, and what a code body holds for it:

| Index space | A body holds | Order at link |
| --- | --- | --- |
| defined functions | a `call` index, as a relocation in a 5-byte padded slot | imports, runtime helpers, then instances |
| `ref.func` targets | the function index, as a relocation | as above |
| Wasm types | type immediates in `struct.new`, `ref.cast`, `array.new`, block types | canonical and deterministic (crates/hd_wasm/DESIGN.md §15.3) |
| immutable constants: vtables, capture-free closures | `global.get`, as a relocation | by content key |
| module storage | `global.get`, `global.set` | by module path, then binding index |
| literals | a `call` of the literal's getter ([Literals](#literals)) | one deduplicated passive segment |
| context key ids | `i64.const` of a content hash | none |
| witness fields `f_I` | a relocation | per program; erased bodies only |

The rule: a code body holds no program-wide dense number except through a
relocation, so its bytes before link depend only on its code key.
"Append-friendly order" would need a numbering kept from earlier builds,
which makes the bytes depend on build history; crates/hd_wasm/DESIGN.md
§15.8 forbids it.

## Baseline Emission

The baseline is what one walk over an instance's TIR writes with no
optimization pass.

- Every direct call stays a `call`; every `CallDyn` and `CallValue` is a
  `call_ref`.
- Every construction allocates: a capturing closure, its cells, an
  `Iterator`, a box over the bound, a `Range` value. A capture-free
  closure is a constant global.
- Every literal read calls its literal's getter, and every fact read calls
  the fact's body, which returns the stored value after its first run.
- Checks follow the spec's profile rules, which are semantics, not
  optimization: overflow checked in debug and test, wrapping in release.
- `for` over a numeric range is a counted loop ([Loops](#loops)).
- A1, slot sharing, the bound and the literal pool are representation, so
  they are part of the baseline too.

## Optimization Passes

Each optimization is a pass in a pipeline of
[docs/compiler/tiering.md](tiering.md). Analyses and decisions write side
tables; no pass materializes instance IR. Every pass keeps a function's
output a function of its own code key's inputs: budgets are local to the
caller.

Built, in the optimized pipeline only
([crates/hd_opt/DESIGN.md](../../crates/hd_opt/DESIGN.md)):

| Pass | What it does | Speed it buys |
| --- | --- | --- |
| `devirt_known` | a `CallValue` whose closure is made in the same sub-body calls the closure's code directly, the closure passed as its environment | one `call_ref` and its code-field read |
| `closure_spec` | a callee of at most 64 TIR instructions that calls a parameter gets one instance per closure literal passed for it, in which those calls are direct; at most two levels deep | the `call_ref`s inside `map`, `filter`, `fold` and similar callees |

Planned, not built:

| Pass | Input | Output | Speed it buys | Size effect |
| --- | --- | --- | --- | --- |
| `const-fold` | the instance's TIR, constants | folded constants, dead branches removed | small | negative |
| `inline-trivial` | a call; the callee's TIR (at most 8 instructions, no loop, no suspension, no closure) | the callee's body in place; collection then skips callees that are always inlined (15 to 25 percent of instances, estimate) | one call per small callee | none or negative |
| `inline-bounded` | a call; the callee's TIR; the caller's size so far; budgets | the callee's body in place | a call, and the chance for the passes below | up to the caller cap (default 2x or 2 KB) |
| `escape` (analysis) | the instance's TIR after inlining | an escape bit per allocation | none by itself | none |
| `scalar-replace` | escape bits | non-escaping closures, cells, boxes, `Iterator`s and other data values as locals | an allocation each | usually negative |
| `devirt-vtable` | a `CallDyn` whose vtable operand is a known constant global | a direct `call` | one `call_ref`; on wasmtime, enables inlining (`mut dyn Hasher` above all) | none |
| `hoist-constants` | a box, enum or tuple construction whose payloads are constants | an immutable constant global | an allocation per evaluation (`.Err(ParseError.Empty)`) | a global per constant |
| `cse-getters` | getter calls (literals, facts) in a body | one call per dominating path; literal getters hoisted out of loops | a call per repeated read | negative |
| `leb-shrink` (link) | padded 5-byte LEB immediates | minimal LEBs, with offset maps for lines | none | negative, several percent |

Merging byte-identical functions (`fold`): not built yet; see #175.

**Closure specialization by inlining** is a schedule of these passes:
`inline-bounded`, `escape`, `scalar-replace`, `devirt_known` and
`devirt-vtable`, then `inline-bounded` again, one round per stage of an
iterator chain, under a size cap per caller. It turns
`xs.iter().map(fn x: x + 1).sum()` from about 7 allocations per chain and
3 `call_ref`s per element into a loop: about 0.3 ns per element on V8,
against 1 to 5 ns with real indirect calls. Fusing adapter chains: not
built yet; see #65.

**Not designed here:** bounds-check hoisting and loop-invariant code
motion, which the engines do in part; whole-program passes, which would
break the caller-local rule; and `wasm-opt` in release.

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
  at most 16 bits are `i31ref`. Wider integers are `i31ref` when the value
  fits in 31 signed bits, else `$Box_i32` or `$Box_i64`, one allocation of
  16 to 24 bytes. Floats are always `$Box_f32` or `$Box_f64`.
  Implementation differs: every erased scalar is an immutable box
  (`Emit::erase`, `layout::box_of`), with no `i31ref` path; worse, since
  each erasure of a small integer allocates. Not built yet; see #175.
- **Type-only:** yes. The static layout is `eqref` for every width; only
  the runtime form (an `i31ref` or a box) depends on the value.

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

A string is a Go-style shared view `(array, start, len)`, and `slice` and
`s[a..b]` are O(1) and share bytes
([`module.string.slice.shared`](../../spec/lang/10-modules.md#r-module.string.slice.shared)).
The layout and its table by position are in crates/hd_wasm/DESIGN.md §15.2.

```wat
(type $bytes (array i8))                      ;; immutable, valid UTF-8 in every viewed range
;; a string: two values (ref $bytes) and i64 span = len << 32 | start
(i32.wrap_i64 (i64.shr_u (local.get $span) (i64.const 32)))     ;; len()
;; s[i]: i < len, else index-out-of-bounds; then bytes[start + i]
(array.get_u $bytes (local.get $b) (i32.add (i32.wrap_i64 (local.get $span)) (local.get $i)))
;; s[a..b]: a <= b <= len and both scalar boundaries, then a new span; no allocation
```

- **Speed:** a slice allocates nothing: a few ALU instructions and the
  checks. `len()` is two instructions; a byte read adds one `add` to the
  start. Equality compares the lengths, returns early when both views
  share one array and one start (`ref.eq`), and otherwise compares bytes
  one per step.
- **Size:** 12 bytes per string at rest in a field or a list element (a
  4-byte reference plus the `i64`), with no header object. Each string
  argument or result is two Wasm values.
- **Why this form.** Against `(ref, i32, i32)` it counts 2 values toward
  the bound, not 3, so `enum Token: Num(i64) | Ident(string) | Op(char) |
  End` and a `(string, string)` map entry stay unboxed. Against one box
  `{bytes, start, len}` a slice allocates nothing (a box costs 3.6 to
  6.8 ns per slice on V8). Erased (`dyn`, `Any`), a string is one box.
- **Engines:** both engines run 64-bit integer operations natively. The
  explicit length check replaces the engine's array bounds check, which
  still guards the array.
- **Boundary:** raw UTF-8 bytes of the viewed range through the exchange
  buffer, with no conversion
  ([`types.string.host-bytes`](../../spec/lang/04-type-system.md#r-types.string.host-bytes)).
  A string from the host is a new exact-size array viewed from start 0.
- **Retention:** a small slice keeps its whole backing array alive, as in
  Go. A known risk, watched by `long-run-memory`.
- **Type-only:** yes.

### Bytes: `List[u8]`

```wat
(type $Arr_u8 (array (mut i8)))
(type $List_u8 (struct (field $len (mut i32)) (field $data (mut (ref $Arr_u8)))))
```

- **Speed:** packed, one byte per element. A host read of 10 MB costs
  about 5 ms of copying on V8 (0.4 to 0.8 ns per byte, one byte per load;
  0.3 to 0.6 with wider loads). `array.copy` is no faster.
- **Size:** a `List` instance at a scalar class.
- **Engines:** the browser cannot build a GC byte array from JS and does
  not need to: the Wasm loop copies from the exchange buffer.
- **Boundary:** length then bytes (§17.4). Parsing in place from the
  exchange buffer is the later step for `serde-throughput`.
- **Type-only:** yes.

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
  Over it, one allocation per construction. Reading a boxed tuple from a
  list is a load, never an allocation, because the list
  stores the same box.
- **Size:** about 2 to 4 bytes per extra value per pass-through within
  the bound. A `List` method over a tuple of `k` values costs about
  `60 + 25k` bytes, one array per value.
- **Engines:** both support multi-value results. Wide multi-value
  returns cost register pressure on Cranelift; the bound keeps them small.
- **Boundary:** elements in order (§17.4).
- **Type-only:** yes. The layout reads the element layouts only.

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
  field shape, shared structurally (§15.3).
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
  Reading one from a list is a load, never an allocation: the box is the
  value in every position.
- **Payloadless variants** of a subtype box are constant singleton
  globals, one per enum type and tag (`GSym::Variant`), local to each
  module as constant closures are: constructing one is a `global.get`.
- **Size:** about 130 bytes per `List` method; under A1 the method is
  shared with every other reference list.
- **Self-recursive** payloads (directly or through other enums) are
  always references, so the layout stays finite.
- **Engines:** an allocation costs more on wasmtime, whose collector
  copies the whole live set; a `ref.cast` to a final type is a header load
  and compare on both.
- **Type-only:** yes. Flat or subtypes depends on the enum's own field
  count; crossing the bound depends on the enum's deep layout. A change
  across the bound cascades to every value type that holds the enum by
  value, which is why there is one bound.

### `Option`

```wat
;; T? with T a non-null reference (data, List, string, a box, a closure): (ref null $T)
;; T? with T a scalar: (i32 tag, T)                  -> two arrays in a list
;; T? with T a dyn pair: (eqref, (ref null $VT))     -> None is a null vtable
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
  one `call_ref`. The planned `devirt-vtable` pass would make a call
  through a known constant vtable direct, which matters most for
  `mut dyn Hasher`.
- **Size:** a 3-slot vtable is about 27 bytes beyond its functions; a call
  site about 8 bytes.
- **Engines:** V8 inlines a monomorphic `call_ref` (0.31 ns, the same as
  inlined); with 8 targets it costs 5.3 ns. wasmtime never inlines it.
- **`is`:** `ref.eq` on the payloads, which is why the payload is
  `eqref`, not `anyref`.
- **`dyn Any`, `Inspectable`:** the value's `Inspectable` dictionary
  gives its `runtime_type`; `downcast` compares it with `T`'s identity,
  then casts or unboxes ([`TypeId`](#typeid)).
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
  25 KB). Planned, not built: a warning past 200 thunks for one method,
  and `hd build --size-report` listing the counts.
- **Engines:** V8 may inline a monomorphic thunk call; wasmtime never does.
- **Type-only:** the thunks are; the witness field index `f_I` is the one
  known exception to stable numbering, limited to erased bodies.

### Closures And Mutably Captured Variables

```wat
(type $Fn (sub (struct (field $code funcref))))
(type $Env_k (sub final $Fn (struct (field $code funcref) (field $count (ref $Cell_i32)))))
(type $Cell_i32 (struct (field $v (mut i32))))
;; call f(x): the code is cast to its final type (cheap on wasmtime, unlike
;; reading a typed funcref field); the code casts the closure to its environment
(call_ref $Code_i32_i32 (local.get $f) (local.get $x) (ref.null $Ctx)
          (ref.cast (ref $Code_i32_i32) (struct.get $Fn $code (local.get $f))))
```

This is the `Wasmtime` form. `V8` and `Browser` keep the typed form,
`(type $Fn_i32_i32 (sub (struct (field $code (ref $Code_i32_i32)))))`,
and call the field without the cast (crates/hd_opt/DESIGN.md (optimization pipelines) §4).

- **Speed:** per call, a load, a cast to the code type (`Wasmtime`
  only), a `call_ref`, and a cast in the callee (skipped when the
  closure has no capture).
  Per construction, one allocation, plus one per shared cell. A
  capture-free closure is a constant global.
- **With passes:** `devirt_known` and `closure_spec` make the call direct
  ([Closure Calls](#closure-calls)).
- **Size:** about 78 bytes for a small closure: type, code, construction.
- **Engines:** V8 inlines a monomorphic call site; wasmtime pays the
  indirect call every time, and it blocks loop optimization across it.
- **Boundary:** no serializable closures in the first release.
- **`T?` of a closure:** `(ref null $Fn)`, a null niche.
- **Type-only:** yes. The code type depends on the signature (rows left
  out, crates/hd_mono/DESIGN.md §12.4); the subtype on the capture shape.

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
  wasmtime unmeasured). Push: amortized one store; growth uses `array.copy`.
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
- **Engines:** bounds checks stay unless the engine proves them; V8 hoists more often. The A1 cast is unmeasured on wasmtime.
- **Type-only:** yes. `List[Point]` is always `{len, (array eqref)}`
  under A1, whatever the program.

### `Map[K, V]`

A compact insertion-ordered map, as CPython's compact dictionary and
JavaScript's `Map` (`layout::map_ty`):

```wat
(type $Map_K_V (struct
  (field $live (mut i32)) (field $used (mut i32))
  (field $index  (mut (ref $Arr_i32)))   ;; twice the entry capacity, a power of two; entry number + 1, 0 empty
  (field $hashes (mut (ref $Arr_i32)))   ;; per entry, insertion order; -1 marks a removed entry
  (field $keys ...) (field $values ...)))  ;; one array per key value and per map value; eqref under A1
```

- **Speed:** a lookup hashes the key, probes `index`, compares the stored
  hash, then compares the key. Removal marks the entry; growth drops
  removed entries and keeps order. Iteration walks entries and skips
  removed ones, in insertion order.
- **Hashing.** An integer, `bool`, `char`, 64-bit integer or string key is
  hashed and compared inline by the map's helpers (`rt::KeyOps`); any
  other key calls its `hash_of` and `Eq.eq` instances. This is better than
  the design, which sent every key through the `Hash` protocol and a
  `dyn Hasher`: common keys hash with no allocation and no indirect call.
  The bucket hash is an implementation detail
  ([`std-hash.map.bucket-hash`](../../spec/std/hash.md#r-std-hash.map.bucket-hash)),
  so this is sound. Implementing `Map` in hd instead of runtime helpers:
  see #180.
- **Size:** the helpers per key layout and map type.
- **Order:** iteration order is deterministic but need not be insertion
  order
  ([`types.map.order.deterministic`](../../spec/lang/04-type-system.md#r-types.map.order.deterministic)),
  so this table is one valid layout, not a requirement.
- **Boundary:** count, then key and value pairs in iteration order;
  decoding calls the key's `Eq` and `Hash` in Wasm.
- **Type-only:** yes.

### `Set[T]`, `Deque[T]` And `Heap[T]`

Today these are std `data` types (`lib/std/collections.hd`): `Set[T]`
over `Map[T, bool]`, `Deque[T]` over `List[T?]` with a head and a count,
and `Heap[T]` over `List[T?]` with a count.

Planned, not built, as a std change:

- `Set[T]` over `Map[T, void]`, whose map has no values array, since
  `Array[void]` has none; `insert` probes once through an internal
  insert-if-absent method.
- `Deque[T]` as a ring buffer over `Array[T]` with a head and a count,
  slots outside `[head, head+count)` holding `default(L)`: one array and
  no tag test per access, where `List[T?]` costs two arrays and a `match`
  for a scalar `T`.
- `Heap[T]` over `List[T]`: a comparison is two array reads and the
  type's own `cmp`, with no `match` per read.

### Iterators And Ranges

```wat
;; Iterator[T] = data { step: fn() -> T? }   -> (ref $Iterator_T) holding (ref $Fn_unit_OptT)
;; for i in a..b, a..=b over a numeric type -> a counted loop; no range value exists
;; Range[T] as a value -> data { start, end, inclusive }; one allocation in the baseline
```

- **Speed:** in the baseline, `xs.iter().map(f).sum()` costs about 7
  allocations per chain (3 closures, 3 iterators, 1 cell) and 3
  `call_ref`s, 3 casts and 2 option tests per element. `closure_spec`
  makes the user closure's calls direct inside the adapters it fits.
- **Size:** about 30 bytes per call into shared adapters.
- **Ranges in slices:** `items[1..3]` builds a `Range` data value for the
  `Index` impl.
- **Engines:** V8 hides one monomorphic `call_ref` per stage; wasmtime
  pays each one and loses loop optimizations across it.
- **Type-only:** yes; specialization makes code, never layouts.

### Newtypes

```wat
;; type Mile(i32) -> i32;  type Order(Draft) -> (ref $Draft): no wrapper, no allocation
```

- **Speed, size:** free; construction and unwrapping emit nothing.
- **`TypeId`:** distinct, by `canon(Mile)`.
- **Engines:** no difference.
- **Type-only:** yes.

### `TypeId`

```wat
(type $TypeId (struct (field $id i64) (field $name (ref $LitFn))))   ;; $LitFn: the name as a string view
(global $tid_Point (ref $TypeId) (struct.new $TypeId (i64.const 0x9f3a...) (ref.func $lit_Point)))
;; ==: compare $id; Hash: write_u64($id); Display: call_ref the name getter
```

- **The id** is the first 64 bits of `H(canon(T))`, so it depends only on
  the type. Link checks the program's ids for collisions; on one, link
  rehashes every id with the next fixed salt, deterministically. The spec
  promises nothing across builds
  ([`trait.typeid.cross-build`](../../spec/lang/09-traits.md#r-trait.typeid.cross-build)).
- **Speed:** equality is one `i64.eq`; no allocation.
- **Size:** about 15 bytes per type that reaches a type-id read.
- **Type-only:** yes.

Implementation differs: a `TypeId` is a std `data` value of two strings,
the type's identity key (its full name) and its printed name
(`Emit::type_id_parts`), built by each `TypeId::of` and `runtime_type`
call. Worse: every call allocates, and equality compares strings. Not
built yet; see #175.

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
;; f$body, f$cold, f$poll, f$cancel per suspending instance (crates/hd_wasm/DESIGN.md §14.1)
```

- **Speed:** the ready path is a call and a null test; the pending path
  allocates one frame per level, once.
- **Size:** four functions per suspending instance.
- **Engines:** no difference in shape; the browser drives `hd.poll` from the worker's event loop (§17.6).
- **Type-only:** yes.

### Facts

A fact's body is its own getter (`OnceCell` in `hd_wasm` emission): a
flag global, and one global per Wasm value of its result.

```wat
(func $fact_k (result (ref $T))          ;; readers call it
  (if (global.get $fact_k_done) (then (return (global.get $fact_k_value))))
  ... run the fact body inside the forbidden-context counter, store, set the flag, return)
```

- **Speed:** per read, a direct call, a `global.get`, a test and a branch.
- **Size:** the flag and value globals, and a few bytes per read.
- **Engines:** no difference.
- **Type-only:** not a type question. The getter keeps the fact's global
  indices out of every reader, which is what stable numbering asks.

The design used null as the flag of a fact with one reference layout;
the code keeps an `i32` flag for every fact. Equal: one more global per
fact, and one shape for facts and structure handles alike.

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
    (call $lit_fill (global.get $lits_m) (local.get $k))))       ;; shared: array.new_data from segment 0
;; a use of m's literal number k:      (call $lit_m (i32.const k)) (i64.const len_k << 32)
;; a literal is a view over its pooled array, from start 0; slicing it shares the pooled bytes
```

**One table per module, not one getter per literal.** At 10k lines a
getter per literal is about 2,000 tiny functions, and each one pays
Cranelift's fixed cost per function and a member of the Cranelift cache;
wasmtime does not inline across functions, so nothing removes them. With
one table per module, m's pooled literals are numbered in content order
within m, so a number changes only when m's own set of literals changes,
and no body holds a program-wide number.

| Cost (`ordinary-10k`: about 2,000 literals, 3,000 uses, 140 modules; estimates) | One getter per literal | One table per module |
| --- | --- | --- |
| functions added | about 2,000 | about 140 |
| code bytes | about 41 KB | about 37 KB |
| Cranelift, cold | 20 to 60 ms of CPU | 1.4 to 4 ms |
| hit path | call, null test | call, array read with bounds check, null test |

- **Host fast path:** `println` of a literal copies its bytes from the
  segment into the exchange buffer with `memory.init`, so no array is
  built and hello world needs no pool.
- **Implementation differs:** one table per linked module, numbered at
  link through a `Reloc::Lit` immediate, so no cached body holds a
  number either; and no host fast path
  ([crates/hd_wasm/DESIGN.md §15.4](../../crates/hd_wasm/DESIGN.md)).
- **List and map literals** build fresh values each evaluation, since
  lists and maps are mutable.
- **Engines:** `array.new_data` is not a constant instruction, which is
  why long literals cannot be constant globals on either engine.
- **Type-only:** not a type question.

Implementation differs (#258): each distinct literal has its own getter function
and its own lazily filled global (`Helper::Lit`, `rt::lit_code`), over
one deduplicated passive segment, and there is no `memory.init` fast path.
Worse by the cost model above: about 2,000 functions at 10k lines, each
with Cranelift's fixed cost.

## Expressions And Statements

For a syntax form, "type-only" asks whether the emitted code depends only
on the instance's own TIR, its type arguments and the layouts and impls
it reads, which the code key lists (crates/hd_mono/DESIGN.md §13.8).

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
;; primitives: i32.eq, i64.lt_s, f64.eq ...; string ==: the StrEq helper ([string](#string))
;; data or enum ==: a direct call of the selected Eq impl
;; a and b: (if (result i32) (local.get $a) (then (local.get $b)) (else (i32.const 0)))
```

- **Speed:** primitives free; strings one byte per step; user `Eq` a
  direct call.
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
;; for i in a..b: i = a; end = b; loop: if i >= end break; bind i; advance; body
;; for x in items: len captured; loop over the backing array; compare len each step (iterator-invalidated)
;; for (k, v) in m: loop over entries, skipping holes; the same length check
;; for x in it (an Iterator): call step until .None
;; while c: (block $exit (loop $top (br_if $exit (i32.eqz c)) body (br $top)))
```

- **Speed:** counted loops allocate nothing in either pipeline; this is a
  lowering rule, not an optimization. A loop over a list under A1 casts
  each element only where the body needs its exact type.
- **Size:** about 20 to 30 bytes of loop scaffolding.
- **Engines:** Cranelift does not vectorize; V8 neither for Wasm GC
  arrays.
- **Type-only:** yes.

A numeric range loop advances before the body runs, so `continue` needs
no step of its own, and at an inclusive end it stops instead of adding,
so `0..=255u8` cannot overflow (`hd_check::expr`, `range_loop`).
Implementation differs: only a numeric `a..b` or `a..=b` bound to a plain
name is a counted loop. A `for` over a list or a map calls its iterator: a
cursor closure (`ListStep`, `MapStep`) allocated per loop and a
`call_ref` plus an `Option` per element. Worse: the `allocations` target
asks for no allocation in these loops.

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
```

- **Speed:** static and bound calls are direct; a `dyn` call is a load
  and a `call_ref`.
- **Size:** a direct call is about 3 bytes after LEB shrinking, 6 with the
  padded slot; a `dyn` call about 8.
- **Engines:** V8 inlines monomorphic `call_ref`s; wasmtime never does,
  so devirtualization is the compiler's job.
- **Type-only:** yes. The callee is what collection selected, recorded in
  the code key.

### Closure Calls

```wat
;; baseline: the code field, call_ref with the closure first, cast in the callee
;; devirt_known: a closure made in the same sub-body: call its code directly
;; closure_spec: a closure literal passed to a small callee: the callee's specialized
;;   instance calls the literal's code directly
```

- **Speed:** baseline, one `call_ref` and one cast per call (plus a
  `ref.cast` of the code field on wasmtime); direct when a pass applies.
- **Size:** one specialized instance per (callee, closure literal).
- **Engines:** the passes matter on wasmtime; V8 already inlines
  monomorphic calls at run time.
- **Type-only:** yes; specialization makes instances, never layouts, and
  its budget is local to the caller.

### Providers: `$.with`, `$.use` And Rows

```wat
;; a concrete row: one parameter per key, in canon(K) order; a provider is a dyn pair
(call $load_user (local.get $id) (local.get $db_payload) (local.get $db_vt))
;; a function value's context: (array i64) key ids and (array eqref) providers, after its arguments
```

- **Speed:** a concrete row costs one argument pair per key and no
  allocation.
- **Function values.** Every closure code takes the key ids and providers
  of the caller's row (`layout::ctx_keys`, `ctx_provs`), so a value of any
  narrower row looks its own keys up and row subsumption is no
  instruction.
- **Row-polymorphic bodies.** One body per row parameter, taking one
  context parameter. Not built yet; see #176: today a row-polymorphic body
  is compiled once per row instantiation.
- **Type-only:** yes. Key ids are the low bits of `H(canon(K))`, which
  covers the trait's arguments, so a new key elsewhere in the program
  changes no body.

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
  functions per suspending instance (crates/hd_wasm/DESIGN.md §14.1).
- **Engines:** `block_on` uses JSPI in the browser, else a synchronous
  same-origin XHR, else a `host-contract` panic (§17.6). Implementation
  differs: the browser host always uses the synchronous XHR from its
  worker and has no JSPI path; not judged.
- **Boundary:** a waiting host call is `.start` and `.finish` (§17.2).
- **Type-only:** yes.

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
```

- **Speed:** a call per omitted argument.
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
;; per registered case, hd.test.i: its TestCase body evaluates the registration's run-time
;; arguments and calls the std registration function, which drives the test body
;; per module with tests, hd.init.j: the init groups module j reaches
```

- **Speed:** a fresh instance per case, through `InstancePre` and the
  pooling allocator.
- **Size:** a filtered program holds only the selected cases' code; it
  shares every code entry with the full program.
- **Type-only:** yes.

### Derives

```wat
;; @derive(Debug, Eq, Hash) on data Point: ordinary instances of the templates;
;; each member call (w.member(h, value)) is a direct call
```

- **Speed:** a direct call per member. Derived hashing writes through
  `Hasher.write(List[u8])`, which builds a byte list per field.
- **Size:** derived code is about 27 percent of a 10k-line program's
  instances (estimate); derives are about 1.8x hand-written code (#175).
- **Engines:** no difference.
- **Type-only:** yes.

Planned, not built: fixed-width `Hasher` methods (`write_u8`, `write_u16`,
`write_u32`, `write_u64`, `write_str`) with default bodies that write
today's bytes, so derived and std `Hash` impls allocate nothing. That is a
std change and a change to the trait in
[Hashing](../../spec/lang/09-traits.md#hashing).

### Explicit Panics And Unreachable Code

```wat
(call $panic_explicit (... message in the exchange buffer ...))   ;; stores the category, then unreachable
```

- **Speed:** a panic traps; the host reads the category global and the
  buffer, never calling into the poisoned instance.
- **Size:** one `call` per site; the stub once per category.
- **Engines:** wasmtime's `WasmBacktrace` and V8's `Error.stack` both give
  the caller frame's code offset, which maps to the site.
- **Type-only:** yes.

Implementation differs: a panic stub calls the `hd:rt` `panic` import,
which writes `panic: `, writes `CATEGORY: MESSAGE` through the `hd:rt`
`stderr` import, then traps (`Helper::Panic`, `PanicHead`). There is no
category global: the host records the report text from the `panic` call
to the trap, and runners read the category and message it recorded,
never standard error. The panic site comes from the trap's frames
through `hd.lines`, and there is no `hd.sites` table.
