# Testing Redesign: Open Points

> **Archived 2026-10-01.** Its decisions are applied, and the
> [specification](../../spec/README.md) is authoritative. The generator
> parameter style and the two deferred fixtures moved to
> [Open Issues](../OPEN_ISSUES.md#testing-open-points). The record is kept as history, so its
> examples and rule IDs may describe retired rules.

Status: open. Nothing here is accepted behavior. Owner decisions T1-T54
(2026-09-27 and 2026-09-28) are decided, and the specification is
authoritative for their language parts:
[Test Blocks](../../spec/02-grammar.md#test-blocks),
[Propagation In Test Blocks](../../spec/05-expressions.md#propagation-in-test-blocks),
[Test Modules](../../spec/10-modules.md#test-modules),
[Test Cases](../../spec/10-modules.md#test-cases),
[Test Outcomes](../../spec/10-modules.md#test-outcomes),
[Registration Functions](../../spec/std/testing.md#registration-functions),
[Exit Status](../../spec/10-modules.md#exit-status), and the `Debug` rules in
[Traits](../../spec/09-traits.md). The runner and library parts are in
[Runtime And Library](RUNTIME_AND_LIBRARY.md#testing) and
[Standard Library](STDLIB.md#testing-layer). The survey, the decision log,
and the testing stress test are in git history. The property-test API
decisions PT1-PT9 (batch 12, 2026-09-29) are applied in
[Property Tests](../../spec/std/testing.md#property-tests),
[Draw Budget](../../spec/std/testing.md#draw-budget), and
[Derived Arbitrary](../../spec/std/testing.md#derived-arbitrary). The
answers to that pass's open points, Q5-Q10 (batch 13, 2026-09-30), are
applied in the same sections.

## Owner Decisions

All applied, SR1 (batch 17, 2026-09-30), AT-with (batch 20,
2026-09-30), AT-any (batch 21, 2026-09-30), and AT-gen (batch 26,
2026-09-30) included;
[Spec Tiers](SPEC_TIERS.md#migration-plan) migration step 5 applied
AT-with. The specification is authoritative; the
decision texts and the batch 12 apply-pass readings, confirmed by Q9 and
Q10, are in git history.

| ID | Decision | Where |
| --- | --- | --- |
| PT1 | No size API; draws are biased toward small values and edges | [`std-testing.choices.no-size`](../../spec/std/testing.md#r-std-testing.choices.no-size); the `list` explanation is in [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults) and the [guide](../../guide/LANGUAGE_TOUR.md#tests) |
| PT2 | Derived-`Arbitrary` tuning is one fact, `arbitrary.with(gen)`, unchecked until a test runs | [Derived Arbitrary](../../spec/std/testing.md#derived-arbitrary) |
| PT3 | `int` and `float` are generic over `Integer` and `Float` | [`std-testing.choices.int-generic`](../../spec/std/testing.md#r-std-testing.choices.int-generic), [`.float-generic`](../../spec/std/testing.md#r-std-testing.choices.float-generic) |
| PT4 | `string(max_chars)` counts chars | [`std-testing.choices.string-chars`](../../spec/std/testing.md#r-std-testing.choices.string-chars) |
| PT5 | `map(max, key, value)`; a duplicate key keeps the last value | [`std-testing.choices.map`](../../spec/std/testing.md#r-std-testing.choices.map) |
| PT6 | Recursion ends by a per-case draw budget | [Draw Budget](../../spec/std/testing.md#draw-budget) |
| PT7 | `examples: List[T] = []` run first | [`std-testing.prop.examples`](../../spec/std/testing.md#r-std-testing.prop.examples) |
| PT8 | `assume` is generator-only | [`std-testing.prop.body-no-discard`](../../spec/std/testing.md#r-std-testing.prop.body-no-discard) |
| PT9 | Default float generators include NaN, infinities, -0.0, and subnormals | [`std-testing.arbitrary.float`](../../spec/std/testing.md#r-std-testing.arbitrary.float) |
| Examples | Worked examples go in the guide or STDLIB | [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults), the [guide](../../guide/LANGUAGE_TOUR.md#tests) |
| Stateful | Stateful testing waits for the event log | [Open Issues](../OPEN_ISSUES.md#runtime-library-abi-and-tooling-work) |
| Q5 | `arbitrary.with` is generic and returns a `Generator`; a failed downcast panics | `std-testing.arbitrary.with.wrap` (since retired), `.with.downcast-failure` |
| Q6 | An enum with no finite value panics on the first case | [`std-testing.arbitrary.derive.no-finite`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.no-finite) |
| Q7 | `List[E]`, `Map[_, E]`, and `E?` members do not make a variant recursive | [`std-testing.arbitrary.derive.recursive.containers`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.recursive.containers) |
| Q8 | Simplest values of `T?`, `Result`, and tuples | [`std-testing.budget.simplest.optional`](../../spec/std/testing.md#r-std-testing.budget.simplest.optional) and the rules after it |
| Q9, Q10 | The batch 12 readings are confirmed | unchanged |
| SR1 | `std.structure` gains a compiler-computed `pub enum SelfRef: Absent / Optional / Required` and a `self_ref: SelfRef` field on `VariantInfo` and `Member`. Derived `Arbitrary` becomes an ordinary `std.testing` template that picks the simplest variant by `self_ref != .Required`; a type whose every variant, or one data field, is `.Required` has no finite value. The prototype's checker-generated `Arbitrary` is a stopgap. | [`std-testing.arbitrary.derive.template`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.template), [`.no-finite.data`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.no-finite.data), [Self References](../../spec/14-annotations.md#self-references); the full text is in [TYPED_DERIVATION](TYPED_DERIVATION.md#owner-decision-sr1-2026-09-30) |
| AT-with | Option B (batch 20): "let's first go with B, thanks if it is not derivable, ask users to do manual impl". Annotations stay unchecked, with no compile-time check hook (M30). `arbitrary.with(gen)` stores the generator as `Any`. The derived template requires every member `F < Arbitrary & Inspectable`, tuned or not, and downcasts a tuned member's generator result. A mismatch panics with `explicit-panic`, naming the member. A type whose members fail either bound, such as one with a function-typed or non-inspectable member, is not derivable: the derive reports `unsatisfied-trait-bound`, and the user writes a manual `impl Arbitrary`. AT-with supersedes Q5's `arbitrary.with` details only where they differ. Member-typed facts (option D) are a future option in [Open Issues](../OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets), not decided. | [`std-testing.arbitrary.derive.member-bounds`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.member-bounds), [`.derive.not-derivable`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable), `.with.downcast-failure` |
| AT-any | Batch 21 confirms the reading of AT-with's "as `Any`": `with[T < Inspectable](../gen)` returns an opaque `Generator` holding `fn(mut Choices) -> Inspectable`. Each drawn value is erased to `Inspectable`, and the template downcasts it to the member's type; a mismatch is `explicit-panic`, naming the member. A raw `Any` could not be type-tested. | `std-testing.arbitrary.with.wrap` (since retired), and a Note in [Derived Arbitrary](../../spec/std/testing.md#derived-arbitrary) |
| ST8-newtype | Batch 21: a newtype gets no `Structure` today, so a newtype that derives `Arbitrary` through a base with no finite value panics with the base's name. The newtype-name rule stays for when newtypes gain a `Structure`. | Notes in [Derived Arbitrary](../../spec/std/testing.md#derived-arbitrary) and [The Structure Trait](../../spec/14-annotations.md#the-structure-trait) |
| ST8-clash | Batch 21: inside a template, a clash between the generated `facts` or `name` and the derived trait's own receiverless member is resolved by qualifying, `Structure::name` vs `MyTrait::name`, as M30 does for `Structure::walk`. | The Note on generated names in [Templates](../../spec/14-annotations.md#templates); `Self` of the qualified call is open in [Open Issues](../OPEN_ISSUES.md#applied-decisions) |
| AT-gen | Batch 26: derived `Arbitrary` generates `T < Arbitrary & Inspectable` for each type parameter a member uses, so `@derive(Arbitrary)` on `data Box[T]: value: T` works without a manual block. | [`std-testing.arbitrary.derive.params`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.params) |

### Apply-Pass Readings

Applying batch 13 read these points from the decisions. The owner
confirmed both (batch 15, 2026-09-30).

| Reading | Where |
| --- | --- |
| The no-finite-value panic is `explicit-panic`, since the accepted recommendation says it panics "as PT2's mismatch does". | [`std-testing.arbitrary.derive.no-finite`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.no-finite); [`derived-arbitrary-no-finite-value.hd`](../../spec/conformance/runtime/valid/derived-arbitrary-no-finite-value.hd) |
| A member that `arbitrary.with` tunes, whose type is not inspectable, is `unsatisfied-trait-bound`, the code for any unmet bound. AT-with moved the report from the `@arbitrary.with` line to the opt-in, where the derive reports every member that fails its bounds. | [`std-testing.arbitrary.derive.not-derivable`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable); [`arbitrary-with-non-inspectable-member.hd`](../../spec/conformance/typing/invalid/arbitrary-with-non-inspectable-member.hd) |

## Still Open

| Question | Applied | **Recommendation** |
| --- | --- | --- |
| Parameter style for generators | This batch keeps `mut Choices` parameters. The owner is comparing a requirement-row style (`fn() -> T $ Choices`), which may replace it. | Re-evaluate the two styles against a working compiler. |

## Decided, Waiting For Coverage

T54 (2026-09-28): test-layout fixture packages for
`cyclic-test-dependency` and for a `tests` root inside `tests/` are added
when those rules need coverage. The `# fixture-test-layout:` header exists
([Test Layouts](../../spec/conformance/README.md#test-layouts)), but neither
code has a fixture yet.
