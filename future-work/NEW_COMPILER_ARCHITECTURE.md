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

The owner asked for goal metrics from the three Arena pillars. Running
agents is too expensive, so every metric is a **proxy measured by running
only the compiler** (owner: "instead of measure how many agents work,
measure compiler's cpu and mem usage").

### Pillar 1: Single-agent development cost

An agent's cost is mostly time spent waiting, tokens spent reading, and
retries. Each has a compiler-only proxy.

| Proxy | Stands in for | How measured | Target |
|---|---|---|---|
| Edit → diagnostics latency | waiting per loop | scripted one-function edit in a 10k-line package, time `hd check` | p50 ≤ 50 ms, p95 ≤ 200 ms |
| Cold check; edit → test result | waiting per loop | cold `hd check` of 10k lines; `hd test --filter one` after an edit | ≤ 1 s; ≤ 300 ms |
| Fix-it success rate | retries | mistake corpus from the writing log (~250 rows, grown by probes): apply the diagnostic's machine-readable fix, recheck | ≥ 80% fixed by the first suggestion |
| Diagnostics per root cause | retries, noise | same corpus: diagnostics per single-mistake program | 1.0 in ≥ 95% |
| Output tokens per answer | tokens read | size of: one typical error, `hd doc ITEM`, one failing `hd test`, `--format json` | fixed budgets (e.g. diagnostic ≤ 60 tokens, doc item ≤ 150), regression-gated |
| Determinism | re-runs | same input run 10 times | byte-identical, 100% |

### Pillar 2: Agent scalability, as compiler CPU and memory

Run N compiler processes, not N agents.

| Proxy | How measured | Prototype | Target |
|---|---|---|---|
| CPU-seconds per operation | `hd check`/`test`/`build` on fixed small, 10k- and 50k-line packages, cold and warm cache | not measured | budget per size (e.g. warm check of 10k lines ≤ 0.2 CPU-s) |
| Peak RSS per operation | same runs | not measured; intern caches grow without bound | ≤ 50 MB at 10k lines; flat over 1,000 REPL inputs |
| Startup cost | `hd --version` and checking an empty file: CPU and RSS | Node + TypeScript load | ≤ 20 ms, ≤ 10 MB |
| Concurrency slowdown | N = 1, 4, 16, 64 concurrent `hd check` on one box; p95 vs N = 1 | full suite ~9x slower with several worktrees | ≤ 1.5x at N = cores |
| Cache sharing | N processes over the same std and dependencies: module reuse, total CPU vs N | 0% reuse | ≥ 95% reuse; total CPU sublinear in N |
| Disk per worktree | artifacts plus toolchain | `node_modules` per worktree | ≤ 10 MB |
| Conformance-suite CPU | total CPU, ~2,800 cases | ~764 s | ≤ 60 s |

### Pillar 3: Artifact quality (correctness is the gate)

| Proxy | How measured | Prototype | Target |
|---|---|---|---|
| Conformance | portable suite | 2,803 / 2,825, 22 known failures | 100% minus listed known failures; known failures → 0 |
| Oracle agreement | fixtures plus fuzzed programs through both compilers | — | 0 unexplained divergences |
| Runtime speed | microbenchmarks with warm-up and spread, geomean vs Node | fib 0.66x … sum 11x | ≤ 1.5x; no case > 3x |
| Allocations in hot loops | instrumented runs | ~3 per iteration in a counted loop | 0 for counted loops; ≤ 1 for iterator chains |
| Size, startup, peak heap | release Wasm; instantiate → first output; heap high-water mark | tiny 1,669 B; others not measured | tiny ≤ 2 KB; ≤ 5 ms; ≤ 2x Node |

### Tools To Build First

- **Mistake corpus with fix-it checking.** It turns the hd writing log into an
  automatic diagnostics regression test, and is the best proxy for agent
  cost.
- **Resource harness.** One script runs check, test and build at three
  package sizes, alone and N-way concurrent. It reports CPU-seconds, peak
  RSS, p50/p95 latency and cache reuse.

Both can run against the frozen prototype now, to fill the "not measured"
cells. In CI, the cheap ones become gates, as `perf:check` is today.

Staging: Milestone 1 is correctness parity plus the pillar 1 latency and
pillar 2 resource numbers, which come from the architecture: a native
binary, a shared cache and incremental checking. Milestone 2 is runtime
speed: `i31ref`, then per-layout code.

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
    std (task P4c, deferred).
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

## Follow-Up Questions

<!-- Questions that the notes raise, each with a recommendation. -->

- **Cleanup that suspends** (from usability probe 6). `defer` can't make
  a bang call (`flow.defer.suspend`), so a temp file can't be removed in
  a `defer`. **Owner, 2026-10-06: keep the rule for the first release,
  and revisit it with NonEscapable.** It touches how suspension is
  lowered, so the new compiler should leave room for it.
