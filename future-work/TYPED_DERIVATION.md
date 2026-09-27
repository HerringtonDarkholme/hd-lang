# Typed Derivation: Survey And Design Options

**Deferred by the owner (2026-09-27).** After two stress tests
([round 1](DERIVATION_STRESS_TEST.md), [round 2](DERIVATION_STRESS_TEST_2.md))
left critical problems open (R1 `mut` member handles, R2 the enum protocol,
R3 `= pass` and law partners, R5 handles escaping the walk, P13 tier-1
selection), and error derivation moved to the `@derive(Error)` intrinsic
([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)), typed
derivation is parked. Until it is reopened, `@derive` stays a closed list of
compiler intrinsics (the comparison and hash traits, and `Error`). M1-M16
below are kept as the record of the design so far; nothing here is to be
applied.

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
    Function targets are decided with the function-item question.
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
Not yet applied to 09 (recorded in audit/types/QUESTIONS.md). Still under discussion: P1-P3 (the core walk), P5 (where
configuration lives), and the rest of the stress test's problems.

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
type.

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
handle API.

(M15, decided 2026-09-27; closes stress-test P12 and P14) Typed derivation
deliberately does not cover impl families: one impl per variant or member,
each a different trait instantiation keyed by the member's type. A template
gives one trait instantiation per opt-in. Error derivation, the one case that
needs a family (`From` per variant) together with `Display` and
`Error::cause` from the same per-variant markers, is the compiler intrinsic
`@derive(Error)`, hd's `thiserror`, with the variant markers `@message`,
`@from`, `@source`, and `@transparent`; see
[Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions). For
other libraries' facts, the fact check hook is accepted (P14): a fact type
may define a compile-time `check` against the member or variant it is
attached to, run at the opt-in site. Its exact form is not yet designed.

(M16, decided 2026-09-27; closes the embedding part of stress-test P11)
An embedded part is one member. `walk` and `build` pass a handle
`Field[S, P]` for the embedded field, named by the embedded type as in
08 (`post.Timestamps`), with `info` marking it `embedded`; `get` returns the
part, a copy, per value semantics. Flattening or nesting is a library
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

(M18, decided 2026-09-27 while reviewing the parked design) R5: handles
must not escape the walk (no capture, no storage); this waits for the
parked NonEscapable design (TQ-24 to TQ-26), so typed derivation stays
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
P11e: a tier-2 block lives only in the target type's module. P11f: GADT
enums are rejected at the opt-in for now. P11g: a member's `info` carries
name, position, facts, doc comment, and the `embedded` flag (the handle
has `has_default`); a variant's carries name, index, facts, and its shared
constants. P13
(tier-1 template selection; the proposal is `@derives(...)` on
the annotation function) is deferred. R2 (the enum protocol, with a proposed
`variants()`/`variant_of`/`walk_variant` design) is deferred by the owner.

### Current Design: Full Example (M1-M14)

This is the reference example for the design as decided on 2026-09-27. When
a later decision changes the design, update this example in the same change.

The code is written in specification syntax (02, 05, 07, 08, 10): named
arguments use `=`, data literals use braces, no `mut` is written at an
argument site, packages are used as `use dep.json` (so json's annotation
function is written `@json.json(case=.Camel)`), and a template names its
target as `T::`, because `Self::f()` is not a primary expression. Two forms
are not yet specified. The member lines of M3 and M13 (`name = [facts]`,
`name += [facts]`, `name = pass`, `Self += [facts]`) are new syntax, marked
`# hypothetical syntax` below. `impl Trait for X by Structure` parses as 09
trait delegation (`by identifier`) but means a template here (P18). The
[Parse Log](#parse-log) records each block's result.

Since M14 the compiler generates value-free walks over typed member handles
in place of M9's value-passing `visit`. Every traversal below is library code
over those handles: json encodes with `walk` plus `h.get(value)`, decodes
with `build` and falls back to `h.default()`, and describes a type with a
`walk` that holds no value; `std.cmp` compares two values in one walk, and
`Clone` builds a new value from an old one. Error enums are not derived
here: `@derive(Error)` is a compiler intrinsic (M15).

```text
# ══ std.structure ═════════════════════════════════════════════════
# The compiler supplies Structure and the handle constants. Everything else
# here, and in every library below, is ordinary code. The exact handle API
# is open (M14): Member and VariantInfo are not yet declared by this record.
# The example uses m.name and m.facts on a Member, and v.info.name,
# v.info.index, and v.info.facts on a Variant; facts.find[M]() returns the
# fact of type M, if any, as M?.

pub trait Structure:                     # sealed; exists only inside `by Structure` templates
    fn facts() -> Facts                  # the type-level facts (M13)
    fn walk[W < Walker[Self]](w: mut W) -> Result[void, W::Error]
    fn build[S < Source[Self]](s: mut S) -> Result[Self, S::Error]

pub data Field[S, F]:                    # one constant per member of S, of type F
    pub info: Member

impl[S, F] Field[S, F]:
    pub fn get(self, s: S) -> F:         # one projection; panics on another variant's payload
        pass                             # compiler-supplied
    pub fn has_default(self) -> bool:
        pass                             # compiler-supplied
    pub fn default(self) -> F?:          # evaluates the declared default, if any
        pass                             # compiler-supplied

pub data Variant[S]:                     # one constant per variant of S
    pub info: VariantInfo

impl[S] Variant[S]:
    pub fn holds(self, s: S) -> bool:
        pass                             # compiler-supplied

# An impl may strengthen member's bound (M9, applied to handles by M14).
pub trait Walker[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> bool          # true: walk its members
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]

pub trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]


# ══ std.cmp: one Eq, derived with a walker that holds two values ═══
use std.structure.{Structure, Field, Variant, Walker}

pub trait Eq:                            # the Swift model (M12, EQ-1)
    fn eq(self, other: Self) -> bool

impl[T] Eq for T by Structure:
    fn eq(self, other: Self) -> bool:
        let w: mut EqWalker[T] = EqWalker { a: self, b: other, equal: true, enums: false, entered: false }
        _ := T::walk(w)
        w.equal && (w.entered || !w.enums)   # values of different variants enter none

data EqWalker[S]:
    a: S
    b: S
    equal: bool
    enums: bool
    entered: bool

impl[S] Walker[S] for EqWalker[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> bool:
        self.enums = true
        both := v.holds(self.a) && v.holds(self.b)
        if both:
            self.entered = true
        both

    fn member[F < Eq](mut self, h: Field[S, F]) -> Result[void, never]:
        if self.equal:
            self.equal = h.get(self.a).eq(h.get(self.b))
        .Ok()


# ══ std.clone (illustrative): a new value built from an old one ════
use std.structure.{Structure, Field, Variant, Source}

pub trait Clone:
    fn clone(self) -> Self

impl[T] Clone for T by Structure:
    fn clone(self) -> Self:
        let s: mut CloneSource[T] = CloneSource { old: self }
        match T::build(s):
            .Ok(copy) => copy
            .Err(e) => e                 # e: never

data CloneSource[S]:
    old: S

impl[S] Source[S] for CloneSource[S]:
    type Error = never

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], never]:
        for v in choices:
            if v.holds(self.old):
                return .Ok(v)
        panic("a value holds exactly one variant")

    fn member[F < Clone](mut self, h: Field[S, F]) -> Result[F, never]:
        .Ok(h.get(self.old).clone())     # build asks only for the chosen variant's members


# ══ library json ══════════════════════════════════════════════════
use std.structure.{Structure, Facts, Field, Member, Variant, Walker, Source}
use std.inspect.{Inspectable, TypeId}

pub enum Case:
    Plain
    Camel
    Snake

pub data Style:                          # json's configuration fact; json owns the type
    case: Case = .Plain
    key: Option[fn(Member) -> string] = .None
    tag: string = "type"

# Tier-1 annotation function: evaluated at compile time; requirement-free,
# non-suspending. On a type, its value is a type-level fact (M13).
pub fn json(case: Case = .Plain, key: Option[fn(Member) -> string] = .None, tag: string = "type") -> Style:
    Style { case: case, key: key, tag: tag }

pub data Rename:                         # a fact is a plain value
    name: string

pub fn rename(name: string) -> Rename:
    Rename { name: name }

# Three traits (M11). None has a configuration member: configuration is a
# type-level fact (M13). A schema describes the encoding, so Schema < Encode.
pub trait Encode:
    fn encode(self, out: mut Writer) -> Result[void, EncodeError]

pub trait Decode:
    fn decode(p: mut Parser) -> Result[Self, DecodeError]

pub trait Schema < Encode:
    fn schema(defs: mut Defs) -> Node

# (elided: Writer and Parser, json's output and input streams, where
# Writer {} starts an empty output, Parser { text: text } reads text, and
# p.seek_key(k) reports whether the current object has key k; EncodeError
# and DecodeError, which implement std.error.Error; Defs and Node, a schema
# under construction; the helpers apply_case, find_variant, and
# default_node; hand-written Encode, Decode, and Schema impls for i64,
# string, bool, List[T], Map[K, V], and T?)

# Configuration: a template reads Style from the type-level facts and falls
# back to json's default when none is present (M13).
fn style_of(facts: Facts) -> Style:
    match facts.find[Style]():
        .Some(style) => style
        .None => Style {}

# Precedence is json's choice, not a language rule.
fn key_for(style: Style, m: Member) -> string:
    match m.facts.find[Rename]():
        .Some(r) => r.name
        .None => match style.key:
            .Some(f) => f(m)
            .None => apply_case(style.case, m.name)

# Templates. They never apply by themselves; only the trait's module may
# declare them. Each has a body that walks or builds (M12, M14).
impl[T] Encode for T by Structure:
    fn encode(self, out: mut Writer) -> Result[void, EncodeError]:
        out.begin_object()
        let w: mut Encoder[T] = Encoder { value: self, out: out, style: style_of(T::facts()) }
        T::walk(w)?
        out.end_object()
        .Ok()

impl[T] Decode for T by Structure:
    fn decode(p: mut Parser) -> Result[Self, DecodeError]:
        p.begin_object()?
        let s: mut FieldSource[T] = FieldSource { parser: p, style: style_of(T::facts()) }
        value := T::build(s)?
        p.end_object()?
        .Ok(value)

# Structure gives no type name (open: exact handle API), so the $defs key
# comes from std.inspect under a T < Inspectable bound.
impl[T < Inspectable] Schema for T by Structure:
    fn schema(defs: mut Defs) -> Node:
        key := TypeId::of[T]().to_string()
        if defs.reserve(key):            # false when known or in progress: recursion
            let w: mut SchemaWalker[T] = SchemaWalker { defs: defs, style: style_of(T::facts()), objects: [] }
            _ := T::walk(w)
            defs.define(key, Node.OneOf(w.objects))
        Node.Ref(key)

# The walkers and the source: the only place json's member bounds appear.
data Encoder[S]:
    value: S
    out: mut Writer
    style: Style

impl[S] Walker[S] for Encoder[S]:
    type Error = EncodeError

    fn variant(mut self, v: Variant[S]) -> bool:
        if !v.holds(self.value):
            return false
        self.out.key(self.style.tag)
        self.out.string(apply_case(self.style.case, v.info.name))
        true

    fn member[F < Encode](mut self, h: Field[S, F]) -> Result[void, EncodeError]:
        self.out.key(key_for(self.style, h.info))
        h.get(self.value).encode(self.out)   # F's own impl: nesting follows M7

data FieldSource[S]:
    parser: mut Parser
    style: Style

impl[S] Source[S] for FieldSource[S]:
    type Error = DecodeError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], DecodeError]:
        if !self.parser.seek_key(self.style.tag)?:
            return .Err(DecodeError.Missing(self.style.tag))
        name := self.parser.string()?
        find_variant(choices, name, self.style.case)

    fn member[F < Decode](mut self, h: Field[S, F]) -> Result[F, DecodeError]:
        key := key_for(self.style, h.info)
        if self.parser.seek_key(key)?:
            return F::decode(self.parser)    # type → impl, through F's dictionary
        match h.default():                   # absent key: the declared default (P3)
            .Some(value) => .Ok(value)
            .None => .Err(DecodeError.Missing(key))

data SchemaWalker[S]:
    defs: mut Defs
    style: Style
    objects: mut List[Node]              # one object per variant; one for a data type

impl[S] Walker[S] for SchemaWalker[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> bool:  # no value: enter every variant
        self.objects.append(Node.tagged(self.style.tag, apply_case(self.style.case, v.info.name)))
        true

    fn member[F < Schema](mut self, h: Field[S, F]) -> Result[void, never]:
        if self.objects.is_empty():
            self.objects.append(Node.object())
        node := F::schema(self.defs)
        match h.default():                   # value-free, and the default is reachable
            .Some(d) => self.objects.last_mut().optional(key_for(self.style, h.info), node, default_node(d))
            .None => self.objects.last_mut().required(key_for(self.style, h.info), node)
        .Ok()

pub fn to_json[T < Encode](value: T) -> Result[string, EncodeError]:
    let w: mut Writer = Writer {}
    value.encode(w)?
    .Ok(w.finish())

pub fn from_json[T < Decode](text: string) -> Result[T, DecodeError]:
    T::decode(Parser { text: text })

pub fn schema_of[T < Schema]() -> string:
    let defs: mut Defs = Defs {}
    root := T::schema(defs)
    defs.document(root)


# ══ app ═══════════════════════════════════════════════════════════
use dep.json
use dep.db
use std.clone.{Clone}
use std.error.{Error}
use std.structure.{Member}

# ── Tier 0: no derivation. No Structure, no JSON. ──
data Secret:
    value: string

# ── Tier 1: one annotation per concern, shared by every trait it derives ──
@json.json(case=.Snake)
pub data Address:
    pub streetLine: string               # "street_line"
    pub zipCode: string                  # "zip_code"

@json.json(case=.Camel)
pub data User:
    pub id: i64
    pub full_name: string                # "fullName"
    @json.rename("mail")                 # a declaration fact, visible to every impl
    pub email: string
    pub address: Address                 # Address's own derivation: snake_case (M7)
    pub nickname: string = ""            # absent on input: decode uses "" (P3)

# @json.json(case=.Camel) puts the type-level fact Style { case: .Camel } on
# User, where every impl sees it (M13). It also opts User in to json's
# templates, as if these bodiless tier-2 blocks were written:
#   impl json.Encode for User by Structure
#   impl json.Decode for User by Structure
#   impl json.Schema for User by Structure
# (open: P13) How a tier-1 annotation selects the templates it opts in to is
# not decided; no hook returns the annotation's type any more.

@json.json(tag="kind")
pub enum Event:
    Login(user: i64)
    Logout(user: i64, reason: string)

# Other libraries' templates, applied with bodiless tier-2 blocks.
impl Eq for Event by Structure
impl Clone for Event by Structure

# Generic target: T < Trait for each parameter in a walked member (M12).
# Derived: impl[T < json.Encode] json.Encode for Tree[T], and the same for
# json.Decode and json.Schema. Node's Tree[T] members may assume that impl
# while it is checked (coinductive recursion).
@json.json()
pub enum Tree[T]:
    Leaf(value: T)
    Node(left: Tree[T], right: Tree[T])

# ── Tier 2: one block per trait; lines are local to their block ──
fn legacy_key(m: Member) -> string:
    "x_" + m.name

@db.table("orders")                      # db stays tier 1: a type-level fact for db
pub data Order:
    @db.primary_key()
    pub id: i64
    pub items: List[string]
    pub total_cents: i64
    pub cache: Cache = Cache::empty()    # Cache has no json impls

impl json.Encode for Order by Structure:
    Self += [json.json(key=.Some(legacy_key))]   # hypothetical syntax; this block's facts() only
    total_cents = [json.rename("total")]         # hypothetical syntax; this block's walk only
    cache = pass                                 # hypothetical syntax; not walked

impl json.Decode for Order by Structure:
    Self += [json.json(key=.Some(legacy_key))]   # hypothetical syntax; may differ per direction
    total_cents = [json.rename("total", "total_cents")]   # hypothetical syntax; accept both
    cache = pass                                 # hypothetical syntax; decode uses Cache::empty()

# ── Tier 3: hand-written, no Structure ──
pub data Money:
    cents: i64

impl json.Encode for Money:              # encode-only: no Decode impl (M11)
    fn encode(self, out: mut json.Writer) -> Result[void, json.EncodeError]:
        out.raw(format_decimal(self.cents, places=2))
        .Ok()

# ── Use ──
pub fn main() -> Result[void, Error] $ Console:
    u := User { id: 7, full_name: "Ada L", email: "ada@x",
                address: Address { streetLine: "1 Main", zipCode: "02139" } }
    text := json.to_json(u)?
    println(text)
    # {"id":7,"fullName":"Ada L","mail":"ada@x","address":{"street_line":"1 Main","zip_code":"02139"},"nickname":""}
    let back: User = json.from_json(r"""{"id":7,"fullName":"Ada L","mail":"ada@x","address":{"street_line":"1 Main","zip_code":"02139"}}""")?
    println(back.nickname == "")         # true: the declared default
    println(json.schema_of[User]())      # $defs for User and Address; nickname optional, default ""

    e := Event.Logout(user=7, reason="idle")
    println(json.to_json(e)?)
    # {"kind":"Logout","user":7,"reason":"idle"}
    println(e.eq(e.clone()))             # true: Clone built a new Logout from e
    println(e.eq(Event.Login(user=7)))   # false: different variants, no variant entered
    println(json.to_json(Order { id: 1, items: ["tea"], total_cents: 1250 })?)
    # {"x_id":1,"x_items":["tea"],"total":1250}
    .Ok()
```

(`json.rename("total", "total_cents")` with aliases is illustrative; the
fact vocabulary is json's own. `Self += [...]` keeps `@db.table`'s fact;
`Self = [...]` would replace the type-level facts for that block.)

What the compiler generates (ordinary hd; tooling can print it). Handle
names such as `h_id` and `v_login` are illustrative constants.

```text
# Handles for User, one constant per member:
#   h_id: Field[User, i64], h_full_name: Field[User, string],
#   h_email: Field[User, string]    (h_email.info.facts = [Rename { name: "mail" }]),
#   h_address: Field[User, Address], h_nickname: Field[User, string]
#   h_nickname.has_default() is true; h_nickname.default() evaluates "" and returns .Some("")

# User.walk, inside User's Encode impl, for json's Encoder[User]. The template
# built the Encoder with Style { case: .Camel }, read from T::facts().
fn walk(w: mut Encoder[User]) -> Result[void, EncodeError]:
    w.member[i64](h_id)?                           # needs i64 < Encode ✓
    w.member[string](h_full_name)?
    w.member[string](h_email)?
    w.member[Address](h_address)?                  # needs Address < Encode ✓ (its own @json.json)
    w.member[string](h_nickname)?
    .Ok()

# Event.walk, inside Event's Eq impl, for std.cmp's EqWalker[Event]: it asks
# about every variant and walks the members of the variants it enters.
fn walk(w: mut EqWalker[Event]) -> Result[void, never]:
    if w.variant(v_login):
        w.member[i64](h_login_user)?
    if w.variant(v_logout):
        w.member[i64](h_logout_user)?
        w.member[string](h_logout_reason)?
    .Ok()

# The projection behind h_logout_reason.get, and v_logout.holds.
fn get_logout_reason(s: Event) -> string:
    match s:
        Event.Logout(reason=r) => r
        _ => panic("Field.get: the value holds another variant")

fn holds_logout(s: Event) -> bool:
    match s:
        Event.Logout(_, _) => true
        _ => false

# Event.build, inside Event's Decode impl, for json's FieldSource[Event]
fn build(s: mut FieldSource[Event]) -> Result[Event, DecodeError]:
    choice := s.variant([v_login, v_logout])?
    if choice.info.index == 0:
        .Ok(Event.Login(user=s.member[i64](h_login_user)?))
    else:
        .Ok(Event.Logout(user=s.member[i64](h_logout_user)?, reason=s.member[string](h_logout_reason)?))

# Order.build, inside Order's Decode impl: only that block's lines apply.
# Its facts() are db.table's fact plus the block's Style; the Encode block's
# lines do not reach here. `cache = pass` is shown as M3 states it: no
# member call and the declared default (open: how `= pass` members appear
# in walk and build).
fn build(s: mut FieldSource[Order]) -> Result[Order, DecodeError]:
    .Ok(Order { id: s.member[i64](h_id)?, items: s.member[List[string]](h_items)?,
                total_cents: s.member[i64](h_total_cents)?, cache: Cache::empty() })
    # db.Row's build for Order sees the declaration facts only, and builds cache.
```

At run time each `member[i64]` call runs the `i64`-shaped body of the
walker's `member` with `i64`'s dictionary, a constant, and `h.get` is one
projection; there is no per-member allocation. `h.default()` constructs a
`.Some` only when a decoder finds a key missing, or when a schema reads the
default.

What the compiler rejects:

```text
@json.json()
pub data Bad:
    handle: FileHandle
# error at @json.json: member `handle`: FileHandle does not implement json.Encode
#   use `handle = pass` in a tier-2 block, or change the member's type

@json.json()
pub data Twice:
    x: i64
impl json.Encode for Twice by Structure
# error: overlapping-impl: json.Encode for Twice is already implemented by @json.json
#   (ordinary 09 Overlap; tier 1 is sugar for this impl, as in Rust's E0119)

# in the module that declares the marker trait Audited
pub trait Audited
impl[T] Audited for T by Structure
# error: a `by Structure` template must have a body that walks or builds;
#   there are no marker templates (M12)

fn dump[X < Structure](x: X) -> void:
    pass
# error: Structure may bound only a `by Structure` template

impl Structure for Secret
# error: sealed-trait-implementation

fn sneak[S, W < Walker[S]](w: mut W, h: Field[S, Cache]) -> Result[void, W::Error]:
    w.member[Cache](h)
# error: Walker::member may be called through a generic walker only by
#   generated code

# in std.convert
impl[T, P] From[P] for T by Structure:
    fn from(value: P) -> Self:
        pass
# rejected by design (M15): templates give one trait instantiation per
#   opt-in, never an impl family; per-variant From comes from @derive(Error)
```

Not rejected: a walker whose `variant` answers `true` for a variant that its
value does not hold makes the generated walk call `member` for that
variant's handles, and `h.get` then panics at run time (M14).

### Current Rules (M1-M14)

1. `Structure` is sealed (`impl Structure for T` is
   `sealed-trait-implementation`). It exists only inside `by Structure`
   templates and may bound nothing else.
2. `impl[T] Trait for T by Structure:` declares a template. Only the trait's
   module may declare it, so there is at most one per trait. It never
   applies by itself, so it cannot overlap a hand-written impl (hd otherwise
   has no blanket impls: 09 `bare-parameter-impl-target`). A template must
   have a body that walks or builds; a bodiless template is an error, so
   there are no marker templates (M12, M14).
3. `impl Trait for X by Structure:` (tier 2) applies one trait's template to
   `X`. Its body overrides template methods like default methods and carries
   M3 member lines: `f += [facts]`, `f = [facts]`, `f = pass`. The line
   named `Self` edits the type-level facts: `Self += [facts]`,
   `Self = [facts]` (M13).
4. Member lines, `Self` lines included, are local to their impl (M10).
   Declaration facts, type-level and member, are visible to every impl.
   Tier-2 blocks for related traits (such as `Encode` and `Decode`) are
   written separately and may repeat lines, and their configuration may
   differ per direction (M11, M13).
5. Tier 1: an annotation function runs at compile time and must be
   requirement-free and non-suspending. On a type, its value is a type-level
   fact, as a member annotation's value is a member fact (M13). The
   annotation also opts the type in to templates, as bodiless tier-2 blocks
   would. Which templates it selects is still open (open: P13).
6. Configuration is a type-level fact, not a trait member (M13). There is
   no `visitor()` hook. A template reads its configuration through
   `Structure`, as in `T::facts().find[Style]()` (M14), and falls back to
   its own default when none is present. Derivation adds no member to the
   trait, so it does not affect the trait's dynamic safety. Accepted cost: a
   missing or foreign fact silently means the default.
7. There is no separate duplicate rule: tier 1 is sugar for tier-2 impls, so
   both on one type for one trait is an ordinary `overlapping-impl`
   (09 Overlap).
8. Generic targets (M12): a derived impl for a generic type gets
   `T < Trait` for each type parameter that appears in a walked or built
   member. Recursion is checked coinductively: `Tree[T]` may assume its own
   impl while its members are checked. When a member needs more (a `Set[T]`
   member needs `T < Hash`), the error suggests a tier-2 block with an
   explicit header, such as `impl[T < Trait + Hash] Trait for X[T] by
   Structure`.
9. Handles (M14): for each opted-in type the compiler generates only
   `facts()`, `walk`, `build`, one constant `Field[S, F]` per member, and one
   constant `Variant[S]` per variant. `walk` holds no value: for a data type
   it calls `member` once per member; for an enum it asks `w.variant(v)`
   about each variant and calls `member` only for the members of the
   variants it enters. `build` asks `s.variant(choices)` for an enum and
   then `member` for each member of the chosen variant. `h.get(s)` is one
   projection, and on an enum payload handle it panics for a value of
   another variant. `h.has_default()` reports a declared default, and
   `h.default()` evaluates it. This replaces the value-passing `visit` and
   `member(m, value)` protocol of M9.
10. Walkers and sources (M9, applied by M14): an impl of `Walker[S]` or
    `Source[S]` may strengthen `member[F]`'s bound; only the generated `walk`
    and `build` may call `member` through a generic walker or source;
    templates pass a concrete walker or source; the member obligation is
    checked at the opt-in site, naming the member, and `= pass` members are
    exempt. Two-value traversals (`Eq`, `Ord`, diff) are walkers that hold
    both values and enter a variant only when both hold it; value-to-value
    traversals (clone, patch, shrinking) are sources that read an old value;
    value-free traversals (schemas, CLI help, tool schemas) are walkers that
    hold no value.
11. Nested members use their own derivation; a parent's walker never
    propagates (M7).
12. Per-member customization is metadata only; custom behavior for one member
    means changing the member's type (M6).
13. Comparison (M12, Swift model): there is one `Eq`, with
    `fn eq(self, other: Self) -> bool`, and `PartialEq` is dropped. Floats
    implement `Eq` with IEEE semantics (`NaN != NaN`, a documented law
    exception). `PartialOrd` and `Ord` stay, so floats are
    `Eq + PartialOrd` but not `Ord`. Not yet applied to 09.
14. Impl families are out of scope (M15). A template gives one trait
    instantiation per opt-in. Error enums use the compiler intrinsic
    `@derive(Error)`, which generates `Display`, `Error` with `cause()`, and
    one `From[P]` per `@from` variant
    ([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)).
    A fact type may define a compile-time `check` against its member or
    variant, run at the opt-in site (M15, P14); its form is open.
15. Cost: handles are constants, and generated code passes a constant
    dictionary per member, so there is no per-member allocation. `default()`
    allocates `.Some`; `has_default()` does not (M14).

Still open: how a tier-1 annotation selects its templates (P13); how `= pass`
members appear in `walk` and `build`; whether `T -> U` mapping between two
types is in scope; the exact handle API, including the member and variant
information types and any type-level information beyond `facts()` (the
example's schema takes its `$defs` key from `TypeId`); the compile-time
evaluator's exact limits (proposed: a panic is a compile error at the
annotation, a step budget, and results built from literals, data, enums,
`List`, `Map`, strings, numbers, and references to named functions);
function targets wait for [FN_TYPE.md](FN_TYPE.md); the chapter 14 rewrite
and the removal of `Annotate` wait for the spec style rollout; and the rest
of the [stress test](DERIVATION_STRESS_TEST.md)'s problems.

Found while updating this example to M14, recorded without changing any
decision:

- **`default()` allocation.** M14 says `default()` allocates `.Some` for
  reference-shaped members only. 04 Composite Representation and 05 give
  every `.Some` construction its own identity, as a tagged record that holds
  a scalar unboxed, so `default()` allocates for every member type.
- **"Sealed" walkers and sources.** M9 calls `Visitor` and `Source` sealed,
  and M14 carries that to `Walker` and `Source`, but libraries implement
  them. Under 09 Sealed Traits, any impl of a sealed trait outside the
  standard library is `sealed-trait-implementation`. What M9 means is a
  trait whose `member` bound an impl may strengthen, and whose `member` only
  generated code may call through a generic parameter.
- **`mut` members.** For a member `hits: mut Cell`, `build` needs
  `F = mut Cell` to construct the value, but `get(s: S)` on a readonly `s`
  cannot return `mut Cell` (04 Mutable Paths). One `F` cannot serve both
  (stress test P17).
- **The bound rule names the trait, not the walker's bound.** Rule 8 gives
  `T < Trait`, while the member obligation is the walker's or source's
  strengthened bound. They differ whenever a walker asks for more than the
  trait (for example `F < Schema + Encode`). This example avoids the gap by
  declaring `Schema < Encode`.
- **Superseded spelling.** M13's text reads configuration through
  `T::describe().facts`. M14 replaces `describe()` with `facts()`, so the
  example and rule 6 use `T::facts()`.

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

The three blocks of the [current design](#current-design-full-example-m1-m14)
and its [rules](#current-rules-m1-m14) were rewritten in specification
syntax on 2026-09-27, rewritten again for M14 (typed member handles) the same
day, and checked with the same parser each time. The decision records M8,
M9, and M14 above keep their original spelling.

| Block | Result |
| --- | --- |
| Full example (std.structure, std.cmp, std.clone, library json, app) | As written, `syntax-error` at the first M3/M13 member line (`Self += [...]` in Order's Encode block), as expected. With the six lines marked `# hypothetical syntax` removed (and the colon of an impl header left without a body), parses. `by Structure` parses as the 09 delegation form. |
| Generated code | Parses. |
| Rejected code | Parses; every error shown is semantic. |

Parsing checks syntax only. The M14 surface (`Field`, `Variant`, `Walker`,
`Source`, `facts()`) and the compiler-supplied method bodies, written `pass`,
are not type-checked, and nothing in the example ran.

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
