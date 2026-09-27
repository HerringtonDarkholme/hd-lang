# Type-Checking Rules: Proposed Normative Text

Draft text for chapters 04 and 09, ready to paste once approved. A rule tagged
**[TQ-n]** encodes the recommended answer to that question and changes if the
owner decides otherwise. Untagged rules restate or complete behavior the spec
already implies. Codes marked *(new)* are not in the README table yet.

## R0. Terms

- **Head** of `impl[P..] Tr[A..] for X`: the pair `(Tr[A..], X)`.
- **strip(V)**: `V` with an outer `mut` removed.
- **Slot**: `(Tr[A..], strip(X))`. Coherence counts impls per slot.
- **View**: the type through which a value is accessed, `X` or `mut X`.
- **Assumption set Γ**: the bounds in scope. These are generic-parameter bounds,
  impl bounds, `where` predicates, GADT arm equalities, and existential bounds,
  closed under R5.3.
- **Obligation** `V < Tr[A..]`: a requirement to be proven.
- **Evidence**: what a proof selects. This is an impl instantiation, an assumption,
  or a compiler-provided rule. The dictionary passed at run time is the evidence.

## R1. Implementation Declarations

- R1.1 A trait impl defines every trait method without a default
  (`missing-trait-method`). Every written method matches the instantiated
  requirement exactly, including receiver permission, suspension marker,
  generic parameters and bounds, and normalized row (`trait-method-signature`).
  Extra methods are rejected (`trait-method-signature`); use an inherent impl.
- R1.2 An impl binds every associated type of the trait with `type N = T`,
  and binds nothing else.
- R1.3 **[TQ-5]** An impl target must not be written with an outer `mut`
  (`permission-in-impl-head` *(new)*). Trait arguments are complete types:
  `Store[User]` and `Store[mut User]` are distinct instantiations.
- R1.4 Every impl parameter occurs in a trait argument or in the target type.
  Otherwise the impl is rejected (`unconstrained-impl-parameter` *(new)*).
  Projections such as `I::Item` do not count as separate parameters.
- R1.5 Supertrait obligations are checked per R5.1.
- R1.6 **[TQ-20]** The subject of every `where` predicate and inline bound
  contains at least one impl parameter. A predicate with no parameter is
  rejected (`unconstrained-where-predicate` *(new)*).
- R1.7 Local impls: as in chapter 09 today (`local-impl-nonlocal-pair`).

## R2. Ownership

- R2.1 **[TQ-2]** A trait impl is allowed in package P when one of these holds:
  (a) P owns the trait;
  (b) P owns the outer nominal constructor of the target;
  (c) P owns the outer nominal constructor of some trait argument, and the
  target is not a bare impl type parameter.
  Otherwise report `orphan-impl`. Transparent aliases confer nothing; newtypes
  confer ownership; `std` owns primitives, built-in collections, and tuples.
  `annotate F for X` follows the same rule through its lowered
  `impl Annotate[F] for X`, plus the root-package exception of chapter 14.
- R2.2 An inherent impl is allowed only in the package that owns its target.
  Its target is a nominal type, not a trait value, primitive, alias, or
  foreign type (`orphan-impl`).
- R2.3 **[TQ-17]** Within the owning package, a trait impl sits in a module
  that declares one of the things that gave ownership under R2.1. An inherent
  impl sits in the module that declares its target. Otherwise report
  `nonlocal-impl` *(new)*.

## R3. Coherence

- R3.1 A resolved program has at most one impl per slot. Checking happens per
  package at compile time and again at link time over all interface files.
- R3.2 **[TQ-1]** Two impls of the same trait overlap when their trait
  arguments and stripped targets unify under some substitution. Bounds and
  `where` predicates are not consulted. Overlap is `overlapping-impl`,
  reported on the impl that is later by module identity, then by source
  position.
- R3.3 Derived impls (R10) and lowered annotation impls take part in the same
  table. For annotation slots, report `duplicate-annotation-impl` or
  `overlapping-annotation-impl` instead.
- R3.4 Compiler-provided rules (R13) are not impls. A written impl in a slot
  that a compiler-provided rule covers is `overlapping-impl`.
- R3.5 A head containing `(Ts...)` unifies with a tuple of any arity.

## R4. Satisfaction

`Γ ⊢ V < Tr[A..]` holds by exactly one of:

- R4.1 (Assumption) the obligation is in Γ after normalization. Γ includes
  GADT arm equalities.
- R4.2 (Impl) exactly one impl `impl[P..] Tr[B..] for X` has a substitution σ
  with `σX` equal to strip(V), `σB` equal to `A`, and every bound of the impl
  satisfied under `Self := V`.
- R4.3 (Provided) a rule from R13 applies.
- R4.4 A view satisfies a trait for itself. Readonly `X` and `mut X` share one
  slot, but a bound written `mut Tr` or a bound that requires a `mut` view
  holds only for `mut X`. `Self` inside the selected impl is `V`.
- R4.5 If both an assumption and an impl apply, the assumption is used. By
  R3.1, the evidence is observably the same.
- R4.6 An unmet obligation is `unsatisfied-trait-bound`. The message names
  the type, the trait, and the origin of the obligation.
- R4.7 **[TQ-20]** Proof search that nests more than 64 obligations is
  `trait-resolution-depth` *(new)*.
- R4.8 A bound on a pack, `Ts... < Tr`, is one obligation per element, and each
  element passes its own evidence.
- R4.9 Constructing a GADT variant with bounded existential parameters stores
  the evidence for those bounds in the value. Matching the variant adds the
  bounds to Γ for that arm, backed by the stored evidence.
- R4.10 `T::N` resolves when exactly one trait among T's elaborated bounds
  declares `N`. Otherwise report `ambiguous-projection` *(new)*.

## R5. Supertraits

- R5.1 For `impl[P..] Child[A..] for X`, each supertrait `Parent[B..]` of
  `Child`, after substitution, must be provable for `X` under the impl's own
  bounds as Γ. Otherwise report `missing-supertrait-implementation` on the
  impl.
- R5.2 Supertrait cycles: as today (`supertrait-cycle`).
- R5.3 (Elaboration) An assumption `T < Child[A..]` adds `T < Parent[B..]`
  for every transitive supertrait, after substitution.

## R6. Default Methods

- R6.1 A default body is checked once, inside the trait, with Γ = {`Self <`
  the trait}. Names in it resolve against that bound and never against methods
  of the eventual implementing type.
- R6.2 A default applies to an impl unless the impl writes the method, or a
  promoted method fills it under R7.2 (with the TQ-7 answer).
- R6.3 A trait provides defaults only for its own members.
- R6.4 **[TQ-6]** A trait must not declare a member whose name is also a
  member of one of its transitive supertraits (`duplicate-trait-member`).
- R6.5 If more than one trait supplies a dot-call candidate with the same
  name, the call is `ambiguous-method`, whether the methods are defaults or
  written. An inherent method, or a qualified `Trait::m` call, resolves it.

## R7. Embedding

- R7.1 Embedding never grants conformance.
- R7.2 **[TQ-7]** In an explicit impl, a trait method that has no default and
  is not written is filled by the unique shortest promoted inherent method of
  that name. The promoted signature must equal the instantiated requirement
  exactly. A missing or ambiguous candidate is `missing-trait-method`, and the
  diagnostic lists the candidates.
- R7.3 A `mut self` requirement cannot be filled by promotion
  (`promoted-mutable-requirement`).
- R7.4 **[TQ-8]** Only fields and inherent methods are promoted. An embedded
  type's trait methods are not.

## R8. Dynamic Trait Values

- R8.1 **[TQ-10]** A trait is dynamically safe when it and every supertrait
  meet all of these:
  - it declares no associated types or associated functions;
  - `Self` occurs only as a receiver;
  - every method-level type parameter is bounded by `AnyRef`;
  - suspending methods are allowed;
  - row parameters are allowed, and providers are passed keyed;
  - `reified` parameters are allowed;
  - pack parameters are not allowed.
  Otherwise, using the trait as a value type is `trait-not-dynamically-safe`.
- R8.2 A dynamic `Tr` value satisfies `Tr` and its supertraits, and exposes
  exactly their methods.
- R8.3 Construction: `S` to `Tr` when `S < Tr`, and `mut S` to `mut Tr`.
  `S` to `mut Tr` is `mutable-upgrade`.
- R8.4 A dynamic child value widens to a supertrait value. There is no reverse
  conversion.
- R8.5 A requirement key must be a dynamically safe trait instantiation
  (`trait-not-dynamically-safe`, reported at the row).
- R8.6 Method bodies reached through dynamic dispatch are the impl's bodies.
- R8.7 **[TQ-22]** No impl, inherent or trait, may target a dynamic trait value
  type or `Any` (`orphan-impl`).
- R8.8 **[TQ-16]** Trait parameters are invariant, and variance markers on a
  trait are rejected (`invalid-variance`).

## R9. Method Resolution

For `e.m(args)`, where `e` has view `V`:

- R9.1 Candidates are the members named `m` that are visible from the calling
  module.
- R9.2 Tier 1: inherent methods of strip(V)'s nominal type, or of a built-in
  receiver.
- R9.3 **[TQ-3]** If tier 1 is empty, tier 2 is the union of:
  - unique shortest promoted inherent methods (R7.4);
  - methods of traits that are available (chapter 09 scope rule, or the bounds
    of a generic receiver) and that V satisfies.
  Candidates that denote the same body count once, as when a promotion fills
  an impl slot. More than one candidate is `ambiguous-method`.
- R9.4 A dynamic trait value receiver has only R8.2 methods.
- R9.5 **[TQ-4]** If the candidates come from several instantiations of one
  generic trait, the call is `ambiguous-method`. Argument types never select
  among them.
- R9.6 Selection uses names only. Receiver permission is checked after
  selection (R12.2), and a failed check never falls through to another
  candidate.
- R9.7 `Trait[A..]::m(recv, ..)` selects that trait's own member `m`.
  Supertrait members need their own trait name.
- R9.8 **[TQ-9]** Associated function calls:
  - `Type::f` looks up inherent functions first. If there are none, it looks
    up functions of traits that are available and implemented by `Type`; more
    than one is `ambiguous-method`.
  - `T::f` with `T` generic resolves through T's bounds (exactly one must
    declare `f`), using the dictionary.
  - `Trait::f` for a receiverless `f` is rejected unless the trait's arguments
    determine `Self`; write `Type::f` or `T::f` instead.
- R9.9 **[TQ-19]** Inherent member names are unique across all inherent impls
  of one nominal type constructor (`duplicate-inherent-member`). A bounded
  inherent method is a candidate only when its bounds hold for the receiver.

## R10. Derivation

- R10.1 **[TQ-11]** `@derive(Tr, ..)` applies to module-level data, enum, and
  newtype declarations. For a newtype, the derived impl behaves like the base
  type's impl applied to the single field.
- R10.2 **[TQ-13]** Derivable traits are `PartialEq`, `Eq`, `PartialOrd`,
  `Ord`, and `Hash`. Any other trait is `underivable-trait` *(new)*.
- R10.3 The generated impl is `impl[P..] Tr for D[P..]` with bound `Pi < Tr`
  for each parameter `Pi` that occurs in a field the trait uses.
- R10.4 Each used field type must satisfy `Tr` under those bounds, with the
  impl being derived also available. This covers recursion. Failures report:
  `missing-partial-eq` for `PartialEq`, `field-not-eq` for `Eq` and `Ord`,
  `missing-partial-ord` for `PartialOrd`, and `field-not-hash` for `Hash`. The
  diagnostic sits on the field.
- R10.5 Using the impl at a type where the added bound fails is
  `missing-derived-bound`.
- R10.6 An enum with a variant that has an existential parameter cannot derive
  `PartialEq`, `Eq`, `PartialOrd`, or `Ord`. It can derive `Hash` only when
  every existential field is `Hash`-bounded. Otherwise report
  `underivable-variant` *(new)*.
- R10.7 The generated impl belongs to the declaration's module and package,
  and follows R2 and R3. A derive plus a written impl for the same slot is
  `overlapping-impl`.
- R10.8 Supertraits must be satisfied by an existing impl or by another derive
  in the same list, in any order.
- R10.9 Field order and variant order are as chapter 09 states today.
- R10.10 **[TQ-12]** Deriving `Hash`, `PartialOrd`, or `Ord` requires the
  type's `PartialEq` to be derived in the same list, and `Ord` also requires
  `PartialOrd`. Otherwise report `mixed-derived-law` *(new)*.

## R11. Assignability And Least Common Type

- R11.1 **[TQ-14]** Assignability is rules 1-9 of chapter 04, plus `never` to
  any type, plus these two-step compositions and no others:
  - widening or weakening, then optional injection;
  - trait construction or supertrait widening, then optional injection;
  - weakening, then trait construction (`mut S` to `Tr`).
  Weakening combined with variance stays deferred.
- R11.2 `mut S` to `mut Tr` is rule 6 applied at the mutable view (R8.3).
- R11.3 **[TQ-15]** Least-common-type inference does not use trait
  construction (rule 6) or supertrait widening (rule 7).
- R11.4 Operands of type `never` are dropped before computing the least common
  type. If every operand is `never`, the result is `never`.
- R11.5 If a common type exists only through a conversion R11.3 excludes, the
  error is `no-common-type`.
- R11.6 If a candidate needs weakening plus variance, it is not a candidate
  (R11.5 applies).

## R12. Mutable Paths

- R12.1 An expression has **mutable access** when its type is `mut U`, or a
  type parameter bounded by `mut Tr` or `mut Any`, or `Self` inside a
  `mut self` method, or a dynamic `mut Tr`. Chapter 04 uses "has mutable
  access" wherever it says "has type `mut T`".
- R12.2 A `mut self` call or container-mutating method on a receiver without
  mutable access is `mutable-receiver-required`, including through promotion.
  Field reassignment and element replacement keep `readonly-edge` and
  `readonly-root`.

## R13. Compiler-Provided Traits

- R13.1 **Sealed** traits: user impls are `sealed-trait-implementation`. The
  sealed traits are `AnyRef`, `Suspend[T]`, `ShapeMetadata`, `Any`, and,
  when adopted, `Inspectable` and `NonEscapable`.
- R13.2 `Any` is satisfied by every type except optionals, `void`, and
  `never`. `mut X` satisfies `mut Any` when X is a composite.
- R13.3 `AnyRef`: as the prelude lists today.
- R13.4 Provided evidence is constant and cannot be overridden.
- R13.5 **[TQ-1]** Iterator adapter: a `mut X` view with `mut X < Iterator[T]`
  satisfies `Iterable[T]` with `iter` returning itself. A type with any
  `Iterator` impl must not also have an `Iterable` impl (`overlapping-impl`).

## R14. Diagnostics

Existing codes, now tied to the rules above:

| Code | Rule |
| --- | --- |
| `ambiguous-method` | R6.5, R9.3, R9.5, R9.8 |
| `orphan-impl` | R2.1, R2.2, R8.7 |
| `overlapping-impl` | R3.2, R3.4, R10.7, R13.5 |
| `trait-not-dynamically-safe` | R8.1, R8.5 |
| `sealed-trait-implementation` | R13.1 |
| `promoted-mutable-requirement` | R7.3 |
| `missing-derived-bound` | R10.5 |
| `field-not-eq`, `field-not-hash`, `missing-partial-eq`, `missing-partial-ord` | R10.4 |
| `mutable-receiver-required` | R12.2 |
| `missing-supertrait-implementation` | R5.1 |

Proposed new codes: `permission-in-impl-head`,
`unconstrained-impl-parameter`, `unconstrained-where-predicate`,
`nonlocal-impl`, `trait-resolution-depth`, `ambiguous-projection`,
`underivable-trait`, `underivable-variant`, `mixed-derived-law`.
