# F-210: Nineteen fixtures claim runtime or implementation behavior their oracle cannot observe
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-test-quality/inventory.tsv (15 rows `implementation detail`); audit/evidence/01-test-quality/oracle.tsv (column c_runtime_observation: 3 `result-constant-literal`); probes.log (assert_equal(0.0, -0.0) passes)
Effect: the portable run counts these as coverage of their claims, but they only prove that a program compiles or returns a constant.
- suspension/11 and suspension/12 claim host pending and driver misuse; the body is `fn main!() -> i32: 42`.
- suspension/15, suspension/27, suspension/31 and traits/suspending-cancel claim runtime effects (cancellation order, console output, shared module state) under `# expect: accept`, which only type-checks.
- suspension/35 checks a -0.0 round trip with `assert_equal`, which follows IEEE equality and cannot see a lost sign bit.
- Twelve more implementation-detail fixtures (trace, replay, host-profile, explain-requirements and AST-snapshot inputs) run in the portable suite with unrelated oracles.
Recommendation: fixture change: move the implementation-detail inputs out of the portable run, give runtime claims an observation, and use a sign-sensitive check for -0.0. OPEN_ISSUES question: should the format gain a stdout expectation for console fixtures?
