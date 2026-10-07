# New Compiler: Architecture Notes

The owner's direction for the new compiler and CLI, recorded as given.
Nothing here is spec; the [specification](../spec/README.md) stays
authoritative, and anything that changes language behavior goes through it.

## Context

- **Decision, 2026-10-06:** start the new compiler now; the prototype in
  [`src/`](../src/README.md) is frozen as a test oracle
  ([Roadmap](ROADMAP.md#order)).
- **Measured state of the prototype:**
  [baseline report](../audit/compiler/baseline-2026-10-06.md).
- **Earlier analysis:**
  [architecture directions, 2026-10-05](../audit/compiler/architecture-directions-2026-10-05.md),
  [status quo](../audit/compiler/status-quo-2026-10-04.md).
- **Core decisions already made (owner, 2026-10-06):**
  - NonEscapable comes after the first release.
  - No serializable closures in the first release.
  - Suspension lowering reserves no-op observability and replay hook
    points from day one.
  - `usize` is target-defined: 32 bits on Wasm32.

## Owner's Notes

<!-- Each entry: date, topic, the owner's point as stated. -->

### Goals (2026-10-06)

1. **Fast.**
2. **Parallel.**
3. **Supports incremental builds.**

### Must-Have Features (2026-10-06)

- **Parallel checking.**
- **Incremental checking.**

### Pillar 1 Features (owner-approved, 2026-10-06)

The owner approved the whole pillar 1 list ("all these are good") and added
two features.

- **Waiting:**
  - incremental checking (must-have);
  - parallel checking (must-have);
  - skip dependency bodies;
  - std checked in advance and built into the binary;
  - `hd test --affected`;
  - test runs that compile once and start from a snapshot;
  - `hd watch` / `hd dev`;
  - fast debug builds;
  - hard limits instead of hangs.
- **Tiered compilation** (owner). Dev builds are very fast; release builds
  are optimized. For example: a fast tier with a cheap Cranelift build or a
  baseline compiler for `hd run`, `hd test` and `hd dev`, and an optimizing
  tier (optimized Cranelift, later LLVM) for release. The design is deferred.
- **Module hot reloading** (owner), for server and frontend programs. In a
  running program, swap a changed module's code without restarting. The
  rules for state that crosses a reload are design work for later.
- **Tokens read:**
  - compact diagnostics by default;
  - the program database;
  - `hd doc` Markdown plus the git-distributed source;
  - deterministic output.
- **Retries:**
  - error recovery (all independent mistakes in one run);
  - one diagnostic per root cause;
  - exact-edit fix-its and `hd fix`;
  - did-you-mean, import and `let mut` hints;
  - `hd fmt`;
  - a pathology fuzzer.

### Candidate Features (2026-10-06)

Owner: "just think about features; later, if a feature is too hard to
implement, removing it is fine." Every item is in until its cost says
otherwise. Items marked ✓ exist in the prototype or are already decided.

- **Build and targets**
  - `hd build --target wasm | native | js | wasi`, with cross-compilation.
  - Native: one standalone executable with the runtime linked in.
  - WASI output for wasmtime, edge and serverless hosts.
  - JS: an npm package with generated TypeScript declarations.
  - `hd build --size-report`.
  - `hd app.wasm` ✓, and running native executables directly.
- **Dev loop**
  - `hd watch` / `hd dev`: incremental check, test and rerun on save.
  - Hot reload in dev: swap changed functions into the running program.
  - `hd fix`: apply all safe fix-its.
  - `hd fmt`.
  - **A program database instead of a language server** (owner,
    2026-10-06: "program database is better than lsp in agentic
    world"). The compiler writes queryable facts about the program, for
    example SQLite in `build/`: declarations, signatures, requirement
    rows, call edges, implementations, derives and diagnostics. An agent
    answers structural questions with one query. It falls out of the
    incremental engine's stored facts. The 2026-09-27 on-hold decisions
    are in git history. A language server for human editors is optional,
    later.
  - `hd explain CODE` ✓, `hd doc` ✓ (HTML and Markdown).
  - A notebook-style REPL whose cells rerun when what they depend on
    changes.
- **Testing and quality**
  - `hd test --affected`, `--watch`, run in parallel.
  - Snapshot tests with `--update`.
  - `hd test --coverage`.
  - `hd bench`: warm-up and spread reported.
  - `hd fuzz`: coverage-guided, reusing the derived `Arbitrary` generators.
  - Property tests ✓.
  - Conformance and differential testing across backends.
- **Debugging and observability**
  - Native debugging (lldb/gdb) and Wasm debugging in browser devtools,
    with source lines.
  - `hd run --profile`: CPU flamegraph and allocation profile.
  - `hd run --record` / `--replay`: deterministic replay, built on the
    reserved hooks.
  - Panic locations and cause chains ✓, `dbg` ✓.
- **Safety**
  - Capability grants ✓, identical on every backend.
  - `hd audit`: which capabilities each dependency's code requires.
- **Packages**
  - Git dependencies, `hd.sum` and workspaces ✓.
  - `hd add` / `remove` / `update`, plus offline and vendored mode.
  - `hd api diff` (an open issue).
  - `hd migrate` codemods, since hd has no editions.
- **Interop and embedding**
  - Embedding APIs: a Rust crate for native hosts, an npm package for JS
    hosts. The host supplies custom capability traits.
  - hd libraries exported as Wasm components, with WIT generated from hd
    traits.
  - Plugins: the parked runtime code loading.

### Multiple Backends (2026-10-06)

The language has more than one backend: **Wasm** and **Cranelift** first,
with **LLVM** and **JS** at low priority.

### How To Judge It: The Agentic Programming Language Arena (2026-10-06)

The owner's framework for comparing languages and toolchains for agentic
development. It has three pillars. They are not collapsed into one score;
the comparison looks at the **Pareto frontier**.

**1. Single-agent development cost.** What does it cost one agent to
produce a correct final artifact?

- Two measures only: **elapsed time** and **token cost**. Dollar cost is
  token use priced per model, so a separate dollar metric would count the
  same cost twice.
- Iterations, retries, compiler errors and debugging loops already show up
  in those two numbers, so they need no separate metrics.
- Failed attempts count too. A failed run still adds time and tokens.
  Comparing only successful runs misleads: a language that succeeds 95% of
  the time beats one that succeeds 60% even if its successful runs cost
  the same.
- The measure: **the expected elapsed time and token cost for one agent to
  obtain a correct solution.**

**2. Agent scalability.** Agentic development needs many agents working in
parallel, not one worker at a time.

- The question: **given a fixed compute or infrastructure budget, how much
  useful development work can run concurrently?**
- The resources: memory, CPU and disk footprint.
- The measure is the aggregate useful work of **N concurrent agents**, not
  the footprint of one agent. An environment that supports 100 parallel
  workers has a different economic profile from one that supports 20.

**3. Artifact quality.** What did the agents actually produce? Measure
observable properties of the result, not whether the source "looks good".

- **Correctness:** predefined unit tests, integration tests, property-based
  tests, end-to-end and computer-use tests.
- **Runtime performance:** execution time, throughput, latency, CPU use,
  memory use, executable size.
- Correctness is a gate: a faster wrong program is not a better outcome.

## Goal Metrics (proposal, 2026-10-06; awaiting the owner's edits)

Owner rules for these metrics:
- **Scripts only.** Every metric is checked by a script, with no AI and
  no agent runs ("each check must be tested without AI").
- **Read-only.** A script measures and reports pass/fail against its
  target. It never writes to the repo or the program under test; edits,
  such as applying a fix-it, happen in a temporary copy.
- **No prototype baselines.** The frozen prototype is not a baseline and
  shapes no target ("no need to think about it").
- **Neutral tooling.** Scripts live under `test/metrics/` and take any
  `hd` binary.

### Pillar 1: Single-agent development cost (proxies)

Compile time is agent wait time, so the pathological compile cases belong
here (owner, 2026-10-06).

| Script | Measures | Target |
|---|---|---|
| `edit-latency` | scripted one-function edit in a generated 10k-line package, then `hd check`; p50/p95 over 20 edits | p50 ≤ 50 ms, p95 ≤ 200 ms |
| `cold-check` | cold `hd check` of the 10k-line package | ≤ 1 s |
| `test-latency` | edit, then `hd test --filter one` | ≤ 300 ms |
| `mistakes` | corpus of single-mistake programs (seeded from audit/hd-writing-log.md), each with its expected code: diagnostics per mistake, output bytes, and whether applying the `--format json` fix-it in a temporary copy makes `hd check` pass | 1 diagnostic in ≥ 95%; fix-it resolves ≥ 80%; diagnostic ≤ 60 tokens |
| `answer-size` | bytes of `hd doc ITEM`, one failing `hd test`, `--format json` records on fixed inputs | fixed budgets, regression-gated |
| `determinism` | same inputs run 10 times | byte-identical output |
| `pathological` | compile-time stress cases, each with a time and memory budget: many overlapping impls (e.g. 1,600 `From` calls over two impls), deep nesting, long method and iterator chains, wide literals, large enums and matches, deep generic instantiation, long `?` chains, big files | each case within budget (e.g. ≤ 2 s, ≤ 200 MB); time grows near-linearly with size |
| `recheck-precision` | edit a private function body, count modules rechecked; edit a public signature, check only dependents recheck | 1 module for a private body edit; dependents only for a signature edit |
| `errors-per-run` | a file with N independent mistakes: diagnostics reported in one `hd check` | all N reported, each once |
| `diag-location` | mistake corpus: share of diagnostics whose line is the mistake's line | ≥ 95% |
| `fixit-safety` | applying a fix-it (in a temp copy) never adds a new error | 100% |
| `lookup-latency` | `hd doc ITEM`, `hd def NAME`, `hd explain CODE` wall time | p95 ≤ 100 ms |
| `fmt` | `hd fmt` time on the 10k-line package, and idempotence (if hd has a formatter) | ≤ 200 ms; idempotent |
| `release-check-cost` | runtime cost of overflow and bounds checks: the same test suite in a debug vs a release build, since agents run tests in debug | debug ≤ 1.3x release |

### Pillar 2: Agent scalability (compiler CPU and memory)

| Script | Measures | Target |
|---|---|---|
| `resources` | CPU-seconds and peak RSS for check, test and build on generated small, 10k- and 50k-line packages, cold and warm | warm check of 10k lines ≤ 0.2 CPU-s and ≤ 50 MB |
| `long-session` | RSS across 1,000 REPL inputs or watch-mode rechecks | flat (no growth beyond a fixed bound) |
| `startup` | `hd --version` and checking an empty file: wall time, CPU, RSS | ≤ 20 ms, ≤ 10 MB |
| `concurrency` | N = 1, 4, 16, 64 concurrent `hd check` processes; p95 latency vs N = 1, total CPU vs N | ≤ 1.5x at N = cores; total CPU sublinear in N with a shared cache |
| `disk` | artifacts plus toolchain size per worktree | ≤ 10 MB |
| `suite-cpu` | total CPU of the conformance suite | ≤ 60 s |
| `parallel-speedup` | checking a 50k-line package on 1 core vs all cores (one process) | ≥ 0.6 × cores speedup up to 8 cores |
| `cache-contention` | N processes writing the same cache entries at once | no corruption; each entry computed once |
| `cache-growth` | cache size after a scripted day of edits; eviction | bounded by a configured cap |
| `io-per-check` | files read or stat'ed per warm check | proportional to what changed |
| `fetch-dedup` | a dependency fetched by N worktrees | fetched once |

### Pillar 3: Artifact quality: the user's program, not the compiler

The artifact is the program built from the user's code (owner,
2026-10-06). Pillar 3 measures what that program does and how well the
toolchain helps make it correct.

| Script | Measures | Target |
|---|---|---|
| `proptest-perf` | a fixed property-test suite of user-style code (derived `Arbitrary` generators, `it_prop`, shrinking): cases per second, time to shrink a known failure | e.g. ≥ 100k cases/s for simple generators; shrink ≤ 1 s |
| `unit-test-perf` | a generated suite of unit tests (`tests:` blocks and `*_test.hd`, no host capabilities): time for `hd test` per 1,000 tests, warm, plus per-test overhead | e.g. ≤ 1 ms of overhead per test; 1,000 unit tests ≤ 1 s |
| `integration-test-perf` | a generated `tests/` suite (default profile, `temp_dir()`, real file system) and doc tests: time per test and setup cost per test program | e.g. ≤ 20 ms setup per test program; total sublinear in programs when they share a build |
| `runtime` | microbenchmarks of user-style programs with warm-up and spread, geomean vs Node | ≤ 1.5x; no case > 3x |
| `allocations` | allocations per iteration in counted loops and iterator chains of user programs | 0 for counted loops; ≤ 1 for chains |
| `size-startup-heap` | release Wasm size of user programs, instantiate to first output, peak heap | tiny ≤ 2 KB; ≤ 5 ms; ≤ 2x Node |
| `host-call-overhead` | cost per crossing for Console, Fs and serde-boundary calls | budget per call, e.g. ≤ 1 µs for a scalar call |
| `suspension-overhead` | cost per `!` await; `all!`/`race!` task throughput | budget per await; tasks/s target |
| `serde-throughput` | JSON encode/decode MB/s on fixed documents | within 2x of Node's JSON |
| `text-throughput` | string building, splitting and regex MB/s on fixed inputs | within 2x of Node |
| `dead-code` | Wasm bytes per 1,000 lines; unused std excluded | budget per size; no unused std in the binary |
| `long-run-memory` | a simulated service for 10 minutes: heap over time | flat after warm-up |

`proptest-perf`, `unit-test-perf` and `integration-test-perf` matter for
the program's correctness: the more cases an agent can afford per test run, the more
bugs its tests catch.

### Prerequisite: compiler correctness (a gate, not a pillar)

| Script | Measures | Target |
|---|---|---|
| `conformance` | portable conformance suite pass rate | 100% minus listed known failures; known failures → 0 |
| `incremental-soundness` | random edit sequences: the incremental result equals a clean build every time | 100% |

### Parked Metrics

- **Reproducible builds** (same source → same Wasm bytes across runs and
  machines). Parked by the owner, 2026-10-06.

## Prototype Baselines To Beat (2026-10-06)

The numbers come from the frozen prototype and were taken on a shared,
loaded machine. Treat the ratios as rough; the order of magnitude is the
reliable part.

**Runtime (Arena pillar 3).** `node --experimental-strip-types test/perf/micro/run.ts`,
medians in ms, release builds, after the P1e/P1f string fixes:

| case | hd | Node | Python | hd vs Node | Wasm size |
|---|---:|---:|---:|---:|---:|
| fib (recursion) | 3.9 | 5.9 | 58.0 | 0.66x (faster) | 1,186 B |
| sum (integer loop) | 76.7 | 7.0 | 177.8 | 11x slower | 1,555 B |
| string-build (100k parts) | 21.1 | 5.6 | 8.2 | 3.8x slower | 2,092 B |
| map (insert and lookup) | 27.7 | 5.2 | 7.0 | 5.3x slower | 3,613 B |
| sort | 41.7 | 17.0 | 9.5 | 2.5x slower | 3,412 B |

- **What the runner measures.** `hd build --release` runs once per case
  and only reports the Wasm size. Timing then compiles and instantiates in
  the same Node process, which is excluded, and times only the `main` call,
  three runs, median. The Python and Node programs time themselves the
  same way, so process startup is excluded everywhere.
- **Limits.** Build and compile time are not in these numbers. Three runs
  is a small sample, and V8's Wasm tier-up may fall inside them.
- **Why `sum` is 11x slower** (read from the emitted WAT, 2026-10-06).
  `for i in a..b` is not a counted loop. It runs the generic iterator
  protocol, and each iteration costs:
  - two indirect calls: `next`, then `call_ref` into the range closure;
  - about three heap allocations. The closure's counter is a captured
    `mut` in a `$hd.cell` holding a boxed `i32`, and each increment
    allocates a new box. `next` returns `Option[i32]` as a variant struct,
    and the payload is a second boxed `i32`;
  - about six `ref.cast`s with null checks.

  The loop body itself compiles to one `i64.add`. So 10M iterations do
  about 30M allocations and 20M indirect calls.
- **What the new compiler needs:**
  - **Counted range loops:** lower integer `a..b` / `a..=b` in `for` to a
    counted loop.
  - **Per-value-layout code** (the old task P4a), so `Option[i32]` and
    `i32` aren't boxed in generic code.
  - **Unboxed captured locals:** escape analysis and scalar replacement
    turn a non-escaping closure's captured `mut` back into a local.
  - **Inlined `next`:** inline small `next` bodies, so iterator chains
    compile to loops too. This is the specializing iterator design.
- **Iterator design after specialization.** The public `Iterator[T]`
  stays the closure-backed data type (Chaining Study CS8, 2026-09-29).
  The flat composed-stage design is a later option that waits for this
  specializing compiler.
  - Two of its questions stay open until it is pursued: how `take` stops
    without pulling one element too many, and what `zip`, `chain` and
    `flat_map` return.
  - The study's stage 2 benchmarks produced no valid measurements, because
    the prototype could not run the closure programs. Rerun them on the
    new compiler.
  - The study, the benchmark specification and both questions are in git
    history: `git show 38560c7a^:future-work/archive/ITERATOR_PERF.md`.
- **Sort** (read from lib/std, 2026-10-06). `List.sorted()` is a naive
  recursive merge sort written in hd. Each of its ~1.7M comparisons is an
  indirect closure call plus an `Ord` dictionary call that returns an
  `Ordering`, and both elements are unboxed first. Each recursion level
  allocates new left, right and merged lists and pushes one element at a
  time. The base case copies through `filter(fn: true)`. Node uses TimSort
  on unboxed numbers.
- **Map** (inferred from the stdlib and emitter; WAT not traced): boxed
  `usize` keys and `u64` values, an `Option` allocation per `get`, generic
  hash and equality dispatch, plus the range-loop cost above.
- **Owner, 2026-10-06: "at least compile it to i31ref".** A cheap first
  step before per-layout code: represent small integers as `i31ref`
  instead of a heap-allocated box struct.
  - `bool`, `char`, `u8`, `i8`, `u16` and `i16` always fit.
  - `i32`, `u32` and `usize` use `i31ref` when the value fits and fall back
    to a box when it doesn't (OCaml-style tagging), checked with one
    `ref.test` on unbox.
  - 64-bit integers and floats still need boxes or per-layout code.

  This removes most per-iteration allocations in sum, map and sort. The
  indirect calls and per-step `Option` structs remain.

**Compile and check latency (Arena pillar 1).** From the
[baseline report](../audit/compiler/baseline-2026-10-06.md), as CLI wall
time with process startup included:

| program | `hd check` | `hd build` | Wasm |
|---|---:|---:|---:|
| tiny program | 0.32 s | 0.60 s | 1,669 B |
| `calc.hd` (248 lines) | 0.50 s | 0.82 s | 14,442 B |

**Test suite.** `run-portable` (2,600+ cases) takes 65 s of wall time
alone, and 15–17 min when several worktrees run it at once.

**Benchmark gaps to close for the new compiler.**
- An edit-check latency benchmark: change one function in a mid-size
  package and time `hd check`. This measures pillar 1 and incremental
  builds.
- A memory-per-check measure for pillar 2.
- More runtime runs, with warm-up and a reported spread.

## What The Arena Asks Of The Compiler (orchestrator's reading, not owner text)

- **Pillar 1, development cost:**
  - Each check-edit loop is part of an agent's elapsed time, so check
    latency on a small edit matters more than full-build speed. This is
    where incremental builds pay.
  - Diagnostics that name the fix cut retries, and so cut tokens. The
    [hd writing log](../audit/hd-writing-log.md) records which messages
    failed agents.
  - Deterministic, machine-readable output (`--format json`) keeps an
    agent from re-running commands to parse them.
- **Pillar 2, scalability:**
  - The measure is per-agent memory, CPU and disk when N agents check and
    test at once.
  - That favors a shared, content-addressed cache of checked std and
    dependencies over a per-process copy. Today each process re-checks
    std (task P4c, deferred). Measured 2026-10-04: std is about 78% of a
    small warm compile (75 ms of 96 ms), and about 35% of the portable
    suite's CPU time.
  - It also favors a small resident memory per check, and no heavyweight
    daemon per agent.
  - The prototype's numbers: about 0.3 s to check a tiny program, and 15–17
    minutes for a full suite when several worktrees ran it at once,
    against 1.7 minutes alone.
- **Pillar 3, artifact quality:**
  - Correctness is pinned by the portable conformance suite, which is the
    gate.
  - Runtime performance and executable size are what the prototype does
    worst: 2.5–11x slower than Node outside recursion
    ([baseline](../audit/compiler/baseline-2026-10-06.md)).

## Carried Over From Prototype Records (not owner text)

Implementation notes written for the prototype, moved here when their
records were deleted (2026-10-06). The spec decides behavior; these say
how.

### Literal Inference

From the Standard Library Plan's compiler handoff, for the open literal
variables of [Open Literal Width](../spec/lang/04-type-system.md#open-literal-width):

1. Check bidirectionally first. A literal with an expected type gets its concrete type on the spot, and a binary operator checks its non-literal operand first, on either side. Only a literal with no expected type gets a variable.
2. Keep variables as integer IDs in flat per-body arrays: a union-find parent with path halving and rank, a binding (a width or none), and the first deciding span for blame. Free the arena after the body.
3. Unify in O(α). Detect a conflict at union time, with the stored blame span.
4. Sweep only what is open: a has-vars bit on interned types, and a per-body list of nodes whose types hold variables. The end-of-body sweep walks only that list.
5. Keep obligations in an append-only list of (node, kind). After the fallback, process each once and patch the result into a side table, with no argument re-check.
6. For speculation, push union-find bindings on a trail (an undo log), and roll back by popping it. Never copy checker state (the prototype's F-626 in [src/KNOWN_ISSUES.md](../src/KNOWN_ISSUES.md)).
7. Bodies are independent: check the top-level body first, then function bodies in any order, in parallel or lazily. An edit re-checks only its body.
8. Queue a generic instantiation that meets an open variable until after the sweep, then deduplicate it through the instantiation cache by concrete types. Codegen sees only concrete types.
9. The cost is O(n·α) per body, and nothing extra for a literal with an expected type.

### Wake-Driven Host Entries

From the deferred Wake-Driven Host Entries proposal. The prototype's
suspending entry export polls until Ready and blocks the JavaScript event
loop (F-555), against
[`req.entry.busy-poll`](../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll).

- The embedding exposes a JavaScript host entry interface apart from the
  raw Wasm exports: ordinary entries return directly, and suspending
  entries return a Promise over native start, poll, cancel, and result.
- A stale waker must not affect a later execution.
- Failure cleanup runs once and keeps the original failure if cleanup
  also fails.
- The instance's providers and the execution frame survive each Pending
  interval.
- Tests to cover: delayed, synchronous, duplicate, and stale wakes;
  competing drivers; cancellation; provider retention; and poisoning.

## Host Capabilities: What The New Compiler Inherits (2026-10-06)

The prototype implements the approved host-capability spec (task N3, three
sessions): `std.http` with `ScriptedHttp`, `[capabilities]` and `--cap`
with precedence, path and env scope checks, the test grant, startup refusal
read from the Wasm import list, `Process` behind its grant, and the
playground's same-origin `Http`. The new compiler must also deliver these,
which the prototype doesn't:

- **`std.sys`** and **`std.net`** (TCP, UDP, DNS), with closable handles.
- **An HTTP server and streaming bodies.**
- **Async host calls,** for example through `wasi:http` and `host_wait!`, with
  real cancellation (`std-http.send.cancel`). The prototype answers host
  calls synchronously, so cancelling `send!` doesn't abort the request.
- **A race-free path sandbox.** Use preopened directories rather than
  `realpath`-then-open; the prototype has a check-then-use race on swapped
  symlinks.
- **Runtime code loading,** parked in OPEN_ISSUES.

## Architecture Brainstorm From The Metrics (2026-10-06)

The orchestrator's ideas for which compiler features move which metrics.
Owner: "these suggestions are meaningful". Big features are discussed
first. Runtime performance gets its own systematic discussion later. Many
agents on one machine is "probably too stretched" and is deprioritized.
Nothing here is decided.

### Big Features (to discuss first)

| # | Feature | Moves |
|---|---|---|
| A | A native single-binary toolchain (Rust, Go or Zig rather than Node) | startup, lookup-latency, cold-check, disk, resources |
| B | A query-based incremental engine (salsa / rust-analyzer style), keyed by interface hashes. Explicit signatures mean a body edit never changes an interface | edit-latency, test-latency, recheck-precision, incremental-soundness |
| C | A shared content-addressed on-disk cache of checked interfaces and compiled modules (std, dependencies, own modules), published atomically | cold-check, cache reuse, suite-cpu |
| D | Parallel checking: interfaces resolved in module-graph waves, then all function bodies checked in parallel | parallel-speedup, cold-check, suite-cpu |
| E | Code generated per value layout (monomorphization), with `i31ref` as the first step | runtime, allocations, proptest/serde/test perf |

Open decisions they raise:
- **Implementation language.** Rust leads: it has salsa, rayon, rowan and wasm-encoder.
- **Wasm engine for `hd run` and `hd test`.** V8 is mature but slow to start. Embedded wasmtime starts fast, but its Wasm GC support is maturing. The engine should sit behind an interface either way.
- **Daemon or none.** The lean is none: a native binary plus the shared cache.
- **Limits on monomorphization,** so binaries don't grow without bound.

### Pillar 1 Ideas (agent wait time and retries)

- A lossless CST parser with error recovery, plus checker recovery per item with "poison" types, so one error never hides or multiplies others.
- Machine-readable fix-its as exact text edits, plus `hd fix` to apply every safe fix-it in one command.
- Compact JSON diagnostics by default, with details on request. Today one diagnostic is 7.4 KB.
- A formatter on the same CST. Deterministic output everywhere.
- Skip dependency bodies when checking: explicit signatures make them unnecessary.
- Early cutoff: an unchanged interface hash stops propagation.
- `hd test --affected`: run only the tests the change can reach.
- Pre-checked std baked into the binary, and zero-copy, memory-mappable cache files.
- No exponential algorithms: bounded impl search (F-626), iterative passes for deep nesting and chains, arenas.
- Hard limits (recursion, instantiation depth, impl-search steps), each with its own diagnostic instead of a hang.
- A pathology fuzzer that flags superlinear growth.
- A lightly optimized debug build with cheap overflow and bounds checks.
- `hd doc`, `def` and `explain` answered from the query database.

### Pillar 2 Ideas (deprioritized: "too stretched")

- A cross-process jobserver: `hd` processes share a CPU token pool, as make and cargo do, to stop the 9x slowdown from too many threads.
- A remote cache later, using the same content-addressed keys.
- Interning per compilation run, not global.
- A capped cache with eviction, lock-free reads, and change detection by hash and mtime.

### Pillar 3 Ideas (runtime; discussed systematically later)

- Counted range loops, inlining, and escape analysis that turns captured counters back into locals.
- `Option` of a reference as a nullable ref (no allocation); `Option` of a scalar returned as two values.
- Closures that capture nothing become function references.
- Derive templates compiled to straight-line code at compile time.
- Typed host imports for scalars, serde only for structured values, and buffered console output.
- Suspension compiled to state machines with an allocation-free "already ready" path, or stack switching later.
- Snapshot-started tests and a pooled test runner.
- Integrated shrinking for proptest, with cases run in one instance.
- Enum layout chosen per enum (GC subtypes or a tag plus shared fields).
- Whole-program dead-code elimination and `wasm-opt`.

### Correctness Tooling

- Incremental-soundness fuzzing: random edit scripts, with incremental builds compared against clean builds.
- Differential fuzzing across the new compiler, the frozen prototype and the spec examples.

### Language Note

No language change is needed. Explicit signatures and requirement rows, no
overloading, no wildcard imports, orphan rules, per-module scope, and
checked templates instead of macros already give what this architecture
needs. Two things to watch: typed-derivation templates expanded per type
(cache them per type), and cross-package coherence checks.

## Follow-Up Questions

<!-- Questions that the notes raise, each with a recommendation. -->

- **Cleanup that suspends** (from usability probe 6). `defer` can't make
  a bang call (`flow.defer.suspend`), so a temp file can't be removed in
  a `defer`. **Owner, 2026-10-06: keep the rule for the first release,
  and revisit it with NonEscapable.** It touches how suspension is
  lowered, so the new compiler should leave room for it.
