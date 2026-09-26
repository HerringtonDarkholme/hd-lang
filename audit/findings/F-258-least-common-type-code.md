# F-258: Mixed-permission list literal reports `no-common-type` instead of `no-least-common-type`
Severity: minor
Area: correctness
Evidence: audit/evidence/02-coverage/unselected.tsv row typing/invalid/least-type-weakening-variance.hd; `hd check` prints `5:21: no-common-type: list elements have no common type: mut:list[mut:User] and list[User]`
Effect: The right line is rejected with a different stable code than the one the spec
names (05-expressions.md#list-and-map-expressions). A conformance runner reports a failure,
and tooling keyed on the code misclassifies the error.
Recommendation: implementation change to emit `no-least-common-type` for this case.
OPEN_ISSUES question: should the spec state when each of the two codes applies?
