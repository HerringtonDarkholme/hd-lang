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
and the testing stress test are in git history.

## Still Open

| Question | Applied | **Recommendation** |
| --- | --- | --- |
| `Choices` beyond T53 | The STDLIB draft's `@derive(Arbitrary)`, its member-line facts, and size scheduling are not specified. The `__regressions__` format and the `assume` discard limit are decided and applied ([`module.testing.prop.regression-file`](../spec/10-modules.md#r-module.testing.prop.regression-file), [`module.testing.prop.discard`](../spec/10-modules.md#r-module.testing.prop.discard)). | Decide the rest with the property-test runner. |

## Decided, Waiting For Coverage

T54 (2026-09-28): test-layout fixture packages for
`cyclic-test-dependency` and for a `tests` root inside `tests/` are added
when those rules need coverage. The `# fixture-test-layout:` header exists
([Test Layouts](../spec/conformance/README.md#test-layouts)), but neither
code has a fixture yet.
