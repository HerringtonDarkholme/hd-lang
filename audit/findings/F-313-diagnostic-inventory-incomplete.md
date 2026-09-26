# F-313: The normative diagnostic inventory omits most codes that hd and the reference parser emit
Severity: major
Area: spec
Related: F-205 (fixture markers use the same missing codes); this file measures implementation output under fuzzing
Evidence: audit/evidence/03-fuzz/round1/contract/uninventoried-codes.tsv; round2 same file; SUMMARY.md "Uninventoried codes"
Effect: in round 1, 2,229 of 5,000 contract inputs (45%) got a `check` rejection with a code missing from the spec/README.md table. That is 58% of all `check` rejections, across 44 distinct codes. The top ones are `expected-expression`, `unknown-name`, `expected-token`, `unknown-type`, `type-mismatch`, and `duplicate-binding`. The reference parser also emits eight codes the table lacks, including `syntax-error`, `unclosed-delimiter`, and `unexpected-indentation`. test/README.md uses `type-mismatch` as its example fixture code. A second implementation cannot match these diagnostics from the spec alone.
Recommendation: OPEN_ISSUES question: should the inventory cover parse and name/type errors, with codes such as `type-mismatch` and `unknown-name`? Or should portable fixtures use only inventoried codes, with the rest treated as implementation-defined?
