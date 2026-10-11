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

## Cranelift optimization levels

M32 measurement (2026-10-11): `OptLevel::None` against `Speed` and
`SpeedAndSize` for guest code, compile and execute split. Nothing was
shipped: no setting wins everywhere (details below), so the engine
config is unchanged and the temporary measurement shim was reverted.

### Method and conditions

- Same machine/profile as above (dev `hd`, 8 suite workers, ambient
  load; all figures min-of-5, so they are best-case).
- Micro table: each `test/perf/micro/*.hd` built once to fixed bytes,
  then one compilation plus min-of-7 executions per level in-process
  (fresh process per level, so the M29 cache never hides a compile).
- `hd run` walls: hot-cache `hd run --engine wasmtime` min-of-5 per
  case/level (includes hd startup plus a cache-hit check, constant
  across levels).
- Suite walls: wasmtime-only conformance per level (the runner always
  uses `EngineConfig::default()`, temporarily overridden per run).
- Verdicts were identical at all three levels (lang 763/11/20, std
  235/3/2, 0 differences), so optimization is behavior-preserving here.

### Micro cases: compile and execute (ms)

| case | compile None | compile Speed | compile S&S | exec None | exec Speed | exec S&S |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| fib | 2.27 | 2.56 | 2.87 | 4.44 | 4.46 | 4.48 |
| sum | 1.96 | 2.13 | 2.02 | 14.90 | 12.46 | 12.60 |
| string-build | 2.65 | 3.24 | 3.17 | 14.37 | 13.17 | 13.11 |
| map | 2.71 | 3.07 | 3.39 | 6.27 | 4.53 | 4.58 |
| sort | 4.38 | 4.74 | 4.82 | 85.23 | 69.99 | 70.23 |

None compiles fastest everywhere (Speed costs ~0.2-0.6 ms, SpeedAndSize
about the same again). Speed executes fastest-or-tied everywhere
(fib ties at ~4.5 ms of call overhead; sum 16% faster, map 28%, sort
18%); SpeedAndSize ties Speed on all five (no size pressure at these
sizes).

### `hd run` walls (ms, min-of-5, hot cache)

| case | None | Speed | S&S |
| --- | ---: | ---: | ---: |
| fib | 23 | 20 | 20 |
| sum | 32 | 33 | 33 |
| string-build | 34 | 35 | 38 |
| map | 26 | 27 | 26 |
| sort | 107 | 113 | 108 |

Flat within noise: hd startup plus the cache-hit check (~20 ms)
dominate these small cases, so the level barely moves the user-facing
wall. The exec differences only surface past ~50 ms of guest work.

### Suite walls, wasmtime-only (dev)

| level | suite wall | exec sum lang | exec sum std |
| --- | ---: | ---: | ---: |
| None | 13.73 s | 11.11 s | 27.34 s |
| Speed | 12.73 s | 12.85 s | 20.57 s |
| SpeedAndSize | 12.26 s | 11.80 s | 20.19 s |

(Lang exec rises at Speed while std falls hard; both are sums across 8
workers, so scheduling noise applies — but the direction is consistent:
test bodies execute faster than the extra compile costs.)

### Recommendation

Keep `None` for `hd test` and conformance: thousands of tiny modules
make turnaround compile-dominated, None compiles fastest, and verdicts
are identical. Keep `Speed` for `hd run --release` through the existing
`pipeline(release)` mapping: it executes 0-28% faster where guest work
dominates, at ~0.5 ms extra compile. Do not add a `SpeedAndSize` mode:
it ties `Speed` on compile and execute across everything measured.

## What holds 3.5 GB in the wasmtime suite

M34 measurement (2026-10-11): where the wasmtime-only suite's peak RSS
goes. No code was changed: nothing leaks, so there was nothing to fix.

### Method and conditions

- Peak RSS via `/usr/bin/time -l` on the test binary itself (children
  such as Node subprocesses are not included), dev profile, 8 workers
  unless noted, ambient load. One figure per line below is one run.
- `HD_CONFORMANCE_ONLY` selects subsets by path substring;
  `HD_TEST_JOBS=1` serializes workers. `typing` (1510 cases) exercises
  check only: no builds, no guests, no wasmtime engines.

### The ladder (GiB peak RSS)

| run | peak RSS |
| --- | ---: |
| typing only, check-only, no guests | 0.08 |
| full suite, Node engine (check + build, no wasmtime) | 1.15 |
| full suite, wasmtime-only, 1 worker | 2.06 |
| full suite, wasmtime-only, 8 workers | 3.47 / 3.60 (two runs) |

Subsets at 8 workers scale with case count (16 cases: 0.16 GB;
105 cases: 0.18 GB; 1034 cases: ~3.5 GB), i.e. roughly 3.5 MB retained
per runtime case on top of a ~0.15 GB fixed base (33 MB test binary,
engines, pools, compiler warmup).

### The four questions, answered

- **Per-engine reservations.** One pooled engine (16 slots, 32
  memories, 16 tables, 16 GC heaps; 4 GiB virtual reservations each —
  virtual, not resident) plus one shared on-demand engine. Guest
  threads run on 12 MB stacks. None of these is resident at scale;
  reservations are not where the gigabytes are.
- **Engines alive at once.** Two, for the whole run: the shared
  on-demand engine (one per config since M29) and the single
  process-wide pooled `TestEngine`. Per-case `WasmtimeEngine` wrappers
  are tiny handles, not engines.
- **Stores and instances outliving their case.** None found, by
  tracing: the entry path joins its scoped thread per run
  (`run_profile_with_guards`); test bodies drop their instance in
  `NativeBodyRunner::finish` and `execute_case` releases the only slot
  (covered by unit test); `InstancePre` maps die with their per-case
  `TestModule`. At most ~16 stores/instances are alive at once (8
  workers). No leak, so no code change.
- **Where it goes, then.** About 1.1 GB is the shared compiler store:
  every case's `Check`/`Code`/`Link` artifacts stay in the process-wide
  `MemoryStore` (the typing-only run holds 0.08 GB; the Node-mode full
  run holds 1.15 GB with no wasmtime at all). About 0.5-1 GB is engine
  code memory: wasmtime never frees compiled code while an engine
  lives, and both suite engines live for the whole run, so all ~992
  modules' native code accumulates (~1 MB per test module at
  `OptLevel::None` codegen bloat). About 1.5 GB is 8-worker transient:
  parallel Cranelift arenas, concurrent stores and 12 MB stacks, and
  allocator duplication — serializing to one worker drops peak from
  ~3.5 GB to 2.06 GB (wall climbs 13-16 s to 74 s). The M29/M31 module
  caches are tens of megabytes at most (128 entries), inside the noise.

### Top causes, fixes, expected savings

1. **Compiler artifacts retained for every case (~1.1 GB, in both
   engines' runs).** Fix: scope or drain the shared `MemoryStore` per
   phase/chapter, or bound it like the module caches. Expected: up to
   ~1 GB off peak; needs care since/and because other driver tests
   intentionally share the store for outputs.
2. **Engine code memory grows for the whole run (~0.5-1 GB).**
   Fix: retire and recreate the suite engines per chapter (recompiling
   after each rotation), trading memory for time. Expected: several
   hundred MB off peak at the cost of some recompilation; unmeasured,
   flagged for whoever picks it up.
3. **Eight-way concurrency transient (~1.5 GB).** Fix: fewer workers.
   Measured, not inferred: `HD_TEST_JOBS=1` peaks at 2.06 GB (saves
   ~1.5 GB) for a wall of 74 s against 13-16 s. A middle setting (2-4
   workers) splits the difference; not measured here.
