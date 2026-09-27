# Typed Derivation: Survey And Design Options

Status: design exploration for roadmap area 2. Nothing here is accepted
language behavior. The specification, the prototype compiler, and the
decisions already recorded are unchanged. Questions for the owner are at the
end.


## Owner Decisions

Decided 2026-09-26:

1. **Design G: compiler-generated visitors** (supersedes the Design D
   recommendation below). A sealed, compiler-implemented `std.derive.Structure`
   trait is generated per data type and enum, with `visit[V <
   FieldVisitor](self, visitor: mut V)` and `build[B < FieldSource](source: mut
   B) -> Result[Self, B::Error]` making one statically dispatched, per-field
   generic call (`field[F]`) in declaration order (variants through
   `visit_variant` / `build_variant`). Libraries write visitors and a
   structural function once (for example `json.encode_structure[T <
   Structure]`), and a derivable trait names its structural implementation
   (`@derivable(encode_structure)`), so `@derive(json.Encode)` generates a
   concrete forwarding impl. After specialization the generated code is
   straight-line and typed: no per-field boxing or allocation. A field whose
   type fails the visitor's bound is reported at the derive site, naming the
   field. No packs, views, retyping, or blanket impls. Field metadata stays
   annotations, read from a static `FieldInfo`. `@derive(From)` for error
   enums uses the variant visitor. Still to design: how metadata can mark a
   field as not visited by one library's visitors (skip and custom codecs),
   and the exact `@derivable` form.
2. **Unify with annotations.** `Structure` also has a value-free
   `describe[D < Describer]()` that visits each slot with its static type.
   Chapter 14's aggregate annotators (`DataAnnotator`, `EnumAnnotator`, and
   possibly `FuncAnnotator`) are rebuilt on it, so `map_field` sees the
   field's static type and can use that type's own facet directly (removing
   the "no type-level function from field type to mapped output" limit).
   Visitors and annotators share one metadata vocabulary: they receive the
   existing `FieldShape` and read annotation member metadata. `@derive(T)`
   becomes the general rule for traits marked `@derivable`, replacing
   chapter 14's "compiler-intrinsic exception" wording; ordinary decorators
   still never change behavior.
3. **Skip and custom codecs: metadata changes generated calls.** A
   metadata type implementing `std.derive.Skip[V]` makes the generated
   `visit` omit the field for visitor type `V` (and `build` use the skip's
   default); one implementing `std.derive.With[V, Codec]` routes the field
   through `Codec`, so the visitor's bound applies to the codec's output type.
   This is checked statically, applies only in explicit `@derive` output and
   in annotators rebuilt on `describe`, and affects only the named visitor's
   library. Other libraries' visitors still see the field.
4. **`@derivable` maps each trait method to a generic function over
   `T < Structure`** (`@derivable(encode = encode_structure, schema =
   schema_structure)`); only the trait's package can declare it.
5. **Generated `visit`, `build`, and `describe` include private fields;**
   writing `@derive` in the owning module is the opt-in.

## Contents

1. [Problem](#problem)
2. [What hd Already Has](#what-hd-already-has)
3. [Survey](#survey)
4. [Requirements](#requirements)
5. [Design A: Structural Views And Ordinary Impls](#design-a-structural-views-and-ordinary-impls)
6. [Design B: Compile-Time Deriver Functions](#design-b-compile-time-deriver-functions)
7. [Design C: Runtime Shape-Driven Codecs](#design-c-runtime-shape-driven-codecs)
8. [Design D: Views For Behavior, Annotations For Data](#design-d-views-for-behavior-annotations-for-data)
9. [Worked Example: `std.json`](#worked-example-stdjson)
10. [Worked Example: An MCP Tool Adapter](#worked-example-an-mcp-tool-adapter)
11. [Comparison](#comparison)
12. [Recommendation](#recommendation)
13. [Language Additions The Recommendation Needs](#language-additions-the-recommendation-needs)
14. [Questions For The Owner](#questions-for-the-owner)
15. [Parse Log](#parse-log)

Spelling follows the current naming decision: collections are `List[T]` and
`Map[K, V]`, and every generic parameter, including a requirement-row
parameter, is uppercase.

## Problem

`@derive(...)` accepts only `PartialEq`, `Eq`, `PartialOrd`, `Ord`, and
`Hash` ([Traits](../spec/09-traits.md#comparison-traits)). Owner decision
TQ-13 keeps that set closed until one typed derivation protocol exists, shared
by `std` and libraries, with no ad-hoc additions meanwhile. Until then a
library cannot offer any of these:

```text
@derive(json.Encode, json.Decode)       # typed JSON codecs
pub data User:
    pub id: UserId
    pub email: string

@derive(testing.Arbitrary)              # property-test generators
enum Command:
    Put(key: string, value: i64)
    Delete(key: string)

@mcp.tool                               # a tool adapter from a function
pub fn get_user!(id: UserId) -> Result[User, NotFound] $ Users:
    pass

@derive(Builder)                        # builders
data Request:
    url: string
    timeout_ms: i32 = 30000
```

Annotations ([chapter 14](../spec/14-annotations.md)) come close but stop
short in three places:

1. **No typed field access.** A `DataAnnotator` receives `FieldShape` values.
   It can read names, positions, types as `TypeShape`, and metadata, but it
   cannot read a field of a `User` value or build a `User`.
2. **Uniform output only.** `Facet::Info` is one type for every target. A
   facet cannot produce `Decoder[User]` for `User` and `Decoder[Post]` for
   `Post`; the chapter says explicitly that the type system has no type-level
   function from member type to mapped output.
3. **No typed function value.** A `FuncAnnotator` receives a `FnShape`, not
   the function, so a tool adapter can describe `get_user` but cannot call it.

So today a JSON library must either hand-write every impl or erase to `Any`
and downcast, which the language deliberately does not support.

## What hd Already Has

Any design must fit what the specification and the owner have already fixed:

- **Traits and coherence.** Explicit impls, the orphan rule with trait-argument
  ownership (TQ-2), full-head overlap without bounds (TQ-28), no blanket impls
  over a bare parameter, and impl locality (TQ-17).
- **Trait delegation.** `impl Describe for Service by Logger` generates
  forwarding methods to an embedded part. Derivation is a close relative:
  "implement this trait by the type's structure".
- **Variadic packs.** `Ts...` type packs, per-element bounds such as
  `Ts... < Encode`, tuple types `(Ts...)`, and `pack.map` / `pack.map_list`,
  which type-check and instantiate one generic mapper call per tuple element
  ([chapter 12](../spec/12-variadic-generics.md)). This is the piece that makes
  field-wise generic code typed without a macro. Pack functions are
  specialized, and package interfaces already carry their bodies.
- **Static calls through a bound.** TQ-9: `T::decode(json)` is allowed under
  `T < Decode`.
- **Shapes and metadata.** `shape[T]()`, typed `.fields` / `.variants`,
  `FieldMetadata[T]` values checked against the field type, and
  `metadata[M]()` lookup.
- **Complete shape coverage.** TQ-23 adds `Mut`, `Trait`, `Any`, `Suspend`,
  and `Newtype` cases to `TypeShape`.
- **Newtype derivation.** TQ-11: newtypes may carry `@derive`, and the derived
  impl uses the base type's behavior.
- **Law partners.** TQ-12: partners are derived together or written together.
- **Project direction.** Explicit over inferred, locality, no action at a
  distance, and code that agents write and humans read. Chapter 12 states
  that pattern expansion and tuple mapping cover the accepted use cases
  "without creating a general compile-time metaprogramming language".

## Survey

| System | Mechanism | Descriptor | Field options | When generated code is checked | Generics and sums | Compile cost | Who writes derivers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Rust serde | Procedural macro emits `Serialize`/`Deserialize` impls into a fixed 29-type data model | Compile time, none at run time | `#[serde(rename, skip, default, flatten, tag, untagged, with)]` | After expansion; macro reports spanned errors, bound errors can point into generated code | Full generics, 4 enum representations | `syn`/`quote` cost per crate; widely accepted | Anyone (a proc-macro crate) |
| Rust facet | One `#[derive(Facet)]` builds a `SHAPE` constant; formats walk it | Static data read at run time | Attributes stored in the shape | Formats are ordinary code over shapes | Full | Lighter parser than `syn`; runtime slower than serde | Any consumer of `Shape` |
| Rust schemars, rmcp | Derive `JsonSchema`; rmcp `#[tool]` builds tool schemas from it | Compile time | Reuses `#[serde]` attributes plus its own | After expansion | Full | Macro cost | Anyone |
| Swift Codable | Compiler synthesizes `encode(to:)`, `init(from:)`, `CodingKeys` | Compile time | Hand-written `CodingKeys` rename or omit cases | Synthesized in the type checker | Enums with payloads since 5.5 | Cheap | Compiler only |
| Swift macros | Attached/freestanding macros over SwiftSyntax | Compile time | Macro arguments | Expansion is type-checked | Full | SwiftSyntax build cost was a major complaint until prebuilt binaries (2025) | Anyone |
| Kotlin kotlinx.serialization | Compiler plugin generates `KSerializer` plus `SerialDescriptor` | Generated code plus runtime descriptor | `@SerialName`, `@Transient` (needs default), defaults, `@Serializable(with=)` | Generated code checked with the module | Generics, sealed-class polymorphism | Plugin cost small | Plugin is JetBrains-only; custom serializers by hand |
| Scala 3 `derives` | Compiler synthesizes `Mirror.ProductOf`/`SumOf` (element types and labels as types); library `derived` uses `inline` | Compile time, types only | Mirror does not expose annotations; libraries fall back to macros | Per use site after inlining | Full | Heavy inline derivation reputed slow on large hierarchies | Any library |
| Haskell GHC.Generics | `deriving Generic` gives `Rep` built from `M1`, `K1`, `:*:`, `:+:`; classes use `DefaultSignatures` | Compile-time types | None on fields; aeson `Options` value; `DerivingVia` newtypes carry options | Generic instances checked once; use sites check constraints | Full | Large `Rep` types cost compile time | Any library |
| Haskell `DerivingVia` | Derive a class through a representation-equal newtype (`via Generically T`) | Compile time | Via type parameters | Checked through `coerce` | Full | Low | Anyone |
| MoonBit | Built-in `derive(Eq, Compare, Hash, Show, Default, Arbitrary, ToJson, FromJson, ...)` | Compile time | Derive arguments: `fields(x(rename=...))`, `rename_fields`, enum `style` | Compiler | Full | Cheap | Compiler only (no user deriver mechanism documented) |
| Zig | `@typeInfo` plus `inline for` over fields at comptime | Compile time | None; types add `jsonParse`/`jsonStringify` hooks | After unrolling, per instantiation | Tagged unions by hand | Comptime evaluation per instantiation | Anyone |
| Go | `reflect` plus struct tags `json:"name,omitempty"`; easyjson via `go generate` | Run time (cached) | String tags, unchecked | Never (tags are strings) | Generics recent; no sums | None / codegen step | Anyone |
| TypeScript zod | Schema-first; `z.infer` computes the type from the schema | Run time schema | Builder calls | Ordinary TS | Unions | None | Anyone |
| C# source generators | Incremental generator emits code; System.Text.Json context | Compile time, or reflection | `[JsonPropertyName]`, `[JsonIgnore]` | Generated file compiled normally | Full | Incremental; moderate | Anyone (Roslyn API) |
| Jai | `#run` plus `#insert` over `type_info` | Both | Not documented publicly | After insertion | Manual | Comptime execution | Anyone |
| Odin | Runtime `type_info` plus string field tags | Run time | `` `json:"name"` `` strings | Never | Manual | None | Anyone |

### Takeaways

1. **Two families work well.** Macro/codegen systems (serde, Codable,
   kotlinx, C#) give fast code and good ergonomics but check generated code
   late or rely on a compiler monopoly. Typed-representation systems (Scala
   Mirror, GHC.Generics, DerivingVia) let a library write one ordinary generic
   definition that the compiler checks once; their weak spots are annotations
   (Scala cannot see them) and type-level machinery (`Rep` combinators).
2. **hd has already built the part Scala and Haskell had to encode with types.**
   Packs with per-element bounds and `pack.map` give a flat, typed product
   view with no `:*:` nesting and no `inline`. Metadata values are already
   typed and checked against the field type, which is the part Scala lacks.
3. **Runtime reflection is simplest and weakest.** Go and Odin tags are
   unchecked strings; Rust facet shows a typed runtime shape can serve many
   formats but pays at run time and still needs typed access to be useful.
4. **Field options belong on fields, as typed values.** Every popular system
   converged on per-field rename, skip-with-default, and custom codec; serde's
   `with` and kotlinx's `@Serializable(with=)` show that a field-level codec
   override is needed, not only renames. Haskell's options-as-a-value is the
   least readable at the field.
5. **Skip changes the type obligation.** serde's `skip` lets a field whose type
   lacks `Serialize` exist; kotlinx requires a default. A typed design needs a
   typed way to say "this field is not encoded by this trait family".
6. **Schemas and codecs must agree.** schemars reads serde's attributes; rmcp
   builds tool schemas from them. One metadata vocabulary per library, read by
   all its derivers, is the proven shape.
7. **Compile speed follows the amount of generated text and search.**
   SwiftSyntax and heavy Scala inline derivation are the cautionary cases;
   GHC-style checked-once generic code plus cheap instantiation is the good
   case.
8. **Agent-writes, human-reads.** The user-facing part should stay one line
   (`@derive(json.Encode)`) plus field metadata. What that line means should
   be one short, printable, ordinary impl, not an expansion the reader has to
   reconstruct.

## Requirements

A design is acceptable when:

1. a library, not only `std`, can make `@derive(lib.Trait)` work;
2. the generated impl is ordinary and its obligations are checked at the
   derive site with errors that name the field;
3. the deriver itself is ordinary hd code, checked once where it is written;
4. field and variant metadata are typed values, and a field-level codec or skip
   can change the type obligation;
5. generic types, enums, embedding, newtypes, recursion, and GADTs have a
   stated rule or a clear rejection;
6. coherence follows the orphan rule, with no global registration;
7. no general compile-time evaluation is introduced;
8. tool adapters, schemas, and property generators share the same pieces.

## Design A: Structural Views And Ordinary Impls

A deriver is an ordinary generic `impl` of the library's trait for a
compiler-provided **structural view** type. `@derive(lib.Trait)` generates an
impl for the user's type that forwards to that view impl. This is Haskell
`DerivingVia` with a Scala-3-style flat product, built from hd packs.

### Declaring a deriver

`std.derive` declares the view types. Their members are compiler intrinsics:

```text
pub data Product[S, F]: pass             # F is a tuple type (T1, ..., Tn)
pub data Sum[E, C]: pass                 # C is a tuple of case types
pub data Case[E, P]: pass                # an ordinary variant; P is its payload view
pub data RefinedCase[E, P]: pass         # a GADT variant with a refined result

impl[S, Ts...] Product[S, (Ts...)]:
    pub fn info() -> DataInfo:           # name, doc, data-level metadata
        pass
    pub fn fields(self) -> (Field[Ts]...):   # typed values with FieldInfo
        pass
    pub fn slots() -> (Slot[Ts]...):         # typed, value-free descriptors
        pass
    pub fn build(values: (Ts...)) -> Product[S, (Ts...)]:
        pass
    pub fn value(self) -> S:
        pass
```

The library that owns the trait writes the deriver as an ordinary impl:

```text
impl[S, Ts... < Encode] Encode for Product[S, (Ts...)]:
    fn encode(self) -> Json:
        entries := pack.map_list(self.fields(), encode_entry)
        Json.Object(present_entries(entries))
```

The target starts with the type constructor `Product`, so this is a legal
target today. The package owns `Encode`, so the orphan rule allows it. No new
declaration form is needed for derivers.

### Using it

```text
@derive(json.Encode, json.Decode)
pub data User:
    pub id: UserId
    pub email: string
```

The compiler generates, in `User`'s module:

```text
impl json.Encode for User:
    fn encode(self) -> Json:
        view(self).encode()
```

where `view(self)` has type `Product[User, (UserId, string)]`. Associated
functions that return `Self` are forwarded the same way: the view is
**representation-identical** to `User` (same runtime value, no wrapper), so
the compiler may retype `Result[Product[User, ...], E]` as `Result[User, E]`
without a copy. This retyping is internal to generated forwarding code, like
GHC's `coerce`; it is not a user-visible conversion.

### What the deriver sees

- **Fields**: `Field[T]` values (`info: FieldInfo`, `value: T`) through
  `self.fields()`, and value-free `Slot[T]` descriptors (`info`, declared
  default as `(fn() -> T)?`) through `slots()`.
- **Types**: each field's static type, as a pack element with its bound.
  `TypeShape` is available only where the deriver is `reified`.
- **Annotations**: `FieldInfo`, `VariantInfo`, and `DataInfo` implement the
  sealed `ShapeMetadata`, so `info.metadata[json.Rename]()` works exactly as in
  chapter 14. Metadata stays typed at the field (`FieldMetadata[T]`).
- **Embedding**: an embedded field is one element whose `FieldInfo.embedded`
  is true; promoted members are not repeated, matching shapes. Whether JSON
  flattens it is the deriver's policy (Go flattens by default; serde needs
  `flatten`). `build` copies parts, as construction does.

### How generated code is type-checked

1. The deriver impl is checked once, in its package, as generic code.
2. At the derive site the compiler instantiates the forwarding impl and checks
   the view impl's bounds with `Ts` bound to the field types. A failure is
   reported against the field:

```
error[unsatisfied-derive-field]: cannot derive json.Encode for Session
  --> app/session.hd:1:9
1 | @derive(json.Encode)
  |         ^^^^^^^^^^^ json requires every field to implement json.Encode
4 |     pub token: SessionToken
  |         ^^^^^ SessionToken does not implement json.Encode
  = help: implement json.Encode for SessionToken, or add a json field adapter
          such as @json.skip(default=...)
```

The compiler knows the mapping from pack position to field, so it never has
to show `Product[...]` in the primary message.

### Generics

`@derive(json.Encode) data Page[T]` produces `impl[T < json.Encode]` by the
existing built-in rule: one `T < Trait` bound per declaration parameter used
in a field. The generated obligations (`List[T] < Encode`, `string? < Encode`)
must then follow from those bounds. When they do not, for example a field
`Set[T]` whose impl needs `T < Hash`, the compiler rejects the derive and
suggests writing the header by hand with the one-line structural body
(see [the expansion example](#worked-example-stdjson)).

### Enums and GADTs

`Sum[E, (Cs...)]` lists one case type per variant. An ordinary variant is
`Case[E, P]` with `project(E) -> P?` and `inject(P) -> E`; `P` is the
variant's payload view, itself a `Product` over an unnameable payload type.
Nested packs are never needed: the library states per-case obligations with a
helper trait, as Haskell does with `GEncode`, but flat:

```text
# Headers only; the bodies are in the worked example.
impl[E, P < Encode] EncodeCase[E] for Case[E, P]
impl[E, Cs... < EncodeCase[E]] Encode for Sum[E, (Cs...)]
```

A variant with a refined result (`IntLit(value: i64) -> Expr[i64]`) is a
`RefinedCase[E, P]`, which has `project` but no `inject`, because it cannot
be constructed at an arbitrary `Expr[T]`. A decoder that implements
`DecodeCase` only for `Case` therefore rejects such an enum through an
ordinary unsatisfied bound. A variant with its own generic parameters
(`If[T](...)`) has an existential payload and is rejected with
`unrepresentable-derive-view`.

### Field annotations

- **Renames, docs, descriptions** are plain metadata read at run time:
  `@json.rename("id")` is `impl[T] FieldMetadata[T] for Rename`.
- **Skip and custom codecs change the element type.** A metadata value whose
  type implements `std.derive.Adapter[T]` replaces the field's pack element
  with `Adapter::View`, and the view composes the conversions into `fields()`
  and `build`. `@json.skip(default=SessionToken::none())` makes the element
  `json.Omitted`, so `SessionToken` needs no `Encode` impl. An adapter applies
  only to derivations of traits declared in the adapter's own package, so
  `@json.skip` affects `json.Encode`, `json.Decode`, and `json.Schema` and
  nothing else. See [question 5](#5-how-does-a-field-skip-or-custom-codec-change-the-type-obligation).

### Coherence

- The generated impl lives in the target's module, so it obeys TQ-17.
- The deriver impl `impl Trait for Product[...]` can be written only by the
  trait's package, since `std` owns `Product`. So each trait has at most one
  structural derivation, from its owner. A third party cannot redefine how
  `json.Encode` derives.
- `@derive(X)` for a trait with no view impl is an error naming the missing
  `impl X for std.derive.Product[S, F]`.
- There is no standalone derive for a foreign type; that would be an orphan
  impl.

### Cost

Pack impls are specialized, so each derived type instantiates the library's
already-checked impl once: linear in the number of fields, no parsing, no
generated text, no search beyond ordinary trait resolution. Runtime cost is
that of hand-written code after specialization: field reads are direct, and no
boxing to `Any` occurs. Metadata lookups are per call unless the deriver
caches a plan, for example in a memoized facet.

### Assessment

Strong on requirements 1 to 7. The library-side type machinery (`EncodeCase`)
is more than a JSON author would write by hand, but it is written once per
trait, and the user side stays one line.

## Design B: Compile-Time Deriver Functions

A deriver is hd code that the compiler runs or unrolls per target, in the
style of Zig `inline for` or a restricted Swift macro. Hypothetical syntax; it
does not parse:

```text
derive Encode for data S:
    fn encode(self) -> Json:
        entries: Map[string, Json] = {}
        comptime for field in shape[S]().field_list:
            comptime if field.metadata[Skip]().is_none():
                entries[wire_name(field)] = self.@(field).encode()
        Json.Object(entries)
```

- **Deriver sees** the full `DataShape`, including metadata evaluated at
  compile time, and can branch on it.
- **Checking** happens after unrolling, per target. Errors appear inside the
  library's code at the user's derive site, the C++ template problem.
- **Generics** need the unrolled body to be generic in the target's
  parameters, or the compiler instantiates per use.
- **Field annotations** are easiest here: `comptime if` can drop a field so its
  type needs no impl, and can reject a rename collision at compile time.
- **Coherence** is as in A.
- **Cost**: a compile-time interpreter for a subset of hd, evaluation of
  metadata expressions during compilation, and per-target re-checking. It
  directly contradicts chapter 12's "no general compile-time metaprogramming
  language" and needs an `hd expand` tool before a human can read the result.

## Design C: Runtime Shape-Driven Codecs

Keep derivation out of the trait system. A library builds a runtime codec
through the existing annotation protocol, reading and constructing values
through erased members added to shapes (OPEN_ISSUES option 2). This parses
today; `field.reader()` and `downcast` are the hypothetical parts:

```text
data JsonCodec: pass

data Codec:
    encode: fn(Inspectable) -> Json
    decode: fn(Json) -> Result[Inspectable, DecodeError]

impl DataAnnotator for JsonCodec:
    type FieldTarget = FieldCodec

    fn map_field(self, field: FieldShape, type_metadata: AnnotationRef[Codec]) -> FieldCodec:
        FieldCodec { name: wire_name(field), read: field.reader(), codec: type_metadata }

    fn build(self, target: DataShape, fields: List[(string, FieldCodec)]) -> Codec:
        encode := fn(value: Inspectable) -> Json: encode_object(fields, value)
        decode := fn(json: Json) -> Result[Inspectable, DecodeError]: decode_object(target, fields, json)
        Codec { encode: encode, decode: decode }

pub fn decode[reified T < Annotate[JsonCodec]](json: Json) -> Result[T, DecodeError]:
    erased := JsonCodec::annotation(T).decode(json)?
    .Ok(erased.downcast[T]().expect("codec built for T"))
```

- **Deriver sees** shapes and metadata, as annotators do today.
- **Checking**: completeness is still static (`missing-child-annotation`
  already rejects a field type without the facet), but every value passes
  through `Inspectable` and a downcast. A mistake in the library is a run-time
  panic, not a compile error.
- **Generics** need `reified` everywhere a codec is fetched.
- **Enums/GADTs**: the builder sees variants; constructing a refined variant is
  a dynamic check.
- **Coherence**: annotation slots, as today.
- **Cost**: boxing of scalars, dynamic calls per field, first-use
  materialization. Fine for schemas and tool descriptions; poor for hot
  codecs.
- It gives no trait impl, so `List[User]` does not get `Encode` by
  composition, and `T < json.Encode` bounds do not exist.

## Design D: Views For Behavior, Annotations For Data

The hybrid keeps each existing mechanism where it is already good:

1. **Behavior** (codecs, generators, builders, hashing): Design A. A deriver
   is an ordinary impl for `std.derive.Product` / `Sum`, and `@derive(lib.X)`
   forwards to it.
2. **Uniform data** (a schema document, a UI form, a database column list):
   the existing annotation protocol, unchanged. A uniform `Info` is enough.
3. **Target-indexed data** is expressed as a trait with an associated
   function, not as a new annotation kind. `Strategy[Self]` is
   `trait Arbitrary: fn strategy() -> Strategy[Self]`, derived as in A. This
   makes OPEN_ISSUES option 3 (target-indexed `Info`) unnecessary, and so
   avoids generic associated types.
4. **Functions**: a typed function view, `fn_view(f)`, analogous to
   `shape_of(f)`, gives a generic function the callable, its typed parameter
   descriptors, and its declared defaults. A tool adapter is then an ordinary
   generic function, with no derivation at all.
5. **Newtypes** derive through the base type's impl (TQ-11), for every trait:
   `@derive(json.Encode) type Mile(i32)` forwards to `i32`'s impl, with the
   same representation-identical retyping. No view is needed.
6. **Metadata** is one vocabulary per library, read by all of its derivers, so
   `json.Schema` and `json.Decode` cannot disagree about a rename.

The worked examples below are Design D.

## Worked Example: `std.json`

### User side

This parses today (only `@json.tagged` on an enum and the library names are
new meaning):

```text
use std.json
use dep.mcp

@derive(json.Encode, json.Decode, json.Schema)
pub data Timestamps:
    pub created_at: i64
    pub updated_at: i64

@derive(PartialEq, Eq, Hash, json.Encode, json.Decode, json.Schema)
pub data UserId:
    pub value: string

pub data SessionToken:
    bytes: List[u8]

impl SessionToken:
    pub fn none() -> SessionToken:
        SessionToken { bytes: [] }

@derive(json.Encode, json.Decode, json.Schema)
pub data User:
    Timestamps
    @json.rename("id")
    pub user_id: UserId
    pub email: string
    pub display_name: string? = .None
    @json.skip(default=SessionToken::none())
    pub session: SessionToken

@json.tagged(field="kind")
@derive(json.Encode, json.Decode, json.Schema)
pub enum Event:
    @json.rename("signup")
    SignedUp(user: User)
    Renamed(user: UserId, display_name: string)
    Deleted(user: UserId)

@derive(json.Encode, json.Decode, json.Schema)
pub data Page[T]:
    pub items: List[T]
    pub next_cursor: string? = .None

fn round_trip(text: string) -> Result[Page[User], json.DecodeError]:
    value := json.parse(text)?
    Page[User]::decode(value)
```

Things to notice: `SessionToken` has no JSON impl and needs none, because the
skip adapter changes its element to `json.Omitted`; the embedded `Timestamps`
is one element with `embedded = true`; `Page[User]::decode` is a static call
through the derived impl.

### Library side

`json` is an ordinary package. The whole derivation is these impls; this parses
today, with `pack.try_map` and the `std.derive` members as the new parts:

```text
use std.derive.{Adapter, Case, Field, FieldInfo, Product, RefinedCase, Slot, Sum}

pub trait Encode:
    fn encode(self) -> Json
    fn omitted(self) -> bool:
        false

pub trait Decode:
    fn decode(json: Json) -> Result[Self, DecodeError]
    fn decode_missing() -> Result[Self, DecodeError]:
        .Err(DecodeError.Missing(field=""))

# Metadata: renames are plain values.
pub data Rename:
    pub name: string

pub fn rename(name: string) -> Rename:
    Rename { name: name }

impl[T] FieldMetadata[T] for Rename
impl VariantMetadata for Rename

# A field adapter changes the element type json derivations see.
pub data Omitted: pass

pub data Skip[T]:
    default: T

pub fn skip[T](default: T) -> Skip[T]:
    Skip { default: default }

impl[T] FieldMetadata[T] for Skip[T]

impl[T] Adapter[T] for Skip[T]:
    type View = Omitted
    fn to_view(self, value: T) -> Omitted:
        Omitted {}
    fn from_view(self, view: Omitted) -> T:
        self.default

impl Encode for Omitted:
    fn encode(self) -> Json:
        Json.Null
    fn omitted(self) -> bool:
        true

# Products: checked once, here.
fn wire_name(info: FieldInfo) -> string:
    match info.metadata[Rename]():
        .Some(rename) => rename.name
        .None => info.name

fn encode_entry[T < Encode](field: Field[T]) -> (string, Json)?:
    if field.value.omitted():
        return .None
    .Some((wire_name(field.info), field.value.encode()))

impl[S, Ts... < Encode] Encode for Product[S, (Ts...)]:
    fn encode(self) -> Json:
        entries := pack.map_list(self.fields(), encode_entry)
        Json.Object(present_entries(entries))

fn decode_slot[T < Decode](slot: Slot[T], entries: Map[string, Json]) -> Result[T, DecodeError]:
    match entries.get(wire_name(slot.info)):
        .Some(json) => T::decode(json)
        .None => match slot.default:
            .Some(make) => .Ok(make())
            .None => T::decode_missing()

impl[S, Ts... < Decode] Decode for Product[S, (Ts...)]:
    fn decode(json: Json) -> Result[Product[S, (Ts...)], DecodeError]:
        match json:
            Json.Object(entries) =>
                values := pack.try_map(Product[S, (Ts...)]::slots(), decode_slot, entries)?
                .Ok(Product[S, (Ts...)]::build(values))
            _ => .Err(DecodeError.Expected(what="object"))

# Sums: one helper-trait obligation per case, no nested packs.
pub trait EncodeCase[E]:
    fn encode_if(self, value: E, tag: string) -> Json?

impl[E, P < Encode] EncodeCase[E] for Case[E, P]:
    fn encode_if(self, value: E, tag: string) -> Json?:
        payload := self.project(value)?
        .Some(with_tag(payload.encode(), tag, case_name(self.info())))

impl[E, P < Encode] EncodeCase[E] for RefinedCase[E, P]:
    fn encode_if(self, value: E, tag: string) -> Json?:
        payload := self.project(value)?
        .Some(with_tag(payload.encode(), tag, case_name(self.info())))

fn encode_case[E, C < EncodeCase[E]](case: C, value: E, tag: string) -> Json?:
    case.encode_if(value, tag)

impl[E, Cs... < EncodeCase[E]] Encode for Sum[E, (Cs...)]:
    fn encode(self) -> Json:
        tag := tag_field(Sum[E, (Cs...)]::info())
        results := pack.map_list(Sum[E, (Cs...)]::cases(), encode_case, self.value(), tag)
        first_present(results)

pub trait DecodeCase[E]:
    fn decode_if(self, name: string, body: Json) -> Result[E, DecodeError]?

impl[E, P < Decode] DecodeCase[E] for Case[E, P]:
    fn decode_if(self, name: string, body: Json) -> Result[E, DecodeError]?:
        if case_name(self.info()) != name:
            return .None
        .Some(P::decode(body).map(fn(payload: P) -> E: self.inject(payload)))
```

`DecodeCase` has no impl for `RefinedCase`, so deriving `json.Decode` for a
GADT fails with an ordinary unsatisfied bound on the refined variant. Helpers
such as `present_entries`, `with_tag`, `split_tag`, and `first_present` are
ordinary functions omitted here.

### Schema from the same metadata

```text
pub trait Schema:
    fn schema(defs: mut SchemaDefs) -> SchemaNode

fn property[T < Schema](slot: Slot[T], defs: mut SchemaDefs) -> (string, SchemaNode, bool):
    (wire_name(slot.info), T::schema(defs), !slot.info.has_default)

impl[S, Ts... < Schema] Schema for Product[S, (Ts...)]:
    fn schema(defs: mut SchemaDefs) -> SchemaNode:
        name := Product[S, (Ts...)]::info().qualified_name
        if defs.reserve(name):
            properties := pack.map_list(Product[S, (Ts...)]::slots(), property, defs)
            defs.define(name, object_schema(properties))
        SchemaNode.Ref(name)
```

`wire_name` is shared with the codec, so the schema cannot drift from the
encoding. `defs.reserve` handles recursive types by reference, the same way
`AnnotationRef` handles recursive annotations.

### What `@derive` means, printed

The compiler's explanation command would print this for `Page[T]`. It parses
today, with `view` and `View` as intrinsics:

```text
impl[T < json.Encode] json.Encode for Page[T]:
    fn encode(self) -> Json:
        view(self).encode()

impl[T < json.Decode] json.Decode for Page[T]:
    fn decode(value: Json) -> Result[Page[T], json.DecodeError]:
        View[Page[T]]::decode(value).map(fn(page: View[Page[T]]) -> Page[T]: page.value())

# A hand-written header with a stronger bound, reusing the structural body.
impl[T < json.Encode + Hash] json.Encode for Bag[T]:
    fn encode(self) -> Json:
        view(self).encode()
```

The last impl is the escape hatch for bounds the default rule cannot state:
the user writes the header, and the body stays one line.

## Worked Example: An MCP Tool Adapter

A tool needs a JSON Schema for the parameters and result, and a function that
decodes arguments, calls the target, and encodes the result. With a typed
function view this is an ordinary generic function; no derivation and no new
annotator kind are involved. This parses today; `FnView`, `fn_view`, and
`pack.try_map` are the new parts:

```text
use std.derive.{FnView, Param}
use std.json.{Decode, Encode, Schema, SchemaDefs, SchemaNode}

pub data Describe:
    pub text: string

pub fn describe(text: string) -> Describe:
    Describe { text: text }

impl[T] ParamMetadata[T] for Describe

pub data Tool[Rq]:
    pub name: string
    pub description: string?
    pub input_schema: SchemaNode
    pub output_schema: SchemaNode
    pub invoke: fn!(Json) -> Result[Json, ToolError] $ Rq

fn param_schema[T < Schema](param: Param[T], defs: mut SchemaDefs) -> (string, SchemaNode, bool):
    (param.info.name, T::schema(defs), !param.info.has_default)

fn decode_param[T < Decode](param: Param[T], entries: Map[string, Json]) -> Result[T?, ToolError]:
    match entries.get(param.info.name):
        .Some(value) => .Ok(.Some(T::decode(value).map_err(bad_arguments)?))
        .None =>
            if param.info.has_default:
                return .Ok(.None)
            .Err(ToolError.MissingArgument(name=param.info.name))

pub fn tool[Ps... < Decode + Schema, R < Encode + Schema, Rq](
    view: FnView[fn!(Ps...) -> R $ Rq],
) -> Tool[Rq]:
    let defs: mut SchemaDefs = SchemaDefs::new()
    params := FnView[fn!(Ps...) -> R $ Rq]::params()
    properties := pack.map_list(params, param_schema, defs)
    output := R::schema(defs)
    invoke := fn!(arguments: Json) -> Result[Json, ToolError] $ Rq:
        entries := expect_object(arguments)?
        values := pack.try_map(params, decode_param, entries)?
        .Ok(view.call!(values).encode())
    Tool {
        name: view.info().name,
        description: view.info().doc,
        input_schema: object_schema(properties, defs),
        output_schema: output,
        invoke: invoke,
    }
```

The application registers the tool explicitly, as chapter 14 requires:

```text
pub fn get_user!(@mcp.describe("User identifier") id: UserId, include_deleted: bool = false) -> Result[User, NotFound] $ Users:
    match $.use(Users).find!(id):
        .Some(user) => .Ok(user)
        .None => .Err(NotFound.User(id=id))

fn tools() -> mcp.Registry[Users]:
    registry := mcp.Registry::new()
    registry.add(mcp.tool(fn_view(get_user)))
    registry
```

Notes:

- `call!` takes `(Ps?...)`: `.None` for a parameter means "use its declared
  default", which a plain function value cannot express because defaults are
  not part of function types.
- The requirement row `Rq` flows into `Tool[Rq]`, so the registry's row states
  exactly which providers the host must bind, matching the registered-boundary
  rule in [Modules](../spec/10-modules.md#wasm-boundary).
- A parameter whose type lacks `Decode` or `Schema` is an ordinary bound error
  at `mcp.tool(fn_view(get_user))`, naming the parameter.
- `fn_view` follows `shape_of`'s rules: its argument names a module-level
  function directly. A generic function needs a complete explicit type
  argument list (the Shape Intrinsic Coverage recommendation).
- A non-suspending target is weakened to `fn!` or served by a second
  overload-free function `tool_sync`; that is a library choice.

## Notable Use Case: Error Conversion

[Error Conversion](ERROR_CONVERSION.md) decided that `?` converts an error
through the target error type's pure `From[E]` implementation, at most once,
and that derived `From` implementations wait for this protocol. Application
error enums are therefore a primary client of derivation: each variant with
exactly one payload field wraps one source error, and its `From`
implementation is mechanical.

```text
@derive(From)                      # hypothetical: a std deriver over the Sum view
enum SyncError:
    Fs(error: FsError)
    Http(error: HttpError)
    Invalid(reason: string)        # string payload: no From generated, or opt out

fn sync!(p: Path) -> Result[void, SyncError] $ FsRead + Http:
    text := $.use(FsRead).read_text!(p)?      # From[FsError] for SyncError
    $.use(Http).post!(url, text)?             # From[HttpError] for SyncError
```

What the deriver needs from the protocol:

- the `Sum` view of the enum, with each variant's payload fields as a pack,
  so the deriver can select variants with exactly one payload field;
- one generated `impl From[P] for E` per selected variant `V(p: P)`,
  forwarding to the variant constructor (which is itself a function value
  for single-payload variants, per Error Conversion question 8);
- a per-variant opt-out in field or variant metadata (for example
  `@from.skip`), since not every single-payload variant wraps an error;
- an overlap diagnostic at the derive site when two variants carry the same
  payload type, since two `From[P]` implementations for one enum overlap
  (TQ-28) and `?` would have no unique conversion.

This matches Rust's `thiserror` `#[from]`, but stays a type-checked library
deriver in `std.error` rather than a procedural macro.

## Comparison

| | A: views | B: comptime | C: runtime | D: hybrid |
| --- | --- | --- | --- | --- |
| Library can add derivers | Yes | Yes | Yes (as facets, not traits) | Yes |
| Deriver checked once where written | Yes | No, per target | Yes, but erased | Yes |
| Derive-site errors name the field | Yes | Partly (errors inside deriver) | Yes, for missing facets only | Yes |
| Produces trait impls | Yes | Yes | No | Yes |
| Type-changing skip / codec | Adapters | `comptime if` | Runtime only | Adapters |
| Tool adapters | Needs a function view | Yes | Describe only, cannot call typed | Function view |
| New compile-time evaluation | None | Interpreter | None | None |
| Runtime cost | Hand-written level | Hand-written level | Boxing, dynamic calls | Hand-written level |
| New language surface | View types, `view`, `pack.try_map` | Comptime subset, new declaration | Erased reader, constructor, downcast | A plus `fn_view` |
| Fits "no metaprogramming language" | Yes | No | Yes | Yes |

## Recommendation

Adopt **Design D**. It answers the open issue with option 1 (open `@derive`
to library traits) but implements the protocol as ordinary impls over
compiler-provided structural views rather than a new deriver kind, replaces
option 3 (target-indexed `Info`) with traits that have associated functions,
and adds a typed function view for tool adapters. Option 2 (general typed
reflection) is not needed.

In short:

1. `@derive(lib.X)` on a data type or enum generates
   `impl lib.X for T` whose methods forward to `lib`'s own
   `impl X for std.derive.Product[T, (Ts...)]` or `Sum[...]`.
2. The trait's package, and only it, writes that view impl, in ordinary hd,
   checked once.
3. Field metadata stays typed; an `Adapter[T]` metadata value changes the
   element type for derivations of its own package's traits.
4. Newtypes derive through their base; GADT refined variants are
   project-only; variant-local generics are rejected.
5. Tool adapters use `fn_view(f)` and packs; schemas use the same metadata
   vocabulary as codecs.

The built-in comparison derivations can later be specified as `std` impls over
the same views, which fulfils TQ-13's "one protocol that `std` also uses"
without changing their current behavior.

## Language Additions The Recommendation Needs

1. `std.derive` view types (`Product`, `Sum`, `Case`, `RefinedCase`, `FnView`),
   descriptor types (`Field`, `Slot`, `Param`, `FieldInfo`, `VariantInfo`,
   `DataInfo`, `FnInfo`) implementing sealed `ShapeMetadata`, and the
   `Adapter[T]` trait.
2. Intrinsics `view(x)`, `View[T]`, and `fn_view(f)`, with the
   representation-identical retyping limited to generated forwarding.
3. A rule opening `@derive` to any trait with a view impl, plus the
   generic-bound rule and its diagnostics (`unsatisfied-derive-field`,
   `missing-derivation`, `unrepresentable-derive-view`).
4. `pack.try_map(items, mapper, extras...)`: each mapper returns
   `Result[Ri, E]`; the result is `Result[(R1, ..., Rn), E]`, stopping at the
   first error. It keeps chapter 12's left-to-right evaluation.
5. Container metadata (question 6), for enum tagging and rename-all policies.
6. The already-decided pieces: TQ-9 static calls through a bound, TQ-11 newtype
   derive, TQ-23 shape cases, and the grammar change that lets
   `decorated_decl` include `type_decl` (today `@derive` before
   `type Mile(i32)` is a `syntax-error`).

## Questions For The Owner

### 1. Which derivation mechanism?

- **A.** Structural views: a deriver is an ordinary impl for
  `std.derive.Product`/`Sum`; `@derive` forwards to it.
- **B.** Compile-time deriver functions unrolled per target.
- **C.** Runtime shape-driven codecs through annotations.
- **D.** A for behavior, annotations for uniform data, a function view for
  tools.

**Recommendation: D.** Checked-once derivers, field-level errors, no
compile-time interpreter.

```text
impl[S, Ts... < Encode] Encode for Product[S, (Ts...)]:
    fn encode(self) -> Json:
        Json.Object(present_entries(pack.map_list(self.fields(), encode_entry)))
```

### 2. Who may provide the derivation of a trait?

- **A.** Only the trait's package, which falls out of the orphan rule because
  `std` owns the views.
- **B.** Also a third package, through a registration, so alternative JSON
  policies can coexist.

**Recommendation: A.** One derivation per trait; alternative policies are
metadata or a different trait. No standalone derive for foreign types.

### 3. May a derived impl read and construct private fields?

- **A.** Yes. `@derive` is written by the type's owner in the owning module,
  so it is an explicit grant.
- **B.** Only when every field is `pub`, like boundary values.
- **C.** Read all fields; construct only when every field is `pub`.

**Recommendation: A**, with documentation that a derived decoder bypasses
constructor invariants, as serde's does. Boundary rules stay separate.

```text
@derive(json.Decode)
pub data Account:
    balance: i64        # private; decode may set it under A, not under B or C
```

### 4. What bounds does a derived impl on a generic type get?

- **A.** The built-in rule: `T < Trait` for each parameter used in a field;
  anything more is an error suggesting a hand-written header with
  `view(self).encode()` as the body.
- **B.** Infer the minimal bounds from the obligations.
- **C.** Always require a written header for generic types.

**Recommendation: A.** Explicit, same as the comparison derives, and the
escape hatch is one line.

### 5. How does a field skip or custom codec change the type obligation?

- **A.** Adapter metadata: a value implementing `std.derive.Adapter[T]`
  replaces the field's element type, for derivations of traits declared in the
  adapter's package.
- **B.** A per-trait override block at the type, such as
  `annotate json.Encode for Session: token = json.skip()`.
- **C.** Wrapper types in the field declaration (`token: json.Skipped[Token]`).
- **D.** None: skip is runtime-only and the field type must still satisfy the
  bound.

**Recommendation: A.** The option sits on the field, is typed against the
field type, and cannot leak into another library's derivation.

```text
@derive(json.Encode, json.Decode)
pub data Session:
    pub user: UserId
    @json.skip(default=SessionToken::none())
    pub session: SessionToken      # SessionToken needs no json impl
```

### 6. How is container-level configuration written?

- **A.** A metadata value before the declaration whose type implements a new
  `DataMetadata` or `EnumMetadata` marker, distinct from facets:
  `@json.tagged(field="kind")`.
- **B.** Derive arguments: `@derive(json.Encode(tag="kind"))`, as in MoonBit.
- **C.** None; only field and variant metadata.

**Recommendation: A.** It reuses the metadata model, is typed, and is shared
by `Encode`, `Decode`, and `Schema` automatically. A value implementing both
`Annotation` and a metadata marker is an error.

### 7. Drop target-indexed annotation `Info`?

- **A.** Drop it: target-indexed outputs are traits with associated functions
  (`fn strategy() -> Strategy[Self]`), derived through views.
- **B.** Add generic associated types so `type Info[T]` works.

**Recommendation: A.** No generic associated types; annotations keep their
uniform `Info`.

### 8. How do tool adapters get the function?

- **A.** A `fn_view(f)` intrinsic with typed parameter descriptors, declared
  defaults, and the requirement row; the adapter is an ordinary generic
  function.
- **B.** Extend `FuncAnnotator` so `build` receives the typed function.

**Recommendation: A.** `std` cannot state the library's bounds (`Decode`,
`Schema`) in a `std` annotator trait, and registration is already explicit.

```text
registry.add(mcp.tool(fn_view(get_user)))
```

### 9. Add `pack.try_map`?

- **A.** Yes: mapper results `Result[Ri, E]`, overall `Result[(Ri...), E]`,
  stop at the first error.
- **B.** No; decoders build a tuple of results and a `std` helper sequences it.

**Recommendation: A.** Decoding and argument parsing both need it, and it
adds no pack indexing or iteration.

### 10. How do newtypes derive library traits?

- **A.** Through the base type's impl, for every trait (TQ-11 extended).
- **B.** Through a `Newtype[S, B]` view, so each deriver decides.

**Recommendation: A.** One rule for built-in and library traits; serde also
encodes newtypes transparently by default.

### 11. GADT and existential variants

- **A.** A refined variant is a `RefinedCase` (project, no inject); a variant
  with its own generic parameters is `unrepresentable-derive-view`.
- **B.** Reject every GADT enum from derivation.

**Recommendation: A.** Encoding and schemas still work for GADTs; decoding is
rejected by an ordinary bound.

### 12. Should the built-in comparison derives move onto the views?

- **A.** Later, as a specification of their meaning (`impl PartialEq for
  Product[...]` in `std`), keeping the compiler's direct implementation.
- **B.** Never; they stay a separate intrinsic.

**Recommendation: A**, after the protocol is accepted, so TQ-13's single
protocol is literal. TQ-12's partner rule then applies to library traits that
declare partners.

### 13. Where are semantic metadata errors reported?

Examples: two fields renamed to the same key, or `@json.tagged` naming a field
that a variant also has.

- **A.** At the first call, as an ordinary panic, with a toolchain command that
  evaluates every requirement-free derivation plan at build time.
- **B.** A `Validate` hook that the compiler must run during compilation.

**Recommendation: A.** B needs compile-time evaluation, which D avoids.

### 14. Does this settle the direction for `Secret[T]`?

- **A.** Yes: `Secret[T]` implements no codec trait, so deriving over a secret
  field is a compile error until the field carries an explicit adapter.
- **B.** Decide separately when `Secret[T]` returns.

**Recommendation: A** as the direction, leaving the rest of `Secret[T]`
parked as decided.

## Parse Log

Every `text` code block above was extracted and checked with the chapter-02
reference parser (`spec/reference-parser`) on 2026-09-26. Only the Design B
block fails, as intended. Parsing checks syntax only; names such as `json`,
`view`, and `std.derive` members are not resolved.

| Block | Result |
| --- | --- |
| Problem examples | Parses. |
| Design A declarations, forwarding impl, case headers, question examples | Parses. |
| Design B deriver | Does not parse (new `derive ... for data S` and `comptime` syntax); hypothetical. |
| Design C codec | Parses; `reader()` and `downcast` are hypothetical members. |
| `std.json` user side | Parses. |
| `std.json` library side | Parses; `pack.try_map` and `std.derive` members are hypothetical. |
| Schema | Parses. |
| Printed expansion | Parses; `view` and `View` are hypothetical intrinsics. |
| MCP adapter and registration | Parses; `FnView`, `fn_view`, `pack.try_map` are hypothetical. |
| Probe, not shown: `@derive(json.Encode)` before `type Mile(i32)` | `syntax-error`: `decorated_decl` excludes `type_decl` (TQ-11 is not yet applied). |
| Probe, not shown: a decorator before `trait` | `syntax-error`; not used by the recommendation. |

Two reference-parser findings from this exercise:

- A generic *type* declaration cannot take a pack (`data Product[S, Ts...]`
  is a `syntax-error`, because `type_parameter` has no `...`), so views are
  written `Product[S, (Ts...)]` with a tuple-typed parameter.
- In a multiline parameter clause, a parameter with a default on its own line,
  such as `    include_deleted: bool = false,`, is reported as `missing-let`
  by the parser's contextual check. The MCP example keeps its signature on
  one line for that reason. This looks like a false positive worth an area 1
  finding.

The prototype compiler (`bin/hd.js`) does not implement `pack.map`
(`unknown-name 'pack'`), so none of the pack-based examples could be
type-checked there.
