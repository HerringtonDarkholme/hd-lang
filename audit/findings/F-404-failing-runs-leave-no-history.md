# F-404: `hd record` writes no history when the run panics or never finishes
Severity: minor
Area: runtime
Evidence: audit/evidence/04-runtime/roundtrip.tsv (verdict `not-recordable`: 5 panic fixtures; `cancellation-unwinds-suspending-closure.hd` record ends in a V8 out-of-memory report); audit/evidence/04-runtime/truncated-history.log
Effect: A panicking run exits 1 and leaves no `.replay.json`, so the failure cannot be replayed. Under `pending-gate`, `record` appends one event per spin until the heap is exhausted. A replay whose history ends early stops with an uncaught `Error: replay exhausted before provider site ...` and a Node stack trace.
Recommendation: implementation change: write the sidecar in a `finally` block and report replay mismatches as one structured line. OPEN_ISSUES question: when history runs out, should replay continue live, as RUNTIME_AND_LIBRARY.md describes for durable runs?
