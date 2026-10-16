# New Compiler: Goals, Metrics And Feature Triage

Status: design record; may lag the code. The code and the spec win.

The owner's direction for the new compiler and CLI. The
[specification](../../spec/README.md) stays authoritative: nothing here is
spec, and language decisions live there. Detailed design:
[ARCHITECTURE.md](ARCHITECTURE.md).

## Summary

**Goals (owner):**

1. **Fast.**
2. **Parallel.**
3. **Supports incremental builds.**

**Must-have features (owner):** parallel checking and incremental
checking.

**Decided architecture (owner):**

- The compiler is written in Rust. Incremental checking uses a per-module
  on-disk cache (`hd_cache`), with no salsa and no daemon.
- **The compiler runs in the web playground.** It is compiled to Wasm
  (`hd_web`) and checks, builds and tests in a browser tab, on one thread.
  So the cache has a storage interface without a file system, checking
  works on one thread, and the host interface has a JS implementation
  (`host/`). A threaded browser build: not built yet; see #189.
- Generics are fully monomorphized. The one erased path is a generic
  method called through a `dyn` value: one erased body per impl, which
  accepts any type argument and receives a type witness per method type
  parameter (bare `T` and `T?` boxed, containers of `T` read and written
  in place through the witness, function types adapted by caller-built
  thunks). The owner accepted a slower `dyn`.
- Polymorphic recursion is a build error at the instantiation depth limit
  (`instantiation-too-deep`); there is no boxed fallback.
- Merging byte-identical functions after monomorphization: not built yet;
  see #175.
- The host ABI is hd's own core-Wasm imports, not the Component Model.
- A browser `block_on` that waits on the host uses JSPI, else a
  synchronous same-origin XHR for HTTP, else a `host-contract` panic.
  Implementation differs: the browser host (`host/web.mjs`) always sends
  HTTP with a synchronous XHR from its worker and has no JSPI path; not
  judged.
- **Two pipelines** ([tiering.md](tiering.md)): a dev pipeline for
  `hd run`, `hd test` and the REPL, and an optimized pipeline for
  `--release`. Optimizations run in the optimized pipeline only.
- The first release targets Wasm only, run on wasmtime inside `hd` or on
  V8. A native backend with its own GC: not built yet; see #188.
- The compiler process has no memory cap. The shared cache defaults to
  10 GB with LRU eviction.
- Release builds omit the standard `name` section and keep the compact
  `hd.names` for backtraces.
- NonEscapable types come after the first release; see #163. `defer`
  stays, and a `defer` suite still cannot make a bang call.
- No serializable closures in the first release; see #166.
- Suspension lowering reserves no-op observability and replay hook points
  (crates/hd_wasm/DESIGN.md §14.7).

## How It Is Judged: The Agentic Programming Language Arena

The owner's framework for comparing languages and toolchains for agentic
development. It has three pillars, compared on their **Pareto frontier**,
not collapsed into one score.

**1. Single-agent development cost.** The expected elapsed time and token
cost for one agent to obtain a correct solution. Iterations, retries and
debugging loops show up in those two numbers. Failed attempts count too:
a language that succeeds 95% of the time beats one that succeeds 60% even
if its successful runs cost the same.

**2. Agent scalability.** Given a fixed compute budget, how much useful
work N concurrent agents do. The resources are memory, CPU and disk.

**3. Artifact quality.** Observable properties of what the agents
produced: correctness (unit, integration, property-based and end-to-end
tests) and runtime performance (time, throughput, latency, memory,
executable size). Correctness is a gate: a faster wrong program is not a
better outcome.

## Goal Metrics

Each metric is a script under `bench/metrics/scripts/`
([bench/metrics/README.md](../../bench/metrics/README.md)). The owner's
rules: every metric is checked by a script, with no AI and no agent run;
a script only measures and reports, never writes to the repository or the
program under test; a script takes any `hd` binary; no target comes from
the deleted prototype.

### Pillar 1: Single-agent development cost (proxies)

Compile time is agent wait time, so the pathological compile cases belong
here.

| Script | Measures | Target |
|---|---|---|
| `edit-latency` | scripted one-function edit in a generated 10k-line package, then `hd check`; p50/p95 over 20 edits | p50 ≤ 50 ms, p95 ≤ 200 ms |
| `cold-check` | cold `hd check` of the 10k-line package | ≤ 1 s |
| `test-latency` | edit, then `hd test --filter one` | ≤ 300 ms |
| `mistakes` | corpus of single-mistake programs, each with its expected code: diagnostics per mistake, output bytes, and whether applying the `--format json` fix-it in a temporary copy makes `hd check` pass | 1 diagnostic in ≥ 95%; fix-it resolves ≥ 80%; diagnostic ≤ 60 tokens |
| `answer-size` | bytes of `hd doc ITEM`, one failing `hd test`, `--format json` records on fixed inputs | fixed budgets, regression-gated |
| `determinism` | same inputs run 10 times | byte-identical output |
| `pathological` | compile-time stress cases, each with a time and memory budget: overlapping impls, deep nesting, long method and iterator chains, wide literals, large enums and matches, deep generic instantiation, long `?` chains, big files | each case within budget; time grows near-linearly with size |
| `recheck-precision` | edit a private function body, count modules rechecked; edit a public signature, check only dependents recheck | 1 module for a private body edit; dependents only for a signature edit |
| `errors-per-run` | a file with N independent mistakes | all N reported, each once |
| `diag-location` | mistake corpus: share of diagnostics whose line is the mistake's line | ≥ 95% |
| `fixit-safety` | applying a fix-it (in a temporary copy) never adds a new error | 100% |
| `lookup-latency` | canned program-database queries; N/A until the program database exists (#181) | p95 ≤ 100 ms |
| `fmt` | `hd fmt` time on the 10k-line package, and idempotence | ≤ 200 ms; idempotent |
| `check-cost` | runtime cost of overflow and shift checks: the same suite in the optimized pipeline with checks on (`hd test --release`) vs off (the wrapping release profile) | checked ≤ 1.3x unchecked |
| `dev-speed` | the same suite in one profile, dev pipeline vs optimized pipeline, so tests don't crawl | ≤ 4x geomean; no case > 10x |

### Pillar 2: Agent scalability (compiler CPU and memory)

| Script | Measures | Target |
|---|---|---|
| `resources` | CPU-seconds and peak RSS for check, test and build on generated small, 10k- and 50k-line packages, cold and warm | warm check of 10k lines ≤ 0.2 CPU-s and ≤ 50 MB |
| `long-session` | RSS across 1,000 REPL inputs or incremental rechecks | flat (no growth beyond a fixed bound) |
| `startup` | `hd --version` and checking an empty file: wall time, CPU, RSS | ≤ 20 ms, ≤ 10 MB |
| `concurrency` | N = 1, 4, 16, 64 concurrent `hd check` processes; p95 latency vs N = 1, total CPU vs N | ≤ 1.5x at N = cores; total CPU sublinear in N with a shared cache |
| `disk` | artifacts per worktree | ≤ 10 MB of artifacts |
| `suite-cpu` | total CPU of the conformance suite | ≤ 60 s |
| `parallel-speedup` | checking a 50k-line package on 1 core vs all cores (one process) | ≥ 0.6 × cores speedup up to 8 cores |
| `cache-contention` | N processes writing the same cache entries at once | no corruption; each entry computed once |
| `cache-growth` | cache size after a scripted day of edits; eviction | bounded by a configured cap |
| `io-per-check` | files read per warm check; with no daemon, every source file is still stat'ed | reads proportional to what changed; stats proportional to the source file count |
| `fetch-dedup` | a dependency fetched by N worktrees | fetched once |

### Pillar 3: Artifact quality: the user's program, not the compiler

The artifact is the program built from the user's code.

| Script | Measures | Target |
|---|---|---|
| `proptest-perf` | a fixed property-test suite of user-style code: cases per second, time to shrink a known failure | e.g. ≥ 100k cases/s for simple generators; shrink ≤ 1 s |
| `unit-test-perf` | a generated suite of unit tests: time for `hd test` per 1,000 tests, warm, plus per-test overhead | e.g. ≤ 1 ms of overhead per test; 1,000 unit tests ≤ 1 s |
| `integration-test-perf` | a generated `tests/` suite and doc tests: time per test and setup cost per test program | e.g. ≤ 20 ms setup per test program |
| `runtime` | microbenchmarks of user-style programs with warm-up and spread, geomean vs Node; GC collections and pauses per case | ≤ 1.5x; no case > 3x; pauses reported |
| `allocations` | allocations per iteration in counted loops and iterator chains | 0 for counted loops; ≤ 1 for chains |
| `size-startup-heap` | release Wasm size of user programs, instantiate to first output, peak heap | tiny ≤ 2 KB; ≤ 5 ms; ≤ 2x Node |
| `host-call-overhead` | cost per crossing for Console, Fs and serde-boundary calls | e.g. ≤ 1 µs for a scalar call |
| `suspension-overhead` | cost per `!` await; `all!`/`race!` task throughput | budget per await; tasks/s target |
| `serde-throughput` | JSON encode/decode MB/s on fixed documents | within 2x of Node's JSON |
| `text-throughput` | string building, splitting and regex MB/s on fixed inputs | within 2x of Node |
| `dead-code` | Wasm bytes per 1,000 lines; unused std excluded | no unused std in the binary |
| `long-run-memory` | a simulated service for 10 minutes: heap over time, GC pauses, p99 latency of its request loop | flat after warm-up; pause and p99 reported |

### Prerequisite: compiler correctness (a gate, not a pillar)

| Script | Measures | Target |
|---|---|---|
| `conformance` | portable conformance suite pass rate | 100% minus listed known failures; known failures → 0 |
| `incremental-soundness` | random edit sequences: the incremental result equals a clean build every time | 100% |

Reproducible builds across machines are parked by the owner.

## Features

Triaged with the owner. **Day 1** items shape the architecture. **v1**
items ship in the first release. **Later** items come after v1, with any
hook they need reserved now. Pillar 2 features are deprioritized and not
triaged.

### Pillar 1 Features: Agent Wait Time And Retries

**Day 1:** incremental checking on a per-module cache; parallel checking;
early cutoff, where an unchanged interface hash stops propagation;
dependency bodies skipped, since explicit signatures make them
unnecessary; a lossless CST parser with error recovery; checker recovery
per item with poison types; no exponential algorithms, and hard limits
(recursion, instantiation depth, impl-search steps) with their own
diagnostics instead of hangs; deterministic output; compact diagnostics
with details on request, JSON, and exact-edit fix-its.

**v1:** std checked in advance and built into the binary (`hd_stdpack`);
the fast dev pipeline; one diagnostic per root cause; `hd fix`; `hd fmt`;
scoped `hd check FILE`; `hd test --affected`; `--max-errors` and
`--summary`; quiet passing tests, a repro command and the seed on every
failure; `hd doc`.

Planned, not built: structured assert diffs that print only the differing
fields; typed holes (`_` / `todo()` report the expected type and the names
in scope that fit); a pathology fuzzer in CI that flags superlinear growth.

**Later:**

- A program database instead of a language server: not built yet; see
  #181.
- Module hot reload: not built yet; see #182.
- LLVM for the optimizing tier.
- Deferred type errors, code-generating fix-its, diff-aware output, a
  notebook-style REPL, and a language server for human editors.

**Dropped** (owner): `hd explain`; `hd watch` / `hd dev`; background test
precompilation; tests started from a snapshot; a syntax-and-names
`hd check --fast` tier.

### Pillar 2 Features: Agent Scalability

Deprioritized; "probably too stretched" (owner). The ideas: share work
instead of repeating it (content-keyed caches, std checked once per
machine, shared compiled code, copy-on-write builds, shared fetches,
results cached by content, a remote cache later); don't oversubscribe (a
cross-process jobserver, priority classes, per-run caps); keep each
process small and cheap to start; batch and split work (`--roots`, test
sharding); hd's sandbox as a container substitute; and visibility (`hd
stats`, cache quotas, `hd cache gc`, which exists).

### Pillar 3 Features: Artifact Quality

**Day 1:** code generated per value layout (monomorphization), with an
enum layout chosen per enum; suspension lowered to state machines with
reserved hook points; every source of nondeterminism (time, random, I/O,
scheduling) behind a capability or the scheduler; only reachable items
compiled; the conformance suite on every engine.

**v1:** fast property tests (integrated shrinking, cases in one instance,
a regression file that replays first); unit and integration tests in
parallel on a pooled runner; counted range loops, capture-free closures
as constant globals, cheap `Option`; cheap host calls and host calls that
can suspend; a resource limit (`--time-limit`) with
capability grants; symbolized backtraces in release builds.

**Later:**

- Deterministic simulation testing, record and replay, structured
  tracing, generated recording fakes, `hd test --coverage`, `hd lint`.
- General inlining, escape analysis, profile-guided optimization, SIMD.
- Data parallelism; until then the runtime keeps no global mutable state.
- `hd bench`: not built yet; see #184.
- A native backend with its own GC: not built yet; see #188.

**Dropped** (owner): `hd fuzz`; per-request arenas.

### Toolchain-Wide Features

**Day 1:** the `hd` CLI is itself a host on the embedding API, so
embedding packages come later without splitting the CLI.

**v1:** Wasm output run on wasmtime inside `hd`; `hd app.wasm`; git
dependencies, `hd.sum` and workspaces; `hd add`, `hd remove`,
`hd update`.

**Later:** `hd build --target native | wasi | js`; embedding APIs for
native and JS hosts (the Wasm boundary: not built yet; see #51); hd
libraries as Wasm components; offline and vendored mode; plugins and
runtime code loading, which need a design pass.

**Deferred** (owner): an API compatibility checker, `migrate` codemods,
and a per-dependency capability audit.

## Inherited Implementation Notes

The spec decides behavior; these say how.

### Literal Inference

For the open literal variables of
[Open Literal Width](../../spec/lang/04-type-system.md#open-literal-width):

1. Check bidirectionally first. A literal with an expected type gets its concrete type on the spot, and a binary operator checks its non-literal operand first, on either side. Only a literal with no expected type gets a variable.
2. Keep variables as integer IDs in the body's flat inference table (`hd_types::unify::InferTable`): a union-find parent with union by rank, a binding, and a kind (integer literal, signed integer literal, float literal or general). Lookups never write, so every link and rank change is on the trail and rolls back exactly; that is simpler than path halving with the same log n bound.
3. Unify in near-constant time. Detect a conflict at union time.
4. Sweep only what is open at the end of the body.
5. For speculation, push changes on a trail (an undo log) and roll back by popping it; a failed trial also drops the variables it made. Never copy checker state.
6. Bodies are independent: an edit re-checks only its body.
7. Codegen sees only concrete types.

### Host Capabilities Still To Deliver

- An HTTP server and streaming bodies: not built yet; see #186.
- **A race-free path sandbox.** Granted directories are opened once as
  directory handles, and every file operation resolves relative to them,
  as the wasmtime host does (`hd_run_wasmtime/src/fs/sandbox.rs`).
  Implementation differs: the Node host (`host/node.mjs`) checks
  `coversPath`, then opens the path by name, since Node has no `openat`.
  Worse: there, a symlink swapped between the check and the open escapes
  the grant (docs/audit/diagnostic-notes.md).
- Runtime code loading: parked in
  [Open Issues](../future-work/OPEN_ISSUES.md#library-and-host-design-work).
