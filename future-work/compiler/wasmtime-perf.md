# Wasmtime Performance Notes

## Conformance on wasmtime

M28 measurement (2026-10-11, current main): where driver-conformance time
goes with `HD_CONFORMANCE_ENGINE=wasmtime-only` against Node, and why.
No code was changed for this note.

### Method and conditions

- Machine: darwin, 14 CPUs; suite runs with 8 workers (`workers()` is
  `min(parallelism, 8)`).
- Dev profile throughout (`cargo test --features wasmtime`), system load
  roughly 12-24 during measurement, so absolute numbers are approximate;
  ratios and code-structure findings are load-independent. All
  engine-per-run figures below are min-of-5.
- The runner already separates the walls: `report_runtime` prints the
  selected-suite wall (including fixture check+build) and the aggregate
  execution walls per engine (sums across workers), and every figure below
  comes from those lines plus per-program timing loops with the dev `hd`
  (`--engine wasmtime` vs `node compiler/host/run.mjs`).
- Caveat on the quoted 110 s: it does not reproduce on current main. A
  full wasmtime-only suite run measured 12.5 s wall in dev; the Node-only
  suite is of the same order. The 110 s figure predates the M22 native
  test engine (pooled, shared `TestEngine`) or bundles binary build time
  and a loaded machine. What follows is where time goes *now*.

### Headline numbers (dev)

- Full suite, Compare mode (every runtime case on both engines): suite
  wall 26.0 s, 0 output/status differences.
- Full suite, wasmtime-only: suite wall 12.5 s.
- Aggregates over 992 runs (155 entry runs + 837 test-module
  invocations), Compare mode:
  - lang (794 runtime cases): Node 66.06 s, wasmtime 11.75 s.
  - std (240 runtime cases): Node 34.41 s, wasmtime 23.13 s.
  - Per run, averaged: Node ~101 ms (subprocess spawn dominates),
    wasmtime ~35 ms (includes real test-body execution).
- Entry runs through the fresh-engine path (`run_captured`): ~5 ms
  fixed pipeline per run in-suite (12 MB-stack thread spawn +
  `Engine::new` + `Module::new` + instantiate; a 1 KB trap-fast module
  end-to-end). CLI-measured hot-cache `hd run`: 15 ms wasmtime-engine vs
  52 ms node-engine on the same package (hd startup ~10 ms, Node
  subprocess ~35-40 ms).
- `Module::new` (Cranelift, OptLevel::None) is ~2-4 ms of that ~5 ms for
  0.7-18 KB modules: CLI totals stay flat (22-30 ms) across a 24x size
  range, so compilation, not size, dominates the fixed cost.
- Guest execution with unoptimized codegen can dominate heavy fixtures:
  the 9.4 KB sort takes ~100 ms under wasmtime against ~22 ms under V8
  (~4-5x), i.e. most of its 123 ms CLI total is execution, not compile.

### The four questions

1. **Cranelift per fixture vs execution.** For typical small fixtures
   (the common case: asserts, panics, short valid programs), ~2-4 ms
   Cranelift against <1 ms execution: compilation is most of the ~5 ms
   fixed per-entry cost. Across the suite this is roughly 5-8 s of
   Cranelift inside the 12.5 s wall; the rest is fixture check+build
   (shared with Node) plus real test execution. Heavy fixtures flip the
   ratio (sort: ~100 ms exec vs ~3 ms compile).
2. **Same module compiled more than once.** Yes. `Engine::load`
   (`crates/hd_run_wasmtime/src/lib.rs`) ignores its `CacheStore`
   argument and calls `wasmtime::Module::new` on every call, and
   `TestEngine::compile` (`test_engine.rs`) likewise has no bytes-keyed
   cache (`prepare` caches only `InstancePre` per module/kind, i.e.
   instantiation, not compilation). Concrete repeats: every
   `pending-first-poll` fixture (10+ cases) runs *identical bytes*
   through `run_captured` twice (two `Module::new`); test-harness cases
   additionally compile each fixture twice in two forms (test wasm via
   `TestEngine::compile`, program wasm via `run_captured`).
3. **Engine/Module reuse.** Entry path (`run_program` →
   `run_captured_with_profile` → `run_profile_with_guards`): neither is
   reused — fresh thread, fresh `wasmtime::Engine::new`, fresh
   `Module::new`, fresh `Linker`/`Store` per case (155 entry runs).
   Test path (`test_stdout` → `native_test_results`): the engine IS
   reused (process-wide `OnceLock<TestEngine>` with the pooling
   allocator), but each of the 837 test modules is still compiled
   individually.
4. **Compilation cache.** Off. `wasmtime_config` never enables wasmtime's
   cache (no `cache_config_load` anywhere under `compiler/crates/`),
   and the default is disabled — so every suite run recompiles all ~1000
   modules in process, and nothing persists across runs.

### Top three causes (wasmtime-specific time)

1. **No compiled-module reuse on either path** (`lib.rs` `Engine::load`,
   `test_engine.rs` `TestEngine::compile`). Every case pays a full
   Cranelift compile plus, on the entry path, a full engine/Linker/Store
   setup. Suggested fix: share one `Engine` per worker and hash-cons
   compiled `Module`s by content hash (the `CacheStore` parameter
   already exists for this). Expected gain: removes the ~2-4 ms
   Cranelift plus ~1-2 ms setup from most of the ~1000 compilations;
   required for cause 2 to pay off in process.
2. **Wasmtime's on-disk compilation cache is off.** Suggested fix:
   enable it (`cache_config_load`) for suite runs. Expected gain: repeat
   runs skip Cranelift almost entirely — the ~5-8 s compile share of the
   12.5 s wall drops toward zero on warm cache.
3. **Release runs still use unoptimized guests.** The runner uses
   `EngineConfig::default()` (= `OptLevel::None`) everywhere;
   `EngineConfig::pipeline(release)` exists but nothing in the suite
   path calls it. Suggested fix: use `pipeline(release)` for release
   suite runs. Expected gain: 2-5x guest execution on heavy fixtures
   (sort-like: ~100 ms toward ~22 ms); slightly slower Cranelift per
   compile, offset by causes 1-2.

Context, not a cause: Node pays ~100 ms per run in process spawning
(100.5 s aggregate), so wasmtime's in-process model is already
structurally cheaper per run; the remaining wasmtime-specific time is
almost entirely recompilation (causes 1-2) plus unoptimized guest exec
(cause 3). The suite wall itself is dominated by fixture check+build,
which is engine-independent.
