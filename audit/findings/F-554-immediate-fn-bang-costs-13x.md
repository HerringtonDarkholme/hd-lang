# F-554: a `fn!` call that completes immediately costs about 13x a plain call, and -O2 does not help
Severity: minor
Area: runtime
Evidence: audit/evidence/05-requirements/bench.md (b4 rows, pair table, stub-hook line; `node --experimental-strip-types audit/scripts/arch/bench-run.ts`), audit/evidence/05-requirements/susp-probes.md "Poll cost versus depth", audit/bench/b4-suspend.dev.wat and b4-suspend.O2.wat
Effect: a 200,000-iteration loop of `step!(n)` takes 10.1 ms, against 0.79 ms for `step(n)`. That is 12.8x in dev and 12.7x after binaryen -O2 (least-contended samples, two runs). Stubbing the dev hooks `hd.pending` and `hd.trace` removes only about 13%, leaving 11x. Each immediate call still allocates a child frame, runs the state checks, spills and reloads every frame local, and walks the `$pc` dispatch chain. A root poll at depth D also makes D `pending` and 2D `trace` calls into JS.
Recommendation: implementation change. Run a child's first poll inline and allocate its frame only when it returns Pending. Emit `trace` and `pending` calls only in trace or fixture builds.
