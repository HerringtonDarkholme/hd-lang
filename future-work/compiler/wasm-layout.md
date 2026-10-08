# New Compiler Design: Wasm GC Layout And Emission

Part of the [compiler design](README.md).

## 15. Wasm GC Layout And Emission

### 15.1 Layout Classes

Each instance's types map to a layout. A layout is one or more Wasm value
types in locals, parameters and results, and one or more fields in a
struct or an array element.

| Class | Wasm values | hd types |
| --- | --- | --- |
| `i32` | `i32` | `bool`, `char`, integers of 32 bits or less, `usize`, payloadless enums |
| `i64` | `i64` | `i64`, `u64` |
| `f32`, `f64` | `f32`, `f64` | floats |
| `ref` | `(ref $T)` or `(ref null $T)` | data, lists, maps, closures, frames, and the box of a value layout over the bound or of a self-recursive enum |
| `multi` | 2 Wasm values up to the bound | `string` (a view), `Option` of a scalar, `Result`, tuples, value enums, trait values (§15.2) |
| `erased` | `eqref` | the payload of a trait value, `Any`, an open value in an erased method body (codegen.md §13.5.1) |
| `void` | none | `void`, `()`, and `never` |

**One representation per type in every position (lowering pass,
2026-10-07).** A type has the same Wasm values in a local, a parameter,
a result, a field and an array element, apart from packing scalars (`i8`
for `bool`). A value layout within the **bound** is several values,
several fields, or a structure of arrays. A value layout over the bound
is one immutable box struct in every position, fields and arrays
included. An earlier draft boxed only in locals and kept fields and
arrays unboxed, so every read of such an element allocated a box; the
runtime study ([representation-runtime.md](representation-runtime.md))
found that. The spec allows either form
([Shapes and Generic Code](../../spec/lang/04-type-system.md#shapes-and-generic-code)).

- **The bound** counts Wasm values after slot sharing, the tag included,
  and counts a boxed payload as one value. **Default 4, decided by E2**
  of spike 0c: the largest payload count where parallel arrays win a
  read-only kernel by more than 10 percent on wasmtime and `push` stays
  under 256 bytes. There is one bound, because crossing it changes the
  layout of every value type that holds the type by value.
- **Slot sharing.** Payload fields of different variants with the same
  Wasm value type share a slot. Reference fields share reference slots:
  the slot is typed exactly when every variant's field in it has one
  reference type, and `eqref` with a `ref.cast` after the tag test
  otherwise ([research-enum-values.md §5.1](research-enum-values.md#51-layouts)).
- **Erased storage (A1).** A struct field or array element whose declared
  type is a type parameter, instantiated at a layout of one reference
  (nullable or not), is stored as `eqref`, and readers cast at the first
  use that needs the exact type. So `List[Point]` is always `{len,
  (array eqref)}`. Scalars and `multi` layouts keep exact storage.
  **Default adopted, decided by E1:** rejected if the wasmtime read
  penalty is over 1 ns per element or over 5 percent on the `sort` and
  `map` microbenchmarks with reference elements. Collection then gives
  move-only generic bodies one instance per class (codegen.md §13.2).
- Every rule here reads only the type's definition and the definitions
  its fields name, never a use or the program, so a cached instance is
  valid in every program that reaches it. Each data type and syntax
  form's cost is in [lowering-catalog.md](lowering-catalog.md).

### 15.2 Values

| hd value | In locals and results | In a field or array element |
| --- | --- | --- |
| integers, `bool`, `char`, floats | the scalar | packed: `i8` for `bool`, `u8`, `i8`; `i16` for 16-bit integers; else the scalar |
| `data T` | `(ref $T)`: a struct with one mutable field per declared field, in declaration order | same |
| embedded part | a separate struct, referenced by an immutable field of the outer struct ([`data.part.unobservable`](../../spec/lang/08-data-and-enums.md#r-data.part.unobservable)) | same |
| payloadless enum | `i32` tag | `i8` or `i16` when the variant count fits |
| enum with payloads, within the bound | `multi`: `(i32 tag, shared payload slots...)` (§15.1) | the same fields; a structure of arrays in an array |
| enum box, **flat** (over the bound, at most 4 payload fields in total) | one immutable struct: tag plus the union of payload fields, unused fields null or zero | same: the box reference |
| enum box, **subtypes** (over the bound with more fields, or self-recursive) | a non-final base struct (tag, shared fields) and one final subtype per payload variant; payloadless variants are constant singletons | same: the box reference |
| `T?`, `T` a non-nullable reference (data, list, string, closure, a box) | `(ref null $T)`; null is `.None` | same |
| `T?`, `T` a trait value | `(eqref, (ref null $VT))`; a null vtable is `.None` (lowering pass) | two fields |
| `T?`, `T` a scalar | `(i32 tag, T)` | two fields |
| `T??` and `Option` of a `multi` | a tag plus the inner layout, boxed over the bound | the same fields |
| `Result[T, E]` | `multi`: `(i32 tag, dflt(T')..., dflt(E')...)` with slot sharing, where `T'` and `E'` are the payload layouts (see "Identity" and "Defaultable forms" below) | the same fields |
| tuple | its elements' values within the bound; one immutable box over it | its elements' fields, flattened, or the box |
| trait value, `Any` | `(eqref, (ref $VT))`; `eqref`, so `is` can use `ref.eq` on the payload | two fields |
| closure | `(ref $Fn_sig)`: a base struct holding the code as a typed function reference; one subtype per capture shape | same |
| capture-free closure | a constant global of the base type, with no environment | same |
| `string` | a view of two values, `(ref $bytes, i64 span)`: `$bytes = (array i8)`, immutable, valid UTF-8 in the viewed range; `span` packs `len << 32 \| start` (owner, 2026-10-07: Go-style shared slices; "Strings" below) | two fields, `(mut (ref $bytes))` and `(mut i64)`; in an array, a structure of two arrays |
| `string?` | `(ref null $bytes, i64)`; a null `bytes` is `.None` | two fields |
| `List[T]` | `(ref $List_T)` = struct `{len: mut i32, data: mut (ref $Arr_T)}`; under A1 every reference `T` shares `$List_eq` over `(array (mut eqref))` | same |
| `Map[K, V]` | std hd over arrays (§16.1): a compact insertion-ordered table of `index`, `hashes`, `keys` and `values` arrays ([lowering-catalog.md](lowering-catalog.md#mapk-v)) | same |
| `TypeId` | `(ref $TypeId)`, a constant global per type: `{id: i64, name: (ref $LitFn)}`, `id` the first 64 bits of `H(canon(T))` | same |
| `mut Suspend[T]` | `(ref $Suspend_L)` (§14.1) | same |

**Strings (owner, 2026-10-07: Go-style shared slices).** A string is a
view of a byte array: `(array, start, len)`. `slice` and `s[a..b]` are
O(1) and share bytes, as
[`module.string.slice.shared`](../../spec/lang/10-modules.md#r-module.string.slice.shared)
says. Wasm GC has no interior pointers, so the view needs the array, a
start and a length; Go's two words become hd's `(ref $bytes, i64 span)`,
with `start` in the low 32 bits and `len` in the high 32 (`usize` is 32
bits on Wasm32). The layout is the same in every position, as for every
value layout (§15.1):

| Position | Representation | Bytes at rest |
| --- | --- | --- |
| local, parameter, result | two values: `(ref $bytes)`, `i64` | none |
| field of a `data` struct | two fields, `(mut (ref $bytes))` and `(mut i64)` | 12 (a 4-byte reference on both engines, plus 8) |
| element of `List[string]`, `Array[string]` | a structure of two arrays, `(array (mut (ref null $bytes)))` and `(array (mut i64))` | 12 per element |
| enum payload, `Result`, tuple | two slots: the reference slot is shared with other references (`eqref` and a cast after the tag test), the `i64` slot with other `i64` payloads | counts 2 toward the bound |
| `string?` | `(ref null $bytes, i64)`; null is `.None` | as `string` |
| erased (`dyn`, `Any`) | `$Box_str`, one immutable `{bytes, span}` struct: one allocation of about 16 to 24 bytes | the box |
| a pooled literal | a view over its pooled array: start 0, `len` its length (§15.4) | none per use |

- **Operations.** `len()` is `span >> 32`, two ALU instructions.
  `s[i]` checks `i < len` (the array may be longer than the view, so the
  engine's bounds check is not enough), then reads `bytes[start + i]`.
  `s[a..b]` checks `a <= b <= len` and both scalar boundaries, then
  builds the new span; it allocates nothing. Concatenation and the
  builder allocate one exact-size array and a span with start 0.
- **`==` and hashing** read only the viewed bytes. `==` compares the
  lengths, takes a fast path when both views share one array and one
  start (`ref.eq`), and otherwise compares bytes. `Hash` writes the
  viewed bytes, so a slice hashes as an equal fresh string does, and the
  backing array never shows.
- **Why two values, not three or a box** (cost numbers in
  [lowering-catalog.md](lowering-catalog.md#string)). Three values
  `(ref, i32, i32)` cost the same 12 bytes in a field but count 3 toward
  the bound of 4, so the catalog's `Token` enum (an `i64`, a `string` and
  a `char` payload) and a `(string, string)` map entry go over it and
  allocate a box per value. Packing `start` and `len` into one `i64` keeps
  both within the bound for one or two ALU instructions per `len()` or
  `start` read, hoisted in loops. A boxed `{bytes, start, len}` struct in
  every position allocates on every slice (3.6 to 6.8 ns on V8) and two
  objects per fresh string. Boxing only in fields and arrays, with values
  in locals, breaks the one-representation rule: every store of a slice
  into a field or a list would allocate.
- **Retention (known risk, no mitigation in the first release).** A small
  slice keeps its whole backing array alive, as in Go: a 10-byte token
  from a 10 MB input pins 10 MB. `long-run-memory` watches it. Copying a
  slice that must outlive its source is the user's fix, as Go's
  `strings.Clone` is.
- **Type-only:** yes. The layout depends on nothing but the type.

**Defaultable forms (Codex re-review N3).** A language layout may hold
a non-null reference: data, lists, strings, closures. Wasm has no default
value for one, so a slot that may be inactive cannot hold it. Each layout
`L` therefore has a defaultable form `dflt(L)`, the same Wasm values with
each non-null reference made nullable, and `default(L)` fills it with
null or zero. The rule is applied recursively, wherever a slot can be
inactive:

| Slot | Form | Narrowed by |
| --- | --- | --- |
| an inactive payload of a value enum or `Result` (`multi` layout), and the payload fields of a flat enum struct | `dflt` per payload field; the inactive variant's fields hold `default` | the tag test, then `ref.as_non_null` on the read field |
| the Pending result and the resume arguments of `f$body`, every saved field of a suspension frame, the result fields of an `all!` frame | `dflt` (suspension.md §14.1) | the state test or the done mask |
| a module storage global before init | `dflt` (§15.4) | init order; reads after init narrow |
| an `Array[T]` slot past the count that `List`, `Map` or `Deque` keeps (lowering pass) | `dflt` per element value; such slots hold `default` and std never reads them | std's count check, then `ref.as_non_null`, or the A1 `ref.cast` to a non-null type, which checks both |

Language-level locals, parameters and fields keep the non-null form, so a
value that is always present costs nothing. A narrowing read is one
`ref.as_non_null`, which cannot fail after the test. `struct.new_default`
is valid for every frame type.

**M4b local gap (M4b gap 6).** M4b declares every reference local in its
defaultable nullable form and emits `ref.as_non_null` on each read. That
keeps validation simple, but adds redundant casts to locals whose value
is always present. The non-null language-local rule above remains the
target; only genuinely inactive slots use `dflt`.

**Enum layout per enum (mine).** Every enum is an identity-free value
(S1c) and normally uses the value layout of "One predicate for
identity-free enums" below. The flat and subtype rows above are the
shapes of its **box**, used past the bound (§15.1) and for a self-recursive
payload. The rule is deterministic, with no annotation: a box is
**flat** when the enum's payload fields number at most 4 in total.
Otherwise it uses **subtypes**. Flat enums need no
cast on a match, and their payloadless variants need no allocation.
Subtype enums cast once per matching arm, which the engine checks with
one load and compare.

**Erased scalars.** In an erased position, `bool`, `char` and integers of
16 bits or less become `i31ref`. Wider integers (`i32`, `u32`, `usize`,
`i64`, `u64`) become `i31ref` when the value fits in 31 signed bits and
are boxed in `$Box_i32` or `$Box_i64` otherwise (owner, 2026-10-06,
reaffirmed 2026-10-07). Erasing checks the range, which is a shift, a
shift back and a compare, and allocates only on a miss. Unboxing is
`br_on_cast` to `i31`, then `i31.get_s` and an extend; a miss takes
`ref.cast` to the box and `struct.get`. A box-only layout would need
that `ref.cast` on every unbox anyway, so the fast path costs nothing
extra. Floats are always boxed in `$Box_f32` or `$Box_f64`. The static
layout is `eqref` for every erased scalar, so layout stays type-only;
only the runtime form depends on the value. This is where the owner's
"at least `i31ref`" lands: in monomorphized code no scalar is boxed at
all. A string is boxed in `$Box_str`, an immutable `{bytes, span}`
struct, as a tuple is boxed in one immutable struct.

**M4b erased-scalar gap (M4b gap 3).** The current erased path always
allocates a scalar box. It does not emit the `i31ref` range fast path or
the corresponding unbox branch. The mixed `i31ref`/box representation
above remains required.

**Identity (owner decision B, extended, 2026-10-07).** The Codex review (finding 1)
showed that the rows above broke the spec's allocation identity: the spec
gives each `.Some(...)` and each primitive-to-`Any` box its own identity
(`expr.is.some`, `expr.is.box`, `expr.is.box.distinct`). The owner chose
to change the language instead of the layouts. `.Some`, `.Ok`, `.Err`
and primitive, string or tuple boxes have no identity. `is` is a compile
error on an operand whose static type is a value type and on function
values; since S1c that covers every enum, optionals and `Result`
included. On an `Any` or trait value that holds a value at run time,
the result is unspecified (Codex review, finding 1). The spec has this
as
[`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand),
[`expr.is.box-values`](../../spec/lang/05-expressions.md#r-expr.is.box-values)
and
[`expr.is.box-unspecified`](../../spec/lang/05-expressions.md#r-expr.is.box-unspecified).
The other option was to keep the spec and allocate one object per `.Some`
and per box wherever identity could be observed. That costs one 16 to
24 byte allocation per `.Some` that crosses a call, which `T?`-heavy code
pays on every lookup. B was chosen because it costs nothing and an
optional's identity has no known use.

So the layouts follow these rules:

1. **`T?`.** `.Some(x)` is `x` itself for a reference `T` and `(1, x)`
   for a scalar `T`. Nothing is allocated.
2. **Boxes.** A primitive, string or tuple converted to a trait value or
   `Any` is erased as above. Equal small scalars give equal `i31ref`s, and
   two boxes of one value are two objects. The proposed spec change makes
   `is` on such values unspecified, as it already is for function values.
3. **`Result` has no identity either** (open question 23.1-7, answered
   yes). It uses the `multi` layout `(i32 tag, T', E')` and allocates
   nothing. A field that holds a `Result` stores the same fields
   inline, or the box past the bound. Its payload fields share slots
   by §15.1's slot-sharing rule: when `T'` and `E'` are references of
   different types, they share one `eqref` slot cast after the tag test
   (lowering pass; an earlier draft kept them apart).
4. **Other enums** are identity-free too (S1c). They follow the
   predicate below; the flat and subtype shapes are only their boxes.

**One predicate for identity-free enums (mine).** Layout, `is` lowering
and the checker's value-type test read one predicate, `identity_free(E)`.
Since the S1c spec pass it holds for every enum
([`types.sealed.anyval-values`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-values)).
An identity-free enum
uses a value layout: a nullable reference when it has one reference
payload and one payloadless variant (`T?`), else `(i32 tag, payload
fields...)` as `multi`, boxed in one immutable struct past the bound in every position
as §15.1 says. A self-recursive payload stays a reference, as the flat
or subtype layout above has it.

**Representation equivalence (Codex re-review N-B2).** Identity-free
means the enum's own shell may be copied, deduplicated or hoisted into a
constant. It does not mean a referenced object may be replaced by an
equal one. Two enum values are interchangeable only when their value
components are equal by representation and their reference components
are the same objects (`ref.eq`); hd's `Eq` is never the test. Constant
hoisting, enum hash-consing and the fact graph (codegen.md §12.3) all
use this rule.

**Emitting `is`.** The TIR `Is` instruction
([checking-and-tir.md](checking-and-tir.md#instruction-catalog)) lowers by
the operands' layout:

| Operand layout | Wasm |
| --- | --- |
| `ref` (data, lists, maps, an `AnyRef` type parameter's instance) | `ref.eq` |
| trait value, `Any` | `ref.eq` on the payloads; unspecified when the payload is a boxed value |

An enum, optional, `Result`, tuple, primitive, string or function operand
has no row: `is` on it is the compile error
`identity-requires-references` (S1c,
[`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)).
No type-id test is needed in the last row. Two different objects are
never `ref.eq`, and two equal `i31ref`s of different types are boxed
values, whose result is unspecified.

**Arrays.** `Array[T]` is the one compiler-known collection: `(array (mut
T'))` with `T'` the element layout of the table. An array of a `multi`
layout, such as `Array[Option[i32]]` or `Array[(i32, f64)]`, is a struct
of one array per Wasm value (mine): a structure of arrays, which keeps
every element unboxed, as the spec's packed `List[i32]` and list of
tuples ask, and allocates nothing per element. `Array[T]` is intrinsic,
so `List[T]` and `Map[K, V]` in std see one type either way.

- **Element forms (lowering pass).** An element is stored in its
  defaultable form, since slots past a collection's count hold
  `default(L)`. Under A1 an element of a reference layout is `eqref`.
  An element over the bound is the box reference.
- **`Array[void]`** has no Wasm array: it is one shared empty constant
  struct, and its operations emit nothing. So `Map[T, void]`, which
  `Set[T]` wraps, has no values array, with no special case.
- **Collections over plain storage (lowering pass; std work, not done
  here).** `Heap[T]` is a `List[T]` with sift operations, and `Deque[T]`
  a ring buffer over `Array[T]` with a head and a count. Neither stores
  `T?`, which for a scalar `T` doubled the arrays and added a tag test
  per access. `Set[T]` wraps `Map[T, void]` with an internal
  insert-if-absent, so `insert` probes once. `Map` is the compact
  insertion-ordered table of [lowering-catalog.md](lowering-catalog.md#mapk-v).

### 15.3 The Type Section

1. **Canonical descriptors.** The linker maps each hd type to a canonical
   Wasm type descriptor: kind, fields with mutability and storage type,
   supertype, finality. Structurally equal descriptors are one type.
2. **Structural, not nominal (mine).** hd never tests an object's Wasm
   type to learn its hd type. Enum variants use the tag, and `Any` uses
   the vtable's type id. So two hd types with the same layout may share a
   Wasm type, which shrinks the type section and lets instances fold
   (§13.7).
3. **Rec groups.** The type graph's strongly connected components become
   rec groups; a non-recursive type is a group of one. Members of a group
   are ordered first so that a declared supertype precedes its subtypes,
   then by the members' `canon(T)` bytes (codegen.md §13.3), a nominal
   key that exists before any Wasm index does. Two groups are one type
   when their encoded bytes under that order are equal. Recognizing equal
   recursive shapes under different member names is deferred until the
   size measurements ask for it (Codex re-review N-B1, N-S3).
4. **Subtyping.** Declared only where hd needs it: enum variants under
   their base, frames under `$Suspend_L`, closure environments under
   `$Fn_sig`. Every leaf is `final`, which lets engines skip subtype
   checks.
5. **Order.** A Wasm type may only name types of its own group or of
   earlier groups, so groups follow a topological order of the SCC
   condensation graph: a group comes after every group it references.
   Among groups that are ready at the same step, the one with the
   smaller encoded bytes comes first (Kahn's algorithm with a sorted
   ready set). The type section is then a pure function of the type set,
   and valid by construction
   ([Wasm type validity](https://webassembly.github.io/spec/core/valid/types.html)).
6. **Stability (lowering pass).** No dense order is stable under
   insertion: a new type that sorts early shifts the type immediates of
   later functions, and wasmtime's per-function cache keys include them.
   The first release keeps this order and measures the misses (S2 of
   spike 0c). If fewer than 90 percent of functions hit after adding one
   type, allocation and cast sites name types through a call to a
   per-type helper, or the cache key is asked to abstract type indices
   upstream. Under A1, fewer types exist, so fewer shift.

### 15.4 Globals And Module Initialization

| Global | Wasm | Initialized by | A reader holds |
| --- | --- | --- | --- |
| runtime state: panic category, the wake table, the forbidden-context counter | mutable | constants | `global.get` of a fixed low index |
| one literal table per source module | immutable `(ref $LitTab)`: a `(ref $Pool)`, an `(array (mut (ref null $bytes)))` with one slot per pooled literal of the module, and the module's index-area offset | a constant expression (`struct.new`, `array.new_default`) | nothing: only the module's literal getter reads it |
| vtables, capture-free closures, payloadless variant singletons, member handles, witnesses, `TypeId` values | immutable | a constant expression (`struct.new` and `ref.func` are constant in Wasm GC) | `global.get`; globals of a kind ordered by content key |
| string literals of 4 bytes or less (default, decided by E10) | immutable | a constant expression (`array.new_fixed`) | `global.get`, then `i64.const` of the literal's span (`len << 32`) |
| other string literals | a slot of their module's table | lazily: the module's getter runs `array.new_data` on segment 0 at the literal's offset | `i32.const` of the module-local number, a `call` of the module's getter, then `i64.const` of the span: a literal is a view over its pooled array |
| module storage (top-level bindings) | mutable, nullable or zero | the group's init function | `global.get`, `global.set`; ordered by module path, then binding index |
| fact values and metadata | mutable, nullable; null is the flag for a one-reference layout, else an `i32` flag | lazily: a getter runs the fact's body on the first read (codegen.md §12.3) | a `call` of the getter |
| shared enum data | mutable, nullable or zero | the declaring module's group init function ([`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init)) | `global.get` |

**M4b vtable gap (M4b gap 2).** M4b does not place vtables in immutable
globals. Each coercion executes `struct.new`, and the struct omits the
direct-supertrait vtable fields required by codegen.md §13.5. The global
row above remains the intended allocation and layout rule.

**Stable numbering (lowering pass).** A reader never holds the index of
a lazily initialized global: it calls a getter, and wasmtime's
per-function cache abstracts call targets, so adding a literal or a fact
elsewhere recompiles no other function. "Append-friendly" global order
would need numbers kept from earlier builds, which §15.8 forbids. The
immutable constants are read directly by default; S2 of spike 0c moves a
global kind behind getters if adding one global leaves fewer than 95
percent of functions hitting the cache. The full table of index spaces is
in [lowering-catalog.md](lowering-catalog.md#numbering).

**Constant strings (lowering pass; per-module tables after the systems
review).** All pooled literals share one passive data segment,
deduplicated by content, so the program has one data segment. After the
bytes come each module's index area: an `(offset, length)` pair per
module-local literal number. Each module has one literal table and one
getter, `$lit_m(k)`, which reads slot `k` and on null calls one shared
fill helper. Module-local numbers depend only on the module's own
literals, so they are stable under every edit elsewhere
([lowering-catalog.md](lowering-catalog.md#literals) compares this with
one getter per literal). A function reads each literal once on a
dominating path. A host call whose argument is a literal, such as
`println("hello")`, takes the host fast path: the module's span function
copies the bytes from segment 0 into the exchange buffer with
`memory.init` and returns the length, so no array is built at all, and
the offset stays inside that function. Eager filling of the tables in
`hd.init` is measured by E10.

- **Init order.** The `hd.init` export calls each reachable group's init
  function once, in D1's order (`InitOrder` and M3), after every group it
  uses ([`module.init.cycle.once`](../../spec/lang/10-modules.md#r-module.init.cycle.once)).
- **No Wasm `start` function** (mine). Init can panic and can call
  imports. Calling it as an export after instantiation lets the host
  refuse a start, attribute a trap to init, and run a test case's init
  inside the case's time limit.
- Initialization is synchronous: module top level is not a driver
  context.

### 15.5 Panic Sites And Backtraces

**Explicit panics.** A panic stub writes the category id into a global,
copies the message into the exchange buffer, sets its length, and
executes `unreachable`. The host reads the global and the buffer after
the trap, so it never calls into a poisoned instance
([`flow.panic.poison`](../../spec/lang/06-control-flow.md#r-flow.panic.poison)).

**No site numbers (lowering pass).** A call of a stub carries no site
immediate. The site is the stub's caller frame: its function index and
the code offset of the `call`, which both engines' backtraces give, and
`hd.sites` is keyed by function and code offset. So no body holds a
program-wide site number, and a new site elsewhere recompiles nothing
else (the compile study found that site numbers alone made one edit miss
the per-function cache for about half a debug program). Each check keeps
its own `call` instruction, so two sites never share an offset. Category
ids are fixed by the runtime, not numbered per program.

**Engine traps.** A trap without a stored category is mapped by its trap
code and its code offset:

| Trap | Category |
| --- | --- |
| integer division by zero | `integer-division-by-zero` |
| array out of bounds (string byte access) | `index-out-of-bounds` |
| call stack exhausted (wasmtime's trap code; V8's `RangeError`) | `stack-exhausted` |
| epoch deadline (§17.8) | `time-limit` (proposed by answer 11) |
| GC heap growth refused (§17.8) | `heap-exhausted` (proposed by answer 11) |
| null dereference, failed cast, `unreachable` without a category | an internal error: a compiler bug, reported with the site and exit status 101 |

**Sections.**

| Section | Holds | Release |
| --- | --- | --- |
| `name` | function names: printed stable path plus type arguments, as `shop.cart/Cart.total` or `std.list/List.push[i32]` | no: the dev pipeline only (owner, 2026-10-07) |
| `hd.names` | per function: an index into a path table and a list of indices into a type-argument table; the symbolizer prints the same names as `name` | yes; about 30 KB instead of 150 KB at 10k lines |
| `hd.sites` | per site, keyed by function index and code offset: category or kind, and an anchor (item path index, TIR instruction index); in a file that `hd build` writes, the anchor is resolved to a file index, line and column, plus a file table of package-relative paths (codegen.md §13.8, "Positions") | yes |
| `hd.lines` | per function: sorted code offsets with an anchor each, for every statement that can call or trap; resolved to delta-encoded lines in a written file | yes |
| `hd.folds` | folded aliases per representative (§13.7) | yes |
| `hd.runtime` | §16.4 | yes |

Spans become line and column at link time, from the source's line table.
The link key includes the source hashes through the code keys, so a
stale line cannot survive.

**Backtraces.** wasmtime's `WasmBacktrace` gives each frame's function
index and module offset. V8's `Error.stack` gives
`wasm-function[i]:0xOFF` frames. `hd_run` maps both through `hd.lines`,
`hd.sites` and `hd.names` (or `name` in the dev pipeline) to `file:line function`,
and prints the panic's own site first: for an explicit panic, the frame
that called the stub.
Release builds keep these sections, so release backtraces are symbolized,
as the first-release feature list asks. They omit the standard `name`
section (owner, 2026-10-07), so external tools such as browser devtools,
`wasm-objdump` and native profilers show unnamed functions for a release
module; a dev build keeps `name` for them.

**Mapping must survive every Wasm-level pass (owner, 2026-10-07).**
`hd.sites`, `hd.lines` and engine-trap mapping key on code offsets. So
any pass that rewrites the code section after emission must carry the
offsets through. LEB compaction already does, with its offset map
(codegen.md §13.10, step 8). A Binaryen pass, if spike T2 adopts one,
must round-trip them through a source map (`--input-source-map`,
`--output-source-map`), or it does not run. The optimized pipeline's
inliner records each inlined call site in `hd.lines`, a few bytes per
site, so backtraces keep the inlined frames ([tiering.md §4](tiering.md#4-divergence-between-tiers)).

**Browser devtools (mine).** The program worker builds a source map from
`hd.lines` when the page asks for it, and serves it through a
`sourceMappingURL` that names a blob URL. The binary carries no source
map, so it costs nothing in `hd build` output.

### 15.6 The 2 KB Tiny Program

There is no runtime blob. Everything in a module is reachable code
generated for that program, std included (§16.1). The tiny program
(`println("hello")`) needs:

| Part | Estimate |
| --- | --- |
| header, type section (`$bytes`, the import and export signatures) | 70 B |
| imports: `hd:Console` `write_line`, `hd:rt` stderr writer | 50 B |
| function, memory (the exchange buffer), global, export sections | 80 B |
| code: the entry wrapper, the copy loop to the exchange buffer, std's `println` and its panic on a closed pipe | 300 B |
| data: `hello`, copied to the exchange buffer by `memory.init` (the host fast path of §15.4: no pool, no string array) | 10 B |
| `hd.names`, `hd.sites`, `hd.lines`, `hd.runtime` | 300 B |
| total | about 800 B (an estimate, not an accounting) |

**M4b measurement (M4b gap 8).** The current hello-world module is about
4.8 KB with its standard `name` section, so it misses the 2 KB target.
Reachability currently brings std number formatting and helpers along
with `println`; link also emits dev names rather than the compact release
metadata above. The estimate table is not an accounting. The queued size
breakdown must report each section and reachable function before any
design change or tuning decision.

What keeps it small:

- reachability and folding; no unused type; no literal pool when every
  literal goes straight to the host;
- compact LEBs in release (§13.10);
- the string copy loop is one shared helper per program;
- short export names (`hd.init`, `hd.poll`, `hd.wake`, `hd.x`).

**Start to first output (≤ 5 ms).** A warm `hd FILE.wasm` deserializes
the cached precompiled module (§18.2), instantiates it from an
`InstancePre`, and runs `hd.init` and `hd.poll`. Process start of `hd`
dominates: the `startup` target is 20 ms for `hd --version`, so a cold
process may miss 5 ms. How the metric counts it is inconsistency 7
(§23.2).

### 15.7 Emission

- **`wasm-encoder`** builds each body. A wrapper around its instruction
  sink records a relocation wherever it writes a function, type, global,
  or data index, and writes that index as a 5-byte padded LEB. There is
  no site index (§15.5).
- **Link** assembles the module with `wasm_encoder::Module`, writes code
  bodies with `CodeSection::raw` after patching, and appends the custom
  sections.
- **Validation.** `wasmparser` validates every linked module in the
  compiler's debug builds, in CI, and in verify mode. A release `hd`
  trusts its own output; the engine validates again anyway.
- **WAT** for the playground's view and `hd build --wat` (Later) comes
  from `wasmprinter`.

### 15.8 Deterministic Bytes

Reproducible builds are parked as a metric, but the cache needs
deterministic bytes anyway: equal keys must mean equal entries (verify
mode, §5.6). Every order in §13.10 and §15.3 is content-based: instance
keys, canonical descriptors, content keys of constant globals, and
literal pool slots by literal content. No hash map's iteration order
reaches the output. Locals in a body are numbered in TIR order; closures
and temporaries per body (§6.5). Wasm bytes join the determinism matrix
(§21.1).

Deterministic is not the same as stable. A content order still shifts
when an item is inserted before others, and a body that holds a shifted
index misses wasmtime's per-function cache. So bodies hold no dense
number except a call target: lazily initialized globals sit behind
getters, panic sites are code offsets, and key ids and type ids are
content hashes (§15.4, §15.5,
[lowering-catalog.md](lowering-catalog.md#numbering)). Numbers kept from
earlier builds would make the bytes depend on history, so they are not
used.
