# Typed Derivation: Survey And Design Options

Status: design record for roadmap area 2. Typed derivation is fully
decided by owner decisions M1-M24 (2026-09-27), M25 (2026-09-28, applied
the same day), M26 (2026-09-28, applied the same day, its readings awaiting
confirmation), M27 and M28 (2026-09-28, applied to the spec the same
day), and M29 (2026-09-28, applied the same day), tested by three stress tests
([round 1](DERIVATION_STRESS_TEST.md), [round 2](DERIVATION_STRESS_TEST_2.md),
[round 3](DERIVATION_STRESS_TEST_3.md)). M1-M24 are applied to the
specification in [Typed Derivation](../spec/14-annotations.md#typed-derivation),
with grammar in [02](../spec/02-grammar.md#traits-and-implementations) and
rules in [08](../spec/08-data-and-enums.md#typed-derivation-of-data-and-enums)
and [09](../spec/09-traits.md#derived-implementations). The prototype
compiler implements them by lowering each derivation to an ordinary
implementation ([src/README.md](../src/README.md)); its remaining gaps are
rows of [KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv), mostly
`K1` (the shape intrinsics) and `F-250`, plus one `M29` fixture that needs
package roles, and [Still Open](#still-open-after-the-prototype-pass) lists what the pass found
unspecified. The spec is the
accepted behavior; this record is history and rationale.

`@derive` lists the intrinsic comparison traits and traits with a `by
Structure` template (M18). It does not include `Error`: an error type uses
the separate `@error` intrinsic
([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)). The
[current example](#current-design-full-example-m1-m21) and
[rules](#current-rules-m1-m21) show M1-M21 together, and
[Remaining Open](#remaining-open) lists what is still undecided. Decisions
1-12, the survey, and its questions below are kept as history.


## Owner Decisions

Decided 2026-09-26 (decisions 1-12, superseded by M1-M21 below and kept
as history):

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
6. **Derived bounds are inferred from the fields.** For each method the
   trait maps through `@derivable`, every visited (not skipped) field
   contributes the obligation `F < B`, where `B` is the bound its visitor or
   source puts on `field[F]`. Each obligation is traced to type-parameter
   bounds through the unique impl per trait and type constructor (no blanket
   impls, no overlap): a parameter gives a bound, a concrete type is checked
   directly, a constructor is replaced by its impl header's requirements, a
   trait value satisfies its own trait's bounds, and the type being derived
   is assumed to hold (coinductive recursion). Phantom and method-only
   parameters get no bound. An obligation on a function type, a missing
   impl, or an associated projection is an error naming the field. Tooling
   shows the inferred header; a hand-written header whose body calls the
   structural function is the escape hatch. The rule is the same for every
   derivation (codecs, comparison and hash traits rebuilt on `Structure`,
   generators, `From`).
7. **Facets fold into derivation.** A facet's `info()` is derived like any
   trait method: the facet's package provides a structural describer over
   `Structure::describe` (typed per field; a child's information comes
   through the field type's own `Annotate[Facet]` bound, or through
   `annotation_ref` for recursive types). Chapter 14's aggregate annotator
   protocols (`DataAnnotator`, `EnumAnnotator`) with uniform targets, the
   field-override assignment grammar, and the local `fn build` override are
   replaced: a field's result override becomes metadata implementing
   `Replace[D]` (carrying the value), and a whole-result rewrite is a
   hand-written `impl Annotate[F] for T` that calls the structural function.
   Kept: shapes, member metadata, `Annotation`/`Annotate`/`Info`,
   `Facet::annotation(T)` and `annotation_ref`, the per-instance lazy
   memoizing registry with cycle detection, the requirement-free rule (an
   empty row on `info`), coherence and the root-application orphan
   exception, and the `@Facet` decorator as sugar for `@derive(Facet)`.
   `@derive` is the single documented rule; decorators never change
   behavior.
8. **Configured facets use container metadata.** Configuration such as
   `@tool.config(strict=true)` is metadata on the declaration (new
   `DataMetadata`, `EnumMetadata`, `FnMetadata` markers), read by the
   describer from the declaration's shape; the same concept covers enum
   tagging and rename-all policies.
9. **Foreign targets use a standalone derive.** `derive Validation for
   dep.models.User` is allowed where the ownership rule allows the impl, plus
   the root-application orphan exception. A derive written outside the
   type's package sees only public fields; a private field that would be
   visited is an error.
   Still open: function targets (tools), previously `FuncAnnotator`; see
   [Nominal Function Types](FN_TYPE.md#per-declaration-data-for-tools).
10. **`Annotation` and `Annotate` are removed.** A facet is an ordinary
    derivable trait with an associated function (for example
    `trait Validate: fn validator() -> Validator`, derived through
    `@derivable`); `Validation::annotation(User)` becomes
    `User::validator()`. Memoization and recursion move to one general
    standard-library cache for derived associated functions: a value built
    lazily once per (trait, type) per program instance, with `Ref[T]`
    deferred references and cycle detection replacing `AnnotationRef`.
    Function targets are decided with the function-item question. (Applied
    2026-09-27: the facet protocol is removed from
    [Annotations](../spec/14-annotations.md), and the cache is listed in
    [STDLIB](STDLIB.md#derived-function-cache).)
11. **One block per concern; derivations are never merged in one place.**
    A derivation is requested with a block `derive X for T:` whose field
    and container options are metadata scoped to that derivation only
    (other derivations never see them). A block names one trait or one
    library-declared group, never traits from different libraries. Inline
    `@derive(X)` is only shorthand for an option-less block. General facts
    every concern may read (docs, shared metadata) stay on the declaration.
    Blocks live in the type's module; a foreign type may be targeted under
    the ownership rule plus the root-application orphan exception, seeing
    only public fields.
12. **Library-declared groups.** A library declares which of its traits are
    derived together and share one option vocabulary (for example
    `json.Codec` for `Encode`, `Decode`, and `Schema`); users write
    `derive json.Codec for User:`. Still to design: the group declaration
    form, and whether one member of a group may be derived alone.

**Rethink in progress (2026-09-27): a minimal design is replacing decisions
1-12.** Agreed so far: (M1) the compiler adds only the sealed `Structure`
trait (the zero-cost visitor: `visit`, `build`, `describe`) for every data
type and enum; (M2) an impl body may name fields, variants, or parameters as
`name = [facts]`, where facts are typed metadata values that only that
impl's derived methods read; facts are purely descriptive, with no
compiler-interpreted fact (so no view substitution, adapters, or markers);
a field whose type does not support the derivation needs a changed data type
or a hand-written method. Under discussion: how a trait opts in as derivable
and how a type opts in to a derived impl.

(M3, decided 2026-09-27) Facts may be declared on the data declaration and
are shared by every impl. An impl body adjusts them per field, variant, or
parameter with three line forms and no new keywords: `f += [facts]` extends
the declaration's facts for this impl (a fact whose concrete type is already
present is an error; use `=` to change it); `f = [facts]` replaces them for
this impl; `f = pass` leaves the member out of this impl's generated calls,
so its type need not support the derivation, and a construction uses the
member's declared default (an error when there is none). Members without a
line keep the declaration's facts. This supersedes the "no
compiler-interpreted fact" wording above: `= pass` is the one form that
changes generated code.

(M4, owner proposal 2026-09-27, under discussion) Derivation takes a
visitor, so users can customize it (for example, camelCase for every field)
without the library anticipating it. The library author writes an ordinary
impl for a wrapper, `impl[T] Trait for Derive[T, V]:`, whose methods get the
visitor with `Self::visitor()` and consume it however they choose (for
example `Self::visitor().visit(self)`). The library author decides the
visitor's type and how it is used; the compiler provides only `Structure`.
Prior art: Haskell's `DerivingVia` with `Generically`. Open points: how
`Self` in non-receiver positions (such as `decode -> Result[Self, E]`)
forwards between `User` and `Derive[User, V]`, how M3's per-impl member lines
reach the wrapper's impl, and how `visitor()` constructs a `V`.

(M5, decided 2026-09-27) Two tiers of customization. Tier 1: a library
annotation on the declaration, such as `@json(case: .Camel) data T:`, is the
easiest way to derive; the annotation is a plain function returning a
visitor. Tier 2: an explicit `impl Trait for T by Derive[T]:` with M3 member
lines. The visitor stays a visitor: no map/reduce split (a fold would still
need build and variant halves, and map-then-reduce allocates). `Structure` is
opt-in, never implemented for every type automatically. An annotation
function is evaluated at compile time and must be requirement-free and
non-suspending.

(M6, decided 2026-09-27) `Structure` is a sealed trait (09 Sealed Traits):
`impl Structure for T` is rejected, and a type gets it only by opting in to a
derivation (tier 1 or tier 2). A tier-1 annotation and a tier-2 impl of the
same trait for the same type is an error. Per-member customization is
metadata (facts) only; custom behavior for one member means changing that
member's type. Direction for how an annotation knows its traits: the
returned visitor's type carries that knowledge.

(M7, decided 2026-09-27) Nested members use their own derivation: a parent's
visitor never propagates into a member's type, as in serde and Go. Recursion
is the visitor's per-member bound (`F < json.Encode`) calling `F`'s own impl.
The owner asked to rethink the `Derive[T]` wrapper because forwarding `Self`
between `T` and the wrapper needs coercions (Haskell's roles); a no-wrapper
alternative is under discussion.

(M8, decided 2026-09-27; supersedes the `Derive[T, V]` wrapper of M4)
Derivation templates, no wrapper, so `Self` is always the user's type and
nothing is forwarded:

```text
# std
pub trait Derivable[V]:
    fn visitor() -> V

# library json
pub type Json = Derivable[Style]                    # transparent alias
impl[T] Json for T by Structure:                    # default visitor
    fn visitor() -> Style:
        Style()
impl[T < Json] Encode for T by Structure:           # templates
    fn to_json(self) -> string:
        # Self::visitor(), self.visit(...)
impl[T < Json] Decode for T by Structure:
    fn from_json(text: string) -> Result[Self, DecodeError]:
        # Self::visitor(), Self::build(...)

# user, tier 2: one block per concern
impl json.Json for Order by Structure:
    fn visitor() -> json.Style:                     # override only to customize
        json.json(key: .Some(legacy_key))
    total_cents = [json.rename("total")]            # seen by Encode and Decode
    cache = pass

# user, tier 1: sugar for the same block, visitor() returning the value
@json(case: .Camel)
data User: ...
```

- `impl[T] Trait for T by Structure:` declares a template; it never applies
  by itself, so it cannot overlap a hand-written impl. Only the trait's module
  may declare it, so there is at most one per trait. Inside, `self` has
  `Structure`. `Structure` may bound nothing else (`fn f[X < Structure]` is
  rejected), and `impl Structure for T` stays `sealed-trait-implementation`.
- `impl Trait for X by Structure:` applies the template. Its body overrides
  template methods like default methods and carries M3 member lines, which
  edit the `Structure` that instantiation sees.
- The visitor travels only through `Derivable[V]`, a template *bound*, not a
  supertrait, so hand-written impls (`impl json.Encode for Money`) need no
  visitor.
- Opting in to `Derivable[V]` by `Structure` (the tier-2 block, or a tier-1
  annotation returning `V`) derives every template in `V`'s package that
  requires `Derivable[V]`. Tier 1 is exactly the tier-2 block with
  `visitor()` returning the annotation's compile-time value.
- One block means member lines are written once, so encode and decode cannot
  drift. A subset (only `Encode`) cannot be derived through the block; the
  trait left out is hand-written.

Open (gap 1): std's `Visitor::member[F]` cannot carry each library's bound
(`F < json.Encode`, `F < db.Column`). Options: (1) a visitor impl of the
sealed `Visitor`/`Source` may strengthen `F`'s bound, becoming the obligation
"every visited member implements it", checked at the opt-in site and naming
the member (`= pass` exempts it); templates must call `visit` with a concrete
visitor (recommended; what decision 6 assumed); (2) trait-kinded generic
parameters (rejected by the owner: too complicated); (3) associated type packs
from chapter 12 (enums need a pack of packs, and decode infers only from the
return type); (4) a trait value per member, `Visitor[D]` with
`member(m, value: D)` (no special rule for encoding, but decode cannot
produce a value from a trait value, and converting a value without identity
to a trait value allocates a box, per 04 Runtime Values).

Cost note: hd compiles generic code once per shape with dictionaries
(04 Shapes and Generic Code), so "zero cost" here means no per-member
allocation and constant dictionaries, not full monomorphization. Option 1
and option 4 both make one indirect call per member; only option 4
allocates.

(M9, decided 2026-09-27; closes gap 1) Option 1: strengthened member bounds.

```text
# std.structure (sealed)
pub trait Visitor:
    type Error
    fn member[F](mut self, m: Member, value: F) -> Result[(), Self::Error]
    fn variant(mut self, v: Variant) -> Result[(), Self::Error]
pub trait Source:
    type Error
    fn member[F](mut self, m: Member) -> Result[F, Self::Error]
    fn variant(mut self, choices: List[Variant]) -> Result[Variant, Self::Error]

# library json
impl Visitor for Encoder:
    type Error = EncodeError
    fn member[F < Encode](mut self, m: Member, value: F) -> Result[(), EncodeError]:
        # value.encode(...)
impl Source for FieldSource:
    type Error = DecodeError
    fn member[F < Decode](mut self, m: Member) -> Result[F, DecodeError]:
        # F::decode(...)
```

1. An impl of the sealed `Visitor` or `Source` may strengthen `member[F]`'s
   bound; no other trait may.
2. `member` may be called through a generic `V < Visitor` / `S < Source`
   only by the compiler-generated `visit` and `build`; such a call in user
   code is an error. A call on a concrete visitor type applies its own bound.
3. Templates call `visit` and `build` with a concrete visitor or source, so
   the generated body is checked at the opt-in site, where member types and
   the strengthened bound are both known. The obligation "every visited
   member satisfies the bound" is reported there, naming the member; a
   `= pass` member is exempt.
4. Cost: the generated code is typed per member; each call passes a constant
   dictionary and an unboxed value, so there is no per-member allocation.

Explored and rejected on the way (2026-09-27): trait-kinded parameters (too
complicated); associated type packs (enums need nested packs, decode infers
only from return types); trait values per member, including `Field[F]` and
`Slot[F]` with `Fill[P, E]` (zero typing rules, but decode needs a typed slot,
zero cost needs a non-escape rule plus mandatory specialization that compiles
back to option 1's code, and dynamically safe member traits cannot suspend);
treating trait values as bounds language-wide (generics stay: same-type
parameters, typed returns, receiverless constructors, homogeneous
collections).

(M10, decided 2026-09-27; supersedes M8's `Derivable[V]`, the `Json` alias,
and the fan-out of one opt-in to several traits) Member lines are local to
the impl they are written in, as M3 said: they edit only the `visit` and
`build` calls made inside that impl's bodies, and whatever visitor or source
those bodies pass receives the edited members. Another impl of the same type
(for example `db.Row`) sees the declaration facts only. Consequently a
library that needs both directions to agree puts them in one trait: json has
one trait `Json` with `encode` and `decode`, and the visitor hook is an
ordinary default method `fn visitor() -> Style` on it. One opt-in derives
exactly one trait. Cost: a type that can only be encoded must still write a
`decode` (for example, one that returns an error); derivation never
supported encode-only.

(M11, decided 2026-09-27; supersedes M10's single `Json` trait, keeps
M10's locality rule) Rust avoids duplicated configuration because serde's
attributes sit on the struct, shared by the `Serialize` and `Deserialize`
derives. hd's tier 1 is that place: an annotation and the declaration facts
are shared by every trait the annotation derives. Tier 2 is for control, which
is often per direction (serde itself has `skip_serializing`,
`skip_deserializing`, and `rename(serialize = ..., deserialize = ...)`), so
tier 2 deliberately splits the traits: json keeps separate `Encode` and
`Decode` traits, each with its own template and a default `visitor()`; a
tier-2 block is written per trait, and repeating member lines across them is
accepted. A type may be encode-only (a hand-written `Encode` without
`Decode`).

(M12, decided 2026-09-27, from the [stress test](DERIVATION_STRESS_TEST.md))
P6: a derived impl for a generic type gets `T < Trait` for each type
parameter that appears in a visited member; recursion is checked
coinductively (`Tree[T]` may assume its own impl while checking its
members); when a member needs more (a `Set[T]` member needs `T < Hash`), the
error suggests a tier-2 block with an explicit header. P4: there are no
marker templates: a template must have a body that visits or builds, so a
bodiless `by Structure` template is an error. The owner considers today's
marker `Eq` wrong: `Eq` should carry a method, so it is derivable and
never needs an empty hand-written impl. Decided (Swift model): one `Eq` with
`fn eq(self, other: Self) -> bool`; `PartialEq` is dropped; floats implement
`Eq` with IEEE semantics (`NaN != NaN`, a documented law exception);
`PartialOrd` and `Ord` stay, so floats are `Eq + PartialOrd` but not `Ord`.
Applied as EQ-1 in [Comparison Traits](../spec/09-traits.md#comparison-traits). Still under discussion: P1-P3 (the core walk), P5 (where
configuration lives), and the rest of the stress test's problems. (Closed
since: P1-P3 by M14 and M19, P5 by M13, the rest by M15-M21.)

(M13, decided 2026-09-27; P5 of the stress test; supersedes the
`visitor()` hook of M10 and M11) Derivation configuration is a type-level
fact, not a trait member. A type-level annotation produces a fact on the
type, exactly as a member annotation produces a fact on a member
(`@json(case=.Camel)` on `data User`). A tier-2 block edits the type-level
facts with an ordinary member line named `Self` (`Self = [facts]`,
`Self += [facts]`), local to that block like every member line. A template
reads the facts through `Structure` (for example
`T::describe().facts.find[Style]()`) and falls back to its own default when
none is present. No hook, no parameter, no empty impl; derived traits stay
dynamically safe; nothing collides; configuration may differ per direction.
Accepted cost: a missing or foreign fact silently means the default, not a
compile error. Still open: how a tier-1 type-level annotation selects the
templates it opts in to (stress test P13), now that no hook returns its
type. (Decided by M18 P13: `@derive` selects, and annotations only attach
facts.)

(M14, decided 2026-09-27; closes stress-test P1, P2 and P3; replaces the
value-passing `visit` and `member(m, value)` protocol of M9) Typed member
handles. For each opted-in type the compiler generates only:

- `facts() -> Facts`: the type-level facts (M13);
- `walk[W < Walker[Self]](w: mut W) -> Result[void, W::Error]`: a value-free
  walk that passes one handle per member, and for an enum first asks
  `w.variant(v)` whether to enter each variant;
- `build[S < Source[Self]](s: mut S) -> Result[Self, S::Error]`;
- one constant handle per member, `Field[S, F]` (`info: Member`,
  `get(s: S) -> F`, `has_default() -> bool`, `default() -> F?`), and one per
  variant, `Variant[S]` (`info`, `holds(s: S) -> bool`).

```text
pub trait Walker[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> bool          # true: walk its members
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]

pub trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]
```

M9's strengthened-bound rule now applies to `Walker::member` and
`Source::member`. Everything else is library code over handles: encoding is
`walk` plus `h.get(value)`; `Eq`, `Ord`, and diff hold two values and enter a
variant only when both hold it; clone, patch, and shrinking are `build` with
a source that reads an old value; schemas, CLI help, and tool schemas are
`walk` with no value (P2); decoders fall back to `h.default()` when input
lacks a member (P3). An enum payload handle's `get` on a value of another
variant panics; generated `walk` calls `member` only for entered variants.
Handles are constants and `get` is one projection, so there is still no
per-member allocation (`default()` allocates `.Some` for reference-shaped
members; `has_default()` does not). Prior art: GHC.Generics, Scala 3
`Mirror`. Still open: whether `T -> U` mapping between two types is in
scope, how `= pass` members appear in `walk` and `build`, and the exact
handle API. (Since: the handle API is written by M18 P11g, M20, and M21;
`= pass` follows M3; `T -> U` is still open.)

(M15, decided 2026-09-27; closes stress-test P12 and P14) Typed derivation
deliberately does not cover impl families: one impl per variant or member,
each a different trait instantiation keyed by the member's type. A template
gives one trait instantiation per opt-in. Error derivation, the one case that
needs a family (`From` per variant) together with `Display` and
`Error::cause` from the same per-variant markers, is the compiler intrinsic
`@derive(Error)`, hd's `thiserror`, with the variant markers `@message`,
`@from`, `@source`, and `@transparent` (since respelled as the `@error`
annotation, not part of `@derive`); see
[Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions). For
other libraries' facts, the fact check hook is accepted (P14): a fact type
may define a compile-time `check` against the member or variant it is
attached to, run at the opt-in site. Its exact form is not yet designed.

(M16, decided 2026-09-27; closes the embedding part of stress-test P11)
An embedded part is one member. `walk` and `build` pass a handle
`Field[S, P]` for the embedded field, named by the embedded type as in
08 (`post.Timestamps`), with `info` marking it `embedded`; `get` returns the
part, a copy, per value semantics (corrected by M21: the part itself, per
08 `data.part.alias`). Flattening or nesting is a library
policy read from that mark (json may flatten like Go's `encoding/json`, or
nest like serde's default); the language never flattens, because promotion's
depth shadowing would make a flattened walk follow lookup rules and leave
`build` unable to fill a shadowed member. The part's insides come from its
own derivation, like any nested type (M7). Related 08 rule, decided the same
day: an embedded field must name a data type (`embedded-non-data`
otherwise).

(M17, decided 2026-09-27 while reviewing the parked design; closes
stress-test round 2 R1 and round 1 P17) A handle's `get` follows the
field-read rule: `h.get(s)` is viewpoint-adapted exactly like `s.field`
(04 Mutable Paths), so for `hits: mut Cell` a readonly `s` gives `Cell`
and a `mut s` gives `mut Cell`; `Source::member[F](h) -> F` produces the
member's declared type. `build` returns `mut Self`, because a built value
is fresh like a data literal; callers weaken it by ordinary assignability.

(M18, decided 2026-09-27 while reviewing the parked design) R5 (revised
the same day): handles may escape the walk for now; opting a type in is
consent for that library to read its members, as with round 1's `visit`.
This may change when the parked NonEscapable design (TQ-24 to TQ-26) is
ready, which could make handles non-escaping; typed derivation is not
blocked on it. P8: traversal stays pure: `walk`, `build`, `Walker`, and
`Source` have the empty requirement row and never suspend; I/O happens
before `build`, as in serde. R3: the comparison traits (`Eq`, `PartialOrd`,
`Ord`, `Hash`) stay on the closed `@derive` list permanently (all members,
no member lines); hand-written impls are trusted obligations as in Rust, and
TQ-12 forbids mixing derived and hand-written partners. P7: `build` is
input-driven, as in serde: the source names the next member, and generated
code keeps one local per member, then constructs. P10: templates only
implement existing traits; builders, patch types, and field-key enums are
not derived (literals with defaults and copy-update cover builders). P11b:
a newtype derives through its base type (TQ-11), so `Mile` encodes like
`i32`. P11a: shared enum fields are visible to walks, visited first with
handles marked `shared` (libraries choose to show or skip them), and `build`
never reads them from input (the variant's `->` expression computes them);
the owner also asked to revisit the shared-data design itself (now
ENUM_SEMANTICS decision 4: per-variant constants, read from the variant).
(Superseded by Enum Semantics decision 4 and M21 R3-7: shared constants
live on `VariantInfo` as `(name, Any)` pairs and are never visited as
handles.)
P11e: a tier-2 block lives only in the target type's module. P11f: GADT
enums are rejected at the opt-in for now. P11g: a member's `info` carries
name, position, facts, doc comment, and the `embedded` flag (the handle
has `has_default`); a variant's carries name, index, facts, and its shared
constants. P11d: in an enum's tier-2 block, member lines name whole
variants only (`Login = [facts]`, `Login += [facts]`); payload members get
no lines (declaration facts on payload parameters still apply), and a whole
variant cannot be skipped (`Login = pass` is an error, since `build` could
never produce it). P20: wire stability is documented, not checked:
reordering or renaming members or variants changes derived formats,
`Variant.index`, and derived `Ord`; libraries may require explicit tags,
and a toolchain check can come later from recorded interfaces. P19:
package interfaces carry template bodies, the walker and source bodies they
name, and annotation-function bodies (matching the 04 decision that
interfaces carry needed generic bodies); code size is one specialization
per (type, walker). P15: a foreign type is derived through a local mirror
type, as with serde's `remote`: the user declares a local data type with
the foreign type's public fields, derives on it, and converts; no orphan
exception for `by Structure`. P13 (tier-1 template selection), decided:
`@derive(...)` lists the traits and is the only thing that creates impls.
It accepts the intrinsic comparison traits and any trait with a `by
Structure` template; `@derive(json.Encode)` means exactly `impl
json.Encode for T by Structure`. Annotations only attach facts:
configuration is a separate annotation such as `@json.style(case=.Camel)`
that templates read through `T::facts()`. A lint flags a configuration fact
whose package supplies no template derived on that type. This replaces M5's
"the annotation is the opt-in" and the proposed `@derives(...)` groups;
tier 2 (`impl X for T by Structure:` with member lines) stays for per-trait
customization, and listing a trait in `@derive` plus a tier-2 block for it
is an ordinary `overlapping-impl`. R2 (the enum protocol, with a proposed
`variants()`/`variant_of`/`walk_variant` design) is deferred by the owner.
(Closed by M19.)

(M19, decided 2026-09-27; closes stress-test round 2 R2; revises M14's
walk protocol) The walk is value-driven and hands out member values:

```text
pub trait Walker[S]:                  # a walk over one value
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, Self::Error]

pub trait Describer[S]:               # a walk over the type only (schemas, DDL, help)
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]
```

For each opted-in type the compiler generates `walk(value, w)` (one
`match` on the value: `w.variant(v)` once for the value's variant, then
`w.member(h, member_value)` for each of its members, value
viewpoint-adapted per M17), `describe(d)` (every variant in order, then its
member handles), and the input-driven `build(source)` (P7; returns
`mut Self`, M17). A data type is an enum with one variant. The variant check
is done by generated code, so walkers no longer answer "enter this
variant?" or store the value; two-value walkers store only the other value
and use `h.get` on it; nested members go through their own type's impl
(M7), so each walk covers exactly one value's own members. M9's
strengthened member bounds apply to `Walker::member`, `Describer::member`,
and `Source::member`. The Current Design example below still shows M14's
protocol and needs updating. (Updated to M21 per R3-12.)

(M20, decided 2026-09-27; answers stress-test round 3, see
[DERIVATION_STRESS_TEST_3.md](DERIVATION_STRESS_TEST_3.md)) R3-12: the
record is refreshed in one pass (status line, Current Design example and
rules, P11a and M16 wording, M19 `Structure` signatures). R3-11: `h.get` on
another variant's payload keeps panicking; the walker guide documents the
`v.holds(other)` check for two-value walkers such as diff. R3-9: payload
parameter facts reuse the decorator-on-payload-parameter grammar of Error
Conversion decision 12; an unnamed payload member is named `_0`, `_1`, ...,
and `Member` info gains `positional: bool`. R3-2 (A): `VariantInfo` gains
`of_data: bool` and `doc: string?`; for a data type, its one variant's name
and doc are the type's. R3-6 (A): a lint warns when tier-2 blocks (or a
tier-2 block beside a `@derive`) for traits from one package on one type
have different member lines; differences stay legal (M11). R3-7 is
answered by M21.

(M21, decided 2026-09-27; closes the rest of stress-test round 3) R3-7:
shared enum constructor data (`enum HttpStatus(code: i32, ...)` with
`NotFound -> HttpStatus(404, ...)`) stays; typed derivation exposes a
variant's constants as an untyped list of `(name, Any)` pairs on
`VariantInfo`, built once at compile time; typed constant handles may come
later. R3-10: no in-place traversal; zeroize and in-place merge are
hand-written, and a derived merge returns a new value. R3-8: flattening an
embedded part is library code (a members-only encode plus buffered decode,
as in serde, so flattening excludes `deny_unknown_fields`); M16's "`get`
returns the part, a copy" is corrected to 08's alias rule
(`data.part.alias`). R3-1 (A, two handle views): walk and describe pass
handles whose member type is the read type (for `hits: mut Cell`, `Cell`);
build passes handles with the declared type (`mut Cell`), and `get` on a
build handle requires `s: mut S`, so a derived `Clone` or source cannot
upgrade a readonly member (`data.edge.principle`). `Field[-S, +F]`
variance lets a declared handle weaken to its read type. R3-5: newtypes keep
deriving through their base (P11b, TQ-11, VC-2), and forwarding is allowed
only when `Self` appears as the receiver, plain `Self`, `Self?`,
`Result[Self, E]`, or `List[Self]`; any other position (for example
`Map[Self, V]` or `Set[Self]`, whose contents depend on the key's own
`Hash`, `Eq`, or `Ord`) is an error at the opt-in naming the trait method.
R3-3: `Source` has four methods, following Kotlin's `decodeElementIndex`
loop: `variant(choices)`, `next(members) -> Key[S]` (a member key or end),
`member[F](h, previous: F?) -> F` (`previous` is the earlier value of a
repeated key, so protobuf merges and JSON rejects), and `missing[F](h) -> F`
(default or error); generated code never creates a `Self::Error`. R3-4: a
template may declare a constant computed once per opt-in at compile time by
the annotation evaluator, from `T::facts()` and `T::describe` with a pure
`Describer` (M18 P8), so key tables and lookups are not rebuilt per call.

(M22, decided 2026-09-27) Build handle `get` follows field access: `h.get(x)`
accepts any `S`, and returns the member's readonly view when `x` is
readonly and the declared type `F` when `x` is `mut S`. This replaces M21
R3-1's "requires `s: mut S`"; no member is upgraded, because the result
has the same permission ordinary field access gives
(`types.readonly.nested`). A `Clone` trait has two methods: `clone(self)`,
which copies from a readonly value through the readonly view, and
`clone_mut(mut self) -> mut Self`, which needs `mut self` and reads the
declared types, so a derived `clone_mut` can clone `mut` members as `mut`.
`= pass` keeps M3: walk and describe skip the member, build fills it from
its declared default, and a `= pass` member with no default is a compile
error. The design is applied to the specification now; the `src/`
prototype is a later, separate pass.

(M23, decided 2026-09-27, after the spec pass) `by Structure` needs an
explicit `use std.structure.Structure`, and it is never delegation. Code
outside generated code may call `missing[F]` through a generic source; only
the existing `member` restriction stays. `Clone` is a standard-library
trait. A derivation block may not target a newtype: a newtype derives only
through its base. The diagnostic codes and the panic category
`structure-variant-mismatch` chosen in the spec pass are accepted. (Applied
2026-09-27: see
[Typed Derivation](../spec/14-annotations.md#typed-derivation).)

(M24, decided 2026-09-27, after the M23 pass) A source may strengthen only
`member[F]`'s bound, not `missing[F]`'s, so a generic `missing` call is
always checked. Member and parameter metadata are `List[Any]`, evaluated
once at compile time like facts; checking a value against its member's
type waits for the fact check hook. `block_on` is forbidden in fact and
metadata expressions. `@derive(X)` needs no `use` of `Structure`; the
import is needed only where code writes `by Structure`. Decorators before
functions stay rejected until the function-target question is decided.
`Clone`'s module and the derived-function cache API are chosen with the
standard library. (Applied 2026-09-28: see
[`annot.walker.missing-fixed`](../spec/14-annotations.md#r-annot.walker.missing-fixed),
[`annot.metadata.eval`](../spec/14-annotations.md#r-annot.metadata.eval),
[`annot.fact.no-block-on`](../spec/14-annotations.md#r-annot.fact.no-block-on),
[`annot.derive.no-use`](../spec/14-annotations.md#r-annot.derive.no-use), and
`annot.decorator.function`, which
[Decorators D1](DECORATORS.md#owner-decisions) retired on 2026-09-28.
Also decided 2026-09-27: the root-application orphan exception is dropped
everywhere, so decisions 9 and 11's "plus the root-application orphan
exception" no longer holds; a foreign type is derived through a local mirror
type or a newtype.)

### Current Design: Full Example (M1-M21)

This is the reference example for the design as decided through M21. When a
later decision changes the design, update this example in the same change.

The code uses specification syntax: named arguments with `=`, brace data
literals, no `mut` at an argument site, and `T::f()` inside a template.
`Structure`'s signatures are the ones [round 3](DERIVATION_STRESS_TEST_3.md#surface-being-tested)
assumed (A1): `walk` takes the value as a readonly `self` and is called
`Structure::walk(self, w)`; `describe` and `build` are receiverless
(`T::describe(d)`, `T::build(s)`). Three forms are not yet specified and are
marked `# hypothetical syntax`: member lines (M3, M13), a template's
compile-time constant (M21 R3-4), and decorators on payload parameters (M20
R3-9). `impl Trait for X by Structure` parses as 09 trait delegation but
means a template here. The [Parse Log](#parse-log) records each block.

```text
# ══ std.structure ═════════════════════════════════════════════════
# The compiler supplies Structure, the handle constants, and the bodies
# written `pass`. Everything else, here and below, is ordinary library code.

pub trait Structure:                     # sealed; usable only inside `by Structure` templates
    fn facts() -> Facts                                                     # type-level facts (M13)
    fn walk[W < Walker[Self]](self, w: mut W) -> Result[void, W::Error]     # one value (M19)
    fn describe[D < Describer[Self]](d: mut D) -> Result[void, D::Error]    # the type only
    fn build[S < Source[Self]](s: mut S) -> Result[mut Self, S::Error]      # input-driven (M17, M21)

pub data Member:                         # M18 P11g, M20 R3-9
    pub name: string                     # `_0`, `_1`, ... for an unnamed payload member
    pub position: i32                    # 0-based within its variant
    pub facts: Facts
    pub doc: string?
    pub embedded: bool                   # an embedded part (M16)
    pub positional: bool                 # an unnamed payload member

pub data VariantInfo:                    # M18 P11g, M20 R3-2, M21 R3-7
    pub name: string                     # for a data type: the type's name
    pub index: i32
    pub facts: Facts
    pub doc: string?                     # for a data type: the type's doc
    pub of_data: bool                    # true for the one variant of a data type
    pub shared: List[(string, Any)]      # shared constructor data, built once at compile time

pub data Field[-S, +F]:                  # one constant per member (M14, M21 R3-1)
    pub info: Member

impl[S, F] Field[S, F]:
    pub fn get(self, s: S) -> F: pass    # a build handle needs `s: mut S`; panics on another variant
    pub fn has_default(self) -> bool: pass
    pub fn default(self) -> F?: pass     # evaluates the declared default

pub data Variant[S]:                     # one constant per variant
    pub info: VariantInfo

impl[S] Variant[S]:
    pub fn holds(self, s: S) -> bool: pass

pub data Members[S]:                     # the chosen variant's members (M21 R3-3)
    pub infos: List[Member]

impl[S] Members[S]:
    pub fn end(self) -> Key[S]: pass                               # the input has no more members
    pub fn at(self, position: i32) -> Key[S]: pass                 # end() when out of range
    pub fn find(self, matches: fn(Member) -> bool) -> Key[S]: pass # end() when none matches

pub data Key[S]:                         # one member of the chosen variant, or the end
    pub info: Member
    pub is_end: bool

# An impl of these three traits may strengthen F's bound (M9).
pub trait Walker[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, Self::Error]

pub trait Describer[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]

pub trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn next(mut self, members: Members[S]) -> Result[Key[S], Self::Error]
    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, Self::Error]
    fn missing[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]
```

A json library: an encoder walker, a decoder source, a schema describer, and
a key plan built once per opt-in at compile time.

```text
# ══ library json ══════════════════════════════════════════════════
use std.structure.{Structure, Facts, Member, Field, Variant, Members, Key, Walker, Describer, Source}

pub enum Case:
    Plain
    Camel
    Snake

pub data Style:                          # json's configuration: a type-level fact (M13)
    case: Case = .Plain
    tag: string = "type"

pub fn style(case: Case = .Plain, tag: string = "type") -> Style:    # @json.style(...)
    Style { case: case, tag: tag }

pub data Rename:                         # a member fact: @json.rename("mail")
    name: string

pub fn rename(name: string) -> Rename:
    Rename { name: name }

pub trait Encode:
    fn encode(self, out: mut Writer) -> Result[void, EncodeError]

pub trait Decode:
    fn decode(p: mut Parser) -> Result[Self, DecodeError]

pub trait Schema:
    fn schema() -> Node

# (elided: Writer, Parser, Node, EncodeError, DecodeError, apply_case,
# find_variant, KeyPlan.name, and hand-written Encode, Decode, and Schema
# impls for i64, string, and List[T])

fn style_of(facts: Facts) -> Style:
    match facts.find[Style]():
        .Some(style) => style
        .None => Style {}                # no fact: json's default

fn key_for(style: Style, m: Member) -> string:
    match m.facts.find[Rename]():
        .Some(r) => r.name
        .None => apply_case(style.case, m.name)

# A pure describer that plans every key once (M21 R3-4).
data KeyPlan:
    style: Style
    variant: i32
    names: mut Map[(i32, i32), string]       # (variant index, member position) -> key
    positions: mut Map[(i32, string), i32]   # (variant index, key) -> member position

fn planner(facts: Facts) -> mut KeyPlan:
    KeyPlan { style: style_of(facts), variant: 0, names: {}, positions: {} }

impl[S] Describer[S] for KeyPlan:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.variant = v.info.index
        .Ok()

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        key := key_for(self.style, h.info)
        self.names[(self.variant, h.info.position)] = key
        self.positions[(self.variant, key)] = h.info.position
        .Ok()

# Templates: they never apply by themselves, and only this module may declare them.
impl[T] Encode for T by Structure:
    const KEYS: KeyPlan:                 # hypothetical syntax: evaluated once per opt-in (M21 R3-4)
        let p: mut KeyPlan = planner(T::facts())
        _ := T::describe(p)
        p

    fn encode(self, out: mut Writer) -> Result[void, EncodeError]:
        out.begin_object()
        let w: mut Encoder = Encoder { out: out, keys: KEYS, variant: 0 }
        Structure::walk(self, w)?
        out.end_object()
        .Ok()

impl[T] Decode for T by Structure:
    const KEYS: KeyPlan:                 # hypothetical syntax
        let p: mut KeyPlan = planner(T::facts())
        _ := T::describe(p)
        p

    fn decode(p: mut Parser) -> Result[Self, DecodeError]:
        p.begin_object()?
        let s: mut FieldSource = FieldSource { parser: p, keys: KEYS, variant: 0 }
        value := T::build(s)?
        p.end_object()?
        .Ok(value)

impl[T] Schema for T by Structure:
    fn schema() -> Node:
        let d: mut SchemaDescriber = SchemaDescriber { style: style_of(T::facts()), variants: [] }
        _ := T::describe(d)
        Node.one_of(d.variants)          # one object for a data type; $defs elided

data Encoder:
    out: mut Writer
    keys: KeyPlan
    variant: i32

impl[S] Walker[S] for Encoder:
    type Error = EncodeError

    fn variant(mut self, v: Variant[S]) -> Result[void, EncodeError]:
        self.variant = v.info.index
        if !v.info.of_data:              # an enum writes its tag; a data type does not (M20 R3-2)
            self.out.key(self.keys.style.tag)
            self.out.string(apply_case(self.keys.style.case, v.info.name))
        .Ok()

    fn member[F < Encode](mut self, h: Field[S, F], value: F) -> Result[void, EncodeError]:
        self.out.key(self.keys.name(self.variant, h.info.position))   # planned, not renamed
        value.encode(self.out)           # F's own impl (M7)

data FieldSource:
    parser: mut Parser
    keys: KeyPlan
    variant: i32

impl[S] Source[S] for FieldSource:
    type Error = DecodeError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], DecodeError]:
        if choices[0].info.of_data:
            return .Ok(choices[0])
        v := find_variant(choices, self.parser.find_tag(self.keys.style.tag)?, self.keys.style.case)?
        self.variant = v.info.index
        .Ok(v)

    fn next(mut self, members: Members[S]) -> Result[Key[S], DecodeError]:
        while self.parser.more_keys()?:
            key := self.parser.key()?
            match self.keys.positions[(self.variant, key)]:   # one lookup
                .Some(position) => return .Ok(members.at(position))
                .None => self.parser.skip_value()?            # an unknown key is ignored
        .Ok(members.end())

    fn member[F < Decode](mut self, h: Field[S, F], previous: F?) -> Result[F, DecodeError]:
        if previous.is_some():           # a repeated key: JSON rejects it; protobuf would merge
            return .Err(DecodeError.Duplicate(h.info.name))
        F::decode(self.parser)

    fn missing[F < Decode](mut self, h: Field[S, F]) -> Result[F, DecodeError]:
        match h.default():               # an absent key: the declared default (P3)
            .Some(value) => .Ok(value)
            .None => .Err(DecodeError.Missing(h.info.name))

data SchemaDescriber:
    style: Style
    variants: mut List[mut Node]

impl[S] Describer[S] for SchemaDescriber:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        let node: mut Node = Node.object(title=v.info.name, doc=v.info.doc)   # a data type's name and doc
        if !v.info.of_data:
            node.constant(self.style.tag, apply_case(self.style.case, v.info.name))
        self.variants.append(node)
        .Ok()

    fn member[F < Schema](mut self, h: Field[S, F]) -> Result[void, never]:
        key := key_for(self.style, h.info)
        self.variants.last_mut().property(key, F::schema(), required=!h.has_default())
        .Ok()

pub fn to_json[T < Encode](value: T) -> Result[string, EncodeError]:
    let w: mut Writer = Writer {}
    value.encode(w)?
    .Ok(w.finish())

pub fn from_json[T < Decode](text: string) -> Result[T, DecodeError]:
    T::decode(Parser { text: text })
```

A diff library: a two-value walker that checks `v.holds(old)` before any
`h.get(old)` (M20 R3-11).

```text
# ══ library diff ══════════════════════════════════════════════════
use std.structure.{Structure, Field, Variant, Walker}

pub trait Diff:
    fn diff(self, old: Self, path: string, out: mut List[string]) -> void

impl[T] Diff for T by Structure:
    fn diff(self, old: Self, path: string, out: mut List[string]) -> void:
        let w: mut DiffWalker[T] = DiffWalker { old: old, path: path, out: out }
        if Structure::walk(self, w).is_err():
            out.append(path)             # another variant: the whole value changed

data VariantChanged: pass

data DiffWalker[S]:
    old: S
    path: string
    out: mut List[string]

impl[S] Walker[S] for DiffWalker[S]:
    type Error = VariantChanged

    fn variant(mut self, v: Variant[S]) -> Result[void, VariantChanged]:
        if !v.holds(self.old):           # checked first: h.get on another variant panics
            return .Err(VariantChanged {})
        .Ok()

    fn member[F < Diff](mut self, h: Field[S, F], value: F) -> Result[void, VariantChanged]:
        value.diff(h.get(self.old), self.path + "." + h.info.name, self.out)   # a walk handle: read type
        .Ok()
```

User code. `@derive` is the only opt-in, and annotations only attach facts
(M18 P13).

```text
# ══ app ═══════════════════════════════════════════════════════════
use dep.json
use dep.diff
use std.error.{Error}

@derive(Eq, Hash, json.Encode, json.Decode)
@json.style(case=.Snake)
pub data Address:
    pub streetLine: string               # "street_line"
    pub zipCode: string                  # "zip_code"

@derive(Eq, Hash, json.Encode, json.Decode)   # two intrinsics, two templates
@json.style(case=.Camel)                 # a type-level fact; creates no impl
pub data User:
    pub id: i64
    pub full_name: string                # "fullName"
    @json.rename("mail")                 # a member fact, visible to every impl
    pub email: string
    pub address: Address                 # Address's own impls: snake_case (M7)
    pub nickname: string = ""            # absent on input: missing() returns ""

# Shared constructor data, with payload and payload-free variants.
@derive(json.Encode, json.Decode, json.Schema, diff.Diff)
@json.style(tag="kind")
pub enum Reply(code: i32):
    Sent(id: i64) -> Reply(200)
    Moved(to: string, @json.rename("why") reason: string) -> Reply(301)   # hypothetical syntax
    Raw(string) -> Reply(200)            # an unnamed payload: member `_0`, positional
    Busy -> Reply(503)                   # payload-free

# A newtype derives through its base: i64's impls, rewrapped (M18 P11b, M21 R3-5).
@derive(Eq, Hash, json.Encode, json.Decode)
type UserId(i64)

# Tier 2: one block per trait; member lines are local to their block (M10, M11).
pub data Order:
    pub id: UserId
    pub total_cents: i64
    pub cache: Cache = Cache {}          # Cache has no json impls

impl json.Encode for Order by Structure:
    Self += [json.style(case=.Snake)]    # hypothetical syntax: this block's facts() only
    total_cents = [json.rename("total")] # hypothetical syntax
    cache = pass                         # hypothetical syntax: not walked or described

impl json.Decode for Order by Structure:
    Self += [json.style(case=.Snake)]    # hypothetical syntax
    cache = pass                         # hypothetical syntax: build uses Cache {}
# warning (M20 R3-6): Order's json.Encode and json.Decode blocks have different member lines

pub fn main() -> Result[void, Error] $ Console:
    u := User { id: 7, full_name: "Ada L", email: "ada@x",
                address: Address { streetLine: "1 Main", zipCode: "02139" } }
    println(json.to_json(u)?)
    # {"id":7,"fullName":"Ada L","mail":"ada@x","address":{"street_line":"1 Main","zip_code":"02139"},"nickname":""}
    let back: User = json.from_json(r"""{"mail":"ada@x","id":7,"fullName":"Ada L","address":{"zip_code":"02139","street_line":"1 Main"}}""")?
    println(back == u)                   # true: keys in any order; nickname from its default

    r := Reply.Moved(to="/new", reason="renamed")
    println(json.to_json(r)?)            # {"kind":"Moved","to":"/new","why":"renamed"}
    let changes: mut List[string] = []
    r.diff(Reply.Busy, "reply", changes)
    println(changes)                     # ["reply"]: another variant, and no panic
    println(json.to_json(Order { id: UserId(1), total_cents: 1250 })?)
    # {"id":1,"total":1250}
    .Ok()
```

What the compiler generates (ordinary hd; tooling can print it). Handle and
member-list names such as `h_id` and `ADDRESS_MEMBERS` are illustrative.

```text
# User's handles. Walk and describe pass the read view; build passes the
# declared view. They coincide here because no member is `mut` (M21 R3-1).
#   h_id: Field[User, i64], h_full_name: Field[User, string],
#   h_email: Field[User, string] (h_email.info.facts == [Rename { name: "mail" }]),
#   h_address: Field[User, Address], h_nickname: Field[User, string] (has_default() is true)
#   v_user.info: name "User", index 0, of_data true, shared []
# Reply: v_moved.info.shared == [("code", 301)]; h_raw_0.info: name "_0", positional true

# Structure::walk(r, w) for Reply inside its Encode impl: one match, then
# one call per member of the value's variant (M19).
fn walk(value: Reply, w: mut json.Encoder) -> Result[void, json.EncodeError]:
    match value:
        Reply.Sent(id) =>
            w.variant(v_sent)?
            w.member[i64](h_sent_id, id)?          # needs i64 < json.Encode
        Reply.Moved(to, reason) =>
            w.variant(v_moved)?
            w.member[string](h_moved_to, to)?
            w.member[string](h_moved_reason, reason)?
        Reply.Raw(text) =>
            w.variant(v_raw)?
            w.member[string](h_raw_0, text)?
        Reply.Busy =>
            w.variant(v_busy)?
    .Ok()

# T::build(s) for Address inside its Decode impl: the input names the members (M21 R3-3).
fn build(s: mut json.FieldSource) -> Result[mut Address, json.DecodeError]:
    _ := s.variant([v_address])?
    let street: string? = .None                    # one local per member
    let zip: string? = .None
    while true:
        key := s.next(ADDRESS_MEMBERS)?
        if key.is_end:
            break
        match key.info.position:
            0 =>
                street = .Some(s.member[string](h_street_line, street)?)
            1 =>
                zip = .Some(s.member[string](h_zip_code, zip)?)
            _ =>
                panic("a key from another type or variant")
    street_value := match street:
        .Some(v) => v
        .None => s.missing[string](h_street_line)?
    zip_value := match zip:
        .Some(v) => v
        .None => s.missing[string](h_zip_code)?
    .Ok(Address { streetLine: street_value, zipCode: zip_value })
```

Each `member[i64]` call runs the walker's `member` body with `i64`'s
dictionary, a constant, so there is no per-member allocation. Generated code
never creates a `Self::Error`: every failure it could detect is the
source's call.

What the compiler rejects:

```text
@derive(json.Encode)
pub data Bad:
    handle: FileHandle
# error at @derive: member `handle`: FileHandle does not implement json.Encode;
#   use `handle = pass` in a tier-2 block, or change the member's type

@derive(Error)
pub enum Failure:
    Io
# error: Error has no template and is not intrinsic; an error type uses @error

pub trait Index:                         # with a `by Structure` template, elided
    fn index(values: List[Self]) -> Map[Self, i64]

@derive(Index)
type Tag(string)
# error at @derive: Index.index has Self in Map[Self, i64]; a newtype forwards
#   only the receiver, Self, Self?, Result[Self, E], and List[Self] (M21 R3-5)

data ShareSource[S]:
    old: S

impl[S] Source[S] for ShareSource[S]:   # variant, next, and missing elided
    type Error = never

    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, never]:
        .Ok(h.get(self.old))
# error: `get` on a build handle needs `mut S`; `self.old` is readonly (M21 R3-1)
```

Not rejected: a two-value walker that calls `h.get(old)` without checking
`v.holds(old)` compiles, and panics when `old` holds another variant (M20
R3-11).

### Current Rules (M1-M21)

1. **Opt-in** (M18 P13). `@derive(...)` lists traits and is the only thing
   that creates impls. It accepts the intrinsic comparison traits and any
   trait with a `by Structure` template; `@derive(json.Encode)` means
   exactly `impl json.Encode for T by Structure`. `Error` is not on the
   list: an error type uses the `@error` intrinsic
   ([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)).
2. **Comparison intrinsics** (M12, M18 R3). `Eq`, `PartialOrd`, `Ord`, and
   `Hash` stay compiler intrinsics over all members, with no member lines.
   There is one `Eq` with `fn eq(self, other: Self) -> bool`, applied as
   EQ-1 in [Comparison Traits](../spec/09-traits.md#comparison-traits).
3. **Structure** (M1, M6, M8, M19). The compiler generates `Structure` for
   each opted-in type, with the four signatures shown above. It is sealed
   (`sealed-trait-implementation`), exists only inside `by Structure`
   templates, and may bound nothing else. `walk` takes the value as `self`;
   `describe` and `build` are receiverless; `build` returns `mut Self`
   (M17).
4. **Templates** (M8, M12, M15, M18 P10). `impl[T] Trait for T by
   Structure:` declares a template, only in the trait's module, so there is
   at most one per trait. It never applies by itself, so it cannot overlap a
   hand-written impl. Its body must walk, describe, or build: there are no
   marker templates. A template implements an existing trait once per
   opt-in: no impl families, derived types, or derived builders.
5. **Tier 2** (M3, M10, M11, M13, M18 P11d, P11e). `impl Trait for X by
   Structure:` applies one template, in `X`'s module only. Its member lines
   (`f += [facts]`, `f = [facts]`, `f = pass`, and `Self` lines for
   type-level facts) are local to that block. In an enum's block, lines name
   whole variants, and `Variant = pass` is an error. Listing a trait in
   `@derive` and writing its block is an ordinary `overlapping-impl`.
6. **`= pass`** (M3). The member is left out of that block's generated
   calls, so its type need not support the trait; `build` uses its declared
   default, and a member without one is an error.
7. **Facts** (M2, M13, M20 R3-9). Facts are typed values, purely
   descriptive. Declaration facts (type-level, member, and payload
   parameter, the last through Error Conversion decision 12's grammar) are
   seen by every impl. A template reads configuration from `T::facts()`
   and falls back to its own default. A lint flags a configuration fact
   whose package derives nothing on the type (M18 P13).
8. **Drift lint** (M20 R3-6). Tier-2 blocks for traits of one package on
   one type, or such a block beside a `@derive` of the same package, get a
   warning when their member lines differ. Differences stay legal.
9. **Walk** (M19). Generated `walk` makes one `match` on the value, calls
   `w.variant(v)` once for its variant, then `w.member(h, value)` for each
   member of that variant. A data type is one variant with `of_data` true
   (M20 R3-2). Nested members use their own type's impl (M7).
10. **Describe** (M19). Generated `describe` calls `d.variant(v)` for every
    variant in order, then `d.member(h)` for each of its members. It holds
    no value.
11. **Build** (M18 P7, M21 R3-3). Generated `build` calls `s.variant` once,
    then `s.next(members)` until the end key. For each key it calls
    `s.member(h, previous)`, where `previous` is the value already read for
    a repeated key. It then calls `s.missing(h)` once per member never
    named, and constructs. It keeps one local per member and never creates
    a `Self::Error`.
12. **Handles** (M14, M17, M21 R3-1). Each member has a constant
    `Field[-S, +F]` and each variant a constant `Variant[S]`. Walk and
    describe handles carry the read type (`Cell` for `hits: mut Cell`);
    build handles carry the declared type (`mut Cell`), and their `get`
    requires `s: mut S`. A declared handle weakens to its read type by
    variance.
13. **Other variants** (M14, M20 R3-11). `h.get` on another variant's
    payload panics. A two-value walker checks `v.holds(other)` first, as the
    diff walker does.
14. **Member and variant information** (M18 P11g, M20, M21 R3-7). `Member`
    has `name`, `position`, `facts`, `doc`, `embedded`, and `positional`;
    an unnamed payload member is named `_0`, `_1`, and so on. `VariantInfo`
    has `name`, `index`, `facts`, `doc`, `of_data`, and `shared`. For a data
    type, the variant's name and doc are the type's.
15. **Shared constructor data** (M21 R3-7,
    [Enum Semantics decision 4](ENUM_SEMANTICS.md#owner-decisions)). A
    variant's shared constants are `(name, Any)` pairs on `VariantInfo`,
    built once at compile time. They are never passed as handles, and
    `build` never reads them: the variant's `->` expression fixes them.
16. **Embedded parts** (M16, M21 R3-8). An embedded part is one member with
    `embedded` true. Its value is the part itself, not a copy
    (`data.part.alias`). Flattening is library code, as in serde.
17. **Bounds** (M9, M12, M24). An impl of `Walker`, `Describer`, or
    `Source` may strengthen `member[F]`'s bound, but not `missing[F]`'s. These traits are not
    09-sealed. Only generated code may call `member` through a generic
    walker or source. The obligation is checked at the opt-in, naming the
    member, and `= pass` members are exempt.
18. **Generic targets** (M12). A derived impl for a generic type gets
    `T < Trait` for each parameter in a walked or built member. Recursion is
    checked coinductively. When a member needs more, the error suggests a
    tier-2 block with an explicit header.
19. **Newtypes** (M18 P11b, M21 R3-5). A newtype derives through its base
    type. Forwarding is allowed only where `Self` is the receiver, plain
    `Self`, `Self?`, `Result[Self, E]`, or `List[Self]`. Any other position
    is an error at the opt-in, naming the trait method.
20. **Purity and plans** (M18 P8, M21 R3-4). `walk`, `describe`, `build`,
    and the three traits have the empty requirement row and never suspend.
    A template may declare a constant that the annotation evaluator computes
    once per opt-in at compile time, from `T::facts()` and `T::describe`
    with a pure describer.
21. **Out of scope** (M18 P11f, P15, M21 R3-10). GADT enums are rejected at
    the opt-in. A foreign type is derived through a local mirror type, with
    no orphan exception. There is no in-place traversal: zeroize and
    in-place merge are hand-written, and a derived merge returns a new
    value.
22. **Escape and stability** (M18 R5, P19, P20). Handles may escape the
    walk; opting in is consent. Package interfaces carry template, walker,
    source, and annotation-function bodies. Wire stability is documented,
    not checked.
23. **Cost** (M9, M14). Handles are constants, and each member call passes
    a constant dictionary, so there is no per-member allocation.
    `default()` allocates `.Some`; `has_default()` does not.

### Remaining Open

Nothing below is decided. Each item waits for the owner.

- **Fact check hook** (M15). The form of a fact type's compile-time `check`,
  and whether it covers cross-member and type-level checks (round 2 R8).
- **Non-escaping handles** (M18 R5). Whether the NonEscapable design (TQ-24
  to TQ-26) makes handles non-escaping once it is ready.
- **Plan constants** (M21 R3-4). The declaration and reference syntax of a
  template's constant (the example's `const KEYS: KeyPlan:` is
  hypothetical), and the compile-time evaluator's exact limits.
- **Typed shared constants** (M21 R3-7). Typed constant handles may come
  later.
- **`T -> U` mapping** (M14). Whether mapping between two types is in
  scope.
- **Name clashes** (round 2 R13). `walk`, `describe`, and `build` collide
  with trait methods of the same name; the example avoids it with
  `Structure::walk(self, w)`.
- **Derived bound** (round 2 R14). Rule 18 names the trait, while the
  checked obligation is the walker's or source's strengthened bound; they
  differ when a walker asks for more than the trait.
- **`default()` allocation** (round 2 R15). 04 and 05 give every `.Some`
  its own identity, so `default()` may allocate for every member type, not
  only reference-shaped ones.
- **Composing templates** (round 1 P16). A wrapper walker cannot forward to
  an inner walker's `member`, because M9 lets only generated code call it
  through a generic parameter.
- **`Clone`'s module** (M24). Chosen with the standard library
  ([STDLIB](STDLIB.md#clone)).
- **Derived-function cache** (M24). Its API and module are chosen with the
  standard library ([STDLIB](STDLIB.md#derived-function-cache)).
- **Trait-less blocks** (M26). Decided by M28: a generic target, several
  blocks for one type, unused facts from a `Self` line, and a member line's
  right side. What its apply pass left open is in
  [Still Open After M27 And M28](#still-open-after-m27-and-m28).
- **Function targets.** Function targets wait for
  [FN_TYPE.md](FN_TYPE.md) questions 9 and 10. Chapter 14's facet protocol
  is removed (decision 10, applied 2026-09-27). Since
  [Decorators D1](DECORATORS.md#owner-decisions) (applied 2026-09-28), an
  ordinary decorator before a function attaches a value; deriving for
  functions still waits.

### Still Open After The Prototype Pass

The `src/` prototype pass (2026-09-28) met these gaps in the specification.
Nothing here is decided; each item says what the prototype does and gives a
**Recommendation** for the owner.

| Question | What the spec leaves open | Prototype | Recommendation |
| --- | --- | --- | --- |
| A data type's variant facts | [`annot.variant.data`](../spec/14-annotations.md#r-annot.variant.data) gives the one variant the type's name and doc, but says nothing of `VariantInfo.facts`. | an empty list | An empty list, stated in the rule: type-level facts are read once, through `T::facts()`. |
| `@derive` before a function | `annot.decorator.function` (retired by Decorators D1) rejected an ordinary decorator; the grammar also admits `@derive(...)` there. | `decorator-not-annotator` for every decorator line | `decorator-not-annotator` for `@derive` too, until function targets are decided. |
| Duplicate declaration facts | [Member Metadata](../spec/14-annotations.md#member-metadata) forbids two values of one concrete type on a member, but names no code; `duplicate-fact` covers only member lines. | not checked | Report it as `duplicate-fact`. |
| Omitting an embedded part | An embedded field cannot declare a default (`embedded-field-default`), so `Part = pass` is always `omitted-member-without-default`. | reports that error | Keep it, and add the case as an example beside [`annot.omit.no-default`](../spec/14-annotations.md#r-annot.omit.no-default). |
| Unused facts of foreign types | `annot.fact.unused` (now [`annot.fact.unused-non-std`](../spec/14-annotations.md#r-annot.fact.unused-non-std)) keyed on the fact's package; a fact of a primitive or standard type, such as `@"note"`, has no library package. | warns for any type-level fact of a type that derives no template | Warn only when the fact's type comes from a package other than `std`. |

**Owner decisions (2026-09-28, M25).** The owner accepted the recommendations
in rows 1, 2, 3 and 5:
- a data type's one variant has an empty `VariantInfo.facts`;
- `@derive` before a function is `decorator-not-annotator` (`@suffix` is the
  only decorator allowed there, per Literal Suffixes L11);
- a duplicate declaration fact is `duplicate-fact`;
- `unused-derivation-fact` warns only for fact types from a package other
  than `std`.

Applied 2026-09-28 in
[`annot.variant.data-facts`](../spec/14-annotations.md#r-annot.variant.data-facts),
[`annot.decorator.function-derive`](../spec/14-annotations.md#r-annot.decorator.function-derive),
[`annot.metadata.duplicate`](../spec/14-annotations.md#r-annot.metadata.duplicate)
and [`annot.fact.unused-non-std`](../spec/14-annotations.md#r-annot.fact.unused-non-std).
The `@suffix` note in row 2 was not applied then: Literal Suffixes L11
waited for the decorator redesign. [Decorators D1](DECORATORS.md#owner-decisions)
(applied 2026-09-28) lets every ordinary decorator precede a function, and
`@derive` there stays `decorator-not-annotator`.
Two readings are open for the owner:

| Point | Applied | **Recommendation** |
| --- | --- | --- |
| Two type-level facts of one concrete type from decorators, as in two `@style(...)` lines | **Decided by M27**: `duplicate-fact` on the later decorator, [`annot.fact.duplicate-decorator`](../spec/14-annotations.md#r-annot.fact.duplicate-decorator) | `duplicate-fact` on the later decorator, as for members. |
| Where `duplicate-fact` is reported for declaration facts | On the later value | Keep. |

**Owner decision M28 (2026-09-28), the M26 open points.** The owner
accepted all four recommendations:
1. A trait-less block's header may use only the declaration's own type
   parameters, with no bounds, as in `impl[T] Box[T] by Structure:`. Any
   other header is `misplaced-derivation`.
2. A type has at most one trait-less block. A second one is
   `overlapping-impl`.
3. A `Self` line fact that no template reads gets `unused-derivation-fact`,
   reported on the line.
4. A member line's right side may be any list-typed expression, so
   `name = shared_list` is valid.

**Owner decision M27 (2026-09-28).**
- Row 4 is kept: `Part = pass` on an embedded part is
  `omitted-member-without-default`. Add the case as an example next to
  `annot.omit.no-default`.
- Two type-level decorators of the same fact type on one declaration (two
  `@style(...)` lines): the later one is `duplicate-fact`, as for members.

**Applied 2026-09-28 (M27 and M28)** in
[`annot.omit.no-default`](../spec/14-annotations.md#r-annot.omit.no-default)
(the embedded-part example),
[`annot.fact.duplicate-decorator`](../spec/14-annotations.md#r-annot.fact.duplicate-decorator),
[`annot.traitless.generic`](../spec/14-annotations.md#r-annot.traitless.generic),
[`annot.traitless.generic.error`](../spec/14-annotations.md#r-annot.traitless.generic.error),
[`annot.traitless.unique`](../spec/14-annotations.md#r-annot.traitless.unique),
[`annot.fact.unused-self-line`](../spec/14-annotations.md#r-annot.fact.unused-self-line)
and [`annot.line.right-typed`](../spec/14-annotations.md#r-annot.line.right-typed),
which replaces the retired `annot.line.right`. The prototype followed on
2026-09-28: all eight new fixtures pass.

**Owner decision M26 (2026-09-28): the `annotate Target:` block is removed.**
`annotate` is no longer a reserved word. Metadata is written three ways:

1. With `@value` on the declaration, a field, a variant or a parameter, as
   today.
2. Shared metadata written away from the declaration goes in a trait-less
   derivation block, `impl User by Structure:`, with member lines such as
   `name = [max_len(80)]`. These facts apply to every derivation, like
   `@` facts. The only grammar change is to allow the existing
   `[ "by", identifier ]` clause after the inherent `impl Type` form. There
   is no new keyword or token, and the body reuses `derivation_line`.
3. Per-trait member lines stay in `impl Trait for User by Structure:` (M3).

A trait-less block is only for writing shared metadata. It is not the only
place metadata can be declared.

Open for the apply pass:
- Which scope may write the trait-less block. The recommendation is the
  same orphan rule as other impls.
- How parameter metadata, which `annotate get_user` wrote, is written away
  from the function. The recommendation is `@` on the parameter only, since
  functions have no derivation block.
- How a trait-less block's `+=` and `=` combine with the declaration's `@`
  facts. The recommendation is the member-line rules, with the declaration
  facts coming first.

**Applied 2026-09-28 (M26)** in
[Trait-Less Derivation Blocks](../spec/14-annotations.md#trait-less-derivation-blocks),
[Member Metadata](../spec/14-annotations.md#member-metadata),
[`grammar.impl.traitless-by`](../spec/02-grammar.md#r-grammar.impl.traitless-by),
[`trait.by.trait-less.error`](../spec/09-traits.md#r-trait.by.trait-less.error)
and [`lex.keyword.reserved-words`](../spec/01-lexical-structure.md#r-lex.keyword.reserved-words).
The prototype follows since 2026-09-28: ten of the thirteen M26 fixtures
pass, and the other three need the shape intrinsics (`K1` rows of
[KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv)). The three open
points are applied as their recommendations. Each is a reading for the
owner to confirm:

| Point | Applied reading | Rules |
| --- | --- | --- |
| Scope | The block must be in the module that declares its target, as an inherent implementation and a derivation block must. Elsewhere it is `misplaced-derivation`. | [`annot.traitless.module`](../spec/14-annotations.md#r-annot.traitless.module) |
| Parameter metadata | `@` on the parameter only. A block for a function is not possible, since its target must be a type. | [`annot.metadata.params-at-only`](../spec/14-annotations.md#r-annot.metadata.params-at-only) |
| Combining | Decorator values come first. A `+=` line appends after them, a `=` line replaces them, and `Self` lines do the same for type-level facts. A per-trait block's lines then edit the result. | [`annot.traitless.after-decorators`](../spec/14-annotations.md#r-annot.traitless.after-decorators), [`annot.traitless.self`](../spec/14-annotations.md#r-annot.traitless.self), [`annot.traitless.then-blocks`](../spec/14-annotations.md#r-annot.traitless.then-blocks) |

The decision says the block is only for writing shared metadata. The apply
pass read that as the rules below, also for the owner to confirm:

| Point | Applied reading | Rules |
| --- | --- | --- |
| An omit line, `f = pass` | `invalid-member-line`: omitting a member changes generated code, so it is not metadata | [`annot.traitless.no-omit`](../spec/14-annotations.md#r-annot.traitless.no-omit) |
| A method or associated type in the block | `misplaced-derivation` on that member | [`annot.traitless.lines-only`](../spec/14-annotations.md#r-annot.traitless.lines-only) |
| A block in a local scope | `misplaced-derivation`, keeping the old rule that local declarations carry no metadata | [`annot.traitless.local`](../spec/14-annotations.md#r-annot.traitless.local), [`names.local.no-metadata`](../spec/03-names-and-scopes.md#r-names.local.no-metadata) |
| Target kinds | A data type or enum only; a newtype, like any other target, is `misplaced-derivation`. A GADT enum is allowed, since the block derives nothing | [`annot.traitless.target`](../spec/14-annotations.md#r-annot.traitless.target) |
| `impl C by E` without a trait, where `E` is not `Structure` | `invalid-delegation`, as for a bad delegation | [`trait.by.trait-less.error`](../spec/09-traits.md#r-trait.by.trait-less.error) |

**Still open after M26.** M28 decided the first four rows as recommended,
and they are applied to the spec (2026-09-28). The fifth came up in the
prototype pass and is fixed. The Prototype column says what `src/` did
before it followed M28 (it does since 2026-09-28):

| Question | Effect | Prototype | **Recommendation** |
| --- | --- | --- | --- |
| A generic target | `impl[T] Box[T] by Structure:` and `impl Box[i32] by Structure:` both parse; neither has a meaning. | Takes either header by its type constructor, `Box`. | **Decided by M28** as recommended: only the declaration's own parameters, without bounds, as `impl[T] Box[T] by Structure:`. Any other header is `misplaced-derivation` ([`annot.traitless.generic`](../spec/14-annotations.md#r-annot.traitless.generic)). |
| Several trait-less blocks for one type | Two blocks that both write `name =` would depend on source order. | Applies them in source order; each sees the facts the earlier ones left. | **Decided by M28** as recommended: at most one per type; a second is `overlapping-impl` ([`annot.traitless.unique`](../spec/14-annotations.md#r-annot.traitless.unique)). |
| Unused facts from a `Self` line | [`annot.fact.unused-non-std`](../spec/14-annotations.md#r-annot.fact.unused-non-std) reports on a decorator, and a `Self` line has none. | No warning: only decorator facts are checked. | **Decided by M28** as recommended: the same warning, reported on the `Self` line ([`annot.fact.unused-self-line`](../spec/14-annotations.md#r-annot.fact.unused-self-line)). |
| A member line's right side | `annot.line.right`, now retired, said "a list expression". `annotate` accepted any expression, so `name = shared_list` was valid. | A list literal only; `name = shared_list` is `invalid-member-line`. | **Decided by M28** as recommended: any expression of a list type ([`annot.line.right-typed`](../spec/14-annotations.md#r-annot.line.right-typed)). The tour now writes `display_name = display_name_metadata`. |
| Two retention rules on one member | [`retention-metadata.hd`](../spec/conformance/typing/valid/retention-metadata.hd), an accept fixture, put `retention_owner(...)` and `delete_when(...)` on `userId`. Both returned `RetentionRule`, which [`annot.metadata.duplicate`](../spec/14-annotations.md#r-annot.metadata.duplicate) rejects. | **Fixed 2026-09-28** in the fixture, not the rule: `delete_when` now returns its own `DeleteWhen`. The fixture stays a known failure only for the shape intrinsics (`K1`). | Applied as recommended: give `delete_when` its own result type. |

#### Still Open After M27 And M28

**Owner decision M29 (2026-09-28).** All four recommendations below are
accepted:
1. A trait-less block's header may rename the type's parameters
   (`impl[U] Box[U] by Structure:` for `data Box[T]`). Only the count, the
   order and the absence of bounds are checked.
2. A member line whose right side isn't a list, such as `name = 5`, is
   `invalid-member-line`.
3. A `Self` line in a per-trait derivation block also warns
   `unused-derivation-fact`, on the line, when the fact's package doesn't
   supply that block's trait.
4. `duplicate-fact` is reported on the later value. This confirms M25's
   reading.

**Applied 2026-09-28 (M29)** in
[`annot.traitless.generic.rename`](../spec/14-annotations.md#r-annot.traitless.generic.rename),
[`annot.line.right.not-list`](../spec/14-annotations.md#r-annot.line.right.not-list)
and [`annot.fact.unused-self-line.per-trait`](../spec/14-annotations.md#r-annot.fact.unused-self-line.per-trait).
Point 4 needed no new text:
[`annot.metadata.duplicate`](../spec/14-annotations.md#r-annot.metadata.duplicate)
already reports on the later value. The prototype now follows M27, M28 and
M29, and their fixtures pass, except the per-trait `Self` line fixture: it
needs a second package, and the prototype CLI has no package roles (`M29`
row of [KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv)).

Applying M27 and M28 raised these readings. M29 above decided all four as
recommended, and they are applied:

| Question | Effect | **Recommendation** |
| --- | --- | --- |
| Parameter names in a generic header | [`annot.traitless.generic`](../spec/14-annotations.md#r-annot.traitless.generic) says "own type parameters". It does not say whether `impl[U] Box[U] by Structure:` for `data Box[T]` is valid. | Valid: every other implementation header binds its own parameter names, so only the count, order, and absence of bounds are checked. |
| A right side that is not a list | [`annot.line.right-typed`](../spec/14-annotations.md#r-annot.line.right-typed) is now a type rule, so `name = 5` could be [`invalid-member-line`](../spec/14-annotations.md#r-annot.line.right.error) or `type-mismatch` from the contextual `List[Any]`. | `invalid-member-line`, as the unchanged `annot.line.right.error` reads today. |
| A `Self` line in a per-trait block | M28 covers a trait-less block's `Self` line. A `Self += [style(...)]` in `impl Encode for User by Structure:` is read only by that block's template, and no rule warns on it. | The same warning on the line when the fact's package does not supply the block's trait. |
| Where M25's declaration facts are reported | M25's second reading, `duplicate-fact` on the later value, was left to confirm. M27 says "as for members", which uses that reading. | Treat M27 as confirming it. |

### Current Design: Full Example (M1-M14)

Superseded by the [M1-M21 example](#current-design-full-example-m1-m21).
Rounds 1-3 of the stress test link here for the example they read; the M14
version is in this file's git history (commit 913bcc7 and earlier).

### Current Rules (M1-M14)

Superseded by the [M1-M21 rules](#current-rules-m1-m21). The M14 rules are
in the same git history.

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

`@derive(...)` accepts only `Eq`, `PartialOrd`, `Ord`, and `Hash`
([Traits](../spec/09-traits.md#comparison-traits)). Owner decision
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

@derive(Eq, Hash, json.Encode, json.Decode, json.Schema)
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

fn sync!(p: Path) -> Result[void, SyncError] $ FsRead, Http:
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
6. The already-decided pieces, now applied: TQ-9 static calls through a
   bound ([Associated Function Calls](../spec/09-traits.md#associated-function-calls)),
   TQ-11 newtype derive ([Derived Newtypes](../spec/09-traits.md#derived-newtypes)),
   TQ-23 shape cases
   ([Common Shape Representation](../spec/14-annotations.md#common-shape-representation)),
   and the grammar change that lets `@derive` precede `type Mile(i32)`.

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

- **A.** Later, as a specification of their meaning (`impl Eq for
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
| Probe, not shown: `@derive(json.Encode)` before `type Mile(i32)` | Was `syntax-error` when logged; parses since TQ-11 was applied ([Derived Newtypes](../spec/09-traits.md#derived-newtypes)). |
| Probe, not shown: a decorator before `trait` | `syntax-error`; not used by the recommendation. |

The [current design](#current-design-full-example-m1-m21) was rewritten in
specification syntax on 2026-09-27, rewritten for M14 the same day, and
rewritten again for M1-M21 (R3-12). Each time its blocks were checked with
the same parser; the M21 results are below. The decision records M8, M9, and
M14 above keep their original spelling, and the M8 and M9 blocks do not
parse as written.

| Block | Result |
| --- | --- |
| std.structure | Parses. `Field[-S, +F]` uses 04's variance markers. |
| library json | As written, `syntax-error` at line 70, the first `const KEYS: KeyPlan:` header (M21 R3-4), as expected. With both headers read as `fn keys() -> KeyPlan:`, parses. |
| library diff | Parses. |
| app | As written, `syntax-error` at line 27, the decorated payload parameter of `Moved` (M20 R3-9), as expected. With the six lines marked `# hypothetical syntax` removed, and each bodiless impl header's colon dropped, parses. `by Structure` parses as the 09 delegation form. |
| Generated code | Parses. |
| Rejected code | Parses; every error shown is semantic. |

Parsing checks syntax only. The M21 surface (`Structure`, `Field`,
`Variant`, `Members`, `Key`, `Walker`, `Describer`, `Source`) and the
compiler-supplied bodies, written `pass`, are not type-checked, and nothing
in the example ran. Decorators on payload parameters still need the grammar
change of Error Conversion decision 12.

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
