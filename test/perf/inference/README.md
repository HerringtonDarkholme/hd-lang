# Inference Perf Cases

`gen.ts` generates pathological inputs for literal and generic inference.
Two runners time them:

- `gate.ts` is the CI gate, `pnpm run perf:check`. It runs each case at two
  sizes and takes about 55 s on a laptop.
- `run.ts` is the full suite at 1x, 10x, and 100x, for finding new problems.
  It takes about 90 s, a third of it in `mutual-bounds` at 100x.

```sh
pnpm run perf:check [--case NAME]... [--update] [--json FILE]
pnpm run perf:check --merge RUN1 --merge RUN2 [--percentile 90] --baseline FILE
node --experimental-strip-types test/perf/inference/run.ts [--case NAME] [--timeout SECONDS] [--emit]
```

Both runners start `measure.ts` in a child process per measurement. It
calls the compiler's own phase entry points (`parse`, `check`, and with
`--emit` `emitWat`) and reports each phase net of an empty program, which
is the cost of checking the standard library.

## The Gate

Each case runs in one process at a smaller and a larger size. Every round
times the empty program, the reference program (`REFERENCE` in `gen.ts`),
and both sizes. The process reports the median of five rounds, or of three
for a program over 300 ms. CI runs it in its own job, next to the main check.

A case fails when any of these holds:

| Rule | Limit |
| --- | --- |
| crash or timeout | 90 s per case |
| growth from the smaller to the larger size | 12x per 10x of input: `12^log10(step)`, so 3.7x for a step of 3.33. Ignored when the larger time is under 250 ms. |
| growth of a known super-linear case | 2x the ratio recorded in `baseline.json` |
| slowdown at either size | 1.5x the score recorded in `baseline.json`. Ignored when the time is under 100 ms. |

A case that fails a timing rule is measured once more, and fails the gate
only if it fails again. A load spike on a shared machine then passes, with
a warning; a real regression fails both times.

A score is a case's time divided by the reference program's time in the
same process. A diagnostic result that differs from `baseline.json` is a
warning, not a failure: the case may no longer test what it was written for.

### Why a Reference Program

Absolute times on GitHub runners vary between machines and over the
course of a job. The gate never compares milliseconds across runs. It compares scores,
and the reference program is timed in the same process, interleaved with
the case, so a slow machine or a slow minute affects both. The reference
is linear, fully annotated code with no open literal, so it measures the
machine and the checker's ordinary passes, not inference. Medians of
several rounds remove the rest of the noise; locally, scores stay within
0.9x to 1.1x of the baseline from run to run.

The empty program is not the yardstick, because its cost grows with every
`lib/std` change. Another design compares against the previous main
commit, built in the same job. It catches slowdowns that build up over
several commits only if each step is over 1.5x, and doubles the job time.

A score cannot catch a slowdown that hits the reference program as much as
the case, such as a checker that is twice as slow everywhere. The growth
rule still catches anything that becomes super-linear.

### Baseline Runner

Record `baseline.json` on the same `ubuntu-latest` GitHub runners that apply
the gate. The manual **Record performance baseline** workflow runs five independent
update jobs, each on its own runner, because the variance that matters is
between runners and not within one. A final job merges the five files and keeps
the nearest-rank 90th percentile of every case's small score, large score,
and growth ratio. With five samples this is the highest observation, so an
ordinarily slow runner remains below the 1.5x slowdown limit. The artifact's
`recorded` object identifies its platform, run count, and percentile.

Each job uploads its own `inference-perf-run-N` artifact. The merge job
invokes `gate.ts` with repeated `--merge FILE` options; merging rejects
different Node versions, platforms, scales, or result codes.

### Sizes

The larger size takes 0.3 s to 1 s on a laptop. The smaller size takes
30 ms or more, so its time is above the noise. For a case that grows fast
on main, that puts it at a third of the larger size.

### Updating the Baseline

After a change that makes the checker faster, a deliberate performance
trade, or a diagnostic change in a case:

1. Open GitHub Actions and run **Record performance baseline** on `main`.
2. Download the `inference-perf-baseline` artifact of the finished run.
3. Commit its file as `test/perf/inference/baseline.json`.
4. Run `pnpm run perf:check` locally as a sanity check, then commit the file.

The workflow never pushes or commits. Do not replace the baseline with a
local `--update` run: local measurements remain useful for investigation,
but they do not represent the machines that enforce the gate.

## Known Super-Linear Cases on Main

Measured on 2026-10-03 at ba7c830e. The gate fails these only when they
grow 2x faster than recorded.

| Case | Gate sizes | Time (ms) | Growth |
| --- | --- | --- | --- |
| `mutual-bounds` | 10, 30 | 40 / 880 | 22x per 3x, about cubic. 100x takes 32 s: 709x to 926x over 10x. |
| `add-many-types` | 10, 30 | 115 / 820 | 7x per 3x. 100x takes 8.6 s: 76x to 78x over 10x. |
| `list-nest` | 300, 1000 | 50 / 570 | 11x per 3.33x, about quadratic in depth. New: 100x showed 0 ms. |
| `closures-shared-var` | 100, 300 | 96 / 620 | 6.5x per 3x, about n^1.6. New: 100x showed 99 ms. |
| `open-lets-reverse` | 5, 50 | 35 / 410 | 10x to 16x per 10x |
| `late-conflict` | 10, 100 | 40 / 410 | 9x to 14x per 10x |
| `big-body` | 10, 100 | 65 / 770 | 10x to 12x per 10x |
| `method-calls-one-var` | 30, 300 | 35 / 370 | 11x to 13x per 10x |
| `occurs-check` | 40, 400 | 40 / 420 | 10x to 13x per 10x |

The last five are near linear but cross 12x on some runs, so they count as
known to keep the gate from flaking. In the full suite at 1x/10x/100x,
`open-lets-reverse`, `big-body`, and `late-conflict` measured 10x to 16x
over two runs. `overload-candidates` (about 10x) and
`polymorphic-recursion` (about 7x) are held to the 12x rule.

## Results on Main

On main the prototype fixes an unannotated literal binding at `i32` where
it is declared. Six cases written for open literals (`list-nest`,
`open-lets-reverse`, `method-calls-one-var`, `add-many-types`, `big-body`,
`closures-shared-var`) are rejected, as the `literal-first-use` and
`literal-var` rows of `test/portable/KNOWN_FAILURES.tsv` describe. They are
still timed, since the checker walks the whole program before it reports.
`run.ts` shows each result next to the spec's (`expect` in `gen.ts`) and
does not fail on a difference.

## Compiler Bugs Kept Out (Task #255)

These cases crash or time out on main, so they are not in `gen.ts`. Each is a
compiler perf bug, tracked as task #255.

| Case | Size (1x/10x/100x) | Failure |
| --- | --- | --- |
| `nested-tuple` | `(a, a)` tuples nested 4/40/400 deep | 10x crashes in `SignatureInference.run` (`src/checker/program-inference.ts`): the type text of the tuple doubles at each level |
| `tuple-destructure` | a literal tuple of 10/100/1000 slots, destructured | 49 ms, 2245 ms, then over 45 s: about 46x growth per 10x |
| `obligation-cascade` | a bound chain 10/100/1000 deep, declared in reverse | 132 ms at 10x, then over 45 s: bound inference repeats a scan of every parameter until nothing changes |

Found while testing the gate, not yet a case: a body of annotated lets
(`let xN: i64 = 5`) is quadratic. 360 lets take 100 ms and 1200 take 900 ms.
Unannotated lets stay linear.

## Phase Timing

The gate reports parse and check time separately, using the compiler's
exported `parse` and `check`. The compiler has no timing hook inside
`check`. A hook there would split check time into its passes, such as the std
join, derivation, signature inference, and body checking. A
failure would then name the pass that slowed down, and the baseline could
record each pass, so a regression in a small pass is not hidden by a large
one.
