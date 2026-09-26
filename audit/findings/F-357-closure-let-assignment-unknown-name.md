# F-357: Assigning captured `let` storage in a plain closure reports `unknown-name`
Severity: note
Area: correctness
Evidence: audit/evidence/03-blind-run/probes.log (p-plain-closure-assign-let.hd: `5:9: unknown-name: unknown binding 'count'`)
Effect: `fn() -> i32: count = count + 1` over a captured `let count` is rejected, but the message says the binding does not exist. The reader is sent looking for a typo. spec/07-functions.md#captures makes this a `mutable-capture-requires-mut-fn` error.
Recommendation: implementation change: resolve the name, then report `mutable-capture-requires-mut-fn`.
