# Goal metrics audit, 2026-11-13

What the missed goal metrics show about the three pillars, traced to causes
in the code. Each cause is meant to become one issue, per the AGENTS.md
rule "Performance Work: General Causes, Not Cases".

|                |                                                                                           |
| -------------- | ----------------------------------------------------------------------------------------- |
| Run            | CI, 2026-11-13 04:12 UTC, `metrics-data` branch file `daily/2026-11-13T04.json`           |
| Commit         | `8d928bfcf`                                                                               |
| Machine        | GitHub `ubuntu-latest`, 4 CPUs, AMD EPYC 7763                                             |
| Page           | [metrics.html](https://herringtondarkholme.github.io/hd-lang/metrics.html)                |
| Targets        | [docs/compiler/goals.md](../compiler/goals.md), Goal Metrics                              |
| Extra evidence | the installed `hd` (built 2026-11-10) run on copies of the mistakes corpus, for fix-its   |

**Verified** means the cause was read in the code at the cited place.
**Suspected** means the code fits the symptom, but nobody has profiled it.

## Counts

| Pillar                         | Met      | Missed |
| ------------------------------ | -------- | ------ |
| Correctness gate               | 3 of 3   | 0      |
| 1. Single-agent cost           | 45 of 56 | 11     |
| 2. Agent scalability           | 29 of 39 | 10     |
| 3. Artifact quality            | 24 of 35 | 11     |

Three misses are harness problems, not compiler problems (see
[Harness problems](#harness-problems)). The rest come from 15 causes.

## Ranked Causes

Ranked by how many misses a cause explains and how far they are from target.

| #  | Cause                                                              | Pillar | Misses it explains                                         | Status    |
| -- | ------------------------------------------------------------------ | ------ | ---------------------------------------------------------- | --------- |
| C1 | No cross-process single-flight in the two caches                   | 2      | cache-contention ×2, concurrency ×3                         | verified  |
| C2 | Most diagnostics carry no structured fix-it                        | 1      | fix-it resolves 33%                                        | verified  |
| C4 | No early cutoff: a module's key hashes its whole closure           | 1      | recheck-precision 9 vs 3                                   | verified  |
| C5 | REPL input N rechecks the chain of all N earlier inputs            | 2      | long-session RSS +96 MB, build 89 ms → 982 ms per input    | verified  |
| C6 | No integer range facts, so provably safe `+ 1` keeps its check     | 1      | check-cost 1.93x                                           | verified  |
| C7 | Poisoned match arms still cascade                                  | 1      | largest diagnostic 92 tokens, some-on-result               | verified  |
| C8 | Release binary not size-tuned; std embedded as text               | 2      | disk 20.1 MB, startup RSS 17 MB                            | suspected |
| C9 | No timing trace, so the parallel gap has no named cause            | 2      | parallel-speedup 1.92x                                     | open      |
| C10 | A Node process and an uncached compile per run                    | 3      | start to first output 56 ms, integration and doc tests ×2  | verified  |
| C11 | A fresh host and instance per unit test case                      | 3      | 1,000 unit tests 1,130 ms                                  | verified  |
| C12 | Property tests cross the host boundary on every draw              | 3      | 23k cases/s                                                | verified  |
| C13 | A JS host ABI that decodes and re-encodes every call              | 3      | host-call-overhead ×3                                      | verified  |
| C14 | JSON makes two passes and checks keys at run time                 | 3      | serde-throughput ×2                                        | verified  |
| C15 | Regex search always runs the Pike VM                              | 3      | regex find_all 0.09x                                       | verified  |

Suggested order: C1, C2 and C10 give the most targets per unit of
work. C4 and C5 share the early-cutoff machinery. C13 and C12 share the
host ABI.

## Pillar 1: Single-Agent Cost

### C2. Most diagnostics carry no structured fix-it

- **Miss.** `mistakes`: fix-it makes `hd check` pass in 23 of 69 (target
  80%). Every fix-it offered resolves its mistake and is safe
  (`fixit-safety` 23 of 23). So the gap is coverage, not correctness.
- **Evidence.** 46 of 69 corpus programs get no `fixes` array. By code:
  12 `syntax-error`, 6 `type-mismatch`, 3 `missing-requirement`, 2 each of
  `unknown-name`, `invalid-result-propagation`, `integer-literal-range` and
  `cannot-infer-type`, and 14 single codes.
- **Cheapest win.** About 15 messages already state the edit in prose, with
  no edit attached:

  | Corpus file                      | Message already says                         |
  | -------------------------------- | -------------------------------------------- |
  | `bound-without-import`           | add `use std.num.Integer`                    |
  | `mut-before-short-binding`       | write `let mut name = ...`                   |
  | `unwrap-on-result`               | did you mean `unwrap_or`?                    |
  | `main-without-result-type`       | write `-> T`                                 |
  | `println-without-console`, `println-in-trait-method` | the row does not name `$ Console` |
  | `use-inside-function`            | `use` goes at the top of the file            |
  | `same-line-if-in-same-line-body` | put the body on an indented line             |
  | `list-plus-list`                 | concatenate with `[keys..., [key]...]`       |
  | `unknown-import`                 | (`assert_eq`, std has `assert_equal`)        |
  | `literal-out-of-range`, `literal-too-wide-for-usize` | (a wider type is the repair) |

  Turning those into edits gives about 38 of 69 (55%). The rest of the way
  to 80% needs type-directed repairs: `i32(x)` for `length-as-i32`, `?` for
  `result-into-optional`, an explicit type argument for
  `uninferable-generic-call`.
- **State of the art.** rustc attaches a `MachineApplicable` suggestion to
  most "help:" lines (`Applicability` in `rustc_errors`); clang emits
  fix-it hints for its "did you mean" notes. The rule there: a hint in
  prose that is one unambiguous edit is a structured fix.
- **Fix.** One pass over the diagnostic sites in `hd_check` and
  `hd_syntax`: every message that names a single edit also emits it as a
  fix. Messages with two options (`mixed-signedness`, `mut-on-binding-type`)
  stay prose or offer both as alternatives, which the metric skips.

### C4. No early cutoff: a module's key hashes its whole closure

- **Miss.** `recheck-precision`: a public signature edit rechecks 9
  modules (`m004` and every transitive dependent); the target is `m004`
  plus its direct dependents, 3.
- **Evidence (verified).** `check_key(m)` hashes `(folder path, deep_hash)`
  for each folder in `closure(m)` (`crates/hd_cache/DESIGN.md` §5.3). A
  changed interface anywhere below therefore changes every key above it.
  Early cutoff by recorded reads is designed but "not built yet; see #49"
  (§5.3.1).
- **State of the art.** GHC recompiles a module only when the interface
  hash of something it imports changed. rustc's red-green marking and
  Salsa's backdating stop at a query whose result fingerprint did not
  change.
- **Fix.** Build §5.3.1: key a module's check on the interface hashes of
  the items it read, not on its closure's deep hashes. A dependent whose
  own interface comes out unchanged then stops the wave.

### C6. No integer range facts, so provably safe `+ 1` keeps its check

- **Miss.** `check-cost`: checked 848 ms against unchecked 439 ms, 1.93x
  (target 1.3x).
- **Evidence (verified).** The fixture's inner loop is
  `total = total + values[j] * 3 - round` and `j = j + 1` under
  `while j < values.len()`. `hd_wasm` removes a check only when an operand
  is a constant (`checked_by_constant`, `crates/hd_wasm/src/emit.rs:4909`).
  `hd_opt` has no range or interval analysis. So `j + 1` and
  `round + 1` keep checks that the loop conditions already rule out, and
  each check is a branch that also blocks Cranelift's loop optimizations.
  The 64-bit multiply already has a fast path for 32-bit operands
  (`emit.rs:5166`), so it is not the main cost.
- **State of the art.** LLVM proves `j + 1` cannot overflow from the
  dominating `j < len` (SCEV, CorrelatedValuePropagation), so Swift and
  rustc's overflow-checked builds pay little in loops. V8's TurboFan types
  every integer node with a range for the same purpose.
- **Fix.** A range fact pass on TIR: a dominating compare against a length
  or a constant bounds the induction variable, and the checked op that
  cannot leave its type becomes a plain op. One pass serves bounds checks
  too.

### C7. Poisoned match arms still cascade

- **Miss.** Largest diagnostic 92 tokens (target 60), from
  `some-on-result`. It is also the one corpus program with more than one
  diagnostic.
- **Evidence (verified).** Matching `.Some(n)` and `.None` on a `Result`
  gives four errors: `nonexhaustive-match` (first, so the expected code is
  not first), two `unknown-variant`, and `unknown-name` for `n`.
- **State of the art.** rustc skips the exhaustiveness check of a match
  with an erroneous pattern, and binds a bad pattern's names at the error
  type.
- **Fix.** An arm with an unknown variant poisons the match: no
  exhaustiveness diagnostic, and its bindings exist at the error type. Only
  the two `unknown-variant` errors remain, and the message can say "this is
  a Result: match `.Ok` and `.Err`".

### Smaller items

- **edit-latency p50, 52 ms (target 50).** Recheck it after the
  [settle-window fix](#harness-problems). Generated files younger than 2 s
  carry no stamp, so each check skims every module again.
- **lookup-latency, n/a.** #181 (program database) is closed as completed,
  but `hd callers` and `hd needs` are in no CLI spec section and no build.
  Either the queries get specified, or the metric is retired.

## Pillar 2: Agent Scalability

### C1. No cross-process single-flight in the two caches

- **Misses.** `cache-contention`: 8 concurrent fetches open 16
  connections (target 2, one fetch's); 8 concurrent checks write 9 files
  where one check writes 7. `concurrency`: total CPU grows as N^1.11 from
  N = 1 to 16 (target below 1); p95 is 2.23x at N = 4 (target 1.5x) and
  8.38x at N = 16 (target 6x).
- **Evidence (verified).** The dependency cache stages each fetch in
  scratch and moves it into place, and "the first rename wins; a loser
  leaves its tree to be removed" (`crates/hd_deps/src/cache.rs:104-118`).
  So every process fetches. The compilation cache is first-writer-wins at
  publish (`crates/hd_cache/DESIGN.md` §5.4); no process waits for another
  that is already computing the same key. The concurrency metric starts N
  checks at once on one fresh shared cache, so all N compute std's skims
  and std's checks. That is why total CPU is linear or worse, not
  sublinear.
- **State of the art.** Cargo holds a package-cache lock while it
  downloads. Go's module cache takes a per-module file lock
  (`cmd/go/internal/lockedfile`) and checks again after it gets the lock.
  Bazel's remote cache and Buck2 deduplicate in-flight actions.
- **Fix.** Dependencies: a per-key `flock`, then look again before
  fetching. Compilation cache: a lease per expensive shared key (the std
  entries first). A second process waits on the lease and then reads the
  entry. Per-package entries need no lease, since agents rarely share
  them.

### C5. REPL input N rechecks the chain of all earlier inputs

- **Miss.** `long-session`: RSS grows 96.5 MB from input 100 to 1000
  (target 22 MB). Build time per input grows from 89 ms at input 50 to
  982 ms at input 1000. So a session's total cost is quadratic.
- **Evidence (verified).** Each input module uses the input just before it
  (`session_uses`, `crates/hd_driver/src/lib.rs:1532`). `repl_texts`
  (`:1477`) walks the uses from the new input, so it reaches every earlier
  input and reads, skims and checks each one again. Each input's
  `check_key` also hashes its whole closure (C4), so nothing is reused.
- **State of the art.** GHCi, the Swift REPL and IPython keep one
  cumulative environment: input N is checked against the session's current
  bindings, at a cost proportional to the input.
- **Fix.** Keep a session scope: the latest binding of each name, as one
  interface. Input N reads that interface and adds its own bindings. Its
  cost no longer depends on N. C4's early cutoff alone would not be
  enough, since the walk itself is O(N).

### C8. Release binary not size-tuned; std embedded as text (suspected)

- **Misses.** `disk`: 20.1 MB per worktree, all of it the `hd` binary
  (artifacts are 7.6 KB; target 10 MB). `startup`: `hd check empty.hd`
  peaks at 17.0 MB RSS (target 10 MB); `hd help` takes 3.9 MB.
- **Evidence.** `Cargo.toml` has no `[profile.release]`, so the CLI builds
  with 16 codegen units, no LTO and its symbols kept. Only the `web` profile
  is tuned. The 1.2 MB of std source is embedded as text, and
  `embedded_std()` (`crates/hd_driver/src/lib.rs:109`) copies every file
  into a new `Arc<str>` and hashes the whole set on each run.
- **State of the art.** rustup's `rustc` and Go's toolchain ship stripped,
  LTO-built binaries. Compilers that bundle a standard library ship it
  pre-parsed (Swift `.swiftmodule`, GHC `.hi`), not as source to re-read.
- **Fix.** A `[profile.release]` with `lto = "thin"`, `codegen-units = 1`
  and `strip = "symbols"`, then measure. Hash std at build time (a `const`
  from `build.rs`) and borrow the static text instead of copying it. Take a
  heap profile (dhat) of `hd check empty.hd` before changing more.
- **Note.** The disk metric counts the toolchain because CI runs
  `target/release/hd` from inside the worktree. An installed toolchain is
  shared per user, so the owner may prefer that the metric count artifacts
  only.

### C9. No timing trace, so the parallel gap has no named cause

- **Miss.** `parallel-speedup`: 1.92x on 4 threads against 1, 50k lines
  (target 2.4x).
- **Evidence.** No scheduler trace or critical-path report exists
  (`hd_sched`, `hd_cli`, `hd_driver`). So the serial parts cannot be named
  yet. Candidates: module discovery, skims before the task graph starts,
  `PackageResult`, coherence and the cache publish.
- **State of the art.** rustc `-Z self-profile`, Go's `go tool trace`, and
  Chrome trace-event JSON from Clang `-ftime-trace`.
- **Fix.** First add an `HD_TRACE=file` that writes trace events per task.
  Then file the serial phase it shows as its own cause.

## Pillar 3: Artifact Quality

**The fact behind most of these.** CI builds `cargo build --release -p
hd_cli` (`.github/workflows/metrics.yml:42`). That build leaves out the
`wasmtime` feature (`crates/hd_cli/Cargo.toml:13`), and the default engine
is Node (`crates/hd_cli/src/engine.rs:229`). So every `hd FILE.wasm` and
every `hd test` in the pillar 3 metrics starts a `node` child process. It
writes the JS host and `main.wasm` to a fresh temp directory
(`crates/hd_cli/src/node.rs:196-219`), and V8 compiles the module with no
saved code cache (`host/run.ts:213`, `host/test.ts:57`).

### C10. A Node process and an uncached compile per run

- **Misses.** `size-startup-heap`: tiny program start to first output
  56 ms (target 5 ms). `integration-test-perf`: setup 161 ms per test
  program, 146 ms per doc test (targets 20 ms). Part of `unit-test-perf`.
- **Evidence (verified).** A Node boot alone is 30 to 40 ms
  (`node --experimental-wasm-jspi -e 0`), so 5 ms cannot be reached
  this way. Each `tests/` program and each doc test is its own module, run
  one after another (`crates/hd_cli/src/test_cmd.rs:1565`). Within one
  program, each worker starts its own Node process and compiles again
  (`test_cmd.rs:1601-1636`). Even when built in, the wasmtime engine
  caches below wasmtime's module cache, so it still translates every
  function on each run (`crates/hd_run_wasmtime/src/code_cache.rs:1-11`).
- **State of the art.** `wasmtime run` on a precompiled module
  (`Module::deserialize_file`, memory-mapped) starts in 1 to 3 ms. Rust
  2024 rustdoc merges doc tests into one binary. nextest runs prebuilt
  test binaries in parallel.
- **Fix.** Test runs: one host process per `hd test`. Compile each module
  once and send the compiled `WebAssembly.Module` to worker threads, which
  does not recompile. Run test programs concurrently, and link doc tests
  into one module. Running `.wasm` files: in-process wasmtime with a
  serialized-module cache. That needs the owner, since wasmtime is off by
  default for build time.

### C11. A fresh host and instance per unit test case

- **Miss.** `unit-test-perf`: 1,000 tests take 1,130 ms (target 1,000 ms).
- **Evidence (verified).** Every `it` case releases the host and builds it
  again. That constructs every platform provider and wraps every ABI
  method (`host/cases.ts:283-297`, `host/core.ts:1033-1050`), then
  instantiates the module (`cases.ts:269`). Each case also copies the
  export object, which has one entry per test, twice (`cases.ts:272`,
  `core.ts:1562`). That makes the suite quadratic in its test count.
- **State of the art.** libtest runs cases as threads in one process.
  Wasmtime's `InstancePre` with the pooling allocator instantiates in
  microseconds.
- **Fix.** Build the import object once per worker and reset only the
  per-case state. Look up exports once.

### C12. Property tests cross the host boundary on every draw

- **Miss.** `proptest-perf`: 23k cases/s, about 43 µs per case (target
  100k/s).
- **Evidence (verified; the split between items is not measured).** Every
  draw calls the host (`self.runner.record(value)`, `lib/std/testing.hd:131`;
  `BigInt` and a push in `host/core.ts:757`). Every case, passing or not,
  builds `debug(value)` and sends it across (`testing.hd:651`, `673`).
  Each `start` encodes its result with a BigInt LEB encoder into a new
  buffer (`host/wire.ts:49-77`).
- **State of the art.** proptest and Hypothesis record choices in an
  in-process buffer, and format the input only for the failing case.
- **Fix.** Keep the draws in a guest-side list and hand them over once per
  case. Render the input only on failure, since replay is deterministic.
  Better still, run the case loop in the guest, with one host entry per
  property.

### C13. A JS host ABI that decodes and re-encodes every call

- **Misses.** `host-call-overhead`: `println` 8.7 µs per call (target
  1 µs), `read_text!` 27 µs (10 µs), `list_dir!` 47 µs (5 µs).
- **Evidence (verified).**
  - `println` makes two host calls, text then `"\n"`
    (`lib/std/console.hd:114-119`). Each call decodes UTF-8 to a JS string,
    encodes it again with `Buffer.from` (`host/output.ts:158`), and makes
    one unbuffered `writeSync` (`output.ts:123`). Each encodes its `Result`
    through a new encoder (`core.ts:629-644`).
  - `read_text!` runs `realpath` per path component even under an
    unrestricted grant (`host/node-files.ts:41-75`). It adds a `statSync`
    (`node.ts:736`) and creates a new `TextDecoder` per call
    (`node-files.ts:223`).
  - `list_dir!` allocates two `Buffer`s per sort comparison
    (`node-files.ts:231-233`) and encodes every integer as a BigInt LEB
    (`wire.ts:65-80`).
- **State of the art.** Rust's `LineWriter` buffers stdout. wasmtime-wasi
  and cap-std resolve paths with `openat` from a preopened directory, with
  no `realpath` per call. The component model's canonical ABI uses a flat
  layout with no varints.
- **Fix.** Guest-side output buffer, flushed at a newline on a TTY, before
  other host calls, and at exit. Skip path resolution under an
  unrestricted grant. Precompute sort keys. Use fixed-width integers in the
  wire format. The two writes per `println` are a spec rule
  (`module.console.println-two-writes-sync`), so buffering or one call per
  line needs the owner.

### C14. JSON makes two passes and checks keys at run time

- **Misses.** `serde-throughput`: encode 50 MB/s, 0.19x Node; decode
  29 MB/s, 0.20x Node (target 0.5x).
- **Evidence (verified, not profiled).** String scans already use SIMD
  (`crates/hd_wasm/src/scan.rs`).
  - Decode: `Parser.validate` is a recursive one-byte-per-step scan that
    builds an index (`lib/std/json.hd:580-662`). It also skips whitespace
    one byte at a time (`906-914`). `TextReader` then walks the text again,
    matches each field by comparing key bytes, and records a path step per
    member (`3339-3407`).
  - Encode: `TextWriter` runs a frame-stack state machine and returns a
    `Result` per call (`1693-1930`). It checks every member for duplicate
    keys (`add_key`, `1818-1833`) and escapes each member name again
    (`1789`).
- **State of the art.** serde_json decodes in one pass straight into the
  typed value, and its derive matches field names at compile time. It
  writes known keys pre-quoted and escapes through a 256-entry table.
  simdjson finds the structure with SIMD.
- **Fix.** When `T::regular()` holds (it is already computed), skip the
  duplicate check and write keys quoted once per type. Add a one-pass typed
  reader for regular types. Every derived format gains, not one benchmark.

### C15. Regex search always runs the Pike VM

- **Miss.** `text-throughput`, regex `find_all`: 6.7 MB/s, 0.09x Node
  (target 0.5x).
- **Evidence (verified).** `find_all` always runs the Pike VM
  (`lib/std/regex.hd:1443-1554`). Every byte steps every live thread,
  capture slots are copied per step and per new thread, and character
  classes are tested by a linear scan of their ranges (`1497-1504`). There
  is no DFA. On this text the first-byte prefilter (`1556-1590`) skips
  almost nothing.
- **State of the art.** The Rust `regex` crate searches with a lazy DFA
  over byte classes: forward to find the end, backward to find the start.
  It runs a capture engine only when captures are asked for, and uses
  memchr and Teddy prefilters. V8's Irregexp compiles to native code.
- **Fix.** A lazy DFA with byte classes and a capped state cache, for
  `is_match`, `find`, `find_all`, `split` and `replace`. Keep the Pike VM
  for captures. Search stays linear time.

### What pillar 3 does well

Runtime geomean is 1.05x Node, and allocations are 0 B per iteration on
all four loops. Await costs 2.2 ns. Dead code is out of the binary, and
memory is flat over a long run. The misses are the boundary (host, test
runner, start-up) and two std engines (JSON, regex), not the compiled
code.

## Harness Problems

Fix these before reading the related numbers as compiler facts.

| Metric                            | Problem                                                                                                                                                                                                                                                                                       | Fix                                                                                                    |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `io-per-check`, 79 files read     | The package is generated, warmed and edited within 2 s. A file whose mtime or ctime is within `SETTLE` (2 s, `crates/hd_cli/src/disk.rs:132`) gets no stamp (git's racy-clean rule), so its skim is never recorded and every check reads it. The lazy-text and stamp path is in place (`lib.rs:2441`, `skims.rs:471`). | Backdate the generated files with `utimes`, or wait 2 s before the warm-up. Then measure again.        |
| `pathological`, `iterator-chain`  | At 2x the chain has 100 stages. Spec rule `trait.bound.depth` charges a proof's full height, so any chain past 64 adapters is `trait-resolution-depth` by design. rustc accepts long chains only because its cache answers do not carry depth, which hd rejects as order-dependent (`hd_check/DESIGN.md` §7.3). | Owner decision. Keep the rule and scale the fixture by the number of chains of 32 stages. Or change the rule. |
| `disk`                            | Counts the `hd` binary as per-worktree disk (see C8).                                                                                                                                                                                                                                         | Owner decision on what the metric counts.                                                              |

## Owner Decisions

1. `iterator-chain`: keep the depth rule and change the fixture
   (recommended), or change the rule.
2. `disk`: count artifacts only, or keep counting the toolchain.
3. `lookup-latency`: specify `hd callers` and `hd needs`, or retire the
   metric. #181 is closed, but neither command exists.
4. C10: run `.wasm` in-process on wasmtime by default, which puts wasmtime
   in the default build, or accept that the 5 ms start-up target is out of
   reach on Node.
5. C13: `println` makes two host writes by spec rule
   `module.console.println-two-writes-sync`. Buffering or one call per line
   changes that rule.
