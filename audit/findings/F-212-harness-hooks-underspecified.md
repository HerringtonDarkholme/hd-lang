# F-212: Two harness hooks cannot be built from their description, and one is implemented nowhere
Severity: minor
Area: spec
Evidence: audit/evidence/01-test-quality/directives.tsv (rows fixture-runtime-profile, fixture-runtime-scenario, fixture-package-role); probes.log (`hd check --profile disposed-file` exits 2 with usage)
Effect: `fixture-package-role` names synthetic packages `dep.validation` and `dep.models` whose contents appear nowhere, and no runner reads the directive. The `disposed-file` profile does not state the `Files` trait surface or what close does, and the MVP does not implement it. `competing-drivers` says "two drivers" without defining them in source terms. The MVP implements it as "poll once while pending, then call `main` again", which panics even when `main!` is `pass`.
Recommendation: spec change: define each hook as a numbered host procedure, with the fixture source for any synthetic package included in spec/conformance/ (drafted in audit/proposals/fixture-format.md).
