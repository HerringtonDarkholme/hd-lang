# REPL journal checkpoints

Status: design proposal, 2026-10-23; nothing here is accepted behavior or specification.
This answers [#646](https://github.com/HerringtonDarkholme/hd-lang/issues/646), following the owner's request for measured options before code.
It reviews [Live Execution](live-execution.md), [REPL rebuilding](../../spec/cli/command-line.md#rebuilding-a-session), and the RC backend's current session hosts.

## Today and measured cost

The RC REPL compiles every kept input into one module, creates a fresh instance, and replays earlier inputs before each new input.
[Node and browser](../../host/core.mjs) share this implementation; [Wasmtime](../../crates/hd_run_wasmtime/src/session.rs) recreates its Store over the same host.
Host results are served from the journal, never obtained live again.
At 64 MiB, recording stops and the next input is refused; the older live-execution sketch's live-instance fast path is not today's implementation.

The cap measures recorded bytes, not guest state or replay CPU work.
The [browser save budget](../../website/playground/src/repl-state.ts) is separately 4 MiB for journal, files, inputs and transcript together.
A checkpoint exceeding that budget cannot silently become a persistent playground session.

Two real session programs use `fill(67104768)` from `std.random`.
One retains the returned byte list as `bytes`; the other immediately evaluates its length and discards it.
Both produce a 67,104,885-byte journal, 3,979 bytes below the cap; another 8 KiB result stops recording.

| Actual replay path | Retained result, median ms | Discarded result, median ms |
| --- | ---: | ---: |
| Wasmtime 49.0.2, new Store, same host | 2110.40 | 2131.32 |
| Node 24.19.0, fresh host | 996.51 | 932.70 |
| Chrome 155, fresh browser host | 760.50 | 753.00 |

Each median includes all three trials.
Fresh hosts and Stores mean fresh guest state; engine code caches can remain warm.
Node and Chrome replay the native journal with zero live host calls; native measures its existing same-host rebuild API.
Chrome runs the actual browser host, without the playground compiler, IndexedDB save, worker replacement or page reload.
These are shared-host observations, not an optimization result or a portable resource-restore claim.

A second scratch probe exports private state from the final RC module, copies its memories and globals, and restores the identical code.
The restored instance runs input two with the expected output, without replaying input one.
Both workloads copy 134,414,336 bytes: 64 MiB exchange memory plus 64.1875 MiB heap, even after the result becomes dead.
There are 29/28 globals and one statically initialized function table; identical-code instantiation recreates that table.

| Snapshot primitive | Retained: capture / median restore ms | Discarded: capture / median restore ms |
| --- | ---: | ---: |
| Node | 87.22 / 34.34 | 11.41 / 15.33 |
| Chrome | 12.90 / 18.60 | 11.70 / 29.00 |

Capture has one sample; restore has three, including instantiation, memory growth, copying and globals.
These costs exclude compilation, checkpoint-module generation, serialization, storage, resource handles and heap compaction.
Native checkpoint access was not added: its existing public session API hides this state.
**The complete checkpoint cost remains unknown on every engine.**

Measurements pin `d4b85c0be845fb426bc1476d65693d35f9dae3b0`; the scratch executable uses the dev profile and existing compiler/runtime APIs.
Compilation and execution use scratch `HD_CACHE` paths inside the isolated checkout.
The [measurement report](https://github.com/HerringtonDarkholme/hd-lang/issues/646#issuecomment-6370301464) records exact commands, every raw trial, load context, source hashes and frozen artifacts.
The two inputs compile without diagnostics; no new hd syntax is proposed.

## Three mechanisms

| Mechanism | What later inputs use | Cost and limit |
| --- | --- | --- |
| **Base module with initialized bindings** | Import the base's functions, binding globals, memories, table and runtime state | Requires a session linker and stable runtime ABI; avoids executing the checkpointed prefix |
| Fixed-layout memory/global snapshot | Instantiate the identical module and restore its state | Measured primitive is fast to copy, but larger than this journal; adding an input changes code and can invalidate layout |
| Compacted host-result replay | Re-execute kept inputs using selected recorded results | Keeps the existing replay mechanism, but needs dependency evidence; result reachability alone cannot identify removable calls |

All three concern CLI behavior and compiler/runtime representation, rather than a new language form or std API.
The replay option is the smallest implementation change, but cannot guarantee compaction for arbitrary sessions.
A local random result may become unreachable after selecting a branch that mutates an earlier binding.
Deleting that result still makes the earlier input impossible to replay; keeping every such result reproduces the original growth problem.

[WebAssembly](https://www.w3.org/TR/wasm-core/) already permits importing memories, tables, globals and functions from another instance.
This means sharing the same memory object between modules, without requiring threads or `SharedArrayBuffer`.
Wasmtime's [module serialization](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html#method.serialize) saves compiled code, not an instance's current state.
[Temporal's continue-as-new](https://docs.temporal.io/develop/go/workflows/continue-as-new) bounds history by passing explicit current state into a new execution; hd has no equivalent explicit session-state boundary today.

## Recommendation: an initialized base module

Use the first mechanism, with a checkpoint only at a completed input boundary.
Quiescence means no active guest frame, pending root or host operation, and no release work in flight.
The initial proof should reject other states.
The artifact contains input sources, outcomes, interfaces and generic definitions, reachable code, initialized binding values, runtime state, and an identity manifest.
Failed inputs' hidden declarations stay hidden; their earlier mutations remain captured.

Materialize the captured state as base-module data/global initializers, or equivalent validated loader data, without invoking the old input initializers or host effects.
Later input modules import that base; merely retaining one live instance and clearing its journal is insufficient for restoration.

The base owns the allocator, collector and shared function table.
Extensions import exchange memory, heap and any shadow stack, plus the runtime globals and binding storage they reference.
New static data must receive disjoint addresses; new glue, closures and trait dispatch must receive stable table slots.
Today's [emitter](../../crates/hd_wasm_linear/src/module.rs) assigns those positions within each whole program, and the current JS session linker accepts only host-function imports.

Capture includes counted headers, allocator page/free-list state, collector candidate buffers, release state, binding globals and runtime globals.
They must agree with the captured table and code; copying binding values alone is unsound.
Keeping allocator memory avoids reconstructing ownership counts, but also keeps dead pages, as the measurement demonstrates.
Discarding the exchange buffer at a quiescent boundary may reduce bytes; it was not established by this prototype.

Types retain their layout and glue identity for the checkpoint's lifetime.
Appending unrelated declarations must not renumber old tables, descriptors, static addresses or binding slots.
A changed type layout requires replay or an explicit migration mechanism; a raw snapshot cannot reinterpret old references safely.
Toolchain, runtime ABI, profile, grants and base-code identity must be checked before accepting a checkpoint.

## Handles, reload and retained history

A numeric handle is meaningful only with its provider's matching resource table.
An in-process checkpoint can keep that provider context and its open sockets or clients; a fresh host cannot restore those resources from integers.
Keep resource and driver handle tables with that context, including their allocation state.
Initially, checkpoint restore should require the original provider context unless every reachable resource has an approved restore contract.
The probes cover no handles or pending operations, and establish neither cross-process resume nor durable execution for [#18](https://github.com/HerringtonDarkholme/hd-lang/issues/18).

Preserve the documented `:reload` behavior by retaining historical source and host-result evidence outside the active journal, in archived epochs.
Routine inputs start at the checkpoint; a reload changing checkpointed code falls back to historical replay with longest-prefix divergence checks.
This bounds active recording and ordinary replay, **not total stored history**.
Node/native archive storage and the browser's IndexedDB budget therefore need explicit limits and exhaustion behavior.

If the owner instead wants old history physically discarded, the checkpoint becomes a reload boundary.
The implementation must then refuse a changed prefix while preserving the current session, or reset it explicitly; neither follows today's reload rule.
A Stop must also recover the last completed checkpoint and tail, rather than freeze a partially executed input.
No path may repeat old effects live when restoration fails.

## Approval before implementation

The recommendation needs approval for a session-wide base ABI, checkpoint state materialization, provider-context restrictions, and historical-archive storage policy.
Keeping current reload semantics through archives is recommended; discarding that evidence requires a separately approved CLI behavior change.
A bounded implementation proof should restore an initialized base and import a later input on Node, Wasmtime and an actual browser before setting latency or storage budgets.
Only that proof can price complete checkpoint generation, persistence and restoration; this note does not claim those mechanisms already exist.
