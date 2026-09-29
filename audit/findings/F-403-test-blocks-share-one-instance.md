# F-403: `hd test` runs `main` and every test block in one shared instance
Severity: major
Area: runtime
Evidence: audit/probes/runtime/instances/ (test-isolation.hd, main-state-visible-to-test.hd, panic-then-pass.hd; run each with `hd test`)
Effect: Two tests that each increment a top-level counter and expect 1: the second fails with `assertion-failed`. A test sees the value `main` assigned. When the first test panics, the second never runs. The output names neither test. Spec 02 says "Each test runs in its own program instance ... Instances are not reused between tests." spec/conformance/README.md "Runtime Execution" says `main` does not run in a test's instance.
Recommendation: implementation change: instantiate once per test block, run `main` only in its own instance, and report each test by name.
Progress (2026-09-28): the prototype now runs each test case and each `it_each` row in a fresh instance and names a failing test case. `main` still runs in the first instance, and a panic outside `expect_panic` still stops the run.
The probes use the test-block syntax of 2026-09-25 and no longer parse; the Progress line is the current status.
