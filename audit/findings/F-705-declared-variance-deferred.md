# F-705: Declared variance is deferred, so covariant data cannot be written
Severity: note
Area: coverage
Evidence: audit/evidence/w9/verify-findings.log (F-705 block: `hd check spec/conformance/runtime/valid/covariant-readonly-weakening.hd` reports `expected-token: expected a generic data parameter` at `data Producer[+T]`, exit 1); src/MVP_IMPLEMENTATION_PLAN.md line 448 lists declared variance as deferred
Effect: `data Producer[+T]` is a parse error, so the runtime case for readonly covariant weakening cannot run. The deferral is documented in the MVP plan, but the diagnostic is a generic parse error, as F-250 describes for other deferred features.
Recommendation: implementation change when variance is scheduled; until then keep the case in test/portable/KNOWN_FAILURES.tsv.
