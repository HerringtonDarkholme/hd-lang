# Dynamic Count-Operation Gate

Every study and runtime benchmark runs once with the optimized guest release
pipeline, the checked runtime, default V8 and seed 0. The table in
[baseline.json](baseline.json) records retains, releases, sole-owner tests and
their sum. Each program has its own limit; improvements elsewhere do not
hide an increase. Counts use decimal strings to preserve u64 precision.

Build the native tool in dev mode, then check or record the table:

```sh
CARGO_BUILD_JOBS=2 cargo build -j2 -p hd_driver --example count_ops
HD_CACHE=target/count-ops/cache node bench/counts/run.ts
HD_CACHE=target/count-ops/cache node bench/counts/run.ts --record --reason "Why the extra ownership work is needed"
```

An increase needs both the measured new limit and a fresh `reason` on each
increased row. The checked-in explanation travels with the commit for review.
CI compares it with the previous baseline and rejects unexplained increases
or limits higher than the measured total. A new benchmark also needs a reason
after the initial table exists. Main records decreases automatically; if main
advances during the run, its newer run owns the refresh.

`--only bench/study/map_count.hd` selects exact paths for a focused check.
Recording or refreshing requires the complete program set. `--previous FILE`
checks the explanation against an earlier baseline. `--output DIR` selects
scratch modules, `actual.json` and the Markdown summary; `--compiler FILE`
selects a prebuilt `count_ops` example. Each child program has a 60-second
limit and compilation has a ten-minute limit.

Uniqueness counts cover the executed sole-owner decision in list/map changes,
list reservation, list-to-string conversion, string growth and erased-box
extraction. A release's last-owner branch belongs to that release rather than
an additional uniqueness operation. The existing checked retain/release
counters count actual operations on heap objects; null/static no-ops and the
collector's private trial-deletion bookkeeping do not add counts. Unchecked
builds keep their original loads and comparisons.

Those no-ops still cost a call. The table's `No-op calls` column counts the
retains and releases that found no count: a null, static data or a frame slot.
It is outside `total` and outside the gate.

The stage case [operation-counter-loop.hd](../../crates/hd_driver/tests/cases/counts/operation-counter-loop.hd)
and its focused driver test execute both loop variants. The extra owner adds
1,000 retains and 1,000 releases and fails the same comparator used by CI.

## Conservative emitted sites

CI also compiles only the study and runtime benchmarks and counts static
emitted sites by reason, including std and generated runtime bodies. This
counts the places where a module contains ownership work, independently of
how often a benchmark executes them. The Rust corpus reader's `bench`
selection reads no conformance, stage, example or guide files.

```sh
node bench/counts/conservative-sites.ts
node bench/counts/conservative-sites.ts --record --reason "Why this conservative site is still needed"
node --test bench/counts/conservative-baseline.test.ts
```

[conservative-sites.json](conservative-sites.json) limits each conservative
reason separately. A reduction under one reason cannot hide growth under
another. An increase or a new reason needs its measured count and a fresh
explanation, checked against the previous commit's budget. Main refreshes
lower limits automatically, including zero for reasons that disappear.
Named reasons remain visible in the report without spending this budget.

The command forces the optimized guest release pipeline and the complete
benchmark set, using native dev builds with two workers. A skipped, panicked,
missing or duplicate benchmark fails the gate. It writes `actual.json`,
`summary.md` and raw corpus TSVs under `target/conservative-sites`; CI puts
the reason table in its summary and uploads those artifacts.

`--previous FILE` checks increase explanations against the earlier budget;
`--output DIR` chooses scratch output. `--input DIR` reuses the corpus TSVs
from a completed run, for review or recording without compiling again. The
comparator tests sabotage the same CI command with one extra conservative
site and require it to exit unsuccessfully.
