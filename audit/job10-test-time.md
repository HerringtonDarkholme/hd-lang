# Job 10: Where Test Time Goes Now

Measured 2026-10-04 on this machine (darwin/arm64) at commit `4b642b45`
(Jobs S+U landed), from the worktree. Single wall-clock samples; expect
±20% run-to-run noise (seen: `hd-adapter.test.ts` 7.0s vs 14.4s across two
runs, `spec-tools.test.ts` 5.1s vs one 7.7s test). No result below was
near the 10s runner limit.

## Method

- Per test file: `/usr/bin/time -p node --test
  --experimental-strip-types FILE` for each of the 59 files in `pnpm
  test`'s globs (`test/*.test.ts`, `spec/tools/*.test.ts`,
  `website/test/*.test.ts`, `website/playground/test/**/*.test.ts`).
  Slowest files were re-run with `--test-reporter=spec` for per-test
  times. Each file starts a fresh node process, so every file pays a
  cold compile of `lib/std` at least once.
- Portable suite: `run-conformance.ts --manifest
  test/portable/cases.tsv --adapter test/hd-adapter.ts --jobs 8 --tier
  <language|std|cli>`, wall-timed per tier. Per-fixture times come from
  the same runs via a wrapper adapter that timestamps each `run()` call
  (check / test / parse / run actions separately); fixture totals below
  sum a fixture's invocations. The in-process adapter reuses 8 workers,
  so the first fixtures on each worker include cold std compilation.
- Baseline probe (warm, in-process): `analyze()` averages ~190ms for
  both the empty program and the tiny size-guard program on this
  machine, so roughly 190ms of every `check` invocation is joining and
  re-checking `lib/std`, before the checker's work on user code. (The
  compiler perf audit measured the same split as ~78% std on faster
  hardware.)

## Portable suite per tier (wall, 8 jobs, all green)

| Tier | Cases | Wall |
| --- | ---: | ---: |
| language | 2,021 | 41.19s |
| stdlib | 276 | 15.76s |
| CLI | 9 | 1.77s |

Total ≈ 58.7s wall. 0 timeouts in 3,124 invocations; slowest single
invocation 3.6s (`hd test --format json` in the `json-test-order` CLI
scenario, which tests a whole temp package).

## 15 slowest test files (wall per file)

| # | Time | File | Slowest tests inside |
| --- | ---: | --- | --- |
| 1 | 24.83s | website/playground/test/runner.test.ts | bundled examples run 9.8s; tour snippets 5.8s |
| 2 | 14.33s | test/repl.test.ts | values render structurally 5.8s; stdin session 1.2s; bindings kept 1.0s |
| 3 | 8.99s | website/test/site.test.ts | website build 7.6s |
| 4 | 7.94s | test/compiler.test.ts | top cases ~0.3–0.5s each |
| 5 | 7.34s | test/compiler-suspension.test.ts | suspension CFG compiles |
| 6 | 6.98s | test/hd-adapter.test.ts | adapter-vs-spawned-CLI parity 12.6s (noisy); timeout replacement 1.8s |
| 7 | 6.87s | test/compiler-types.test.ts | type compiles |
| 8 | 6.79s | test/cli.test.ts | executable args/exit 4.2s; CLI end to end 2.4s |
| 9 | 6.46s | test/std.test.ts | property.hd 2.6s; annotation.hd 0.6s |
| 10 | 5.05s | spec/tools/spec-tools.test.ts | rewrite-from-git-history 7.7s (noisy); CLI usage 1.4s |
| 11 | 4.49s | test/cli-commands.test.ts | package/REPL/CLI scenarios |
| 12 | 3.24s | test/dogfood.test.ts | example checks |
| 13 | 3.12s | test/types.test.ts | type checks |
| 14 | 3.11s | test/call-speculation.test.ts | speculation checks |
| 15 | 2.38s | test/reachability.test.ts | reachability compiles |

## 20 slowest fixtures (invocation sums; check + test split)

| # | Total | check | test | Fixture |
| --- | ---: | ---: | --- | --- |
| 1 | 2.10s | 0.27s | 1.83s | runtime/valid/property-draw-budget.hd |
| 2 | 1.89s | 0.30s | 1.59s | runtime/valid/derived-arbitrary-with.hd |
| 3 | 1.66s | 0.27s | 1.39s | runtime/valid/derived-arbitrary-no-finite-value.hd |
| 4 | 1.58s | 0.15s | 1.43s | runtime/valid/property-generators-scalars.hd |
| 5 | 1.44s | 0.76s | 0.68s | runtime/valid/string-split.hd |
| 6 | 1.41s | 0.73s | 0.68s | runtime/valid/it-each-rows.hd |
| 7 | 1.37s | 0.79s | 0.57s | runtime/valid/test-timeout-options.hd |
| 8 | 1.37s | 0.27s | 1.10s | runtime/valid/property-assume-discards.hd |
| 9 | 1.34s | 0.76s | 0.58s | runtime/valid/replace-non-overlapping.hd |
| 10 | 1.28s | 0.71s | 0.57s | runtime/valid/string-trim-and-lower.hd |
| 11 | 1.28s | 0.71s | 0.57s | runtime/valid/empty-string-operations.hd |
| 12 | 1.23s | 0.72s | 0.51s | runtime/valid/trim-unicode-white-space.hd |
| 13 | 1.23s | 0.73s | 0.50s | runtime/valid/split-empty-input-nonempty-separator.hd |
| 14 | 1.20s | 0.37s | 0.83s | runtime/valid/derived-arbitrary-recursive-members.hd |
| 15 | 1.19s | 0.40s | 0.79s | runtime/valid/display-dispatch.hd |
| 16 | 1.12s | 0.20s | 0.92s | runtime/valid/choices-choose.hd |
| 17 | 1.04s | 0.45s | 0.59s | runtime/valid/assert.hd |
| 18 | 1.00s | 0.16s | 0.84s | runtime/valid/property-generators-collections.hd |
| 19 | 0.97s | 0.35s | 0.62s | runtime/valid/partial-equality-dispatch.hd |
| 20 | 0.96s | 0.34s | 0.62s | runtime/valid/default-standard-types.hd |

CLI scenarios are whole-package runs, not fixtures; the slowest were
`exit-test-failure` 5.1s (3 invocations), `json-test-order` 3.6s,
`json-test-ignored` 3.2s, `json-test-fail` 3.1s, `json-test-pass` 2.9s,
`exit-package-file` 2.8s. Each compiles several temp-package files, so
each pays cold std compiles.

## For the slowest: std re-checking, our work, or timeout

- No timeouts. Nothing timed out in 3,124 invocations; the max single
  invocation (3.6s) is far from the 10s limit. One overlapping run
  during this job showed 2 stdlib failures that passed on immediate
  re-run; that was load, not a 10s timeout (both runs finished in
  normal time).
- `runner.test.ts` (24.8s): our work. It esbuild-bundles the
  playground runner in `before` and then runs every bundled example and
  tour snippet through the browser pipeline. Bundling plus executing,
  not std re-checking.
- `repl.test.ts` (14.3s): mostly std re-checking. 22 tests drive the
  REPL input by input (5.8s render test, many 0.5–1.2s checks); every
  input re-joins and re-checks `lib/std` (~190ms warm here) plus the
  checker's work on the input.
- `site.test.ts` (8.99s): both. The 7.6s website build checks every hd
  doc block (each a fresh std re-check) plus markdown/link rendering
  (our work).
- `compiler*.test.ts`, `call-speculation`, `types`, `package`,
  `reachability` (2–8s): std re-checking plus our checker. Each test
  compiles one or more programs in-process; every compile pays the
  ~190ms std baseline, and speculation/suspension tests pay checker
  search on top (their top cases run 0.3–1.8s each).
- `cli.test.ts`, `hd-adapter.test.ts`, `cli-commands.test.ts`
  (4.5–7s): process spawns. These drive the real `hd` executable or
  compare adapter vs spawned CLI, so each case pays node startup plus
  a cold std compile; the 12.6s parity test is spawn overhead, not a
  slow fixture.
- `std.test.ts` (6.5s): our work running. It runs `lib/std/*.hd`
  test files; `property.hd` alone is 2.6s executing hundreds of
  generated property cases in wasm.
- `spec-tools.test.ts` (5.1s): our work plus git. The rewrite and
  refs tests shell out to git history (7.7s/0.5s, noisy); counts/refs
  walk the spec tree.
- Slowest fixtures: test-phase dominated (property/derive fixtures
  run hundreds of generated cases in wasm: our compiled code
  executing), except the string/it-each/timeout rows, whose ~0.7s
  `check` is std re-check plus checker work on the file. No fixture
  is slow because of a timeout.
