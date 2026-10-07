# Research: Enums As Identity-Free Values

Part of the [compiler design](README.md).

Status: Research for an owner decision, 2026-10-07.

The question (owner, 2026-10-07): make every enum an identity-free value
type, `Option`, `Result` and user enums alike. Enums move from `AnyRef`
to `AnyVal`, and `is` on any enum becomes a compile error. "Research,
how to do that and how to handle cyclic stuff."

## Summary

**Recommendation: adopt.** Confidence: high for the semantics, medium for
the one forced library change (`Error.find`, below). The change replaces
the special rules of decision B for `.Some`, `.None`, `.Ok` and `.Err`
with one rule: an enum is a value, like a tuple with a tag.

**What changes.**

- `AnyVal` gains every enum, optionals and `Result` included. `AnyRef`
  keeps data, `List`, `Map`, functions, trait values, `Any`, suspensions
  and handles.
- `is` with an enum operand is `identity-requires-references`, by the
  existing rule that `is` operands implement `AnyRef`. No new diagnostic.
- `is` on two trait or `Any` values that hold enums has an unspecified
  result, as decision B already proposes for boxed primitives.
- `Error` can no longer extend `AnyRef`: 9 of the 10 std error types are
  enums. `Error` drops `AnyRef`, and `find` becomes a free function
  `std.error.find[T < Error](error: Error) -> T?`, since a dynamically
  safe method cannot take a value-typed `T`.
- Enum downcasts use `downcast_val`, as tuples and strings do today.
- The compiler may give small enums a multi-value layout, hoist constant
  enum values to globals, and share or copy any enum value freely.

**What is lost.**

- `is` on optionals such as `User?`, which decision B kept. No std code
  and one fixture use it.
- Identity of enum values: memoizing or interning an AST node by `is`.
  hd has no identity map, so such code was already a linear scan.
- The guarantee that `find` returns "the same reference" for an enum
  target. It returns an equal value instead.

**How cycles are handled.** An enum can never close a cycle by itself:
its payloads exist before it does, and it never changes. So every cycle
in hd passes through at least one data value, list, map, or closure
capture cell, all of which stay `AnyRef`. Cycle-sensitive code tracks
only `AnyRef` nodes:

- `dbg` tracks data values, lists and maps, and stops tracking enums.
- Boundary encoding detects `boundary-cycle` the same way.
- Derived `Eq`, `Ord`, `Hash`, `Display`, serde and the `Structure`
  walker keep their current rule. They do not detect cycles, and a cyclic
  value may exhaust the stack.
- `Error` cause chains and garbage collection need nothing new.

## 1. Prior Art

| Language | Sum type | Identity on variants | Recursion | What it gained | What it lost |
| --- | --- | --- | --- | --- | --- |
| Swift | `enum`, a value type | none: `===` needs `AnyObject`, which only classes satisfy | `indirect` marks the case, and the compiler boxes it | inline layout, no allocation for small enums, enums as `Error` types | a keyword to learn; class-only protocols can't take enums |
| Rust | `enum` | none (no `is`; raw pointer compare only) | explicit `Box`, `Rc` or `Arc` | niche layout: `Option<Box<T>>` is one pointer; no heap for small enums | users write the indirection by hand |
| Kotlin | `sealed class`, `enum class` | yes, they are classes | free (references) | open hierarchies, Java interop | every payload case allocates |
| Kotlin | `value class` | `===` is prohibited | not applicable (one field) | unboxed in direct positions | boxed when used as a generic, nullable or interface type |
| Java (Valhalla) | `value class` | `==` compares field values; `synchronized` throws `IdentityException` | a value object cannot reach itself: fields are set before `super()` | flattening in fields and arrays, `Optional` and `Integer` migrated | migration breaks code that synchronizes on such instances |
| C# | `struct` vs `record class` | structs have none; record classes keep `ReferenceEquals` | structs cannot contain themselves | stack and inline storage | copying cost for large structs; two kinds to choose from |
| OCaml | variants, immutable | `==` is defined, but on immutable values its result is implementation-dependent | free (boxed blocks) | constant constructors are unboxed integers | `==` on immutable values is unreliable |
| F# | unions, reference by default; `[<Struct>]` opt-in | reference unions keep .NET identity; struct unions have none | a multi-case struct union cannot be recursive | struct unions avoid allocation | the author must pick per type |
| Scala 3 | `enum`, compiled to sealed classes | `eq` works; parameterless cases are singletons | free | Java interop | every payload case allocates |
| Haskell | algebraic data | none in the language; `StableName` and an unsafe primitive exist for memo tables | free (lazy heap) | sharing, update in place by the runtime | identity-keyed memo tables need `StableName` |
| Gleam, Elm | custom types | none: `==` is always structural | free | one equality, simple model | no reference identity at all (Elm also crashes on function `==`) |
| Roc | tags | none | free | in-place update when a value is unique (reference counting) | not applicable |

### 1.1 Lessons

1. **No language with identity-free sum types regrets it.** Swift, Rust,
   Gleam, Elm, Roc and Haskell have none, and their users build trees,
   graphs and memo tables anyway. Graphs use a mutable node type or ids.
2. **The cost of opting in is the lesson from F# and C#.** Two kinds of
   union force a choice per type. hd should have one kind, and let the
   compiler pick the layout.
3. **Recursion needs indirection only without a GC.** Swift's `indirect`
   and Rust's `Box` exist because values are stored inline and ownership
   must be explicit. F# forbids recursive struct unions for the same
   reason. With GC references, the compiler can insert the indirection
   itself. Section 2 says how.
4. **Valhalla shows the cyclic argument.** A value object's fields are set
   before it exists, so value objects cannot form a cycle among
   themselves. That is why Java can define `==` on them as a recursive
   field compare that always ends. hd enums have the same property
   (section 3).
5. **Swift's `Error` protocol is not class-bound.** Swift says
   enumerations are "particularly well suited" to errors. Its errors are
   values, boxed when erased to `any Error`. hd's `Error < AnyRef` would
   be the outlier (section 4.2).
6. **OCaml and Kotlin show the cost of a half rule.** OCaml's `==` on
   immutable values "is implementation-dependent". Kotlin forbids `===` on
   value classes because "referential equality is pointless" when a value
   may be boxed or unboxed. hd should make the static case an error, as
   Kotlin does, and leave only the erased `Any` case unspecified.

## 2. Recursive Enums

A recursive enum has a payload that contains the same enum, directly or
through other types:

```text
enum Expr:
    Num(value: i64)
    Neg(inner: Expr)
    Add(left: Expr, right: Expr)

enum Json:
    Null
    Num(value: f64)
    Text(value: string)
    Items(values: List[Json])
```

### 2.1 Representation

- **No keyword.** The compiler finds the strongly connected components of
  the type graph. It already does, for Wasm rec groups
  ([wasm-layout.md §15.3](wasm-layout.md#153-the-type-section)). An enum
  whose payload reaches the enum again, directly, is **self-recursive**.
- **Self-recursive enums keep a reference layout.** Each payload variant
  is one immutable struct, as the flat or subtype layout in
  [§15.2](wasm-layout.md#152-values) says today. A payload of the enum's
  own type is a reference to such a struct. This is Swift's `indirect`,
  chosen by the compiler.
- **Recursion through a collection needs nothing.** `Json.Items` holds a
  `List[Json]`, which is already a reference. `Json` itself may then use
  a value layout, and the list's elements use the same layout.
- **Values may share structure.** `Add(e, e)` holds two references to one
  struct. That was legal before and remains so. Without identity, nothing
  can tell sharing from copying.

### 2.2 When Codegen May Copy, Deduplicate Or Flatten

Without identity, every enum value may be represented by any equal value.
The compiler gains these freedoms:

| Freedom | Allowed before | Allowed after | Example |
| --- | --- | --- | --- |
| Pass a small enum as several Wasm values | `.None` only; payload variants needed one struct per construction | yes, for any enum whose layout fits | `Result[i32, ParseError]` as `(i32, i32, i32)` |
| Copy an enum into a containing struct's fields | no | yes | a `data Token` field of an enum type stored inline |
| Store a list of enums as a structure of arrays | no | yes | `List[Token]` as a tag array plus one array per payload slot |
| Hoist a constant enum value to a global | payload-free variants only | any value whose payloads are constants | `.Err(ParseError.Empty)` built once |
| Deduplicate equal values at run time (hash-consing) | no | yes, as an optimization | interning `Expr.Num(0)` |
| Scalar-replace a payload variant | only when it never reaches `is`, a field or an erasure (§12.6) | whenever it does not escape | `match parse(s): .Ok(v) => ...` allocates nothing |
| Flatten a self-recursive payload | no | no | a recursive position needs a reference to stay finite |

The last row is the one limit. A self-recursive payload must stay a
reference, or the layout would be infinite. Unrolling one level is legal
but not worth its code size.

## 3. Cyclic Structures

### 3.1 How A Cycle Forms In hd

A cycle needs a reference stored into an object after that object
exists. These are all the ways a stored reference can change after
construction:

| # | Path | Example | Involves enums? |
| --- | --- | --- | --- |
| 1 | Assigning a data field through a `mut` view | `node.next = .Some(node)` | yes, as an interior node: the enum holds the data value |
| 2 | Changing a list: `push`, index store, insert | `kids.push(Tree.Node(kids))` | yes, as an interior node |
| 3 | Changing a map entry | `m["a"] = Value.Table(m)` | yes, as an interior node |
| 4 | Assigning a captured mutable local after a closure captured it | `let mut e = E.None; f := fn: e; e = E.Call(f)` | yes, as an interior node; the cycle goes through the capture cell |
| 5 | A suspension frame holding a local that refers to the frame | a frame stored in a data field that its own locals reach | no |
| 6 | Module-level variables | a global read later by a closure | not a heap cycle: a closure reads a global at call time and holds no reference |

These cannot form a cycle:

- **Enum construction.** The payloads are evaluated first, so they exist
  before the enum value. `data.enum.immutable` then fixes them.
- **Tuples and strings.** Immutable, so the same argument holds.
- **Shared constructor data.** It is evaluated once at compile time and is
  readonly ([`data.shared.compile-time`](../../spec/lang/08-data-and-enums.md#r-data.shared.compile-time)).
- **Copy-update, defaults and data literals.** Each builds a new object
  from values that already exist. A later assignment is path 1.
- **Embedded parts.** A type that embeds itself is `embedding-cycle`, and
  a part's mutable edges are ordinary fields (path 1).

**Lemma.** Every cycle contains at least one data value, list, map or
capture cell. Proof: order values by the time they were built. An
immutable value only refers to values built before it. A cycle made only
of immutable values would need a value built before itself.

So an enum can be part of a cycle, but never closes one alone. Each
mutable node on a cycle is `AnyRef`, before and after this change.

### 3.2 What The Spec Says Today

| Operation | Rule | On a cycle |
| --- | --- | --- |
| `dbg` | [`module.dbg.cycle`](../../spec/lang/10-modules.md#r-module.dbg.cycle.tracked) | prints `<cycle>`; the 10-level depth limit is a backstop |
| derived `Eq` | [`std-cmp.derive.eq.cycles`](../../spec/std/cmp.md#r-std-cmp.derive.eq.cycles), `std-cmp.derive.eq.stack` | no detection; may exhaust the stack |
| derived `Hash` | [`std-hash.derive.hash.cycles`](../../spec/std/hash.md#r-std-hash.derive.hash.cycles) | same as `Eq` |
| derived `Ord`, `Display`, serde, `Structure` walk | no rule | recursion without detection, so the same as `Eq` |
| host boundary encoding | [`module.boundary.cycle`](../../spec/lang/10-modules.md#r-module.boundary.cycle) | `boundary-cycle` failure |
| `Error` cause chain | no rule | `chain` loops forever on a cyclic chain |
| garbage collection | [`data.repr.gc`](../../spec/lang/08-data-and-enums.md#r-data.repr.gc) | unreachable cycles are reclaimable; no user finalizers |

None of these rules mentions enums or uses enum identity. Only the
implementations of `dbg` and of boundary encoding use identity at all.

### 3.3 The Design For Each Operation

**`dbg` and `Debug`.** The writer keeps a stack of the nodes it is inside
(`DebugWriter.active` in `lib/std/format.hd`). Today the generated code
pushes data values and enum values, and compares with `is` on `Any`.
After the change:

1. Push only `AnyRef` nodes that can be changed: data values with fields,
   lists and maps.
2. Stop pushing enum values. On `Any`, their `is` would be unspecified,
   and could report a false cycle between two equal variants.
3. Start pushing lists and maps. Today a cycle `Tree -> List -> Tree` is
   caught at the enum. Without the enum it must be caught at the list.

By the lemma, every cycle has a pushed node, so `<cycle>` still prints.
The stack is at most one entry per nesting level, and the depth limit
bounds it at 10 under `dbg`. The prototype's generator
(`src/checker/debug-print.ts`) needs the same two edits.

**Boundary encoding.** The encoder keeps the same kind of path stack, with
the same rule: track data values, lists and maps. An enum on the path is
not tracked. [`module.boundary.sharing`](../../spec/lang/10-modules.md#r-module.boundary.sharing)
already says sharing is not restored, so nothing changes for enums.

**Derived `Eq`, `Ord`, `Hash`, `Display`; serde; `Structure`.** No change.
They never used identity. They recurse, and on a cycle they may exhaust
the stack, as the spec says. Making enums values cannot add a cycle, by
the lemma. An identity fast path (`a is b` implies equal) was never
allowed for `Eq`, because `NaN != NaN`. The compiler may still add one as
a private optimization for types whose leaves are all reflexive.

**`Error` cause chains.** A cyclic chain needs a data error whose `cause`
field is reassigned to point back. That is contrived code, and the spec
leaves it alone today. Detecting it would need `is` on `Error` values,
whose payloads are now mostly enums, with an unspecified result. Keep no
rule.

**Garbage collection.** Wasm GC traces, so cycles are reclaimed. hd has
no user finalizers ([`data.repr.no-manual`](../../spec/lang/08-data-and-enums.md#r-data.repr.no-manual)).
No change.

## 4. Everything Else That Changes

### 4.1 `AnyVal`, `AnyRef` And Generic Bounds

| Type | Before | After |
| --- | --- | --- |
| user enums, payload-free or not | `AnyRef` | `AnyVal` |
| `T?` | `AnyRef`; decision B: the payload's category | `AnyVal` |
| `Result[T, E]` | `AnyRef` | `AnyVal` |
| newtype over an enum | `AnyRef` | `AnyVal`, by [`types.sealed.newtype`](../../spec/lang/04-type-system.md#r-types.sealed.newtype) |
| `mut E` | `AnyRef` | `AnyVal`; permission does not change the category |

- A `T < AnyRef` function no longer accepts an enum. A `T < AnyVal` one
  now does. `T < Any` accepts both, as before.
- The meaning of `AnyRef` becomes exact: "a value that has identity". The
  meaning of `AnyVal` becomes "immutable at its top level, without
  identity". Enums fit the second, by `data.enum.immutable`.

### 4.2 `Error` And Dynamically Safe Generic Methods

A method of a dynamically safe trait may have a type parameter only if
its bounds imply `AnyRef`
([`trait.dyn.safe.implied-anyref-param`](../../spec/lang/09-traits.md#r-trait.dyn.safe.method-type-param)).
The reason is representation, not identity: the one shared body receives
`T` as `anyref`, and so must every `List[T]` it touches
([codegen.md §13.5](codegen.md#135-dictionaries-trait-values-and-gadt-evidence)).
A value-typed `T` would need its containers converted, which is not
possible for a mutable `List[T]`. So the rule must stay.

`Error` is `trait Error < Display & Inspectable & AnyRef`, and it has the
method `find[T < Error]`. With enums in `AnyVal`, `impl Error for FsError`
fails `missing-supertrait-implementation`. That breaks 9 of the 10 std
error types and about 54 fixture error enums. The options:

| Option | Change | Result |
| --- | --- | --- |
| A. Free `find` (recommended) | `Error` drops `AnyRef`. `find` becomes `std.error.find[T < Error](error: Error) -> T?` in std, monomorphized at each call and written with `downcast_val` | every enum can be an error; `Error` stays dynamically safe; a newtype over `string` may now implement `Error` |
| B. Keep `find` as a method bounded `T < AnyRef & Error` | `Error` drops `AnyRef` | `find` cannot find enum errors, the common case |
| C. Relax dynamic safety for value `T` | erase value `T` to a box at the call | unsound for `List[T]` parameters, as above |
| D. Keep `Error < AnyRef` | none | enums cannot be errors; not viable |

Option A changes call sites from `failure.find::[FsError]()` to
`find::[FsError](failure)`. That is 4 calls in one fixture and 2 guide
pages. `root_cause` and `cause` stay methods. The spec's example
`Registry.lookup[T < Error]` becomes `lookup[T < AnyRef & Error]`.

`Inspectable.downcast[T < AnyRef & Inspectable]` keeps its bound. An enum
target uses `downcast_val`, as scalars, strings and tuples do
([`trait.downcast.val`](../../spec/lang/09-traits.md#r-trait.downcast.val)).
One fixture changes. `downcast_mut` is moot for enums, which are
immutable.

### 4.3 Trait Values, `Any` And `TypeId`

- **Erasure.** Converting an enum to a trait value or `Any` creates no
  identity, as decision B says for primitive, string and tuple boxes. A
  payload-free variant erases to an `i31ref` tag. A struct-layout enum
  erases to its struct, and a multi-layout enum to one immutable box.
- **`is` on erased values.** Decision B, item 5, makes `is` between two
  trait or `Any` values with `AnyVal` payloads unspecified. Enums join
  that rule. No new rule is needed.
- **The type-id test.** [wasm-layout.md §15.2](wasm-layout.md#152-values)
  compares type ids in `is` on `Any`, because payload-free variants of
  two enums erase to equal `i31ref` tags. After the change, an `i31ref`
  payload is always an `AnyVal` value, whose result is unspecified. So
  the test is no longer needed for enums. It can be dropped, unless an
  erased newtype over a reference needs it, which is a separate question.
- **`TypeId`.** No change. `TypeId::of::[E]()` and `runtime_type()` name
  types, not values.

### 4.4 Newtypes Over Enums

`type Code(Status)` now implements `AnyVal`. So
[`types.newtype.construct-value`](../../spec/lang/04-type-system.md#r-types.newtype.construct-value)
applies: `Code(s)` is a value, not a fresh mutable object.
[`types.newtype.construct-ref`](../../spec/lang/04-type-system.md#r-types.newtype.construct-ref)
and `types.newtype.unwrap-permission` no longer apply. One edge case: a
newtype over an enum with a `mut U` payload now unwraps to a readonly
enum. That is contrived code with a simple fix (wrap a data type), so it
needs no rule.

### 4.5 Shared Constructor Data

Shared data is per variant and never stored in the value
([`data.shared.not-stored`](../../spec/lang/08-data-and-enums.md#r-data.shared.not-stored)).
Its rule text says it adds nothing "to a value's size or identity". The
word "identity" goes. `expr.is.shared-data-canonical` is removed: with no
enum identity, it has nothing to say. Reading `StatusCode.NotFound._0` is
unchanged.

### 4.6 Mutable Enum Roots And Reassignment

- **Reassignment** of a binding, field or element that holds an enum is
  unchanged. It stores a different value.
- **`mut E`** stays. It still decides whether a non-generic payload
  declared `mut U` reads as `mut U`
  ([`types.path.enum-payload`](../../spec/lang/04-type-system.md#r-types.path.enum-payload)).
  `types.fresh.mutable-outer` still gives a fresh enum construction
  mutable access.
- Tuples instead have no `mut` form, and their elements carry their own
  permission. Making enums follow tuples would be a simplification, but a
  separate one; see open question 3.

### 4.7 Map Keys, Equality And The Host Boundary

- **Map keys.** No change. Enum keys already need explicit or derived
  `Eq` and `Hash` ([`types.map-key.user-enums`](../../spec/lang/04-type-system.md#r-types.map-key.user-enums)).
  hd has no identity map.
- **Equality.** No change. `==` never falls back to identity.
- **Host boundary.** No change. Enums already cross as tree values, and
  `mut` types are not boundary-safe for other reasons.

### 4.8 Canonical Payload-Free Variants

[`expr.is.canonical`](../../spec/lang/05-expressions.md#r-expr.is.canonical-data)
says a payload-free enum value is canonical for its variant. With no
enum identity, this is moot: two equal variants are the same value by
definition. The rule keeps only its fieldless-data half. `expr.is.none`
goes the same way. Constructing a payload-free variant still allocates
nothing, but that is now a layout fact, not a rule.

### 4.9 Fixtures And Std Code

Counted with a scan that skips comments and strings, on 2026-10-07.

| What | Where | Count | Action |
| --- | --- | --- | --- |
| `is` with an enum or optional operand | `spec/conformance/runtime/valid` | 24 assertion sites in 8 files | see below |
| an enum passed to a `T < AnyRef` bound | `typing/valid/value-category-traits.hd`, `typing/valid/anyref-bound-accepts-references.hd` | 3 calls in 2 files | move the enum calls to `by_value` |
| `downcast::[E]` with an enum `E` | `runtime/valid/error-downcast-through-inspectable.hd` | 1 | use `downcast_val` |
| `find::[E]` with an enum `E` | `runtime/valid/error-find.hd` | 4 | free-function call (option A) |
| `@error` enums and `impl Error for` an enum | fixtures | 46 `@error` enums and 8 `impl`s | none under option A |
| `impl Error for` an enum | `lib/std` | 9 of 10 error types | none under option A |
| `is` in std | `lib/std/format.hd` line 197 | 1 | keep; callers push only `AnyRef` nodes |
| `AnyRef` in std | `lib/std/error.hd` | 2 | drop from `Error`; rewrite `find` |
| `.hd` fixtures | `test/portable` | 0 | none: it holds only indexes |

The 8 runtime files:

| File | Sites | Action |
| --- | --- | --- |
| `payload-free-variant-canonical.hd` | 7 | delete; add a typing case `enum-identity.hd`, expecting `identity-requires-references` |
| `optional-identity.hd` | 5 | delete; its typing half joins `enum-identity.hd` (decision B already rewrote it) |
| `variant-identity-through-any.hd` | 4 | delete: the results become unspecified |
| `shared-data-variant-identity.hd` | 3 | delete; keep its shared-data reads in `enum-shared-data-per-variant.hd` |
| `reference-identity.hd` | 2 | drop the two `Status` clauses |
| `enum-shared-data-per-variant.hd` | 1 | drop the `is` assertion |
| `shared-enum-data-computed-once.hd` | 1 | drop the `empty is again` term and adjust the expected result |
| `error-find.hd` | 1 | drop the "same reference" case, or retarget it at the data error `DiskError` |

## 5. Codegen Gains

### 5.1 Layouts

This extends the table in [wasm-layout.md §15.2](wasm-layout.md#152-values).
"Payload values" counts Wasm values after slot sharing: payload fields of
different variants with the same Wasm type share a slot, and all
reference fields share one `anyref` slot, cast on match (mine).

| Enum shape | In locals and results | In fields and arrays | Allocation |
| --- | --- | --- | --- |
| payload-free variants only | `i32` tag | `i8` or `i16` | none (as today) |
| one variant with one non-null reference, one empty variant (`User?`, `Tree` with `Leaf` and `Node(Box)`) | `(ref null $T)`: null is the empty variant | same | none |
| the same with 2 or more empty variants | `(i32 tag, ref null $T)` | two fields | none |
| tag plus payload values at most 4 | `multi`: tag and slots | flattened fields; a structure of arrays in a list | none |
| larger, not self-recursive | one immutable struct | `(ref $E)` | one per construction, unless hoisted or scalar-replaced |
| self-recursive | flat or subtype structs, as today | `(ref $E)` | one per payload node, as today |

Examples: `Result[i32, ParseError]` with a payload-free `ParseError` is
`(i32, i32, i32)`. `Ordering` is an `i32`. A lexer's
`Token: Num(i64) | Ident(string) | Op(char) | End` is `(i32, i64, anyref)`.
`Json` from section 2 is `(i32, f64, anyref)`.

### 5.2 Other Gains

- **Constant hoisting.** An enum whose payloads are constants becomes an
  immutable global, so `.Err(ParseError.Empty)` or `Token.End` never
  allocates, even with a struct layout.
- **No escape analysis for identity.** Scalar replacement needed to prove
  that a payload variant never reached `is`, a field, an erasure or a
  call (§12.6). Now it only needs the layout.
- **Matching is cheaper.** A `multi` value's tag is a local. There is no
  `ref.cast` and no `struct.get`.
- **Decision B's optional and open question 23.1-7 merge into this rule.**
  `Result` gets the `multi` layout with no extra decision.

### 5.3 Effect On The Targets

These are estimates, not measurements. Slice 7 of the build order
measures the real layouts.

| Target ([goals.md](goals.md)) | Effect |
| --- | --- |
| `allocations`: 0 for counted loops, at most 1 for chains | Removes one allocation per enum construction that crosses a call: each fallible call returning `Result`, each `next_token`, each state-machine step. Iterator `next` was already free under decision B. User enum code in a loop reaches 0. |
| `runtime`: at most 1.5x Node, no case over 3x | For enum-heavy loops (lexers, interpreters, parsers), an allocation is roughly 5 to 20 ns with its share of GC, against a few ns of loop body. Removing it may gain 1.3x to 2x on those loops. Other programs see little change. |
| size | Slightly smaller: fewer struct types, no per-variant structs for small enums. A `multi` enum widens the function signatures it passes through. |

## 6. Costs And Risks

| Who loses | What | How bad | Workaround |
| --- | --- | --- | --- |
| Code comparing optional references, `a is b` with `a: User?` | a compile error | no use in std, 1 fixture | match both and compare payloads, or compare the data values |
| Graph algorithms whose nodes are enums | node identity | low: graph nodes are usually data, because they need mutable edges | make the node a data type, or give nodes ids |
| Memoization keyed by an enum node | `is`-based lookup | low: hd has no identity map, so this was already a linear scan | memoize by `Eq` and `Hash`, or by an id in a data wrapper |
| Interning enums for fast comparison | `is` as a fast equal | low | the compiler may hash-cons; derived `Eq` still works |
| `Error.find` users | method syntax | 4 calls in fixtures, 2 guide pages | `find::[T](error)` |
| `downcast` to an enum | method form | 1 fixture | `downcast_val` |
| Generic code `T < AnyRef` that took enums | a bound error | 2 fixtures | `T < Any`, or the caller erases explicitly |

**Risk: large enums copied by value.** The compiler chooses. Above 4 Wasm
values an enum stays one shared struct, so no large copy appears.

**Risk: `multi` enums in generic code.** Each layout gets its own
instance, as tuples already do. Polymorphic recursion falls back to
boxes, which is unobservable without identity.

### 6.1 Migration Of The Frozen Prototype

The prototype in `src/` is a test oracle and takes spec-sync fixes. Each
fix here is small:

1. `src/checker/value-categories.ts`: optionals and enums classify as
   `AnyVal` (two conditions move).
2. `src/checker/debug-print.ts`: stop the `enter` call for enums; add it
   for lists and maps.
3. `lib/std/error.hd`: drop `AnyRef` from `Error`; replace the `find`
   method with the free function. The prototype's direct-bound workaround
   on `find` goes away.
4. The fixture edits in section 4.9.

The prototype's enum layout stays a reference. Nothing can observe that,
so no `KNOWN_FAILURES` row is needed. If step 3 meets a prototype bug in
free generic functions over `Error`, that one case gets a row.

## 7. Recommendation

**Adopt.** It removes a special case (decision B's optional rules), makes
`AnyRef` mean exactly "has identity", unblocks the fastest enum layouts,
and costs one std API change and about 15 fixture edits. Confidence: high
that the semantics are right; medium that option A is the best answer
for `Error.find`.

### 7.1 Spec Changes For The Spec Pass

| Rule | File | New meaning |
| --- | --- | --- |
| `types.sealed.anyval-types` | 04-type-system.md | adds enums, optionals and `Result`: "These values have no identity." |
| `types.sealed.anyref` | 04-type-system.md | drops enums; keeps data, `List`, `Map`, functions, trait values, `Any`, suspensions, handles |
| `types.option.identity` | 04-type-system.md | `is` on an optional is an error, as on every enum. Error: `identity-requires-references`. |
| `types.option.identity.none`, `types.option.identity.some` | 04-type-system.md | removed |
| `types.option.any` | 04-type-system.md | unchanged; erasure creates no identity, by `expr.is.box` |
| `types.trait.safe.method-type-param-implied` | 04-type-system.md | rule unchanged; its example `T < Error` becomes `T < AnyRef & Error` |
| `types.trait.safe.convert-value` | 04-type-system.md | "a primitive or tuple value" becomes "an `AnyVal` value, such as a primitive, tuple or enum" |
| `expr.is.heap` | 05-expressions.md | drops "stored enum payloads" |
| `expr.is.primitive` | 05-expressions.md | names enums with primitives: an operand must implement `AnyRef`; add an enum line to the error example |
| `expr.is.canonical` | 05-expressions.md | keeps only fieldless data values |
| `expr.is.shared-data-canonical`, `expr.is.none`, `expr.is.some` | 05-expressions.md | removed |
| `expr.is.box`, `expr.is.box.identity` | 05-expressions.md | decision B's text, with enums added to primitives, strings and tuples |
| `data.enum.immutable` | 08-data-and-enums.md | adds: an enum value has no identity and implements `AnyVal` |
| `data.shared.not-stored` | 08-data-and-enums.md | "size or identity" becomes "size" |
| `trait.sealed.anyval-types`, `trait.sealed.anyref` | 09-traits.md | follow the 04 table |
| `trait.error.supertraits-anyref` | 09-traits.md | `trait Error < Display & Inspectable`; the ID keeps its spelling, as IDs never change |
| `trait.error.complete-reference` | 09-traits.md | the compiler supplies `Inspectable` only |
| `trait.error.not-anyref` | 09-traits.md | removed: an `AnyVal` type may implement `Error` |
| `trait.error.chain-methods` | 09-traits.md | `Error` declares `root_cause`; `find` moves to `std.error` as a function |
| `trait.downcast.val`, `trait.downcast.bound.examples` | 09-traits.md | add enums to the types that `AnyRef` excludes |
| `module.prelude.anyref`, `module.prelude.anyref-not`, `module.prelude.anyval-types` | 10-modules.md | move enums, optionals and payload-free variants to `AnyVal` |
| `std-error.find`, `std-error.find.same-reference` | std/error.md | `find::[T](e)`; the same reference only for an `AnyRef` target |
| `std-hash.derive.hash.enum` | std/hash.md | "variant identity" becomes "the variant", a wording fix |

Text without rule IDs that the pass rewrites, all in 04-type-system.md:
the Value Categories table and its two paragraphs on canonical and boxed
identity; the Shapes table, which moves enums and optionals to the
value-layout row; and the Composite Representation bullets on enums and
`T?`, which become "an enum has a value layout that the implementation
chooses; a self-recursive payload is a reference". The Why callout under
the Error Trait section and the Registry example in 09-traits.md change
with option A.

Fixtures: section 4.9.

### 7.2 Interaction With The Queued Spec Pass

The queued decision B list is in
[codex-review-response.md](codex-review-response.md#spec-changes-for-the-spec-pass).

| B item | Rule | With this change |
| --- | --- | --- |
| 1, 2 | `expr.is.some`, `types.option.identity.some` | superseded: removed, since `is` on optionals is an error |
| 3 | `types.option.identity` | superseded: an error, not a payload compare |
| 4 | `expr.is.box` | kept, and enums join it |
| 5 | `expr.is.box.identity` | kept, and enum payloads join it |
| 6 | `expr.is.box.distinct` | kept: removed |
| 7, 8 | an optional takes its payload's category | superseded: every optional is `AnyVal` |
| open question 23.1-7 | extend B to `Result` | answered: `Result` is a value |

**`is` on functions.** Functions stay `AnyRef` with unspecified identity,
and a direct `is` stays `unsupported-function-identity`. If the owner's
queued item moves functions to `AnyVal` too, `AnyRef` becomes exactly the
types whose values can change, plus trait values and `Any`. That is a
clean follow-up, but it touches `T < AnyRef` erasure of function
arguments, so it is not folded in here.

### 7.3 Open Questions For The Owner

1. **`Error.find`.** With enums in `AnyVal`, `Error` cannot extend
   `AnyRef`. Recommendation: option A, a free function
   `std.error.find[T < Error](error: Error) -> T?`; `Error` becomes
   `Display & Inspectable`. This is agent-originated.
2. **`is` on `Any` holding enums.** Decision B, item 5, leaves `is` on
   erased `AnyVal` payloads unspecified. Valhalla instead compares field
   values. Recommendation: unspecified, to match item 5 and keep `Any` a
   pointer compare.
3. **Should enums lose their `mut` form, like tuples?** Payloads would
   then carry their own permission, as generic payloads already do.
   Recommendation: not in this change; queue it for the complexity
   reducer.
4. **A helper to compare optional references?** Such as
   `same_ref[T < AnyRef](a: T?, b: T?) -> bool` in `std.option`.
   Recommendation: no, until real code asks for it.

## Sources

- Swift: [Enumerations, Recursive Enumerations](https://github.com/swiftlang/swift-book/blob/main/TSPL.docc/LanguageGuide/Enumerations.md);
  [Error Handling](https://github.com/swiftlang/swift-book/blob/main/TSPL.docc/LanguageGuide/ErrorHandling.md);
  [`AnyObject`](https://developer.apple.com/documentation/swift/anyobject).
- Rust: [`std::option`, Representation](https://doc.rust-lang.org/std/option/index.html#representation);
  [Enumerated types](https://doc.rust-lang.org/reference/types/enum.html);
  [Recursive types](https://doc.rust-lang.org/reference/types.html#recursive-types).
- Kotlin: [Inline value classes](https://kotlinlang.org/docs/inline-classes.html);
  [Equality](https://kotlinlang.org/docs/equality.html);
  [Sealed classes](https://kotlinlang.org/docs/sealed-classes.html).
- Java: [JEP 401, Value Classes and Objects](https://openjdk.org/jeps/401).
- C#: [Records](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/record).
- OCaml: [Stdlib, physical equality](https://ocaml.org/manual/5.2/api/Stdlib.html);
  [Real World OCaml, Memory Representation of Values](https://dev.realworldocaml.org/runtime-memory-layout.html).
- F#: [Discriminated Unions, Struct Discriminated Unions](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/discriminated-unions).
- Scala 3: [Enumerations](https://docs.scala-lang.org/scala3/reference/enums/enums.html).
- Haskell: [`System.Mem.StableName`](https://hackage-content.haskell.org/package/base-4.22.0.0/docs/System-Mem-StableName.html).
- Gleam: [Equality](https://tour.gleam.run/basics/equality/).
- Elm: [Runtime error when comparing functions](https://github.com/elm/compiler/issues/1145).
- Roc: [Functional](https://www.roc-lang.org/functional);
  [Reference Counting with Reuse in Roc](https://studenttheses.uu.nl/handle/20.500.12932/44634).
- WebAssembly GC: [Overview](https://github.com/WebAssembly/gc/blob/main/proposals/gc/Overview.md)
  (`i31ref`, `eqref`, `ref.eq`).
- In this repository: [wasm-layout.md §15.2](wasm-layout.md#152-values),
  [codex-review-response.md](codex-review-response.md#spec-changes-for-the-spec-pass),
  [open-questions.md §23.1](open-questions.md#231-open-questions-for-the-owner),
  [`types.sealed.anyref`](../../spec/lang/04-type-system.md#r-types.sealed.anyref-values),
  [`expr.is.box`](../../spec/lang/05-expressions.md#r-expr.is.box-values),
  [`trait.error.supertraits-anyref`](../../spec/lang/09-traits.md#r-trait.error.supertraits).
