# F-208: Four reject cases repeat their code on other lines, so the marker does not isolate one construct
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-test-quality/oracle-runs.tsv (verdict `code-persists` and column extra_list); audit/evidence/01-test-quality/probes.log (`hd check spec/conformance/typing/invalid/discarded-result.hd`)
Effect: deleting the marked line leaves the same code in the file. typing/invalid/discarded-result.hd reports `discarded-must-use-value` on lines 9, 12 and 15. generic-requirement-key-collision.hd reports its code on lines 5 and 11, and supertrait-cycle.hd on lines 1 and 4. test/fixtures traits/multiple-bounds/duplicate-bound.hd reports on lines 22 and 28. An implementation that misses the marked construct but catches a sibling one still fails, yet the case gives no clue which. (The two entry fixtures with an extra `private-type-leak` are F-150.)
Recommendation: fixture change: one offending construct per reject file; move the others into their own cases.
