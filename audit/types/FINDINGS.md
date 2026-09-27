# Type-Checking Rules: Findings

Roadmap area 2. Scope: Type System (04), Traits (09), Variadic Generics (12),
GADTs (13), and the trait-related parts of 03, 05, 06, 07, 08, 10, 11, 14.
Audited at commit 158a430; re-checked against the specification on 2026-09-26.
Resolved findings are removed (TY-01, TY-02, TY-03, TY-09, TY-11, TY-12,
TY-33); the decisions that settled them are in the spec's Revision Notes.
Decisions taken but not yet applied are in [QUESTIONS.md](QUESTIONS.md).

Severity: **High**: two normative statements contradict, or a permitted reading
makes ordinary programs unsound or impossible. **Medium**: unspecified; careful
implementations would disagree. **Low**: recoverable from context, but a code,
anchor, or statement is missing.

| ID | Sev | Topic | Summary | Rule | Question |
| --- | --- | --- | --- | --- | --- |
| TY-04 | High | Impl selection | "Constrained by a reachable bound" lets one impl apply twice to one pair | R1.4 | - |
| TY-05 | High | GADTs | Existential bounds need stored dictionaries; representation rule omits them | R4.9 | - |
| TY-06 | Med | Impl heads | `mut` inside trait arguments undefined (outer `mut` on targets settled by TQ-5) | R4.4 | TQ-30 (not applied) |
| TY-07 | Med | Supertraits | Supertrait obligations of generic impls unchecked | R5.1 | - |
| TY-08 | Med | Supertraits | Implied supertrait bounds used but never stated | R5.3 | - |
| TY-10 | Med | Assoc. functions | `Type::f`, `Trait::f()`, `T::f()` have no lookup rule | R9.8 | TQ-9 (not applied) |
| TY-13 | Med | Defaults | What a default body sees, child-trait defaults, `x.greet()` for `T < A + B` | R6.1-R6.5 | TY-13 (not applied) |
| TY-14 | Med | Dynamic safety | Row, pack, reified, suspending method params uncovered | R8.1 | TQ-10 (decided; 09 is silent on suspending methods) |
| TY-15 | Med | Dynamic safety | Requirement keys must be dynamically safe; rule missing | R8.5 | - |
| TY-16 | Med | Derivation | Ch. 04 says newtypes can derive; grammar forbids | R10.1 | TQ-11 (not applied) |
| TY-17 | Med | Derivation | Derive on GADT variants with existential parameters | R10.6 | - |
| TY-18 | Med | Derivation | Field obligations, recursion, placement, conflicts, codes | R10.3-R10.9 | - |
| TY-19 | Med | Derivation | Derived and hand-written law partners disagree silently | R10.10 | TQ-12 (not applied) |
| TY-20 | Med | Assignability | `never` and `mut` trait values missing from the single-step list | R11.1, R11.2 | TQ-14 |
| TY-21 | Med | LCT | Supertrait widening and `never` operands | R11.3-R11.5 | TQ-15 (not applied) |
| TY-22 | Med | Mutable paths | `mut`-bounded type params and `Self` receivers uncovered | R12.1 | - |
| TY-23 | Med | Packs | Bounds on packs and impls over packs have no semantics | R4.8, R3.5 | - |
| TY-24 | Med | Solver | Bound-solving termination unspecified | R1.6, R4.7 | TQ-20 |
| TY-25 | Med | Locality | Impls may sit in any module of the owning package | R2.3 | TQ-17 (not applied) |
| TY-26 | Med | Inherent impls | Generic inherent impls and member uniqueness | R9.9 | TQ-19 (not applied) |
| TY-27 | Med | Trait values | Where `downcast` lives (impl targets settled) | R8.7 | TQ-22 (not applied) |
| TY-28 | Low | Diagnostics | `mutable-receiver-required` contradicts Mutable Paths prose | R12.2 | - |
| TY-29 | Low | Diagnostics | 11 trait codes only in README table and fixtures | R14 | - |
| TY-30 | Low | Variance | Trait-parameter and dynamic-value variance undefined | R8.8 | TQ-16 (not applied) |
| TY-31 | Low | Provided traits | "Sealed" undefined; `Any` coverage loose | R13 | - |
| TY-32 | Low | Assoc. types | An ambiguous projection has no code | R4.10 | - |

## TY-04: "Constrained By A Reachable Bound" Is Ambiguous
High. Anchor: 09 Trait Implementations.

    trait Summary:
        fn summary(self) -> string
    impl[T < Display, I < mut Iterator[T]] Summary for I:
        fn summary(self) -> string: ...
    impl Iterator[i32] for Cursor: ...
    impl Iterator[string] for Cursor: ...

For `mut Cursor` the impl applies with `T = i32` and `T = string`, giving two
dictionaries for one `(Summary, Cursor)` pair without two declarations
overlapping. The chapter's adapter example does not need the clause (its `T`
occurs in `Iterable[T]`).
Fix: every impl parameter must occur in the trait arguments or target
(R1.4, `unconstrained-impl-parameter`). Rejects only already-ambiguous impls;
listed for review because it narrows accepted programs.

## TY-05: GADT Existential Bounds Need Stored Dictionaries
High. Anchor: 13 Type-Checking Requirements, Runtime Representation.

    enum Shown:
        Item[U < Display](value: U) -> Shown
    fn render(item: Shown) -> string:
        match item:
            Shown.Item(value) => value.to_string()

13 says an existential "may be used through its declared bounds"; at the match
`U` is unknown, so its `Display` operations must travel with the value. 13 also
says runtime values carry only "tag and payload". `U` may be `i32`, so boxing
through `Reference` does not cover it.
Fix: construction captures the evidence for bounded existential parameters as
part of the value's representation (R4.9). Not a choice: the only way the
existing arm rule can run.

## TY-06: `mut` Inside Trait Arguments
Status: outer `mut` on targets resolved by TQ-5 (`mutable-impl-target`).
Permission inside trait arguments is decided by TQ-30 (distinct
instantiations) but not yet applied.
Medium. Anchor: 09 Trait Implementations; 03 `Self`.

    impl Store[User] for Shelf: ...
    impl Store[mut User] for Shelf: ...      # one slot or two?

Undefined: whether permission in trait arguments distinguishes
instantiations, and what `Self` is when a blanket impl is selected through a
`mut` view.
Fix: apply TQ-30; R4.4.

## TY-07: Supertrait Obligations Of Generic Impls
Medium. Anchor: 09 Trait Declarations.

    trait Parent
    trait Child < Parent
    impl[T < Eq] Parent for Box[T]
    impl[T] Child for Box[T]    # Box[fn() -> void] is Child but not Parent

`missing-supertrait-implementation` is defined for "X has no implementation";
for generic impls, some instantiations do. Check at impl or at use is not said.
Fix: impl bounds must entail supertrait obligations, checked at the impl (R5.1).

## TY-08: Implied Supertrait Bounds Never Stated
Medium. Anchor: 09 Dynamic Trait Values.

    fn needs_parent[T < Parent](value: T) -> void: ...
    fn has_child[T < Child](value: T) -> void:
        needs_parent(value)     # T < Parent known?

09 says a child bound "exposes the methods" of supertraits (lookup only).
Fix: elaboration rule (R5.3).

## TY-10: Associated Function Lookup
Medium. Anchor: 09 Trait Declarations; Open Issues runtime type identity Q2.

    trait Factory:
        fn create() -> Self
    impl User:
        fn create() -> User: ...
    impl Factory for User:
        fn create() -> User: ...
    a := User::create()          # inherent or Factory? scope needed?
    b := Factory::create()       # which Self?
    fn make[T < Factory]() -> T:
        T::create()              # allowed?

Fixture `trait-associated-functions.hd` shows `Type::f` finding a trait
function; no rule states order, scope, `Self` for `Trait::f()`, or `T::f()`.
Fix: TQ-9 (R9.8).

## TY-13: Default Method Bodies
Status: name reuse resolved by TQ-6 (`duplicate-trait-member` at the child
trait), and E5 settles that an inherent or promoted method never replaces a
default. Decided, not yet applied: a default body sees only the trait's and
its supertraits' members, not `Self`'s fields or inherent methods. Still
open: child-trait defaults for supertrait methods beyond TQ-6, and
`x.greet()` for `T < A + B`.
Medium. Anchor: 09 Default-Method Conflicts.

    trait Greeter:
        fn name(self) -> string
        fn greet(self) -> string: "hi " + self.name()
    impl User:
        fn name(self) -> string: "inherent"   # does greet see this?

Fix: R6.1-R6.5.

## TY-14: Dynamic Safety Misses Row, Pack, Reified, Suspending
Medium. Anchor: 09 Dynamic Trait Values; 04 Trait Values And Any.

    trait Runner:
        fn run[R](self, job: fn() -> void $ R) -> void $ R
    trait Lookup:
        fn metadata[reified M](self) -> M?

"Every method-level generic parameter must be bounded by Reference": row
parameters cannot carry bounds; pack bounds are element-wise; `reified` not
mentioned; suspending methods not mentioned though `Console` needs them.
Fix: TQ-10 (R8.1).

## TY-15: Requirement Keys Must Be Dynamically Safe
Medium. Anchor: 11 Provider Access.

    trait Repo:
        type Item
        fn load(self) -> Self::Item
    fn f() -> void $ Repo:
        repo := $.use(Repo)      # a Repo trait value; Repo is not safe

Fix: requirement keys name dynamically safe instantiations; report
`trait-not-dynamically-safe` at the row (R8.5).

## TY-16: Newtype Derivation Contradiction
Medium. Anchor: 04 Transparent Aliases And Newtypes; 02 Annotations.

    @derive(PartialEq, Eq, Hash)
    type Mile(i32)               # 04: allowed; grammar: syntax error

Fix: TQ-11 (R10.1).

## TY-17: Derive On Existential GADT Variants
Medium. Anchor: 09 Comparison Traits; 13.

    @derive(PartialEq)
    enum Cell:
        Wrapped[U < PartialEq](value: U) -> Cell

Two values may hold different `U`; `eq(self, other: Self)` needs one type.
Fix: reject equality/ordering derives for such enums; allow `Hash` when every
existential field is `Hash`-bounded (R10.6).

## TY-18: Derived Impl Obligations, Recursion, Placement, Conflicts
Medium. Anchor: 09 Comparison Traits.

    impl[T < Hash] PartialEq for Wrapper[T]: ...
    @derive(PartialEq)
    data Holder[T]:
        inner: Wrapper[T]        # T < PartialEq does not give Wrapper[T] < PartialEq
    @derive(PartialEq)
    enum Tree[T]:
        Leaf(value: T)
        Node(left: Tree[T], right: Tree[T])

Unstated: the field check under added bounds, availability of the derived impl
while its fields are checked, owning module, code for derive plus manual impl,
code for an underivable trait, codes for failing PartialEq/PartialOrd fields.
Fix: R10.3-R10.9.

## TY-19: Law Partners Disagree Silently
Medium. Anchor: 09 Comparison Traits.

    impl PartialEq for Account:
        fn eq(self, other: Account) -> bool: self.id == other.id
    @derive(Hash)                # hashes cache too
    data Account:
        id: string
        cache: string

Fix: TQ-12 (R10.10).

## TY-20: Assignability Omits `never` And `mut` Trait Values
Status: compositions settled by TQ-14 (assignability stays single-step, so
`let wide: i64? = small_i8` and passing a `User` to a `Display?` parameter
need explicit conversions). Still open below.
Medium. Anchor: 04 Assignability And Coercion.

    let edit: mut Display = mutable_user  # rule 6 silent on mut

The rule list in 04 does not include `never`, which Type Forms makes
assignable to every type and which Least Common Type cites through the list.
Rule 6 does not say whether a `mut` trait value may be constructed.
Fix: R11.1, R11.2.

## TY-21: Least Common Type Gaps
Status: the `no-common-type` / `no-least-common-type` split is now defined
in 04. Supertrait widening is decided by TQ-15 (never used by inference) but
not yet applied; `never` operands are still open.
Medium. Anchor: 04 Least Common Type.

    items := [shown_value, tagged_value]   # both extend Named
    x := if ok: 1 else: return .None

Fix: R11.3-R11.5; widening is TQ-15.

## TY-22: Mutable Paths Omit Bounded Type Parameters And Self
Medium. Anchor: 04 Mutable Paths, Generics.

    fn clear_value[T < mut Clear](value: T) -> void:
        value.clear()            # value: T, not mut T

Fix: define "mutable access type" once (R12.1).

## TY-23: Bounds On Packs, Impls Over Packs
Medium. Anchor: 12; 02 Generic Parameters.

    fn show_all[Ts... < Display](values: Ts...) -> List[string]: ...
    impl[Ts... < Display] Display for (Ts...): ...

Fix: element-wise bounds, one dictionary per element; a pack tuple head unifies
with every arity (R4.8, R3.5).

## TY-24: Bound-Solving Termination
Status: K2 removed `where` clauses, so every bound is inline and its subject
is a generic parameter. Termination of bound solving is still unspecified.
Medium. Anchor: 02 Traits And Implementations; 09.

Fix: TQ-20 (a fixed depth limit as a backstop), R1.6, R4.7.

## TY-25: Impl Placement Is Package-Wide
Medium. Anchor: 09 Trait Implementations; Open Issues Annotation Locality.

    # pkg/model.hd
    pub data User:
        pub name: string
    # pkg/far/away.hd
    impl User:
        fn to_string(self) -> string: "shadow"   # now wins every user.to_string()

Fix: TQ-17 (R2.3).

## TY-26: Generic Inherent Impls
Medium. Anchor: 09 Inherent Implementations.

    impl[T < Display] Box[T]:
        fn show(self) -> string: ...
    impl Box[i32]:
        fn show(self) -> string: "int box"   # duplicate?

Fix: TQ-19 (R9.9).

## TY-27: Where `downcast` Lives
Status: impl targets settled (`trait-value-impl-target`: no impl targets a
trait value type). The other half of TQ-22, `downcast` as a compiler-provided
method limited to `Inspectable`, is decided but not yet applied.
Medium. Anchor: 09 Inherent Implementations; Open Issues runtime Q1.

Fix: apply TQ-22 (R8.7).

## TY-28: mutable-receiver-required Versus Prose
Low. Anchor: 04 Mutable Paths.

    fn invalid(parent: Parent) -> void:
        parent.child.rename("new")   # prose: readonly-root; fixture: mutable-receiver-required

Nine fixtures expect `mutable-receiver-required`; no chapter names it; the
promoted `mut self` code is unstated.
Fix: R12.2 (align prose with the normative table and fixtures).

## TY-29: Codes Without A Chapter Anchor
Low. Re-checked 2026-09-26: `trait-not-dynamically-safe`,
`sealed-trait-implementation`, `trait-method-visibility`,
`local-impl-nonlocal-pair`, `field-not-eq`, `field-not-hash`,
`missing-derived-bound`, `missing-partial-eq`, `missing-partial-ord`,
`duplicate-annotation-impl`, and `overlapping-annotation-impl` appear in no
chapter. (`ambiguous-method`, `orphan-impl`, `overlapping-impl`, and
`mutable-receiver-required` now do; `promoted-mutable-requirement` was
removed.)
Fix: R14.

## TY-30: Trait-Parameter Variance
Low. Anchor: 04 Variance; 02.

    trait Source[+T]:
        fn source(self) -> T
    fn widen(value: Source[mut User]) -> Source[User]: value

`trait_decl` uses `type_params`, which admits markers; 04's "trait method
signatures" sentence is ambiguous.
Fix: TQ-16 (R8.8).

## TY-31: Sealed And Any
Low. `Reference`, `Suspend`, `ShapeMetadata` are each called sealed in a
different chapter; no definition. `Any` coverage of `void`, `never`, function
types, `mut T`, and `impl Any for X` unstated.
Fix: R13.

## TY-32: Ambiguous Projections Have No Code
Low. K2 added associated type bindings in bounds
(`S < Supplier[Item = string]`), so a projection can now be constrained. 09
says "Ambiguous projections are compile-time errors" but names no code.

Fix: code `ambiguous-projection` (R4.10).
