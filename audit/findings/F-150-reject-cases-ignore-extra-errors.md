# F-150: Reject cases pass even when unrelated errors are also reported
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-harness/plant-run.log (plants C03, F03); audit/evidence/01-harness/existing-unexpected-diagnostics.tsv
Effect: A reject fixture keeps passing when the file also fails for a second, unrelated reason. Two real selected cases do this today: `typing/invalid/nondisplay-entry-error.hd` and `typing/invalid/nonhost-entry-requirement.hd` both also report `private-type-leak` at line 3. The harness only checks that the marked code appears on the marked line (`containsLocatedCode`); it never checks the rest of the output.
Recommendation: implementation change (harness): in reject cases, fail on any error-severity diagnostic whose code or line differs from the marker, or record an explicit allow-list per case. Fix the two fixtures so `private-type-leak` does not fire (make the types `pub`). OPEN_ISSUES question: may a conformance reject case report errors other than its marker?
