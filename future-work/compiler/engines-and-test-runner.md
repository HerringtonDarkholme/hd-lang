# New Compiler Design: Engines And Test Runner

Part of the [compiler design](README.md).

## 18. Execution Engines And Tiers

### 18.1 wasmtime Configuration

The columns are the two pipelines of [tiering.md](tiering.md) (owner,
2026-10-07). `hd run`, `hd test` and the REPL use the dev pipeline;
`--release` selects the optimized one, also for `hd test --release`,
which keeps the test profile's checks.

| Setting | Dev pipeline | Optimized pipeline |
| --- | --- | --- |
| `wasm_gc`, `wasm_function_references` | on | on |
| exceptions, threads, stack switching | off | off |
| compiler | Cranelift `OptLevel::None` with the single-pass register allocator, if spike T1 confirms it; else the fallback of §18.6 | Cranelift, `OptLevel::Speed`, backtracking allocator |
| collector | the default copying collector | same |
| initial GC heap | `hd run`: from the profile, default 64 MiB, decided by E9; `hd test`: the pooling allocator's small per-slot heaps, sized by `unit-test-perf` (§18.3) | the same, from the profile |
| `epoch_interruption` | on | on |
| `consume_fuel` | off (fuel is for simulation, Later) | off |
| `max_wasm_stack` | the profile's stack limit | same |
| incremental compilation cache | on, through `CacheStore` | on |
| allocation strategy | pooling (§18.3) | pooling for `hd test`; on-demand for one `hd run` |
| parallel compilation | on, capped at the `--jobs` budget | same |

Winch cannot be the dev tier: it supports no GC. Both pipelines use
Cranelift, as the research found.

### 18.2 Compile Caches

- **Precompiled modules.** `Engine::precompile_module` output is stored
  as a `cwasm` entry. A disk entry is loaded with
  `Module::deserialize_file`, which maps the file instead of copying it.
  The key holds the Wasm hash, the wasmtime version, the engine
  configuration and the target, so a config change misses.
- **A `cwasm` is trusted native code (Codex re-review N-B7).**
  `deserialize_file` is unsafe: wasmtime runs the bytes as machine code,
  and neither a checksum nor the Wasm sandbox makes a foreign file safe.
  So `cwasm` entries live only in a private native-code directory,
  `$HD_CACHE/obj/native/`, created with owner-only permissions and
  refused if another user can write it. They are never shared between
  users and never fetched from a remote cache; a remote cache, if one
  comes later, carries Wasm only. The common framed entry format (cache.md
  §5.4) does not fit `deserialize_file`, which wants the raw artifact, so
  a `cwasm` is stored raw, published by the same write-then-rename, with
  a small framed sidecar entry that holds its key and checksum. The
  checksum is checked when the file is loaded, at about 10 GB/s.
- **Per-function cache.** `Config::enable_incremental_compilation` takes
  a cache store; an adapter maps it onto `CacheStore` as entry kind
  `cranelift`, and reads and writes it only through `clpack`s, one per
  folder group of the program (cache.md §5.4). No file holds one
  function. A program whose other functions did not change
  recompiles only the changed ones. The key is the function's meaningful
  CLIF, which abstracts direct call targets
  ([wasmtime#4155](https://github.com/bytecodealliance/wasmtime/issues/4155))
  but not global indices, type immediates or constants. So emitted bodies
  hold no program-wide dense number besides call targets (stable
  numbering, codegen.md §13.8): no site immediates, getters for lazily
  initialized globals, and content hashes for key ids and type ids.
  Without that, one new panic site in a debug build missed the cache for
  about half the program. S2 of spike 0c measures each remaining index
  space (types, constant globals, `ref.func`) after insertions; S3
  measures what a cache hit costs, since wasmtime still translates every
  function to compute its key. If a hit costs more than 40 percent of a
  compile, splitting a program into one module per module group is
  raised as an owner question.

### 18.3 Instantiation

- **One `Linker` per host kind** (default profile, unit test, integration
  test), built once per process.
- **One `InstancePre` per module**: imports resolved once, so each
  instantiation skips name lookup.
- **The pooling allocator** reserves slots for instances, exchange
  buffers, tables and GC heaps up front. Its totals scale with the worker
  count. An instantiation then takes a slot instead of mapping memory, a
  few microseconds.
- **A `Store` per run or per test case**, with its own `ResourceLimiter`
  and epoch deadline. Dropping the store returns the slot.

### 18.4 The Browser Runner

- The compiler worker builds the module (§20.6) and sends the bytes, the
  `hd.runtime` section and the source map request to the program worker.
- The program worker compiles with `WebAssembly.compile`, instantiates
  with the generated glue's imports, and runs the poll and wake loop on
  its event loop. V8 starts every function in Liftoff, which suits short
  playground programs.
- **Stop** or a timeout terminates the program worker only.
- **Conformance in a headless browser** (§21.2): a page hosts the program
  worker, and a Node driver feeds it the portable runtime cases, as the
  playground's e2e test drives a page today.

### 18.5 The Reserved Hot-Reload Table

Hot reload is Later. The emitter reserves its hook now
([Q14](research.md#q14-hot-reload-later-brief)):

- an emitter switch routes every user-to-user call through one `funcref`
  table, one slot per user function, in content order;
- the type section is already canonical and deterministic (§15.3), so
  the next build gives the same layouts the same Wasm types;
- with the switch on, module storage globals are exported.

The switch is off in the first release. Turning it on changes the
`pipeline_hash` part of `code_key` and nothing else.

### 18.6 Measured Choices

These engine settings are decided by measurements, not here:

- the dev pipeline's Cranelift setting, by spike T1
  ([tiering.md §7.2](tiering.md#72-spike-0c-additions); owner,
  2026-10-07: fast dev builds with poor code allowed). The dev pipeline
  uses `None` with the single-pass allocator if that saves at least 30
  percent of Cranelift CPU, passes every runtime conformance case, and
  keeps dev code within `dev-speed` (at most 4x the optimized pipeline,
  geomean, no case over 10x). Else it uses `None` with the backtracking
  allocator if that saves at least 10 percent; else `Speed`. The earlier
  rule, that debug moves to `Speed` if it costs more than 1.3x release,
  is retired with `release-check-cost`. Caching, stable numbering, packs
  and filtered test programs remain the latency levers in any pipeline;
- the initial GC heap of `hd run`: default 64 MiB, decided by E9.
  wasmtime's copying collector decides growth against the whole heap, so
  a long-lived set near the semi-space size is re-copied on every
  collection; one report measured 337 collections and an 8.8x slowdown,
  fixed by a larger initial heap. `--max-heap` still caps it;
- a null collector for short test instances (the research's experiment):
  adopted only if it measurably cuts `unit-test-perf`.

### 18.7 As Built (W1, #243): wasmtime Runs `hd run`

Owner, 2026-10-10: the `hd` CLI must not need Node. W1 links wasmtime
and runs programs in-process behind a switch; Node stays the default.

- **Engine.** `wasmtime = "=49.0.2"` (the newest release; the feature
  audit is `audit/compiler/wasmtime-audit-2026-10-10.md`), with default
  features off and `cranelift`, `gc`, `gc-copying`,
  `parallel-compilation`, `runtime` and `std` on: no component model, no
  async, no WASI, no pooling allocator, no `cwasm` cache yet. The
  `Config` of `hd_run_wasmtime::WasmtimeEngine::new`: `wasm_gc`,
  function references, reference types, multi-value, bulk memory and
  extended-const on; tail calls, exceptions and stack switching off; the
  copying collector with a 64 MiB initial GC heap; epoch interruption on
  (each store's deadline is far away until `--time-limit` exists);
  `max_wasm_stack` 4 MiB, on a thread of its own with 12 MiB of native
  stack; parallel compilation. The dev pipeline is Cranelift
  `OptLevel::None` with the backtracking allocator (spike T1 has not
  chosen the single-pass one, which wasmtime calls not production-ready);
  `--release` selects `OptLevel::Speed`.
- **Switch.** `hd run --engine wasmtime`, `hd FILE.hd --engine wasmtime`
  and `hd FILE.wasm --engine wasmtime`, or `HD_ENGINE=wasmtime` for any
  of them; `--engine node` or no setting keeps Node. The flag is a
  development switch and is not in `hd help`. Grants (`--cap`, the
  `[capabilities]` table) are resolved and the startup refusal checked
  before either engine runs, and the wasmtime providers ask the same
  `hd_run::Grants`.
- **Host.** `hd_run_wasmtime/src/host.rs` ports `compiler/host/core.mjs`
  for `hd:rt` (`block`, `abort`, `stderr`), `hd:prim` (`prim.rs` ports
  `float.mjs`: Burger and Dybvig shortest text and exact fixed-point text
  over a small big integer; `parse_f64` checks the grammar and uses
  Rust's correctly rounded parse; case mapping is Rust's full Unicode
  mapping with the final sigma), `Console`, `ConsoleInput`, `Clock`
  (`now`, `monotonic`, `sleep`), `Args` and `Env` (grant check and the
  once-per-name notice). Each import is bound with `Linker::func_new` at
  the module's own import type, so a width difference (`usize` digits as
  `i32`) needs no second signature. The output contract is the Node
  host's: standard output is held until standard error is written or the
  run ends, a panic stub's report and status 3, an entry's own status
  (`.Err` is 1, `ExitCode(n)` is n), `internal error: <V8's trap text>`
  and status 3 for any other trap, the deadlock report, and
  `panic: host-contract: ...`. Two traps Node cannot report are spec
  panics here: a stack overflow is `stack-exhausted` (Node ends with V8's
  `RangeError` and status 1) and a refused exchange-buffer growth is
  `heap-exhausted`.
- **Waiting (`reactor.rs`).** No host call blocks on an operation that is
  intrinsically asynchronous: `sleep.start` and `write_line.start` return
  a handle (status 1), and `read_line.start` is pending too while a
  reader thread of its own reads the next line of standard input. The
  driver loop and `hd:rt` `block` wait on one condition variable for the
  earlier of the next timer deadline and a delivered line. A read that
  is aborted loses no input: a line read for it goes to the next read.
  Console writes complete at once and reach the stream when the held
  output is written, as on Node. Http and Net deliver to the same
  reactor from their client threads (§20.4); no async runtime is linked.
- **Not yet on wasmtime.** `FsRead`, `FsWrite`, `Process`, `Random`,
  `Sys`, `Http`, `Net`, the handle traits, `TestRunner` and
  `PropertyRunner`, and the conformance runtime profiles. A module that
  imports one is refused at load: ``the program uses `FsRead` (`hd:FsRead`
  `read_text.start`), which is not yet on wasmtime; run it on Node,
  without `--engine wasmtime` `` (status 101). `hd test` stays on Node.
- **Tests.** `hd_cli/tests/wasmtime_engine.rs` runs the samples and
  programs for arithmetic, panics, `.Err` from `main`, `ExitCode`, `Args`,
  `Env` under grants, float text, a suspending entry with `sleep!`, and
  console input on both engines and asserts the same standard output,
  standard error and status. `hd_driver/tests/float_text.rs` checks
  `prim.rs` against `float.mjs` on its 38,000 samples.
  `HD_CONFORMANCE_ENGINE=wasmtime` runs the driver conformance suite with
  every runtime entry program whose imports all run on wasmtime on both
  engines, judged by its wasmtime run: 129 programs, 128 identical to
  Node; the one difference is `runtime/panic/unbounded-recursion.hd`,
  which passes on wasmtime (`stack-exhausted`) and fails on Node. Five
  runtime-profile programs stay on Node.
- **Measured** (aarch64 macOS, load about 10, median of 5 runs of `hd
  FILE.wasm`, prebuilt debug modules, wall ms, Node v24.19.0):

  | Program | Node | wasmtime | wasmtime `--release` |
  | --- | ---: | ---: | ---: |
  | hello (startup) | 41 | 7 | 8 |
  | closures | 374 | 1337 | 1186 |
  | int-loop | 2397 | 1054 | 1372 |
  | list-sort | 1623 | 1293 | 546 |
  | map-count | 48 | 14 | 13 |
  | string-build | 2611 | 166 | 167 |
  | trait-dispatch | 895 | 811 | 711 |

  Startup is about 6x faster: no Node process, and Cranelift compiles a
  5 KB module in a few milliseconds. `closures` (20 million `call_ref`
  closure calls over a list) is 3.6x slower than V8, which inlines the
  closures; a 1 GiB initial GC heap did not change it, so it is code, not
  collection. The `hd` binary grows from 7.90 MB to 19.02 MB (6.57 MB to
  14.68 MB stripped). The hello module is unchanged (5,209 bytes).
- **Left for W2.** The other host traits (files under `cap-std`, process,
  random, sys, then Http and Net on client threads, as §20 plans), `hd test` on
  wasmtime (a `Linker` per host kind, `InstancePre`, the pooling
  allocator, test timeouts through epochs), the `cwasm` and incremental
  compile caches of §18.2, `--max-heap` and `--time-limit` through a
  `ResourceLimiter` and epoch deadlines, the conformance runtime profiles
  as Rust providers, making wasmtime the default, and then dropping
  Node.

## 19. Test Runner

### 19.1 Building

1. `hd check --tests` (§7.3), then the test plan: **one unit-test
   program per package**, holding every module's `tests:` cases and doc
   tests, and one program per integration test file (systems review,
   finding 2; the owner accepted the review's recommendation).
   - **Init per module.** The package program exports one init function
     per module with tests (codegen.md §13.10). A case's instance calls
     the init export of its own module, which runs exactly the init
     groups that module reaches, in D1's order. That is the set a
     per-module program initialized before, so every case still runs in
     a fresh instance after its module's initialization
     ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)).
   - **Why one program.** Programs share code entries, but each program
     collects, links, Cranelift-compiles and stores its own copy of the
     shared code: std's collections, formatting, `Debug` and the
     `std.testing` harness. With one program, shared code is collected,
     linked and compiled once.
   - **Integration tests stay one program per file.** Their environments
     differ ([Test Environments](../../spec/cli/command-line.md#test-environments)),
     and each sees only the package's public interfaces.
   - **A build error in one module's tests** (such as
     `instantiation-too-deep`) is reported for that module's cases.
     `Collect` drops the roots whose walk reached the error, and the
     other modules' cases are built and run.
2. The package's executables are built first
   ([`cli.test.builds-executables`](../../spec/cli/command-line.md#r-cli.test.builds-executables)),
   in the test profile ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
3. Each program goes through `Collect`, `Emit`, `Link` and `Precompile`
   as tasks, in parallel across programs. Warm, each program is one
   `prog_key` hit and one `cwasm` hit.
4. **`--filter` builds only the selected cases** (lowering pass). The
   filter is matched against the test plan's registration names, which
   are string literals known after checking, before collection. The
   program's roots are then the selected `TestCase` bodies plus the
   module's reachable init groups, and the selection joins the root
   description, so a filtered program has its own `prog_key` and shares
   every code entry with the full one. Its roots are the selected
   `TestCase` bodies and the init exports of their modules only, so
   `hd test --filter one` builds about what one module's program built
   before (about 800 instances at 10k lines), not the package's. That
   halves the functions to compile and look up for `test-latency`.
   `--affected` selects whole modules' cases the same way (commands.md
   §7.4). A build error that only an
   unselected case reaches, such as `instantiation-too-deep`, does not
   appear under that filter; a plain `hd test` still reports it, and a
   failure's repro command already uses `--filter`, so the case it names
   reproduces.

**Cost of the test plan** (`tests-1k`: 1,000 unit tests in 100 modules;
compile-study sizes of about 800 instances and 250 KB of code per
module's program, of which most is shared; the package program is
estimated at 5,000 to 6,000 instances and 1.5 to 2 MB; the review's Mac
I/O costs):

| Cost | 100 module programs (before) | One package program |
| --- | --- | --- |
| collect, at about 2 ms per 800 instances | 0.2 CPU-s | about 15 ms |
| link reads | 80,000 code entries, or 100 × 30 packs | about 30 packs: 2 to 4 ms |
| Cranelift, cold, at 1.5 µs per byte | 10 CPU-s with the per-function cache shared, 38 if not | 2 to 3 CPU-s |
| Cranelift lookup pass after an edit to a widely used module | 100 programs × 12 to 90 ms | one program, about 0.1 to 0.6 CPU-s |
| `cwasm` on disk | 100 to 200 MB per package state | 5 to 10 MB |
| `cwasm` loads per warm `hd test` | 100 | 1 |
| constant globals built per case instantiation | one module's, about 1,500 | the package's, an estimated 2,000 to 3,000: about 100 to 150 µs per case |

The last row is the price: every instantiation builds every immutable
constant global of the program, so a case pays for constants its module
never reads. At 1,000 cases that is about 0.1 to 0.15 CPU-s, inside the
1 ms per test budget. Slice 8 measures it; a large package can split its
program per folder with no other change.

### 19.2 Listing Cases

- Registration names are string literals, so the linker writes each
  program's cases into `hd.runtime` (§16.4): export index, name, kind
  (`it`, `it_each`, `it_prop`, doc test), the module's init export, and
  the registration's anchor, resolved to a file and line when printed
  (codegen.md §13.8, "Positions").
- `--filter` selects cases from the test plan before anything is built
  (§19.1), and the built program lists only those cases.
- **`it_each`** rows are evaluated at run time
  ([`std-testing.it-each.rows-at-run`](../../spec/std/testing.md#r-std-testing.it-each.rows-at-run)).
  The first instance of an `it_each` case runs row 0; its `row(count)`
  call tells the runner the count. The runner then schedules rows 1 to
  `count - 1`, each in a fresh instance. A filter that names `name[i]`
  still runs row 0 first to learn the count, and reports only the rows it
  names.

### 19.3 Running

```text
work queue: (program, case) in content order: program path, then registration order, then row
workers (--jobs): pull a case, then
    store = Store::new(engine, limits)              ;; pooling slot
    inst  = instance_pre.instantiate(store)         ;; imports pre-resolved
    inst.init(case.module)                          ;; the case's module's init export: the groups it reaches
    outcome = drive(inst.call_test(case))           ;; poll/wake loop; reactor only for integration tests
    drop(store)                                     ;; slot returned
results: slot per case; printed by the release cursor in content order (§6.5)
```

- **A fresh instance per case**
  ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)),
  made cheap by `InstancePre` and the pooling allocator. Starting from a
  snapshot was dropped by the owner, so module initialization runs per
  case.
- **Host setups.** A unit test gets `TestRunner` alone. An
  integration test or doc test gets the default profile, `TestRunner`, the
  test runner's `Process`, the test grant, the package directory as its
  working directory, no arguments and closed input
  ([Test Environments](../../spec/cli/command-line.md#test-environments)).
- **`temp_dir()`** makes the case's directory on first call and the
  runner removes it when the case ends
  ([`cli.test.env.temp-dir`](../../spec/cli/command-line.md#r-cli.test.env.temp-dir)).
  A unit test that never calls it costs nothing.

**Per-case overhead budget** (target: at most 1 ms per test, and 1,000
unit tests in at most 1 s warm):

| Step | Estimate |
| --- | --- |
| take a pooling slot and instantiate from `InstancePre` | 5 to 20 µs |
| `hd.init` of a small test program | 1 to 50 µs |
| call, poll, `TestRunner` calls | 1 to 5 µs |
| report into the result slot | under 1 µs |
| total per case | about 0.1 ms, before the test's own work (an estimate, not a bound) |

Warm `hd test` on 1,000 unit tests in 100 modules: process start and the
warm check fast path (about 30 ms), one mapped `cwasm` load for the
package's unit-test program (a few ms), and 1,000 cases over 8 workers. That is well under a
second **if** each case's init is small. A fresh instance repeats all
reachable initialization and constant-global allocation. At 2 ms of
init per case, 1,000 cases cost 2 CPU-s and at least 250 ms of wall
time on 8 workers. **Status: not established** (Codex review, P8). The
`unit-test-perf` benchmark fixes its module count, init work per module
and body work per test, and slice 8 measures store creation, constant
globals, `hd.init`, teardown and pool reset separately. It also counts
the executable builds that `hd test` needs first. `test-latency` (edit, then `hd test --filter one` in at most
300 ms) pays one module's check, the instances whose code keys changed,
one link, Cranelift for the changed functions, and one case.

### 19.4 Property Tests, Panics And Timeouts

**`it_prop`.** One property test is one test case, so all its generated
cases run in one instance (a first-release feature):

- The runner implements `PropertyRunner` on the host. `start` returns
  the case's `PropertyCase` through the exchange buffer; `record` is a
  scalar import, one per draw; `show` sends the input's `Debug` text.
- **A discard** is a panic before `show`
  ([`std-testing.runner.discard-read`](../../spec/std/testing.md#r-std-testing.runner.discard-read)).
  The instance is poisoned, so the runner takes a fresh instance and
  continues with the next seed. A discard costs one instantiation.
- **A failure** poisons the instance too. Shrinking runs on the host,
  over the recorded draws. Each shrink attempt replays in an instance;
  passing attempts reuse it, and a failing attempt costs a fresh one.
- **One property runs on one worker.** Its cases share one instance, so a
  case can see state that earlier cases left. Splitting the cases across
  workers would change that history and so the outcome (Codex review,
  D3). Workers run different properties in parallel instead. Within a
  property, the instance sequence is a function of the seed and the
  outcomes alone: a pass keeps the instance, a discard or failure takes a
  fresh one. So the result does not depend on `--jobs`. The
  `proptest-perf` target is per worker, so it does not need the split.
- **Regression file.** Saved streams replay first, and a new failure's
  shrunk stream is written to `__regressions__/`
  ([`std-testing.prop.regression-file`](../../spec/std/testing.md#r-std-testing.prop.regression-file)).
- **Throughput.** A simple generator's case is a few draws (each a
  scalar import of tens of nanoseconds), one `Debug` render and one short
  string crossing, and the property body. That is a few microseconds per
  case, inside the 10 µs that 100k cases per second allow, on one worker.

**`expect_panic`.** The panic's category is read from the instance's
globals (§15.5). A matching category passes the case; completion or
another category fails it.

**`timeout`.** `report_timeout(millis)` sets the store's epoch deadline
and a reactor timer, so a busy loop traps and a pending host wait is cut
at the same moment. Either fails the case with `time-limit`. A case
without a `timeout` has no deadline unless `--time-limit` sets one.

### 19.5 Reports

- **Quiet passes.** Passing cases print nothing; a summary line ends the
  run. JSON lines list every case.
- **Each failure** prints its name, the panic category and message, the
  symbolized backtrace (§15.5), and a **repro command**:
  `hd test src/billing.hd --filter "sums prices"`. A property failure adds
  its seed and the regression file path; rerunning the repro replays the
  saved stream.
- **Structured assert diffs.** `assert_equal` in `std.testing` compares
  the two values through their derived structure and builds a message
  that lists only the differing paths (`.items[2].price: 3 != 4`). It is
  std hd, so the runner only prints the message.
- **Order.** Results are printed in content order by the release cursor,
  whatever order workers finish in.

### 19.6 Tests In The Browser

The playground runs a program's tests in its program worker with the same
Wasm. The JS glue implements `TestRunner` and `PropertyRunner`, and each
case gets a fresh `WebAssembly.Instance` of the compiled module.
Instantiation in a worker is synchronous for any module size.

## 20. Porting The Node Host To Rust (M3, 2026-10-10)

The host today is `compiler/host/*.mjs` (core, float, http, net, test,
profiles, run; 2156 lines). It moves behind
`compiler/crates/hd_run_wasmtime` as wasmtime `Linker` definitions plus a
reactor, reusing two crates that already exist: `hd_host_abi` (the
capability tables, codecs, import names, handles) and `hd_run` (the
`Engine`/`Instance` traits, `Grants`, the test-runner model). The
wasmtime crate itself is not a dependency yet (owner approval pending,
SK-10); the version below is the input to that decision. Ground rules
for the port: host functions stay **synchronous** (no `async_support`,
no Tokio), the workspace lint `unsafe_code = "deny"` applies, and the
workspace today has zero third-party dependencies, so every new crate
below pays its way.

### 20.1 Provider by provider

| Provider / helper | Node API it uses | Rust replacement (version, size, why) |
| --- | --- | --- |
| `hd:rt` (`stderr`, `block`, `abort`) | `Atomics.wait` sleep loop, `process.stderr` | `std`: driver parks on `Condvar`/`mpsc::recv_timeout` (§20.3); stderr is `std::io::Write` to the sink |
| `hd:prim` (float format/parse, case) | pure ECMAScript (`float.mjs`, no `node:` import) | exact Rust port of `float.mjs` + `num-bigint 0.5` (S; replaces `BigInt` decimal scaling). `f64::log10` etc. are the same IEEE ops in the same order; any divergence is a conformance bug, caught by Q3 |
| `hd:Console`, `hd:ConsoleInput` | `Buffer`, stdout/stderr writes | `std::io` (`StdoutLock`/`StdinLock`); line buffering matches `out.push(line)` |
| `hd:Clock` (`now`, `monotonic`, `sleep`) | `Date.now`, `performance.now`, pending deadline | `std::time::{SystemTime, Instant}`; `sleep.start` is a pending op with `at = now + ms` (§20.3) |
| `hd:Random` | `crypto.randomBytes` | `getrandom 0.4` (XS, audited, no RNG state): `fill` reads bytes, `next_u64` reads 8 LE bytes |
| `hd:Sys` | `os.{arch,platform,hostname}`, `availableParallelism` | `std::env::consts::{ARCH, OS}`, `std::thread::available_parallelism`; hostname has no std API, so `gethostname 1.1` (XS) |
| `hd:Http` | `node:http/https` + a worker thread (§20.4) | `ureq 3.4` (M, blocking, rustls TLS, no runtime): redirect policy off with a manual ≤10-hop loop for per-hop grant rechecks and the 303/301/302+POST→GET rewrite; per-hop `timeout(remaining)`; bodies fully buffered (`Vec<u8>`), as today |
| `hd:Net` | `node:net`, `node:dgram`, `dns.promises`, a worker thread (§20.4) | `std::net` (`TcpListener`/`TcpStream`/`UdpSocket`) + `ToSocketAddrs` DNS on a per-instance worker thread; `Inbox` becomes `mpsc` channels with per-op reply senders |
| `hd:Args`, `hd:Env` | `process.argv`, `process.env` | `std::env::{args, vars}`; grant checks reuse `hd_run::Grants::covers_name`, path mapping reuses `resolve_path` |
| `hd:FsRead`, `hd:FsWrite` | sync `node:fs` (every call completes at once) | `std::fs`/`std::path` 1:1 (all calls are already synchronous, so they stay `Wait::Never`); `runProcess` becomes `std::process::Command` with piped stdio and the same 1 GiB cap |
| `hd:TestRunner`, `hd:PropertyRunner` | `node:fs` (regressions, snapshots, temp dirs) | `std::fs`; result JSON lines and `expect.txt`-style configs via `serde_json 1.0` (S; Q6); temp dirs via `std::env::temp_dir` + unique names (no crate) |
| `profiles.mjs` (conformance-only) | none | plain Rust closures over `put`/`Enc`, as today over `io` |
| `run.mjs` / `test.mjs` drivers | `WebAssembly.instantiate`, worker supervisor, `setTimeout` grace | `hd_cli` wiring over `hd_run_wasmtime`: one `Store` per case on the `--jobs` pool; supervisor timeouts via epoch deadlines (§20.3), no processes |

### 20.2 Exchange buffer and `hd_host_abi` codecs are reused

`hd_host_abi` is already the source of truth: the `Codec`/`Wait` tables
drive the import names (`imports_of`, `is_known_import`), and the
`Handle` type models the slot+generation scheme. The port adds the
Rust half that `generate_js_glue` is for the browser: `Enc`/`Dec`
operating on wasmtime `Memory` bytes (little-endian LEB128/zigzag/f64,
UTF-8 via `str`, same `HostContract` truncation errors), `put` writing
the framed result at offset 0 and growing with `Memory::grow`, and
completion words as LE `i32`s. The `Linker` definitions are generated
from the same tables, so a new capability touches one row, not N call
sites. `hd.wake`, `hd.init`/`hd.poll` keep their names and protocol
(§17.2, §19.3).

### 20.3 Suspension maps onto sync host calls plus epochs

The Node driver (`run` in core.mjs, `runOnce` in test.mjs) ports 1:1 to
a Rust reactor owning the `Store`: `init`, then `poll` until the status
is not `-1`, running due completions, calling `hd.wake(n)`, and parking
with the next deadline. `block_on` parks the calling thread
(`recv_timeout`), so wasmtime's `async_support` stays **off** and Tokio
is not a dependency: from the program's view every `.start` still
completes synchronously, exactly as the worker-thread blocking does
today. Timeouts use epoch interruption (`Config::epoch_interruption`,
already on in §18.1 and assumed by §19.4): a timer thread calls
`Engine::increment_epoch`, and `report_timeout`/`--time-limit` set
`Store::set_epoch_deadline` plus a reactor deadline, so a busy loop
traps and a parked host wait is cut at the same moment. `deadlock`,
`TimeLimit`→status 3, and the `stderr` panic lines keep their bytes, so
`hd test` output does not change.

### 20.4 Http/Net client threads and grants carry over

- **Http** keeps one lazily started client thread per process (as
  `httpClient` today): the program thread sends the request over `mpsc`
  and blocks on the reply. The manual redirect loop re-resolves and
  re-checks `Grants::covers_host` per hop (already unit-tested in
  `hd_run`), preserving the `*.suffix`/`:port` semantics.
- **Net** keeps one worker thread per host instance owning all sockets
  in a `handles` map, with `Inbox` ported to channels; `read`/`accept`/
  `receive` hold until data arrives, `close` marks disposed, async socket
  errors finish sticky errors. Grant checks stay on the program thread
  before posting (`connect`/`listen`/`bind_udp`/`send_to` re-checked per
  datagram, `lookup` by name only), reusing `covers_host`/`covers_name`.
- `stop` (today `worker.terminate`) becomes joining the threads on host
  `close`; in-flight blocking socket calls are woken by closing the
  socket first, as today.

### 20.5 Dependency budget

The `hd` release binary is 7.5 MiB today. Estimated additions (to be
measured at link time; the wasmtime row dominates and is the SK-10
decision):

| Crate | Version | Size | Why |
| --- | --- | --- | --- |
| `wasmtime` | 49.0 (stable; CLI spike used 49.0.2) | L (~100 transitive crates, Cranelift) | the engine; sync `Linker`/`Store`/epoch APIs cover §20.2-20.4 with no async runtime |
| `ureq` | 3.4 | M (rustls, no Tokio) | blocking HTTP matching the sync model; manual redirects for grant rechecks |
| `serde_json` | 1.0 | S | test-runner JSON lines and JSON configs (Q6) |
| `num-bigint` | 0.5 | S | `float.mjs` decimal scaling (Q3) |
| `getrandom` | 0.4 | XS | `hd:Random` without RNG state |
| `gethostname` | 1.1 | XS | `hd:Sys.hostname` (no std API) |

Everything else is `std`. Deliberately absent: Tokio (no async host
calls), reqwest/hyper (async runtimes or larger trees than `ureq` for
the same buffered semantics), `rand` (no distribution sampling; seeds
are discussed in Q4), TLS crates beyond what `ureq` pulls (Net is
plain TCP/UDP, as today).

### 20.6 Questions with a recommendation

- **Q1. Sync host calls only?** Yes (recommended): the Node host already
  presents synchronous `.start` semantics via worker-thread blocking;
  sync `Linker` functions plus a driver thread preserve them with no
  Tokio. Revisit only if a future capability needs true async I/O
  (e.g. streaming bodies).
- **Q2. `ureq` vs `reqwest::blocking` vs `hyper`?** `ureq` (recommended)
  for the smallest tree that fits the blocking model. Open
  implementation checks: `Host`-header override, raw header
  order/repeat preservation, and disabling connection reuse to match
  `agent:false`; if `ureq` cannot do any of them, fall back to
  `reqwest::blocking` (larger tree, Tokio inside, same sync surface).
- **Q3. Float bit-fidelity?** Port `float.mjs` op-for-op and add a
  differential conformance run (Node vs Rust over the std float corpus)
  before switching `hd test` to the Rust host; any divergence fails the
  gate.
- **Q4. Property-test PRNG?** `test.mjs` draws from seeded streams that
  replay from regression files. Recommended: hand-roll SplitMix64 in
  `hd_run` (30 lines, no dependency, identical streams on every
  engine) rather than adding `rand`/`rand_chacha`.
- **Q5. Net parity details?** Confirm at implementation: listen backlog
  (`std` 128 vs Node 511 default), IPv4/IPv6 dual-stack and
  `verbatim:true` DNS parity, and `read(max == 0)`/spillover edge
  semantics, each with a conformance probe.
- **Q6. `serde_json` or hand-rolled JSON?** `serde_json` (recommended):
  the test protocol is flat JSON lines and the team already audits
  small trees; hand-rolling saves kilobytes and risks divergence.
- **Q7. Pin wasmtime 49.0?** Yes (recommended): it is the newest stable
  that validates every module the emitter produces (M2 spike), pending
  the SK-10 linkage approval already recorded in `hd_run_wasmtime`.

Recommendation: approve the sync-only design with the §20.5 crate set
on wasmtime 49.0, and start with the reactor + `hd:rt`/`hd:Console`/
`hd:Clock` providers (no new dependencies), then Http/Net, then the
test-runner supervisor.
