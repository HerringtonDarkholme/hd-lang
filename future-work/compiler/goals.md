# New Compiler: Architecture Notes

The owner's direction for the new compiler and CLI, recorded as given, with
the orchestrator's proposals kept apart. Last restructured 2026-10-06.

- The [specification](../../spec/README.md) stays authoritative. Nothing here
  is spec, and anything that changes language behavior goes through it.
- Every section says whose text it is: the owner's, or the orchestrator's.
- Measured state of the prototype:
  [baseline report](../../audit/compiler/baseline-2026-10-06.md).
- Earlier analysis:
  [architecture directions, 2026-10-05](../../audit/compiler/architecture-directions-2026-10-05.md),
  [status quo](../../audit/compiler/status-quo-2026-10-04.md).

## Summary

Detailed design: [README.md](README.md)

**Goals (owner, 2026-10-06):**

1. **Fast.**
2. **Parallel.**
3. **Supports incremental builds.**

**Must-have features (owner, 2026-10-06):**

- **Parallel checking.**
- **Incremental checking.**

**Decided (owner, 2026-10-07):**

- Identity (option B, extended): `.Some`, `.Ok`, `.Err` and primitive or
  tuple boxes have no identity. `is` is a compile error on any operand whose
  static type is a value type (an optional or `Result` takes its payload's
  category) and on function values; on a value known only dynamically as
  `Any`, the result is unspecified.
- **Every enum is an identity-free value type** (`Option`, `Result` and user
  enums; [research-enum-values.md](research-enum-values.md)). Enums and
  function types move to `AnyVal`; `is` on an enum or a function value is
  `identity-requires-references`. `Error` drops its `AnyRef` bound. Its
  helpers `find`, `root_cause` and `chain` become Rust-style inherent
  methods on the `dyn Error` type (`impl dyn Error:` in the trait's own
  module), so `err.find::[T]()` keeps working and cannot be overridden;
  `find` matches by `TypeId` and returns values through `downcast_val`,
  data-type errors by reference.
- Generics are fully monomorphized. The one erased path, a generic method
  called through a trait value, accepts any type argument: value-typed
  arguments are boxed at that call (boxes have no identity), so the
  `T < AnyRef` restriction of `types.trait.safe.method-type-param-implied`
  is dropped.
- **Trait value types are written `dyn Trait`** (`dyn Any`, `dyn Error`,
  `dyn Supplier[Item = i32]`); a bare trait name in type position is an
  error with a fix-it that inserts `dyn`. `dyn` marks where boxing and
  dynamic dispatch happen; `T < Trait` stays the monomorphized form. The
  per-trait dynamic-safety gate is dropped (Swift 5.7 style): every trait
  can be a `dyn` type, and members that cannot work dynamically (`Self`
  parameters, associated functions) are unavailable on the `dyn` value,
  with the error at the call. Applied in S1d:
  [Dynamic Trait Values](../../spec/lang/09-traits.md#dynamic-trait-values),
  [Inherent Methods On `dyn` Types](../../spec/lang/09-traits.md#inherent-methods-on-dyn-types).
- Generic methods called through a `dyn` value keep one erased body per
  impl and accept any type argument, with no restriction (owner: "it is
  already dyn, keep it erased, i can accept slower dyn"). The erased body
  receives a type witness per method type parameter (Swift-style layout
  operations for the caller's concrete type): bare `T` and `T?` are boxed,
  containers of `T` are read and written in place through the witness, and
  function types are adapted by caller-built thunks. Everything outside
  `dyn` stays fully monomorphized.
- **Facts are runtime values.** A fact or metadata expression is evaluated
  once, lazily, on first read (like a lazily initialized global), not at
  compile time. A panic there is a runtime panic with category
  `fact-evaluation-failed`; unread facts are never evaluated; a fact may
  hold anything a global can. This removes the compile-time fact
  evaluator, its budget, the `fact` cache entry and value-graph
  serialization (Codex re-review N6, questions 3 and 4). Requirement-free
  and the direct `block_on`/`println` ban still apply. Applied in S1e:
  [Facts](../../spec/lang/14-annotations.md#facts).
- REPL redefinition is shadowing: earlier items keep the old definition
  (`live-execution.md`). A rebuild may lose a stopped (busy, interrupted)
  input's changes, with a spec rule and a REPL report. Applied in S1e:
  [Redefinition](../../spec/cli/command-line.md#redefinition),
  [Rebuilding A Session](../../spec/cli/command-line.md#rebuilding-a-session).
- Associated types: a `dyn` type must bind every associated type (Rust
  style, `trait.dyn.binding.complete` unchanged). When two supertrait paths
  bind the same associated type, equal bindings merge and different ones
  are `duplicate-associated-binding` at the declaring trait.
- **Tiers** ([tiering.md](tiering.md)): a dev pipeline (almost no hd
  passes; Cranelift `None` with the single-pass allocator if spike T1
  confirms) and an optimized pipeline (bounded inlining, scalar
  replacement, closure specialization, devirtualization, then Cranelift
  `Speed`). Those optimizations run in the optimized pipeline only,
  reversing "the same in debug and release". `release-check-cost` splits
  into `check-cost` (optimized pipeline, checks on vs off, at most 1.3x)
  and `dev-speed` (dev vs optimized in one profile, at most 4x geomean, no
  case over 10x). `--release` selects the optimized pipeline everywhere:
  on `hd build`, `hd run` and `hd FILE` it also selects the wrapping
  release profile; on `hd test` it keeps checks (tests are always
  checked). Binaryen in release builds is decided after spike T2.
- **Strings are Go-style shared slices** (owner, 2026-10-07): a string is
  `(array, start, len)`, and `slice` / `s[a..b]` stay O(1) and share bytes
  as the spec already says (`module.string.slice.shared`). The known cost:
  a small slice keeps its whole backing array alive.
- **Map:** the internal bucket hash is an implementation detail, and
  iteration order is no longer required to be insertion order; it stays
  deterministic (same program and data, same order). No deliberate
  Go-style scrambling for now.
- The `dead-code` target is re-based after spike S7 measures real section
  sizes. Release builds omit the standard `name` section and keep the
  compact `hd.names` for backtraces.
- **GADTs are removed from the language** (chapter 13, its fixtures and
  every refinement rule). Typed request/response APIs use traits with
  associated types; typed interpreters use a runtime value enum or traits.
  This removes pattern refinement, existential variant parameters and the
  runtime path they needed (the GX research is cancelled). Applied in
  S1e: [`grammar.enum.no-result-type`](../../spec/lang/02-grammar.md#r-grammar.enum.no-result-type).
- A private item that a derive template names follows the public signature
  rules (explicit result type, `$` clause or the empty row); the private
  types it names are exported as hidden items too.
- Accepted readings of the type-checking design: cross-statement literal
  joins also cover private functions with omitted results; a GADT arm's
  outer variables bind only to types free of the arm's refinements.
- Literal widths are decided once per connected literal class; no
  per-statement retry.
- An impl head may not project an impl parameter
  (`unconstrained-impl-parameter`).
- Implementation of the new compiler runs one agent at a time.

- Bounded inlining and scalar replacement of small non-escaping values are
  in the first release, the same in debug and release builds, so the
  `runtime` and `allocations` targets can be met.
- Polymorphic recursion is a build error at the instantiation depth limit
  (`instantiation-too-deep`); there is no boxed fallback.
- A derive template may call private helpers of its trait's module; the
  interface exports them as hidden items that only expanded template code
  can call.
- The compiler process has no default memory cap (opt-in flag or
  environment variable); the shared cache defaults to 10 GB with LRU
  eviction; `hd build` on a library-only package writes only its interface.

**Decided (owner, 2026-10-06):**

- **Architecture, after [the research](research.md#open-questions-for-the-owner):**
  the compiler is written in Rust; incremental checking uses a per-module
  on-disk cache (no salsa, no daemon); generic code is generated per
  concrete type, then byte-identical functions are merged; the browser
  build is single-threaded in v1; a browser `block_on` that waits on the
  host uses JSPI, else a synchronous same-origin XHR for HTTP, else a
  `host-contract` panic; the host ABI is hd's own core-Wasm imports, not
  the Component Model; the `block_on` ban in `defer`, defaults, facts and
  module initialization becomes direct-only, with a run-time panic for an
  indirect call.

- **The compiler runs in the web playground.** It is compiled to Wasm and
  checks, builds and tests in a browser tab, as the prototype does today.
  The playground runs the user's program on the browser's engine. So the
  implementation language must target the browser, the cache needs a
  storage interface without a file system, checking must also work on one
  thread, and the host interface has a JS implementation.

- The prototype in [`src/`](../../src/README.md) is frozen as a test oracle
  ([Roadmap](../ROADMAP.md#order)); the new compiler starts now.
  See [Prototype Baselines](#prototype-baselines-to-beat-2026-10-06).
- Multiple backends: **Wasm** and **Cranelift** first, **LLVM** and **JS**
  at low priority. See [Toolchain-Wide Features](#toolchain-wide-features).
- A program database instead of a language server, after the first
  release.
- Pillar 1, pillar 3 and toolchain-wide features are triaged: day 1, v1,
  later and dropped. See [Pillar 1 features](#pillar-1-features-agent-wait-time-and-retries)
  and [Pillar 3 features](#pillar-3-features-artifact-quality).
- The first release targets Wasm only; the native Cranelift backend with
  its own GC comes right after.
- NonEscapable comes after the first release.
- No serializable closures in the first release.
- Suspension lowering reserves no-op observability and replay hook points
  from day one.
- `usize` is target-defined: 32 bits on Wasm32.
- `defer` is kept for the first release. See
  [Follow-Up Questions](#follow-up-questions).
- `hd doc` is decided: [HD_DOC.md](../HD_DOC.md).
- Host capabilities are decided: [HOST_CAPABILITIES.md](../HOST_CAPABILITIES.md).
  See [Host Capabilities](#host-capabilities-what-the-new-compiler-inherits-2026-10-06).
- `Http` is its own trait.

## How It Is Judged: The Agentic Programming Language Arena (2026-10-06)

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

This whole section is the orchestrator's proposal. Only the rules below are
the owner's.

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
| `lookup-latency` | canned program-database queries (`hd callers`, `hd needs Http`) wall time; N/A until the program database ships (Later) | p95 ≤ 100 ms |
| `fmt` | `hd fmt` time on the 10k-line package, and idempotence (if hd has a formatter) | ≤ 200 ms; idempotent |
| `release-check-cost` | runtime cost of overflow and bounds checks: the same test suite in a debug vs a release build, since agents run tests in debug | debug ≤ 1.3x release |

### Pillar 2: Agent scalability (compiler CPU and memory)

| Script | Measures | Target |
|---|---|---|
| `resources` | CPU-seconds and peak RSS for check, test and build on generated small, 10k- and 50k-line packages, cold and warm | warm check of 10k lines ≤ 0.2 CPU-s and ≤ 50 MB |
| `long-session` | RSS across 1,000 REPL inputs or incremental rechecks | flat (no growth beyond a fixed bound) |
| `startup` | `hd --version` and checking an empty file: wall time, CPU, RSS | ≤ 20 ms, ≤ 10 MB |
| `concurrency` | N = 1, 4, 16, 64 concurrent `hd check` processes; p95 latency vs N = 1, total CPU vs N | ≤ 1.5x at N = cores; total CPU sublinear in N with a shared cache |
| `disk` | artifacts per worktree; the `hd` binary (with wasmtime and Cranelift) gets its own budget once measured | ≤ 10 MB of artifacts |
| `suite-cpu` | total CPU of the conformance suite | ≤ 60 s |
| `parallel-speedup` | checking a 50k-line package on 1 core vs all cores (one process) | ≥ 0.6 × cores speedup up to 8 cores |
| `cache-contention` | N processes writing the same cache entries at once | no corruption; each entry computed once |
| `cache-growth` | cache size after a scripted day of edits; eviction | bounded by a configured cap |
| `io-per-check` | files read per warm check; with no daemon, every source file is still stat'ed to detect changes | reads proportional to what changed; stats proportional to the source file count |
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

## Features

Owner, 2026-10-06 (candidate features): "just think about features; later,
if a feature is too hard to implement, removing it is fine." Every item is in
until its cost says otherwise. Items marked ✓ exist in the prototype or are
already decided. Pillars 1 and 3 and the toolchain-wide features are
triaged; pillar 2 is deprioritized and not triaged.

### Pillar 1 Features: Agent Wait Time And Retries

Triaged with the owner, 2026-10-06. **Day 1** items shape the architecture
and would cost a rewrite to add later. **v1** items ship in the first
release. **Later** items come after v1, with any hook they need reserved
now.

**Day 1:**

- incremental checking on a query engine (must-have);
- parallel checking (must-have);
- early cutoff: an unchanged interface hash stops propagation;
- dependency bodies skipped, because explicit signatures make them
  unnecessary;
- a lossless CST parser with error recovery;
- checker recovery per item with "poison" types, so one error never hides
  or multiplies others;
- no exponential algorithms (bounded impl search, F-626; iterative passes
  for deep nesting and chains), and hard limits (recursion, instantiation
  depth, impl-search steps) instead of hangs, each with its own
  diagnostic;
- deterministic output everywhere;
- the diagnostic format: compact by default with details on request (today
  one diagnostic is 7.4 KB), JSON, and a field for exact-edit fix-its.

**v1:**

- std checked in advance and built into the binary;
- a fast debug tier (the first tier of tiered compilation): lightly
  optimized, with cheap overflow and bounds checks, for `hd run` and
  `hd test`;
- one diagnostic per root cause;
- exact-edit fix-its and `hd fix`, which applies every safe fix-it in one
  command; did-you-mean, import and `let mut` hints;
- `hd fmt`, on the same CST;
- scoped `hd check FILE`;
- `hd test --affected` (only the tests the change can reach), run in
  parallel;
- capped, grouped output (`--max-errors`, a summary mode), quiet passing
  tests, a repro command on every failure, and the seed printed on a
  failing test;
- structured assert diffs that print only the differing fields;
- typed holes: `_` / `todo()` report the expected type and the names in
  scope that fit;
- a pathology fuzzer in CI that flags superlinear growth;
- `hd doc` ✓ (HTML and Markdown, [HD_DOC.md](../HD_DOC.md)).

**Later:**

- **Program database instead of a language server** (owner: "program
  database is better than lsp in agentic world"). The compiler writes
  queryable facts, for example SQLite in `build/`: declarations,
  signatures, requirement rows, call edges, implementations, derives and
  diagnostics. Canned queries (`hd callers`, `hd needs Http`) sit on it.
  Day 1 hook: the engine keeps its facts queryable. The schema waits until
  the engine settles, because it becomes a public API.
- **The optimizing tier** of tiered compilation (optimized Cranelift, later
  LLVM). Day 1 hook: the IR boundary between tiers.
- **Module hot reload** (owner), for server and frontend programs. It needs
  a design pass after v1. Starting point, as Dart and the JVM ship: swap
  function bodies only, and restart on a signature or `data` layout
  change. Open questions: a task suspended in an old body (finish in old
  code, as Erlang does, or restart); the trigger (`hd run --hot` watching
  its own sources). Day 1 hook: dev-tier calls go through a table.
- deferred type errors: a broken item compiles to a panic, so tests that
  don't reach it still run;
- code-generating fix-its (missing match arms, `impl` stubs, `@derive(Eq)`,
  auto-imports), requirement-propagating fix-its (add `$ Clock` and carry
  it up through callers), and signature suggestions offered as explicit
  fix-its (declarations are still never inferred);
- diff-aware output ("fixed 3, new 1"), against the last run's stored
  result;
- a notebook-style REPL whose cells rerun when what they depend on
  changes;
- a language server for human editors, on the same engine.

**Dropped** (owner, 2026-10-06): `hd explain` and its worked examples;
`hd watch` / `hd dev`; background test precompilation; tests started from
a snapshot; the `hd check --fast` syntax-and-names tier (incremental
checking already covers it); project templates.

### Pillar 2 Features: Agent Scalability

Owner: "add all", 2026-10-06; **deprioritized, triage later**. Many agents on
one machine is "probably too stretched".

- **Share work instead of repeating it:**
  - a cache keyed by file content, not path, so worktrees on one commit
    share everything;
  - std and dependencies checked once per machine;
  - a shared cache of compiled machine code (wasmtime-cache or
    V8-code-cache style);
  - copy-on-write builds (hardlinks or reflinks from the cache);
  - dependency fetches shared across worktrees, with an offline mirror;
  - a remote cache later (CI warms it), using the same content-addressed
    keys;
  - **test results cached by content** (Bazel style): a test whose inputs
    already passed anywhere on the machine is skipped;
  - diagnostics cached by content;
  - one program database per commit content;
  - `hd prepare`, which pre-warms a new worktree from the base commit's
    cache.
- **Don't oversubscribe:**
  - a cross-process jobserver (a shared pool of CPU tokens, as make and
    cargo do), to stop the 9x slowdown from too many threads;
  - priority classes (interactive checks before background suites);
  - per-test-run caps (`--jobs`, heap, time).
- **Keep each process small and cheap to start:**
  - arenas per module, bodies dropped after codegen, cache files mapped
    into memory, interning per compilation run (not global);
  - lazy std loading;
  - no daemon per agent (an optional machine-wide daemon);
  - **a fork server** (Android zygote style): a warm process forks per
    command, and children share std's pages copy-on-write;
  - read-only cache files mapped into memory, so the OS shares their
    pages across processes;
  - compressed cache entries.
- **Batch and split work:**
  - `hd check --roots a b c` (several worktrees in one process);
  - test sharding (`--shard i/n`);
  - background work (lint, program database, docs) throttled when the
    machine is loaded.
- **Lighter isolation per agent:** **hd's sandbox as a container
  substitute**, made of capability grants plus resource limits plus
  Wasm/native confinement, so agent-written hd code runs without a
  container or VM per agent.
- **Visibility:**
  - `hd stats` / per-command JSON reporting CPU-seconds, peak RSS and
    cache hits, so an orchestrator can size its parallelism;
  - cache quotas and `hd cache gc`;
  - a capped cache with eviction, lock-free reads, and change detection
    by hash and mtime.

### Pillar 3 Features: Artifact Quality

Triaged with the owner, 2026-10-06, in the same tiers as pillar 1. Runtime
performance is **discussed systematically later**, so only the choices
the architecture locks in are triaged here.

**Day 1:**

- code generated per value layout (monomorphization), with `i31ref` first
  on Wasm, and an enum layout chosen per enum (GC subtypes, or a tag plus
  shared fields);
- suspension lowered to state machines, with the reserved no-op hook
  points;
- every source of nondeterminism (time, random, I/O, scheduling) goes
  through a capability or the scheduler, so simulation testing and record
  and replay can be added later;
- only reachable items are compiled (whole-program DCE and std
  tree-shaking fall out of demand-driven queries);
- the conformance suite runs on every backend (cross-backend conformance
  and differential testing).

**v1:**

- fast property tests: integrated shrinking, cases run in one instance,
  compiled `Arbitrary` generators, and a regression file of failing seeds
  that runs first (property tests ✓);
- unit and integration tests run in parallel on a pooled runner;
- counted range loops (no allocation), closures that capture nothing as
  function references, cheap `Option` (a nullable reference, or a scalar
  returned as two values), and derive templates compiled to straight-line
  code;
- cheap host calls (typed scalar imports, serde only for structured
  values, buffered console), and host calls that can suspend, driven by
  the host event loop on Wasm, for servers;
- resource limits (`--max-heap`, time or fuel), alongside capability
  grants ✓;
- debug-tier runtime checks: use of a closed handle, deadlocked
  suspension;
- symbolized crash backtraces in release builds; panic locations and
  cause chains ✓, `dbg` ✓.

**Later:**

- **deterministic simulation testing**: a whole program runs against fake
  capability providers under a seeded scheduler that varies the order of
  suspension points, and a failure replays from its seed; plus
  interleaving exploration for `all!` / `race!` (in the style of loom);
- record and replay (`hd run --record` / `--replay`) and structured
  tracing (OpenTelemetry-style), both on the reserved hooks;
- generated recording fakes for any capability trait, for example
  `@derive(Fake)` (generalizing `ScriptedHttp`);
- `hd test --coverage`, and snapshot tests with `--update`;
- `hd lint`: unused requirements (least privilege), ignored `Result`s,
  unreachable code;
- general inlining beyond the first release's bounded pass, escape
  analysis, profile-guided optimization, startup snapshots for serverless,
  SIMD: for the runtime-performance discussion (bounded inlining and
  scalar replacement moved to the first release, owner, 2026-10-07);
- data parallelism (`par_map`, parallel iterators). It raises the language
  question of real threads; until then the runtime keeps no global
  mutable state;
- `hd run --profile` (CPU flamegraph and allocation profile), `hd bench`,
  native and browser-devtools debugging with source lines, heap snapshots
  and leak detection, `wasm-opt` and `hd build --size-report`;
- **a native backend with its own GC** (Cranelift), native async I/O
  (epoll, kqueue or io_uring), and packaging as a standalone native
  binary, a WASI component, or a serverless or edge bundle.

**Dropped** (owner, 2026-10-06): `hd fuzz` (hd is memory-safe, and
property tests over the derived `Arbitrary` generators cover the rest);
per-request arenas (they need region safety, which comes with
NonEscapable after v1, and the GC covers the rest).

### Toolchain-Wide Features

Items that fit no single pillar. Triaged with the owner, 2026-10-06.

The language has more than one backend (owner, 2026-10-06): **Wasm** and
**Cranelift** first, with **LLVM** and **JS** at low priority. The first
release targets **Wasm only**, run on wasmtime (which compiles with
Cranelift); the native Cranelift backend with its own GC comes right
after.

**Day 1:**

- the `hd` CLI is itself a host on the embedding API: capability providers
  plug in through one interface, so embedding packages come later without
  splitting the CLI.

**v1:**

- Wasm output, run on wasmtime inside the `hd` binary; `hd app.wasm` ✓;
- git dependencies, `hd.sum` and workspaces ✓;
- `hd add` / `remove` / `update`.

**Later:**

- `hd build --target native | wasi | js`, with cross-compilation, each
  with its backend: a standalone native executable with the runtime linked
  in (run directly), WASI output for third-party wasmtime, edge and
  serverless hosts, and an npm package with generated TypeScript
  declarations;
- embedding APIs: a Rust crate for native hosts and an npm package for JS
  hosts, where the host supplies custom capability traits;
- hd libraries exported as Wasm components, with WIT generated from hd
  traits;
- offline and vendored mode;
- **plugins and runtime code loading**: parked until now, and needs a
  design pass later (owner, 2026-10-06).

**Deferred** (owner, 2026-10-06): an API compatibility checker (see
[API Compatibility Checking](../OPEN_ISSUES.md#api-compatibility-checking-from-the-deleted-packagesmd)),
`migrate` codemods, and a per-dependency capability audit. Whether each
becomes an `hd` command or a third-party tool is decided later; either way
it can read the public surface from `build/doc/md`, and from the program
database later.

## Architecture Direction (orchestrator's proposals, not decided)

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

### Open Decisions

- **Implementation language.** Rust leads: it has salsa, rayon, rowan and wasm-encoder.
- **Wasm engine for `hd run` and `hd test`.** V8 is mature but slow to start. Embedded wasmtime starts fast, but its Wasm GC support is maturing. The engine should sit behind an interface either way.
- **Daemon or none.** The lean is none: a native binary plus the shared cache.
- **Limits on monomorphization,** so binaries don't grow without bound.

### Correctness Tooling

- Incremental-soundness fuzzing: random edit scripts, with incremental builds compared against clean builds.
- Differential fuzzing across the new compiler, the frozen prototype and the spec examples.

### Language Note

No language change is needed. Explicit signatures and requirement rows, no
overloading, no wildcard imports, orphan rules, per-module scope, and
checked templates instead of macros already give what this architecture
needs. Two things to watch: typed-derivation templates expanded per type
(cache them per type), and cross-package coherence checks.

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
[baseline report](../../audit/compiler/baseline-2026-10-06.md), as CLI wall
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

## Inherited From The Prototype (not owner text)

Implementation notes written for the prototype, moved here when their
records were deleted (2026-10-06). The spec decides behavior; these say how.

### Literal Inference

From the Standard Library Plan's compiler handoff, for the open literal
variables of [Open Literal Width](../../spec/lang/04-type-system.md#open-literal-width):

1. Check bidirectionally first. A literal with an expected type gets its concrete type on the spot, and a binary operator checks its non-literal operand first, on either side. Only a literal with no expected type gets a variable.
2. Keep variables as integer IDs in flat per-body arrays: a union-find parent with path halving and rank, a binding (a width or none), and the first deciding span for blame. Free the arena after the body.
3. Unify in O(α). Detect a conflict at union time, with the stored blame span.
4. Sweep only what is open: a has-vars bit on interned types, and a per-body list of nodes whose types hold variables. The end-of-body sweep walks only that list.
5. Keep obligations in an append-only list of (node, kind). After the fallback, process each once and patch the result into a side table, with no argument re-check.
6. For speculation, push union-find bindings on a trail (an undo log), and roll back by popping it. Never copy checker state (the prototype's F-626 in [src/KNOWN_ISSUES.md](../../src/KNOWN_ISSUES.md)).
7. Bodies are independent: check the top-level body first, then function bodies in any order, in parallel or lazily. An edit re-checks only its body.
8. Queue a generic instantiation that meets an open variable until after the sweep, then deduplicate it through the instantiation cache by concrete types. Codegen sees only concrete types.
9. The cost is O(n·α) per body, and nothing extra for a literal with an expected type.

### Wake-Driven Host Entries

From the deferred Wake-Driven Host Entries proposal. The prototype's
suspending entry export polls until Ready and blocks the JavaScript event
loop (F-555), against
[`req.entry.busy-poll`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll).

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

### Host Capabilities: What The New Compiler Inherits (2026-10-06)

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

### What The Arena Asks Of The Compiler (orchestrator's reading, not owner text)

- **Pillar 1, development cost:**
  - Each check-edit loop is part of an agent's elapsed time, so check
    latency on a small edit matters more than full-build speed. This is
    where incremental builds pay.
  - Diagnostics that name the fix cut retries, and so cut tokens. The
    [hd writing log](../../audit/hd-writing-log.md) records which messages
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
    ([baseline](../../audit/compiler/baseline-2026-10-06.md)).

## Follow-Up Questions

<!-- Questions that the notes raise, each with a recommendation. -->

- **Cleanup that suspends** (from usability probe 6). `defer` can't make
  a bang call (`flow.defer.suspend`), so a temp file can't be removed in
  a `defer`. **Owner, 2026-10-06: keep the rule for the first release,
  and revisit it with NonEscapable.** It touches how suspension is
  lowered, so the new compiler should leave room for it.
