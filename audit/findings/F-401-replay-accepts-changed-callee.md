# F-401: Replay accepts a changed body in an executed non-suspending function
Severity: minor
Area: runtime
Evidence: audit/evidence/04-runtime/edits.tsv (row `v04-helper-body-change.hd`)
Effect: base.hd is recorded, and its `main` returns 42. Changing `helper` from `value * 2` to `value * 3` still replays with exit 0 and prints 63. The history claims one execution, and the replay produces a different result with no warning. src/MVP_IMPLEMENTATION_PLAN.md says replay "rejects changed executed functions". Only functions that own a suspension event are checked.
Recommendation: implementation change. Decided rule (future-work/RUNTIME_AND_LIBRARY.md "Replay Rules"): code identity is a hash of the whole module's semantic content, so any semantic change anywhere in the module, including this helper body, invalidates the history. Hash the whole module, not each suspension-site owner.
