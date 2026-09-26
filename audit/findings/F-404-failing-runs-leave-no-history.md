# F-404: `hd record` writes no history when the run panics or never finishes
Severity: minor
Area: runtime
Evidence: audit/evidence/04-runtime/roundtrip.tsv (verdict `not-recordable`: 5 panic fixtures; `cancellation-unwinds-suspending-closure.hd` record ends in a V8 out-of-memory report); audit/evidence/04-runtime/truncated-history.log
Effect: A panicking run exits 1 and leaves no `.replay.json`, so the failure cannot be replayed. Under `pending-gate`, `record` appends one event per spin until the heap is exhausted. A replay whose history ends early stops with an uncaught `Error: replay exhausted before provider site ...` and a Node stack trace.
Recommendation: implementation change: write the sidecar in a `finally` block, keep a history for a run that never finishes, and report replay mismatches as one structured line. Decided rule (future-work/RUNTIME_AND_LIBRARY.md "Replay Rules"): every run records a history, including one that panics or never finishes, and replay past the end of a history stops with a distinct failure instead of continuing live.
