# Goal Metrics

Scripts that check the goal metrics of the new compiler, as listed in the
Goal Metrics section of
[goals.md](../../future-work/compiler/goals.md).
Each metric is one script, judged against its target with no AI and no
agent run.

The owner's rules (2026-10-06) shape every script:

- **Scripts only.** A metric measures and reports pass or fail.
- **Read-only.** A script never writes to the repository or to the program
  under test. Generated packages, edited copies, applied fix-its and caches
  live in temporary directories, removed at the end of the run.
- **Neutral.** A script takes the `hd` to test as a command and runs it as
  a child process. It imports no compiler code.
- **No prototype baselines.** The frozen prototype only smoke-tests the
  scripts. No target comes from its numbers.

## Running

```sh
pnpm run metrics                                  # every metric
pnpm run metrics -- --only mistakes               # one metric
pnpm run metrics -- --only edit-latency,fmt       # several
pnpm run metrics -- --pillar 1                    # one pillar
pnpm run metrics -- --list                        # list the metrics
pnpm run metrics -- --hd "/path/to/hd" --only cold-check
pnpm run metrics -- --pillar 2 --suite --max      # with the slow and large runs
pnpm run metrics -- --pillar gate --suite         # the correctness gate, with conformance
pnpm run metrics -- --only long-run-memory --long # the 10-minute soak
```

- `--hd COMMAND` is the `hd` under test. The default is
  `node --experimental-strip-types bin/hd.js`, the prototype. A word of the
  command that names an existing file is made absolute, so relative paths
  work from the repository root. `HD_METRICS_COMMAND` also sets it.
- `--keep-temp` keeps the temporary directories, for debugging a script.
- `--suite` runs `suite-cpu` and the `conformance` pass rate, which take
  minutes; without it, those lines are `n/a` with "run with --suite".
- `--max` adds the largest scale: 64 concurrent checks in `concurrency`.
- `--long` runs `long-run-memory` for 10 minutes instead of 60 s.

The runner prints one line per target as it is judged, then a summary
table, then the counts. It exits 1 when any target fails, and 2 on a usage
error. An `n/a` target never fails the run.

## Status Of A Target

| Status | Meaning |
| --- | --- |
| `PASS` | the measured value meets the target |
| `FAIL` | it does not, or the measurement failed: a timeout, a crash, or a wrong answer |
| `N/A` | this `hd` lacks the feature the target needs, such as `hd fmt`; never a made-up value |

Every script stops a run that exceeds its timeout, kills it, and fails the
target with the timeout in the note. The timeouts are 10x to 50x the
target, so a slow `hd` fails fast instead of holding the run for minutes.

## Pillar 1 Metrics

| Script | Measures | Target | n/a when |
| --- | --- | --- | --- |
| `edit-latency` | 10k-line package: warm-up `hd check`, then 20 edits of a private function body, each followed by `hd check` | p50 ≤ 50 ms, p95 ≤ 200 ms | never |
| `cold-check` | `hd check` of a fresh copy of the 10k-line package with an empty `HD_CACHE`, median of 3 | ≤ 1 s | never |
| `test-latency` | 10k-line package: edit one module, then `hd test --filter NAME` of one of its test cases, median of 5 | ≤ 300 ms | never |
| `mistakes` | the [mistake corpus](#mistake-corpus): diagnostics per program, the expected code, the fix-it, the text size | one diagnostic in ≥ 95%; expected code in ≥ 95%; fix-it resolves ≥ 80%; largest text ≤ 60 tokens | fix-it line: no `fix` field |
| `answer-size` | bytes of `hd doc m001.Item1`, one failing `hd test`, its JSON test object, one JSON diagnostic object | doc ≤ 1,000 B; failing test ≤ 800 B; test object ≤ 400 B; diagnostic object ≤ 600 B | doc line: no `hd doc` |
| `determinism` | 8 or 9 fixed inputs, each run 10 times: exit status, stdout and stderr | byte-identical | never |
| `pathological` | the [stress cases](#pathological-cases) at scales 1, 2 and 4 | at 4x: ≤ 2 s and ≤ 200 MB; growth exponent ≤ 1.3 | RSS line: no `/usr/bin/time` |
| `recheck-precision` | small package: modules rechecked after a private body edit, then after a public signature edit | 1; that module and its direct dependents | no `modules_checked` in the JSON summary |
| `errors-per-run` | two files of 10 independent mistakes, one with 2 syntax errors among them | all 10 reported, each once | never |
| `diag-location` | the mistake corpus: the reporting diagnostic's line against the marked line | ≥ 95% | never |
| `fixit-safety` | the mistake corpus: errors after applying each offered fix-it | 100% add no new error code | no `fix` field, or no fix-it offered |
| `lookup-latency` | 10k-line package: canned program-database queries `hd callers NAME` and `hd needs Http`, 10 runs each after a warm-up | p95 ≤ 100 ms each | a query whose `hd help COMMAND` fails |
| `fmt` | `hd fmt` on a copy of the 10k-line package, then a second `hd fmt` | ≤ 200 ms; the second run changes nothing | no `hd fmt` |
| `release-check-cost` | a checked-arithmetic loop run with `hd run` and `hd run --release` at two sizes; each profile's time is large minus small | debug ≤ 1.3x release | `hd help run` names no `--release` |

Notes on the choices:

- **Tokens** are estimated as bytes / 4. No tokenizer is used, so the
  count is the same on every machine.
- **`answer-size` budgets** are this harness's proposal, from what an
  agent needs to read: a 60-token diagnostic is about 240 bytes of text,
  and a JSON object about 2.5x its text. The owner may change them. A JSON
  object is measured as `JSON.stringify` writes it, compact.
- **`release-check-cost`** uses an executable because the CLI gives
  `hd test` no `--release` flag
  ([`cli.profile.flag-only`](../../spec/cli/command-line.md#r-cli.profile.flag-only)).
  Release work under 50 ms is within noise and fails.
- **`recheck-precision`** reads `modules_checked`, a number in the summary
  object of `hd check --format json`: the modules the run type-checked
  rather than reused. The CLI specification has no such field yet; the name
  is this harness's convention until it does.
- **Fix-its** are read from a `fix` field of a JSON diagnostic object: one
  fix with a list of edits, each a `span` with 1-based `line` and `column`
  (UTF-16), an optional 0-based `offset`, and a `replacement`. With no
  `fix`, a `fixes` list of exactly one entry counts. This is the shape the
  prototype's [src/README.md](../../src/README.md) documents;
  [`cli.json.diagnostic.fields`](../../spec/cli/command-line.md#r-cli.json.diagnostic.fields)
  names no fix-it field yet.
- **`mistakes`** checks with `hd check --tests FILE`, so a mistake inside a
  `tests:` block is checked too
  ([`cli.check.tests`](../../spec/cli/command-line.md#r-cli.check.tests)).

## Pillar 2 Metrics

| Script | Measures | Target | n/a when |
| --- | --- | --- | --- |
| `resources` | CPU time and peak RSS of `hd check`, `hd test` and `hd build` on the small, 10k and 50k packages, each cold (fresh copy, empty `HD_CACHE`) then warm; one line per run | warm check of 10k lines ≤ 0.2 CPU-s and ≤ 50 MB; every other run completes within its timeout | no `/usr/bin/time` |
| `long-session` | RSS of one `hd repl` session over 1,000 inputs, sampled with `ps` every 50 inputs; without such a REPL, peak RSS over 50 rechecks after edits | growth from input 100 (or recheck 10) ≤ 10 MB | no REPL and no `/usr/bin/time` |
| `startup` | wall time, CPU time and peak RSS of `hd --version` (or `hd version`, else `hd help`) and of `hd check empty.hd`, median of 5 | ≤ 20 ms; ≤ 10 MB | CPU and RSS lines: no `/usr/bin/time` |
| `concurrency` | N = 1, 4, 16 (and 64 with `--max`) `hd check` processes at once, each in its own copy of the small package, one shared empty `HD_CACHE` | p95 ≤ 1.5x of N = 1 up to N = cores; total CPU sublinear in N | CPU line: no `/usr/bin/time` |
| `disk` | files `hd check`, `hd test` and `hd build` add to the small package, plus the toolchain | ≤ 10 MB | never |
| `suite-cpu` | CPU time of `test/run-portable.ts --suite conformance --compiler HD` | ≤ 60 s | without `--suite` |
| `parallel-speedup` | cold check of the 50k package (10k when 50k times out) with 1 thread, then with one per core | ≥ 0.6 × cores, up to 8 cores | no documented thread count |
| `cache-contention` | 8 checks at once that fetch one dependency into one empty cache; the same for a compilation cache | no corruption; fetched or written once | no git, `hd add` or `HD_CACHE`; no compilation cache |
| `cache-growth` | compilation cache size after 100 edits and checks, with the documented cap set to 5 MB | ≤ 5 MB | no compilation cache, or no documented cap |
| `io-per-check` | source files a warm check opens for reading after a one-module edit, by `strace` | ≤ 1 | no unprivileged tracer (always on macOS); no compilation cache |
| `fetch-dedup` | connections to a local remote when 4 worktrees check, one after another, with one shared cache | as many as the first worktree's | no git, `hd add` or `HD_CACHE` |

Notes on the choices:

- **Timeouts.** `resources` gives each run 30 s at small and 10k lines, and
  150 s at 50k. A command is skipped, as a failing line, at a size above
  one where it timed out. Every command is skipped at a size where
  `hd check` timed out, since test and build type-check too.
- **Bounds this harness proposes.** `long-session` allows 10 MB of growth,
  and `concurrency` scales its bound past the core count to
  1.5x × N / cores. The owner may change both.
- **`suite-cpu`** counts the conformance runner's own CPU time with the
  hd processes it waits for. It does not judge the suite's pass or fail;
  the note gives its exit status. Its timeout is 30 minutes.
- **`startup`** judges CPU time against the 20 ms wall target too. Its wall
  time includes `/usr/bin/time`'s own start, about 1 ms.
- **`long-session`** feeds the REPL inputs that each show the value
  1000000 + i (`cli.repl.value`), so the run knows how far the session
  got. A REPL "reads standard input" when `hd repl` fed `1 + 2` shows 3.
- **Toolchain.** When a file the `--hd` command names lies in a Node
  package with `node_modules`, that folder is the toolchain, and the `disk`
  line says so. Otherwise it is the program on PATH and the files the
  command names. A hard-linked file counts once. `HD_CACHE` is shared per
  user ([`cli.cache.shared`](../../spec/cli/command-line.md#r-cli.cache.shared)),
  so the note gives its size but the total leaves it out.
- **Dependencies without a network.** [`lib/deps.ts`](lib/deps.ts) makes a
  local repository and a temporary `GIT_CONFIG_GLOBAL` whose `insteadOf`
  maps `https://hd-metrics.invalid/` to git's `ext::` transport. That
  transport runs a script that logs each connection and serves the
  repository with `git upload-pack`. `GIT_ALLOW_PROTOCOL=ext` blocks every
  other transport. hd fetches with the system's git
  ([`cli.dep.git`](../../spec/cli/command-line.md#r-cli.dep.git)), so the
  log counts any hd's fetches.
- **A compilation cache** is found by checking the small package, which has
  no dependencies, with an empty `HD_CACHE`. Files written there, or
  inside the package, are the cache.
- **Documented controls.** A thread count is a `--jobs`, `--threads` or `-j`
  flag in `hd help check`, or `HD_THREADS`, `HD_JOBS` or `HD_PARALLELISM`
  in any help text. A cache cap is an `HD_CACHE_…_MAX`, `_CAP` or `_LIMIT`
  variable in a help text, set to a byte count. These names are this
  harness's convention; the CLI specification names none yet.

## Pillar 3 Metrics

| Script | Measures | Target | n/a when |
| --- | --- | --- | --- |
| `proptest-perf` | `hd test` of 3 `it_prop` properties over derived `Arbitrary` types, at 100 and 2,100 `cases`; a failing property with `shrink=500` against `shrink=0` | ≥ 100k cases/s; shrink ≤ 1 s | never |
| `unit-test-perf` | warm `hd test` of 100 and 1,000 generated unit tests, half in `tests:` blocks, half in `_test.hd` modules | 1,000 tests ≤ 1 s; ≤ 1 ms per test | never |
| `integration-test-perf` | warm `hd test` with 0, 2 and 10 `tests/` programs that use `temp_dir()` and the real file system; 2 and 10 doc tests | ≤ 20 ms setup per program and per doc test; total sublinear in programs | never |
| `runtime` | the test/perf/micro cases minus a no-work program, 7 runs after a warm-up, median, p10 and p90; Node runs each case's `main` self-timed | geomean ≤ 1.5x Node; each case ≤ 3x | never |
| `allocations` | V8 bytes allocated per iteration of a counted loop and an iterator chain, from 1,000 to 1,000,000 iterations | 0 allocations (< 1 B/iter); ≤ 1 allocation (≤ 32 B/iter) | programs that do not run on Node |
| `size-startup-heap` | release Wasm size of a tiny program and of the micro cases; start to first output; peak RSS of two micro cases beyond a no-work program, against Node | tiny ≤ 2 KB; ≤ 5 ms; ≤ 2x Node | size: no `.wasm` written; heap: no `/usr/bin/time` |
| `host-call-overhead` | a loop of `println`, `read_text!` or `FsRead.list_dir!` minus the same loop without the call | ≤ 1 µs; ≤ 10 µs; ≤ 5 µs | never |
| `suspension-overhead` | a loop of `echo!(i)`, `all!` or `race!` of two ready tasks minus a loop of plain calls | ≤ 100 ns per await; ≥ 1M tasks/s | never |
| `serde-throughput` | `std.json.encode` and `decode` of a 45 KB list of derived records, at 2 and 32 repeats, against Node's `JSON` | ≥ 0.5x Node's MB/s | never |
| `text-throughput` | `StringBuilder` building, `split` and `Regex.find_all` over `word-I` text, at two sizes, against Node | ≥ 0.5x Node's MB/s | never |
| `dead-code` | release size of the 1k and 10k generated packages with an executable that calls every module; a std-free program with and without unused `use` of 5 std modules | ≤ 10 KB per 1,000 lines; no unused std | never |
| `long-run-memory` | RSS of a simulated JSON service, sampled 30 times over 60 s (10 min with `--long`) | growth after the first quarter ≤ 10 MB | never |

Notes on the choices:

- **Programs.** [`lib/artifact.ts`](lib/artifact.ts) writes each program
  as `src/main.hd` of a package named `bench` and builds it with
  `hd build --release` (plain `hd build` without `--release`). A `.wasm`
  file the build wrote runs as `hd FILE.wasm`
  ([`cli.wasm.run`](../../spec/cli/command-line.md#r-cli.wasm.run)), an
  executable file runs directly, and with neither the program runs with
  `hd run`. Every result line's note says which.
- **Differences, not totals.** A process run includes start-up and
  instantiation. Each measurement takes the difference of two programs
  that differ only in the work: a size, a repeat count, or the call under
  test. Runs interleave after a warm-up run of each, and the value is the
  median of the paired differences.
- **Node comparisons** are self-timing JavaScript programs, as in
  test/perf/micro: each warms up 3 times and times 7 runs in its own
  process. So Node is measured warm, and hd by process differences; the
  ratio favors Node by its JIT warm-up. Python, when installed, is a note
  in `runtime`.
- **Heap.** No portable heap counter exists, so `size-startup-heap` and
  `long-run-memory` use RSS, from `/usr/bin/time` or `ps`. A Node growth
  under 1 MB counts as 1 MB.
- **Allocations** come from V8. A module preloaded through `NODE_OPTIONS`
  turns on `--trace-gc-nvp` and writes `v8.getHeapStatistics()` at exit;
  the bytes a run allocated are the heap in use at exit plus what every
  collection freed. A count needs an object size, so the bounds are bytes.
- **Bounds this harness proposes.** The architecture gives examples, or no
  number, for these: the Fs (10 µs) and serde-boundary (5 µs) call budgets,
  100 ns per await and 1M tasks/s, 10 KB per 1,000 lines, 32 B for one
  allocation, the 20 ms doc test budget, and 10 MB of long-run growth. The
  owner may change them.
- **Unused std** is judged two ways: unused `use` declarations must not
  change the program's size, and the std-free program's Wasm import and
  export names must hold no std module name as a word (the spec/std/ file
  names, less words a runtime uses for its own helpers, such as `host`).
- **Property regressions.** A failing property saves its stream
  ([`std-testing.prop.regression-replay`](../../spec/std/testing.md#r-std-testing.prop.regression-replay)),
  so `proptest-perf` runs every `hd test` in a fresh copy of its package.
  It passes `--seed 7` when `hd help test` names `--seed`.

## Correctness Gate

| Script | Measures | Target | n/a when |
| --- | --- | --- | --- |
| `conformance` | `test/run-portable.ts --suite conformance --compiler HD`: passed / selected cases; and the rows of `test/portable/KNOWN_FAILURES.tsv` | 100%; 0 known failures | pass rate: without `--suite` |
| `incremental-soundness` | a seeded script of 16 edits to the small package: after each, `hd check --tests --format json` in the working copy, with its own cache, against a fresh copy with an empty cache; `hd test --format json` too every 4th edit | 100% match | no compilation cache: a 4-edit sample still runs |

- **Conformance** selects the cases of `test/portable/cases.tsv`, so the
  listed known failures are excluded from the pass rate, and a case that
  fails only by the time limit reruns once, as the runner does.
- **Soundness edits** are a private body edit, a public signature edit, a
  type error put into a body or taken out, a broken test expectation or its
  repair, and a new public function. Two runs match when they exit alike and
  report the same diagnostics (code, severity, file, line, column, message)
  and test outcomes, in any order. Without an incremental mode, a mismatch
  still fails: it shows nondeterminism.

## Mistake Corpus

[`mistakes/`](mistakes) holds 72 programs, one per distinct mistake in
[audit/hd-writing-log.md](../../audit/hd-writing-log.md), deduplicated.
Each holds exactly one mistake. Its header says what the mistake is, which
log rows it comes from, and the code the specification gives it; its
mistake line ends in the conformance line marker
([spec/conformance/README.md](../../spec/conformance/README.md)):

```text
# mistake: Python's `not` for logical negation
# log: 2026-09-29 property-test trial; 2026-10-05 GADT variant results
# expect: syntax-error
# fixed:     !ready

fn negate(ready: bool) -> bool:
    not ready  # diagnostic: syntax-error
```

- `# expect:` is the specification's code, not what any implementation
  prints. Warnings use `# warning: CODE` as the line marker.
- `# fixed:` is optional: the corrected text of the marked line. `\n`
  starts another line, and `# fixed-lines: N` replaces N lines. No metric
  reads it; it lets a reviewer confirm the program holds no other mistake.
- Log rows left out: runtime failures, mistakes inside `lib/std` or the std
  loader, prototype bugs that the specification makes valid code, and
  mistakes that need a package layout or a host.

To see what an `hd` reports for each program, and whether each fixed
variant checks clean:

```sh
node --experimental-strip-types test/metrics/mistakes/check-corpus.ts [--hd "COMMAND"] [NAME...]
```

## Pathological Cases

[`pathological/cases.ts`](pathological/cases.ts) generates each case at a
scale of 1, 2 or 4; the budget applies at 4. Every program must
type-check.

| Case | At scale 4 |
| --- | --- |
| `overlapping-from` | 1,600 locals bound by `Cents::from(N)`, each choosing between two `From` impls (F-626) |
| `deep-blocks` | `if` blocks nested 96 deep |
| `deep-parens` | an expression nested 1,000 parentheses deep |
| `method-chain` | 2,000 chained string method calls |
| `iterator-chain` | 200 `map` and `filter` stages with typed closures |
| `wide-list-literal` | one list literal of 20,000 integers |
| `large-enum-match` | a 1,000-variant enum and an exhaustive match |
| `deep-generic` | `Wrap[Wrap[...[i32]]]` 64 levels deep |
| `question-chain` | one function of 1,000 `?` propagations |
| `big-file` | one file of about 20,000 lines |

The scaling check takes the growth exponent k of `(time - start-up) ~
size^k` from scale 2 to scale 4, where start-up is the median check of a
one-line program. k ≤ 1.3 counts as near-linear. When the work beyond
start-up stays under 100 ms at scale 4, growth is within noise and passes.

To print one program: `node --experimental-strip-types test/metrics/pathological/cases.ts CASE SCALE`.

## Layout

| Path | Holds |
| --- | --- |
| `run.ts` | the runner |
| `scripts/` | one file per metric, and `index.ts`, the list the runner reads |
| `lib/hd.ts` | running `hd`: command parsing, timeouts, CPU time and peak RSS, time to first output, JSON lines, paced sessions and long runs sampled with `ps` |
| `lib/artifact.ts` | user programs: release builds, how a built program runs, interleaved timings and their differences, self-timing Node programs, the V8 heap probe |
| `lib/gen.ts` | the seeded package generator: small, 10k and 50k lines |
| `lib/fixture.ts` | generated packages in temporary directories, an added executable, timed series |
| `lib/stats.ts` | percentiles, growth exponents, token estimates |
| `lib/tmp.ts` | temporary directories, including read-only cache entries; tree sizes |
| `lib/metric.ts` | the metric contract and target judging |
| `lib/corpus.ts`, `lib/mistake-run.ts`, `lib/fixit.ts` | the mistake corpus, its shared run, fix-it application |
| `lib/capability.ts` | probes of what this `hd` offers: help text, thread control, cache cap, compilation cache, REPL |
| `lib/deps.ts` | the local remote that counts fetches, and a dependent package |
| `lib/trace.ts` | `strace` file-call tracing and its parser |
| `mistakes/`, `pathological/` | the corpora |
| `metrics.test.ts` | unit tests of the helpers, part of `pnpm test`: `node --test --experimental-strip-types test/metrics/metrics.test.ts`. They start no process. |
| `integration/` | opt-in tests that start processes, outside the `pnpm test` glob: `node --test --experimental-strip-types 'test/metrics/integration/*.test.ts'` |

CPU time and peak RSS come from `/usr/bin/time -l` on macOS and `-v` on
Linux, written to a temporary file with `-o`. Each run starts in its own
process group, so a timeout kills `hd` and every process it started.

## Generated Packages

`generatePackage({ seed, lines })` writes modules `src/m000.hd`,
`src/m001.hd`, ... of about 130 lines each, plus `src/lib.hd`, one
integration test and `hd.toml`. A module has a data type, an enum with
payloads, a trait and its impl, a `match`, a `?`, loops, a `tests:` block
of two cases, and helper functions drawn from seven shapes. Module K
imports module K-1, and sometimes K-3. The same seed and size give the same
bytes. Each module has two edit sites: the private body line
`let bump = +N` in `tweakK`, and the public signature of `shapeK`, which
gains a defaulted parameter so its callers still type-check.

## Adding A Metric

A new metric is added the same way:

1. Write `scripts/NAME.ts` exporting a `Metric`: its name, its pillar, a
   one-line summary, and `run(context)`, which returns one `TargetResult`
   per target. Use `judge`, `judgeBool`, `notApplicable` and `failed` from
   `lib/metric.ts`.
2. Add it to `scripts/index.ts`, in the order of the Goal Metrics tables.
3. Add a row to its pillar's table above, with its n/a condition.

`runProcess` with `measure: true` gives CPU time and peak RSS, and
`materialize("50k", seed)` the large package, for the resource metrics.
