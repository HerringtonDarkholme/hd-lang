# F-251: A plain `fn` closure may mutate a captured mutable list; the program is accepted and runs
Severity: major
Area: correctness
Duplicates: F-351 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-claims/repros.log (section F-251); probes audit/probes/claims/c2-plain-closure-mutates-capture.hd, c2-plain-closure-mutation-runs.hd; conformance case spec/conformance/typing/invalid/plain-closure-mutable-capture.hd (unselected) is accepted by `hd check`
Effect: `fn(value: i32) -> void: items.append(value)` over a captured `mut list[i32]` type-checks,
and `hd run --entry run c2-plain-closure-mutation-runs.hd` prints `3` after two appends through
the plain closure. 07-functions.md#captures requires `mutable-capture-requires-mut-fn`:
inside a plain closure a captured `mut T` is viewed as readonly. The permission system
has a hole: any readonly callback type can hide mutation of captured state.
Recommendation: implementation change: view captures as readonly inside plain `fn`
closures and emit `mutable-capture-requires-mut-fn`; then select the conformance case.
