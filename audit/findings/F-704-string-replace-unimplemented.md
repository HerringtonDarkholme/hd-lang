# F-704: `string.replace` is not implemented
Severity: minor
Area: coverage
Evidence: audit/evidence/w9/verify-findings.log (F-704 blocks: `hd check` on spec/conformance/runtime/valid/replace-non-overlapping.hd and multibyte-scalar-strings.hd reports `unknown-method: type 'string' has no supported method 'replace'`, exit 1)
Effect: The prelude table in spec 10 (Prelude) lists `replace(self, old: string, replacement: string) -> string`, and spec 07 uses it in an example. Calling it fails with `unknown-method`, a code outside the spec/README.md inventory, so the user reads it as a typo.
Recommendation: implementation change: implement `replace` with non-overlapping, left-to-right matching, or report a structured unsupported diagnostic.
