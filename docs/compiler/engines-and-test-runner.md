# New Compiler Design: Engines And Test Runner

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](ARCHITECTURE.md). The runtime and host
interface are in [crates/hd_run_wasmtime/DESIGN.md](../../crates/hd_run_wasmtime/DESIGN.md).

## 18. Execution Engines And Tiers

Two engines run a built program: V8 through Node (`hd_command::node`, the
default) and wasmtime in-process (`hd_run_wasmtime`). `--engine NAME`, or
else `HD_ENGINE`, picks one. wasmtime is the `wasmtime` cargo feature of
`hd_command`, off by default; the REPL runs on wasmtime only. Making wasmtime
the default: not built yet; see #19.

### 18.1 wasmtime Configuration

The columns are the two pipelines of [docs/compiler/tiering.md](tiering.md).
`hd run`, `hd test` and the REPL use the dev pipeline; `--release`
selects the optimized one, also for `hd test --release`, which keeps the
test profile's checks. `hd_run_wasmtime::wasmtime_config` and
`EngineConfig::pipeline` hold these settings.

| Setting | Dev pipeline | Optimized pipeline |
| --- | --- | --- |
| multi-value, bulk memory, extended-const | on | on |
| `wasm_gc`, tail calls | off | off |
| compiler | Cranelift `OptLevel::None`, backtracking allocator | Cranelift `OptLevel::Speed`, backtracking allocator |
| memory | linear memory, reference counts and the cycle collector of crates/hd_wasm/DESIGN.md §23 | same |
| `epoch_interruption` | on | on |
| `consume_fuel` | off (fuel is for simulation, Later) | off |
| `max_wasm_stack` | 4 MiB, on a thread with 8 MiB more native stack | same |
| allocation strategy | pooling for `hd test` (§18.3); on-demand otherwise | same |
| parallel compilation | caller-selected, within `--jobs` | caller-selected, within `--jobs` |

- **Test heaps start empty.** A pooled slot reserves no fixed initial
  heap before its case allocates anything. This is better than the
  design's small fixed per-slot heaps.
- **Parallel compilation.** Single-program CLI runs and a package's
  single test-module compile use the `--jobs` budget. Conformance and
  fixture pools compile serially within each module, since their callers
  already parallelize across small programs. The caller chooses; module
  size does not. Scoped compiler workers start only on an in-process
  module-cache miss, and inherit that compile's function-cache session.
  A disk function-pack hit still translates the module and uses those
  workers; it is not an in-process module-cache hit.
- Winch cannot be the dev tier: it supports no GC.

### 18.2 Compile Caches

- **Modules, in process only.** `hd_run_wasmtime::cache` keeps compiled
  modules for the life of the process: a 128-entry LRU per engine, keyed
  by the module bytes. Loading a serialized module needs `unsafe`, which
  the crate denies, so no module is written to disk.
- **Per-function machine code** (`hd_run_wasmtime::code_cache`): Cranelift's
  incremental compilation cache, kept as `clpack` entries of the compiled
  cache. The key is the function's CLIF, which abstracts direct call
  targets but not global indices, type immediates or constants, so emitted
  bodies hold no program-wide dense number besides call targets
  ([Numbering](lowering-catalog.md#numbering)).
- **One pack per module.** wasmtime hands the adapter one function at a time with no program context, so a `clpack`
  holds every function of one module, keyed by the module's bytes and
  `Engine::precompile_compatibility_hash` (target, Cranelift flags,
  tunables, features, wasmtime version). Each function inside is found by
  Cranelift's own key. A warm `hd run`, `hd test` or REPL session
  compiles no function; a module that changed misses whole, so an edit
  recompiles every function of its module (functions identical within a
  module still compile once).
- **Wasm is still translated.** Only Cranelift's code generation is
  skipped; parsing, validation and the CLIF translation run every time.
- **Private entries.** A pack is machine code wasmtime runs, so it is
  published with mode 0600 and read only when the reader owns it, it has
  one link and no group or other permission, and the path is no symbolic
  link (`DiskStore::get_private`). Platforms without Unix permissions
  keep no packs.
- **The inlining pipeline, inlining nothing.** Cranelift 0.136's key also
  covers an exception table that a reused compiler context keeps from an
  earlier trampoline, and a compile clears it while a cache hit does not.
  So with wasmtime's default pipeline, which translates and compiles one
  function after another in one context, a warm compile looks up other
  keys than the cold compile wrote. With the cache on, the engines use
  `Inlining::Intrinsics` (no Wasm function is inlined, so code and
  backtraces stay the same), whose pipeline translates every function
  before it compiles any. Compiles running at once on one engine still
  share its context pool, so there a function can have a few keys; a
  pack keeps each key a compile used within its last eight writes. A
  process that compiles one module at a time (`hd run`, `hd test`, the
  REPL) compiles no function on its second run; the wasmtime conformance
  run (eight workers on shared engines) still compiled 2.4% of its
  functions on its fourth.

### 18.3 Instantiation

- **An `InstancePre` per module and host kind** (`TestEngine::prepare`):
  imports resolved once, so each instantiation skips name lookup. The
  host kinds are unit, integration and doc (with its runtime profile).
- **The pooling allocator** (`TestEngine::new`) reserves one slot per
  worker plus one spare per in-flight property shrink, each with one
  set of linear memories (up to three) and one table. An instantiation takes a
  slot instead of mapping memory.
- **A `Store` per run or per test case**, with its own epoch deadline.
  The host sets no heap limit of its own (`hd_run_wasmtime::limits`).
  Dropping the store returns the slot.
- A test's named child programs compile on an ordinary on-demand engine,
  so a running child cannot take a slot its waiting parent case needs.

### 18.4 The Browser Runner

The playground's compiler worker (`website/playground/src/worker.ts`)
builds the module. The run worker (`run-worker.ts`) runs it:
`WebAssembly.compile`, `instantiate` with the JS host's imports
(`host/web.ts`), and the poll and wake loop on that worker's event loop.
V8 starts every function in Liftoff, which suits short programs.

**Stop** terminates the run worker alone, so the compiler stays warm, and
the next run loads a fresh run worker. A module that imports a trait the
browser does not provide is refused before it starts.

### 18.5 The Reserved Hot-Reload Table

The design record in #182 is closed; no implementation ticket is open.

### 18.6 Measured Choices

- The dev pipeline's Cranelift setting is chosen by spike T1
  ([docs/compiler/tiering.md §7.2](tiering.md#72-spike-0c-additions)),
  which has not run. Until then dev uses `None` with the backtracking
  allocator.

### 18.7 As Built: The wasmtime Host

- **Host traits.** wasmtime runs the default profile's `Console`,
  `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`, `FsWrite`, `Process`,
  `Random`, `Sys`, `Http`, `Net` and their handle traits, the `hd:rt` and
  `hd:prim` imports, `TestRunner`, `PropertyRunner`, and the conformance
  runtime profiles (`profiles.rs`). A module that imports any other trait
  is refused at load with a message naming it, except a package host trait
  its `hd.capabilities` contract describes: instantiation refuses that one
  unless an embedder registered a native provider for it.
- **Output contract.** Standard output is held until standard error is
  written or the run ends. A panic prints its report and exits with
  status 3; an entry's `.Err` is status 1 and `ExitCode(n)` is n. A stack
  overflow is `stack-exhausted` and a refused allocation `heap-exhausted`,
  which Node cannot report.
- **Waiting (`runtime/reactor.rs`, `runtime/io.rs`).** No host call blocks. A
  `.start` that cannot finish at once returns a handle. Timers and console
  writes complete in the reactor; a reader thread delivers standard input;
  `Process`, `Net`, `Http` and special-file operations run as tasks on one
  process-wide current-thread tokio runtime (the `hd-io` thread, started
  lazily) and deliver into their instance's reactor inbox. The driver loop
  and `hd:rt` `block` wait on a condition variable for the next timer or
  delivery.
- **Native behavior, not Node's.** Providers use Rust's native errors and
  keep the spec's error variants, grants and deadlines; they do not copy
  Node's messages. A UTF-8 BOM stays file content
  ([`std-fs.read.text`](../../spec/std/fs.md#r-std-fs.read.text)).
- **Conformance.** `HD_CONFORMANCE_ENGINE=wasmtime` runs each runtime
  entry program on both engines and judges the wasmtime run;
  `wasmtime-only` runs in-process alone, never falling back to Node. CI
  runs the `wasmtime` mode.

## 19. Test Runner

### 19.1 Building

1. `hd check --tests`, then the test plan: **one unit-test program per
   package**, holding every module's `tests:` cases and doc tests, and one
   program per integration test file (`hd_driver::test_programs`).
   - **Init per module.** The unit-test program exports `hd.test.init(j)`,
     which runs exactly the init groups module `j` reaches, and
     `hd.test(i, ctx)`, which polls case `i` (`hd_wasm` emission). Each
     dispatches by a `br_table`, so an instance holds two test exports
     however many cases and modules it has: an engine that builds an
     exports object per instantiate does constant work there. Over 256
     cases or modules the dispatch splits into a tree of functions of at
     most 256 arms each (`hd_wasm` DESIGN.md §23.12), so no function's
     optimizing compile grows with the suite.
   - **Node version.** The host refuses a Node older than the repository
     `package.json`'s `engines.node`, and `hd test` reports that alone. A case's
     instance calls its own module's init, so every case runs in a
     fresh instance after its module's initialization
     ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)).
   - **Why one program.** Each program collects, links and compiles its
     own copy of the shared code: std's collections, formatting, `Debug`
     and the `std.testing` harness. With one program, shared code is
     collected, linked and compiled once.
   - **Integration tests stay one program per file.** Their environments
     differ ([Test Environments](../../spec/cli/command-line.md#test-environments)),
     and each sees only the package's public interfaces.
   - **A build error in one module's tests** (such as
     `instantiation-too-deep`) is reported for that module's cases;
     the other modules' cases are built and run.
2. The package's executables are built first
   ([`cli.test.builds-executables`](../../spec/cli/command-line.md#r-cli.test.builds-executables)),
   in the test profile ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
3. **`--filter` builds only the selected cases.** The filter is matched
   against the test plan's registration names, known after checking and
   before collection. The program's roots are the selected cases plus
   their modules' init exports, so `hd test --filter one` builds about
   one module's worth of code, not the package's. A build error that only
   an unselected case reaches does not appear under that filter; a plain
   `hd test` still reports it. `--affected` selects whole modules' cases
   the same way (crates/hd_command/DESIGN.md §7.4).

The price of one program: every case's fresh instance pays for the
whole program, not its own module. On Node, `WebAssembly.instantiate`
allocates per-instance arrays sized by the program's defined function
count: measured on Node 26.13.1 (Apple arm64), about 27 µs plus 5.2 ns
per defined function per instance (#2035). A suite of K cases defines at
least one function per case, so it pays K × functions, quadratic in the
suite's size: about 104 µs per case at 20,000 cases. wasmtime's
`InstancePre` instantiate has no per-function cost. Every instance also
builds every immutable constant global of the program, so a case pays
for constants its module never reads. The dispatch tree above (#2049)
bounds each function's size, not the instance's cost.

The decided fix (#2035) chunks the unit-test program into programs of
bounded function count, still a fresh instance per case. Each extra
chunk program costs about 35 ms to build, which prices a chunk at about
2,048 to 4,096 cases: below that, one program stays cheaper, so suites
of that size keep one program. Splitting needs shared code compiled once
and linked into every chunk: relocatable per-instance code, with each instance's recorded reads
([Backend-Neutral TIR](../future-work/BACKEND_NEUTRAL_TIR.md); its unit is
the lowered body, after #49 point 4). Without it, each chunk would
collect and compile its own copy of std, which is what "Why one program"
avoids, so chunking waits for that code cache.

### 19.2 Listing Cases

- The `hd.runtime` section lists each case's export, name, kind (`it`,
  `it_each`, `it_prop`, doc test), file and line (`hd_wasm::meta`). The
  test plan (`hd_run::tests_model::TestPlan`) lists the cases in content
  order, after `--filter`.
- **`it_each`** rows are evaluated at run time
  ([`std-testing.it-each.rows-at-run`](../../spec/std/testing.md#r-std-testing.it-each.rows-at-run)).
  The first instance of an `it_each` case runs row 0; its `row(count)`
  call tells the runner the count. The runner then runs rows 1 to
  `count - 1`, each in a fresh instance.

### 19.3 Running

```text
work queue: (program, case) in content order
workers (--jobs): pull a case, then
    store = new store (pooling slot, limits)
    inst  = instance_pre.instantiate(store)
    inst.init(case.module)              ;; hd.test.init(j): the groups its module reaches
    outcome = drive(inst.call_test(case))
    drop(store)                         ;; slot returned
results: printed by the release cursor in content order
```

On Node, one process supervises the command's test programs (`host/test.ts`).
It compiles each module once and sends the `WebAssembly.Module` to fresh
worker threads by structured clone. A single `--jobs` budget schedules
case chunks across programs; each chunk keeps its property cases together.
Results carry their program index to the CLI's existing release cursor.
The process runs in the package directory, while each case keeps its own
grants, arguments, seed, snapshot paths and temporary directory.

- **A fresh instance per case**
  ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)),
  made cheap by `InstancePre` and the pooling allocator.
- **Host setups.** A unit test gets `TestRunner` alone. An integration
  test or doc test gets the default profile, `TestRunner`, the test
  runner's `Process`, the test grant, the package directory as its working
  directory, no arguments and closed input
  ([Test Environments](../../spec/cli/command-line.md#test-environments)).
- **`temp_dir()`** makes the case's directory on first call, and the
  runner removes it when the run ends
  ([`cli.test.env.temp-dir`](../../spec/cli/command-line.md#r-cli.test.env.temp-dir)).
- **Budget.** At most 1 ms of overhead per test, and 1,000 unit tests in
  at most 1 s warm (`unit-test-perf`). The estimate is about 0.1 ms per
  case before the test's own work, if each case's init is small; a fresh
  instance repeats all reachable initialization. Not established by
  measurement.

### 19.4 Property Tests, Panics And Timeouts

**`it_prop`.** One property test is one test case:

- The runner implements `PropertyRunner` on the host (`property.rs`).
  `start` returns the case's `PropertyCase` through the exchange buffer;
  `record` is a scalar import, one per draw; `show` sends the input's
  `Debug` text.
- **A pass keeps the instance** for the next generated case. **A discard**
  (a panic before `show`,
  [`std-testing.runner.discard-read`](../../spec/std/testing.md#r-std-testing.runner.discard-read))
  or **a failure** poisons it, and the runner takes a fresh one. Shrinking
  runs on the host over the recorded draws.
- **One property runs on one worker.** Its cases may share an instance,
  so splitting them across workers would change their history and so the
  outcome. Workers run different properties in parallel instead, and the
  result does not depend on `--jobs`.
- **Regression file.** Saved streams replay first, and a new failure's
  shrunk stream is written to `__regressions__/`
  ([`std-testing.prop.regression-file`](../../spec/std/testing.md#r-std-testing.prop.regression-file)).
  The write runs after the guest finishes, outside the body's deadline, as
  a new file renamed over the old one.

**`expect_panic`.** The panic's category is read from the instance's
globals, and a matching category passes the case. Implementation
differs: the emitter exports no category or message globals. A panic
stub calls the `hd:rt` `panic` import before it writes its report, and
the host records the report's text from that call to the trap; a host
panic (`time-limit`, `heap-exhausted`, a deadlock) records its own. A
result carries that category and message (`PanicInfo`; the Node
record's `category` and `message`), and `expect_panic`, a property's
discard and the conformance harness judge them, never standard error,
so a message that looks like a report line keeps its own category.

**`timeout`.** `report_timeout(millis)` sets the store's epoch deadline
and a reactor timer, so a busy loop traps and a pending host wait is cut
at the same moment. Either fails the case with `time-limit`. A case
without a `timeout` has no deadline unless `--time-limit` sets one; with
both, the earlier wins.

### 19.5 Reports

- **Quiet passes.** Passing cases print nothing; a summary line ends the
  run. `--format json` writes one record per case.
- **Each failure** prints its name, the panic category and message, the
  frames from `hd.lines` (crates/hd_wasm/DESIGN.md §15.9), and a **repro
  command**: `hd test src/billing.hd --filter "sums prices"`. A property
  failure adds its seed; rerunning the repro replays the saved stream.
- **Order.** Results are printed in content order by the release cursor,
  whatever order workers finish in.

Planned, not built: structured assert diffs, where a failed
`assert_equal` lists only the differing paths (`.items[2].price: 3 != 4`).
Today it shows both values as `debug` renders them
([`module.testing.assert-equal-debug`](../../spec/lang/10-modules.md#r-module.testing.assert-equal-debug)).

### 19.6 Tests In The Browser

The playground runs a program's tests in its compiler worker with the same
case loop as `hd test` on Node (`host/cases.ts`). The JS host implements
`TestRunner` and `PropertyRunner`. Implementation differs: every body
run, property cases included, gets a fresh `WebAssembly.Instance`, where
§19.4 keeps the instance after a pass. Not judged: results can differ
only for a property whose cases leave state behind.

## 22. Resource Limits

The spec's resource limits (`cli.limit.*`, `flow.panic.heap-exhausted`,
`flow.panic.time-limit`) on wasmtime:

- **`--time-limit`.** A shared 10 ms epoch ticker (`runtime/epochs.rs`) and an
  absolute wall deadline per store; a wall-clock check stops an early tick
  from expiring a fresh store. Init, polling and reactor waits share the
  deadline, as do Http, Net and Process waits. Expiry is
  `panic: time-limit`, status 3. In `hd test` it applies per case and
  composes with `report_timeout`. Epochs cannot interrupt one blocked
  kernel call, such as a file read.
- **Heap.** No flag and no limit of the host's own. A linear-memory or
  GC growth that the engine cannot satisfy panics `heap-exhausted`.
- **Stack.** `max_wasm_stack` from the profile traps as `stack-exhausted`;
  there is no stack flag.
- **On Node,** `--time-limit` is enforced by the Node host.
