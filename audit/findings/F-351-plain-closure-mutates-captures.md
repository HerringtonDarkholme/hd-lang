# F-351: A plain `fn` closure may mutate through a captured `mut` value
Severity: major
Area: correctness
Evidence: audit/evidence/03-blind-run/probes.log (p-plain-closure-mut-capture.hd: `hd check` exits 0, marker unmatched; p-plain-closure-mutates-runtime.hd: `hd run --entry probe` prints 7, i.e. `user.name == "renamed"`); `hd check spec/conformance/typing/invalid/plain-closure-mutable-capture.hd` prints `ok`
Effect: `call := fn() -> void: rename(user)` with a captured `user: mut User` type-checks and mutates `user` at run time. spec/07-functions.md#captures says a plain closure views captured `mut T` as readonly `T` and must be `mut fn` to obtain mutable access (`mutable-capture-requires-mut-fn`). The code never appears in src/. The spec's own conformance case for it is not selected in test/portable/cases.tsv, so the gap is untested.
Recommendation: implementation change: weaken captured bindings to readonly inside plain closures and emit `mutable-capture-requires-mut-fn`; select typing/invalid/plain-closure-mutable-capture.hd.
