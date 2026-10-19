# New Compiler Design: Wasm GC Layout And Emission

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

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
| `erased` | `eqref` | the payload of a trait value, `Any`, an open value in an erased method body (crates/hd_mono/DESIGN.md §13.5.1) |
| `void` | none | `void`, `()`, and `never` |

**One representation per type in every position.** A type has the same Wasm values in a local, a parameter,
a result, a field and an array element, apart from packing scalars (`i8`
for `bool`). A value layout within the **bound** is several values,
several fields, or a structure of arrays. A value layout over the bound
is one immutable box struct in every position, fields and arrays
included; boxing only in locals would allocate a box on every read of
such a field or element. The spec allows either form
([Shapes and Generic Code](../../spec/lang/04-type-system.md#shapes-and-generic-code)).

- **The bound** counts Wasm values after slot sharing, the tag included,
  and counts a boxed payload as one value. It is 4 (`layout::BOUND`):
  the largest payload count where parallel arrays won a read-only kernel
  by more than 10 percent on wasmtime and `push` stayed under 256 bytes.
  There is one bound, because crossing it changes the
  layout of every value type that holds the type by value.
- **Slot sharing.** Payload fields of different variants with the same
  Wasm value type share a slot. Reference fields share reference slots:
  the slot is typed exactly when every variant's field in it has one
  reference type, and `eqref` with a `ref.cast` after the tag test
  otherwise.
- **Erased storage (A1).** A struct field or array element whose declared
  type is a type parameter, instantiated at a layout of one reference
  (nullable or not), is stored as `eqref`, and readers cast at the first
  use that needs the exact type. So `List[Point]` is always `{len,
  (array eqref)}`. Scalars and `multi` layouts keep exact storage. Collection then gives
  move-only generic bodies one instance per class (crates/hd_mono/DESIGN.md §13.2).
- Every rule here reads only the type's definition and the definitions
  its fields name, never a use or the program, so a cached instance is
  valid in every program that reaches it. Each data type and syntax
  form's cost is in [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md).

### 15.2 Values

| hd value | In locals and results | In a field or array element |
| --- | --- | --- |
| integers, `bool`, `char`, floats | the scalar | packed: `i8` for `bool`, `u8`, `i8`; `i16` for 16-bit integers; else the scalar |
| `data T` | `(ref $T)`: a struct with one mutable field per declared field, in declaration order | same |
| `struct T` | `(ref $T)`: the same struct as a data type's, which nothing writes after `struct.new` ("Struct values" below) | same |
| `&T`, `&mut T` | `(ref $Ref_T)`: the data type `std.core.Ref[T]`, two closures that read and replace the place, and the place's identity ("References" below) | same |
| embedded part | a separate struct, referenced by an immutable field of the outer struct ([`data.part.unobservable`](../../spec/lang/08-data-and-enums.md#r-data.part.unobservable)) | same |
| payloadless enum | `i32` tag | `i8` or `i16` when the variant count fits |
| enum with payloads, within the bound | `multi`: `(i32 tag, shared payload slots...)` (§15.1) | the same fields; a structure of arrays in an array |
| enum box, **flat** (over the bound, at most 4 payload fields in total) | one immutable struct: tag plus the union of payload fields, unused fields null or zero | same: the box reference |
| enum box, **subtypes** (over the bound with more fields, or self-recursive) | a non-final base struct (tag, shared fields) and one final subtype per payload variant; payloadless variants are constant singletons | same: the box reference |
| `T?`, `T` a non-nullable reference (data, list, string, closure, a box) | `(ref null $T)`; null is `.None` | same |
| `T?`, `T` a trait value | `(eqref, (ref null $VT))`; a null vtable is `.None` | two fields |
| `T?`, `T` a scalar | `(i32 tag, T)` | two fields |
| `T??` and `Option` of a `multi` | a tag plus the inner layout, boxed over the bound | the same fields |
| `Result[T, E]` | `multi`: `(i32 tag, dflt(T')..., dflt(E')...)` with slot sharing, where `T'` and `E'` are the payload layouts (see "Identity" and "Defaultable forms" below) | the same fields |
| tuple | its elements' values within the bound; one immutable box over it | its elements' fields, flattened, or the box |
| trait value, `Any` | `(eqref, (ref $VT))`; `eqref`, so `is` can use `ref.eq` on the payload | two fields |
| closure | per the target's lowering profile (crates/hd_opt/DESIGN.md (optimization pipelines) §4). `V8`, `Browser`: `(ref $Fn_sig)`, a base struct holding the code as a typed function reference. `Wasmtime`: `(ref $Fn)`, one base struct for every signature, holding the code as an abstract `funcref`; a call casts it to the final `$Code_sig` before `call_ref` (wasmtime reads an abstract funcref field without a subtype check; crates/hd_opt/DESIGN.md (known closure calls) §7). One subtype per capture shape | same |
| capture-free closure | a constant global of the base type, with no environment | same |
| `string` | a view of two values, `(ref $bytes, i64 span)`: `$bytes = (array i8)`, immutable, valid UTF-8 in the viewed range; `span` packs `len << 32 \| start` ("Strings" below) | two fields, `(mut (ref $bytes))` and `(mut i64)`; in an array, a structure of two arrays |
| `string?` | `(ref null $bytes, i64)`; a null `bytes` is `.None` | two fields |
| `List[T]` | `(ref $List_T)` = struct `{len: mut i32, data: mut (ref $Arr_T)}`; under A1 every reference `T` shares `$List_eq` over `(array (mut eqref))` | same |
| `Map[K, V]` | a compact insertion-ordered table of `index`, `hashes`, `keys` and `values` arrays (`layout::map_ty`, [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#mapk-v)). Its operations are runtime helpers (`map.rs`); std hd over arrays is not built yet; see #180 | same |
| `TypeId` | `(ref $TypeId)`, a constant global per type: `{id: i64, name: (ref $LitFn)}`, `id` the first 64 bits of `H(canon(T))`. Implementation differs: `TypeId` is std's `data TypeId {key, name}`, built with `struct.new` at each use and compared by its `key` bytes; that allocates per use and compares strings, not one `i64` (see #175) | same |
| `mut Suspend[T]` | `(ref $Suspend_L)` (§14.1) | same |

**Struct values: immutable objects, rebuilt on a field write.** A
`struct` value has no identity and is copied on every binding, store and
call ([Value Types](../../spec/lang/04-type-system.md#value-types)). On
Wasm GC it is emulated, not laid out inline: a struct value is the GC
object a data type of the same fields would be, and the checker never
emits a store into one. A field write to a writable place,
`a.b.x = v`, is lowered in TIR (`crates/hd_check/src/value.rs`) as
`a = A { ...a, b: B { ...a.b, x: v } }`: one `CopyData` per struct on the
path, then a `LocalSet`, `GlobalSet` or `FieldSet` of the place's root.

- **Why not a copy on every copy.** Copying at each read needs the
  concrete type at every read, which an erased or `A1` generic body does
  not have (`List[Point]` stores `eqref`), and a missed copy is a silent
  aliasing bug. An object that never changes can be shared by any number
  of places, so a copy is a reference move and nothing can be missed.
- **Why not the tuple value layout.** Flattening a struct into its
  fields' values (multi within the bound, a box over it) changes every
  layout that holds one, the erased storage included; the object model's
  backend move (OBJECT_MODEL_TIER1.md job 11, REFCOUNT_RUNTIME.md)
  replaces this layout anyway.
- **Cost.** A field write allocates one object per struct on the path,
  so a loop that writes a field of a `let mut` struct allocates per
  iteration; a data type's field write allocates nothing. Reads cost what
  a data field read costs. Correct first: inline layout and in-place
  writes come with the linear-memory backend.

**References: a pair of closures.** `&T` and `&mut T`
([Values And References](../../spec/lang/13-values-and-references.md))
are emulated, not laid out as addresses: a reference is an object of the
compiler-supplied data type `std.core.Ref[T]` (`hd_resolve::seed`), whose
first two fields are closures, `get: fn() -> T` and `set: fn(T) -> void`,
that read and replace the referenced place, and whose last three are the
place's identity, `base: dyn Any`, `serial: i64` and `key: string`. The checker builds both when it
checks `&e` (`crates/hd_check/src/refs.rs`) and lowers every `&T` and
`&mut T` in a finished body to `Ref[T]` (`hd_types::Types::lower_refs`,
`hd_tir::ir::map_body_types`); the driver lowers the declarations the
backend reads (`params`, `ret`, `data_fields`, enum payloads, impl heads,
row keys), and `hd_structure` its generated bodies. So the backend sees an
ordinary generic data type, and the emitter is unchanged but for one line.

- **Places.** `&x` on a local makes closures that capture `x`; since the
  `set` closure assigns it, the capture is `Shared` and `x` lives in a
  shared cell, the existing closure mechanism, one fresh cell per
  execution of its declaration. A parameter moves into its cell on entry
  and no store of it makes a fresh one (`cell_locals`), so `&mut p` of a
  `mut` parameter writes the callee's copy. `&e` on any other value binds
  it to a fresh local that only the closures capture: the new object.
- **Interior places.** A struct field path, `&mut p.pos.x`, reads the
  root and the path in `get`, and in `set` rebuilds each struct on the
  path with `CopyData` and stores the root, as a struct field write does
  ("Struct values" above). A place reached through a reference,
  `&mut r.pos`, evaluates `r` once into a fresh local; its closures call
  `r`'s own. A data field, `&mut d.n`, uses `struct.get` and `FieldSet`
  on the data object, evaluated once.
- **Identity.** `r is s` compares the three identity fields
  ([Identity](../../spec/lang/13-values-and-references.md#identity)):
  the emitter lowers `Is` on two `Ref` operands (`ref_identity_eq`) to
  `ref.eq` on the bases' payloads, `i64.eq` on the serials and the string
  helper on the keys. It decides by the operand's type after
  substitution, so `is` in a generic body over `T < AnyRef` compares
  places when `T` is a reference. The checker fills the fields where it
  takes the reference (`refs.rs`, `place_identity`):

  | Place | `base` | `serial` | `key` |
  | --- | --- | --- | --- |
  | a local or parameter | `RefRoot`'s one value | the local's serial | the struct field path, as `.1.0` |
  | a new object | `RefRoot`'s one value | a fresh serial | `""` |
  | a top-level binding | `RefRoot`'s one value | 0 | `g:` and its path, then the field path |
  | a field of a data value | the data object | 0 | `f` and the field index, then the field path |
  | a place reached through `r` | `r.base` | `r.serial` | `r.key` and the field path |

  A serial comes from the `ref_serial` intrinsic, a runtime counter
  global (`rt::ref_serial_global`), so serials are unique within a run.
  A local draws its serial where it is declared: its first store, or a
  parameter's entry to the body. So each execution of the declaration
  is a new place, as each makes a new cell. Only a local whose name some
  `&` in the file names draws one (`BodyCx::ref_names`, a scan of the
  file's syntax for `&name` and `&name.field`); the rest pay nothing.
  `RefRoot` is a private fieldless data type, so its one value is a
  canonical constant ([`expr.is.canonical-data`](../../spec/lang/05-expressions.md#r-expr.is.canonical-data))
  and no `base` but a data field's allocates.
- **Why a serial and not the cell.** The cell that holds a referenced
  local is the emitter's, made after checking; TIR has no name for it.
  A serial needs no new layout, and it hashes, which a Wasm GC reference
  cannot: `Id[T]` (lib/std/cmp.hd) hashes the serial and the key, so
  references to locals and new objects spread over a map's buckets. Two
  places in one data object differ only in their keys, so `Id` keys of
  fields of many data objects collide on the field path; that is the one
  case a hash of `base` would help.
- **`dbg`.** The structural printer `dbg_show` (`hd_structure::dbg`)
  prints a `Ref[T]` as the value its `get` closure returns, so no field
  of the emulation shows ([`module.dbg.value.reference`](../../spec/lang/10-modules.md#r-module.dbg.value.reference)).
- **Reads and writes.** `*r` is `Field(r, get)` then `CallValue`, and
  `*r = v` calls `set`; a field read or method call through `r` reads
  through `get` first. A read-only reference's `set` is `unreachable`,
  since the type system never calls it.
- **Why not a cell per reference.** A reference to a field of a struct
  value has nothing to point at: struct values are immutable objects
  shared by every place that copied them. A pair of closures handles a
  local, a field path, a value behind another reference and a data field
  with one layout, so `&mut T` to `&T` is free (both lower to `Ref[T]`),
  and generic code needs nothing new: `Ref[T]` takes `A1` erased storage
  like any generic data type.
- **Cost.** Taking a reference allocates the `Ref` object and two
  closures (the read-only `set` is a constant); a local some reference
  refers to lives in a heap cell for the whole function. A read or write
  through a reference is an indirect call, and a write to a field path
  rebuilds the path. A reference to a field of another reference
  concatenates its key, and `is` compares two strings. A local whose name
  some `&` in the file names draws a serial when declared, an `i64`
  global increment, even when no reference to that local is taken.
  Correct first: the linear-memory backend makes a reference an address,
  and `is` one compare (OBJECT_MODEL_TIER1.md job 11).
- **Trait selection.** std writes `Eq`, `Hash`, `DeepEq`, `Debug` and
  `Display` for `&T` and `&mut T`; both heads lower to `Ref[T]`. The backend's selection
  (`hd_mono` `select`) raises a lowered self type back to `&T`
  (`hd_types::Types::raise_refs`) to match those heads, then unifies the
  lowered head with the lowered self type for the impl's arguments. So
  the `&T` impl serves `&mut T` too once lowered, and the two heads never
  meet as one.
- **Reference receivers.** A `&self` or `&mut self` method takes `Ref[Self]`
  as its first parameter
  ([Reference Receivers](../../spec/lang/07-functions.md#reference-receivers)).
  A call on a place takes its address in the checker
  (`crates/hd_check/src/receiver.rs`): the receiver is read as `&` reads its
  operand (`refs.rs` `operand`, no code for the place itself) and the call
  passes the `Ref` that `make_ref` builds for it, so the backend sees an
  ordinary call. A receiver that already is a reference is passed as is; a
  `&self` call on a value that is no place builds a `Ref` to a new object.
  A bound method reference (`p::shift`) captures that `Ref` instead of the
  value. The cost is a reference's per call: one `Ref` object and two
  closures, and the receiver local lives in a heap cell for the whole
  function. Only a local whose name some `&` in the file names draws an
  identity serial, so a receiver otherwise gets a fresh serial per call.
- **The receiver bridge.** A struct's `&mut self` method may fill a trait
  method declared `mut self`
  ([`trait.impl.receiver-bridge`](../../spec/lang/09-traits.md#r-trait.impl.receiver-bridge)).
  Its instance takes `Ref[S]` while the trait's declaration passes `S`, so
  only a call the checker resolves to the struct's impl (choice `Impl`,
  receiver addressed) may reach it. The checker rejects every other use, and
  `hd_mono`'s `method_target` stops a call through a bound that selects
  such a method (`bridged`) with an unsupported-stage error rather than
  emit a mismatched call. Trait values have no slot for a `&self` or
  `&mut self` method (the driver's `trait_methods`), until trait values
  become references.

**Strings: Go-style shared slices.** A string is a
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
  [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#string)). Three values
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

**Defaultable forms.** A language layout may hold
a non-null reference: data, lists, strings, closures. Wasm has no default
value for one, so a slot that may be inactive cannot hold it. Each layout
`L` therefore has a defaultable form `dflt(L)`, the same Wasm values with
each non-null reference made nullable, and `default(L)` fills it with
null or zero. The rule is applied recursively, wherever a slot can be
inactive:

| Slot | Form | Narrowed by |
| --- | --- | --- |
| an inactive payload of a value enum or `Result` (`multi` layout), and the payload fields of a flat enum struct | `dflt` per payload field; the inactive variant's fields hold `default` | the tag test, then `ref.as_non_null` on the read field |
| the Pending result and the resume arguments of `f$body`, every saved field of a suspension frame, the result fields of an `all!` frame | `dflt` (crates/hd_wasm/DESIGN.md §14.1) | the state test or the done mask |
| a module storage global before init | `dflt` (§15.4) | init order; reads after init narrow |
| an `Array[T]` slot past the count that `List`, `Map` or `Deque` keeps | `dflt` per element value; such slots hold `default` and std never reads them | std's count check, then `ref.as_non_null`, or the A1 `ref.cast` to a non-null type, which checks both |

Language-level locals, parameters and fields keep the non-null form, so a
value that is always present costs nothing. A narrowing read is one
`ref.as_non_null`, which cannot fail after the test. `struct.new_default`
is valid for every frame type.

Implementation differs: `Asm::local` declares a reference local in its
defaultable form and emits `ref.as_non_null` on each read of a non-null
one. `Asm::local_init` instead declares the local non-null and writes it
at declaration: an instruction's result stored in the block the
instruction started in, a `let` binding's first write, and a temporary.
Wasm accepts a read of such a local only after a write in the same or an
enclosing block. `Asm` tracks that check, and a read that fails it, such
as a `defer` suite reading a body binding or a match arm reading a
decision's binding, demotes the local to its defaultable form; finish
then narrows each of its reads. A suspending body declares every local
defaultable, since it reloads them from frame fields (#274).

**Enum layout per enum.** Every enum is an identity-free value
(S1c) and normally uses the value layout of "One predicate for
identity-free enums" below. The flat and subtype rows above are the
shapes of its **box**, used past the bound (§15.1) and for a self-recursive
payload. The rule is deterministic, with no annotation: a box is
**flat** when the enum's payload fields number at most 4 in total.
Otherwise it uses **subtypes**. Flat enums need no
cast on a match, and their payloadless variants need no allocation.
Subtype enums cast once per matching arm, which the engine checks with
one load and compare.

**Self-recursive enums.** An enum is
self-recursive when its declaration reaches itself through value
positions: enum payloads, tuple elements, `T?`, and another enum's
argument when that enum holds the parameter in a value position. Data,
collections, functions and trait values are references and end a path.
A cycle of declarations decides, found once per program
(`hd_mono::layout::recursive_enums`), so a payload naming its own enum at
growing arguments (`E[T?]` in `E[T]`) is boxed like a plain one and every
other value layout is finite. A projection in a value position is not
resolved there, so its enum counts as self-recursive. "Payload fields"
counts declared fields across the variants.

Implementation differs: the subtype base (`layout::enum_base`) holds the
tag only, a payloadless variant allocates a base struct (no singleton
globals), and a payload read casts per field, not once per arm; that
allocates for every payloadless value and repeats casts.

**Erased scalars.** In an erased position, `bool`, `char` and integers of
16 bits or less become `i31ref`. Wider integers (`i32`, `u32`, `usize`,
`i64`, `u64`) become `i31ref` when the value fits in 31 signed bits and
are boxed in `$Box_i32` or `$Box_i64` otherwise. Erasing checks the range, which is a shift, a
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

Implementation differs: only an `i32` slot has the `i31ref` form. `Em::erase`
calls `Helper::EraseI32`, which makes an `i31ref` when the value fits in
31 signed bits and a `box_of([i32])` otherwise. `rt::unerase` calls
`Helper::UneraseI32`, which tests for `i31` (`ref.test`, not
`br_on_cast`) and reads the box on a miss. Each is a helper call, not
inline code. An `i64` and a float are still always boxed.

**Identity.** These layouts rely on the language giving value types no
identity. `.Some`, `.Ok`, `.Err`
and primitive, string or tuple boxes have no identity. `is` is a compile
error on an operand whose static type is a value type and on function
values; since S1c that covers every enum, optionals and `Result`
included. On an `Any` or trait value that holds a value at run time,
the result is unspecified. The spec has this as
[`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand),
[`expr.is.box-values`](../../spec/lang/05-expressions.md#r-expr.is.box-values)
and
[`expr.is.box-unspecified`](../../spec/lang/05-expressions.md#r-expr.is.box-unspecified).
Keeping identity would cost one 16 to 24 byte allocation per `.Some`
that crosses a call, which `T?`-heavy code pays on every lookup, and an
optional's identity has no known use.

So the layouts follow these rules:

1. **`T?`.** `.Some(x)` is `x` itself for a reference `T` and `(1, x)`
   for a scalar `T`. Nothing is allocated.
2. **Boxes.** A primitive, string or tuple converted to a trait value or
   `Any` is erased as above. Equal small scalars give equal `i31ref`s, and
   two boxes of one value are two objects; `is` on such values is
   unspecified, as for function values.
3. **`Result` has no identity either.** It uses the `multi` layout `(i32 tag, T', E')` and allocates
   nothing. A field that holds a `Result` stores the same fields
   inline, or the box past the bound. Its payload fields share slots
   by §15.1's slot-sharing rule: when `T'` and `E'` are references of
   different types, they share one `eqref` slot cast after the tag test.
4. **Other enums** are identity-free too (S1c). They follow the
   predicate below; the flat and subtype shapes are only their boxes.

**One predicate for identity-free enums.** Layout, `is` lowering
and the checker's value-type test read one predicate, `identity_free(E)`.
Since the S1c spec pass it holds for every enum
([`types.sealed.anyval-values`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-values)).
An identity-free enum
uses a value layout: a nullable reference when it has one reference
payload and one payloadless variant (`T?`), else `(i32 tag, payload
fields...)` as `multi`, boxed in one immutable struct past the bound in every position
as §15.1 says. A self-recursive payload stays a reference, as the flat
or subtype layout above has it.

**Representation equivalence.** Identity-free
means the enum's own shell may be copied, deduplicated or hoisted into a
constant. It does not mean a referenced object may be replaced by an
equal one. Two enum values are interchangeable only when their value
components are equal by representation and their reference components
are the same objects (`ref.eq`); hd's `Eq` is never the test. Constant
hoisting, enum hash-consing and the fact graph (crates/hd_mono/DESIGN.md §12.3) all
use this rule.

**Emitting `is`.** The TIR `Is` instruction
([crates/hd_tir/DESIGN.md](../hd_tir/DESIGN.md#instruction-catalog)) lowers by
the operands' layout:

| Operand layout | Wasm |
| --- | --- |
| `ref` (data, lists, maps, an `AnyRef` type parameter's instance) | `ref.eq` |
| `Ref[T]`, a reference ("References" above) | its identity fields compared |
| trait value, `Any` | `ref.eq` on the payloads; unspecified when the payload is a boxed value |

An enum, optional, `Result`, tuple, primitive, string or function operand
has no row: `is` on it is the compile error
`identity-requires-references` (S1c,
[`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)).
No type-id test is needed in the last row. Two different objects are
never `ref.eq`, and two equal `i31ref`s of different types are boxed
values, whose result is unspecified.

**Arrays.** Not built yet; see #180: std has no `Array[T]`, and `List`
and `Map` are compiler-known types over Wasm arrays. The design:
`Array[T]` is the one compiler-known collection, `(array (mut
T'))` with `T'` the element layout of the table. An array of a `multi`
layout, such as `Array[Option[i32]]` or `Array[(i32, f64)]`, is a struct
of one array per Wasm value: a structure of arrays, which keeps
every element unboxed, as the spec's packed `List[i32]` and list of
tuples ask, and allocates nothing per element. `Array[T]` is intrinsic,
so `List[T]` and `Map[K, V]` in std see one type either way.

- **Element forms.** An element is stored in its
  defaultable form, since slots past a collection's count hold
  `default(L)`. Under A1 an element of a reference layout is `eqref`.
  An element over the bound is the box reference.
- **`Array[void]`** has no Wasm array: it is one shared empty constant
  struct, and its operations emit nothing. So `Map[T, void]`, which
  `Set[T]` wraps, has no values array, with no special case.
- **Collections over plain storage.** `Heap[T]` is a `List[T]` with sift
  operations, and `Deque[T]` a ring buffer over `Array[T]` with a head
  and a count. Neither stores `T?`, which for a scalar `T` doubles the
  arrays and adds a tag test per access. `Set[T]` wraps `Map[T, void]`
  with an internal insert-if-absent, so `insert` probes once.
  Implementation differs: `Deque` and `Heap` in `lib/std/collections.hd`
  store `List[T?]`, and `Set` wraps `Map[T, bool]`; that doubles scalar
  storage, adds a tag test per access, and stores a `bool` per entry.
 `Map` is the compact
  insertion-ordered table of [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#mapk-v).

### 15.3 The Type Section

1. **Canonical descriptors.** The linker maps each hd type to a canonical
   Wasm type descriptor: kind, fields with mutability and storage type,
   supertype, finality. Structurally equal descriptors are one type.
2. **Structural, not nominal.** hd never tests an object's Wasm
   type to learn its hd type. Enum variants use the tag, and `Any` uses
   the vtable's type id. So two hd types with the same layout may share a
   Wasm type, which shrinks the type section and lets instances fold
   (§13.7).
3. **Rec groups.** The type graph's strongly connected components become
   rec groups; a non-recursive type is a group of one. Members of a group
   are ordered first so that a declared supertype precedes its subtypes,
   then by the members' `canon(T)` bytes (crates/hd_mono/DESIGN.md §13.3), a nominal
   key that exists before any Wasm index does. Two groups are one type
   when their encoded bytes under that order are equal. Recognizing equal
   recursive shapes under different member names waits until size
   measurements ask for it.
   **As built.** Storage erases a reference to `eqref` in an
   enum's slots and box and in a list's or map's arrays, and layout reads
   only the parts' storage there, so Wasm types recur only through data
   fields and trait values (vtable slot types), possibly by way of
   tuples, optionals, closures and suspensions. Layout runs Tarjan's
   algorithm, with an explicit stack, over those two nominal kinds, and
   builds each component's descriptors after the components it names; a
   member is named by its position until the group closes. The group's
   members are its nominal types, ordered by `canon(T)`, and the types
   inside them that name one (a closure code type, a slot type, a
   tuple box), ordered by their content hash over the nominal order,
   supertypes first. A nominal type on no cycle is a group of one that
   link emits as an ordinary type. A descriptor is `Rec(group, i)`,
   inside a member `Back(i)`; a group is identified by a hash of its
   members that names other groups by their hashes, so groups compare in
   constant time. A code entry writes its groups once, in a table before
   the entry, each after the groups it names, so no encoding nests; link
   emits a group's named groups first with a loop. The program's
   emissions share the laid-out nominal types, so a group is laid out
   once per build. At link, a type built outside the group whose
   resolved form equals a member's (a function's type from its
   parameters, a closure code type from its signature) is that member. An
   item whose declarations recur at ever larger arguments (`D[T]` naming
   `D[List[T]]?`, an expansive cycle in Kennedy and Pierce's sense) has
   no finite descriptor and stops emission as unsupported; the check
   runs only when an item is on the walk's path at two arguments.
   **Cost.** A code entry carries the descriptors of every group it
   reaches, so a program with one group of `N` types used by `N`
   functions, or a chain of `N` nested types, writes `O(N^2)` descriptor
   bytes: about 0.26 s at `N = 800` and 1.5 s at `N = 3200` cold. A
   program-wide type table would make it linear; see #49.
4. **Subtyping.** Declared only where hd needs it: enum variants under
   their base, frames under `$Suspend_L`, closure environments under
   `$Fn`. Every leaf is `final`, which lets engines skip subtype
   checks.
5. **Order.** A Wasm type may only name types of its own group or of
   earlier groups, so groups follow a topological order of the SCC
   condensation graph: a group comes after every group it references.
   Among groups that are ready at the same step, the one with the
   smaller encoded bytes comes first (Kahn's algorithm with a sorted
   ready set). The type section is then a pure function of the type set,
   and valid by construction
   ([Wasm type validity](https://webassembly.github.io/spec/core/valid/types.html)).
   Implementation differs: link emits each group on first use, after the
   groups it names (a depth-first order over code in content order),
   which is valid and deterministic but depends on code order; not judged.
6. **Stability.** No dense order is stable under
   insertion: a new type that sorts early shifts the type immediates of
   later functions, and wasmtime's per-function cache keys include them.
   This order is kept until a measurement shows fewer than 90 percent of
   functions hitting the cache after adding one type; then allocation and
   cast sites name types through a per-type helper. Under A1, fewer types
   exist, so fewer shift.

### 15.4 Globals And Module Initialization

| Global | Wasm | Initialized by | A reader holds |
| --- | --- | --- | --- |
| runtime state: panic category, the wake table, the forbidden-context counter | mutable | constants | `global.get` of a fixed low index |
| one literal table per source module | immutable `(ref $LitTab)`: a `(ref $Pool)`, an `(array (mut (ref null $bytes)))` with one slot per pooled literal of the module, and the module's index-area offset | a constant expression (`struct.new`, `array.new_default`) | nothing: only the module's literal getter reads it |
| vtables, capture-free closures, payloadless variant singletons, member handles, witnesses, `TypeId` values | immutable | a constant expression (`struct.new` and `ref.func` are constant in Wasm GC) | `global.get`; globals of a kind ordered by content key |
| string literals of 4 bytes or less | immutable | a constant expression (`array.new_fixed`) | `global.get`, then `i64.const` of the literal's span (`len << 32`) |
| other string literals | a slot of their module's table | lazily: the module's getter runs `array.new_data` on segment 0 at the literal's offset | `i32.const` of the module-local number, a `call` of the module's getter, then `i64.const` of the span: a literal is a view over its pooled array |
| module storage (top-level bindings) | mutable, nullable or zero | the group's init function | `global.get`, `global.set`; ordered by module path, then binding index |
| fact values and metadata | mutable, nullable; null is the flag for a one-reference layout, else an `i32` flag | lazily: a getter runs the fact's body on the first read (crates/hd_mono/DESIGN.md §12.3) | a `call` of the getter |
| shared enum data | mutable, nullable or zero | the declaring module's group init function ([`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init)) | `global.get` |

Implementation differs: of the constant kinds, capture-free closures,
witnesses, fieldless data values, payloadless variant singletons
(`GSym::Variant`) and vtables (`GSym::Vtable`) are globals. A vtable
global's initializer is its whole tree, supertraits included, written
once per tree and named by its hash (`Em::vtable_value`), so a coercion
reads it and allocates nothing; a subtable the tree repeats is built
again in the initializer, once, at instantiation. A `TypeId` is not a
constant: `Helper::TypeIdOf` builds it on its type's first request into
a mutable global (`GSym::TypeId`) and returns the global after, so only
that first call allocates. The global is local to its module, as a
constant closure is: each session module that links the helper has its
own, so no two modules of a session define one name (#314). Short literals have no `array.new_fixed` globals.

**Stable numbering.** A reader never holds the index of
a lazily initialized global: it calls a getter, and wasmtime's
per-function cache abstracts call targets, so adding a literal or a fact
elsewhere recompiles no other function. "Append-friendly" global order
would need numbers kept from earlier builds, which §15.8 forbids. The
immutable constants are read directly; a global kind moves behind
getters if adding one global leaves fewer than 95 percent of functions
hitting the cache. The full table of index spaces is
in [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#numbering).

**Constant strings.** All pooled literals share one passive data segment,
deduplicated by content, so the program has one data segment. After the
bytes come each module's index area: an `(offset, length)` pair per
module-local literal number. Each module has one literal table and one
getter, `$lit_m(k)`, which reads slot `k` and on null calls one shared
fill helper. Module-local numbers depend only on the module's own
literals, so they are stable under every edit elsewhere
([docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#literals) compares this with
one getter per literal). A function reads each literal once on a
dominating path. A host call whose argument is a literal, such as
`println("hello")`, takes the host fast path: the module's span function
copies the bytes from segment 0 into the exchange buffer with
`memory.init` and returns the length, so no array is built at all, and
the offset stays inside that function. Eager filling of the tables in
`hd.init` is a later measurement.

Implementation differs: there is one table per linked module, not one
per source module. A use writes `i32.const` with a `Reloc::Lit`
immediate, then calls the one getter (`Helper::Lit`, `rt::lit_code`).
Link numbers the module's distinct literals in content order and patches
each immediate, so no cached code entry holds a literal number, and the
function count does not grow with the literals. The getter reads each
literal's offset and length from an index after the literal bytes in
segment 0, built on the first miss. The host fast path is not built: a
literal passed to a host call is built as an array first.

- **Init order.** The `hd.init` export calls each reachable group's init
  function once, in `InitOrder`'s order, after every group it
  uses ([`module.init.cycle.once`](../../spec/lang/10-modules.md#r-module.init.cycle.once)).
- **No Wasm `start` function.** Init can panic and can call
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

**No site numbers.** A call of a stub carries no site
immediate. The site is the stub's caller frame: its function index and
the code offset of the `call`, which both engines' backtraces give, and
`hd.sites` is keyed by function and code offset. So no body holds a
program-wide site number, and a new site elsewhere recompiles nothing
else (the compile study found that site numbers alone made one edit miss
the per-function cache for about half a debug program). Each check keeps
its own `call` instruction, so two sites never share an offset. Category
ids are fixed by the runtime, not numbered per program.

Implementation differs: a panic stub (`Helper::Panic`) calls the
`hd:rt.panic` import, which writes `panic: ` and starts the report,
writes `CATEGORY: MESSAGE` and a newline through the `hd:rt.stderr`
import, then executes `unreachable`. There is no category global or
category id: the host records the report's text from the `panic` call
to the trap, and the category is that text up to its first `: `, which
only the compiler writes. A facts body calls the `hd:rt.fact` import as
it starts, with its label, and as it ends; the host starts every report
written in between, its own after a trap included, with
`fact-evaluation-failed` naming the facts
([`annot.fact.eval.panic-runtime`](../../spec/lang/14-annotations.md#r-annot.fact.eval.panic-runtime);
crates/hd_run_wasmtime/DESIGN.md §17.2). Not judged.

**Engine traps.** A trap without a stored category is mapped by its trap
code and its code offset:

| Trap | Category |
| --- | --- |
| integer division by zero | `integer-division-by-zero` |
| array out of bounds (string byte access) | `index-out-of-bounds` |
| call stack exhausted (wasmtime's trap code; V8's `RangeError`) | `stack-exhausted` |
| epoch deadline (crates/hd_run_wasmtime/DESIGN.md §17.8) | `time-limit` |
| GC heap growth refused (crates/hd_run_wasmtime/DESIGN.md §17.8) | `heap-exhausted` |
| null dereference, failed cast, `unreachable` without a category | an internal error: a compiler bug, reported with the site and exit status 101 |

Implementation differs: integer division emits an explicit zero check
that calls a panic stub, rather than mapping the engine trap; not judged.

**Sections.**

| Section | Holds | Release |
| --- | --- | --- |
| `name` | function names: printed stable path plus type arguments, as `shop.cart/Cart.total` or `std.list/List.push[i32]` | no: the dev pipeline only |
| `hd.names` | per function: an index into a path table and a list of indices into a type-argument table; the symbolizer prints the same names as `name` | yes; about 30 KB instead of 150 KB at 10k lines |
| `hd.lines` | per function: sorted code offsets with an anchor each (§15.9) | yes |
| `hd.positions` | only in a module `hd build` writes: a string table of files and printed names, then per function its file and name indices and each `hd.lines` row's line as a zigzag delta (§15.9); about 90 bytes for hello | yes |

Planned, not built: `hd.folds` (folded aliases per representative,
§13.7) and writing `hd.runtime` (crates/hd_run_wasmtime/DESIGN.md §16.4; `hd_wasm::meta` defines its
encoding). `hd.sites` is not written (§15.9).

Spans become line and column at link time, from the source's line table.
The link key includes the source hashes through the code keys, so a
stale line cannot survive.

**Backtraces.** wasmtime's `WasmBacktrace` gives each frame's function
index and module offset. V8's `Error.stack` gives
`wasm-function[i]:0xOFF` frames. `hd_run` maps both through `hd.lines`,
and `hd.names` (or `name` in the dev pipeline) to `file:line function`,
and prints the panic's own site first: for an explicit panic, the frame
that called the stub.
Release builds keep these sections, so release backtraces are symbolized,
as the first-release feature list asks. They omit the standard `name`
section, so external tools such as browser devtools,
`wasm-objdump` and native profilers show unnamed functions for a release
module; a dev build keeps `name` for them.

**Mapping must survive every Wasm-level pass.** `hd.lines` and
engine-trap mapping key on code offsets, so any pass that rewrites the
code section after emission must carry the offsets through. A Binaryen
pass would have to round-trip them through a source map
(`--input-source-map`, `--output-source-map`), or it does not run. An hd
inliner must record each inlined call site in `hd.lines`
([docs/compiler/tiering.md §4](../../docs/compiler/tiering.md#4-divergence-between-tiers)).

Planned, not built: a browser devtools source map, built from `hd.lines`
by the program worker on request and served through a blob
`sourceMappingURL`, so the binary carries none.

### 15.6 The 2 KB Tiny Program

There is no runtime blob. Everything in a module is reachable code
generated for that program, std included (crates/hd_run_wasmtime/DESIGN.md §16.1). The tiny program
(`println("hello")`) needs:

| Part | Estimate |
| --- | --- |
| header, type section (`$bytes`, the import and export signatures) | 70 B |
| imports: `hd:Console` `write_line`, `hd:rt` stderr writer and panic start | 60 B |
| function, memory (the exchange buffer), global, export sections | 80 B |
| code: the entry wrapper, the copy loop to the exchange buffer, std's `println` and its panic on a closed pipe | 300 B |
| data: `hello`, copied to the exchange buffer by `memory.init` (the host fast path of §15.4: no pool, no string array) | 10 B |
| `hd.names`, `hd.sites`, `hd.lines`, `hd.runtime` | 300 B |
| total | about 800 B (an estimate, not an accounting) |

The hello module is about 4 KB in release today, so it misses the 2 KB
target; the size items are #175.

What keeps it small:

- reachability and folding; no unused type; no literal pool when every
  literal goes straight to the host;
- compact LEBs in release (§13.10);
- the string copy loop is one shared helper per program;
- short export names (`hd.init`, `hd.poll`, `hd.wake`, `hd.x`).

**Start to first output (≤ 5 ms).** A warm `hd FILE.wasm` deserializes
the cached precompiled module, instantiates it from an `InstancePre`,
and runs `hd.init` and `hd.poll`. Process start of `hd` dominates: the
`startup` target is 20 ms for `hd --version`, so a cold process may miss
5 ms.

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
  Implementation differs: only tests validate (`hd_wasm` link tests,
  `hd_driver` session tests); not judged.
- **WAT** for the playground's view and `hd build --wat` (Later) comes
  from `wasmprinter`.

### 15.8 Deterministic Bytes

Reproducible builds are parked as a metric, but the cache needs
deterministic bytes anyway: equal keys must mean equal entries. Every order in §13.10 and §15.3 is content-based: instance
keys, canonical descriptors, content keys of constant globals, and
literal pool slots by literal content. No hash map's iteration order
reaches the output. Locals in a body are numbered in TIR order.


Deterministic is not the same as stable. A content order still shifts
when an item is inserted before others, and a body that holds a shifted
index misses wasmtime's per-function cache. So bodies hold no dense
number except a call target: lazily initialized globals sit behind
getters, panic sites are code offsets, and key ids and type ids are
content hashes (§15.4, §15.5,
[docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#numbering)). Numbers kept from
earlier builds would make the bytes depend on history, so they are not
used.

### 15.9 The Line Table

`cli.test.report.frame` prints a panic's source frames in `hd test`
reports: `FILE:LINE FUNCTION` lines, panic site first.

- **`hd.lines`** holds, per function, its rows as (offset delta, TIR
  instruction index). Emission records a row at each call that starts a
  new anchor; calls in a row at one anchor share it. Every panic site is
  a stub call, so call rows cover them all. The item comes from the
  function's name, so the table repeats no path. A frame whose address
  falls between two recorded offsets belongs to the lower one: both
  engines give a caller frame the offset of its call.
- **Names.** A release build writes `hd.names` (per function, a
  path-table index plus type-argument indices) and drops `name`; a dev
  build keeps `name` and writes no `hd.names`. Both hold stable paths
  (`path#sub`), which key the `check` entry's bodies.
- **No `hd.sites`.** The category and message travel in the stub's
  report, and a stub has no rows, so the first anchored frame is already
  the stub's caller. `hd.sites` would only repeat `hd.lines` rows, so
  leaving it out keeps modules smaller with no loss.
- **Resolution.** Anchors hold no lines or columns, so a comment edit
  changes the reported line and nothing compiled or cached.
  `hd_driver::Positions` resolves an anchor on the first trap of a
  program: the module's `check` entry gives the TIR syntax node, and a
  parse of the file the run read gives the line. One `PositionPool` keeps
  each module once for all programs of an `hd test` run. `hd build`
  keeps the rows anchored and adds `hd.positions`: each row resolved to
  its line when the module is written, with each function's file and
  printed name stored once in a string table, so `hd FILE.wasm` prints
  a panic's frames without the compiler's cache.
- **Reading.** Only on a trap. Node reports V8 frames with
  `Error.stackTraceLimit = Infinity`; wasmtime reports `WasmBacktrace`
  frames, with its frame limit set to `max_wasm_stack / 16`, which no
  stack can exceed. Only guest traps have frames: a host stop (time
  limit, host contract, exhausted heap, deadlock) prints none.
- **Printing.** A frame prints the hd name, not the stable path; the CLI
  maps a path to a name only when it prints, so dev and release print the
  same names and the module does not grow. The name is the module's
  printed path (`trait.typeid.name.*`: the package name with `-` written
  `_`, or a single file's stem; a test or task module drops its `$tests`
  or `$tasks` segment), then the function. A method prints its impl's
  type without generic arguments, then the method
  (`std.collections.List.remove_at`). A hidden segment, such as a default
  body, is left out. A test case's body prints `test "NAME"`.
- **Repeats collapse.** A frame repeated right after itself prints once,
  then `... frame repeated N more times`. A cycle of up to eight frames
  prints once, then `... last K frames repeated N more times`; at each
  frame the cycle covering the most frames wins. So unbounded recursion
  prints a few lines, though the engines capture every frame (about
  27,000 on Node and 131,000 on wasmtime at a 4 MiB stack).

# New Compiler Design: Suspension Lowering

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## 14. Suspension Lowering

The spec fixes the protocol: cold, one-shot, poll-based suspensions,
a suspending `cancel!` with one cleanup per suspension, and a
waker-driven entry driver
([Compilation Strategy](../../spec/lang/11-requirements-and-suspension.md#compilation-strategy)).
Suspensions lower to state machines with lazily materialized frames,
which
[`req.lowering.representation`](../../spec/lang/11-requirements-and-suspension.md#r-req.lowering.representation)
allows.

### 14.1 Functions Per Suspending Body

For each instance of a suspending function `f!`:

| Function | Signature | Use |
| --- | --- | --- |
| `f$body` | `(frame: (ref null $Suspend_L), dflt(args)..., dflt(providers)...) -> (dflt(T), (ref null $Suspend_L))` | the body. A null frame starts at state 0 with arguments in locals. A non-null frame is cast to the private `$F_f` and resumes from its saved state; its arguments are default values that it ignores. A null second result means Ready with the first; a frame means Pending, and the first result is `default(T)` |
| `f$cold` | `(args..., providers...) -> (ref $Suspend_L)` | the plain call `f(args)`: allocates a private `$F_f` holding the arguments and providers in state 0, then returns it at the public result-layout base type |
| `f$poll` | `(frame: eqref) -> (i32, dflt(T))` | the result-layout vtable's `poll`; casts the frame to `$F_f` and calls `f$body` with it |
| `f$cancel` | `(frame: eqref) -> ()` | the layout-independent `$Task` cancel function (§14.6); it casts the frame to `$F_f` |

**Defaultable forms.** A value of a data, list or
closure type is a non-null reference, which has no default inhabitant.
So every slot that can be inactive uses the defaultable form `dflt(L)`
of its layout (crates/hd_wasm/DESIGN.md §15.2): each non-null reference becomes
nullable, and `default(L)` is null or zero per Wasm value. Here that is
the Pending result, the arguments of a resume call, and every saved
field of a frame. The state test proves a slot active before it is read,
and the read narrows with `ref.as_non_null`, which cannot fail. On the
fresh path the arguments arrive non-null, and the prologue narrows each
reference argument once into a non-null local.

Every suspension starts with the layout-independent `$Task` prefix.
`$Suspend_L` extends it once per result layout `L` (§15.2), and the
callee-private `$F_f` extends that base:

```text
$Task      = (sub (struct (field $cancel (ref null $CancelFn))
                          (field $state (mut i32))      ;; resume point; 0 = not started
                          (field $flags (mut i32))))    ;; ACTIVE, DONE, CANCELLED, UNWINDING, ABANDONED bits
$Suspend_L = (sub $Task (struct ... $Task fields ...
                          (field $poll (ref null $PollFn_L))
                          (field $driver (mut anyref))))
$F_f       = (sub final $Suspend_L (struct ... base fields ...
                          (field $child (mut (ref null $Task)))
                          ... saved locals ...))
```

The public body signature uses `$Suspend_L`, not `$F_f`: a caller knows
the callee's result layout before the callee's private frame type is
emitted. Parents retain a child as `$Task` and cancel it without knowing
either its result or frame layout.

Implementation differs: there is no separate `f$cold`; the instance's
own function is the cold constructor, and its parts are `f$body`,
`f$poll` and `f$cancel` (`hd_wasm::Part`). That is the same design under
other names. `$Suspend_L` has no `$driver` field (`layout::suspend_base`):
competing drivers are detected only at the root, by the `owner` runtime
global that the first `hd.poll` context claims. Not judged.

### 14.2 The State Machine

Emission builds the state machine from TIR's explicit suspension points;
TIR itself is not changed:

1. **Liveness.** One backward dataflow over the body's structured
   blocks, iterated to a fixed point on loops (§12.1), gives the live
   set at every suspension point at once. It is not a scan per point,
   which would cost points times instructions. A point's frame fields
   are its live locals and values, plus the locals that the `defer`
   suites of the enclosing `Scope`s read (the point's side record names
   those scopes). The analysis is layout-independent, so it runs once per
   body per run and is memoized.
2. **Blocks containing a suspension point** are flattened into a
   dispatch loop: `loop $dispatch { block_n ... block_1 {
   br_table pc } ... case code ... }`. Each basic block of a flattened
   block becomes one case, and a transfer sets `pc` and branches to
   `$dispatch`.
3. **Blocks without a suspension point** stay structured inside their
   case. Most control flow (a non-suspending loop, an `if`, a match)
   keeps its plain Wasm shape.
4. **Resume points** are the cases that follow each suspension point. The
   prologue reads `frame.state` and sets `pc` to its case, after
   reloading the saved locals.

Code size is linear: one case per basic block of a flattened block, one
save sequence and one reload sequence per suspension point.

Wasm's structured control flow cannot jump into a nested block, which is
why blocks with a suspension point are flattened. The alternative,
re-entering the nesting with skip flags as Binaryen's Asyncify does,
costs code size on every path. The cost of flattening is a branch through `$dispatch` at
each control transfer inside a suspending loop, a few cycles.

Implementation differs: liveness runs on Wasm, not TIR, and once per
body, not per point. `emit_suspending` runs two emission passes. The
first writes no save or reload, and `live::live_at_entry` reads its code
back: a resume is one path from the body's start, so a local that every
path writes before reading needs no field. The second pass gives a field
to each local live at the start and saves all of them at every point
(`Em::save`). So a value dead before every wait is never saved, but one
live at some point is stored at every point, and nothing is cleared.
Over the 31 suspending `main!` fixtures this halved the frames' fields
and cut module size by 18%.
Blocks are not flattened either. A block list whose items hold two or
more states is entered through one `br_table` on `pc - lo`
(`Em::resume_list`): each such item starts a segment after the `end` of
its own block, so fresh code falls through every segment and a resume
branches straight to its own. A list with one such item guards only the
code before it. Constructs inside an item (`if`, `match`, a loop) still
choose their resumed branch with `pc`-range tests. Against the guard per
item before, the 31 suspending `main!` fixtures shrank by 0.4%.

### 14.3 Lazy Frame Materialization

The ready path allocates nothing (C#'s design, as the research says):

```text
;; g!(x) inside f!
call $g$body (ref.null $F_g) x ...       ;; -> (result, child frame)
if child frame is null:                   ;; Ready: continue with the result
    ...
else:                                     ;; Pending
    if f's frame is null: frame = struct.new_default $F_f   ;; every field defaultable
    save the live locals into frame; frame.child = child frame; frame.state = k
    return (default(T), frame)            ;; f is Pending too
```

- A deep chain allocates one frame per level, on the first real wait
  only. Later waits reuse the frames.
- **Dead references are cleared.** A reused frame
  would keep a value saved for an earlier wait reachable through a later
  one. So each suspension point's save sequence also stores null into
  every reference field that was live at an earlier point but is dead at
  this one. Liveness (§14.2) already gives both sets, so the clear list
  is known per point at emission. Completion and cancellation null the
  child field and every saved reference, after the `defer` suites ran.
  A capture that a pending `defer` suite reads stays live until that
  suite runs, which liveness already says. Not built: see the note in
  §14.2.
- **A resumed `f$body` resumes its child directly**: the saved child's
  type is known statically, so `f$body` calls `g$body(child,
  default(args)...)` with no vtable call. The child ignores those
  arguments, since its frame holds what it needs.
- **A stored suspension** (`s!()` on a `mut Suspend[T]`) is polled
  through the vtable: one `call_ref`.
- **A cold call** (`f(x)` without `!`) allocates its frame at once. That
  is the spec's semantics, not overhead
  ([`req.suspend.cold.captures`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.cold.captures)).
- **Runtime checks** sit in `f$poll`, not in `f$body`: polling an ACTIVE
  frame panics `suspension-reentrant-poll`, a second driver panics
  `suspension-competing-driver`, and polling a DONE frame panics
  `suspension-invalid-state`
  ([Runtime Checks](../../spec/lang/11-requirements-and-suspension.md#runtime-checks)).
  Polling a CANCELLED frame answers Pending (§14.6, "Awaiting a cancelled
  suspension").
  A bang call of a direct callee never needs them: its frame is fresh and
  has one driver by construction.

The ready-path cost of `g!(x)` is one direct call and one null test,
which fits the 100 ns `suspension-overhead` budget with room to spare.

### 14.4 Wakers And The Entry Driver

**Wakers.** A waker is a small struct `(task, slot)`: the frame of
an `all!` or the root, and a child index. `wake` sets the slot's bit in
the task's wake mask, propagates the bit up to the root, and sets the
instance's "woken" global. Wakes coalesce in the mask
([`req.waker.coalesced`](../../spec/lang/11-requirements-and-suspension.md#r-req.waker.coalesced)).

Host operations cannot hold GC references across both engines cheaply, so
the instance keeps a **wake table**: a growable array of wakers, indexed
by host handle. The host only ever names handles.

**The entry driver** is a pair of exports:

| Export | Does |
| --- | --- |
| `hd.poll() -> i32` | polls the root (the entry or the running test case); returns `-1` for Pending, `-2` or `-3` for a cancelled root whose cleanup ran to its end or was abandoned, or the exit status after `report()` |
| `hd.wake(n: i32)` | reads `n` completed handles from the exchange buffer and invokes their wakers |
| `hd.cancel()` | cancels the pending root, as `cancel!()` does with the host as the waiter; the next `hd.poll` steps its cleanup |
| `hd.abandon()` | abandons a cancelled root (`rt::Helper::EntryAbandon`); its unwinding has ended when it returns |

```text
host loop:
    status = hd.poll()
    while status == -1:
        if no host operation is pending and no timer is set: deadlock (§14.8)
        wait in the host reactor for at least one completion
        write the completed handles to the exchange buffer; hd.wake(n)
        status = hd.poll()
```

`hd.poll` drains internal wakes before it answers: while the "woken"
global is set, it clears it and polls the root
again. So `-1` always means that no wake is queued inside the instance,
and the host loop's deadlock test matches §14.8's definition. Handles
are generation-tagged, and a wake for a stale handle is ignored
(crates/hd_run_wasmtime/DESIGN.md §17.2).

One `hd.wake` call carries every completion of one reactor turn, so a
burst of completions costs one crossing. The driver returns to the host
whenever the root is Pending
([`req.entry.busy-poll`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll)).

Implementation differs: there is no "woken" global, and the root is
re-polled after every wake.

**Cancelling the entry**
([`req.entry.cancel`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.cancel)).
`link` exports `hd.cancel` and `hd.abandon` from every program whose
`main!` suspends, and `hd.cancel` alone from a REPL session. The host
owns the protocol, the same on both hosts (`host/core.mjs` `run`, and
`crates/hd_run_wasmtime/src/cancel.rs`): the first request, a Ctrl-C,
that finds the root Pending calls `hd.cancel` and sets a deadline one
cleanup bound away. The next request, or the deadline, calls
`hd.abandon` once. While the deadline stands the host waits for it as
for a timer, so a cleanup waiting on nothing else is abandoned there
instead of deadlocking. A request while Wasm runs stops the run, as a
time limit does. `hd.poll` then answers `-2` (`rt::CANCELLED_END`) or
`-3`, the host writes `cancelled` or `cancelled, cleanup abandoned`, and
the run exits with 130 (spec/cli/command-line.md, Cancelling A Run).
`hd.abandon` raises the `aband` global around the root's `f$cancel`, so
the cancel is an abandon, and closes no handle: one still open is a leak
`hd test` reports.
- **Wakers.** An `all!` has a waker node (`rt::wake_node`): a mask, its
  parent node and its slot there. While it polls child `j`, the runtime
  globals `wnode` and `wslot` name the node and `j`.
- **Leaves.** A host leaf that goes Pending registers its handle under
  them (`wake_register`, in the `wakers` table by handle slot).
- **Wakes.** `hd.wake(n)` records each completed handle in the wake table
  (`rt::wake_table`, an `i32` array by slot), where the leaf finds it.
  It also wakes the handle's registration, if the generation-tagged
  handle matches: `wake_up` sets the slot's bit and each node's bit in
  its parent, up to the top.
- **`block_on`.** It clears `wnode` for the subtree it polls and restores
  it after.
- **Cost.** Frames outside any `all!` are still walked on each poll, but
  an `all!` visits only its woken children (§14.5).
- **Only with `all!`.** Link gives `wake_register` and `wake_mark` their
  node bookkeeping only when some code polls an `all!` (it names the
  `allpolls` global), or a later session module may. Elsewhere every
  leaf sits under no node, so `wake_register` is empty and `wake_mark`
  walks no registration: hello pays for no node runtime.

### 14.5 `all!` And `race!`

`all!(a, b)` is an intrinsic with one frame type per tuple of child
result types:

- **Fields**: the child frames, one result field per child, a done mask
  and a wake mask.
- **First poll**: poll each child in argument order
  ([`req.schedule.all-unfinished`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished)).
  When every child completes on the first poll, `all!` returns the tuple
  from locals and allocates nothing of its own. Only the children's cold
  frames were allocated.
- **Later polls** visit the unfinished children in argument order. A
  child whose wake bit is clear is skipped. Polling it would only return
  Pending with no effect, so skipping is not observable, and it makes a
  wake O(woken children) instead of O(children).
  - A Pending child that registered no host leaf has nothing that would
    wake it, so `Em::await_all` wakes it at once (`wake_up`) and polls it
    again next time.
  - An `all!` whose unfinished children all wait on wakes counts as
    registered for its own parent.
  - Implementation differs: the node is made on the first poll, with
    every bit set, and kept in a frame local. An `all!` takes at most 31
    children, since its masks are one `i32`.
  - The runtime counts `all!` child polls in `allpolls`, which the module
    exports as `hd.all_polls` for tests
    (crates/hd_driver/tests/all/all_wakes.rs).
- **Cancellation** cancels the unfinished children in argument order.

`race!(tasks...)` takes a `List[mut Suspend[T]]`:

- It polls the children in list order. The first Ready wins.
- It then cancels every other unfinished child, in list order, before
  returning
  ([`req.combinator.race-losers-disposed`](../../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers-disposed)).
  A loser left UNWINDING (§14.6) keeps the race Pending: the winner's value
  waits in the frame (`F_STATE` 1), and each later poll cancels the
  losers again, which continues them, until none unwinds.
- A child that answers Pending while `CANCELLED` cancels the race at
  once, as it would cancel any awaiter (§14.6, "Awaiting a cancelled
  suspension"); a child earlier in the list that completed on the same
  poll wins first.
- An empty list at run time panics `explicit-panic`.

`all_list!` is std hd over nested `all!` (std's task chapter), so it
needs nothing here.

### 14.6 Cancellation And `defer`

`f$cancel(frame)`:

1. If the frame is ACTIVE on the current poll stack, panic
   `suspension-reentrant-poll` with no state change.
2. If it is DONE or CANCELLED, return
   ([`req.check.cancel-idempotent`](../../spec/lang/11-requirements-and-suspension.md#r-req.check.cancel-idempotent)).
3. Mark it CANCELLED.
4. Cancel the child, if any, through the child's vtable. A child that
   waits on a host operation calls the `hd:rt` abort import for its handle
   ([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
5. Run the `defer` suites registered at the frame's state, last in, first
   out ([`req.cancel.defer`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.defer)).
   `f$cancel` is one `br_table` on the state; each case runs that state's
   suites over the saved locals. Suites cannot suspend, so this is plain
   synchronous code.

A frame that was never polled has no registered suite, so cancelling it
only marks it. Dropping a started frame without a cancel runs nothing, as
the spec says.

Implementation differs: steps 1 to 3 are the shared helper
`rt::Helper::CancelCheck`, and `f$poll`'s checks are
`rt::Helper::PollCheck`, so each frame's parts hold one call each.

**Shielded disposes.** A `using` binding's dispose suite holds a
suspension point ([`flow.using.cancel`](../../spec/lang/06-control-flow.md#r-flow.using.cancel)),
so a cancelled body can go Pending inside its exit ladder:

- The ladder of a scope with such a suite (`Em::waiting_scope`) skips
  the body and the entries that already ran when it resumes into a
  suite; each entry clears its flag before it runs. A suite is not a
  forbidden context.
- A point inside such a suite is shielded: it has no cancellation branch,
  so it keeps waiting on its child, the `dispose!`.
- `f$cancel` marks a started frame UNWINDING before it steps the body,
  and the body clears the flag when it leaves by `Cancel`, handing back
  its frame, still CANCELLED, instead of null. So a frame left Pending
  stays UNWINDING, and a later `f$cancel` of it resumes its body again
  instead of returning.
- A cancellation branch that cancels a child left UNWINDING suspends at
  its own state with that child, so its frame unwinds too, and each later
  cancel continues the child first. Once no child unwinds, it leaves by
  `Cancel` with `pc` 0, so the ladder runs as fresh code.
- After a waiting ladder, a cancelled frame leaves by `Cancel` whatever
  exit it took, so code after the scope never starts.

**One cleanup per job.** `Suspend[T].cancel!` is std hd: it awaits a
cleanup frame that the `task_cleanup_frame` intrinsic builds
(`rt::Helper::CleanupCold`), a `Suspend[void]` holding the task
([`req.cancel.wait`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.wait)):

- Its poll calls the task's `f$cancel`, which marks a cold or started
  task cancelled, continues an UNWINDING one, and does nothing to a
  finished one. It is Ready once the task no longer unwinds.
- The task's frame holds the one cleanup, so every waiter steps the same
  frame and none restarts it
  ([`req.cancel.shared`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.shared)).
  A cancel that reaches a frame disposing after an ordinary exit takes
  that dispose over the same way.
- Its cancel continues the task's cleanup too, and leaves the frame
  UNWINDING while the task unwinds. So a cancelled waiter's frame unwinds
  through it, as for any child
  ([`req.cancel.nested`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.nested)).
- Several parties may continue one cleanup: waiters, and a `race!` whose
  loser it is. A leaf registers its wake under the last poller only, so
  the others would never be woken. Both the cleanup frame's poll and
  `race!`'s loser cancels therefore run under no waker node
  (`rt::unwatched`), and a Pending poll restores `wregs`, so it counts as
  unregistered. An `all!` then polls such a party again on every poll.
- The cleanup frame is a `Suspend[bool]`: Ready with whether the task's
  frame carries `ABANDONED`, which `cancel!` turns into `CleanupEnd`.

**Abandoning.** `Suspend[T].abandon` is std hd over the `task_abandon`
intrinsic, which calls the task's `f$cancel` with the runtime global
`aband` raised
([`req.abandon.unwind`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.unwind)):

- `CancelCheck` marks every frame it lets go on `ABANDONED` (flag 32)
  while `aband` is set. That flag is the kept result every later
  `cancel!` reads
  ([`req.abandon.result`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.result)).
- A shielded point (§14.6 above) gets a block around its child step
  (`Em::shield_enter`, `Em::shield_leave`). Resumed into while `aband`
  is set, it cancels its waiting `dispose!` without stepping it, and the
  suite goes on as if the dispose returned. A fresh dispose that goes
  Pending while `aband` is set is cancelled the same way, so it runs to
  its first suspension point only. That cancel runs under `aband` too:
  the dispose's own registered suites run, and its pending host leaf
  aborts and closes nothing.
- Nothing else changes: a cancellation branch's child never stays
  UNWINDING under `aband`, so the cancel ends in one call, innermost
  first, in the ladders' order. `race!`'s and the cleanup frame's
  cancels reach their children through the same `f$cancel`s.
- **Carry.** Each `f$cancel` brackets its work with `CancelEnter` and
  `CancelLeave` over the global `abseen`: the `ABANDONED` bits that the
  cancels it made saw mark its frame, and it adds its own bit for its
  caller. So a cleanup that folds an abandoned one ends abandoned too
  ([`req.abandon.carry`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.carry)).
  A point's cancellation branch brackets its child cancels the same way,
  since an await of a cancelled suspension enters it from a poll, outside
  any cancel of the frame (below): a child whose cleanup already ended
  `ABANDONED` reports its bit when cancelled again, and marks the frame.
- **Waking.** Waiters on the ended cleanup registered no wake (above),
  and no host operation is left to wake them. An abandon that changed
  its task's flags sets the global `rewake`, and the entry's `hd.poll`
  polls the root again while it is set. `block_on` does the same before
  it waits in the host (§14.9), and sets `rewake` again on return when
  it consumed one, since the abandoned cleanup may have outer waiters.
- **A failed dependency**
  ([`req.abandon.dependency`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.dependency)).
  An await inside a `dispose!` of a cancelled suspension cancels the
  awaiting frame, as any such await does (below), so the `dispose!`
  ends CANCELLED or unwinds. A shielded point whose stepped `dispose!`
  is CANCELLED cancels it under `aband`, which cuts short what is left,
  marks its own frame `ABANDONED`, and goes on with the rest of the
  ladder. A `race!` inside the dispose that a live child wins before
  the cancelled one is polled never turns CANCELLED, so it does not kill
  the dispose.

**Awaiting a cancelled suspension**
([`req.cancel.await`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.await)).
A suspension that another task cancelled never completes, so its awaiter
is cancelled instead of panicking. No runtime global serves this; the
awaiter reads its child's `CANCELLED` bit:

- Every poll of a CANCELLED suspension answers Pending: `PollCheck`
  (generated frames, and `race!`'s and `cancel!`'s frames, which call it
  too) and a host stub's poll. A CANCELLED frame that still unwinds is
  continued first, under no waker node and counted as unregistered, so
  every party that polls one cleanup drives it.
- An await that gets Pending from a CANCELLED child, or for `all!` from
  any child polled, cancels its own frame (`Em::cancel_here`): it
  allocates the frame if it is still null, marks it CANCELLED and
  UNWINDING, and branches to the point's cancellation branch, which a
  resume for cancellation enters too. That branch cancels the child, so
  a child still unwinding folds into this frame's cleanup, then the
  ladders run and the body leaves by `Cancel` with its frame. Its own
  awaiter sees that frame CANCELLED, so the cancellation climbs one
  awaiter at a time: through `f$poll`'s Pending, and through a direct
  call's returned frame.
- `race!` checks each child that answers Pending: a CANCELLED one
  cancels the race at once. It cancels every child in list order and
  unwinds while one does.
- A pending host operation that is aborted gets no completion, and its
  awaiter may be another task than the canceller. `HostCancel` therefore
  calls `rt::Helper::WakeAborted`: it sets `rewake`, so `hd.poll` polls
  the root again, and wakes the handle's registration as a completion
  would, so each `all!` on the way polls the child that waited on it.
  Without `all!` node bookkeeping it only sets `rewake`.
- Drivers whose caller cannot be cancelled
  ([`req.check.driver-cancelled.await`](../../spec/lang/11-requirements-and-suspension.md#r-req.check.driver-cancelled.await)):
  `block_on` keeps polling a CANCELLED argument, which continues its
  cleanup, and returns once the argument no longer unwinds (§14.9).
  `hd.poll` steps a CANCELLED root through its `f$cancel` instead of its
  poll, and ends the run cancelled once the root no longer unwinds
  ([`req.entry.cancelled`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.cancelled)):
  it answers `-3` when the root's flags hold ABANDONED, else `-2`.

**A frame that never completes.** std's `timeout!` bounds its work's
cleanup from a `dispose!` of a `race!` child that must stay pending until
the race cancels it, which no hd code can build: every suspension
completes once a host operation or a child does, and a `ManualClock`
sleep never waits. The `task_pending_frame` intrinsic builds that child
(`rt::Helper::PendingCold`), a frame with no fields of its own:

- Its poll (`PendingPoll`) runs `PollCheck` and answers Pending with no
  wake registered, so its parent counts it as unregistered. A cancelled
  one answers Pending too, as every cancelled suspension does.
- Its cancel (`PendingCancel`) runs `CancelCheck`, which marks it
  CANCELLED, and `ABANDONED` under `aband`. It never unwinds.
- A root that waits only on it has no host operation, timer, or wake
  pending, so the entry reports a deadlock (§14.8).

### 14.7 Hook Points

A `Hook` instruction sits in TIR at each suspension point, resume, frame
creation, completion, cancellation, and host call start and finish. Normal
emission drops it, so it costs nothing in debug or release code.

Planned, not built: a trace, replay or simulation build sets a flag in the tier
part of `code_key` and emits each hook as a call to an `hd:hook` import
with the site and the frame. Hooks live in the cached TIR, so turning them
on re-emits code entries and never re-checks.

Every nondeterministic input already crosses a host import (time, random,
I/O) or a wake. A simulation host can therefore replay an instance by
feeding recorded host results and wake orders
([`req.determinism.replay`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.replay)).

### 14.8 Deadlock Detection

A deadlock is a root that is Pending while no host operation is pending,
no timer is set and no wake is queued. Nothing can ever wake it.

- **Both tiers** detect it in the host loop for free (§14.4), and fail
  the run with `panic: deadlock` instead of hanging.
- **The debug tier** adds a report. Each frame records the site of its
  current `Await` in its state, so the host walks the frame tree from the
  root and prints each waiting frame's function and source line. Planned,
  not built: the host prints no frames for a deadlock.

A `block_on` whose argument waits on a suspension of the outer driver
hangs by the spec's note. That is a deadlock of the same shape inside
`block_on`, and the same check reports it.

### 14.9 `block_on`

`block_on(s)` is std's `@intrinsic("block_on")`, an inline loop over one
runtime import:

```text
loop:
    poll s with a waker that sets a local flag
    if Ready: return .Completed(value)
    if s is CANCELLED and not UNWINDING: return .Abandoned if ABANDONED else .Cancelled
    if rewake: clear it, note it, continue
    if the flag is set: continue
    hd:rt/block()   ;; the host waits for >= 1 completion, writes the handles, returns n
    process the n wakes
```

- **Result.** The intrinsic builds the `BlockOn[T]` variant itself, by
  the enum's layout, so the drive allocates nothing more than the
  variant does.
- **Cancelled.** A poll of a CANCELLED suspension answers Pending and
  continues its cleanup (§14.6), so the same loop drives the cleanup to
  its end, waiting in the host while a dispose waits.
- **`rewake`.** An `abandon()` inside the drive may end a cleanup whose
  waiters registered no wake (§14.6, "Abandoning"); the loop polls again
  instead of waiting. A consumed `rewake` is set again on return, so an
  outer `hd.poll` polls its root again too.

- **wasmtime:** the import runs the reactor on the same thread until a
  completion arrives. The Wasm stack waits below the host call.
- **Browser:** crates/hd_run_wasmtime/DESIGN.md §17.6.
- **The indirect ban.** A global counter counts entered
  forbidden contexts: `defer` suites, `DefaultCall`s whose default body
  makes a call, and module initialization other than the entry's. A fact's
  getter is one too (facts are lazily initialized globals, crates/hd_mono/DESIGN.md
  §12.3), so an indirect call there panics, reported as
  `fact-evaluation-failed`. `block_on` and `println`
  read it and panic `suspension-forbidden-context` when it is not zero.
  The checker rejects the direct calls (crates/hd_tir/DESIGN.md §4.13.5).
  `block_on`'s read is the runtime helper `forbid_check`. Link makes it
  empty, with no counter and no panic message, when no code of the
  program references the counter, so a program with no forbidden
  context pays for neither. A session module always gets the full
  check, since a later input may add a forbidden context.

Implementation differs: `block_on` is a seeded `std.task` item that the
emitter lowers by name (`Em`, `"block_on"`), not std hd; not judged.

# New Compiler Design: Linear-Memory Layout

Status: design record for the linear-memory backend, which is being
built behind `--backend linear`; may lag the code. The code and the spec
win. The Wasm GC backend (§14, §15) stays the default until the linear
backend passes conformance, then it is deleted (owner decision).

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md). The
design it records is the owner-approved runtime study
([REFCOUNT_RUNTIME.md](../../docs/future-work/REFCOUNT_RUNTIME.md) §2,
§5, §6 and §10) and the object model it runs
([OBJECT_MODEL_TIER1.md](../../docs/future-work/OBJECT_MODEL_TIER1.md)
§3, §4, §6 and §9; tier 2's borrow flag in
[OBJECT_MODEL_TIER2.md](../../docs/future-work/OBJECT_MODEL_TIER2.md)
§6.4).

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
| this record | `crates/hd_wasm/DESIGN.md` §23 | 1 |
| the backend: layouts, emission from TIR, the module's link, the runtime pieces it generates | the crate `crates/hd_wasm_linear`, apart from `hd_wasm`, so it never edits the Wasm GC emission in `emit.rs`, `rt.rs` and `layout.rs` | 2, 3 and on |
| `layout.rs` | a type's linear layout: its Wasm values, its size and alignment in memory, its field offsets (§23.3) | 3, 6 |
| `emit.rs` | one instance's TIR body to one Wasm function | 3 and on |
| `rt.rs` | generated runtime pieces: the entry wrapper, panic stubs, the exchange-buffer copies, the wake table | 3 and on |
| `lib.rs` | `build`: the whole program's module from collection's output, with no per-instance code cache yet | 2, 3 |
| the switch | `hd_driver::Backend` in `Packages`, `--backend linear` on `hd build`, `hd run` and `hd FILE`, with its spec rules in spec/cli/command-line.md | 2 |
| conformance on both backends | `HD_CONFORMANCE_BACKEND=linear` runs every runtime case on the linear backend, judged by the root `KNOWN_FAILURES_LINEAR.tsv` (the columns of `KNOWN_FAILURES.tsv`, each row citing an issue) | 2 |
| memory intrinsics: load, store, grow, copy, fill | the privileged std module `std.rt.memory`: plain hd declarations whose bodies are `@intrinsic` | 4 |
| the allocator | hd in `std.rt.alloc`, over `std.rt.memory` | 5 |
| counts, release glue, the collector | hd in further `std.rt` modules, called by emitted code | 8, 19, 21 |

`std.rt` modules are privileged: only std modules may import them, so the
runtime can change its representation without breaking a program.

### 23.2 Memories And The Address Space

| Memory | Holds | Exported as |
| --- | --- | --- |
| 0 | the exchange buffer, exactly as on Wasm GC (crates/hd_run_wasmtime/DESIGN.md §17.3) | `hd.x` |
| 1 | the heap: static data, the page table, every counted object | not exported until the host codecs move (jobs 14, 15) |

- **Why two memories.** The exchange buffer grows from address 0 as the
  host's results need, so it cannot share a memory with a heap that must
  never move. A second memory (Wasm multi-memory, on by default in V8
  and wasmtime) keeps the host contract unchanged, so both hosts run both
  backends' modules until jobs 14 and 15 rewrite the codecs. A string
  crosses with one `memory.copy` from memory 1 to memory 0.
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
type in every position, as on Wasm GC (§15.1), except that a value is
inline in memory where Wasm GC stored a reference.

| hd value | In locals and results | In memory |
| --- | --- | --- |
| integers, `bool`, `char`, floats | the scalar | packed: 1 byte for `bool`, `i8`, `u8`; 2 for 16-bit integers; 4 or 8 otherwise, aligned to its size |
| struct, tuple | its fields' values, flattened, in declaration order (Wasm multi-value) | its fields inline, each at its alignment; the struct aligned to its widest field |
| enum | an `i32` tag, then the payload slots: variants' fields of one Wasm value type share a slot (§15.1's slot sharing, with no bound) | the tag (1, 2 or 4 bytes by variant count), then the largest variant's fields at their offsets; variants overlap |
| self-recursive enum (today's language) | the address of a counted cell holding the enum value | the address |
| `T?` of a reference `T` | the address; 0 is `.None` | 4 bytes |
| `T?` of another `T` | as an enum with one payload | as an enum |
| `string` | `(address, length)`: two `i32` | 8 bytes |
| `&T`, `&mut T` | the address of the `T`, possibly inside an object (§23.5) | 4 bytes |
| `data T` (today's language) | the address of a counted object holding the fields | 4 bytes |
| `List[T]`, `Map[K, V]` | the address of a counted buffer (§23.7) | 4 bytes |
| closure | `(code, environment)`: a function-table index and the address of a counted environment, 0 when nothing is captured | 8 bytes |
| `dyn Trait`, `Any` | `(data, vtable)`: the value's address and a static vtable's (§23.8) | 8 bytes |
| suspension | the address of a counted frame owned by its task | 4 bytes |
| `void`, `()`, `never` | none | 0 bytes |

- **Today's language first.** Until the tier 1 jobs change the language,
  a `data` value keeps its identity as a counted object, and a struct
  value, which Wasm GC rebuilt on every field write (§15.2, "Struct
  values"), is inline at last, so a field write is one store.
- **No bound.** Wasm GC boxed a value layout over 4 values (§15.1); in
  linear memory a wide value costs only its moves, as in Go and C#. A
  large struct passed by value is the price OBJECT_MODEL_TIER1.md §3
  accepts.
- **Type-only.** Every rule reads the type's definition and the
  definitions its fields name, never a use, so a laid-out instance is
  valid in every program that reaches it.

### 23.4 Counted Objects

Every counted object is one allocator slot (§23.6), and its header is at
the slot's start:

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | the count: 32-bit, non-atomic, with no overflow check on wasm32 (REFCOUNT_RUNTIME.md §4.4) |
| 4 | 4 | the descriptor word: bits 0 to 23 the type descriptor's number, bits 24 to 31 tier 2's borrow flag (a writer bit and a 7-bit reader count, OBJECT_MODEL_TIER2.md §6.4; zero in tier 1) |
| 8 | the payload's size | the payload: the fields, or a buffer's header and elements |
| after the payload, 4-aligned | 4 | the collector word, for a cycle-capable type only: the candidate flag, the color and the candidate-buffer index (REFCOUNT_RUNTIME.md §7.2) |

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
whose base and length are allocator globals. It starts at one page (16,384
entries, 1 GiB of heap) and moves to a larger run when the heap outgrows
it, so its cost follows the heap: 256 KiB for a full 4 GiB memory.

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
  (the free-list head, the bump offset, the live count, the class), then
  its slots. A fresh page is bump-allocated; freed slots go on the page's
  free list, popped and pushed in constant time; a page whose live count
  falls to zero returns to the page pool.
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
- **A collection buffer** is a counted object whose payload starts with
  its own header: the pin count (the second count of OBJECT_MODEL_TIER1.md
  §6.3), the capacity, then the length for a list; then its elements
  inline at their alignment. The object's count is the owner count, for
  copy-on-write: a write first checks it is 1 and copies the buffer
  otherwise (REFCOUNT_RUNTIME.md §9.1). A pinned buffer is copied when its
  list is copied, so it is never shared, and is freed when owners and pins
  are both zero.

| Buffer payload offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | pins |
| 4 | 4 | capacity, in elements |
| 8 | 4 | length (a list), or the entry count (a map) |
| 12 | 4 | reserved: 8-aligns the elements |
| 16 | capacity times the element size | the elements, inline |

Today a list is a shared object (OBJECT_MODEL_TIER1.md §6.2), so until
the tier 1 jobs make it a value, a list value is the buffer's address and
every copy shares it; growth that moves the buffer is job 9's to place.

### 23.8 Closures, `dyn` And Vtables

- **The function table.** Every function called indirectly (a vtable
  slot, a closure's code, release and trace glue) has an entry in one
  Wasm table, filled by an active element segment; a call is
  `call_indirect` through the entry's index.
- **A closure** is `(code, environment)`. The environment is a counted
  object holding the captures inline, absent (0) when nothing is captured;
  each closure literal's environment layout has its own descriptor, whose
  release and trace know the captures (OBJECT_MODEL_TIER1.md §9.3).
- **A `dyn Trait` value** is `(data, vtable)`. A vtable is a static
  record: its first two words are the concrete type's release and trace
  table indices, so dropping a `dyn` releases its data with no knowledge
  of the type, then the method slots in the trait's declared order, then
  each direct supertrait's vtable address. `Any`'s vtable also holds the
  type's identity (the first 64 bits of `H(canon(T))`, §15.2's `TypeId`).
- **Providers** are trait values, passed as `(data, vtable)` pairs in key
  order as on Wasm GC (crates/hd_mono/DESIGN.md §13.18).

### 23.9 Globals, The Entry Wrapper And The Host

- **Module storage** (top-level bindings) is one Wasm global per Wasm
  value of the binding's layout, set by its group's init function, as on
  Wasm GC (§15.4).
- **Runtime state** (the panic flag, the woken flag, the
  forbidden-context counter) are `i32` globals; the wake table and the
  candidate buffer are counted buffers in memory 1.
- **Exports and imports** are Wasm GC's: `hd.init`, `hd.poll`, `hd.wake`,
  `hd.x` (memory 0), `hd.cancel` for a scenario, and the `hd:rt`,
  `hd:prim` and capability imports of crates/hd_run_wasmtime/DESIGN.md
  §17.2, with the same signatures. A host therefore cannot tell the
  backends apart.
- **Panics** call a stub that writes the report through `hd:rt` and
  traps (§15.5); a message literal is copied straight from static data to
  the exchange buffer.
- **Sections.** `hd.lines`, `hd.positions` and `name` as on Wasm GC
  (§15.5, §15.9), so panic frames print the same way.

### 23.10 What The Jobs Build On This

| Job | Adds |
| --- | --- |
| 2 | the switch, an empty module that runs, the linear conformance mode and its known-failures list |
| 3 | both memories, the static data segment, globals, the entry wrapper, integer code and panics: hello and integer programs run |
| 4 | `std.rt.memory`'s intrinsics, with spec rules and hd tests on the linear backend |
| 5 | `std.rt.alloc`: size classes, pages, the page table, large runs, with hd tests |
| 6 to 15 | values inline, strings, counts and release glue, lists, maps, closures and `dyn`, references, frames, the host codecs, each removing rows from `KNOWN_FAILURES_LINEAR.tsv` |
