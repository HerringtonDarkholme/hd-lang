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
| `ref` | `(ref $T)` or `(ref null $T)` | data, `string`, lists, maps, closures, frames, and an enum's box (over 4 Wasm values, or a self-recursive payload) |
| `multi` | 2 to 4 Wasm values | `Option` of a scalar, `Result`, tuples, trait values (§15.2) |
| `erased` | `anyref` | the payload of a trait value, `Any`, an open value in an erased method body (codegen.md §13.5.1) |
| `void` | none | `void`, `()`, and `never` |

In locals, parameters and results, a `multi` layout over 4 Wasm values
is passed as one immutable struct instead (mine); Wasm's multi-value
results make 4 a cheap bound on both engines. In fields and arrays a
value layout stays unboxed, as the spec's shape rules ask
([Shapes and Generic Code](../../spec/lang/04-type-system.md#shapes-and-generic-code)):
it becomes several fields, or several arrays.

### 15.2 Values

| hd value | In locals and results | In a field or array element |
| --- | --- | --- |
| integers, `bool`, `char`, floats | the scalar | packed: `i8` for `bool`, `u8`, `i8`; `i16` for 16-bit integers; else the scalar |
| `data T` | `(ref $T)`: a struct with one mutable field per declared field, in declaration order | same |
| embedded part | a separate struct, referenced by an immutable field of the outer struct ([`data.part.unobservable`](../../spec/lang/08-data-and-enums.md#r-data.part.unobservable)) | same |
| payloadless enum | `i32` tag | `i8` or `i16` when the variant count fits |
| enum with payloads, **flat** | one struct: tag plus the union of payload fields, unused fields null or zero | same |
| enum with payloads, **subtypes** | a non-final base struct (tag, shared fields) and one final subtype per payload variant; payloadless variants are constant singletons | same |
| `T?`, `T` a non-nullable reference | `(ref null $T)`; null is `.None` | same |
| `T?`, `T` a scalar | `(i32 tag, T)` | two fields |
| `T??` and `Option` of a `multi` | a tag plus the inner layout | the same fields |
| `Result[T, E]` | `multi`: `(i32 tag, dflt(T'), dflt(E'))`, where `T'` and `E'` are the payload layouts (see "Identity" and "Defaultable forms" below) | the same fields |
| tuple | its elements' values | its elements' fields, flattened |
| trait value, `Any` | `(anyref, (ref $VT))` | two fields |
| closure | `(ref $Fn_sig)`: a base struct holding the code as a typed function reference; one subtype per capture shape | same |
| capture-free closure | a constant global of the base type, with no environment | same |
| `string` | `(ref $str)`, `$str = (array i8)`, immutable, valid UTF-8 | same |
| `List[T]` | `(ref $List_T)` = struct `{len: mut i32, data: mut (ref $Arr_T)}` | same |
| `Map[K, V]` | std hd over arrays (§16.1) | same |
| `mut Suspend[T]` | `(ref $Suspend_L)` (§14.1) | same |

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

Language-level locals, parameters and fields keep the non-null form, so a
value that is always present costs nothing. A narrowing read is one
`ref.as_non_null`, which cannot fail after the test. `struct.new_default`
is valid for every frame type.

**Enum layout per enum (mine).** Every enum is an identity-free value
(S1c) and normally uses the value layout of "One predicate for
identity-free enums" below. The flat and subtype rows above are the
shapes of its **box**, used past 4 Wasm values and for a self-recursive
payload. The rule is deterministic, with no annotation: a box is
**flat** when the enum's payload fields number at most 4 in total.
Otherwise it uses **subtypes**. Flat enums need no
cast on a match, and their payloadless variants need no allocation.
Subtype enums cast once per matching arm, which the engine checks with
one load and compare.

**Erased scalars.** In an erased position, `bool`, `char` and integers of
16 bits or less become `i31ref`. Wider scalars are boxed in
`$Box_i32`, `$Box_i64`, `$Box_f32` or `$Box_f64`. This is where the owner's
"at least `i31ref`" lands: in monomorphized code no scalar is boxed at
all. A string is already a reference and is erased as itself. A tuple is
boxed in one immutable struct.

**Identity (owner decision B, extended, 2026-10-07).** The Codex review (finding 1)
showed that the rows above broke the spec's allocation identity: the spec
gives each `.Some(...)` and each primitive-to-`Any` box its own identity
(`expr.is.some`, `expr.is.box`, `expr.is.box.distinct`). The owner chose
to change the language instead of the layouts. `.Some`, `.Ok`, `.Err`
and primitive, string or tuple boxes have no identity. `is` is a compile
error on an operand whose static type is a value type and on function
values; since S1c that covers every enum, optionals and `Result`
included. On an `Any` or trait value that holds a value at run time,
the result is unspecified. The spec pass applies
it; the rule list is in
[codex-review-response.md](codex-review-response.md#spec-changes-for-the-spec-pass).
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
   inline. When `T'` and `E'` are both references, the fields may share
   one `anyref`-typed slot cast by tag; the first release keeps them
   apart, which is simpler and costs one word.
4. **Other enums** are identity-free too (S1c). They follow the
   predicate below; the flat and subtype shapes are only their boxes.

**One predicate for identity-free enums (mine).** Layout, `is` lowering
and the checker's value-type test read one predicate, `identity_free(E)`.
Since the S1c spec pass it holds for every enum
([`types.sealed.anyval-values`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-values)).
An identity-free enum
uses a value layout: a nullable reference when it has one reference
payload and one payloadless variant (`T?`), else `(i32 tag, payload
fields...)` as `multi`, boxed in one immutable struct past 4 Wasm values
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

### 15.4 Globals And Module Initialization

| Global | Wasm | Initialized by |
| --- | --- | --- |
| module storage (top-level bindings) | mutable, nullable or zero | the group's init function |
| vtables, capture-free closures, payloadless variant singletons, member handles | immutable | a constant expression (`struct.new` and `ref.func` are constant in Wasm GC) |
| short string literals (16 bytes or less) | immutable | a constant expression (`array.new_fixed`) |
| other string literals | mutable, nullable | lazily: the first use runs `array.new_data` |
| fact values, metadata, shared enum data | immutable | one global per allocation of the value graph, in allocation order, each a constant expression over earlier ones (codegen.md §12.3); no fact has a getter |
| runtime state: panic category and site, the wake table, the forbidden-context counter | mutable | constants |

- **Init order.** The `hd.init` export calls each reachable group's init
  function once, in D1's order (`InitOrder` and M3), after every group it
  uses ([`module.init.group.once`](../../spec/lang/10-modules.md#r-module.init.group.once)).
- **No Wasm `start` function** (mine). Init can panic and can call
  imports. Calling it as an export after instantiation lets the host
  refuse a start, attribute a trap to init, and run a test case's init
  inside the case's time limit.
- Initialization is synchronous: module top level is not a driver
  context.

### 15.5 Panic Sites And Backtraces

**Explicit panics.** A panic stub writes the category id and the global
site number into globals, copies the message into the exchange buffer,
sets its length, and executes `unreachable`. The host reads the globals
and the buffer after the trap, so it never calls into a poisoned instance
([`flow.panic.poison`](../../spec/lang/06-control-flow.md#r-flow.panic.poison)).

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
| `name` | function names: printed stable path plus type arguments, as `shop.cart/Cart.total` or `std.list/List.push[i32]` | yes, always |
| `hd.sites` | per site: category or kind, file index, line, column; plus a file table of package-relative paths | yes |
| `hd.lines` | per function: sorted code offsets with a delta-encoded line, for every statement that can call or trap | yes |
| `hd.folds` | folded aliases per representative (§13.7) | yes |
| `hd.runtime` | §16.4 | yes |

Spans become line and column at link time, from the source's line table.
The link key includes the source hashes through the code keys, so a
stale line cannot survive.

**Backtraces.** wasmtime's `WasmBacktrace` gives each frame's function
index and module offset. V8's `Error.stack` gives
`wasm-function[i]:0xOFF` frames. `hd_run` maps both through `hd.lines` and
`name` to `file:line function`, and prints the panic's own site first.
Release builds keep these sections, so release backtraces are symbolized,
as the first-release feature list asks.

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
| header, type section (`$str`, the import and export signatures) | 70 B |
| imports: `hd:Console` `write_line`, `hd:rt` stderr writer | 50 B |
| function, memory (the exchange buffer), global, export sections | 80 B |
| code: the entry wrapper, the copy loop to the exchange buffer, std's `println` and its panic on a closed pipe | 300 B |
| data: `hello` | 10 B |
| `name`, `hd.sites`, `hd.lines`, `hd.runtime` | 300 B |
| total | about 800 B (an estimate, not an accounting) |

**Status: not established.** The table guesses part sizes; it is not a
byte count of a real module. It may miss the path from `println` to the
`Console` provider, a `block_on` or suspension helper that path keeps,
vtable and provider types, the failure path, and the exchange-memory
exports. Slice 6 starts with a spike that emits the real hello-world
closure, release metadata included. It reports bytes per section and,
separately, instantiation to first output. The 2 KB target stands until
that spike measures it.

What keeps it small:

- reachability and folding; no unused type; no lazy-literal code for short
  literals;
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
  data or site index, and writes that index as a 5-byte padded LEB.
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
keys, canonical descriptors, first reference in function order. No hash
map's iteration order reaches the output. Locals in a body are numbered
in TIR order; sites, closures and temporaries per body (§6.5). Wasm bytes
join the determinism matrix (§21.1).
