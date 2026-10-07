# New Compiler Design: Build Order

Part of the [compiler design](../README.md).

## 9. Build Order

Consistent with [Q7](research.md#build-order),
updated for answer 13: no drive summary in any slice.

| Slice | Delivers | Exit test |
| --- | --- | --- |
| 1. Syntax | `hd_base`, `hd_intern`, `hd_diag`, `hd_syntax`: lexer, layout cursor, parser, green tree, typed views, skim mode and skeletons; `hd parse FILE` | parse-phase portable cases pass; every fixture and std file round-trips byte for byte; skim and full-parse skeletons agree on every file; lexer and parser fuzzed with no panic; `hd_web` builds for `wasm32-unknown-unknown` in CI; lexing and parsing throughput recorded separately |
| 1b. Formatter (beside slice 2) | `hd_fmt`, `hd fmt` | idempotent on every fixture and std file; `fmt` timing recorded |
| 2. Std interfaces | `hd_project`, `hd_iface`, `hd_types` (types and rows), `hd_resolve`, `hd_sched` serial, `hd_cache` memory store, `hd_driver`; keys computed from the start | std's folder interfaces build with no diagnostic; name-phase cases pass (`unknown-import`, `private-import`, `re-export-loop`, `folder-cycle`, `private-type-leak`, `orphan-impl`, `nonlocal-impl`); blobs decode to equal interfaces; blob bytes equal across shuffled serial orders; a deep-hash test changes a type reached only through a signature and sees the dependent's key change |
| 2b. `hd doc` (beside slice 3) | `hd_doc` on interfaces | HD_DOC cases pass; `answer-size` budget met |
| 3. Bodies | `hd_check` chapter by chapter in spec order: inference, rows, suspension, GADTs, exhaustiveness, templates, coherence, init order, TIR with its verifier and printer | type-phase cases pass with a known-failures list; std's bodies check clean; `errors-per-run`, `mistakes` and `diag-location` measured; `pathological` runs within budgets |
| 3b. Fix-its and `hd fix` | re-parse safety, `hd fix` rounds | `fixit-safety` at 100%; fix-it share of `mistakes` measured |
| 4. Cache and threads | disk store, manifest, eviction and `hd cache gc`, the pool scheduler, the opt-in memory cap, the embedded std pack, verify mode | `recheck-precision`, `edit-latency`, `cold-check`, `resources`, `startup`, `determinism` (full matrix), `incremental-soundness`, `cache-contention`, `cache-growth`, `parallel-speedup` with peak memory at 8 threads at most 1.5x that at 1 thread |
| 5. Browser front end | `hd_web` with the stepping scheduler and the JS stores | the playground checks programs in a worker; size measured so the owner can set a budget (answer 4); `long-session` flat in the browser |

D2's slices 6 to 10 follow in §22, refining [Q16](research.md#build-order-1).

## 22. Back-Half Build Order

D1's slices 1 to 5 come first. Slice 3's exit already includes TIR with
its verifier and printer.

| Slice | Delivers | Exit test | Pillar 3 metrics it should start meeting |
| --- | --- | --- | --- |
| 6. Scalars end to end | `hd_mono`, `hd_wasm` for scalars, functions, `println`; `hd_run` and `hd_run_wasmtime` with `hd run`, `hd build`, `hd FILE.wasm`; the `code`, `link`, `cwasm` entries | the scalar cases of `runtime/valid` pass; Wasm bytes deterministic across the matrix | `size-startup-heap` (tiny ≤ 2 KB); `hd` binary size and Cranelift time recorded; `release-check-cost` on scalar code |
| 7. Data and std | data, enums, closures, strings, lists, maps, `Option` and `Result` layouts, the exchange buffer for structured values, panics with sites and backtraces, folding | `runtime/valid` and the `runtime/panic` cases chapter by chapter, with a known-failures list | `runtime`, `allocations` (counted loops: 0), `dead-code`, `text-throughput` |
| 8. Suspension, host and tests | state machines, `all!`, `race!`, cancellation, the reactor, grants and startup refusal, resource limits, the test runner with property tests | the suspension and capability cases; the CLI cases of [`cli-cases.tsv`](../../spec/conformance/cli-cases.tsv) | `suspension-overhead`, `host-call-overhead`, `unit-test-perf`, `proptest-perf`, `integration-test-perf`, `test-latency` |
| 9. The browser | the JS glue, the program worker, synchronous mode, the headless-browser adapter | the runtime cases pass on wasmtime and in the browser with one known-failures list | browser size recorded; `allocations` on V8 through the glue |
| 10. Optimization within the shared tier | bounded inlining and scalar replacement if question 1 says so; the measured engine choices (§18.6) | no conformance regression; Wasm still deterministic | `runtime` (≤ 1.5x Node), `allocations` (chains ≤ 1), `serde-throughput`, `long-run-memory` |

### 22.1 How The Pillar 3 Targets Are Met

| Metric | Target | How | Status |
| --- | --- | --- | --- |
| `runtime` | geomean ≤ 1.5x Node; no case > 3x | per-type code with direct calls; counted loops; unboxed layouts; std written for it (streaming JSON) | at risk: wasmtime's copying GC is slower than V8's on allocation-heavy cases, and general inlining is open question 1 |
| `allocations` | 0 per counted loop; ≤ 1 per chain | counted loops are a lowering rule (§12.5); chains need inlining and scalar replacement | loops met by design; chains at risk until question 1 |
| `size-startup-heap` | tiny ≤ 2 KB; ≤ 5 ms; heap ≤ 2x Node | reachability, folding, no runtime blob (§15.6); `InstancePre` and `cwasm` | size met by estimate (about 800 B); 5 ms at risk if it counts process start (§23.2) |
| `host-call-overhead` | scalar ≤ 1 µs; Fs ≤ 10 µs; serde boundary ≤ 5 µs | scalar imports; one exchange buffer and one copy loop; no async machinery in wasmtime calls | met by design; measured in slice 8 |
| `suspension-overhead` | ≤ 100 ns per await; ≥ 1M tasks/s | lazy frames: a ready await is a call and a null test; `all!` allocates nothing of its own when children finish at once | met by design |
| `serde-throughput` | ≥ 0.5x Node | byte arrays, direct monomorphized calls, a streaming `JsonWriter` in std | plausible; std work |
| `text-throughput` | ≥ 0.5x Node | `(array i8)` strings, a growable builder, `Interp` sized once | plausible |
| `dead-code` | ≤ 10 KB per 1,000 lines; no unused std | reachability over TIR; folding; structural types | met by design; measured in slice 7 |
| `long-run-memory` | flat after warm-up | no global caches in std's runtime; wake table entries freed on completion | depends on wasmtime's collector; measured in slice 10 |
| `unit-test-perf` | ≤ 1 ms per test; 1,000 in ≤ 1 s | one module per program, `InstancePre`, pooling, parallel workers (§19.3) | met by estimate |
| `proptest-perf` | ≥ 100k cases/s; shrink ≤ 1 s | cases in one instance; scalar `record`; host-side shrinking | met by estimate for simple generators |
| `integration-test-perf` | ≤ 20 ms setup per program | warm `prog_key` and `cwasm` hits; lazy temp directories | met by design |
| `release-check-cost` | debug ≤ 1.3x release | one emission for both tiers; only checks differ | at risk on `i64` multiplication heavy code and on the Cranelift level (§18.6) |
| `test-latency` | ≤ 300 ms | one module rechecked, code keys reused, Cranelift's per-function cache | met by estimate |
