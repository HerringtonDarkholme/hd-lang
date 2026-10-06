# Goal Metrics

Scripts that check the goal metrics of the new compiler, as listed in the
Goal Metrics section of
[NEW_COMPILER_ARCHITECTURE.md](../../future-work/NEW_COMPILER_ARCHITECTURE.md).
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
```

- `--hd COMMAND` is the `hd` under test. The default is
  `node --experimental-strip-types bin/hd.js`, the prototype. A word of the
  command that names an existing file is made absolute, so relative paths
  work from the repository root. `HD_METRICS_COMMAND` also sets it.
- `--keep-temp` keeps the temporary directories, for debugging a script.

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
| `lookup-latency` | 10k-line package: `hd doc ITEM`, `hd def NAME`, `hd explain CODE`, 10 runs each after a warm-up | p95 ≤ 100 ms each | a command whose `hd help COMMAND` fails |
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
| `lib/hd.ts` | running `hd`: command parsing, timeouts, CPU time and peak RSS, JSON lines |
| `lib/gen.ts` | the seeded package generator: small, 10k and 50k lines |
| `lib/fixture.ts` | generated packages in temporary directories, timed series |
| `lib/stats.ts` | percentiles, growth exponents, token estimates |
| `lib/tmp.ts` | temporary directories |
| `lib/metric.ts` | the metric contract and target judging |
| `lib/corpus.ts`, `lib/mistake-run.ts`, `lib/fixit.ts` | the mistake corpus, its shared run, fix-it application |
| `mistakes/`, `pathological/` | the corpora |
| `metrics.test.ts` | unit tests of the helpers: `node --test --experimental-strip-types test/metrics/metrics.test.ts` |

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

Pillar 2, pillar 3 and the correctness gate add their scripts the same way:

1. Write `scripts/NAME.ts` exporting a `Metric`: its name, its pillar, a
   one-line summary, and `run(context)`, which returns one `TargetResult`
   per target. Use `judge`, `judgeBool`, `notApplicable` and `failed` from
   `lib/metric.ts`.
2. Add it to `scripts/index.ts`, in the order of the Goal Metrics tables.
3. Add a row to the table above, with its n/a condition.

`runProcess` with `measure: true` gives CPU time and peak RSS, and
`materialize("50k", seed)` the large package, for the resource metrics.
