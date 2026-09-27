# F-259: The `disposed-file` runtime profile named in spec/conformance/README.md does not exist
Severity: minor
Area: test-integrity
Evidence: `hd check --profile disposed-file spec/conformance/runtime/valid/resource-disposed-result.hd` prints the usage text and exits 2 (re-checked 2026-09-26)
Effect: The only runtime case for `ResourceError.Disposed` cannot run. The CLI answers
with a usage error, not a diagnostic, so the portable runner reports "did not type-check".
Recommendation: implementation change: add the profile to src/cli.ts RUNTIME_PROFILES
and select the case. The profile's surface is fixed in spec/conformance/README.md
(Runtime Profiles): `open!`, `read!` (`Ok("")` before close), and `close`, with every
operation after a successful close returning `Err(ResourceError.Disposed)`. The
case also needs the fixture change in F-150: it exposes the private trait
`Files` from `pub fn main`.
