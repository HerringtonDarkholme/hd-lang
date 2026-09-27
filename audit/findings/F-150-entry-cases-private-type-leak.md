# F-150: Entry-point cases use a private declaration in `pub fn main`, so they also report `private-type-leak`
Severity: minor
Area: test-integrity
Evidence: `hd check` on `spec/conformance/typing/invalid/nondisplay-entry-error.hd` and `.../nonhost-entry-requirement.hd` reports `private-type-leak` at 3:5 besides the marked code; `hd build spec/conformance/runtime/valid/resource-disposed-result.hd` reports `private-type-leak` for the private trait `Files` at 14:5 (re-checked 2026-09-26)
Effect: Each case declares a private declaration (`HiddenError`, `Database`,
`Files`) and uses it in the signature or requirement row of `pub fn main`. Since `main` must be public
to be an entry point (10-modules.md, Executable Entry Point), the signature
exposes a private type, which 10-modules.md makes a `private-type-leak` error.
No conforming implementation can pass the two reject cases, because the runner
rejects errors other than the marker. The runtime case is tagged F-259 because
it fails earlier on the missing profile.
Recommendation: spec fixture change: make the declarations that `main` exposes
`pub` in the three cases, then move the two reject cases to
test/portable/cases.tsv.
