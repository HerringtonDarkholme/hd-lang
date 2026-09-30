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
decisions PT1-PT9 (batch 12, 2026-09-29) are decided, not yet applied.

## Owner Decisions

Decided 2026-09-29 (batch 12, the property-test API), not yet applied. The
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
