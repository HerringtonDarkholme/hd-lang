# Type-Checking Rules: Findings

Roadmap area 2. Scope: Type System (04), Traits (09), Variadic Generics (12),
GADTs (13), and the trait-related parts of 03, 05, 06, 07, 08, 10, 11, 14.
Audited at commit 158a430.

Severity: **High**: two normative statements contradict, or a permitted reading
makes ordinary programs unsound or impossible. **Medium**: unspecified; careful
implementations would disagree. **Low**: recoverable from context, but a code,
anchor, or statement is missing.

| ID | Sev | Topic | Summary | Rule | Question |
| --- | --- | --- | --- | --- | --- |
| TY-01 | High | Overlap | Resolved (TQ-1): bounds both do and do not count when proving impls disjoint | R3.2 | TQ-1 |
| TY-02 | High | Orphan | Resolved (TQ-2): `annotate Facet for string` is legal; its lowered impl is an orphan | R2.1 | TQ-2 |
| TY-03 | High | Method resolution | Resolved (TQ-3, E1): numbered tiers contradicted the "remaining candidates" sentence; own members now precede promoted ones | R9.3 | TQ-3, E1 |
| TY-04 | High | Impl selection | "Constrained by a reachable bound" lets one impl apply twice to one pair | R1.4 | - |
| TY-05 | High | GADTs | Existential bounds need stored dictionaries; representation rule omits them | R4.9 | - |
| TY-06 | Med | Impl heads | Resolved for targets (TQ-5): `mut` in impl heads and trait arguments undefined; trait arguments still open | R1.3, R4.4 | TQ-5 |
| TY-07 | Med | Supertraits | Supertrait obligations of generic impls unchecked | R5.1 | - |
| TY-08 | Med | Supertraits | Implied supertrait bounds used but never stated | R5.3 | - |
| TY-09 | Med | Method resolution | Dot call with two instantiations of one generic trait | R9.5 | TQ-4 |
| TY-10 | Med | Assoc. functions | `Type::f`, `Trait::f()`, `T::f()` have no lookup rule | R9.8 | TQ-9 |
| TY-11 | Med | Embedding | Resolved (E5): which trait slots a promoted method fills | R7.2 | E5 |
| TY-12 | Med | Embedding | Resolved (E4): whether embedded trait methods promote | R7.4 | E4 |
| TY-13 | Med | Defaults | Partly resolved (TQ-6, E5): default-body dispatch, supertrait defaults, name reuse | R6.1-R6.5 | TQ-6, E5 |
| TY-14 | Med | Dynamic safety | Row, pack, reified, suspending method params uncovered | R8.1 | TQ-10 |
| TY-15 | Med | Dynamic safety | Requirement keys must be dynamically safe; rule missing | R8.5 | - |
| TY-16 | Med | Derivation | Ch. 04 says newtypes can derive; grammar forbids | R10.1 | TQ-11 |
| TY-17 | Med | Derivation | Derive on GADT variants with existential parameters | R10.6 | - |
| TY-18 | Med | Derivation | Field obligations, recursion, placement, conflicts, codes | R10.3-R10.9 | - |
| TY-19 | Med | Derivation | Derived and hand-written law partners disagree silently | R10.10 | TQ-12 |
| TY-20 | Med | Assignability | Coercion list is single-step; compositions unspecified | R11.1 | TQ-14 |
| TY-21 | Med | LCT | Supertrait widening, `never`, error-code split | R11.3-R11.6 | TQ-15 |
| TY-22 | Med | Mutable paths | `mut`-bounded type params and `Self` receivers uncovered | R12.1 | - |
| TY-23 | Med | Packs | Bounds on packs and impls over packs have no semantics | R4.8, R3.5 | - |
| TY-24 | Med | Solver | Where-predicate subjects and termination unspecified | R1.6, R4.7 | TQ-20 |
| TY-25 | Med | Locality | Impls may sit in any module of the owning package | R2.3 | TQ-17 |
| TY-26 | Med | Inherent impls | Generic inherent impls and member uniqueness | R9.9 | TQ-19 |
| TY-27 | Med | Trait values | Trait impls targeting a dynamic trait value type | R8.7 | TQ-22 |
| TY-28 | Low | Diagnostics | `mutable-receiver-required` contradicts Mutable Paths prose | R12.2 | - |
| TY-29 | Low | Diagnostics | 16 trait codes only in README table and fixtures | R14 | - |
| TY-30 | Low | Variance | Trait-parameter and dynamic-value variance undefined | R8.8 | TQ-16 |
| TY-31 | Low | Provided traits | "Sealed" undefined; `Any` coverage loose | R13 | - |
| TY-32 | Low | Assoc. types | No projection equality; ambiguous projection has no code | R4.10 | TQ-21 |
| TY-33 | Low | Grammar | Ch. 12 wrote bounds with `:` | fixed (ce2a0d9) | - |

## TY-01: Overlap Both Uses And Ignores Bounds
Status: resolved by TQ-1, applied to 09 Implementation Targets and Overlap,
and 06 For Loops (2026-09-26).
High. Anchor: 09 Trait Implementations; 06 for-loops.

    data Bag:
        items: List[i32]
    impl Iterable[i32] for Bag:
        fn iter(self) -> mut Iterator[i32]: self.items.iter()

The prelude has `impl[T, I < mut Iterator[T]] Iterable[T] for I`; heads unify
with `I = Bag`. 09 says impls overlap "when their trait and target heads can
unify under any satisfying substitutions", then "`where` predicates are not
used to claim that otherwise unifying implementations are disjoint". Inline
bounds and `where` predicates are interchangeable in the grammar. If bounds are
ignored, no user type can implement `Iterable`. If they count, a later
`impl Iterator[i32] for Bag` turns a valid impl into an overlap, and across
packages a dependency adding an impl breaks a downstream that compiled. The
spec's own "overlap between the standard Iterator adapter and a direct Iterable
implementation" only makes sense if bounds count.
Fix: TQ-1; recommended: bounds never prove disjointness, and the adapter
becomes a compiler-provided rule outside the impl table (R3.2, R13.5).

## TY-02: Annotation Slots Violate The Ordinary Orphan Rule
Status: resolved by TQ-2, applied to 09 Implementation Ownership and 14
Coherence And Package Rules (2026-09-26).
High. Anchor: 09 Trait Implementations; 14 Derived Facet Information, Coherence.

    # package validation, which declares the facet
    data Validation: pass
    annotate Validation for string:
        fn build(self, target: TypeShape) -> Validator: Validator.String

Lowers to `impl Annotate[Validation] for string`; `std` owns both `Annotate`
and `string`. 09 allows an impl only in the package owning the trait or target,
so the lowered impl is an orphan, while 14 relies on facet packages annotating
primitives and says an explicit impl "occupies the same coherence slot". The
same gap blocks `impl Add[Money] for i32` in Money's package.
Fix: TQ-2 (R2.1).

## TY-03: Method Resolution Order Contradicts Itself
Status: resolved by TQ-3 (inherent methods win; several trait candidates are
`ambiguous-method`) and E1 (own members, trait methods included, precede
promoted members), applied to 03 Member Resolution and 09 Method Resolution
(2026-09-26). In the example, `page.to_string()` selects Page's `Display`
method.
High. Anchor: 09 Method Resolution.

    data Base:
        name: string
    impl Base:
        fn to_string(self) -> string: "base"
    data Page:
        Base
        title: string
    impl Display for Page:
        fn to_string(self) -> string: self.title
    fn show(page: Page) -> string:
        page.to_string()   # "base", title, or ambiguous?

The numbered list picks promoted `Base.to_string` over Page's own impl; the
following paragraph ("multiple remaining candidates ... ambiguity") pools
promoted and trait candidates. Under tiers, adding an inherent method to `Base`
in another module silently changes this call.
Fix: TQ-3; recommended: inherent, then promoted and trait candidates pooled,
ambiguity error (R9.3).

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

## TY-06: `mut` In Impl Heads And Trait Arguments
Status: outer `mut` on targets resolved by TQ-5 (`mutable-impl-target`).
Permission inside trait arguments is still open.
Medium. Anchor: 09 Trait Implementations; 03 `Self`.

    impl Marker for mut Counter              # legal alone?
    impl Store[User] for Shelf: ...
    impl Store[mut User] for Shelf: ...      # one slot or two?

Undefined: whether `mut` in a head is legal without a conflict; what `Self` is
in `impl Marker for mut Counter` (`mut self` would be `mut mut Counter`);
whether permission in trait arguments distinguishes instantiations; what `Self`
is when a blanket impl is selected through a `mut` view (the iterator adapter
relies on `I = mut X`).
Fix: TQ-5; R1.3, R4.4.

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

## TY-09: Two Instantiations Of One Generic Trait At A Dot Call
Medium. Anchor: 09 Method Resolution.

    impl Add[i32] for Money: ...
    impl Add[Money] for Money: ...
    total := price.add(5)

Legal pair; selection by argument type neither allowed nor forbidden.
Fix: TQ-4; recommended `ambiguous-method`, qualify with `Add[i32]::add` (R9.5).

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

## TY-11: Which Slots A Promoted Method Fills
Status: resolved by E5, applied to 09 Embedding And Trait Satisfaction
(2026-09-26): a promoted method fills no slot, required or defaulted; the
implementation writes the method.
Medium. Anchor: 09 Embedding And Trait Satisfaction.

    trait Named:
        fn name(self) -> string
        fn label(self) -> string: "name: " + self.name()
    impl Base:
        fn name(self) -> string: ...
        fn label(self) -> string: "base label"
    data User:
        Base
    impl Named for User          # label: default or Base.label?

Unstated: replacing defaults, filling in partial-body impls, signature match.
Fix: TQ-7 (R7.2).

## TY-12: Do An Embedded Type's Trait Methods Promote?
Status: resolved by E4, applied to 03 Member Resolution and 08 Data Embedding
(2026-09-26): they never promote, so `page.to_string()` is `unknown-method`.
Medium. Anchor: 03 Member Resolution; 08 Data Embedding.

    impl Display for Label:
        fn to_string(self) -> string: self.text
    data Page:
        Label
    page.to_string()             # promoted from Label's Display impl?
    println(page)                # rejected: Page is not Display

Fix: TQ-8; recommended only fields and inherent methods promote (R7.4).

## TY-13: Default Method Bodies
Status: name reuse resolved by TQ-6 (`duplicate-trait-member` at the child
trait). E5 settles that an inherent or promoted method never replaces a
default in an implementation. Whether a default body sees `Self`'s inherent
methods and fields (`greet` calling `self.name()` above), and the other parts,
are still open; under M1 this also decides whether `self.name()` in a default
is ambiguous when `Self` has a field `name`.
Medium. Anchor: 09 Default-Method Conflicts.

    trait Greeter:
        fn name(self) -> string
        fn greet(self) -> string: "hi " + self.name()
    impl User:
        fn name(self) -> string: "inherent"   # does greet see this?
    trait Loud < Greeter:
        fn greet(self) -> string: "HI"        # new member or supertrait default?

Unstated: defaults checked generically with `Self < Trait` (no late binding to
inherent methods, the Swift pitfall); child-trait defaults for supertrait
methods; child members reusing supertrait names (`duplicate-trait-member` is
per trait); `x.greet()` for `T < A + B`.
Fix: R6.1-R6.5; name reuse is TQ-6.

## TY-14: Dynamic Safety Misses Row, Pack, Reified, Suspending
Medium. Anchor: 09 Dynamic Trait Values; 04 Trait Values And Any.

    trait Runner:
        fn run[r](self, job: fn() -> void $ r) -> void $ r
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

## TY-20: Assignability Is Single-Step
Medium. Anchor: 04 Assignability And Coercion.

    let wide: i64? = small_i8          # widening then injection
    note(user)                         # note(value: Display?): construction then injection
    let shown: Display = mutable_user  # weakening then construction
    let edit: mut Display = mutable_user  # rule 6 silent on mut

"At least one of these rules applies" is one step; none of these lines match
one rule. `never` is missing from the list LCT cites.
Fix: TQ-14 (R11.1, R11.2).

## TY-21: Least Common Type Gaps
Medium. Anchor: 04 Least Common Type.

    items := [shown_value, tagged_value]   # both extend Named
    pick := if ok: display_value else: user
    x := if ok: 1 else: return nil

Rule 7 widening participation; `no-common-type` vs `no-least-common-type` when
only an excluded conversion works; `never` operands; weakening plus variance.
Fix: R11.3-R11.6; widening is TQ-15.

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

## TY-24: Where-Predicate Subjects And Termination
Medium. Anchor: 02 Traits And Implementations.

    impl[T] Show for Box[T] where List[T] < Hash: ...
    impl[T] Loop for T where Box[T] < Loop: ...     # never terminates

Functions have no `where` clause, so `T::Item` cannot be bounded on a function.
Fix: TQ-20 (R1.6, R4.7).

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

## TY-27: Trait Impls For Trait Value Types
Medium. Anchor: 09 Inherent Implementations; Open Issues runtime Q1.

    impl Describe for Display:
        fn describe(self) -> string: self.to_string()

Fix: TQ-22 (R8.7).

## TY-28: mutable-receiver-required Versus Prose
Low. Anchor: 04 Mutable Paths.

    fn invalid(parent: Parent) -> void:
        parent.child.rename("new")   # prose: readonly-root; fixture: mutable-receiver-required

Nine fixtures expect `mutable-receiver-required`; no chapter names it; the
promoted `mut self` code is unstated.
Fix: R12.2 (align prose with the normative table and fixtures).

## TY-29: Codes Without A Chapter Anchor
Low. `ambiguous-method`, `orphan-impl`, `overlapping-impl`,
`trait-not-dynamically-safe`, `sealed-trait-implementation`,
`promoted-mutable-requirement`, `trait-method-visibility`,
`local-impl-nonlocal-pair`, `field-not-eq`, `field-not-hash`,
`missing-derived-bound`, `missing-partial-eq`, `missing-partial-ord`,
`duplicate-annotation-impl`, `overlapping-annotation-impl`,
`mutable-receiver-required`; "ambiguous projections" has no code.
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

## TY-32: Projections Cannot Be Constrained
Low.

    fn names[S < Supplier](source: S) -> List[string]:
        # no way to say S::Item = string

Fix: code `ambiguous-projection` (R4.10); feature decision TQ-21.

## TY-33: Chapter 12 Bound Token
Low. Fixed in ce2a0d9.
