# Type-Checking Rules: Questions For The Owner

Applied decisions have been removed from this file; the spec's Revision Notes
in `spec/README.md` are their record (TQ-1 to TQ-6, TQ-4 and its follow-ups,
TQ-27 to TQ-29, TQ-31 and TQ-36, E1 to E5, M2, O1 to O3, P2, P6, VE and VE-S,
Cut 2, trait delegation, the embedding limits, the single view, A2, A3, and
C1 to C3). Superseded questions (TQ-7, TQ-8, TQ-32 to TQ-35) are gone too.
TQ-21 is answered by K2: a bound may bind associated types, as in
`I < Supplier[Item = T]`. Findings with the evidence are in
[FINDINGS.md](FINDINGS.md), and the proposed rule text in
[PROPOSED_RULES.md](PROPOSED_RULES.md).

## Decided, Not Yet Applied

Decided 2026-09-26. None of these is in the specification yet.

- **TQ-9** (TY-10). `Type::f` checks inherent, then implemented available
  traits; `T::f` under a bound goes through the bound's dictionary;
  `Trait::f()` with an undetermined `Self` is rejected. `T::f()` is allowed
  under a bound, never through a runtime type object (answers the runtime
  type identity question on static calls).
- **TQ-10** (TY-14): dynamic safety stays literal. A dynamically safe trait
  may not have methods with row parameters, `reified` parameters, packs, or
  suspension; only `Reference`-bounded method generics are allowed (G2).
  09 Dynamic Trait Values states the `Reference` rule but not the exclusion
  of suspending methods.
- **TQ-11** (TY-16). Newtypes may carry `@derive(...)`; the derived
  implementations use the base type's behavior.
- **TQ-12** (TY-19), revised. Law partners may not mix derived and
  hand-written implementations. Deriving `Hash`, `PartialOrd`, or `Ord`
  requires its partners (`PartialEq`, and `PartialOrd` for `Ord`) to be
  derived in the same list; if any partner is hand-written, all of them must
  be hand-written.
- **TQ-15** (TY-21). Least-common-type inference never constructs trait
  values or widens to a supertrait; a mixed list needs an expected type.
- **TQ-16** (TY-30). Trait parameters are invariant; variance markers on
  trait parameters are rejected.
- **TQ-17** (TY-25). An inherent impl sits in the target type's module; a
  trait impl sits in the module declaring the trait, the target, or the owned
  trait argument.
- **TQ-18** (settles Annotation Locality). A foreign-target annotation may
  appear in the facet's defining module without a marker, and in the root
  application package under the existing exception with an explicit marker;
  an explicit `impl Annotate[F] for X` obeys the same rule.
- **TQ-19** (TY-26). Inherent impls of one type constructor may repeat a
  member name when their targets cannot unify (`impl Box[i32]` and
  `impl Box[string]`); `impl[T] Box[T]` and `impl Box[i32]` with the same
  name are `duplicate-inherent-member`.
- **TQ-22** (TY-27, settles runtime identity question 1).
  `value.downcast[T]()` is a compiler-provided method available only on
  `Inspectable` values and on parameters bounded by `Inspectable`. (The
  other half, `trait-value-impl-target`, is applied.)
- **TQ-23** (settles Complete Runtime Shape Coverage). `TypeShape` gains
  `Mut(inner)`, `Trait(decl, args)`, `Any`, `Suspend(result)`, and
  `Newtype(decl, base)`; 14 today has `Newtype(base)` only.
- **TQ-30** (TY-06). `Store[User]` and `Store[mut User]` are distinct trait
  instantiations; only the outer `mut` of a target is banned.
- **TY-13.** A trait's default method body sees only the trait's members and
  its supertraits' members; `Self`'s fields are not accessible there.

## Decided, No Specification Change

- **TQ-13.** `@derive` stays limited to the comparison and hash traits; a
  single typed derivation protocol, shared by `std` and libraries, comes
  later with the Typed Derivation issue. No ad-hoc additions meanwhile.
- **TQ-14** (TY-20): assignability stays single-step. `let wide: i64? =
  small_i8` and passing a `User` to a `Display?` parameter need explicit
  conversions.

## Open Questions

### TQ-20: Bound-solving termination

K2 removed `where` clauses, so every bound is written inline and its subject
is a generic parameter; the original question about predicate subjects no
longer arises. What remains is termination: should the solver carry a fixed
depth limit as a backstop (the recommendation), rely on a depth limit alone
as Rust does, or impose Paterson-style structural conditions?

## Parked

TQ-24 to TQ-26: the owner does not want to discuss NonEscapable now. The
initial answers, not to be applied until the owner reopens the topic, are
declared-and-checked propagation (TQ-24), opt-in generic parameters (TQ-25),
and no NonEscapable returns (TQ-26).

### TQ-24: How NonEscapable propagates
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

### TQ-25: Do generic parameters accept NonEscapable arguments by default?
Options: (A) no, a parameter accepts them only when it opts in (Swift's
`~Escapable` model); (B) yes, and every generic body is checked
conservatively.
**Recommend A.** Existing generic code stays valid, and opt-in is explicit.

### TQ-26: Dependent-return provenance
    fn first_line(file: File) -> Line    # Line keeps file alive
Options: (A) a NonEscapable result depends conservatively on all NonEscapable
parameters, with no syntax; (B) the signature names the parameters it depends
on; (C) no NonEscapable returns.
**Recommend A now and B later**, when a real API needs the precision.
