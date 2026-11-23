# Ownership-Cost Gate

Two measurements catch ownership-cost regressions: dynamic count operations per
benchmark program, and static conservative emitted sites per reason. No
expected number is committed. CI compares each change with its merge base's
measurement:

- **Where the base comes from.** Each Test run on main stores its measurement as
  the `ownership-costs` artifact. A branch or pull request run reads the one of
  `git merge-base HEAD origin/main`. If that commit has none (its run was
  replaced, or the artifact expired), it reads the newest ancestor that has one.
  If none exists, the run only reports. Nothing is built again to measure the base.
- **Decreases pass** and are listed in the job summary.
- **Increases fail** unless a commit since the base measurement has a line
  `Cost-OK: <reason>`. The summary lists each increase per program and per reason,
  with the accepted reason.

Each program and each reason is judged on its own: a cut in one cannot hide a rise
in another. A new benchmark is reported only; a new conservative reason starts
from zero, so any site under it is an increase.

## Dynamic count operations

Every study and runtime benchmark runs once with the optimized guest release
pipeline, the checked runtime, default V8 and seed 0. The table records retains,
releases, sole-owner tests and their sum, `total`. Counts use decimal strings to
preserve u64 precision.

Build the native tool in dev mode, then measure:

```sh
CARGO_BUILD_JOBS=2 cargo build -j2 -p hd_driver --example count_ops
HD_CACHE=target/count-ops/cache node bench/counts/run.ts
HD_CACHE=target/count-ops/cache node bench/counts/run.ts --base OLD/actual.json
```

`--base FILE` compares with an earlier run's `actual.json`; `--cost-ok REASON`
accepts its increases. `--only bench/study/map_count.hd` selects exact paths for a
focused check. `--output DIR` selects scratch modules, `actual.json` and the
Markdown summary; `--compiler FILE` selects a prebuilt `count_ops` example. Each
child program has a 60-second limit and compilation has a ten-minute limit.

A local run executes only the modules it has not seen. Each program's counts are
stored under `target/count-ops/results`, named by the digest of the module, the
program path, the Node version and options, `host/` and the measuring scripts.
`--no-cache` executes every program; CI always does. A stored result would hide a
count that changes between runs of identical bytes, and only full runs can catch
that.

Uniqueness counts cover the executed sole-owner decision in list/map changes,
list reservation, list-to-string conversion, string growth and erased-box
extraction. A release's last-owner branch belongs to that release rather than
an additional uniqueness operation. The existing checked retain/release
counters count actual operations on heap objects; null/static no-ops and the
collector's private trial-deletion bookkeeping do not add counts. Unchecked
builds keep their original loads and comparisons.

Those no-ops still cost something. `No-op calls` counts the retains and
releases that reached the runtime and found no count: static data or a frame
slot behind an address only the page table can place. `Skipped by a test` counts
those a test at the call site turned away (`count_tests.rs`), which cost the
test, not the call. Both are outside `total`. Their sum is judged per program
beside `total`, so a cut in one can't pay for a rise in the other.

The stage case [operation-counter-loop.hd](../../crates/hd_driver/tests/cases/counts/operation-counter-loop.hd)
and its focused driver test execute both loop variants. The extra owner adds
1,000 retains and 1,000 releases and is an increase for the comparator CI uses.

## Conservative emitted sites

CI also compiles only the study and runtime benchmarks and counts static
emitted sites by reason, including std and generated runtime bodies. This
counts the places where a module contains ownership work, independently of
how often a benchmark executes them. The Rust corpus reader's `bench`
selection reads no conformance, stage, example or guide files.

```sh
node bench/counts/conservative-sites.ts
node bench/counts/conservative-sites.ts --base OLD/actual.json
node --test bench/counts/*.test.ts
```

Only `conservative: ` reasons are judged. Named reasons are deliberate and stay
visible in the report.

The command forces the optimized guest release pipeline and the complete
benchmark set, using native dev builds with two workers. A skipped, panicked,
missing or duplicate benchmark fails the gate. It writes `actual.json`,
`summary.md` and raw corpus TSVs under `target/conservative-sites`; CI puts
the reason table in its summary and uploads those artifacts.

`--base FILE` and `--cost-ok REASON` work as above; `--output DIR` chooses scratch
output. `--input DIR` reuses the corpus TSVs from a completed run, for review
without compiling again. The comparator tests sabotage the same CI command with
one extra conservative site and require it to exit unsuccessfully.
