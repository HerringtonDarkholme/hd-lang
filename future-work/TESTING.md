# Testing Redesign: Open Points

Status: open. Nothing here is accepted behavior. Owner decisions T1-T54
(2026-09-27 and 2026-09-28) are decided, and the specification is
authoritative for their language parts:
[Test Blocks](../spec/02-grammar.md#test-blocks),
[Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks),
[Test Modules](../spec/10-modules.md#test-modules),
[Test Cases](../spec/10-modules.md#test-cases),
[Test Outcomes](../spec/10-modules.md#test-outcomes),
[Table Tests](../spec/10-modules.md#table-tests),
[Exit Status](../spec/10-modules.md#exit-status), and the `Debug` rules in
[Traits](../spec/09-traits.md). The runner and library parts are in
[Runtime And Library](RUNTIME_AND_LIBRARY.md#testing) and
[Standard Library](STDLIB.md#testing-layer). The survey, the decision log,
and the testing stress test are in git history. The property-test API
decisions PT1-PT9 (batch 12, 2026-09-29) are applied in
[Property Tests](../spec/10-modules.md#property-tests),
[Draw Budget](../spec/10-modules.md#draw-budget), and
[Derived Arbitrary](../spec/10-modules.md#derived-arbitrary). The
answers to that pass's open points, Q5-Q10 (batch 13, 2026-09-30), are
applied in the same sections.

## Owner Decisions

All applied, SR1 (batch 17, 2026-09-30) included. The specification is
authoritative; the decision
texts and the batch 12 apply-pass readings, confirmed by Q9 and Q10, are in
git history.

| ID | Decision | Where |
| --- | --- | --- |
| PT1 | No size API; draws are biased toward small values and edges | [`module.testing.choices.no-size`](../spec/10-modules.md#r-module.testing.choices.no-size); the `list` explanation is in [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults) and the [guide](../guide/LANGUAGE_TOUR.md#tests) |
| PT2 | Derived-`Arbitrary` tuning is one fact, `arbitrary.with(gen)`, unchecked until a test runs | [Derived Arbitrary](../spec/10-modules.md#derived-arbitrary) |
| PT3 | `int` and `float` are generic over `Integer` and `Float` | [`module.testing.choices.int-generic`](../spec/10-modules.md#r-module.testing.choices.int-generic), [`.float-generic`](../spec/10-modules.md#r-module.testing.choices.float-generic) |
| PT4 | `string(max_chars)` counts chars | [`module.testing.choices.string-chars`](../spec/10-modules.md#r-module.testing.choices.string-chars) |
| PT5 | `map(max, key, value)`; a duplicate key keeps the last value | [`module.testing.choices.map`](../spec/10-modules.md#r-module.testing.choices.map) |
| PT6 | Recursion ends by a per-case draw budget | [Draw Budget](../spec/10-modules.md#draw-budget) |
| PT7 | `examples: List[T] = []` run first | [`module.testing.prop.examples`](../spec/10-modules.md#r-module.testing.prop.examples) |
| PT8 | `assume` is generator-only | [`module.testing.prop.body-no-discard`](../spec/10-modules.md#r-module.testing.prop.body-no-discard) |
| PT9 | Default float generators include NaN, infinities, -0.0, and subnormals | [`module.testing.arbitrary.float`](../spec/10-modules.md#r-module.testing.arbitrary.float) |
| Examples | Worked examples go in the guide or STDLIB | [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults), the [guide](../guide/LANGUAGE_TOUR.md#tests) |
| Stateful | Stateful testing waits for the event log | [Open Issues](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work) |
| Q5 | `arbitrary.with` is generic and returns a `Generator`; a failed downcast panics | [`module.testing.arbitrary.with.wrap`](../spec/10-modules.md#r-module.testing.arbitrary.with.wrap), [`.with.downcast-failure`](../spec/10-modules.md#r-module.testing.arbitrary.with.downcast-failure) |
| Q6 | An enum with no finite value panics on the first case | [`module.testing.arbitrary.derive.no-finite`](../spec/10-modules.md#r-module.testing.arbitrary.derive.no-finite) |
| Q7 | `List[E]`, `Map[_, E]`, and `E?` members do not make a variant recursive | [`module.testing.arbitrary.derive.recursive.containers`](../spec/10-modules.md#r-module.testing.arbitrary.derive.recursive.containers) |
| Q8 | Simplest values of `T?`, `Result`, and tuples | [`module.testing.budget.simplest.optional`](../spec/10-modules.md#r-module.testing.budget.simplest.optional) and the rules after it |
| Q9, Q10 | The batch 12 readings are confirmed | unchanged |
| SR1 | `std.structure` gains a compiler-computed `pub enum SelfRef: Absent / Optional / Required` and a `self_ref: SelfRef` field on `VariantInfo` and `Member`. Derived `Arbitrary` becomes an ordinary `std.testing` template that picks the simplest variant by `self_ref != .Required`; a type whose every variant, or one data field, is `.Required` has no finite value. The prototype's checker-generated `Arbitrary` is a stopgap. | [`module.testing.arbitrary.derive.template`](../spec/10-modules.md#r-module.testing.arbitrary.derive.template), [`.no-finite.data`](../spec/10-modules.md#r-module.testing.arbitrary.derive.no-finite.data), [Self References](../spec/14-annotations.md#self-references); the full text is in [TYPED_DERIVATION](TYPED_DERIVATION.md#owner-decision-sr1-2026-09-30) |

### Apply-Pass Readings

Applying batch 13 read these points from the decisions. The owner
confirmed both (batch 15, 2026-09-30).

| Reading | Where |
| --- | --- |
| The no-finite-value panic is `explicit-panic`, since the accepted recommendation says it panics "as PT2's mismatch does". | [`module.testing.arbitrary.derive.no-finite`](../spec/10-modules.md#r-module.testing.arbitrary.derive.no-finite); [`derived-arbitrary-no-finite-value.hd`](../spec/conformance/runtime/valid/derived-arbitrary-no-finite-value.hd) |
| A member that `arbitrary.with` tunes, whose type is not inspectable, is `unsatisfied-trait-bound`, the code for any unmet bound, reported on the `@arbitrary.with` line. | [`module.testing.arbitrary.with.inspectable`](../spec/10-modules.md#r-module.testing.arbitrary.with.inspectable); [`arbitrary-with-non-inspectable-member.hd`](../spec/conformance/typing/invalid/arbitrary-with-non-inspectable-member.hd) |

## Still Open

| Question | Applied | **Recommendation** |
| --- | --- | --- |
| Parameter style for generators | This batch keeps `mut Choices` parameters. The owner is comparing a requirement-row style (`fn() -> T $ Choices`), which may replace it. | Re-evaluate the two styles against a working compiler. |

## Decided, Waiting For Coverage

T54 (2026-09-28): test-layout fixture packages for
`cyclic-test-dependency` and for a `tests` root inside `tests/` are added
when those rules need coverage. The `# fixture-test-layout:` header exists
([Test Layouts](../spec/conformance/README.md#test-layouts)), but neither
code has a fixture yet.
