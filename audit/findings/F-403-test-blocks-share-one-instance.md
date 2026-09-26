# F-403: `hd test` runs `main` and every test block in one shared instance
Severity: major
Area: runtime
Evidence: audit/evidence/04-runtime/instances-and-panic-output.log (probes under audit/probes/runtime/instances/)
Effect: Two tests that each increment a top-level counter and expect 1: the second fails with `assertion-failed`. A test sees the value `main` assigned. When the first test panics, the second never runs. The output names neither test. Spec 02 says "Each test runs in its own program instance ... Instances are not reused between tests." Fixtures 32, 33, 35 and 36 assert on a global that only `main!` sets. They would fail under the spec's rule.
Recommendation: implementation change: instantiate once per test block, run `main` only as its own case, and report each test by name. Fixture change: move the assertions in 32/33/35/36 into `main!` or a test that drives the provider itself.
