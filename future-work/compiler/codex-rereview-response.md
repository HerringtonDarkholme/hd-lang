# New Compiler Design: Response To The Codex Re-Review

Part of the [compiler design](README.md).

Status: Response to the Codex re-review through 54f249b7, 2026-10-07. Backend rows; frontend rows follow.

The re-review is not in the repository. Each claim below was checked
against the design files on main at 128bc809 (S1c landed) and the spec,
not against the reviewed commit.

**Verdicts.** `accepted-fixed`: the claim holds and the design is fixed
where the row says. `partly`: part of the claim holds, and that part is
fixed. `rejected`: the claim does not hold. `already-fixed`: the current
files already fix it. `needs-owner`: the fix needs an owner decision,
listed under Needs Owner. `frontend-lane`: verified and fixed by the
frontend agent.

## Backend Lane

### Ten Most Important Findings

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| N2 | accepted-fixed | [data-structures.md §3.21](data-structures.md#321-the-schedulers-task-graph), [scheduler.md §6.1](scheduler.md#61-tasks) | True: "a task marks itself `Done` and then walks the list" while a pusher that saw `Done` also satisfied its edge. Registration and completion now take the producer's successor lock, the list is detached once, and a creation guard keeps a task from going Ready while its edges are added. |
| N3 | accepted-fixed | [wasm-layout.md §15.2](wasm-layout.md#152-values), [suspension.md §14.1](suspension.md#141-functions-per-suspending-body), [§14.3](suspension.md#143-lazy-frame-materialization) | True: "the first result is a zero value", `zeros` and `default fields` have no inhabitant for a non-null reference. Each layout now has a defaultable form; inactive payloads, Pending results, resume arguments and frame fields use it, and reads narrow after the tag or state test. |
| N4 | accepted-fixed | [codegen.md §13.7](codegen.md#137-merging-byte-identical-functions) | True: the class key was "body bytes" without `CodeEntry.sig`. The fold key now starts with the canonical signature and holds locals, exact relocation targets and site records. |
| N5 | accepted-fixed | [codegen.md §13.5.1](codegen.md#1351-the-erased-method-abi-codex-re-review-n5), §13.2 step 5, §13.8 `Field` relocation; [checking-and-tir.md](checking-and-tir.md#instruction-catalog) `CallDyn`; [build-order.md §22](build-order.md#22-back-half-build-order) spike 0b | True: "read and written in place through the witness" had no record, ABI or construction rule. An open value is now the caller's own representation as `anyref`; every open instruction is outlined into a thunk built at the caller's types; witnesses are link-time constant globals per method and type arguments. Erasure is one level deep, so containers stay in place and `Buffer[T]` is built by a monomorphic thunk. |
| N6 | accepted-fixed; domain needs-owner | [codegen.md §12.3](codegen.md#123-facts-defaults-derives-and-tests) items 5 to 8, [data-structures.md §3.9.2](data-structures.md#392-the-internpool), [wasm-layout.md §15.4](wasm-layout.md#154-globals-and-module-initialization) | True: "converted to a global pool constant" with hash-consed `Aggregate` rows merges two equal data objects. Fact values are now a graph of numbered allocations over interned identity-free leaves; equal records never merge. Which values may appear is open question 23.1-10. |
| N9 | accepted-fixed | [cache.md §5.3](cache.md#53-key-composition), [data-structures.md §3.7](data-structures.md#37-spans-and-files), [testing-the-compiler.md §8.2](testing-the-compiler.md#82-incremental-soundness) | True: "offsets from the start of that declaration" move with interior whitespace under an unchanged key. Spans on disk are now token anchors (token index, count, offsets inside a token), resolved by re-lexing one declaration; a new edit row tests interior edits. |

### Other Findings

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| N-A2 | accepted-fixed | [codegen.md §12.3](codegen.md#123-facts-defaults-derives-and-tests) item 9, [§12.6](codegen.md#126-tiers-and-optimizations) | Partly hypothetical, since no rule specialized on a fact, but nothing forbade it. Code now reads a fact only through its global; folding and inlining stop there, so a changed fact only relinks. |
| N-D2 | accepted-fixed | [codegen.md §13.10](codegen.md#1310-the-link-step) step 2, [engines-and-test-runner.md §18.2](engines-and-test-runner.md#182-compile-caches) | True: insertion renumbers later functions. The stability claim is removed; slice 6 measures cache hits after insertions. |
| N-P1 | partly | [build-order.md §22.1](build-order.md#221-how-the-pillar-3-targets-are-met), [§22.2](build-order.md#222-engine-pin-and-benchmark-matrix) | True that nothing is measured. The design cannot measure; it now names fixtures, machines, states, percentiles and gates, adds packed arrays, a large live heap and erased container mutation to the runtime suite, and reports the witness path's cost from spike 0b. |
| N-I1 | accepted-fixed | [data-structures.md §3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation) | True: "Slots live on the scratch stack" while the block's scratch is flushed and truncated before the join fills its slot. A slot is now a durable `Slot` instruction, filled in place or as a `Splice` of several instructions. A fill made inside a trial waits until no trial is open (follow-up item 5 below). |
| N-I2 | accepted-fixed | [codegen.md §12.2](codegen.md#122-lowering-rules), [§12.4](codegen.md#124-rows-and-providers); [checking-and-tir.md](checking-and-tir.md#instruction-catalog) coercion table | True: TIR said "an adapter", codegen said "nothing", and `Supertrait` had no emission row. One callable ABI (every closure takes a context) makes row subsumption a no-op; `Supertrait` emits one `struct.get`. |
| N-I3 | accepted-fixed | [data-structures.md §3.3](data-structures.md#33-interners), [§3.17](data-structures.md#317-impl-tables); [checking-and-tir.md](checking-and-tir.md#instruction-catalog) choice word | True: 29-bit and 30-bit payloads cannot hold IDs from owners 16 and 32. Head keys and choices now carry a full 32-bit payload, with assertions and an owner-63 determinism run. |
| N-B1 | accepted-fixed | [wasm-layout.md §15.3](wasm-layout.md#153-the-type-section) | True: "groups in order of their canonical descriptor bytes" is not a dependency order. Groups follow a topological order of the SCC graph with content ties; members are ordered by nominal key; recursive-shape equivalence is deferred. |
| N-B2 | accepted-fixed | [wasm-layout.md §15.2](wasm-layout.md#152-values), representation equivalence | True of research-enum-values.md §2.2's "any equal value". The design rule now compares reference components by identity, never by `Eq`; constants and the fact graph use it. The research record is left as written. |
| N-B3 | accepted-fixed | [runtime-and-host.md §17.2](runtime-and-host.md#172-import-shapes), [§17.3](runtime-and-host.md#173-the-exchange-buffer), [suspension.md §14.4](suspension.md#144-wakers-and-the-entry-driver) | True: `-1` carried no length, and handles had no lifecycle. `.start` returns status and length or handle; handles are generation-tagged with Pending, Completed and Cancelled states; the writer grows the buffer; `hd.poll` drains internal wakes; `hd.wake(h)` prose is now `hd.wake(n)`. |
| N-B4 | accepted-fixed | [runtime-and-host.md §17.6](runtime-and-host.md#176-host-calls-in-the-browser) | True: a settle could start a poll while JSPI held a suspended one. The glue keeps Idle, Running and Suspended states and starts an export only from Idle. |
| N-B5 | accepted-fixed | [runtime-and-host.md §17.6](runtime-and-host.md#176-host-calls-in-the-browser), [scheduler.md §6.2](scheduler.md#62-executors) | True: the fallback switched the whole program, and a step cannot interrupt a body. The fallback is now scoped to `block_on` through an exported `hd.blocking` counter; the playground restarts the compiler worker after 200 ms instead of promising an intra-body yield. |
| N-B6 | accepted-fixed | [suspension.md §14.3](suspension.md#143-lazy-frame-materialization) | True: reused frames kept dead references. Each save sequence nulls the fields dead at that point; completion and cancellation clear the rest; a heap test checks it. |
| N-B7 | accepted-fixed | [runtime-and-host.md §17.8](runtime-and-host.md#178-resource-limits), [engines-and-test-runner.md §18.2](engines-and-test-runner.md#182-compile-caches), [build-order.md §22.2](build-order.md#222-engine-pin-and-benchmark-matrix) | True: refused memory growth returns `-1` rather than trapping, host buffers were uncapped, and a checksum does not make native code trusted. One aggregate heap budget with both failure paths, a separate host-buffer budget, a pinned engine, and `cwasm` stored raw in a private owner-only directory, never shared. |
| N-B8 | accepted-fixed | [codegen.md §13.10](codegen.md#1310-the-link-step) step 8 | True: release LEB shrinking moved sites. The encoder builds an offset map and rewrites sites, lines and the source map through it. |
| N-C1 | partly | [codegen.md §13.5](codegen.md#135-dictionaries-trait-values-and-gadt-evidence), [wasm-layout.md §15.1–15.2](wasm-layout.md#151-layout-classes), [checking-and-tir.md §4.13.6](checking-and-tir.md#4136-gadt-refinement) and catalog, [design-overview.md](design-overview.md), [build-order.md §9](build-order.md#9-build-order) | True. Backend files now follow S1c and the GADT removal: every enum is a value layout, `is` on enums is a compile error, the `Evidence` callee, `Refine` and variant evidence are retired. Section headings are kept so existing links work. The per-trait safety gate (trait-solver.md §9) and `is` desugaring (type-checking.md §2.2, §6) are frontend lane; research-enum-values.md stays a dated record. |
| N-C2 | accepted-fixed; needs-owner | [codegen.md §12.3](codegen.md#123-facts-defaults-derives-and-tests), [open-questions.md §23.1](open-questions.md#231-open-questions-for-the-owner) items 9 and 10 | True: demand-driven evaluation and the value domain were design choices without a spec contract. Fact-to-fact reads, cycles and deterministic budgets are now specified; the two language questions go to the owner. |
| N-C3 | accepted-fixed | [testing-the-compiler.md §8.2](testing-the-compiler.md#82-incremental-soundness), [build-order.md §9.1](build-order.md#91-status-of-the-pillar-1-and-2-targets), [cache.md §5.4](cache.md#54-entry-format-and-atomic-publish), [open-questions.md](open-questions.md) | True. Comment and blank-line edits now expect one module's recheck; the cold-check row counts trie work, not pairs; the `cache-contention` answer (an orchestrator's call) now bounds warm runs only, since misses are not coordinated. |
| N-S1 | accepted-fixed | as N2 | Accepted: the lock-free protocol had no measured need. A loom test covers the locked one. |
| N-S2 | accepted-fixed | as N6 | Accepted: pool interning is for identity-free leaves; allocations are graph records. |
| N-S3 | accepted-fixed | [codegen.md §13.7](codegen.md#137-merging-byte-identical-functions), [wasm-layout.md §15.3](wasm-layout.md#153-the-type-section) | Accepted with one keep: folding repeats bottom-up, resolving targets to already-folded representatives, so `List[u32].push` and `List[i32].push` still fold. That merges only proven-equal bodies; optimistic recursive refinement and recursive type-shape equivalence wait for a size measurement. |
| N-R1 | partly | [build-order.md §9](build-order.md#9-build-order), [§22](build-order.md#22-back-half-build-order) | Accepted for the backend gates: spikes 0a (suspension, payloads, limits, size) and 0b (the `dyn` witness ABI) run after slice 1, before the checker, and slice 3 starts with a thin vertical slice through the disk cache. The solver-only adversarial fixtures are frontend lane. Order stays sequential, one agent at a time. |

### Questions For The Authors

| Question | Verdict | Answer |
| --- | --- | --- |
| 1. Witnesses for `Buffer[T]` | accepted-fixed | Concrete composite-operation thunks. Every open instruction of an erased body is outlined into a thunk emitted at the caller's types, and a link-time constant witness per method and type arguments holds them ([codegen.md §13.5.1](codegen.md#1351-the-erased-method-abi-codex-re-review-n5)). Runtime representation factories cannot work on Wasm GC, which cannot create a struct type at run time. |
| 3. An unread fact that panics | needs-owner | Recommendation: demand-driven, with a spec note ([open-questions.md 23.1-9](open-questions.md#231-open-questions-for-the-owner)). |
| 4. Closures and cycles in compile-time values | needs-owner | Recommendation: identity-free values plus an acyclic graph of fresh allocations; reject captured closures, suspensions, handles and cycles ([open-questions.md 23.1-10](open-questions.md#231-open-questions-for-the-owner)). |
| 5. Engine pin and benchmark matrix | accepted-fixed | The newest stable wasmtime line when slice 0 starts (the 49 line on 2026-10-07), moved at most quarterly; two fixed machines, eight fixtures, p50 and p95 ([build-order.md §22.2](build-order.md#222-engine-pin-and-benchmark-matrix)). |

**Rejected:** none. Every backend finding held against the current
files, at least in part.

**For the frontend lane, from these fixes.** Codegen no longer reads
`CallDyn`'s evidence operands or the shape's `dyn_bounds`, so both can
go (codegen.md §13.5.1). Durable slots (data-structures.md §3.9.5)
give N10's deferred fills a stable target. Token anchors assume the API hash
covers each kept declaration's tokens, which N-A3 settles. The link to
codegen.md §13.5 in trait-solver.md keeps working.

### Frontend Follow-ups Applied (2026-10-07)

The items of
[codex-rereview-response-frontend.md](codex-rereview-response-frontend.md#changes-for-the-backend-lane)
under "Changes for the backend lane".

| Item | File | Summary |
| --- | --- | --- |
| 1 (N-A1) | [cache.md §5.3](cache.md#53-key-composition) | `argc` and `arg_impls_closure_hash` are gone; the `check`, `hdr` and `test` keys list the transitive dependency closure, which the deep hashes make sound. |
| 2 (N-D1) | [cache.md §5.3](cache.md#53-key-composition) | `coh_key`'s head hash is `H(canonical head, rank)`, with rank `(package, module path, item index)`. |
| 3 (N7) | [codegen.md §13.2](codegen.md#132-collection) | `select` is a head match plus the plan's `Bind` steps, returns every impl argument, and keeps a selection table apart from the proof memo. |
| 4 (N8) | [checking-and-tir.md §4.13.4](checking-and-tir.md#4134-rows) | `CallerRow` gains `Closure(sub_body)`; a closure's row belongs to its function type, and no fact reaches its creator. |
| 5 (N10, N-I1) | [data-structures.md §3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation), §3.19 | An older slot's fill waits in `deferred_fills` until no trial is open; the durable `Slot` keeps its identity; the fill log is gone. |
| 6 (N1) | [scheduler.md §6.1](scheduler.md#61-tasks), [design-overview.md §1.3](design-overview.md#13-the-task-graph) | One interned `ImplUniverseId` per solving context, computed after M1 builds the closure bit sets; it adds no edge. |
| 7 (GADTs) | [data-structures.md §3.4](data-structures.md#34-types), [checking-and-tir.md §4.13.6](checking-and-tir.md#4136-gadt-refinement), [codegen.md §13.5](codegen.md#135-dictionaries-trait-values-and-gadt-evidence) | `Rigid`, `HAS_RIGID`, `Refine`, the `Evidence` callee and `NewVariant` evidence are removed; linked headings stay with a one-line body. |
| 8 | [testing-the-compiler.md §8](testing-the-compiler.md#8-determinism-and-soundness-tests) | The universe A/B case in both orders, template layout edits, swapped overlapping impls and the trial context-growth cases. |
| 9 | [README.md](README.md) | Already lists the frontend response file; verified. |

Codegen also no longer claims to read `CallDyn`'s evidence operands or
the vtable shape's `dyn_bounds` (codegen.md §13.2 step 5, §13.5.1).

## Frontend Lane

The frontend rows (N1, N7, N8, N10, N-A1, N-A3, N-D1, N-T1 to N-T7 and
question 2) are in
[codex-rereview-response-frontend.md](codex-rereview-response-frontend.md).

## Needs Owner

1. **Must an attached but unread fact that panics fail the build?**
   (Question 3, N-C2.) **Recommendation:** no. Evaluate facts on
   demand, only those a built program reads, and add a spec note that an
   unread fact's panic is not reported. It keeps `hd check` per-module
   and costs nothing for unused facts. The cost is that adding a read
   can surface an old panic, which is rare and contrived. Needs a spec
   change, so owner approval.
2. **What may a compile-time value hold?** (Question 4, N6, N-C2.)
   **Recommendation:** identity-free values (primitives, strings, enums,
   tuples, capture-free function values) plus an acyclic graph of data,
   list, map and array objects allocated by that evaluation, with their
   sharing and distinctness kept. A closure with captures, a suspension,
   a handle or a cycle is `fact-evaluation-failed`. Constant expressions
   cannot rebuild those, and no known template needs one. This defines
   "compile-time value" in
   [`annot.metadata.any-value`](../../spec/lang/14-annotations.md#r-annot.metadata.any-value),
   so it needs owner approval.
