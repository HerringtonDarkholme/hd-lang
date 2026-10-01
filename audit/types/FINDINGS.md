# Type-Checking Rules: Findings

Former roadmap area 2. Scope: Type System (04), Traits (09), Variadic Generics (12),
GADTs (13), and the trait-related parts of 03, 05, 06, 07, 08, 10, 11, 14.
Audited at commit 158a430; re-checked against the specification on 2026-09-26.
Resolved findings are removed (TY-01, TY-02, TY-03, TY-06, TY-09, TY-10,
TY-11, TY-12, TY-13, TY-14, TY-16, TY-19, TY-24, TY-25, TY-26, TY-27, TY-30,
TY-32, TY-33); the commit messages record the decisions that settled them.
Decisions taken but not yet applied are in [QUESTIONS.md](QUESTIONS.md).

Severity: **High**: two normative statements contradict, or a permitted reading
makes ordinary programs unsound or impossible. **Medium**: unspecified; careful
implementations would disagree. **Low**: recoverable from context, but a code,
anchor, or statement is missing.

| ID | Sev | Topic | Summary | Rule | Question |
| --- | --- | --- | --- | --- | --- |
| TY-04 | High | Impl selection | "Constrained by a reachable bound" lets one impl apply twice to one pair | R1.4 | - |
| TY-05 | High | GADTs | Existential bounds need stored dictionaries; representation rule omits them | R4.9 | - |
| TY-07 | Med | Supertraits | Supertrait obligations of generic impls unchecked | R5.1 | - |
| TY-08 | Med | Supertraits | Implied supertrait bounds used but never stated | R5.3 | - |
| TY-15 | Med | Dynamic safety | Requirement keys must be dynamically safe; rule missing | R8.5 | - |
| TY-17 | Med | Derivation | Derive on GADT variants with existential parameters | R10.6 | - |
| TY-18 | Med | Derivation | Placement and derive-plus-impl conflicts | R10.3, R10.7-R10.9 | - |
| TY-20 | Med | Assignability | `never` and `mut` trait values missing from the single-step list | R11.1, R11.2 | TQ-14 |
| TY-21 | Med | LCT | `never` operands (supertrait widening settled by TQ-15) | R11.4 | - |
| TY-22 | Med | Mutable paths | `mut`-bounded type params and `Self` receivers uncovered | R12.1 | - |
| TY-23 | Med | Packs | Bounds on packs and impls over packs have no semantics | R4.8, R3.5 | - |
| TY-28 | Low | Diagnostics | `mutable-receiver-required` contradicts Mutable Paths prose | R12.2 | - |
| TY-29 | Low | Diagnostics | 9 trait codes only in README table and fixtures | R14 | - |
| TY-31 | Low | Provided traits | Whether `void` and `never` satisfy `Any` | R13.2 | - |

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
through `AnyRef` does not cover it.
Fix: construction captures the evidence for bounded existential parameters as
part of the value's representation (R4.9). Not a choice: the only way the
existing arm rule can run.

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

## TY-15: Requirement Keys Must Be Dynamically Safe
Medium. Anchor: 11 Provider Access.

    trait Repo:
        type Item
        fn load(self) -> Self::Item
    fn f() -> void $ Repo:
        repo := $.use(Repo)      # a Repo trait value; Repo is not safe

Fix: requirement keys name dynamically safe instantiations; report
`trait-not-dynamically-safe` at the row (R8.5).

## TY-17: Derive On Existential GADT Variants
Medium. Anchor: 09 Comparison Traits; 13.

    @derive(Eq)
    enum Cell:
        Wrapped[U < Eq](value: U) -> Cell

Two values may hold different `U`; `eq(self, other: Self)` needs one type.
Fix: reject equality/ordering derives for such enums; allow `Hash` when every
existential field is `Hash`-bounded (R10.6).

## TY-18: Derived Impl Obligations, Recursion, Placement, Conflicts
Status: field obligations, the underivable-trait code, and the failing-field
codes are settled (`trait.derive.templated`,
`trait.derive.field-missing-trait`, `trait.derive.bound-unmet`). The
impl's owning module and a derive beside a written impl remain.
Medium. Anchor: 09 Comparison Traits.

    impl[T < Hash] Eq for Wrapper[T]: ...
    @derive(Eq)
    data Holder[T]:
        inner: Wrapper[T]        # T < Eq does not give Wrapper[T] < Eq
    @derive(Eq)
    enum Tree[T]:
        Leaf(value: T)
        Node(left: Tree[T], right: Tree[T])

Unstated: the field check under added bounds, availability of the derived impl
while its fields are checked, owning module, code for derive plus manual impl,
code for an underivable trait, codes for failing Eq/PartialOrd fields.
Fix: R10.3-R10.9.

## TY-20: Assignability Omits `never` And `mut` Trait Values
Status: compositions settled by TQ-14 (assignability stays single-step, so
`let wide: i64? = small_i8` and passing a `User` to a `Display?` parameter
need explicit conversions). Chapter 04 now states that `never` is assignable
to every type (`types.never.assignable`), though not in the rule list. The
`mut` trait value is still open.
Medium. Anchor: 04 Assignability And Coercion.

    let edit: mut Display = mutable_user  # rule 6 silent on mut

The rule list in 04 does not include `never`, which Type Forms makes
assignable to every type and which Least Common Type cites through the list.
Rule 6 does not say whether a `mut` trait value may be constructed.
Fix: R11.1, R11.2.

## TY-21: Least Common Type Gaps
Status: the `no-common-type` / `no-least-common-type` split is now defined
in 04, and TQ-15 settled supertrait widening
(`types.lct.no-supertrait-widening`). `never` operands are still open.
Medium. Anchor: 04 Least Common Type.

    x := if ok: 1 else: return .None

Fix: R11.4.

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

## TY-28: mutable-receiver-required Versus Prose
Low. Anchor: 04 Mutable Paths.

    fn invalid(parent: Parent) -> void:
        parent.child.rename("new")   # prose: readonly-root; fixture: mutable-receiver-required

Nine fixtures expect `mutable-receiver-required`; no chapter names it; the
promoted `mut self` code is unstated.
Fix: R12.2 (align prose with the normative table and fixtures).

## TY-29: Codes Without A Chapter Anchor
Low. Re-checked 2026-09-27: `trait-method-visibility`,
`local-impl-nonlocal-pair`, `missing-partial-eq`, `missing-partial-ord`,
`duplicate-annotation-impl`, and `overlapping-annotation-impl` appear in no
chapter. (`ambiguous-method`, `orphan-impl`, `overlapping-impl`,
`mutable-receiver-required`, `sealed-trait-implementation`,
`trait-not-dynamically-safe`, and `missing-derived-bound` now do;
`field-not-eq` and `field-not-hash` were replaced by
`derive-field-missing-trait` on 2026-09-28;
`promoted-mutable-requirement` was removed.)
Fix: R14.

## TY-31: `Any` Coverage Of `void` And `never`
Low. Status: "sealed" is now defined (09 Sealed Traits), and `impl Any for X`
is `sealed-trait-implementation`. Chapter 04 now says every value type,
including optionals, function types, and `mut` views, satisfies `Any`. Still
unstated: whether `void` and `never` do.
Fix: R13.2.
