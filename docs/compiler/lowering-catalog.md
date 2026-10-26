# New Compiler: Lowering Catalog

Status: implementation index for the linear-memory backend. The code and
specification win when this index lags them.

Part of the [compiler design](ARCHITECTURE.md). The compiler emits scalar
Wasm over linear memory with reference counting. Node, browsers and
Wasmtime execute that representation. Layouts, ownership and runtime
algorithms are documented in
[Wasm Emission And Runtime](../../crates/hd_wasm_linear/DESIGN.md);
this catalog links those descriptions rather than maintaining a second copy.

## Conventions

WAT excerpts below are copied from linked
[stage baselines](../../crates/hd_wasm_linear/tests/baselines/).
They retain the baseline's local names and instruction order. An excerpt
is a function or instruction sequence, not necessarily a complete module.
Fixture imports stand in for runtime helpers or separately emitted callees;
they do not describe the imports of a complete executable.

A baseline's source file and view directives select the profile, functions
and instructions shown. These examples are observations, not a promise
that every profile emits identical instructions. Cost claims here are
structural: loads, stores, calls and allocations.

The layout rules are [§23.3](../../crates/hd_wasm_linear/DESIGN.md#233-values).
Structs and tuples flatten into scalar leaves; aggregate memory layouts
store those leaves inline. There is no four-value boxing bound. Wide
signatures use the
[shadow stack](../../crates/hd_wasm_linear/DESIGN.md#2329-wide-signatures-and-the-shadow-stack).

## Numbering

Linking assigns indices to symbolic functions, imports, table entries,
globals and static data. Indirect calls use function-table slots; vtables
are records in static linear memory. Literal reads use static addresses
and lengths, without getter functions.

See [§15.7–15.9](../../crates/hd_wasm_linear/DESIGN.md#157-emission)
for emission, deterministic ordering and line tables, and
[the link step](../../crates/hd_mono/DESIGN.md#1310-the-link-step)
for collection's code entries. Function and table indices are separate
index spaces; a table slot is not a direct-call index.

## Baseline Emission

The emitter reads concrete bodies and targets from collection and emits
scalar functions. Direct calls use `call`; unresolved function-value and
trait-value calls use `call_indirect`. Known closure targets can call directly.

[Inline aggregates](../../crates/hd_wasm_linear/DESIGN.md#2314-inline-aggregate-layouts)
do not allocate just because they are constructed. Captured environments,
shared cells, collection buffers and saved suspension frames are counted
objects when materialized. String literals are static bytes.

See [scalar emission](../../crates/hd_wasm_linear/DESIGN.md#2311-scalar-instruction-emission)
and [counted values](../../crates/hd_wasm_linear/DESIGN.md#2316-counted-values-and-cleanup).
Profile checks are semantics, independent of optimization level.

## Optimization Passes

The current architecture and pipeline declarations live in
[TIR v2](tir-v2.md),
[Optimization Pipelines](../../crates/hd_opt/DESIGN.md) and
[hd_opt's pass table](../../crates/hd_opt/src/passes.rs).

Collection supplies known call targets; emission consumes them. The
examples below deliberately name their baselines instead of assuming an
optimization always applies. Current measurements belong in
[the benchmarks](../../bench/README.md), not in a layout description.

## Data Types

### Scalars: Integers, Floats, `bool`

See [§23.3 Values](../../crates/hd_wasm_linear/DESIGN.md#233-values)
for scalar widths and packed memory fields, and
[§23.11](../../crates/hd_wasm_linear/DESIGN.md#2311-scalar-instruction-emission)
for checked arithmetic. Narrow field reads and writes use linear-memory
loads and stores, with the appropriate width and signedness.

### `char`

A Unicode scalar value uses `i32`. Packed storage and conversions are
covered by [Values](../../crates/hd_wasm_linear/DESIGN.md#233-values)
and [string primitives](../../crates/hd_wasm_linear/DESIGN.md#2315-string-emission-and-host-primitives).

### `void`, `()` And `never`

These have no Wasm values and occupy no memory. See
[Values](../../crates/hd_wasm_linear/DESIGN.md#233-values);
this does not remove a collection's length or capacity metadata.

### `string`

A string is an address/length pair of `i32` values. Slices share bytes;
empty strings use the static empty value. Ownership of dynamic buffers,
static literals and interior views is described in
[§23.7](../../crates/hd_wasm_linear/DESIGN.md#237-strings-and-collection-buffers)
and [§23.15](../../crates/hd_wasm_linear/DESIGN.md#2315-string-emission-and-host-primitives).

From [a-string-literal.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-string-literal.wat):

```wat
i32.const 65 ;; &"hello"
i32.const 5
```

The address is assigned by that fixture's static data layout, not fixed
for all programs. There is no allocation or literal getter at this read.

### Bytes: `List[u8]`

Bytes occupy inline elements in a counted list buffer, with capacity,
length and owner metadata. See
[§23.7](../../crates/hd_wasm_linear/DESIGN.md#237-strings-and-collection-buffers)
and [list operations](../../crates/hd_wasm_linear/DESIGN.md#2317-list-operations).
The [host codecs](../../crates/hd_wasm_linear/DESIGN.md#2323-host-value-codecs)
describe exchange-buffer copies.

### Tuples, Narrow And Wide

Tuple fields flatten into scalar leaves in locals and occupy aligned,
inline storage in memory. Wide tuples do not acquire a box at four
values. See [§23.14](../../crates/hd_wasm_linear/DESIGN.md#2314-inline-aggregate-layouts)
and [wide signatures](../../crates/hd_wasm_linear/DESIGN.md#2329-wide-signatures-and-the-shadow-stack).

### `data` Types, Mutable And Readonly Views

Programs declare value structs; the former user-declared `data` model is
retired. Struct fields flatten like tuple fields. Only compiler-supplied
data types retain counted identity. See
[§23.3](../../crates/hd_wasm_linear/DESIGN.md#233-values) and
[reference addresses and typed views](../../crates/hd_wasm_linear/DESIGN.md#2320-reference-addresses-and-typed-views).

### Embedded Parts

Part layouts and paths follow aggregate fields and reference ownership.
See [inline aggregates](../../crates/hd_wasm_linear/DESIGN.md#2314-inline-aggregate-layouts)
and [reference addresses](../../crates/hd_wasm_linear/DESIGN.md#2320-reference-addresses-and-typed-views).
There is no separate object allocation per part merely because it is embedded.

### Payloadless Enums

A payloadless enum needs its tag only. The tag width in memory and the
scalar representation are specified in
[§23.3](../../crates/hd_wasm_linear/DESIGN.md#233-values).

### Enums With Payloads Within The Bound

There is no bound-based split. An enum uses an `i32` tag and scalar
payload slots, shared by Wasm value type across variants. See
[§23.14](../../crates/hd_wasm_linear/DESIGN.md#2314-inline-aggregate-layouts)
for slot order, zeroed unused slots and aligned memory storage.

### Enums Over The Bound, And Self-Recursive Enums

A wide nonrecursive enum uses the same inline rules as a narrow enum.
Self-recursive enums instead use counted cells. See
[§23.3](../../crates/hd_wasm_linear/DESIGN.md#233-values)
and [counted objects](../../crates/hd_wasm_linear/DESIGN.md#234-counted-objects).

### `Option`

An optional reference uses address 0 for absence. Other optional values
use an enum tag and payload slots. See
[§23.3](../../crates/hd_wasm_linear/DESIGN.md#233-values).
From [option-unwrap-or.wat](../../crates/hd_wasm_linear/tests/baselines/calls/option-unwrap-or.wat),
a scalar option passes its tag, payload and fallback to the selected method:

```wat
local.get $found.0
local.get $found.1
i64.const 0
call $"impl [ T ] T ?.unwrap_or[i64]"
```

### `Result`

`Result` follows the ordinary enum layout: tag and shared payload slots,
or a counted cell for a recursive enum. See
[§23.14](../../crates/hd_wasm_linear/DESIGN.md#2314-inline-aggregate-layouts).

### `dyn Trait` And `dyn Any`

A trait value is `(data, vtable)`, both `i32` addresses. The data either
already has counted identity or is boxed; the vtable is static data
containing function-table slots and supertrait vtable addresses. See
[§23.8](../../crates/hd_wasm_linear/DESIGN.md#238-closures-dyn-and-vtables)
and [§23.19](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values)
for allocation, dispatch, widening and downcasts.

### Generic Methods Through `dyn`: Witnesses And Thunks

Erased methods use witnesses and instruction thunks over the linear-memory
ABI. See [§23.27](../../crates/hd_wasm_linear/DESIGN.md#2327-erased-generic-methods)
and [the erased method ABI](../../crates/hd_mono/DESIGN.md#1351-the-erased-method-abi).

### Closures And Mutably Captured Variables

A closure is `(code, environment)`, two `i32` values. A capture-free
closure has environment 0; captured values and shared cells use counted
storage. See [§23.19](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values)
for mutable captures and immutable self-recursion.

### `List[T]`

A list is one counted buffer address. Elements occupy inline storage at
their layout's stride, including wide aggregate elements. Copy-on-write,
pinning and growth are described in
[§23.7](../../crates/hd_wasm_linear/DESIGN.md#237-strings-and-collection-buffers)
and [§23.17](../../crates/hd_wasm_linear/DESIGN.md#2317-list-operations).
There are no parallel per-field arrays or element-layout boxing threshold.

### `Map[K, V]`

A map is one counted table address, with entries and a probing index in
linear memory. See
[§23.18](../../crates/hd_wasm_linear/DESIGN.md#2318-map-operations)
for hashing, updates, tombstones, iteration and copy-on-write.

### `Set[T]`, `Deque[T]` And `Heap[T]`

These are ordinary std structs using the same aggregate and collection
layouts. Their current implementations are in
[collections.hd](../../lib/std/collections.hd); they have no independent
backend representation.

### Iterators And Ranges

Iterators are ordinary trait implementations, not a compiler-defined
closure/iterator object per adapter. See
[iter.hd](../../lib/std/iter.hd),
[§23.19](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values)
and [Loops](#loops). Range values use ordinary aggregate layouts;
counted range loops need not construct one.

### Newtypes

A newtype uses its underlying layout, with no wrapper allocation.
Nominal identity remains distinct. See the layout walk in
[layout.rs](../../crates/hd_wasm_linear/src/layout.rs)
and the [collection rules](../../crates/hd_mono/DESIGN.md#132-collection).

### `TypeId`

`TypeId` contains two static strings: an identity key and a printable
name. Producing it allocates nothing. See
[§23.19 Inspection](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values),
[dyn_value.rs](../../crates/hd_wasm_linear/src/emit/dyn_value.rs) and
[inspect.hd](../../lib/std/inspect.hd).

### Host Handles

The host boundary encodes handles as scalar IDs; providers validate their
lifetime. See [host value codecs](../../crates/hd_wasm_linear/DESIGN.md#2323-host-value-codecs)
and [the host ABI](../../crates/hd_run_wasmtime/DESIGN.md#17-host-interface-and-embedding).
There is no universal slot/generation bit split for every provider.

### Suspension Frames

A suspension frame is a counted linear-memory address holding saved
state, live values and child state. See
[§14](../../crates/hd_wasm_linear/DESIGN.md#14-suspension-lowering)
and [§23.21](../../crates/hd_wasm_linear/DESIGN.md#2321-suspension-frames-and-wakers)
for frame materialization, polling, wakes and cancellation.

### Facts

Fact getters use a once flag and globals for their result's scalar leaves.
See [facts, defaults, derives and tests](../../crates/hd_mono/DESIGN.md#123-facts-defaults-derives-and-tests),
[derived facts](../../crates/hd_mono/DESIGN.md#1313-omitted-arguments-default-bodies-and-derived-facts)
and [shape.rs](../../crates/hd_wasm_linear/src/emit/shape.rs).

### Literals

Integers and floats are immediates. String bytes live in static data;
a use pushes an address and length, as shown under [string](#string).
Neither a lazy literal table nor a getter function is involved. See
[§23.15](../../crates/hd_wasm_linear/DESIGN.md#2315-string-emission-and-host-primitives).
List and map constructions use the ordinary collection operations instead.

## Expressions And Statements

The sections below link current lowering rules and show selected fixture
excerpts. Ownership decisions and load/store simplification happen in TIR;
see [TIR v2](tir-v2.md). An emitter example alone does not establish a
whole-program allocation count or engine speedup.

### Arithmetic And Overflow Checks

Checks follow the build profile. From
[a-top-level-binding.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-top-level-binding.wat),
a checked `i32` addition widens and tests the result before narrowing:

```wat
global.get $calls
i64.extend_i32_s
i32.const 1
i64.extend_i32_s
i64.add
local.tee $t0
i64.const 2147483647
i64.gt_s
local.get $t0
i64.const -2147483648
i64.lt_s
i32.or
if
  call $rt:panic
  unreachable
end
local.get $t0
i32.wrap_i64
global.set $calls
```

See [integer widths](../../crates/hd_mono/DESIGN.md#1315-integer-widths)
and [checked builds](../../crates/hd_wasm_linear/DESIGN.md#2324-checked-builds).

### Division, Remainder And Shifts

Signedness, width and profile determine the scalar instruction and guards.
See [integer widths](../../crates/hd_mono/DESIGN.md#1315-integer-widths)
and [checked builds](../../crates/hd_wasm_linear/DESIGN.md#2324-checked-builds).
The linked rules cover zero divisors, signed overflow and shift counts.

### Comparison, Equality And Logical Operators

Scalar comparisons use scalar instructions. Strings use the string
kernels; user-defined equality calls the selected implementation.
Short-circuit control uses structured blocks. See
[scalar emission](../../crates/hd_wasm_linear/DESIGN.md#2311-scalar-instruction-emission)
and [string emission](../../crates/hd_wasm_linear/DESIGN.md#2315-string-emission-and-host-primitives).

### Indexing And Slicing

From [a-field-read-through-a-list-element.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-field-read-through-a-list-element.wat),
the field address is buffer + index × stride + elements offset:

```wat
local.get $t1
local.get $t2
i32.const 16
i32.mul
i32.add
f64.load $heap offset=16
```

The complete baseline includes the bounds check before this sequence.
See [list operations](../../crates/hd_wasm_linear/DESIGN.md#2317-list-operations),
[string operations](../../crates/hd_wasm_linear/DESIGN.md#2315-string-emission-and-host-primitives)
and [range bounds](../../crates/hd_wasm_linear/DESIGN.md#2337-range-bounds-and-exact-list-indices)
for slices and proved indices.

### Path Mutation And Compound Assignment

A field write evaluates the place and stores its scalar leaves. From
[a-field-write-through-a-list-element.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-field-write-through-a-list-element.wat):

```wat
local.get $t2
local.get $t3
i32.const 16
i32.mul
i32.add
local.get $t1
f64.store $heap offset=16
```

The full baseline checks the index before this store. Place ownership,
copy-on-write and loans are described in
[TIR v2](tir-v2.md) and
[element field stores](../../crates/hd_wasm_linear/DESIGN.md#2332-element-field-stores-and-constant-branches).
A local struct's fields can remain locals; a store into linear memory is
not necessary for every assignment.

### String Interpolation And Concatenation

Interpolation writes through ordinary `Display.write` and `StringBuilder`
calls. Concatenation still operates on address/length views, as
[constant-plus-constant.wat](../../crates/hd_wasm_linear/tests/baselines/arithmetic/constant-plus-constant.wat) shows:

```wat
;; "id=$id name=$name": std's StringBuilder, written by each value's Display.write
(call $"impl StringBuilder.new")                ;; one builder for the whole string
(call $"impl StringBuilder.push" (local.get $sb) (i32.const &"id=") (i32.const 3))
(call $"impl Display for i32.write" (local.get $id) (local.get $sb))
(call $"impl StringBuilder.push" (local.get $sb) (i32.const &" name=") (i32.const 6))
(call $"impl Display for string.write" (local.get $name) (local.get $sb))
(call $"impl StringBuilder.build" (local.get $sb))  ;; takes the buffer without copying
;; sb.push("item-$i;"), every value a name or a field path: written to sb itself
;; a + b on strings: one allocation of the summed length, two copies
```

- **Speed:** one byte-buffer builder; each value's `write` appends its
  bytes and `build` takes the buffer without copying.
- **Size:** a call per text piece and per value.
- **Engines:** no difference.
- **Boundary:** `println("...")` of a literal alone takes the host fast
  path (no array built).
- **Type-only:** yes. The callee per part is the impl `select` picks.

### Pipes, Binding Expressions And Ranges

Pipes and binding expressions lower through ordinary evaluation and locals.
Ranges are aggregate values unless consumed by counted-loop lowering.
See [lowering rules](../../crates/hd_mono/DESIGN.md#122-lowering-rules)
and [Loops](#loops); there is no separate runtime pipe object.

### Propagation `?`

Propagation tests an option or result, converts an error when required,
and takes the appropriate exit through cleanup. See
[scope.rs](../../crates/hd_wasm_linear/src/emit/scope.rs)
and [scalar emission](../../crates/hd_wasm_linear/DESIGN.md#2311-scalar-instruction-emission).
It uses scalar tags or addresses, not reference-typed branches.

### `if` And `match` As Values

Structured blocks and decision-tree branches select an arm; the result's
scalar leaves become the expression's value. See
[§23.11](../../crates/hd_wasm_linear/DESIGN.md#2311-scalar-instruction-emission).
There is no payload-object cast after matching an inline enum.

### Loops

From [range-for.wat](../../crates/hd_wasm_linear/tests/baselines/loops/range-for.wat),
the range loop tests the unsigned index, binds it, and advances before
the body:

```wat
local.get $t0
local.get $t1
i32.lt_u
i32.eqz
br_if 1
local.get $t0
local.set $i
local.get $t0
i32.const 1
i32.add
local.set $t0
```

See [counted loops](../../crates/hd_mono/DESIGN.md#125-counted-loops-and-checks),
[collection cursors](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values)
and [range proofs](../../crates/hd_wasm_linear/DESIGN.md#2337-range-bounds-and-exact-list-indices).
Snapshots retain collection buffers; element references use addresses and
pins, rather than allocating an object and closures for every element.

### `break` With A Value, `continue` And Loop `else`

These use structured branches and result locals, with scope cleanup on
exits that need it. See
[scope.rs](../../crates/hd_wasm_linear/src/emit/scope.rs)
and [lowering rules](../../crates/hd_mono/DESIGN.md#122-lowering-rules).

### Comprehensions

Comprehensions lower to loops and collection operations. See
[lowering rules](../../crates/hd_mono/DESIGN.md#122-lowering-rules),
[list operations](../../crates/hd_wasm_linear/DESIGN.md#2317-list-operations)
and [map operations](../../crates/hd_wasm_linear/DESIGN.md#2318-map-operations).
Their elements use inline layouts, not per-field arrays.

### Method Calls: Static, Bound, `dyn`

Static and resolved trait calls target concrete instances. From
[a-call-of-a-small-function.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-call-of-a-small-function.wat):

```wat
local.get $a
local.get $a
call $add
```

Dynamic dispatch loads a method slot from the static vtable and uses
`call_indirect`. See
[trait values](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values).

### Closure Calls

From [a-closure-call.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-closure-call.wat),
the call passes the environment, argument and empty context, then the
code's function-table slot:

```wat
local.get $f.1
local.get $x
i32.const 0
local.get $f.0
call_indirect $table0 (param i32 i32 i32) (result i32)
```

See [§23.19](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values)
for direct known-target calls, adapters and environment cleanup.

### Providers: `$.with`, `$.use` And Rows

Concrete providers pass as data/vtable pairs. A function-value call also
passes one context address. From
[a-function-value-call-builds-a-context.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-function-value-call-builds-a-context.wat):

```wat
local.get $t0
local.get $send.0
call_indirect $table0 (param i32 i32 i32 i32)
local.get $t0
call $rt:release_ref'1
```

The earlier baseline instructions build the context and store its key
and provider. The caller releases that context after the call. See
[§23.26](../../crates/hd_wasm_linear/DESIGN.md#2326-requirement-rows-and-context-values)
for open rows, forwarding, lookup and provider ownership.

### Bang Calls, `all!`, `race!` And `block_on`

Suspending calls use ready/Pending state and counted saved frames.
See [§14](../../crates/hd_wasm_linear/DESIGN.md#14-suspension-lowering)
and [§23.21](../../crates/hd_wasm_linear/DESIGN.md#2321-suspension-frames-and-wakers)
for child polling, cancellation and entry drivers.

### `defer`

Deferred suites run through scope cleanup on the exits that require them;
cancellation uses frame cleanup. See
[scope.rs](../../crates/hd_wasm_linear/src/emit/scope.rs) and
[cancellation and defer](../../crates/hd_wasm_linear/DESIGN.md#146-cancellation-and-defer).

### Let Patterns And Let-Else

Pattern tests and extraction use scalar values and structured branches;
a diverging else takes the scope's exit. See
[scope.rs](../../crates/hd_wasm_linear/src/emit/scope.rs)
and [aggregate values](../../crates/hd_wasm_linear/DESIGN.md#2314-inline-aggregate-layouts).
Tuple destructuring does not itself allocate a wrapper.

### `is`

Identity compares counted addresses: an object's address, a collection's
buffer, a trait value's data or a reference's place. See
[§23.19 Identity](../../crates/hd_wasm_linear/DESIGN.md#2319-closure-and-trait-values)
and [§23.20](../../crates/hd_wasm_linear/DESIGN.md#2320-reference-addresses-and-typed-views).
This is address comparison, not structural equality.

### Default Arguments

An omitted argument calls its default body with the earlier arguments.
A scalar constant default can instead become an immediate, without a
default-body instance. See
[§13.13](../../crates/hd_mono/DESIGN.md#1313-omitted-arguments-default-bodies-and-derived-facts)
and `default_call` in [emit.rs](../../crates/hd_wasm_linear/src/emit.rs).

### Module Initialization Across Folders

`hd.init` initializes reachable module storage in dependency order; each
binding has globals for its scalar leaves. See
[§15.4](../../crates/hd_wasm_linear/DESIGN.md#154-globals-and-module-initialization)
and [§23.9](../../crates/hd_wasm_linear/DESIGN.md#239-globals-the-entry-wrapper-and-the-host).

### Tests

Tests have numbered initialization and test exports and use the entry
polling protocol. See
[§23.12](../../crates/hd_wasm_linear/DESIGN.md#2312-memory-intrinsics-and-runtime-tests)
and [the test runner](engines-and-test-runner.md).

### Derives

Derived methods are ordinary collected instances, emitted with the same
layouts and call rules as handwritten code. See
[facts, defaults, derives and tests](../../crates/hd_mono/DESIGN.md#123-facts-defaults-derives-and-tests)
and [error derivation](../../crates/hd_mono/DESIGN.md#1314-error-derivation).

### Explicit Panics And Unreachable Code

A panic calls its report helper and is followed by `unreachable`.
From [a-field-read-through-a-list-element.wat](../../crates/hd_wasm_linear/tests/baselines/emitter/a-field-read-through-a-list-element.wat),
the failed unsigned bounds check takes this path:

```wat
i32.ge_u
if
  call $rt:panic
  unreachable
end
```

See [§15.5](../../crates/hd_wasm_linear/DESIGN.md#155-panic-sites-and-backtraces)
and [§23.9](../../crates/hd_wasm_linear/DESIGN.md#239-globals-the-entry-wrapper-and-the-host)
for reporting and traps. The host records the report and uses the module's
line metadata to locate the source site.
