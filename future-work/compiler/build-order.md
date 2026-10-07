# New Compiler Design: Build Order

Part of the [compiler design](README.md).

## 9. Build Order

Consistent with [Q7](research.md#build-order),
updated for answer 13: no drive summary in any slice.

| Slice | Delivers | Exit test |
| --- | --- | --- |
| 1. Syntax | `hd_base`, `hd_intern`, `hd_diag`, `hd_syntax`: lexer, layout cursor, parser, green tree, typed views, skim mode and skeletons; `hd parse FILE` | parse-phase portable cases pass; every fixture and std file round-trips byte for byte; skim and full-parse skeletons agree on every file; lexer and parser fuzzed with no panic; `hd_web` builds for `wasm32-unknown-unknown` in CI; lexing and parsing throughput recorded separately |
| 0. Backend spikes (after slice 1, before slice 2; Codex re-review N-R1) | a hand-written emitter over hand-built TIR, no checker: slice 0a, then 0b (§22) | 0a and 0b's exit tests in §22 pass on the pinned wasmtime and on V8 |
| 1b. Formatter (beside slice 2) | `hd_fmt`, `hd fmt` | idempotent on every fixture and std file; `fmt` timing recorded |
| 2. Std interfaces | `hd_project`, `hd_types` (types and rows), `hd_resolve` (interface codec included), `hd_sched` serial, `hd_cache` memory store, `hd_driver`; keys computed from the start | std's folder interfaces build with no diagnostic; name-phase cases pass (`unknown-import`, `private-import`, `re-export-loop`, `folder-cycle`, `private-type-leak`, `orphan-impl`, `nonlocal-impl`); blobs decode to equal interfaces; blob bytes equal across shuffled serial orders; a deep-hash test changes a type reached only through a signature and sees the dependent's key change |
| 2b. `hd doc` (beside slice 3) | `hd_doc` on interfaces | HD_DOC cases pass; `answer-size` budget met |
| 3. Bodies | first a thin vertical slice: functions, integers, data, one generic call and one trait call, from source through TIR, emission and the native disk cache, with private-body, dependency and location-only edits tested. The slice runs on the designed types (`hd_types`, `hd_tir::ir`, `hd_resolve`, `hd_project`, `CacheStore`, `hd_diag`); the walking skeleton's types are deleted when it lands ([reconciliation, item 10](reconciliation.md#design-changes-proposed)). Then `hd_check` proceeds chapter by chapter in spec order: inference, rows, suspension, exhaustiveness, templates, coherence, init order, TIR with its verifier and printer | a source-feature-to-TIR coverage table, one row per expression and statement form of spec chapters 5 to 14 with its tag, verifier rule and emission rule (checking-and-tir.md, Instruction Catalog); type-phase cases pass with a known-failures list; std's bodies check clean; `errors-per-run`, `mistakes` and `diag-location` measured; `pathological` runs within budgets |
| 3b. Fix-its and `hd fix` | re-parse safety, `hd fix` rounds | `fixit-safety` at 100%; fix-it share of `mistakes` measured |
| 4. Cache and threads | disk store, manifest, eviction and `hd cache gc`, the pool scheduler, the opt-in memory cap, the embedded std pack, verify mode | `recheck-precision`, `edit-latency`, `cold-check`, `resources`, `startup`, `determinism` (full matrix), `incremental-soundness`, `cache-contention`, `cache-growth`, `parallel-speedup` with peak memory at 8 threads at most 1.5x that at 1 thread |
| 5. Browser front end | `hd_web` with the stepping scheduler and the JS stores | the playground checks programs in a worker; size measured so the owner can set a budget (answer 4); `long-session` flat in the browser |

D2's slices 6 to 10 follow in §22, refining [Q16](research.md#build-order-1).
Each slice's exit test also includes its rows of §9.2.

A stage that answers "not implemented" during a build stops that build
with an internal `unsupported` diagnostic naming the stage and its first
reason. Only `analyze_package` may count such an answer and continue
([reconciliation, item 11](reconciliation.md#design-changes-proposed)).

**Performance is eyeballed, not gated, during the first implementation
(owner, 2026-10-07).** "Drop the perf gate for now and just go ahead,
impl the architecture, and eyeball the perf." Every throughput, latency
and size target in this file, in §9.2 and in goals.md is reported at
each slice exit, not enforced. When a number is atrociously bad, the
slice stops to decide whether it is an implementation slip (fix the
code) or an architecture issue (reopen the design doc that owns it)
before going on.

### 9.1 Status Of The Pillar 1 And 2 Targets

No target below is established. Each figure in this design is an
estimate with stated assumptions, and some are only plausible for
ordinary inputs (Codex review, P1 to P6). The slice named last measures
it.

| Target | Status | What can break it | Benchmark parameters | Measured in |
| --- | --- | --- | --- | --- |
| `resources`: warm check ≤ 0.2 CPU-s, ≤ 50 MB | plausible, not established. data-structures.md §3.24 estimates structures and binary pages; it is not measured RSS | process loading, directory walks and stats (10,000 tiny files cost 20 to 100 ms in stats alone), cache I/O, allocator slack, thread stacks | empty, 100-file and 10,000-file packages; RSS and CPU separately; cold OS cache reported apart | 4 |
| `edit-latency`: p50 ≤ 50 ms | plausible for small modules: an estimated 6 to 20 ms to output on one core for `ordinary-10k` (cache.md §5.9). Not met on one core for `one-file-10k`: 90 to 260 ms, 25 to 60 ms on 8 | a one-function edit rechecks its whole module (checking-and-tir.md §4.13.1); a solver-heavy body; per-file I/O on the Mac (publish about 340 µs, open 60 to 120 µs), which the last-run record and print-then-publish keep off the output path | module-size distribution published; one-file and many-file packages; one core and 8 cores; time split into startup, check, serialization and publish | 4 |
| `cold-check`: ≤ 1 s | plausible for ordinary code; line count alone does not bound it | wide impl buckets (the overlap check's trie and unification work, counted, not the old pairwise estimate), deep folder chains on the serial interface path, bodies near the fuel limit, shared proof subgoals (frontend lane) | an ordinary benchmark and a separate pathological set, each reporting work counts, not only time | 3 and 4 |
| `startup`: ≤ 20 ms, ≤ 10 MB | feasible only with the lazy command path (commands.md §7.1 step 2) | binary page faults, pool or engine creation, eager std validation, cache scans | warm and cold executable pages on named platforms; resident pages, not file size | 1, then 4 |
| `determinism` | contract fixed for the backend: the wire rule, one key order, print-at-end, per-state operational fields | the frontend's solver memo keys (Codex finding 8, frontend lane) | the matrix of testing-the-compiler.md §8.1, with ID shift, shuffled order and mixed warmth run before threads | 2, then 4 |
| `recheck-precision`: one module per private body edit | holds for an ordinary private function; see design-overview.md §1.4 for what is counted and the exceptions | hidden template helpers and facts (frontend lane, Codex findings 4 and 6); init-order recomputation | the edit vocabulary of testing-the-compiler.md §8.2 | 4 |

### 9.2 Systems Measurements Per Slice

From the [systems review](systems-review.md#measurements-to-add-to-the-slice-exit-tests).
Each row joins its slice's exit test. "Both machines" means the CI
runner and the idle Mac of §22.2.

| Slice | Measurement | Gate or report |
| --- | --- | --- |
| 1 | Body size distribution of std and `ordinary-10k`: bytes and tokens per body, p50, p90, p99, max (the review's numbers are the baseline: mean 240 B and 37 tokens, median 118 B and 19 tokens) | report |
| 1 | Resident pages and wall time of `hd --version` with the release binary, wasmtime linked in | gate (`startup`) |
| 3 | Check throughput in tokens per µs, per body size class, one thread | report; sets the minimum split size (scheduler.md §6.2) |
| 3 | Lock acquisitions and contended waits per run for the interners and the global memo; memo hit cost at 1 and 8 threads; per-worker table hit rates (data-structures.md §3.3) | report; gate the 100 ns hit at 1 thread |
| 3 | Fix-it verification time on a 1,000-line file with 20 fix-its | report |
| 4 | Per-entry publish and read cost on both machines, by entry size (2 KiB, 64 KiB, 2 MB) | report; checks the costs in cache.md §5.9 |
| 4 | Files opened, created and renamed per warm edit, by edit kind (systems-review.md, "Incremental Cost Per Edit") | gate on both machines: proportional to the edit (`io-per-check`) |
| 4 | Modules rechecked per public edit in `ordinary-10k` and in a one-folder variant, and how many early cutoff would reuse (cache.md §5.3.1) | report; decides when early cutoff is enabled |
| 4 | One-core `edit-latency` p50 and p95, next to 8 cores, on `ordinary-10k` and `one-file-10k` | gate on both core counts and both machines |
| 4 | Time from start to the first printed diagnostic, against total time to exit | report |
| 4 | Eviction: shards scanned, entries deleted per run, bytes over cap after a scripted day | gate (`cache-growth`) |
| 5 | Longest stepping-executor step, in ms, on a 1,000- and a 3,000-line program in Chrome; worker restarts during a scripted typing session | gate: no step over 50 ms on the 1,000-line program |
| 6 | `prog_key` hits after a comment edit, a `tests:` edit and a private body edit that keeps the TIR | gate: a hit for all three |
| 8 | `hd test` on `tests-1k` cold, and after an edit to a module that 50 modules use: CPU-s, packs read, Cranelift functions compiled, `cwasm` bytes written, constant globals built per case | report |
| 8 | Concurrent `Precompile` peak RSS | report |
| 10 | GC collections, total pause and max pause per `runtime-suite` case; p99 pause and p99 request latency of `long-run-memory` (slice 7 starts measuring) | report; gate the max pause once a budget is set |

## 22. Back-Half Build Order

Slice 0 runs right after slice 1, before the checker exists (Codex
re-review N-R1). Its two spikes settle the representations that TIR,
metadata reachability and cache keys depend on, so the checker is not
built against a `CallDyn` record that later changes. The other D1
slices then come first. Slice 3's exit already includes TIR with its
verifier and printer.

| Spike | Delivers | Exit test |
| --- | --- | --- |
| 0a. Suspension and size | hand-built TIR for a `main!` that awaits one host timer, returns a data value through Pending and Ready, and is cancelled in a second case; hello world | validates and runs on wasmtime and V8; non-null payloads use the defaultable forms (wasm-layout.md §15.2); refused memory and GC growth both report `heap-exhausted` (runtime-and-host.md §17.8); hello world's bytes per section reported |
| 0b. One `dyn` generic method | the erased ABI of codegen.md §13.5.1 for one method, two impls and two type arguments | the test of codegen.md §13.5.1 passes: packed `T`, reference `T`, a caller-owned `mut List[(T, T?)]` mutated in place, a returned `Buffer[T]` with its identity kept, a generic helper, an escaping `fn(T) -> T`; call and open-instruction costs reported |

| Slice | Delivers | Exit test | Pillar 3 metrics it should start meeting |
| --- | --- | --- | --- |
| 6. Scalars end to end | `hd_mono`, `hd_wasm` for scalars, functions, `println`; `hd_run` with a Node-backed `Engine` for `hd run`, plus `hd build` and `hd FILE.wasm`; the `code` and `link` entries | the scalar cases of `runtime/valid` pass on the Node engine; Wasm bytes deterministic across the matrix ([reconciliation, item 9](reconciliation.md#design-changes-proposed)) | `size-startup-heap` (tiny ≤ 2 KB); `hd` binary size recorded; `check-cost` and `dev-speed` on scalar code |
| 7. Data and std | data, enums, closures, strings, lists, maps, `Option` and `Result` layouts, the exchange buffer for structured values, panics with sites and backtraces, folding | `runtime/valid` and the `runtime/panic` cases chapter by chapter, with a known-failures list | `runtime`, `allocations` (counted loops: 0), `dead-code`, `text-throughput` |
| 8. Suspension, host and tests | state machines, `all!`, `race!`, cancellation, the reactor, grants and startup refusal, resource limits, the test runner with property tests | the suspension and capability cases; the CLI cases of [`cli-cases.tsv`](../../spec/conformance/cli-cases.tsv) | `suspension-overhead`, `host-call-overhead`, `unit-test-perf`, `proptest-perf`, `integration-test-perf`, `test-latency` |
| 9. The browser | the JS glue, the program worker, synchronous mode, the headless-browser adapter | the runtime cases pass on wasmtime and in the browser with one known-failures list | browser size recorded; `allocations` on V8 through the glue |
| 10. The optimized pipeline | the pass manager and the optimized pipeline of [tiering.md](tiering.md): bounded inlining, closure specialization, devirtualization and scalar replacement, which the dev pipeline does not run; the measured engine choices (§18.6, spike T1); spike T2 decides Binaryen | no conformance regression; every runtime case passes on both pipelines with the same output (tiering.md §4.3); Wasm still deterministic; GC collections, total and max pause per `runtime-suite` case, and the p99 pause and request latency of `long-run-memory`, reported against the pause budget once set | `runtime` (≤ 1.5x Node), `allocations` (chains ≤ 1), `serde-throughput`, `long-run-memory` |

**Wasm first (owner, 2026-10-07: "let's first focus on wasm").** The
back half first produces correct, deterministic Wasm GC and runs it on
V8 through Node, the engine the browser and the playground use. Until
the Wasm path passes slice 7's runtime cases:

- until wasmtime is approved, `hd run` runs on V8 through Node via an
  `hd_run::Engine` implementation, and slice 6 exits on that engine;
  there are no Cranelift-level or allocator comparisons, `cwasm`
  entries, precompiling or native code packs
  ([reconciliation, item 9](reconciliation.md#design-changes-proposed));
- Binaryen (T2), test-run laziness (T3), dev inlining (T4) and pass
  fusion (T5) wait for slice 10;
- slice 6's "Cranelift time recorded" and `cwasm` entry, and slice 8's
  Cranelift and `cwasm` counts, are deferred with them.

### 22.1 How The Pillar 3 Targets Are Met

| Metric | Target | How | Status |
| --- | --- | --- | --- |
| `dyn` generic calls | no target (owner: slower `dyn` is accepted) | outlined open instructions through a link-time witness (codegen.md §13.5.1) | measured from slice 0b: cost per call and per open instruction, boxes per crossing; reported, not gated (Codex re-review N-P1) |
| `runtime` | geomean ≤ 1.5x Node; no case > 3x | per-type code with direct calls; counted loops; unboxed layouts; std written for it (streaming JSON) | not established, at risk (Codex review, P7). Decision B, extended to `Result` (owner, 2026-10-07), keeps `Option`, `Result` and boxes free of allocation, and bounded inlining and scalar replacement are first-release. The collector's throughput on allocation-heavy code is unmeasured, and one slow case breaks the 3x guard whatever the geomean. Slice 7 benchmarks the real layouts, allocation-heavy programs and long-lived heaps on the pinned wasmtime; slice 9 reports the browser separately. No V8 figure stands in for wasmtime |
| `allocations` | 0 per counted loop; ≤ 1 per chain | counted loops are a lowering rule (§12.5); chains need inlining and scalar replacement (answered: first release) | loops met by design; chains not established until slice 10 measures them |
| `size-startup-heap` | tiny ≤ 2 KB; ≤ 5 ms; heap ≤ 2x Node | reachability, folding, no runtime blob (§15.6); `InstancePre` and `cwasm` | size plausible, not established: about 800 B is a guess per part, not an accounting (Codex review, P9). Slice 6's first spike emits the real hello world and reports bytes per section. The 5 ms counts instantiation to first output (answer 23.2-7) |
| `host-call-overhead` | scalar ≤ 1 µs; Fs ≤ 10 µs; serde boundary ≤ 5 µs | scalar imports; one exchange buffer and one copy loop; no async machinery in wasmtime calls | met by design; measured in slice 8 |
| `suspension-overhead` | ≤ 100 ns per await; ≥ 1M tasks/s | lazy frames: a ready await is a call and a null test; `all!` allocates nothing of its own when children finish at once | met by design |
| `serde-throughput` | ≥ 0.5x Node | byte arrays, direct monomorphized calls, a streaming `JsonWriter` in std | plausible; std work |
| `text-throughput` | ≥ 0.5x Node | `(array i8)` strings, a growable builder, `Interp` sized once | plausible |
| `dead-code` | ≤ 10 KB per 1,000 lines; no unused std | reachability over TIR; folding; structural types | met by design; measured in slice 7 |
| `long-run-memory` | flat after warm-up; GC pause max and p99, and the request loop's p99 latency, reported | no global caches in std's runtime; wake table entries freed on completion; the initial heap sized from the profile | depends on wasmtime's collector, a copying collector without a young generation, whose pause grows with the live heap (systems review, finding 10: an estimated 25 to 50 ms per collection at 50 MB live). Slice 7 measures pauses per `runtime-suite` case; slice 10 gates once the owner sets a budget |
| `unit-test-perf` | ≤ 1 ms per test; 1,000 in ≤ 1 s | one unit-test program per package with an init export per module, `InstancePre`, pooling, parallel workers (§19.1, §19.3) | plausible for small warm tests, not established: init runs per case, and its cost is the program's (Codex review, P8). Slice 8 measures it with fixed module count and init work |
| `proptest-perf` | ≥ 100k cases/s; shrink ≤ 1 s | cases in one instance; scalar `record`; host-side shrinking; one property per worker (§19.4) | plausible for simple generators, by estimate; slice 8 |
| `integration-test-perf` | ≤ 20 ms setup per program | warm `prog_key` and `cwasm` hits; lazy temp directories | met by design |
| `check-cost` | the optimized pipeline with checks ≤ 1.3x without | the same pipeline and layouts on both sides; only the profile's checks differ | at risk on `i64` multiplication heavy code |
| `dev-speed` | dev pipeline ≤ 4x optimized in one profile, geomean; no case > 10x | the same layouts in both pipelines; the dev pipeline's Cranelift setting chosen by spike T1 against this guard (§18.6) | plausible for the geomean; at risk on iterator chains and map lookups, which tiering.md §3.5 estimates at 3 to 6x; measured from slice 10 |
| `test-latency` | ≤ 300 ms | one module rechecked, code keys reused, Cranelift's per-function cache | plausible by estimate, not established; slice 8 |

### 22.2 Engine Pin And Benchmark Matrix

Codex re-review question 5 and N-P1. No target in §9.1 or §22.1 is
established until this matrix measures it.

**The engine pin.** The first release pins one wasmtime minor line in
`Cargo.lock`: the newest stable release when slice 0 starts, which on
2026-10-07 is the 49 line by wasmtime's monthly release cadence (the
documentation's main branch reports `51.0.0-dev`). Slice 0 records the exact patch release. The pin moves
deliberately, at most once a quarter, in a change of its own that
reruns slice 0's spikes, the engine-limit tests (runtime-and-host.md
§17.8) and the `cwasm` compatibility test (engines-and-test-runner.md
§18.2). The browser side tracks Chrome stable, with its version
recorded in each report.

**Machines.** Two, fixed for the life of the first release:

| Machine | Use |
| --- | --- |
| Linux x86-64 CI runner, one fixed 8-vCPU instance type, local SSD | every gated number; CI history |
| Apple Silicon Mac (the owner's class of machine), measured idle | a second report; also a gate for the I/O-bound targets `edit-latency`, `io-per-check` and `test-latency` |

The Mac gates those three because its per-file costs are 5 to 20 times a
Linux runner's (systems review: publish about 340 µs, open 60 to 120 µs,
under load), and the owner's agents run there. A design that passes on
Linux can miss `edit-latency` on the Mac by its I/O alone (systems
review, open question 4; the owner accepted the recommendation).

**Fixtures.** Each fixture is committed and versioned, so a number
names its input:

| Fixture | Shape | Targets it measures |
| --- | --- | --- |
| `empty` | an empty package; `hd --version` | `startup` |
| `ordinary-10k` | 10,000 lines in 100 files over 12 folders, std-heavy, from the conformance corpus | `cold-check`, `resources`, `edit-latency`, `recheck-precision` |
| `one-file-10k` | the same code in one module | `edit-latency` (the large-module case) |
| `tiny-files` | 10,000 one-function files | `resources` (stat and discovery cost) |
| `pathological` | wide impl buckets, deep folder chains, bodies near fuel, shared proof DAGs | work counts within budgets; not timed against a target |
| `runtime-suite` | the existing runtime benchmarks, plus packed `List[i32]` and list-of-tuple loops, a 200 MB live heap, allocation-heavy trees, and erased generic container mutation through `dyn` | `runtime`, `allocations`, `long-run-memory`, `dyn` generic calls |
| `tests-1k` | 1,000 small unit tests in 100 modules | `unit-test-perf`, `test-latency` |
| `hello` | hello world, release | `size-startup-heap` |

**States and statistics.** Each timed run reports cold and warm OS page
cache separately, cold and warm `hd` cache, the thread count, p50 and
p95 over 20 runs after 3 warm-ups, CPU time and peak RSS. A gate uses
the CI machine's warm-page p50, unless the target names a percentile.

**Gates.** Slice 0 gates `size-startup-heap` and reports the `dyn`
costs. Slice 4 gates the pillar 1 and 2 targets. Slices 7, 8 and 10 gate
the pillar 3 targets they list in §22. A target that fails its gate is
reported to the owner with its measurement; it is not quietly relaxed.
