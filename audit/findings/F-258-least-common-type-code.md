# F-258: Mixed-permission list literal reports `no-common-type` instead of `no-least-common-type`
Severity: minor
Area: correctness
Evidence: `hd check spec/conformance/typing/invalid/least-type-weakening-variance.hd` prints `5:21: no-common-type: list elements have no common type: mut:list[mut:User] and list[User]`
Effect: The elements have common types but no unique least one, which
spec/04-type-system.md ("Least Common Type") names `no-least-common-type`. The
prototype reports `no-common-type` on the right line, so a conformance runner
reports a failure and tooling keyed on the code misclassifies the error.
Recommendation: implementation change: emit `no-least-common-type` when common types
exist but no least one does.
