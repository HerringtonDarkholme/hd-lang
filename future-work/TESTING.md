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

Decided and applied 2026-09-29 (batch 12, the property-test API). The
batch keeps the current `mut Choices` parameter style.

| ID | Decision |
| --- | --- |
| PT1 | No size API, as in Hypothesis: draws are biased toward small values and edges. Any small-first scheduling is runner-internal, not in the spec. STDLIB and the guide explain why `list`'s item generator takes its own `c`: it is the same `Choices` passed back, so `list` can bracket each element's draws as a span for shrinking, and the closure does not capture the outer `mut c` while `list` holds it. A named generator, as in `c.list(50, digit)`, needs no closure. |
| PT2 | Derived-`Arbitrary` tuning uses one fact, `arbitrary.with(gen)`; there are no range or length facts. Facts are unchecked values, so a mistake can surface only at test time. The fact stores the generator as `Any`; the derived code casts it to the member's `fn(mut Choices) -> T`. A failed cast panics on the first case, with a message naming the member and the expected generator type; it is never silently ignored. A generic `With[T]` fact was rejected: `find[With[i32]]` would miss a `With[string]` and fall back silently. |
| PT3 | `fn int[N < Integer](mut self, lo: N, hi: N) -> N` and `fn float[F < Float](mut self, lo: F, hi: F) -> F`. |
| PT4 | `fn string(mut self, max_chars: i32) -> string`, called as `c.string(max_chars=12)`. The limit counts chars, not bytes. Which chars it draws is a runner detail. |
| PT5 | `fn map[K < Eq & Hash, V](mut self, max: i32, key: fn(mut Choices) -> K, value: fn(mut Choices) -> V) -> Map[K, V]` draws up to `max` entries. A duplicate key keeps the last value, so the map may have fewer entries. |
| PT6 | Recursive generation ends by a per-case draw budget, with no API. Once it is spent, every draw returns its simplest value: an integer 0 or the bound nearest 0, floats likewise, `bool` false, `pick` its first item, and `list`, `map`, and `string` empty. A derived enum's simplest choice is its first non-recursive variant, whatever the declaration order. Hand-written generators terminate the same way. The budget size is a runner detail. |
| PT7 | `it_prop` and `it_prop_with` take `examples: List[T] = []`. The listed inputs run first on every run, before saved regressions and random cases. |
| PT8 | `assume` stays generator-only (`c.assume`); a property body cannot discard a case. |
| PT9 | The default `f64` and `f32` generators (`c.draw[f64]()` and derived `Arbitrary`) include NaN, the infinities, -0.0, and subnormals, as Hypothesis's `st.floats()` does. `c.float(lo, hi)` stays finite, because its bounds rule them out. |
| Examples | Worked examples go in the guide or STDLIB, whichever the spec-scope rule allows: a JSON round trip (after Zac Hatfield-Dodds, ["Sufficiently Advanced Testing"](https://zhd.dev/sufficiently/)), a two-input test with explicit `examples` (after CPython's `Lib/test/test_zoneinfo/test_zoneinfo_property.py`), and dependent draws (an ordered pair, as Hypothesis's `@composite`). Match arms use `=>`, recursion uses a leaf-first `match c.int(0, 5)`, and several inputs are one tuple. |
| Stateful | Stateful testing stays deferred until the event log. |

For PT2, the record notes that the coordinator first claimed wrongly that a
wrong generator would be a compile-time `type-mismatch`. A fact's value is
not checked against its member's type
([Member Metadata](../spec/14-annotations.md#member-metadata)), so the
mistake can surface only when a test runs.

| Decision | Rules |
| --- | --- |
| PT1 | [`module.testing.choices.no-size`](../spec/10-modules.md#r-module.testing.choices.no-size); the Note that ends [Derived Arbitrary](../spec/10-modules.md#derived-arbitrary) keeps scheduling and bias runner behavior. The `list` explanation is in [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults) and the [guide](../guide/LANGUAGE_TOUR.md#tests). |
| PT2 | [`module.testing.arbitrary.derive`](../spec/10-modules.md#r-module.testing.arbitrary.derive), [`.with`](../spec/10-modules.md#r-module.testing.arbitrary.with), [`.with.only`](../spec/10-modules.md#r-module.testing.arbitrary.with.only), [`.with.unchecked`](../spec/10-modules.md#r-module.testing.arbitrary.with.unchecked), `.with.mismatch` (retired by Q5 for [`.with.downcast-failure`](../spec/10-modules.md#r-module.testing.arbitrary.with.downcast-failure)), [`.with.no-fallback`](../spec/10-modules.md#r-module.testing.arbitrary.with.no-fallback); [Member Metadata](../spec/14-annotations.md#member-metadata) names the fact |
| PT3 | [`module.testing.choices.int-generic`](../spec/10-modules.md#r-module.testing.choices.int-generic), [`.float-generic`](../spec/10-modules.md#r-module.testing.choices.float-generic); they retire `module.testing.choices.int` and `.float` |
| PT4 | [`module.testing.choices.string-chars`](../spec/10-modules.md#r-module.testing.choices.string-chars), [`.string-limit`](../spec/10-modules.md#r-module.testing.choices.string-limit); they retire `module.testing.choices.string` |
| PT5 | [`module.testing.choices.map`](../spec/10-modules.md#r-module.testing.choices.map), [`.map.duplicate`](../spec/10-modules.md#r-module.testing.choices.map.duplicate) |
| PT6 | [Draw Budget](../spec/10-modules.md#draw-budget), [`module.testing.arbitrary.derive.simplest`](../spec/10-modules.md#r-module.testing.arbitrary.derive.simplest) |
| PT7 | the `examples` parameter of both signatures, [`module.testing.prop.examples`](../spec/10-modules.md#r-module.testing.prop.examples) |
| PT8 | [`module.testing.prop.body-no-discard`](../spec/10-modules.md#r-module.testing.prop.body-no-discard) |
| PT9 | [`module.testing.arbitrary.float`](../spec/10-modules.md#r-module.testing.arbitrary.float), and "a finite float" in `.float-generic` |
| Examples | [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults) holds the JSON round trip, which needs `std.json`; the [guide](../guide/LANGUAGE_TOUR.md#tests) holds the two-input test, the ordered pair, and a leaf-first recursive generator |
| Stateful | [Open Issues](OPEN_ISSUES.md) lists it as deferred |

Decided and applied 2026-09-30 (batch 13, all as recommended). They
answer the open points of batch 12.

| ID | Decision |
| --- | --- |
| Q5 | `arbitrary.with` is generic: `pub fn with[T < Inspectable](gen: fn(mut Choices) -> T) -> Generator`. It wraps `gen` to return each drawn value erased to `Inspectable`. The derived code downcasts the first drawn value to the member's type, and a failed downcast panics, naming the member and both types. So the member's type must be inspectable. The apply pass named the result type `Generator`. |
| Q6 | A derived `Arbitrary` for an enum with no finite value, where every variant is recursive, panics on the first case and names the type. There is no compile-time check, since no derivation check hook exists. |
| Q7 | A variant is recursive only when its simplest payload still needs a value of the enum. `List[E]`, `Map[_, E]`, and `E?` members do not count, since their simplest values are empty or `.None`. |
| Q8 | Simplest values: `T?` gives `.None`, `Result[T, E]` gives `.Ok` of `T`'s simplest value, and a tuple gives each element's simplest value. |
| Q9 | The four batch 12 readings below are confirmed: `examples` comes after `shrink` and before `prop`; `arbitrary.with` lives in `std.testing.arbitrary`; a mismatched generator panics with `explicit-panic`, naming the member; `max_chars` counts `char` values. |
| Q10 | A generic `int` with bare literals infers `i32`, the default integer literal type. Confirmed. |

| Decision | Rules |
| --- | --- |
| Q5 | [`module.testing.arbitrary.with.module`](../spec/10-modules.md#r-module.testing.arbitrary.with.module), [`.with.wrap`](../spec/10-modules.md#r-module.testing.arbitrary.with.wrap), [`.with.downcast`](../spec/10-modules.md#r-module.testing.arbitrary.with.downcast), [`.with.inspectable`](../spec/10-modules.md#r-module.testing.arbitrary.with.inspectable), [`.with.downcast-failure`](../spec/10-modules.md#r-module.testing.arbitrary.with.downcast-failure), which retires `module.testing.arbitrary.with.mismatch` |
| Q6 | [`module.testing.arbitrary.derive.no-finite`](../spec/10-modules.md#r-module.testing.arbitrary.derive.no-finite), [`.no-finite.unchecked`](../spec/10-modules.md#r-module.testing.arbitrary.derive.no-finite.unchecked) |
| Q7 | [`module.testing.arbitrary.derive.recursive`](../spec/10-modules.md#r-module.testing.arbitrary.derive.recursive), [`.recursive.containers`](../spec/10-modules.md#r-module.testing.arbitrary.derive.recursive.containers) |
| Q8 | [`module.testing.budget.simplest.optional`](../spec/10-modules.md#r-module.testing.budget.simplest.optional), [`.simplest.result`](../spec/10-modules.md#r-module.testing.budget.simplest.result), [`.simplest.tuple`](../spec/10-modules.md#r-module.testing.budget.simplest.tuple) |
| Q9 | unchanged, except that `.with.downcast-failure` and `.with.module` now state the panic category and the module |
| Q10 | unchanged; [`property-assume-discards.hd`](../spec/conformance/runtime/valid/property-assume-discards.hd) keeps its `i32` reading |

### Apply-Pass Readings

Applying batch 12 read these points from the decisions. The owner
confirmed all five on 2026-09-30 (Q9 and Q10).

| Reading | Where |
| --- | --- |
| `examples` goes after `shrink` and before `prop`: the property options follow `it`'s options, and `prop` stays last. | both signatures in [Property Tests](../spec/10-modules.md#property-tests) |
| `arbitrary` is the module `std.testing.arbitrary`, imported as `use std.testing.arbitrary`, so a member writes `@arbitrary.with(cents)`. | the Derived Arbitrary example; [`derived-arbitrary-with.hd`](../spec/conformance/runtime/valid/derived-arbitrary-with.hd) |
| The mismatch panic is `explicit-panic`, the category of the library's `panic` call. | [`derived-arbitrary-with-mismatch.hd`](../spec/conformance/runtime/valid/derived-arbitrary-with-mismatch.hd) |
| `max_chars` counts `char` values, the Unicode scalar values that a `char` holds. | [`module.testing.choices.string-limit`](../spec/10-modules.md#r-module.testing.choices.string-limit) |
| A generic `int` infers `i32` from bare literals, so `value := c.int(0, 100)` is an `i32`; an `i64` needs a context, as in `let value: i64 = c.int(0, 100)`. | [`property-assume-discards.hd`](../spec/conformance/runtime/valid/property-assume-discards.hd) |

Applying batch 13 read these points from the decisions. Each is listed so
the owner can confirm it.

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
