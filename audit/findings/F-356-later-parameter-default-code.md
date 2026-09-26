# F-356: A default that names a later parameter reports a code outside the spec inventory
Severity: note
Area: spec
Evidence: audit/evidence/03-blind-run/run.log (a8-default-later-parameter-rejected: `2:23: impure-parameter-default: default for 'later.first' is not compile-time pure`); audit/evidence/03-blind-run/variants.log (a8-default-later.amb15.hd passes with the implementation code)
Effect: `fn later(first: i32 = second, second: i32 = 1)` is rejected, as spec/07-functions.md#default-values requires, but with `impure-parameter-default`, which is not in the spec/README.md diagnostics table. The spec names no code for this rule, so a second implementation cannot match either. It also calls a visibility error an impurity error.
Recommendation: OPEN_ISSUES question: which stable code covers a default that refers to a later parameter (`binding-not-yet-visible`?), and which covers an impure parameter default (the table has only `impure-data-default` and `impure-enum-default`)?
