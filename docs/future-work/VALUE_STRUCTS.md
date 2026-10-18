# Value Structs: Design Study

Status: proposal (#426). The spec and the compiler are unchanged.

## 1. The Problem

Every record in hd is `data`: a heap object with identity and mutable
fields, passed by reference. A list of a million points is a million
objects plus an array of references to them.

The Rust study measured what that costs
([RUST_MODEL_STUDY.md](RUST_MODEL_STUDY.md), section 8). In its
allocation programs, most of Rust's lead is layout, not ownership:
records inline in a `Vec`, words borrowed as `&str` slices, one growing
`String`. Laying hd's points out by hand as two lists of numbers made hd
1.7x faster, and allocating one object at a time measured cheaper on
Wasm GC than Rust's `Box`. The gain is not in avoiding the collector but
in not making an object per record.

hd stays garbage-collected, and lists and maps stay shared references.
Swift's copy-on-write collections need reference counts, which Wasm GC
does not have. What is open is the record inside the collection.

hd already has a value record: the tuple, "an immutable value without
identity" (Tuple Types in
[04-type-system.md](../../spec/lang/04-type-system.md)), which implements
`AnyVal`. The backend lays it out as values
([crates/hd_wasm/DESIGN.md](../../crates/hd_wasm/DESIGN.md) §15.1):
several Wasm values in locals, fields flattened into a parent, one array
per field in a list, and one immutable box past a bound of four values.
A tuple lacks a name, field names, methods and derivations, so readable
programs write `data Point` and pay for an object per point.

The proposal: **`struct` is a named, nominal tuple.** It has the tuple's
value semantics and layout, plus a name, named fields, methods and
derivations.

## 2. What Value Layout Does Today

One question gates the design: does the existing value layout actually
win? An earlier probe on a busy machine had found a list of 3 million
tuple points no faster than the same program with `data`.

### 2.1 Measurements

Two scratch app packages hold the same program, one over
`List[(f64, f64)]` and one over `data P { x: f64, y: f64 }`. Each times
five phases with `std.time.monotonic`, so start-up is excluded: push
(build a list of 3 million points), an indexed `while` sum of
`p.x + p.y`, the same sum with `for`, with `iter().fold`, and `map`
into a new list. Each phase runs five times. Both packages were built
with `hd build --release` (the dev binary `target/verify-target/debug/hd`)
and run on V8 through `host/run.mjs`, and on wasmtime with Cranelift at
`Speed` through `HD_ENGINE=wasmtime target/release/hd --release FILE.wasm`
(the one existing binary with the engine, used only to run the modules).
Each figure is the median of five runs after a warm-up. The wait for a 1-minute load under 4 ended at
3.47; the load then drifted between 3.3 and 5.8 during the runs. Every
run printed the same checksum.

| Phase (ms) | tuples, V8 | `data`, V8 | `data` / tuples | tuples, wasmtime | `data`, wasmtime | `data` / tuples |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| push, 5 x 3M | 99 | 503 | 5.1 | 89 | 145 | 1.6 |
| indexed read, 5 x 3M | 22 | 25 | 1.1 | 35 | 31 | 0.9 |
| `for`, 5 x 3M | 24 | 24 | 1.0 | 34 | 34 | 1.0 |
| fold, 5 x 3M | 59 | 59 | 1.0 | 210 | 210 | 1.0 |
| map, 5 x 3M | 133 | 470 | 3.5 | 141 | 349 | 2.5 |
| push, 5-field records, 5 x 3M | 722 | 730 | 1.0 | 202 | 201 | 1.0 |
| `for`, 5-field records, 5 x 3M | 29 | 32 | 1.1 | 33 | 35 | 1.1 |

V8 allocated 1,349 MB for the tuple program and 1,637 MB for the `data`
one, and 1,182 MB for each 5-field program (heap at exit plus what every
collection freed, as `target/bench4/heap.py` counts). The debug pipeline
gives the same picture: push 115 against 566 ms on V8.

Then the Rust study's points benchmark (1 million points, ten rounds of
`map` and a `for` sum), as whole-process wall time with start-up
included (36 ms on V8, 26 ms on wasmtime for an empty program):

| Points program | V8 (ms) | wasmtime (ms) | V8 allocated |
| --- | ---: | ---: | ---: |
| `data Point`, the benchmark as written | 358 | 266 | 543 MB |
| `List[(f64, f64)]`, the same code with tuples | 168 | 152 | 375 MB |
| two `List[f64]` by hand, `while` loops (the study's 1.7x variant) | 244 | 159 | 375 MB |
| one `List[f64]` by hand, x and y adjacent, `while` loops | 221 | 121 | 375 MB |
| Rust, records inline in a `Vec` (from the study) | 81 | 43 | 176 MB |

Readings:

- **Value layout already wins, by not allocating.** Tuples push 5.1x
  faster than `data` on V8 and 1.6x on wasmtime, and map 3.5x and 2.5x
  faster. Points runs 2.1x faster on V8 and 1.75x on wasmtime with
  tuples, and allocates exactly what the hand-flattened program does,
  with no rewrite.
- **Reading is a tie.** A `data` element costs a cast and a pointer
  chase, a tuple element a bounds-checked load per field array. Objects
  allocated in order sit together in the nursery, so the chase hits the
  cache.
- **Past the bound there is no gain.** Five `f64` fields are one value
  over the bound of 4; the tuple is boxed and measures as `data`.
- **The earlier probe does not reproduce.** The same 3-million-point
  push and `for` sum as single files with `hd FILE.hd`, debug pipeline,
  took 0.11 to 0.14 s with tuples and 0.21 to 0.29 s with `data` (warm
  cache), and 0.21 against 0.31 s with an empty cache, on both
  repository binaries. The probe's load of 15 to 20 is the likely cause.
- **Adjacent beats parallel.** With the same `while` loops, one list
  holding x and y next to each other beat two lists by 1.1x on V8 and
  1.3x on wasmtime. That prices the strided layout of section 5.3.
- **The rest of the gap to Rust is not layout.** After start-up is
  subtracted, tuples are 2.7x behind Rust on V8 and 3.4x on wasmtime,
  down from 6.6x and 6.5x with `data`. Section 2.3 lists what remains.

A side finding on map keys: the same word count keyed by
`(i32, string)` took 1,179 ms on V8 and allocated 9.7 GB, against 707 ms
and 3.1 GB keyed by a `data` type with derived `Eq` and `Hash`, about
300 and 180 ns per map operation. Both run std's generic `Structure`
walker per hash and per comparison (`impl[T] Hash for T by Structure`);
the tuple path allocates more per call. A struct key would inherit
whichever path its derivation takes.

### 2.2 What the Wasm shows

The hot loops were read from the release modules with a scratch
`wasmprinter` tool.

- **Push.** `List[(f64, f64)]` is `{len, (array f64), (array f64)}`
  (`layout::list_ty`). Push is inlined: a capacity test, one `array.set`
  per field, a length store. Growth to 2n + 4 copies each field array
  separately. No tuple is boxed. `List[P]` is `{len, (array eqref)}`,
  and each push runs `struct.new` for the point.
- **Indexed read.** For tuples: the loop test, then hd's index check
  against the same length again, then per field a load of the field
  array from the list and an `array.get` with the engine's own bounds
  check. For `data`: one `array.get`, a `ref.cast`, two `struct.get`s.
  Neither loop hoists the list's fields, which are mutable.
- **`for`** adds the invalidation test per step. No tuple is rebuilt:
  the two values go straight to the body.
- **`fold`** calls a known closure per element with two `f64`
  parameters; the call is the cost, the same for both layouts.
- **`map`** gets the tuple back as two results through multi-value. The
  result list starts empty and doubles, though its length is known.
- **Past the bound**, the five-field tuple is a box per element in an
  `eqref` array: the `data` layout. The box's fields are declared
  mutable in the module, where the design says immutable.

### 2.3 What the compiler must do for value layout to win

Value layout wins exactly where it avoids an allocation (push, map,
passing records through calls) and ties where `data` allocates nothing
(reading, iterating, folding). To win more, in order of size:

1. **Keep every element a value.** The per-element box is the whole
   difference measured above. A struct must use the tuple's layout in
   every position, with no size bound, so every real record qualifies
   (section 5.6).
2. **Store small homogeneous records adjacent.** One strided array beat
   two parallel arrays by 1.1x to 1.3x with identical loops: one growth,
   one array reference, one memory stream.
3. **Size new lists.** `map` and other builders that know their length
   should allocate once instead of doubling from empty. This helps every
   list, not only structs (PERFORMANCE_ROADMAP.md §3.4).
4. **Drop the repeated checks.** In `while k < pts.len(): pts[k]` the
   index check repeats the loop test, and the field arrays are reloaded
   every step. Hoisting the list's fields in loops that make no call
   that can push, and removing an index check that a dominating test
   already made, leaves only the engine's own bounds checks.
5. **Make derived `Eq` and `Hash` allocation-free** before promising
   anything for struct map keys (#434, and #377 for derived metadata).
6. **Inline the closure in `fold` and `map`.** Fold costs 2.5x a `for`
   loop on V8 and 6x on wasmtime, for both layouts. That is the closure
   call (PERFORMANCE_ROADMAP.md §3.5), not layout.

## 3. The Model

```hd
@derive(Eq, Hash, Debug)
struct Point:
    x: f64
    y: f64

impl Point:
    fn scaled(self, k: f64) -> Point:
        Point { x: self.x * k, y: self.y * k }

fn demo() -> f64:
    let p = Point { x: 1.0, y: 2.0 }
    q := p                        # a copy; nothing is shared
    p = Point { ...p, x: 5.0 }    # a new value; q is unchanged
    q.x + p.scaled(2.0).y
```

The rules, in plain words:

1. A `struct` declaration looks like a `data` declaration: named, typed
   fields, optional defaults, field visibility, `@derive`, `impl` blocks.
2. A struct value has no identity. It implements `AnyVal`, as tuples and
   enums do. `is` on a struct is an error, as on a tuple.
3. Its fields are immutable. A field is never a place: `p.x = 1.0` is an
   error, as `pair._0 = 1` is today.
4. A struct type has no `mut` form. `mut Point` is an error, as
   `mut (f64, f64)` is.
5. Assignment and argument passing copy the value. Since no field can
   change, a copy and the original can never be told apart, so the
   compiler may copy or share as layout prefers.
6. A changed value is a new value: the copy-update literal
   `Point { ...p, x: 1.0 }`, as for `data`.
7. Struct types are nominal. Two structs with the same fields are
   different types, and neither is the tuple of its field types.
8. A struct may hold any type: scalars, strings, enums, tuples, other
   structs, and references to `data`, lists, maps, closures and trait
   values. The rule is shallow, as for enum payloads: a struct holding a
   `List` holds a reference to one shared list.
9. A struct may not contain itself through struct and tuple fields
   alone. A cycle must pass through an enum, a `data` type or a
   collection, as in Go, C# and Rust.
10. A struct has no embedded fields. Embedding is a `data` feature built
    on parts that are objects of their own.

`data` stays exactly as it is. The choice for an author is the one Julia
documents for `struct` against `mutable struct`: if two values with the
same fields are interchangeable, it is a struct; if a thing changes over
time and others must see the change, it is `data`.

## 4. Semantics In Detail

### 4.1 Construction and update

A struct is built with the same brace literal as `data`: required
fields, defaults, shorthand, and any order. A copy-update literal builds
a new value from an old one and evaluates no defaults.

Replacing a struct held somewhere else is ordinary assignment to that
place. With `order: mut Order` holding `total: Money`,
`order.total = Money { ...order.total, cents: 0 }` writes the two Wasm
fields where `Order` stores the struct, and allocates nothing.
`lines[i] = LineItem { ...lines[i], unit }` writes one slot in each of
the list's field arrays (section 7.5).

`p.x = 1.0` is never shorthand for `p = Point { ...p, x: 1.0 }`. A
changed struct is always written as a new value, so a copy never hides
behind what reads as a mutation, and neither `pts[i].x` nor a tuple
element becomes a place.

### 4.2 Copying, equality and hashing

Copying a struct copies all its fields' Wasm values, however many there
are. A struct is never boxed to make a copy cheap (section 5), so the C#
and Go advice against large structs holds: a struct of ten values costs
ten moves on every call and every list read. An optional warning can
point out large ones (section 8).

Equality and hashing are on contents; there is nothing else to compare.
As for `data`, and unlike tuples, they come from `@derive(Eq, Hash)` or
a written `impl`: a struct is nominal, and its author decides what `==`
means for it. Derived `Eq` compares fields in declaration order, and
derived `Hash` writes them in that order. A `data` field compares with
its own type's `Eq`; identity never enters a struct's `==`.

### 4.3 What "no identity" means with references inside

A struct holding `owner: User` has no identity, but `User` does. Copying
the struct copies the reference: both copies name the same `User`, and
a change through a `mut User` field shows in both. This is the rule enums
already follow
([`data.enum.immutable.shallow`](../../spec/lang/08-data-and-enums.md#r-data.enum.immutable.shallow)).
A field may be declared `mut User`, as a tuple element may be
`(mut User, i32)`; the struct itself still has no `mut` form.

For constant hoisting and deduplication, two struct values are
interchangeable only when their scalars are equal and their references
are the same objects (DESIGN.md §15.2, "Representation equivalence").

### 4.4 Generics and traits

Generic code is monomorphized, so `List[Point]` and `Map[Key, i32]` get
their own instances and their own layouts. `T < AnyVal` accepts a
struct, `T < AnyRef` rejects it.

A struct implements traits like any type. Methods take `self` by value.
There are no `mut self` methods, since nothing in a struct can change.

Converting a struct to `dyn Trait` or `Any` boxes it: one immutable
object holding its values, as a tuple is boxed today (§15.2, "erased").
C# boxes a struct converted to an interface the same way, and its
documentation warns that boxing allocates. `is` on such a trait value
is unspecified, as for every boxed value.

### 4.5 Derivations and serialization

Structs derive what `data` derives, with the same members: the fields in
declaration order. Serialization is the same as for a `data` type with
the same fields, so switching a record between `data` and `struct`
changes no JSON. A tuple serializes as an array; a struct, like `data`,
as an object.

### 4.6 Patterns

Struct patterns are data patterns: `Point { x, y }`, `Point { x: 0.0, y }`,
and `let Point { x, y } = p`. A struct has one shape, so a struct
pattern with only binding sub-patterns is irrefutable.

### 4.7 Why not something we already have

- **A tuple** has the layout, but `p._0` names nothing, and a tuple has
  no methods, derivation choices or nominal type.
- **A one-variant enum**, `enum Point: P(x: f64, y: f64)`, is a value
  today, but it is built with call syntax and read only by matching.
- **A newtype**, `type Mile(i32)`, covers one field and stays.
- **`data` plus an optimizer.** Scalar replacement
  ([PERFORMANCE_ROADMAP.md](PERFORMANCE_ROADMAP.md) step 22, #427)
  removes objects that never leave a function. It cannot flatten a
  record stored in a list, because a `data` element has identity and may
  be aliased and mutated elsewhere. Java needed a new kind of class for
  the same reason (section 6).

### 4.8 Migration between `data` and `struct`

Changing `data Point` to `struct Point` either compiles to the same
behavior or fails to compile. Every way a program could observe sharing
needs a field assignment, a `mut Point` or `is`, and each is an error on
a struct. A program that only builds, reads and passes points keeps its
meaning and loses its objects.

## 5. Layout On Wasm GC

Two constraints shape everything. Wasm GC has no interior references:
nothing can point into the middle of an array or a struct. And it has no
inline struct arrays: an array element is one scalar, one packed
integer or one reference. Nested and inline aggregates are listed as
post-MVP work in the GC proposal. So "a `Vec<Point>`" can only be
emulated.

A struct is never boxed for its size. It uses the tuple's unboxed layout
in every position, however many fields it has. Its size counts Wasm
values: one per scalar, enum tag or reference, two per string view (a
reference and an `i64` span), and the sum over nested structs.

### 5.1 Locals, arguments and results

A struct is its fields' Wasm values: a `Point` local is two `f64`
locals, an argument is two parameters, and a result is two results of a
multi-value function. Both V8 and wasmtime support multi-value returns.
Nothing is allocated. Passing a k-value struct through a call costs k
register moves against one for a reference.

### 5.2 Fields

A struct field inside `data`, another struct, an enum payload or a
closure environment is flattened into the parent: a `Rect` of two `Vec2`
is four `f64` fields of the enclosing object, or four values in a local.
Reading `rect.min.x` from a `Rect` field is one `struct.get`, against
two dependent loads when `Rect` and `Vec2` are `data`.

### 5.3 Lists and maps

| Plan | Push | Read all fields | Scan one field | Memory per 2 x `f64` element | Objects per list |
| --- | --- | --- | --- | ---: | ---: |
| `data` element (today) | allocate one object, store one reference | load the reference, cast, then a dependent load per field | same | about 28 bytes (a 24-byte object and a 4-byte slot) | n + 2 |
| One array per field (today's tuple layout) | one `array.set` per field, in separate arrays | one load per field, each with its own bounds check | one array, the best case | 16 bytes | k + 1 |
| One strided array (all fields share a Wasm type) | k stores into one array | k loads from one array, adjacent in memory | every k-th element | 16 bytes | 2 |

The plan:

- **First slice: one array per field**, the tuple layout as it is. It
  already exists, is already measured at 2x on points, and keeps the
  rule that a layout reads only the type's definition.
- **Second step: strided storage when every field has one Wasm storage
  type** (all `f64`, all `i32`, all `i64`). `List[Point]` becomes
  `{len, (array f64)}` with element i at 2i and 2i + 1. The hand-written
  version measured 1.1x (V8) to 1.3x (wasmtime) over parallel arrays.
  Push grows one array, and a read loads one array reference. A scan
  that reads one field of a wide struct pays for the skipped fields'
  bandwidth, where parallel arrays would not; for records of two to four
  fields, read whole, that cost is small.
- **Mixed records** such as `{id: i32, score: f64, name: string}` keep
  parallel arrays. An `i64` array holding the `i32` and the `f64` bits
  (a reinterpret per read) is possible but unmeasured.

The choice is made per element type, never per use, so `pts[i]` has one
meaning in every position.

`Map[K, V]` already stores one array per key value and per map value
(`layout::map_ty`), so a struct key costs no object per entry. A
`(string, i32)` key is three arrays. A data key is one reference to an
object built for every insert and every lookup.

### 5.4 Strings inside structs

A string field is a view, `(ref $bytes, i64 span)`, so it costs two
values and 12 bytes at rest. A slice stored in a
struct allocates nothing: `WordDay { day, word }` built from
`line.split(" ")` refers into the line's bytes. The known retention risk
of views applies: a small slice keeps its whole source alive.

### 5.5 Where a reference is unavoidable

Size never boxes a struct. Only positions that hold exactly one
reference for any type do, as C# boxes a struct converted to an
interface and Swift boxes a large value in an existential:

- **Erased positions**: converting a struct to `dyn Trait` or `Any`
  copies it into one immutable box. Generic code is monomorphized, so a
  type parameter never erases a struct.
- **Inside a self-recursive enum's payload**: the struct's fields sit in
  the enum's own box, with no separate struct object.

### 5.6 No size bound

Tuples and enums are boxed past 4 Wasm values today. That bound came
from measuring parallel arrays on a read-only kernel and keeping `push`
under 256 bytes. A struct has no such bound: its author chose a value,
so the compiler keeps it one. A `LineItem` of five values and the JSON
benchmark's `Record` of six are five and six parallel arrays in a list,
not an object per element.

The cost of a wide struct moves elsewhere: push code grows with the
field count, every copy moves every value, and a scan of one field
skips over the rest only in strided storage. Those are the author's
trade-offs, shown by the optional warning of section 8.

## 6. Prior Art

| Language | Value record | Mutable fields | Inline in arrays | Generic code | Boxing |
| --- | --- | --- | --- | --- | --- |
| C# | `struct`, `readonly struct`, `record struct` | yes, unless `readonly` | yes | specialized per value type | on conversion to `object` or an interface |
| Go | every struct | yes | yes | specialized by memory shape | on conversion to an interface |
| Julia | `struct` (immutable) next to `mutable struct` | no | yes, when the type has no references ("isbits") | specialized | when the element type is abstract |
| Java (Valhalla, JEP 401) | `value class` | no | the JVM may flatten | erased | the JVM decides |
| Swift | every struct | on `var` bindings | yes | specialized or witness tables | existential containers |
| Kotlin | `value class` with one property | no | no | boxed | "whenever they are used as another type" |
| OCaml (unboxed types RFC) | unboxed records and tuples `#( ... )` | no | proposed | not with ordinary polymorphism | explicit |

- **C#** copies a struct on assignment, argument passing and return,
  recommends immutable structs, and warns that boxing one means "a new
  object must be allocated and constructed." Its `with` expression is
  hd's copy-update.
- **Julia** gives the reason this proposal rests on: an immutable object
  "may be copied freely by the compiler since its immutability makes it
  impossible to programmatically distinguish between the original object
  and a copy." Arrays store bits types inline.
- **Java's JEP 401** adds value objects, "immutable and lack object
  identity," so that a JVM can flatten fields and array elements and
  scalarize locals. It is the closest match: a collected runtime that
  needed a new kind of class because no optimizer could prove an
  ordinary object identity-free.
- **Swift** collections avoid struct copies with copy-on-write over a
  reference count, which hd cannot use. **Go** passes "everything" by
  value. **Kotlin's** one-property value class is hd's newtype.
- **OCaml's** unboxed types proposal passes an unboxed tuple "around as
  three separate values," but not through ordinary polymorphism; hd
  monomorphizes, so it has no such limit.

The common shape: immutability without identity is what lets a
collected runtime flatten a record.

## 7. Tour

### 7.1 Points

```hd
struct Point:
    x: f64
    y: f64

fn points(n: i32, rounds: i32) -> i64:
    let pts: mut List[Point] = []
    let i = +0
    while i < n:
        pts.push(Point { x: f64(i % 1000), y: f64(i % 777) })
        i = i + 1
    let total = 0.0
    let r = +0
    while r < rounds:
        shift := f64(r)
        moved := pts.map(fn(p): Point { x: p.x * 0.5 + p.y, y: p.y - p.x * 0.25 + shift })
        for p in moved:
            total = total + (p.x + p.y)
        r = r + 1
    i64(total * 4.0)
```

This is the benchmark with one word changed. `List[Point]` is
`{len, (array f64), (array f64)}`. A push stores two numbers; the
closure takes two `f64` parameters and returns two results; `map` fills
two new arrays. The only allocations are the arrays: 375 MB on V8
against 543 MB with `data`, and 168 ms against 358 ms (V8), 152 against
266 ms (wasmtime), as the tuple version measured in section 2.1. With
strided storage and a sized `map`, it should approach the 121 ms the
strided hand version took on wasmtime.

### 7.2 Map counts with a key struct

```hd
@derive(Eq, Hash)
struct WordDay:
    day: i32
    word: string

fn count(lines: List[(i32, string)]) -> Map[WordDay, i32]:
    let counts: mut Map[WordDay, i32] = {}
    for (day, line) in lines:
        for word in line.split(" "):
            key := WordDay { day, word }
            counts[key] = counts.get(key).unwrap_or(+0) + 1
    counts
```

`WordDay` is three values: the day, and the word's bytes reference and
span. The map stores three key arrays; `word` is a slice of `line`, so a
key costs no object at all. As `data`, every lookup builds a probe
object, and every new entry keeps one.

Expected, once derived `Hash` and `Eq` stop allocating: no object per
key and no object per lookup. Today the measured composite-key paths
cost 180 to 300 ns per map operation on V8 (707 and 1,179 ms for 4
million operations; 2,040 and 2,552 ms on wasmtime). They allocate 3.1
and 9.7 GB, where the Rust study's map count with plain string keys
allocates 447 MB in all, so the per-call walker dominates. Until the
derivation is fixed, a struct key would be no faster than a `data` key,
so job 7 below comes before any claim.

### 7.3 String building

The string-build benchmark pushes three million small pieces into a
`StringBuilder`. Its gap to Rust is the builder keeping a list of piece
strings where Rust appends bytes to one buffer
([PERFORMANCE_ROADMAP.md](PERFORMANCE_ROADMAP.md) §3.3, step 10). No
record is involved, so structs change nothing here; this row is listed
so the tour does not overclaim.

### 7.4 JSON records

```hd
@derive(Serialize, Deserialize)
struct Record:
    id: i32
    name: string
    active: bool
    score: i32
    tags: List[string]
```

`Record` is six values (`name` counts two), so the 400,000 decoded
records are six arrays instead of 400,000 objects. That is not where
most of the JSON gap is: decoding goes through a `Json` tree (2.1 GB
allocated where Rust allocates 164 MB). The measurement plan prices the
records' share.

### 7.5 Money and line items

```hd
enum Currency:
    Usd
    Eur

@derive(Eq, Hash, Debug, Serialize, Deserialize)
struct Money:
    cents: i64
    currency: Currency

impl Money:
    fn usd(cents: i64) -> Money:
        Money { cents, currency: .Usd }

    fn times(self, n: i32) -> Money:
        Money { ...self, cents: self.cents * i64(n) }

impl Add for Money:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        if self.currency != rhs.currency:
            panic("adding money in two currencies")
        Money { ...self, cents: self.cents + rhs.cents }

@derive(Debug, Serialize, Deserialize)
struct LineItem:
    sku: string
    quantity: i32
    unit: Money

data Invoice:
    customer: string
    items: mut List[LineItem]

impl Invoice:
    fn total(self) -> Money:
        let sum = Money::usd(0)
        for item in self.items:
            sum = sum + item.unit.times(item.quantity)
        sum

    fn reprice(mut self, sku: string, unit: Money) -> void:
        let i: usize = 0
        while i < self.items.len():
            if self.items[i].sku == sku:
                self.items[i] = LineItem { ...self.items[i], unit }
            i = i + 1
```

`Money` is two values: an `i64` and an `i32` tag. Every `+` and `times`
returns two values and allocates nothing; today each allocates a `Money`
object. `Invoice` stays `data`: it has identity, and `reprice` changes
it in place. `LineItem` is five values, so the item list is five
arrays, and replacing an item writes one slot in each.

With `data` in place of `struct`, this program runs today; that is the
migration property of section 4.8 at work.

### 7.6 Geometry

```hd
@derive(Eq, Debug)
struct Vec2:
    x: f64
    y: f64

@derive(Eq, Debug)
struct Rect:
    min: Vec2
    max: Vec2

impl Rect:
    fn contains(self, p: Vec2) -> bool:
        p.x >= self.min.x && p.x < self.max.x && p.y >= self.min.y && p.y < self.max.y

fn hits(rects: List[Rect], p: Vec2) -> usize:
    let n: usize = 0
    for r in rects:
        if r.contains(p):
            n = n + 1
    n
```

`Rect` flattens to four `f64` values. `List[Rect]` is four `f64` arrays
today, or one strided array with the plan of section 5.3. `contains`
takes six `f64` parameters and reads no memory. With `data`, a list of
rectangles is three objects per `Rect`, and reading a corner coordinate
costs a cast and two dependent loads.

## 8. Migration And The Large-Struct Lint

**Existing programs** see nothing: `data` is unchanged, and `struct` is
new. The keyword is reserved today, with the error
`old-struct-declaration`
([`data.decl.no-struct`](../../spec/lang/08-data-and-enums.md#r-data.decl.no-struct));
that rule and its fixture go away.

**Standard library types.** A std type becomes a struct when its fields
never change after construction and nothing depends on its identity:

| Type | Fields | Values | Plan |
| --- | --- | ---: | --- |
| `Duration`, `Timestamp`, `Instant` | `millis: i64` | 1 | newtypes, as PERFORMANCE_ROADMAP.md step 16 already plans; a one-field struct buys nothing over a newtype |
| `Date` | year, month, day | 3 | struct |
| `Range[T]`, `RangeFrom[T]`, `RangeTo[T]` | start, end, inclusive | up to 3 for integer `T` | struct |
| `std.regex.Match` | start, end, text | 4 | struct |
| `Reverse[T]` | value | layout of `T` | struct: today every sort key wrapped in `Reverse` is an object |
| `Backoff`, `TimedOut` | durations and counts | 4, 1 | struct once the time types are newtypes |
| `Datagram` | bytes, host, port | 4 | struct |
| `DefaultHasher`, `SeededRandom`, builders, scripted providers | mutated in place | | stay `data` |

Each conversion changes a public type from `AnyRef` to `AnyVal`. A
program that put such a value behind `T < AnyRef` or compared it with
`is` stops compiling; nothing changes meaning silently.

**The lint.** A large struct is legal and stays unboxed, but every copy
moves every value. An optional warning at the declaration can say so.
It is a convenience, not part of the model, and it lands after the
language jobs; its threshold comes from measurement (section 10):

```text
warning: large-struct: `Report` is 14 Wasm values; each copy moves all of them
  help: split it, or declare it `data` if values are shared and changed
```

## 9. Costs

| Cost | Size | Notes |
| --- | --- | --- |
| Language surface | one declaration form | it reuses data's literal, update, pattern and derivation syntax; the new rules are "immutable, no identity, no `mut` form" |
| A choice for every record | real | the guide needs one paragraph: values that are interchangeable are structs |
| Checker | small | structs are tuples with names: field resolution from `data`, value rules from tuples |
| Layout | small for the first slice | the tuple shape with a name; flat list storage of every value layout already exists |
| Code size | per `List[S]` instance, push grows with the field count | no bound caps it; jobs 11 and 12 measure it |
| Compile time | none expected | one more nominal kind in monomorphization keys |
| Copies | one move per Wasm value | no bound caps it; the optional warning points out large structs |
| Binary compatibility | none to keep | layout is not observable ([`data.repr.no-dependence`](../../spec/lang/08-data-and-enums.md#r-data.repr.no-dependence)) |

## 10. Measurement Plan

The section 2 sources are scratch files in `target/structs/`, next to
the Rust study's `target/bench4/`. The first slice (job 4) must show, on
both engines, release builds, medians of five runs at a load under 4:

1. **Points with `struct`** at the speed of points with tuples (section
   2.1), and the same bytes allocated as the hand-flattened program.
2. **No object per element**: a `List[Point]` push loop allocates only
   its arrays (V8 `--trace-gc-nvp` bytes, as `target/bench4/heap.py`).
3. **No regression elsewhere**: hello.wasm unchanged, the runtime suite
   within noise.

Later slices measure their own claims: map-key structs at the speed of
an integer-pair key once derived `Hash` stops allocating; strided
against per-field arrays on the points and geometry loops; `LineItem`
and `Record` lists against `data`, and the call and copy cost of structs
of 8, 12 and 16 values, reporting push code size, read kernels and the
JSON benchmark. That last measurement sets the warning's threshold.

## 11. Implementation Plan

Each job is about an hour, ships its spec rules and conformance fixtures,
and reports the measurement it names.

| # | Job | Spec and fixtures | Reports |
| --- | --- | --- | --- |
| 1 | Parse `struct` declarations with data's field grammar; drop the `old-struct-declaration` rule | a "Struct Declarations" section in `08-data-and-enums.md` replacing `data.decl.no-struct`; parse fixtures, valid and invalid | hello.wasm unchanged |
| 2 | Check construction and access: literals, defaults, shorthand, copy-update, field access, visibility; reject field assignment and `mut S` | rules for immutable fields and no `mut` form, beside `types.tuple.no-mut`; check and run fixtures | none |
| 3 | Value rules: `is` rejected, `AnyVal` and `AnyRef` bounds, `mut U` fields, the struct-cycle error | rules in `04-type-system.md` (Tuple Types, `AnyVal`) and the new section; invalid fixtures per error | none |
| 4 | Lower a struct as a named tuple with no size bound: values in locals and results, flattened fields, one array per field in lists and map keys, boxed only in `dyn` and `Any` | run fixtures per position; a test that a `List[Point]` push emits no `struct.new` | points with `struct` against tuples (equal within noise) and `data`; Wasm bytes; hello.wasm |
| 5 | Struct patterns in `match` and `let`, irrefutability, exhaustiveness | pattern rules beside Data Patterns; fixtures | none |
| 6 | Derivations and serialization on structs: `Eq`, `Hash`, `Ord`, `Debug`, `Serialize`, `Deserialize`, `Structure` members | a rule that struct members are its fields; a fixture that JSON matches the `data` form | none |
| 7 | Derived `Eq` and `Hash` without a walker object per call, with `Hasher.write_u64` and `write_str` (#434) | std hash rules where they change | the map-key programs of section 2.1 with tuple, `data` and struct keys; ns per operation, bytes |
| 8 | Optional, after job 12: the `large-struct` warning | a lint rule and a fixture | none |
| 9 | Builders that know their length allocate once: `map`, `collect` from a sized source | std rules if any; existing fixtures | the points map phase on both engines |
| 10 | Remove an index check that a dominating test already made, and hoist a list's fields out of loops with no growing call | none (optimizer); conformance digests | the indexed and `for` phases on both engines |
| 11 | Strided storage for structs and tuples whose fields share one Wasm storage type | none (layout is unobservable); run fixtures for push, index, set, growth, `for`, map | points and geometry against parallel arrays; push code bytes per instance |
| 12 | Measure wide structs: `LineItem` and `Record` lists against `data`, and calls and copies of 8-, 12- and 16-value structs | none | push code size, read kernels, the JSON benchmark, the warning threshold |
| 13 | std migration: `Date`, the `Range` types, `regex.Match`, `Reverse`, `Datagram`, `Backoff`, `TimedOut` become structs | spec/std rules that name `data`; fixtures that use `is` or `AnyRef` on them | allocation of a `Date` loop and a sort with `Reverse` keys |
| 14 | Guide and website: choosing `struct` or `data`, with the points and money examples | none | none |

Jobs 1 to 6 are the language. Job 4 delivers the measured gain. Jobs 7
and 9 to 12 are what section 2.3 says the compiler must add.

## 12. Open Questions

1. **Should tuples drop their bound too?** A struct is a named tuple,
   yet a five-value struct would be unboxed while a five-value tuple is
   boxed. Recommendation: decide from job 12. If wide structs measure
   well, give tuples the same rule, so the two layouts stay one.

## Sources

- C#: [Structure types](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/struct),
  [Boxing and unboxing](https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/types/boxing-and-unboxing),
  [The `with` expression](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/operators/with-expression).
- Go: [The Go Programming Language Specification](https://go.dev/ref/spec)
  (struct types, comparison operators), [FAQ](https://go.dev/doc/faq) ("everything in Go is passed by value").
- Julia: [Types](https://docs.julialang.org/en/v1/manual/types/) (composite
  types, mutable composite types),
  [isbits Union Optimizations](https://docs.julialang.org/en/v1/devdocs/isbitsunionarrays/).
- Java: [JEP 401: Value Objects](https://openjdk.org/jeps/401).
- Swift: [Structures and Classes](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/classesandstructures/).
- Kotlin: [Inline value classes](https://kotlinlang.org/docs/inline-classes.html).
- OCaml: [Unboxed types RFC, version 2](https://github.com/ocaml/RFCs/pull/34).
- WebAssembly: [GC post-MVP: nested data structures](https://github.com/WebAssembly/gc/blob/main/proposals/gc/Post-MVP.md).
- hd: [RUST_MODEL_STUDY.md](RUST_MODEL_STUDY.md) section 8,
  [PERFORMANCE_ROADMAP.md](PERFORMANCE_ROADMAP.md),
  [crates/hd_wasm/DESIGN.md](../../crates/hd_wasm/DESIGN.md) §15.1 and §15.2,
  `crates/hd_wasm/src/layout.rs` (`list_ty`, `map_ty`, `BOUND`).
