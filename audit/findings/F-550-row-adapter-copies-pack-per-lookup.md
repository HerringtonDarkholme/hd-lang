# F-550: row-generic callback adapter copies the whole provider pack once per lookup
Severity: major
Area: runtime
Evidence: audit/evidence/05-requirements/req-scaling.md (`node --experimental-strip-types audit/scripts/arch/req-probes.ts`), audit/evidence/05-requirements/row-chain-5.wat (`$adapt0`), audit/evidence/05-requirements/abi.wat (`$adapt0`)
Effect: calling a named function through a row-generic callback parameter (`fn g[r](cb: fn(i32) -> i32 $ r)`) costs O(K^2) allocations and time for a K-entry row. Measured per 10-deep call: K=1: 1 pack node copied, 13 ns; K=5: 25 nodes, 85 ns; K=10: 100 nodes, 334 ns. The same chain with concrete rows stays at 4-5 ns for K=0..10.
Recommendation: implementation change. Build the union pack once per adapter call, or pass `$p0` directly when the formal row is a single row variable:

```wat
;; today, repeated for every requirement of the target:
(call $hd.provider_get (call $hd.provider_concat (local.get $p0) (ref.null $hd.providers)) (i32.const 4))
;; instead:
(local.set $pack (local.get $p0))   ;; once
(call $hd.provider_get (local.get $pack) (i32.const 4))
```

Mechanism: `emitCallableAdapters` in `src/emitter/function-body.ts` inlines the `formalUnion` expression string into every `provider_get` call, and `provider_concat(x, null)` copies `x` (see F-551). The instrumented run counts `provider_concat` entries and non-null copies per call (K+K^2 calls, K^2 nodes).
