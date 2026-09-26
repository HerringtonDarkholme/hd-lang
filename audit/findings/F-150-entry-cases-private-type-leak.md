# F-150: Two entry-point reject cases also report `private-type-leak`
Severity: minor
Area: test-integrity
Evidence: `hd check spec/conformance/typing/invalid/nondisplay-entry-error.hd` and `.../nonhost-entry-requirement.hd` both report `private-type-leak` at 3:5 besides the marked code; the conformance runner fails them with "other located errors reported"
Effect: Both cases declare a private declaration (`HiddenError`, `Database`) and use it in the
signature of `pub fn main`. Since `main` must be public to be an entry point
(10-modules.md, Executable Entry Point), the signature exposes a private type, which
10-modules.md makes a `private-type-leak` error. No conforming implementation can pass
either case, because the runner now rejects errors other than the marker.
Recommendation: spec fixture change: make `HiddenError` and `Database` `pub` in the two
cases, then move them to test/portable/cases.tsv.
