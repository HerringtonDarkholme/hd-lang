# F-506: Suspension frames keep every local, including dead temporaries and heap references
Severity: note
Area: runtime
Evidence: audit/evidence/05-object-model/frames.tsv (`node --experimental-strip-types audit/scripts/arch/alloc-frames.ts`); audit/evidence/05-object-model/wat/alloc-frames.wat
Effect: frame value slots against the most locals live across one suspension (liveness found by hand): `wide!` 7 vs 0, `accumulate!` 15 vs 3, `heap!` 6 vs 0, `phases!` 5 vs 1. `heap!` keeps a string, a list, and a tuple reachable across both suspensions after they are dead, so a stored suspension can hold large garbage alive. The plan defers liveness, so this is expected. Each immediately-ready `fn!` call still allocates one frame (alloc-per-iteration.tsv, loop_suspend_ready).
Recommendation: implementation change, later. Run liveness over the suspension CFG and spill only live values. Clear dead reference slots at each suspension point before full liveness exists.
