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
| a compiler-supplied data type, such as `std.core.Ref[T]` | `(ref $T)`: a struct with one mutable field per declared field, in declaration order | same |
| `struct T` | `(ref $T)`: the same struct layout, which nothing writes after `struct.new` ("Struct values" below) | same |
| `&T`, `&mut T` | `(ref $Ref_T)`: the data type `std.core.Ref[T]`, two closures that read and replace the place, and the place's identity ("References" below) | same |
| embedded field | a field of the outer struct holding the embedded struct value, as a named struct field does | same |
| payloadless enum | `i32` tag | `i8` or `i16` when the variant count fits |
| enum with payloads, within the bound | `multi`: `(i32 tag, shared payload slots...)` (§15.1) | the same fields; a structure of arrays in an array |
| enum box, **flat** (over the bound, at most 4 payload fields in total) | one immutable struct: tag plus the union of payload fields, unused fields null or zero | same: the box reference |
| enum box, **subtypes** (over the bound with more fields, or self-recursive) | a non-final base struct (tag, shared fields) and one final subtype per payload variant; payloadless variants are constant singletons | same: the box reference |
| `T?`, `T` a non-nullable reference (a struct, list, string, closure, a box) | `(ref null $T)`; null is `.None` | same |
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
| `List[T]` | `(ref $List_T)` = struct `{len: mut i32, data: mut (ref $Arr_T), cow: mut i32, pin: mut i32}`; under A1 every reference `T` shares `$List_eq` over `(array (mut eqref))`. `cow` is the copy-on-write word ("Collection values" below), `pin` the count of element references ("Element references" below) | same |
| `Map[K, V]` | a compact insertion-ordered table of `index`, `hashes`, `keys` and `values` arrays and a `cow` word (`layout::map_ty`, [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#mapk-v)). Its operations are runtime helpers (`map.rs`); std hd over arrays is not built yet; see #180 | same |
| `TypeId` | `(ref $TypeId)`, a constant global per type: `{id: i64, name: (ref $LitFn)}`, `id` the first 64 bits of `H(canon(T))`. Implementation differs: `TypeId` is std's `struct TypeId {key, name}`, built with `struct.new` at each use and compared by its `key` bytes; that allocates per use and compares strings, not one `i64` (see #175) | same |
| `mut Suspend[T]` | `(ref $Suspend_L)` (§14.1) | same |

**Struct values: immutable objects, rebuilt on a field write.** A
`struct` value has no identity and is copied on every binding, store and
call ([Value Types](../../spec/lang/04-type-system.md#value-types)). On
Wasm GC it is emulated, not laid out inline: a struct value is a GC
struct object with one field per declared field, and the checker never
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
  iteration. A read costs one `struct.get` per step. Correct first:
  inline layout and in-place
  writes come with the linear-memory backend.

**Collection values: shared arrays, a mark, a copy before the change.**
A `List` or `Map` value is a value with copy-on-write
([Collection Values](../../spec/lang/04-type-system.md#collection-values)).
On Wasm GC there is no count to tell whether an array is shared, so a
header's `cow` word stands in for it (`crates/hd_wasm/src/emit/cow.rs`):

- **A copy marks.** The TIR `ValueCopy` (placed by the checker,
  `crates/hd_check/src/coll.rs`, at a binding, an assignment, a return,
  a `mut` parameter's entry and the value arguments below) makes a new header over the same arrays
  and sets bit 0 of both headers' `cow`. The emitter does the same for
  an operand stored into a slot that holds a collection: a list or map
  literal's element, a struct literal's field, a payload, a tuple element,
  `push`, an element store and a map store, unless the operand is a value
  its own instruction made (a literal, a call). A struct holding a
  collection copies by rebuilding its object with each such field
  copied, since a field change writes the collection's header in place.
- **A change copies.** `push`, an element store, `ListIndexMut`, a map
  store, `remove` and `MapIndexMut` first test `cow`: when it is not 0
  they copy every array of the header (a `push` with room to grow), copy
  each element that holds a collection in turn, and clear `cow`. The
  header itself never moves, so a nested write needs no store back into
  its place.
- **A path makes each collection on it its own.** `m[k].push(x)` reads
  `m[k]` with `MapIndexMut`, and `xs[i].x = v` reads and stores `xs[i]`
  with `ListIndexMut` and `ListSet`, so each list or map on the path is
  made unique before its element is read; nothing is read out and stored
  back.
- **A loop adds 2.** `SnapBegin` makes the header the loop iterates over
  the collection's arrays and adds 2 to the collection's `cow`; a change
  during the loop therefore copies first, and the loop reads the old
  arrays. `SnapEnd` takes the 2 off again when the collection still has
  those arrays.

What is eager, and why:

- **The mark never clears.** With no count, a header copied once stays
  marked, so its next change copies its arrays even when the other copy
  is gone: `let mut a = xs.filter(f)` then `a.push(x)` copies once. A
  function's own `let mut` local returned is moved, not copied.
- **A loop left by `return` or `?`** keeps its 2, so the collection's
  next change copies once; a `break` and the normal end take it off.
- **A struct copy rebuilds** its object when the struct holds a
  collection (one allocation per struct and per collection on the copy),
  where a plain struct copy is a reference move.

**Value arguments: copied only where an alias can change the source.**
A value argument or receiver is the callee's own copy
([`types.coll.value`](../../spec/lang/04-type-system.md#r-types.coll.value)).
Passing the header as is is that copy as long as nothing changes the
caller's collection in place while the callee holds it, so the checker
(`crates/hd_check/src/coll.rs`, `value_arg`, `value_receiver`,
`capture_copies`, `loop_binding_copy`) places a `ValueCopy` only where something can:

- **During the call.** An argument read from a place that something
  besides the caller may change is copied at the call: a writable local
  that a closure changes in place, or that a closure or `&mut` assigns
  (`CAPTURED_ASSIGNED`); a mutable global; a value read through a
  reference or from a field of an object that is not a struct. An
  argument read from a `:=` or plain `let` binding, a value parameter, a
  tuple element or a payload is passed as is: nothing changes those in
  place, and a parameter's caller already copied an aliased source. A
  `&mut List[T]` parameter refers to the caller's place and copies
  nothing. The test runs
  after all of the call's arguments, so a callback passed in the same
  call counts.
- **Later in a loop or a closure.** A closure or `&mut` made after the
  call can still run during it when the call sits in a loop or a
  closure. There the argument takes a reserved instruction, filled when
  the body is checked with the copy, or with an `Identity` coercion when
  the local stayed unaliased (a few bytes and no allocation). In
  straight-line code the call passes the header as is.
- **After the call.** A closure that captures a value parameter, or a
  binding that took one as is, makes the local its own copy just before
  the closure is made (a `LocalSet` of its `ValueCopy`), so the caller's
  later change copies first. A `for` binding that a closure in the body
  captures is bound to a copy of the element (a map entry is rebuilt
  with its key and value copied).

Each copy marks the caller's collection, and the mark never clears, so
its next change copies its arrays once. An unaliased local passed in a
loop that also changes it copies nothing.

Not closed: a list or map receiver of a `self` method is passed as is;
a struct receiver is copied as an argument is. The cursor `xs.iter()`
keeps the header it is given, so it still panics on a length change
([Iterator Invalidation](../../spec/lang/06-control-flow.md#iterator-invalidation)).
`xs.view(a, b)` takes `&self`, so a view refers to the list's place
([`std-collections.view.place`](../../spec/std/collections.md#r-std-collections.view.place)).
An argument whose later sibling changes the same list in place
(`f(xs, xs.pop())`) is passed as the sibling left it. The reference-counting
backend (OBJECT_MODEL_TIER1.md job 11, REFCOUNT_RUNTIME.md) replaces
the mark with the count.

**Iterators.** `Iterator[T]` is a struct holding its `step` closure, and
every iterator is used behind `&mut`
([`flow.for.iterator-handle`](../../spec/lang/06-control-flow.md#r-flow.for.iterator-handle)).
`ListIter` and `MapIter` build the struct; the checker wraps it in a new
object (`refs.rs` `new_object_ref`), so `iter()` returns a
`Ref[Iterator[T]]` whose `next` reads the struct through `get`. A `for`
over an iterator keeps that reference in its `$iter` local.

**Closure captures: copies and cells.** A closure's environment is a
struct subtype with one field per captured local
([`fn.capture.copy`](../../spec/lang/07-functions.md#r-fn.capture.copy),
[`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)).
The checker marks a local `Shared` when some side assigns or changes it
after the declaration (`CAPTURED_ASSIGNED` or `MUTATED`), and `Copy`
otherwise. A `Copy` capture stores the value in the environment. A
`Shared` one lives in a cell, a struct holding the local's components,
and the environment holds the cell.

- **Erased methods.** A generic trait method's shared cells use its
  erased layout in the root, construction thunks and concrete closures.
  An open value occupies one `eqref` field; concrete reads unbox it and
  writes box it. A mutable cell cannot be cast to a different concrete
  struct type. Captures pass the initialized cell as a non-null reference,
  narrowing the nullable local at the thunk call.
- **Fresh per declaration.** The cell is made by the local's declaring
  `LocalSet`, which `cell_locals` picks as the lowest-numbered `LocalSet`
  of the local outside every closure that captures it. Any other store
  writes the existing cell. A `LocalSet` inside a capturing closure is an
  assignment through the capture, never the declaration: a `for` or
  `match` pattern's bindings are set after the loop body is built, so a
  closure in the body that assigns the variable has the lowest index, and
  taking it as the declaration made one cell for the whole loop (and, in
  the closure's own function, a private cell that no caller saw). With the
  declaration found, each iteration makes a new cell
  (`flow.for.binding.fresh`).
- **Copies of a closure** are copies of the environment reference, so
  they share every cell (`fn.capture.copies`).

**References: a pair of closures.** `&T` and `&mut T`
([Values And References](../../spec/lang/13-values-and-references.md))
are emulated, not laid out as addresses: a reference is an object of the
compiler-supplied data type `std.core.Ref[T]` (`hd_resolve::seed`), whose
first two fields are closures, `get: fn() -> T` and `set: fn(T) -> void`,
that read and replace the referenced place, and whose last three are the
place's identity, `base: dyn Any`, `serial: i64` and `key: string`. The checker builds both when it
checks `&e` (`crates/hd_check/src/refs.rs`) and lowers
`&mut T` in a finished body to `Ref[Mut[T]]` (an internal permission
marker) and `&T` to `Ref[T]` (`hd_types::Types::lower_refs`,
`hd_tir::ir::map_body_types`); the driver lowers the declarations the
backend reads (`params`, `ret`, `data_fields`, enum payloads, impl heads,
row keys), and `hd_structure` its generated bodies. So the backend sees an
ordinary generic data type, and the emitter is unchanged but for one line.
The TIR names the place itself (`RefPlace`, `RefRead`, `RefWrite`,
`RefDyn`, `RefIdentity`, `crates/hd_tir/DESIGN.md` §4.13.11,
"References"), and each such instruction owns the block that builds or
calls the closures below; this backend emits that block as the
instruction's value, while the linear backend lowers the place to its
address (§23.20).
The GC layout normalizes `Ref[Mut[T]]` to `Ref[T]` before discovering
recursive nominal edges and forming recursion groups. Their storage and
closure signatures are identical; keeping distinct graph nodes can create
incompatible recursive heap types on Wasmtime. This normalization is only
for physical layout: type identity and implementation selection retain the
permission marker. Boundary codec lookup also keeps its semantic,
permission-carrying type; only the physical layout entry points and
recursion edges normalize storage.

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
  `r`'s own. A field of a compiler-supplied data object, `&mut d.n`,
  uses `struct.get` and `FieldSet` on that object, evaluated once.
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
  | a field of a compiler-supplied data object | the object | 0 | `f` and the field index, then the field path |
  | a place reached through `r` | `r.base` | `r.serial` | `r.key` and the field path |

  A serial comes from the `ref_serial` intrinsic, a runtime counter
  global (`rt::ref_serial_global`), so serials are unique within a run.
  A local draws its serial where it is declared: its first store, or a
  parameter's entry to the body. So each execution of the declaration
  is a new place, as each makes a new cell. Only a local whose name some
  `&` in the file names draws one (`BodyCx::ref_names`, a scan of the
  file's syntax for `&name` and `&name.field`); the rest pay nothing.
  `RefRoot` is a private fieldless data type, so its one value is a
  constant global (`canonical_data`), and no `base` but a data object's
  allocates.
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
- **Matching through a reference.** A pattern that tests a value of type
  `&T` ([Matching Through A Reference](../../spec/lang/13-values-and-references.md#matching-through-a-reference))
  is lowered in the checker's decision tree (`crates/hd_check/src/pat.rs`
  `tree`): before the first test of that column it emits the same
  `Field(r, get)` and `CallValue`, once per path, and the tag switch,
  payload reads and literal compares then work on the value as for any
  subject. A name bound to the column keeps the reference itself. So the
  backend sees ordinary TIR and is unchanged.
- **Why not a cell per reference.** A reference to a field of a struct
  value has nothing to point at: struct values are immutable objects
  shared by every place that copied them. A pair of closures handles a
  local, a field path, a value behind another reference and a data field
  with one layout, so `&mut T` to `&T` is free (both have the same physical layout),
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
  `Display` for reference types. The backend's selection (`hd_mono` `select`)
  raises a lowered self type back to `&T` or `&mut T`
  (`hd_types::Types::raise_refs`), preserving permission when it matches impl
  heads. Type identity also retains that permission. Monomorphization shares
  permission-insensitive bodies: generic calls, erasure, trait selection and
  identity operations conservatively retain the distinction where observable.
  Their physical `Ref` layout remains the same.
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
- **Trait value references.** `&dyn Tr` and `&mut dyn Tr`
  ([Trait Value References](../../spec/lang/09-traits.md#trait-value-references))
  lower as every reference does, to `Ref[dyn Tr]`, so generic code over
  `&T` and std's `&T` impls (`Display`, `Eq`, ...) see an ordinary
  reference. What differs is how the checker builds one
  (`crates/hd_check/src/dynref.rs`): converting `&S` builds a new `Ref[dyn Tr]`
  whose `get` closure converts the captured source reference to the trait
  value with `Coerce` of kind `RefToTraitValue`, whose `set` is
  `unreachable` (a whole value is never stored through one,
  `trait.dyn.ref.no-store`), and whose identity fields are copied from the
  source reference, so `is` compares the source places. Widening to a
  supertrait value reference wraps the same way with `Supertrait`
  applied to what the source refers to.
  - **The payload is the reference.** A `RefToTraitValue` boxes the
    `Ref[S]` itself as the trait value's payload; collection (`hd_mono`
    `vtable` with `through`) builds the vtable at `S`'s implementation and
    records per slot how its adapter passes the payload
    (`hd_mono::SlotRecv`): a method that takes `&self` or `&mut self`
    gets the `Ref[S]` unchanged
    (`Payload`), so its writes reach the caller's place; a method that
    takes the value gets what `get` returns (`ReadThrough`): the adapter
    (`Helper::Adapter` with a `RefRead`) calls the `get` closure, as
    `CallValue` does, casting the code on the `Wasmtime` profile. A sealed
    trait's slots (`Any`, `Inspectable`) stay at the reference type itself,
    so `runtime_type` reports `&S` and a downcast to `&S` recovers the
    payload.
  - **Reference-only surface.** Written bare trait values are rejected.
    All reachable reference-receiver slots receive a reference payload;
    no unavailable trap slots are emitted. Internal `dyn` descriptors
    remain in reference getters and vtables, never as user value types.
  - **Cost.** A conversion allocates the `Ref[dyn Tr]` object and its
    `get` closure (the `set` is a constant), on top of the source
    reference; a call through one calls `get`, then the slot, and a
    `self` method's adapter calls the source's `get` again. Correct first:
    on the linear-memory backend a trait value reference is the address
    and the vtable, two words, as Go's interface value is.
- **Providers.** Every provider slot stores its `Ref[dyn K]` as the
  payload beside its getter's vtable. `RefAsProvider` installs it, and
  `ProviderAsRef` reads it back. Two `$.use(K)` calls share the provider's
  place. Value providers of traits without reference receivers are first
  copied into new objects; explicit reference providers share their place.
- **Host providers.** The driver roots `std.rt.host_provider` for every
  entry-row key. The host creates its trait descriptor and wraps it in a
  reference to a new object, using the same provider layout for all keys.
  A program links only the host methods it calls: collection records each
  trait method that scanned code calls through a trait value
  (`hd_mono::Collected::dyn_methods`), and a provider's or host handle's
  slot for any other method is a trap stub with no import, on both
  backends. A session build that reaches an earlier module's instances
  did not scan them, so it lowers every slot (`Collected::calls_dyn`).
- **Host handles behind references.** A host method that returns a
  handle as a trait value reference, as `Net.connect!` returns
  `&mut dyn TcpStream` (`std-net.handles.ref`), crosses as a
  `boundary::BTy::Handle` whose `HandleB::wrap` names the
  `std.rt.host_provider` instance at `dyn H`: the decoder builds the
  handle's trait value from its number, the payload beside its vtable of
  handle slots, and calls that instance, whose `&mut held` puts it behind
  a new object. A call through the reference reads the trait value back
  with `get` and passes its payload, so each slot still finds the handle
  number in its receiver, whatever the method's receiver form, and every
  copy of the reference reaches the one socket. Collection selects the
  instance while it walks the entry rows' provider signatures for their
  boundary codecs (`hd_mono::BoundaryCodecs::handle_ref`, nested handles
  included, as `TcpListener.accept!` returns a stream), and the entry's
  layout reads it from that map (`Lay::entry`). The host's imports and
  the handle numbers are unchanged, so host/ and hd_run_wasmtime need
  nothing new.

**Element references: a reference into the header, a pin count.**
`&xs[i]` and `&mut xs[i]`
([Element References](../../spec/lang/13-values-and-references.md#element-references))
are references as above whose place is a slot of a list: the checker
(`crates/hd_check/src/pins.rs`, `refs.rs` `Root::Elem`) emits `ListPin`
on the list and keeps the header it returns and the index in two locals
that the `get` and `set` closures capture; `get` is `ListIndex` of them
and `set` `ListSet`, and a struct field path through the element is
rebuilt as for any place. A list header gains a `pin` word after `cow`
(`layout::list_ty`, `list_pin`; `crates/hd_wasm/src/emit/cow.rs`):

- **Pinning.** `ListPin` checks the index (`index-out-of-bounds`), makes
  the arrays the header's own as an element store does (`list_reserve`),
  adds 1 to `pin`, and returns the header. `ListUnpin` takes the 1 off.
- **The panic.** `ListPush` and std's `list_truncate`, the two length
  primitives every list mutator in std is written over, test `pin` first
  and panic `collection-pinned` when it is not 0 (a truncation only when
  it changes the length, so `clear` of an empty list passes).
  `list_truncate` takes `&mut List[T]`: it calls the reference's `get`,
  which returns the place's own header, and truncates that header. `insert`
  and `remove_at` move elements with element stores, which a pin allows,
  so `remove_at` panics in its final truncation after moving them; a
  panic ends the instance, so the moved elements are never seen.
- **A copy marks; no eager copy.** A reference holds the header, not the
  arrays, and the header never moves (a change copies arrays into it).
  So a copy of a pinned list still shares its arrays under the `cow`
  mark: the first change on either side copies that side's arrays, and
  a reference reaches the header's current arrays. That is the
  observable result of `ref.elem.copy`, with no copy at the copy site;
  `xs = []` stores a new header in the place, and the references keep
  the old one (`ref.elem.replace`). The reference-counting backend, where
  a reference is an address into the buffer, copies a pinned buffer at
  once instead.
- **Release at last use.** The checker releases a pin at statement
  granularity in the suite that took it (`ref.elem.release.binding`,
  `.statement`): after the statement, or after the last statement of the
  suite that names the reference's binding, it emits `ListUnpin` guarded
  by a flag local that `ListPin`'s statement sets. A suite that takes a
  pin is a cleanup `Scope`, and each pin registers a `defer` suite that
  releases it when still held, so `return`, `break`, `continue` and `?`
  release it too (`ref.elem.release.exit`). `for p in &mut xs` pins once
  for the loop as a statement of its suite; each step's reference
  captures the step's index.

Not closed: a copy of the reference that outlives the binding's
statements (stored in a field or a list, returned, captured by a closure
called later; `ref.elem.release.copies`) holds no pin of its own here, so
the list is unpinned at the binding's last use and a later push does not
panic. A reference the checker finds no suite for (an expression-bodied
closure, a top-level binding, a default) is released at once. Both write
through the header, so the reference still reaches its list's slot; only
the panic is missed. The reference-counting backend counts each copy.
Cost: a list header is one `i32` larger; each `push` and truncation
tests `pin` (about 15 bytes per site, and one panic stub of about 100
bytes in a program that pushes); taking an element reference costs a
reference's closures plus `ListPin`, and a suite that takes one runs its
`defer` flags.

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
collections, functions, trait values and references (`Ref[T]`) end a
path. The checker rejects an enum that contains itself by value
([`data.enum.no-recursion`](../../spec/lang/08-data-and-enums.md#r-data.enum.no-recursion)),
so a recursive enum writes `&` and a checked program reaches the boxing
below only through an unresolved projection; it stays for that case.
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
included. A trait value reference instead compares its place. See
[`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand),
[`ref.expr.allocate`](../../spec/lang/13-values-and-references.md#r-ref.expr.allocate)
and
[`ref.identity.same-place`](../../spec/lang/13-values-and-references.md#r-ref.identity.same-place).
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
([`types.sealed.anyval-members`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-members)).
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
| internal trait descriptors | no user-visible identity operation |

An enum, optional, `Result`, tuple, primitive, string or function operand
has no row: `is` on it is the compile error
`identity-requires-references` (S1c,
[`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)).
Internal trait descriptors are carried only by reference getters and vtables.

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
| `rt.rs` | generated runtime pieces: the entry wrapper, panic stubs, the exchange-buffer copies, the host provider stubs, object headers and release glue, and the bump allocation that host frames still use; the wake table later | 3 and on |
| `lib.rs` | `build`: the whole program's module from collection's output, with no per-instance code cache yet | 2, 3 |
| the switch | `hd_driver::Backend` in `Packages`, `--backend linear` on `hd build`, `hd run` and `hd FILE`, with its spec rules in spec/cli/command-line.md | 2 |
| conformance on both backends | the driver tier runs each runtime case of the root `LINEAR_PASSES.tsv` (`path`, `note` citing an issue) once more on the linear backend, and each must pass; `HD_CONFORMANCE_BACKEND=linear` sweeps every runtime case and prints the passes not listed yet | 2 |
| memory intrinsics: load, store, grow, copy, fill | the privileged std module `std.rt.memory`: plain hd declarations whose bodies are `@intrinsic` | 4 |
| the allocator | hd in `std.rt.alloc`, over `std.rt.memory` | 5 |
| counts | `retain` and `release` in `std.rt.alloc`, called by emitted code, with release glue that `rt.rs` generates per layout | 8 |
| lists | `emit/list.rs` for the list kernels, and `rt.rs` for the header, the buffer, its release glue and the change that makes a buffer a list's own (§23.17) | 9 |
| maps | `emit/map.rs` for the map kernels, and `map.rs` for the table: its probe, put, growth, release glue and the change that makes a table a map's own (§23.18) | 10 |
| closures and `dyn` | `emit/closure.rs` for environments, cells and calls of function values, `emit/dyn_value.rs` for trait values and their vtables, `emit/iter.rs` for collection iterators (§23.19) | 11 |
| references, cleanup scopes, releases at last use | `emit/refs.rs` for references as addresses, `emit/live.rs` for each value's and local's last use, `emit/scope.rs` for the `defer` exit ladder, `emit/list.rs` for element references' pins (§23.20) | 12 |
| host codecs | `boundary.rs`: each boundary type's encoder and decoder over the exchange buffer, and the host method slots that call them (§23.23) | 14 |
| checked mode | `std.rt.checked`, the counts and the allocator with their checks, and `rt.rs`'s audit at an entry's end, under `--backend linear-checked` (§23.24) | 16 |
| iterative release | hd in a further `std.rt` module, called by emitted code | 19 |
| the cycle collector | hd in `std.rt.cycle`, called by emitted code, which runs each object's release glue as its trace (§23.28) | 21 |

`std.rt` modules are privileged: only std modules may import them, so the
runtime can change its representation without breaking a program.

### 23.2 Memories And The Address Space

| Memory | Holds | Exported as |
| --- | --- | --- |
| 0 | the exchange buffer, exactly as on Wasm GC (crates/hd_run_wasmtime/DESIGN.md §17.3) | `hd.x` |
| 1 | the heap: static data, the page table, every counted object | not exported: the host reads and writes only memory 0 (§23.23) |

- **Why two memories.** The exchange buffer grows from address 0 as the
  host's results need, so it cannot share a memory with a heap that must
  never move. A second memory (Wasm multi-memory, on by default in V8
  and wasmtime) keeps the host contract unchanged, so both hosts run both
  backends' modules: the module's own codecs move values between its
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
type in every position, as on Wasm GC (§15.1), except that a value is
inline in memory where Wasm GC stored a reference.

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

- **Struct values inline.** A struct value, which Wasm GC rebuilds on
  every field write (§15.2, "Struct values"), is inline here, so a field
  write is one store. A program declares no `data` type; only a
  compiler-supplied data type is still a counted object with identity.
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
| the block's last 4 bytes | 4 | the collector word, for a cycle-capable type only: the color, the opaque flag and the candidate-buffer index (REFCOUNT_RUNTIME.md §7.2, §23.28) |

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
whose base and length are allocator globals, after 128 bytes that hold
each class's list of pages with room. It starts at one page (16,352
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
reach every holder of that value, as the Wasm GC backend's header does,
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
- **Integer code** is the Wasm GC lowering of §13.15 of
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
  allows overlap, and `memory.fill`. The Wasm GC backend has no heap and
  lowers none of them: a build that reaches one fails as `unsupported`,
  and only std can reach one.
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
- **Division.** `block_of` divides by the class size; the reciprocal
  table of §23.5 is a later speed-up.
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
  for now. The table move past 16,352 pages, almost 1 GiB of heap, has no
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
  unbounded argument whose Wasm GC layout is one reference, under the
  class `REF` (A1, crates/hd_mono/DESIGN.md §13.2). Here that class is
  one address, which is right for data values and boxed enums. But Wasm
  GC also lays out a struct, and an enum or tuple over 4 values, as one
  reference, and here they are inline, so a call that would pass one to
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
  parsing and the snapshot hook are `hd:prim` imports, as on the Wasm GC
  backend. A string argument goes into the exchange buffer at 0, its
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
`rt.rs`. Each change keeps the Wasm GC backend's semantics (§15.2,
"Collection values" and "Element references"), with the buffer's count
in place of the `cow` mark.

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
  instruction made it, as the Wasm GC backend decides. Such a copy's own
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
  The web compiler leaves this backend out (hd_driver's default feature
  `linear`), since the playground links with the Wasm GC backend only.
- **Not yet.** List iterators (`iter()`) hold a closure, and std's
  shrinking primitive `list_truncate`, so `pop`, `insert`, `remove_at`
  and `clear`, takes `&mut List[T]`: they wait for closures (job 11) and
  references (job 12), as do `assert_equal` and `debug`, which write
  through `&mut DebugWriter`. Job 11 lowers all of them (§23.19).

### 23.18 What Job 10 Adds

Job 10 lowers maps (§23.7), in `emit/map.rs` with their glue in
`map.rs`. Each operation keeps the Wasm GC backend's semantics (§15.2,
"Collection values"; `crates/hd_mono/DESIGN.md` §13.12), with the
table's count in place of the `cow` mark.

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
- **Iterators.** `List.iter()` and `Map.iter()` build an `Iterator[T]`
  whose step closure's environment holds the collection's header, with a
  count of its own, the next position and what the collection looked
  like at creation: `rt:list_step` and `rt:map_step` panic
  `iterator-invalidated` when it changed (`flow.for.invalidate.panic`).
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
  owning the block of its closure form for the Wasm GC backend (§15.2,
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
  that a loop writing through `&mut` to a local allocates nothing. The
  Wasm GC backend's hello stays 4,164 bytes.
- **Cleanup scopes.** A `Scope` with `defer` suites is the Wasm GC
  backend's exit ladder (`emit/scope.rs`, crates/hd_mono/DESIGN.md
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
  that is one counted address is that address, never a copy, as the
  Wasm GC backend erases it.
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
  dev profile and 8,824 in release (4,151 on the Wasm GC backend, from
  3,161). Since references are addresses, the result is the host trait
  value itself (a reference to a trait value is that trait value), and
  `host_provider` allocates nothing.
- **Not yet.** Issue #556's three escape cases pass here and stay known
  failures on the Wasm GC backend, whose references hold no count. The
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
- **The parts.** As on the Wasm GC backend, the instance's own function
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
- **`all!` and `race!`.** `all!` keeps a done mask and each child's
  result in the save area. Each poll polls every unfinished child.
  There is no waker node: a host leaf answers from the wake table, so
  skipping a child whose wake bit is clear would save only a poll.
  `race!` is §14.5's frame over the list (`task::race_*`). The results of
  done children are released by the cancellation branch and by the
  release glue.
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
  and 9,568 in release; 6,010 in both after. The Wasm GC backend's hello
  is 4,151. `println("hello")` is 5,988 bytes here and 3,969 there.
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
  §17.4 encoding. This backend generates the encoders and decoders the
  Wasm GC backend's `hd_wasm::boundary` generates, over its own layouts,
  so `host/*.mjs` and the wasmtime host need no backend switch, and the
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
  after, 4,175 on the Wasm GC backend. A program links only the codecs
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
its own counting bugs. `--backend linear-checked` selects it
(`hd_driver::Backend::LinearChecked`), and a normal build is unchanged.

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
- **The sweep.** `HD_CONFORMANCE_BACKEND=linear-checked` builds every
  linear run of the conformance runner in checked mode, so each case
  `LINEAR_PASSES.tsv` lists must pass with its counts audited.
  `crates/hd_driver/tests/std_rt/checked_tests.hd` tests the runtime's
  poison, its reuse rule and its counts.
- **Size.** hello (`println(42)`, dev): 5,463 bytes on `linear` before
  and after, 4,738 on `wasm-gc`, and 7,297 on `linear-checked`.
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
  the call. `HD_CONFORMANCE_BACKEND=linear-checked` passes every
  `LINEAR_PASSES.tsv` case but the eight cycles that wait for the
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
  5,418 after; 4,734 on `wasm-gc`, unchanged; 7,350 on `linear-checked`,
  whose count totals cost 132 bytes. Programs shrink by about 1 to 10 per
  cent, as most entry retains and many copies go.
- **Not yet.** Reads of immutable fields, loop elements, and a local read
  that is not its last still take a count and give it back (job 18).
  A dynamic call lends its arguments, so its slot counts what the target
  owns.
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
  content hash, as on Wasm GC) and a provider, the trait value's data and
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
linear backend (`emit/erased.rs`), with the Wasm GC backend's ABI
(crates/hd_mono/DESIGN.md §13.5.1): one erased body per implementation,
a witness per call's type arguments, and one thunk per open instruction
at the caller's types. Collection is shared, so both backends build the
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
  or an implementation keeps its erased view in every instance, as on
  Wasm GC: the erased body's thunks build the closures, and they are the
  concrete instances' own. So a concrete instance's cell holds an open
  value: a read unboxes it, a write boxes the new value.
- **References.** A reference whose target is open is the address of an
  open value. When the concrete target is one counted address, its place
  holds the same word, so the reference crosses as it is. Any other
  target is copied around the call it is an argument of (issue #617).
  In a thunk, `&mut x` of an open `x` passed to a helper at `T = i32`
  becomes a reference to a new box holding a copy of `x`'s box, which
  replaces it in `x`'s place once the call returns: other open values may
  share the old box, so the helper must not write into it. A `&` names
  the box itself. A dynamic call passing a `&mut i32` to a `&mut T`
  parameter passes a new word holding a box of a copy of the value, and
  after the call stores the value of the box the word then holds back
  into the caller's place. A reference that outlives its call (a result,
  a capture, a field, or an argument of a cold call) has no point to
  write back at and is refused at link.
- **Kept suspensions.** A plain call of a suspending generic method
  through a trait value keeps the erased frame, whose poll answers an
  open value, as a `Suspend[T]` that is polled where only `T` is known:
  by `!`, `race!`, `block_on` or a callee. So unless a `T` value is its
  open value, the caller wraps it in a frame of its own
  (`Helper::OpenFrame`, issue #617): `cancel!`'s cleanup frame with a
  poll that polls the child and, once it is Ready, unboxes its result
  with counts of its own and releases the box. Its cancel cancels the
  child and unwinds while the child does, as the cleanup frame's does,
  and its glue releases the child. The Wasm GC backend wraps every such
  frame the same way (`Helper::OpenCold`, `OpenPoll`), since a frame's
  struct type there carries its result layout.
- **Rows.** A method's own row parameters are open too, so a method with
  only row parameters is erased the same way, and a call at its open row
  is a thunk at the caller's row. A dynamic call reads the keys of its
  rows from its arguments as the instance has them, so a thunk's row,
  which is the caller's own, lists its keys.
- **Checked by** the five conformance cases of the issue, on `linear` and
  `linear-checked`, whose count audit covers every box and cell, and by
  `erased-method-boxed-reference-argument` and
  `dyn-generic-suspension-kept` (issue #617), which run on Wasm GC too.

### 23.28 The Cycle Collector

Issue #618 (job 21) reclaims reference cycles on the linear backend by
synchronous trial deletion (Bacon and Rajan, ECOOP 2001), as
docs/future-work/REFCOUNT_RUNTIME.md §7 plans. The collector is hd in
`std.rt.cycle` (`lib/std/rt/cycle.hd`); the backend adds what only it can
build (`rt.rs`'s `Helper::Collect`, `CycleTrace`, `CycleRelease`,
`CycleFree` and `Opaque`).

- **What takes part.** Every object whose descriptor word names release
  glue, so every type that can hold a counted reference: job 20's per-type
  analysis comes later. Strings and other objects without glue hold
  nothing and never take part. A frame is opaque (`Helper::Opaque`, from
  `task.rs`'s `new_zeroed`): a collection may start while a frame runs,
  when the slots it saved are stale, so it is never entered and never a
  candidate, and what it holds counts as held from outside.
- **The collector word** sits in the last 4 bytes of the block, not right
  after the payload as §23.4 first planned: nothing records a payload's
  size, while the allocator knows each block's. `rt:new` asks for 4 more
  bytes when the descriptor names glue and has std's `track` zero the
  word. Its low 2 bits are the color (black, gray, white, purple), bit 2
  marks an opaque object, and the rest is the object's place in the
  candidate buffer plus one, 0 for none.
- **Candidates.** Std's `release` (`std.rt.alloc`'s and
  `std.rt.checked`'s) makes an object purple and buffers it when it
  leaves a count above zero (`suspect`), and takes it out of the buffer at
  its last count (`forget`), before its glue runs, so a collection never
  sees an object half released. `free` forgets too, for the blocks the
  backend frees directly, such as a list's old buffer. `forget` checks
  that the buffer's entry names the block, so a block without a collector
  word, whose last bytes are data, is left alone. A retain does not
  blacken a purple object: a needless candidate costs a walk, not
  correctness.
- **Tracing is the release glue.** `CycleTrace` runs the descriptor's
  glue on the payload while std's `release` hands each reference to the
  collector's `visit` instead of counting it, so no type needs trace glue
  of its own: every glue releases its references through std's `release`
  and does nothing else.
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
- **Not yet.** Per-type cycle analysis (job 20), so that an acyclic type
  records no candidate and carries no collector word; a cycle through a
  frame is never collected while frames are opaque.
