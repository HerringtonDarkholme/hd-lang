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

## Follow-Up Questions

<!-- Questions that the notes raise, each with a recommendation. -->

- **Cleanup that suspends** (from usability probe 6). `defer` can't make
  a bang call (`flow.defer.suspend`), so a temp file can't be removed in
  a `defer`. **Owner, 2026-10-06: keep the rule for the first release,
  and revisit it with NonEscapable.** It touches how suspension is
  lowered, so the new compiler should leave room for it.
