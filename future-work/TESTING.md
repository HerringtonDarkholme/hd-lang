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
[Derived Arbitrary](../spec/10-modules.md#derived-arbitrary).

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
| PT2 | [`module.testing.arbitrary.derive`](../spec/10-modules.md#r-module.testing.arbitrary.derive), [`.with`](../spec/10-modules.md#r-module.testing.arbitrary.with), [`.with.only`](../spec/10-modules.md#r-module.testing.arbitrary.with.only), [`.with.unchecked`](../spec/10-modules.md#r-module.testing.arbitrary.with.unchecked), [`.with.mismatch`](../spec/10-modules.md#r-module.testing.arbitrary.with.mismatch), [`.with.no-fallback`](../spec/10-modules.md#r-module.testing.arbitrary.with.no-fallback); [Member Metadata](../spec/14-annotations.md#member-metadata) names the fact |
| PT3 | [`module.testing.choices.int-generic`](../spec/10-modules.md#r-module.testing.choices.int-generic), [`.float-generic`](../spec/10-modules.md#r-module.testing.choices.float-generic); they retire `module.testing.choices.int` and `.float` |
| PT4 | [`module.testing.choices.string-chars`](../spec/10-modules.md#r-module.testing.choices.string-chars), [`.string-limit`](../spec/10-modules.md#r-module.testing.choices.string-limit); they retire `module.testing.choices.string` |
| PT5 | [`module.testing.choices.map`](../spec/10-modules.md#r-module.testing.choices.map), [`.map.duplicate`](../spec/10-modules.md#r-module.testing.choices.map.duplicate) |
| PT6 | [Draw Budget](../spec/10-modules.md#draw-budget), [`module.testing.arbitrary.derive.simplest`](../spec/10-modules.md#r-module.testing.arbitrary.derive.simplest) |
| PT7 | the `examples` parameter of both signatures, [`module.testing.prop.examples`](../spec/10-modules.md#r-module.testing.prop.examples) |
| PT8 | [`module.testing.prop.body-no-discard`](../spec/10-modules.md#r-module.testing.prop.body-no-discard) |
| PT9 | [`module.testing.arbitrary.float`](../spec/10-modules.md#r-module.testing.arbitrary.float), and "a finite float" in `.float-generic` |
| Examples | [STDLIB](STDLIB.md#proposal-choices-first-arbitrary-for-defaults) holds the JSON round trip, which needs `std.json`; the [guide](../guide/LANGUAGE_TOUR.md#tests) holds the two-input test, the ordered pair, and a leaf-first recursive generator |
| Stateful | [Open Issues](OPEN_ISSUES.md) lists it as deferred |

### Apply-Pass Readings

Applying batch 12 read these points from the decisions. Each is listed so
the owner can confirm it.

| Reading | Where |
| --- | --- |
| `examples` goes after `shrink` and before `prop`: the property options follow `it`'s options, and `prop` stays last. | both signatures in [Property Tests](../spec/10-modules.md#property-tests) |
| `arbitrary` is the module `std.testing.arbitrary`, imported as `use std.testing.arbitrary`, so a member writes `@arbitrary.with(cents)`. | the Derived Arbitrary example; [`derived-arbitrary-with.hd`](../spec/conformance/runtime/valid/derived-arbitrary-with.hd) |
| The mismatch panic is `explicit-panic`, the category of the library's `panic` call. | [`derived-arbitrary-with-mismatch.hd`](../spec/conformance/runtime/valid/derived-arbitrary-with-mismatch.hd) |
| `max_chars` counts `char` values, the Unicode scalar values that a `char` holds. | [`module.testing.choices.string-limit`](../spec/10-modules.md#r-module.testing.choices.string-limit) |
| A generic `int` infers `i32` from bare literals, so `value := c.int(0, 100)` is an `i32`; an `i64` needs a context, as in `let value: i64 = c.int(0, 100)`. | [`property-assume-discards.hd`](../spec/conformance/runtime/valid/property-assume-discards.hd) |

## Still Open

| Question | Applied | **Recommendation** |
| --- | --- | --- |
| Parameter style for generators | This batch keeps `mut Choices` parameters. The owner is comparing a requirement-row style (`fn() -> T $ Choices`), which may replace it. | Re-evaluate the two styles against a working compiler. |
| How derived code recovers a generator from `Any` | PT2 says the derived code casts the `Any` value to `fn(mut Choices) -> T`. But `Any` erasure is one-way ([`types.any.one-way`](../spec/04-type-system.md#r-types.any.one-way)), and a function is not `Inspectable` ([`trait.inspectable.not.function`](../spec/09-traits.md#r-trait.inspectable.not.function)), so no hd code can make that cast. The spec states only the behavior ([`module.testing.arbitrary.with.mismatch`](../spec/10-modules.md#r-module.testing.arbitrary.with.mismatch)). | Make the constructor generic, `with[T < Inspectable](gen: fn(mut Choices) -> T)`, and store `fn(c) -> Inspectable: gen(c)`. The derived code downcasts the first drawn value to the member's type; a failed downcast is PT2's panic. |
| A type with no finite value | An enum whose every variant is recursive, such as `enum Loop: More(next: Loop)`, has no non-recursive variant, so [`module.testing.arbitrary.derive.simplest`](../spec/10-modules.md#r-module.testing.arbitrary.derive.simplest) gives no choice. No rule covers it. Without one, the derived code recurses until the case panics with `stack-exhausted`. | The derived `arbitrary` panics on the first case with a message naming the type, as PT2's mismatch does. |
| What makes a variant recursive | PT6 names the first non-recursive variant, and no rule says when a variant is recursive. | A variant is recursive when its simplest payload still needs a value of the enum: a member of the enum's type, directly or through a data type's or tuple's members. `List`, `Map`, and `T?` members do not count, since their simplest values are empty or `.None`. |
| Simplest values of the other std generators | PT6 lists integers, floats, `bool`, `pick`, `list`, `map`, and `string`, but not `T?`, `Result[T, E]`, or tuples. | `T?` gives `.None`, `Result[T, E]` gives `.Ok` of `T`'s simplest value, and a tuple gives each element's simplest value. |

## Decided, Waiting For Coverage

T54 (2026-09-28): test-layout fixture packages for
`cyclic-test-dependency` and for a `tests` root inside `tests/` are added
when those rules need coverage. The `# fixture-test-layout:` header exists
([Test Layouts](../spec/conformance/README.md#test-layouts)), but neither
code has a fixture yet.
