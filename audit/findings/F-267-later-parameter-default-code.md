# F-267: A default that reads a later parameter is reported as impure
Severity: minor
Area: spec
Duplicates: F-356 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-claims/repros.log (F-267); audit/probes/claims/p06-default-later-param-rejected.hd
Effect: `fn bad(first: i32 = second, second: i32 = 1)` is rejected with
`impure-parameter-default: default for 'bad.first' is not compile-time pure`. The default is
pure; it breaks the ordering rule in 07-functions.md line 104. The code is also absent
from the spec/README.md inventory, so no portable fixture can name it.
Recommendation: OPEN_ISSUES question: which stable code should a forward parameter
reference use (for example `binding-not-yet-visible`), and should `impure-parameter-default`
join the inventory?
