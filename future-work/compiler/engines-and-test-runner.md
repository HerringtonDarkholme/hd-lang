# New Compiler Design: Engines And Test Runner

Part of the [compiler design](README.md).

## 18. Execution Engines And Tiers

### 18.1 wasmtime Configuration

| Setting | Debug and test | Release |
| --- | --- | --- |
| `wasm_gc`, `wasm_function_references` | on | on |
| exceptions, threads, stack switching | off | off |
| compiler | Cranelift; the level is provisional, pending the tiering design, with S4's numbers (§18.6) | Cranelift, `OptLevel::Speed` |
| collector | the default copying collector | same |
| initial GC heap | `hd run`: from the profile, default 64 MiB, decided by E9; `hd test`: the pooling allocator's small per-slot heaps, sized by `unit-test-perf` (§18.3) | the same, from the profile |
| `epoch_interruption` | on | on |
| `consume_fuel` | off (fuel is for simulation, Later) | off |
| `max_wasm_stack` | the profile's stack limit | same |
| incremental compilation cache | on, through `CacheStore` | on |
| allocation strategy | pooling (§18.3) | pooling for `hd test`; on-demand for one `hd run` |
| parallel compilation | on, capped at the `--jobs` budget | same |

Winch cannot be the debug tier: it supports no GC. Both tiers are
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
  `cranelift`, and reads and writes it per program through a `clpack`
  (cache.md §5.4). A program whose other functions did not change
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

The switch is off in the first release. Turning it on changes the tier
part of `code_key` and nothing else.

### 18.6 Measured Choices

These engine settings are decided by measurements, not here:

- the debug tier's Cranelift level: **provisional, pending the tiering
  design** (codegen.md §12.6; owner, 2026-10-07: fast dev builds with
  poor code allowed). S4 of spike 0c gives the numbers. The two
  representation studies proposed `Speed` by default: register
  allocation dominates Cranelift's time at either level, so `None` is
  expected to save only 10 to 25 percent, while its slower code eats the
  1.3x `release-check-cost` budget that overflow checks already use.
  Their rule was `None` only if it saves at least 25 percent of compile
  CPU and debug stays within 1.3x release, and the single-pass register
  allocator only if its runtime cost is at most 1.1x and it saves at
  least 30 percent. Caching, stable numbering, packs and filtered test
  programs are the latency levers in any tier;
- the initial GC heap of `hd run`: default 64 MiB, decided by E9.
  wasmtime's copying collector decides growth against the whole heap, so
  a long-lived set near the semi-space size is re-copied on every
  collection; one report measured 337 collections and an 8.8x slowdown,
  fixed by a larger initial heap. `--max-heap` still caps it;
- a null collector for short test instances (the research's experiment):
  adopted only if it measurably cuts `unit-test-perf`.

## 19. Test Runner

### 19.1 Building

1. `hd check --tests` (§7.3), then the test plan: one program per module
   with unit tests, one per integration test file, one per module's doc
   tests.
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
   every code entry with the full one. That halves the functions to
   compile and look up for `test-latency`. A build error that only an
   unselected case reaches, such as `instantiation-too-deep`, does not
   appear under that filter; a plain `hd test` still reports it, and a
   failure's repro command already uses `--filter`, so the case it names
   reproduces.

### 19.2 Listing Cases

- Registration names are string literals, so the linker writes each
  program's cases into `hd.runtime` (§16.4): export index, name, kind
  (`it`, `it_each`, `it_prop`, doc test), file and line.
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
    inst.init()                                     ;; hd.init: this program's init groups
    outcome = drive(inst.call_test(case))           ;; poll/wake loop; reactor only for integration tests
    drop(store)                                     ;; slot returned
results: slot per case; printed by the release cursor in content order (§6.5)
```

- **A fresh instance per case**
  ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)),
  made cheap by `InstancePre` and the pooling allocator. Starting from a
  snapshot was dropped by the owner, so module initialization runs per
  case.
- **Host setups.** A unit test case gets `TestRunner` alone. An
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
warm check fast path (about 30 ms), 100 mapped `cwasm` loads (about 1 ms
each, in parallel), and 1,000 cases over 8 workers. That is well under a
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
