# F-153: `# panic: runtime-error` accepts any failing exit, including compiler crashes
Severity: major
Area: test-integrity
Evidence: audit/evidence/01-harness/plant-run.log (plants F05a, F13); audit/evidence/01-special-casing/crash-triggers.log (k3)
Effect: In the fixture suite, `runFixtureCase` skips the code check when the directive value is `runtime-error`. Any nonzero exit therefore passes, including a Binaryen WAT parse crash (F13) or a CLI `Error` stack trace (F05a). The one fixture that uses it, `test/fixtures/compiler/05-...-negative.hd` (`2 ** -1`), hides a raw `RuntimeError: unreachable` Wasm trap and stack trace, with no stable panic category. `runtime-error` is not a category in spec 06-control-flow.md#runtime-panics.
Recommendation: implementation change: remove the `runtime-error` exemption and give negative exponents a stable category. OPEN_ISSUES question: which stable category does a negative integer exponent report (`integer-overflow`, or a new one)?
