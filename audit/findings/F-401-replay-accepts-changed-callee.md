# F-401: Replay accepts a changed body in an executed non-suspending function
Severity: minor
Area: runtime
Evidence: audit/evidence/04-runtime/edits.tsv (row `v04-helper-body-change.hd`)
Effect: base.hd is recorded, and its `main` returns 42. Changing `helper` from `value * 2` to `value * 3` still replays with exit 0 and prints 63. The history claims one execution, and the replay produces a different result with no warning. src/MVP_IMPLEMENTATION_PLAN.md says replay "rejects changed executed functions". Only functions that own a suspension event are checked.
Recommendation: OPEN_ISSUES question: should a replay code identity cover every function reachable from the entry, or only suspension-site owners? If only owners, the MVP plan should stop claiming "changed executed functions" are rejected.
