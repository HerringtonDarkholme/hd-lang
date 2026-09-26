# Type-Checking Rules: Questions For The Owner

## Owner Decisions

Decided 2026-09-26; not yet applied to the specification.

- **TQ-1: MoonBit/Swift overlap rule.** An impl target must start with a type
  constructor (data, enum, newtype, or built-in), never a bare type
  parameter. Two impls overlap when they have the same trait, unifiable trait
  arguments, and the same target constructor; bounds are ignored. The prelude
  `Iterable`-for-`Iterator` adapter is replaced by `for` accepting either
  trait. Rust-style blanket impls may be added later (a compatible
  extension); the standard library must settle its blanket impls before it
  stabilizes.
- **TQ-2: orphan rule.** `impl Trait[Args] for Type` may appear only in the
  package that owns the trait, the target type constructor, or a type
  argument of the trait (when the target is not a bare parameter).
- **TQ-3: method resolution.** For `x.m()`, inherent methods of `x`'s type
  win (Rust's rule); `Trait::m(x)` reaches a trait method. Otherwise promoted
  and trait candidates are pooled regardless of embedding depth, and more
  than one is `ambiguous-method`.


Each question stands alone. Guiding preference: agents write the code and humans
read it, so prefer explicit, checkable rules and locality.

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
