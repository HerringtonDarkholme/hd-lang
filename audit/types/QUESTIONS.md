# Type-Checking Rules: Questions For The Owner

## Owner Decisions

Decided 2026-09-26.

Applied to the specification on 2026-09-26 (see the spec README revision
notes TQ-1, TQ-2, TQ-3, TQ-5, TQ-6, E1 to E5, M1, and later M2, TQ-4,
TQ-27 (partial), TQ-28, and TQ-29):

- **TQ-1** (applied): impl targets start with a type constructor
  (`bare-parameter-impl-target`); overlap is decided by trait, unifying trait
  arguments, and target constructor (TQ-28 later refined this to full-head
  unification); the prelude adapter is gone and `for`
  accepts `Iterable[T]` or `Iterator[T]`.
- **TQ-2** (applied): the owner of a trait argument's outer constructor may
  write the impl; this covers facet packages annotating primitives.
- **TQ-3** (applied): inherent methods win over trait methods, several
  trait candidates are `ambiguous-method`, and `Trait::m(x)` selects one. The
  pooling of promoted and trait candidates is withdrawn; E1 replaces it.
- **TQ-5** (applied): an outer `mut` on an impl target is
  `mutable-impl-target`.
- **TQ-6** (applied): a child trait may not declare a supertrait member name
  (`duplicate-trait-member`).

TQ-7 and TQ-8 are superseded by E1 to E5.

Questions raised while applying these decisions are TQ-27 to TQ-30, and TQ-31
to TQ-35 for E1 to E5 and M1, at the end of this file.


Each question stands alone. Guiding preference: agents write the code and humans
read it, so prefer explicit, checkable rules and locality.

- **Embedding (E1 to E5), applied** to 03 Member Resolution (the one lookup
  algorithm), 08 Data Embedding, 09 Method Resolution and Embedding And Trait
  Satisfaction, and 02 (bodyless implementations). New codes
  `ambiguous-promoted-member`, `private-member`, `trait-not-in-scope`;
  `promoted-mutable-requirement` removed. For `x.name` on nominal type
  `S`: (E1) resolve against `S`'s own members first (fields, inherent
  methods, trait methods of `S`); look into embedded fields only when `S`
  has no member with that name. (E2) Only "no member named `name`"
  triggers the fallback; presence is by name regardless of kind, arity,
  argument types, or visibility. A name present only through a trait that
  is not imported in the calling module is an error ("import or qualify"),
  not a fallthrough. (E3) The fallback searches embedded fields at any
  depth, shortest path wins, and two matches at the same depth are an
  ambiguity error at the use. (E4) The fallback finds fields and inherent
  methods only; an embedded type's trait methods never promote. (E5) A
  promoted method never fills a required trait method; the impl writes the
  body. Replaces TQ-7 and TQ-8.
- **Member namespace (M1), superseded by M2.** Was applied to 03 Member
  Resolution, 05 Member
  Access, and 09 Inherent Implementations; a field and an inherent method
  reuse `duplicate-inherent-member`. Fields and methods share one
  member namespace per type. A field and an inherent method with the same
  name are an error at the declaration; `x.callback()` calls a
  function-typed field; a field and a trait method with the same name are an
  ambiguity error at the use.

- **TQ-4, applied** to 09 Method Resolution. When a type implements one
  generic trait at several instantiations, a dot call infers the
  instantiation from the argument types and expected type (Rust); it is
  `ambiguous-method` only when more than one fits, and `type-mismatch` when
  none fits.
- **TQ-28, applied** to 09 Overlap. Two impls overlap when their trait
  arguments and full targets unify under one substitution: `Box[i32]` and
  `Box[string]` do not overlap; `Box[T]` and `Box[i32]` do. Bounds stay
  ignored. Chapter 14 did not repeat the old wording.
- **TQ-29, applied** to 06 For Loops. When a type implements both
  `Iterable[T]` and `Iterator[T]`, `for` and comprehensions use `Iterable`.
- **TQ-27, tuple and function parts applied** to 09 Implementation Targets
  and Implementation Ownership: tuples are targets (one standard-library
  constructor per arity); a function type target is `function-impl-target`.
  The `Option` part waits on O1 to O3.
- **Optionals (O1 to O3) and TQ-27's `Option` part, not yet applied.** (O1) `T?` is exact
  sugar for a prelude `enum Option[T]: Some(value: T); None`; `T??` is
  `Option[Option[T]]`; `nil` is removed as keyword and literal; the
  representation stays an implementation detail. (O2) The absent value is
  written `.None` or `Option.None`; no new prelude names. (O3) The implicit
  wrap stays: a `T` is accepted where `T?` is expected, one layer only.
  (TQ-27) Impl targets may be nominal types, built-in primitives and
  collections, `Option` (now an enum), and tuples (arity-indexed
  constructors); function types are never impl targets.

- **No overriding, applied** to 03 Member Resolution and 08 Data Embedding.
  A promoted method runs as the embedded type's method (inside `Base`,
  `self.m()` is `Base.m`).
- **Data patterns with `:`, not yet applied.** Data patterns use `:` like data literals
  (`Point { x: 0, y }`, `Point { x: px }`); labels in `Type { }` use `:`,
  labels in `( )` use `=` (grammar Q5).
- **Bound methods stay deferred.** Revisit later.
- **M2: separate field and method namespaces, applied (replaces M1)** to 03
  Member Resolution (a field lookup and a method lookup), 05 Member Access and
  Calls, 08 Data Embedding, and 09 Inherent Implementations and Method
  Resolution. `x.name` looks up fields only; `x.name(args)` looks up methods
  only; a field and a method may share a name; a function-typed field is
  called as `(x.callback)(args)`. A trait adding or renaming a method can
  never collide with a user's field. See [MEMBER_LOOKUP.md](MEMBER_LOOKUP.md).
  TQ-32, TQ-33, and TQ-35 no longer arise.
- **Trait-method promotion, decided and applied (E4 kept; resolves TQ-31).**
  Embedding carries only fields and inherent methods. Trait methods count
  only on the receiver's own type; method lookup skips an embedded type's
  trait methods and keeps searching below it.
- **Embedding critique review (2026-09-26).** (P2, applied to 03
  Member Resolution, 08, 09, and the members fixtures; revises E2) Members not visible from the calling module are skipped, as in
  Rust (rust-lang/rust PR #31938) and Go; inside the defining module the
  private member wins; if nothing visible is found, the private member is
  reported (`private-member`). Private additions never break outside
  callers. An un-imported trait still stops the search.
  (P3) E3 stays: shortest path wins, accepting silent switches when an
  embedded type in another package gains a shallower member, as Rust accepts
  trait imports changing `Deref` resolution. (P4) Accepted: a trait impl for
  `S` added in a third package gives `S` a depth-0 method that blocks an
  embedded member. (P5) TQ-31 stays: trait methods of embedded types are
  never searched. (P6, not yet applied) `Type::name` and `x::name` are
  reserved for future method values, and `x.callback(args)` reports "did you
  mean `(x.callback)(args)`" when a function-typed field `callback` exists.

- **Owned embedding (OE1 to OE4), not yet applied.** (OE1) Access through an
  embedded field follows the container: a `mut` outer value gives `mut`
  access to the embedded part, a readonly one gives readonly; this replaces
  "embedded fields are readonly edges", so promoted `mut self` methods work
  on a `mut` receiver. (OE2) Construction: a readonly value may fill an
  embedded field, but then the constructed value is readonly; a value used
  as `mut` requires every embedded part to be fresh or `mut` (otherwise
  `mutable-upgrade`). (OE3) Copy-update stays shallow: the copy shares
  embedded parts with the original, and the OE2 rule applies to the copy
  (sharing a part of a readonly original makes the copy readonly). (OE4)
  Aliasing out is allowed: `ts := post.Timestamps` on a `mut post` yields a
  `mut` alias; owned means access follows the container, not exclusivity.

- **TQ-4 follow-ups, not yet applied.** When several instantiations fit only
  because of an unsuffixed literal, prefer the literal's default type (`i32`,
  `f64`); otherwise ambiguous. When no instantiation fits, report
  `type-mismatch`, listing the available instantiations.
- **Trait value types are not impl targets, not yet applied.**
  `impl Marker for Display` is an error.
- **Mutable provider install syntax confirmed.** `$.with(mut Clock=clock)`,
  `$.use(mut Clock)`, `$ mut Clock` (applied in chapter 11).

- **TQ-9, not yet applied.** `Type::f` checks inherent, then implemented
  available traits; `T::f` under a bound goes through the bound's
  dictionary; `Trait::f()` with an undetermined `Self` is rejected. `T::f()`
  is allowed under a bound, never through a runtime type object (answers the
  runtime type identity question on static calls).
- **TQ-10: dynamic safety stays literal.** A dynamically safe trait may not
  have methods with row parameters, `reified` parameters, packs, or
  suspension; only `Reference`-bounded method generics are allowed (G2).
- **TQ-11, not yet applied.** Newtypes may carry `@derive(...)`; the derived
  implementations use the base type's behavior.
- **TQ-12 (revised), not yet applied.** Law partners may not mix derived and
  hand-written implementations. Deriving `Hash`, `PartialOrd`, or `Ord`
  requires its partners (`PartialEq`, and `PartialOrd` for `Ord`) to be
  derived in the same list; if any partner is hand-written, all of them must
  be hand-written.

- **TQ-13.** `@derive` stays limited to the comparison and hash traits; a
  single typed derivation protocol, shared by `std` and libraries, comes
  later with the Typed Derivation issue. No ad-hoc additions meanwhile.
- **TQ-14: assignability stays single-step.** `let wide: i64? = small_i8`
  and passing a `User` to a `Display?` parameter need explicit conversions.
- **TQ-15, not yet applied.** Least-common-type inference never constructs
  trait values or widens to a supertrait; a mixed list needs an expected
  type.
- **TQ-16, not yet applied.** Trait parameters are invariant; variance
  markers on trait parameters are rejected.

- **TQ-17, not yet applied.** An inherent impl sits in the target type's
  module; a trait impl sits in the module declaring the trait, the target,
  or the owned trait argument.
- **TQ-18, not yet applied (settles Annotation Locality).** A foreign-target
  annotation may appear in the facet's defining module without a marker, and
  in the root application package under the existing exception with an
  explicit marker; an explicit `impl Annotate[F] for X` obeys the same rule.
- **TQ-19, not yet applied.** Inherent impls of one type constructor may
  repeat a member name when their targets cannot unify (`impl Box[i32]` and
  `impl Box[string]`); `impl[T] Box[T]` and `impl Box[i32]` with the same
  name are `duplicate-inherent-member`.
- **TQ-22, not yet applied (settles runtime identity question 1).**
  `value.downcast[T]()` is a compiler-provided method available only on
  `Inspectable` values and on parameters bounded by `Inspectable`.

- **TQ-23, not yet applied (settles Complete Runtime Shape Coverage).**
  `TypeShape` gains `Mut(inner)`, `Trait(decl, args)`, `Any`,
  `Suspend(result)`, and `Newtype(decl, base)`.
- **TQ-24 to TQ-26: parked.** The owner does not want to discuss
  NonEscapable now. Initial answers, not to be applied until the owner
  reopens the topic: declared-and-checked propagation (TQ-24), opt-in
  generic parameters (TQ-25), no NonEscapable returns (TQ-26).

- **TQ-30, not yet applied.** `Store[User]` and `Store[mut User]` are
  distinct trait instantiations; only the outer `mut` of a target is banned.
- **TY-13, not yet applied.** A trait's default method body sees only the
  trait's members and its supertraits' members; `Self`'s fields are not
  accessible there.

## TQ-1: Do impl bounds prove two impls disjoint?
    impl[T, I < mut Iterator[T]] Iterable[T] for I   # prelude
    impl Iterable[i32] for Bag                       # overlap?
Options: (A) never, and the iterator adapter becomes a compiler rule, not an
impl; (B) yes, with Rust-style knowledge of who may add impls later;
(C) yes, closed-world at link time.
**Recommend A.** Overlap is then decided from heads alone, in one place, and
never breaks when a dependency adds an impl. This is Swift's "conform once".

## TQ-2: Does owning a trait argument allow an impl?
    # package money owns Money
    impl Add[Money] for i32: ...
    # package validation owns Validation
    annotate Validation for string: ...
Options: (A) trait or target owner only, and annotations need a separate
exception; (B) also the owner of a trait argument, when the target is not a bare
type parameter; (C) full Rust ordered rule.
**Recommend B.** It makes today's annotation practice legal, and link-time
conflicts stay impossible.

## TQ-3: Method resolution order
    impl Base:
        fn to_string(self) -> string: "base"
    data Page:
        Base
    impl Display for Page: ...
    page.to_string()
Options: (A) inherent > promoted > trait; (B) inherent, then promoted and trait
pooled, with an ambiguity error; (C) inherent > own trait > promoted.
**Recommend B.** Adding a method elsewhere, or a `use`, never silently changes
the target of a call; it produces an error instead.

## TQ-4: Dot call with several instantiations of one generic trait
    impl Add[i32] for Money
    impl Add[Money] for Money
    price.add(5)
Options: (A) ambiguous, require `Add[i32]::add(price, 5)`; (B) select by
argument types.
**Recommend A.**

## TQ-5: `mut` in impl heads
    impl Marker for mut Counter
Options: (A) forbid outer `mut` in impl targets; (B) allow as the same slot,
as today.
**Recommend A.** Permission belongs to receivers (`self`, `mut self`) and to
bounds, not to slots.

## TQ-6: May a child trait reuse a supertrait member name?
    trait Greeter:
        fn greet(self) -> string
    trait Loud < Greeter:
        fn greet(self) -> string
Options: (A) reject at the trait declaration; (B) allow, and report ambiguity
at uses.
**Recommend A.** The error is local and early.

## TQ-7: May a promoted method replace a trait default?
Superseded by E5: a promoted method never fills a trait method.
    trait Named:
        fn label(self) -> string: "default"
    impl Base:
        fn label(self) -> string: "base"
    data User:
        Base
    impl Named for User
Options: (A) no, promotion fills only methods without a default; (B) yes.
**Recommend A.** Defaults win unless the impl writes the method, so the impl
body shows every override.

## TQ-8: Do an embedded type's trait methods promote?
Superseded by E4: they never promote.
    impl Display for Label
    data Page:
        Label
    page.to_string()
Options: (A) no, only fields and inherent methods promote; (B) yes, when the
trait is in scope.
**Recommend A.** Under B, `Page` answers `to_string` but fails every `Display`
bound. Kotlin delegation is likewise explicit per interface.

## TQ-9: Associated function lookup
    User::create()
    Factory::create()
    fn make[T < Factory]() -> T:
        T::create()
Options: (A) `Type::f` checks inherent, then implemented available traits;
`T::f` goes through the bound's dictionary; `Trait::f()` is rejected when
`Self` is undetermined. (B) allow `Trait::f()` with `Self` inferred from the
expected type.
**Recommend A.** This also answers Open Issues runtime type identity Q2: allow
`T::f()` under a bound, never through a runtime type object.

## TQ-10: Dynamic safety of row, reified, pack, and suspending methods
    trait Runner:
        fn run[r](self, job: fn() -> void $ r) -> void $ r
Options: (A) allow suspending, row (providers passed keyed), and `reified`
methods, but reject packs; (B) reject everything except `Reference`-bounded
types (the literal reading today).
**Recommend A.** Each allowed form still needs only one body.

## TQ-11: `@derive` on newtypes
    @derive(PartialEq, Eq, Hash)
    type Mile(i32)
Options: (A) allow it by extending the grammar; the derived impl uses the
base type's behavior; (B) delete "or derives" from chapter 04.
**Recommend A.** Newtype map keys are a primary use.

## TQ-12: Must law partners be derived together?
    impl PartialEq for Account: ...   # compares id only
    @derive(Hash)                     # hashes every field
Options: (A) any mix, as today; (B) deriving `Hash`, `PartialOrd`, or `Ord`
requires `PartialEq` (and `PartialOrd`, for `Ord`) derived in the same list;
(C) a warning.
**Recommend B.** Consistency then holds by construction; a hand-written
partner means writing both by hand.

## TQ-13: Which traits are derivable, and does derivation open to libraries?
Options: (A) the closed comparison and hash set; (B) add `Display`, `Default`,
and debug formatting; (C) open derivation through the typed protocol from the
Typed Derivation issue (options 1+3).
**Recommend A now and C later**, as one protocol that `std` also uses. No
ad-hoc additions.

## TQ-14: Assignability compositions
    let wide: i64? = small_i8
    note(user)            # note(value: Display?)
    let shown: Display = mutable_user
Options: (A) single step, which rejects all three; (B) a fixed list of
two-step compositions (widening/weakening then injection; construction or
widening then injection; weakening then construction); (C) the transitive
closure.
**Recommend B.** Weakening with variance stays deferred.

## TQ-15: Does least-common-type inference use dynamic supertrait widening?
    items := [shown_value, tagged_value]   # both extend Named
Options: (A) yes, when there is a unique least supertrait; (B) no, require an
expected type.
**Recommend B**, consistent with the existing ban on construction.

## TQ-16: Variance of trait parameters
    trait Source[+T]
Options: (A) invariant, and markers are rejected; (B) declared variance for
dynamic values.
**Recommend A.** Supertrait widening already rewraps metadata, so variance
there would not preserve representation.

## TQ-17: Module-level impl locality
    # pkg/far/away.hd
    impl User:
        fn to_string(self) -> string: ...
Options: (A) anywhere in the owning package, as today; (B) a trait impl sits
in the module declaring the trait, the target, or the owned trait argument;
an inherent impl sits in the target's module.
**Recommend B.** A reader finds every impl of a type in two known places.

## TQ-18: Annotation locality (Open Issues, three sub-questions)
Recommend matching TQ-2 and TQ-17:
1. A foreign-target annotation may appear in the facet's defining module, and
   in the root package under the existing exception.
2. No marker is needed in the facet module. The root orphan case gets an
   explicit marker.
3. Yes: an explicit `impl Annotate[F] for X` obeys the same rule.

## TQ-19: Generic inherent impls
    impl[T < Display] Box[T]:
        fn show(self) -> string
    impl Box[i32]:
        fn show(self) -> string
Options: (A) names are unique per type constructor across all inherent impls;
(B) allow them when disjoint.
**Recommend A.**

## TQ-20: Where-predicate subjects and solver termination
    impl[T] Loop for T where Box[T] < Loop
Options: (A) require every predicate subject to contain an impl parameter,
plus a fixed depth limit as a backstop; (B) only a depth limit, as in Rust;
(C) Paterson-style structural conditions.
**Recommend A.**

## TQ-21: Associated-type equality constraints (`S < Supplier[Item = string]`)
Options: (A) list them under Confirmed Deferred Type Features; (B) design them
now.
**Recommend A.**

## TQ-22: Impls for trait value types, and where `downcast` lives
    impl Describe for Display
Options: (A) no impl may target a trait value type or `Any`, and `downcast` is
a compiler-provided method limited to `Inspectable`; (B) allow impls on trait
value types.
**Recommend A.** This answers Open Issues runtime type identity Q1.

## TQ-23: Complete runtime shape coverage
Recommend option 1 of the existing issue: add `Mut(inner)`,
`Trait(decl, args)` (a dynamic value's trait; supertraits reachable through
decl), `Any`, `Suspend(result)`, and `Newtype(decl, base)`. Keep
`unrepresentable-type-shape` until they are specified.

## TQ-24: How NonEscapable propagates
    data HiddenFile:
        file: File        # File is NonEscapable
Options: (A) structural and automatic, like Rust auto traits; (B) declared: a
data or enum type holding a NonEscapable field must itself be declared
NonEscapable, and the compiler checks it; generic types are NonEscapable
exactly when an argument is.
**Recommend B.** It is visible in the declaration, and a public type's
property cannot change when a private field changes (Swift's resilience
argument). In both options, erasure to `Any` or to a trait value that does not
extend NonEscapable is rejected. Provider values may be NonEscapable only
through such traits.

## TQ-25: Do generic parameters accept NonEscapable arguments by default?
Options: (A) no, a parameter accepts them only when it opts in (Swift's
`~Escapable` model); (B) yes, and every generic body is checked
conservatively.
**Recommend A.** Existing generic code stays valid, and opt-in is explicit.

## TQ-26: Dependent-return provenance
    fn first_line(file: File) -> Line    # Line keeps file alive
Options: (A) a NonEscapable result depends conservatively on all NonEscapable
parameters, with no syntax; (B) the signature names the parameters it depends
on; (C) no NonEscapable returns.
**Recommend A now and B later**, when a real API needs the precision.

## TQ-27: Which target forms count as a type constructor?
    impl Display for (i32, string)
    annotate Validation for string?      # used by full-validation.hd
    impl Marker for fn(i32) -> i32
TQ-1 names data, enum, newtype, and built-ins. Tuples, optionals, and function
types are unstated. If `T?` is one constructor, `annotate Validation for
string?` and `annotate Validation for i32?` overlap.

## TQ-28: Same constructor, different arguments
    impl Marker for Box[i32]
    impl Marker for Box[string]          # applied text: overlapping-impl
The applied text reads TQ-1 literally (Swift's conform-once), so exact
instantiations of one constructor cannot both implement a trait, and
`annotate Validation for list[string]` excludes `list[i32]`. Confirm, or
switch to "targets unify".

## TQ-29: A type that implements both `Iterable[T]` and `Iterator[T]`
    impl Iterable[i32] for Ring
    impl Iterator[i32] for Ring
    for x in ring: ...                   # iter() or next()?
Both impls are now legal. Options: (A) `Iterable` wins; (B) ambiguity
error at the loop; (C) forbid the pair.

## TQ-30: Permission inside trait arguments
    impl Store[User] for Shelf
    impl Store[mut User] for Shelf       # distinct instantiations?
TQ-5 settled only the outer `mut` of targets.

## TQ-31: Does an embedded type's trait method stop the search below it?
    impl Base:
        fn to_string(self) -> string: "base"
    data Label:
        Base
    impl Display for Label: ...
    data Page:
        Label
    page.to_string()                     # Base's method at depth 2, or error?
E3 and E4 read literally: depth 1 has no match (trait methods are not
matches), so depth 2 finds `Base.to_string`. Yet `label.to_string()` reaches
Label's `Display` method. The applied text follows the literal reading.
Options: (A) keep; (B) a trait method of an embedded type blocks the search
through that path.
**Resolved: A.** The search skips embedded trait methods; see
`runtime/valid/embedded-trait-method-skipped.hd`.

## TQ-32: Bare field read beside a trait method of the same name
    impl Named for User:
        fn name(self) -> string: self.name   # field read
M1 makes a field and a trait method with one name ambiguous "at the use".
The applied text makes only the call `x.name(args)` ambiguous; a bare
`x.name` reads the field, because bare method values are deferred. Under the
other reading, nothing could read the field except a pattern. Confirm.
**Moot under M2:** `x.name` is always a field read.

## TQ-33: Field and inherent associated function with one name
    data User:
        guest: bool
    impl User:
        fn guest() -> User: ...
M1 names inherent methods only. Associated functions are reached through
`User::guest()`, never with a dot. Options: (A) allowed; (B)
`duplicate-inherent-member`.
**Moot under M2:** a field and any inherent member may share a name.

## TQ-34: Visibility of promoted members at depth
E2 makes an own member present regardless of visibility. The applied text
extends this to embedded levels: a match counts whatever its visibility, and
an invisible selected member, or an invisible embedded field on its path, is
`private-member`. The alternative skips invisible matches and keeps
searching. Confirm.

## TQ-35: Which diagnostic wins when an own member is both invisible and unavailable?
    # S has a private field `tag` and implements an unimported trait with `tag`
The applied text reports `private-member` when a field or inherent method is
present but not visible, and `trait-not-in-scope` otherwise. Confirm.
**Moot under M2:** the field and the trait method are in different
namespaces, so `x.tag` reports `private-member` and `x.tag()` reports
`trait-not-in-scope`.
