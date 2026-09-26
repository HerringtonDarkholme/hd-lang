# F-707: A comma at depth zero does not end a same-line suite
Severity: minor
Area: correctness
Evidence: audit/evidence/w9/verify-findings.log (F-707 blocks: `hd parse` on spec/conformance/parse/invalid/same-line-let-comma.hd and same-line-suite-comma.hd prints `ok`, exit 0)
Effect: `test "pair": let a, b = pair()` and `if flag: a, b := pair()` parse. Spec 01 (layout) says a comma at the depth where a same-line suite opened always closes it, so both are `syntax-error`. Code that the MVP accepts is rejected by a conforming parser.
Recommendation: implementation change: close the same-line suite at a comma at its opening depth.
