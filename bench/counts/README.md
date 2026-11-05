# Dynamic Count-Operation Gate

Every study and runtime benchmark runs once with the optimized guest release
pipeline, the checked runtime, default V8 and seed 0. The table in
[baseline.json](baseline.json) records retains, releases, sole-owner tests and
their sum. Each program has its own limit; improvements elsewhere do not
hide an increase. Counts use decimal strings to preserve u64 precision.

Build the native tool in dev mode, then check or record the table:

```sh
CARGO_BUILD_JOBS=4 cargo build -j4 -p hd_driver --example count_ops
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

The stage case [operation-counter-loop.hd](../../crates/hd_driver/tests/cases/counts/operation-counter-loop.hd)
and its focused driver test execute both loop variants. The extra owner adds
1,000 retains and 1,000 releases and fails the same comparator used by CI.
