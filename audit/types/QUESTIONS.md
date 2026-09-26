# Type-Checking Rules: Questions For The Owner

## Owner Decisions

Decided 2026-09-26.

Applied to the specification on 2026-09-26 (see the spec README revision
notes TQ-1, TQ-2, TQ-3, TQ-5, TQ-6):

- **TQ-1** (applied): impl targets start with a type constructor
  (`bare-parameter-impl-target`); overlap is decided by trait, unifying trait
  arguments, and target constructor; the prelude adapter is gone and `for`
  accepts `Iterable[T]` or `Iterator[T]`.
- **TQ-2** (applied): the owner of a trait argument's outer constructor may
  write the impl; this covers facet packages annotating primitives.
- **TQ-3** (partly applied): inherent methods win over trait methods, several
  trait candidates are `ambiguous-method`, and `Trait::m(x)` selects one. The
  pooling of promoted and trait candidates is withdrawn pending the embedding
  redesign.
- **TQ-5** (applied): an outer `mut` on an impl target is
  `mutable-impl-target`.
- **TQ-6** (applied): a child trait may not declare a supertrait member name
  (`duplicate-trait-member`).

TQ-7 and TQ-8 are on hold for the embedding redesign.

Questions raised while applying these decisions are TQ-27 to TQ-30 at the end
of this file.


Each question stands alone. Guiding preference: agents write the code and humans
read it, so prefer explicit, checkable rules and locality.

- **Embedding (E1 to E5), not yet applied.** For `x.name` on nominal type
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
  body. Replaces TQ-7 and TQ-8. Open: whether fields and methods share one
  member namespace (see the member-namespace questions).

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
