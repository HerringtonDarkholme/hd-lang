# F-266: Evaluation-order fixtures return the same value under any evaluation order
Severity: minor
Area: test-integrity
Duplicates: F-210 (overlaps) (found independently by another worker; phase-2 evidence corroborates)
Evidence: test/fixtures/compiler/13-named-arguments-map-to-parameters-without-changing-source-evaluation-ord.hd and 33-data-copy-update-evaluates-its-source-before-replacements.hd (both pass); audit/probes/claims/p01-named-arg-order.hd and p13-copy-update-order.hd log the order and pass; controls audit/probes/claims/controls/k01-*.hd and k13-*.hd flip the expected order and fail with `assertion-failed` (audit/evidence/02-claims/probes.tsv)
Effect: Fixture 13 computes `combine(second=first!(), first=second!())` from constants, and
fixture 33 adds two fields. Both results are 21 and 42 whatever the evaluation order. An
implementation that evaluates in parameter order still passes. The behavior is correct
today only because the audit probes check it.
Recommendation: implementation change to the fixtures: record side effects in order and
assert the log, as the audit probes do.
