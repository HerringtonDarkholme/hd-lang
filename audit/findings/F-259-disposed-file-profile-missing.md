# F-259: The `disposed-file` runtime profile named in spec/conformance/README.md does not exist
Severity: minor
Area: test-integrity
Evidence: audit/evidence/02-coverage/unselected.tsv.log section runtime/valid/resource-disposed-result.hd: `hd check --profile disposed-file ...` prints the usage text and exits 2
Effect: The only runtime case for `ResourceError.Disposed` cannot run. The CLI answers
with a usage error, not a diagnostic, so the portable runner reports "did not type-check".
Recommendation: implementation change: add the profile to src/cli.ts RUNTIME_PROFILES
and select the case.
