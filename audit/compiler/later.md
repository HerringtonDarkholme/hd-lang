# Later: Parked Compiler Work

Work that is decided or noted but deliberately not scheduled. These items
are kept out of the orchestrator's task list, which shows only work in
priority order. An item moves back into the task list when the owner
lifts its hold or its phase starts.

| Item | Why it waits | Notes |
| --- | --- | --- |
| Phase 3, make it wonderful | Phase 2 (make it work) first | Optimized pipeline; `br_table` resume dispatch; liveness; i31 fast path; Binaryen or wasmtime chosen by measurement. Size: link the suspension runtime only when a program suspends (~660 B); link the forbid counter and its `block_on` check only when a program has a bracketed default (+285 B on hello since #32); inline constant defaults; derives are 1.8x hand-written (merge identical functions, codegen.md §13.7; handles as constant globals, wasm-layout.md §15.4); capture-free closures and adapters as constant globals (§12.2, §13.11); `runtime_type` allocates a `TypeId` per call. |
| Repo restructure | Held by the owner | Move `src/` to `old_ts/` and `compiler/` to the repository root workspace. |
| Agent-harness research | Parked by the owner | REPL research, tool adapters and access control, a program database, hot reload and plugins, AR4 package-subsystem gaps. |
| Row-polymorphic bodies compiled once | Phase 3 | `req.poly.one-body`: one body with a provider bundle, not one per row. |
| `concat_all` (single-pass `join`) | Spec frozen; nothing new now | The owner chose option A1. Logged in `diagnostic-notes.md`; no spec row and no intrinsic until the freeze lifts. |
| `hd doc`, the `hd fmt` layout, fetching (`hd add`, `hd update`) | Need designs `commands.md` lacks; nothing new now | From the second Opus's O8 questions. |
