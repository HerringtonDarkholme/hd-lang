# Compiler speed audit (measure only, 2026-10-04)

Status: audit only. No code was changed for this report; every number below
comes from a command that was run, shown next to the number. Nothing in this
report is accepted behavior and no fix is approved.

Method: each timing is the median of 3 runs. Wall time and CPU time come from
`/usr/bin/time -p` where shown; in-process timings use `performance.now()`
medians of 5 rounds per process, repeated in 3 processes. Scratch scripts and
profiles live in `/tmp/hd-perf-audit/` (throwaway, not committed). Load
averages are the 1-minute value from `uptime` at run start.

Load caveat: this is a shared 14-core box with 11 active users. The plan asked
for load below 4, but the 1-minute average was below 4 for only three
measurement windows (sizeguard run 1 at 3.47, the dogfood rerun at 3.96, the
first website build at 3.50). Everything else started above 4 and is labeled
with its actual load. `perf:check` scores are load-normalized (each case is
divided by a reference program timed in the same process): all 11 cases stayed
within 0.90x-1.09x of `baseline.json` across runs, so the timing shapes below
are trustworthy even where absolute milliseconds are inflated.

## 1. Baselines

### 1a. `test:portable` (2,116 cases)

Command: `/usr/bin/time -p pnpm run test:portable`

| run | wall | user | sys | result | load at start |
| --- | --- | --- | --- | --- | --- |
| 1 | 45.29 s | 453.05 s | 7.28 s | 2116 passed, 0 failed | 19.17 |
| 2 | 45.01 s | 457.85 s | 6.22 s | 2116 passed, 0 failed | 15.84 |
| 3 | 45.80 s | 457.25 s | 6.96 s | 2116 passed, 0 failed | 15.14 |

Median wall: 45.29 s. CPU/wall ratio is ~10x (8-worker pool, see finding F13).
Per-case CPU is ~453 s / 2116 ~= 214 ms.

### 1b. Node test suites

Command:
`node --test --experimental-strip-types 'test/**/*.test.ts' 'website/test/*.test.ts' 'website/playground/test/**/*.test.ts' --test-reporter=tap`

| run | wall (`time -p real`) | user | `duration_ms` total | tests | load at start |
| --- | --- | --- | --- | --- | --- |
| 1 | 101.99 s | 380.10 s | 101923 ms | 717 pass, 0 fail | 12.04 |
| 2 | 56.41 s | 376.95 s | 56361 ms | 717 pass, 0 fail | 12.79 |
| 3 | 60.59 s | 385.56 s | 60545 ms | 717 pass, 0 fail | 24.35 |

Median wall: 60.59 s. Run 1 shows how load-sensitive the wall clock is; CPU
time is stable (~380 s), so CPU per test file is the comparable number.

Note: per-test times appear as `(Nms)` after each test name, not as
`duration_ms` lines; only the run total is a `duration_ms` line. The 10
slowest tests of the median run (run 3):

| test (run 3) | ms |
| --- | --- |
| website build | 13809.0 |
| the in-process adapter reports what the spawned CLI reports | 13531.7 |
| rewrite reads batch 34 from git history | 11345.7 |
| REPL values render structurally with their types | 11299.6 |
| the bundled examples run | 10862.3 |
| renders every spec chapter and guide file under the Pages base | 7744.5 |
| std tests pass: property.hd | 6862.0 |
| the hd executable passes its arguments and sets the exit status | 6442.1 |
| each tour snippet runs and tests as its page says | 6299.6 |
| serves a playground build at playground/ when one exists | 5616.7 |

The ordering is load-sensitive (run 1's slowest was the tour-snippets test at
36724 ms under end-of-run load 97). The stable pattern across runs is that the
slowest tests are suites that compile and run many hd programs end to end
(examples, tour, property.hd, website build), not single unit tests.

### 1c. `perf:check`

Command: `/usr/bin/time -p pnpm run perf:check`. Median wall 57.92 s
(64.24 / 57.92 / 57.49). Table from run 3 (wall 57.49 s, start load 13.28):

| case | scales | small ms | large ms | parse ms | check ms | growth (limit) | vs baseline | verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| list-nest | 300, 1000 | 51 | 564 | 2.5 | 561 | 11.1 per 3.33x (21.7) known | 0.97x | pass |
| open-lets-reverse | 5, 50 | 36 | 462 | 167 | 267 | 12.8 per 10x (23.4) known | 1.02x | pass |
| method-calls-one-var | 30, 300 | 36 | 388 | 135 | 250 | 10.7 per 10x (21.9) known | 0.91x | pass |
| add-many-types | 10, 30 | 119 | 874 | 1.5 | 872 | 7.3 per 3x (14.0) known | 0.95x | pass |
| overload-candidates | 10, 100 | 66 | 731 | 1.6 | 729 | 11.0 per 10x (12.0) | 1.05x | pass |
| big-body | 10, 100 | 73 | 763 | 314 | 463 | 10.5 per 10x (20.9) known | 1.08x | pass |
| mutual-bounds | 10, 30 | 40 | 946 | 2.5 | 943 | 23.6 per 3x (43.6) known | 0.90x | pass |
| closures-shared-var | 100, 300 | 101 | 658 | 21 | 637 | 6.5 per 3x (13.1) known | 1.09x | pass |
| polymorphic-recursion | 30, 300 | 62 | 383 | 64 | 320 | 6.2 per 10x (12.0) | 0.95x | pass |
| late-conflict | 10, 100 | 40 | 436 | 194 | 243 | 10.8 per 10x (24.6) known | 0.95x | pass |
| occurs-check | 40, 400 | 42 | 424 | 52 | 372 | 10.1 per 10x (20.0) known | 0.98x | pass |

Run 3 footer: `reference program: 179 ms (median); gate time 57.2 s; 11 of 11
cases pass` (run 1: gate 63.3 s, reference 176 ms; run 2: gate 57.1 s,
reference 181 ms). Two readings that matter for the findings: `check ms` is
50-99% of every large case (inference dominates, not parsing), and
`mutual-bounds` grows ~24x per 3x input (near-cubic).

Context, not an anomaly: `add-many-types` rejects with `type-mismatch` at both
gate sizes (checked directly: `@30 check=947 ms codes=[type-mismatch]`,
`@10 check=216 ms codes=[type-mismatch]`). `baseline.json` records
`"codes": ["type-mismatch"]` for it, so the gate is timing the reject path by
design and passes.

### 1d. `website:build`

Command: `/usr/bin/time -p pnpm run website:build` (5 runs; first three were
the plan's triple, last two are repeats after the load spike).

| run | wall | user | sys | load at start |
| --- | --- | --- | --- | --- |
| 1 | 5.21 s | 29.88 s | 5.00 s | 3.50 |
| 2 | 4.32 s | 28.79 s | 4.22 s | 6.99 |
| 3 | 4.46 s | 29.01 s | 4.30 s | 13.15 |
| 4 | 5.13 s | 30.44 s | 4.82 s | 7.50 |
| 5 | 4.55 s | 29.35 s | 3.90 s | 9.23 |

Median of all five: 4.55 s. Output each run:
`website: 74 pages, base /hd-lang/, with the playground` and
`grammar: 264 rules (267 definitions), 699 of 759 references linked`.
CPU/wall is ~6.5x (parallel build); the build itself drives the load spike in
runs 2-5.

### 1e. Size-guard programs, in process (`compileToWat` from `src/compiler.ts`)

Command: `/usr/bin/time -p node --experimental-strip-types
/tmp/hd-perf-audit/sizeguard.mts` (5 timed compiles per process; 3 processes).

tiny =
`fn check(a: i32) -> bool: a == 2 || a < 0\n\npub fn main() -> void $ Console:\n    println(check(2))\n`;
one-test = `tests:\n    it("works"):\n        pass\n`;
empty = `pub fn main() -> void: pass\n` (std-share control, Phase 2).

| program | run | cold (run 1) | warm median (runs 2-5) | all 5 runs | load at start |
| --- | --- | --- | --- | --- | --- |
| tiny | 1 | 258.6 ms | 95.7 ms | 258.6, 100.5, 106.8, 91.0, 78.7 | 3.47 |
| tiny | 2 | 253.2 ms | 99.5 ms | 253.2, 100.5, 123.4, 98.5, 81.8 | 11.02 |
| tiny | 3 | 268.7 ms | 96.3 ms | 268.7, 97.3, 109.4, 95.3, 81.2 | 11.02 |
| one-test | 1 | 136.1 ms | 120.1 ms | 136.1, 129.8, 125.7, 113.4, 114.6 | 3.47 |
| one-test | 2 | 145.0 ms | 126.3 ms | 145.0, 136.4, 127.5, 125.1, 121.0 | 11.02 |
| one-test | 3 | 145.0 ms | 129.0 ms | 145.0, 133.6, 129.5, 128.5, 121.1 | 11.02 |
| empty | 1 | 73.7 ms | 74.7 ms | 73.7, 76.1, 75.3, 74.2, 71.9 | 3.47 |
| empty | 2 | 75.4 ms | 88.5 ms | 75.4, 76.3, 94.2, 93.1, 83.9 | 11.02 |
| empty | 3 | 76.0 ms | 73.9 ms | 76.0, 75.4, 74.0, 73.1, 73.8 | 11.02 |

Median cold: tiny 258.6 ms, one-test 145.0 ms, empty 75.4 ms. Median warm:
tiny 96.3 ms, one-test 126.3 ms, empty 74.7 ms. Cold is ~2.7x warm for tiny
(JIT + first Binaryen-free path); one-test barely warms (derivation-heavy,
see Phase 2).

### 1f. Largest real programs (`examples/dogfood/`)

Commands: `/usr/bin/time -p node --experimental-strip-types bin/hd.js check|run|test FILE`
(3 runs each; sizes: calc 241 lines/8002 B, orders 122/5137, tasks 173/6609,
textstats 75/3177). All runs below started at load 3.96.

| file | command | wall runs | median wall | median user | outcome |
| --- | --- | --- | --- | --- | --- |
| calc.hd | check | 0.67, 0.53, 0.54 | 0.54 s | 0.93 s | ok |
| orders.hd | check | 0.50, 0.48, 0.48 | 0.48 s | 0.91 s | ok |
| tasks.hd | check | 0.49, 0.45, 0.46 | 0.46 s | 0.85 s | ok |
| textstats.hd | check | 0.40, 0.40, 0.37 | 0.40 s | 0.68 s | FAILS: `textstats.hd:51:78: type-mismatch: expected fn(i32,u32)->i32$row:R, found fn(u32,u32)->u32` |
| calc.hd | run | 0.83, 0.83, 0.83 | 0.83 s | 1.78 s | runs, prints 3 lines |
| orders.hd | test | 0.88, 0.82, 0.82 | 0.82 s | 1.82 s | 4 passed |
| tasks.hd | test | 0.82, 0.81, 0.81 | 0.81 s | 1.80 s | 8 passed |
| textstats.hd | run | 0.39, 0.38, 0.39 | 0.39 s | 0.68 s | same type-mismatch as check |

`textstats.hd` does not check on this commit, so its `run` number is a
fail-fast timing, not a compile-and-run timing. `run` costs ~0.3 s over
`check` on the same file (calc 0.83 vs 0.54); that delta is emit + assemble +
instantiate + run + ~0.3 s of CLI startup (in-process empty compile is 75 ms,
so ~0.3 s of every CLI number above is Node startup + type stripping).

## 2. Phase breakdown

Command: `node --experimental-strip-types /tmp/hd-perf-audit/phases.mts`
(5 in-process rounds per workload per process; 3 processes; medians of the 3
process medians). Each stage wraps one exported compiler function; the
`check` row is the full `check()` and therefore includes the join/traits/
derivation work, which are also timed standalone so their share of `check`
is visible. Signature inference is not a separate stage in this codebase: it
runs inside `lowerCheckedProgram` (`src/checker/program-lower.ts:151`), so
"signature inference + body checking + lowering" are one row by construction.

| stage (function) | tiny (98 B) | calc (8002 B) | mutual-bounds@30 (20353 B) |
| --- | --- | --- | --- |
| parse (`parse`) | 0.15 ms (0.2%) | 1.14 ms (0.4%) | 2.47 ms (0.3%) |
| std join (`withStandardLibrary`) | 6.22 ms | 7.93 ms | 5.97 ms |
| trait/support fixed point (`withStandardTraits` + `withTypedDerivationSupport`) | 1.00 ms | 2.88 ms | 1.91 ms |
| derivation (`withTypedDerivation`) | 1.74 ms | 13.32 ms | 2.83 ms |
| check total (`check`: signatures + inference + bodies + lower) | 86.66 ms (92.0%) | 183.42 ms (60.9%) | 970.00 ms (98.3%) |
| Wasm emit (`emitWat`, includes linkWat) | 3.16 ms (3.4%) | 30.61 ms (10.2%) | 4.65 ms (0.5%) |
| Wasm assemble (`assembleWat` = Binaryen parse + validate) | 4.26 ms (4.5%) | 85.96 ms (28.5%) | 9.26 ms (0.9%) |
| instantiate + run entry (`instantiate`, tiny only) | 2.07 ms | n/a | n/a |

Percentages are of end-to-end (parse + check + emit + assemble): tiny
94.2 ms, calc 301.1 ms, mutual-bounds 986.4 ms. Raw process medians:
tiny check 102.17 / 83.05 / 86.66; calc check 190.92 / 177.50 / 183.42;
mutual-bounds check 965.47 / 980.44 / 970.00. Loads during these runs:
7.60 down to 5.43.

What this says: `check` is 61-98% of every compile; parsing never exceeds
0.4%. The join/derivation stages are a visible minority of `check` on tiny
(9.0 of 86.7 ms, ~10%) and calc (24.1 of 183.4 ms, ~13%), so the bulk of
`check` is signatures + inference + body checking + lowering. On calc, emit +
assemble are 116.6 of 301.1 ms (39%): the run path is emitter-dominated for
mid-size programs. On mutual-bounds, inference inside `check` is everything.

Std vs user lines (tiny program): the empty program compiles warm in
74.7 ms vs tiny's 96.3 ms, so ~78% of a small warm compile is the joined
standard library and ~22% (21.6 ms) is the user's two lines. Object counts
from `/tmp/hd-perf-audit/counts.mts`: tiny parses to 2 functions, joins to 89
(87 std), and checks to 498 HIR functions of which 495 are marked standard
(3 user); empty is 1 -> 88 -> 497 (495 standard + 2 user). Closures 40, data
types 36, implementations 220, enums 5 in both. So a 2-line program pays for
checking ~500 HIR functions, ~495 of them std. This is the deferred item in
section 4.

## 3. Top findings, ranked by measured cost

Profiles: `node --cpu-prof --cpu-prof-dir=/tmp/hd-perf-audit/prof
--experimental-strip-types /tmp/hd-perf-audit/work-{tiny,calc,perf}.mts`
(tiny x20 in one process, calc x8, mutual-bounds@30 + add-many-types@30 x2).
Summarizer: `node /tmp/hd-perf-audit/top25.mjs PROF` prints self time (hit
fraction x total) and inclusive time per function with file and line.
Profile totals: tiny 1934 ms / 1546 samples; calc 2521 ms / 2023 samples;
perf (mb+amt) 4633 ms / 3658 samples. Only profile-backed items are listed;
section 4 names what was checked and cleared.

### F1. Trial speculation snapshots the whole checker per candidate call

File and line: `src/checker/call-speculation.ts:24-80` (`speculate`),
restore closure at `:58`, snapshot at `:27`; call sites
`src/checker/expression-calls.ts:826` (method candidates) and `:1444`
(associated-function candidates). Each `speculate` walks the checker's
reachable graph (`Object.getOwnPropertyDescriptors` + `Reflect.ownKeys` per
object, Map/Set entry copies), and the selected candidate is checked again
outside the transaction, so every generic call costs at least 2x.
Workload: mb+amt profile. The restore closure (`call-speculation.ts:58`)
takes 436.9 + 377.4 + 95.0 ms self (909 ms, 19.6% of the 4633 ms profile);
`snapshot` (`:27`) rows add ~1000 ms inclusive on top.
Growth: linear in candidates x call sites, multiplied by the F4 fixpoint that
re-runs inference: mutual-bounds grows 23.6x per 3x input.
Proposed fix: snapshot only the inference state (the substitutions and bounds
maps) instead of the whole checker object graph. Keep the recheck of the
selected candidate outside the transaction as the commit step.
Expected gain: 10% on inference-heavy workloads. Estimated as half of the
19.6% self-share (the walk itself goes away; per-object descriptor work for
the small state remains).
Correctness risk: high; a too-narrow snapshot silently keeps trial mutations
(the exact bug class the transaction exists to prevent). Covered by the
checker trial unit tests (`test/**/*.test.ts`, e.g. the nested/sparse-journal
trials) and the full portable typing suite (1849 language cases).

### F2. `genericTypeName` runs a regex per call on the hottest path

File and line: `src/checker/shared.ts:439`
(`const match = /^generic:([^?[\\](),]+)$/.exec(type)`), called from
`containsGenericParameter` (`:608`), which recurses per type node.
Workload: mb+amt profile. `genericTypeName` self is 134.2 + 117.8 = 252 ms
and the `^generic:` regex rows are 62.1 + 55.7 = 118 ms: ~370 ms combined,
8.0% of the profile.
Growth: per type-string comparison, so linear in bound x argument traffic;
rises with every other inference cost.
Proposed fix: return early unless `type.startsWith("generic:")`, and only
then run the regex. Two lines, no behavior change.
Expected gain: 5-8% on inference-heavy workloads (the full 8.0% combined
share is the ceiling; the prefix check itself still scans a few chars).
Correctness risk: negligible; pure predicate, same result. Covered by typing
fixtures and the perf gate (scores must stay within 1.5x).

### F3. Types re-parsed from text on every inspection

File and line: `src/types.ts:340` (`functionParts`, self 73.5 + 63.3 =
136.8 ms), `src/types.ts:141` (`nominalGenericParts`, self 49.4 ms),
`src/checker/shared.ts:608` (`containsGenericParameter`, inclusive 651.0 +
543.3 ms across two nodes), plus the `Name=type` regex rows
(`^([A-Za-z_][A-Za-z0-9_]*)=(.+)$`, self 120.3 + 89.9 = 210 ms).
Workload: mb+amt profile; combined self ~400 ms, ~8.6% of the profile.
Growth: each inspection is O(type-string length); nested types re-scan
inner text at every level, so deep types (list-nest: 11.1x per 3.33x) pay
repeatedly.
Proposed fix: memoize the text parsers in a Map keyed by the type string
(one wrapper per function). Longer term, intern parsed types; that is a
bigger change and not proposed here.
Expected gain: 3-5% on inference-heavy workloads (cache hits remove the
char scans; Map overhead keeps part of the share).
Correctness risk: low; pure functions, cache is value-keyed. Covered by
typing fixtures and the gate.

### F4. Bound-inference fixpoint rescans everything per round

File and line: `src/checker/bound-inference.ts:25`
(`inferTypesThroughBounds`), fixpoint loop `:151-168`, per-bound
instantiation scan `:34`, supertrait walk with a linear trait lookup at
`:53 (`[...traitTypes.values()].find(...)`), `JSON.stringify` dedup keys at
`:110` and `:129`.
Workload: mutual-bounds@30, `check` 970 ms = 98.3% of the 986 ms compile;
gate growth 23.6x per 3x input (near-cubic), vs-baseline 0.90x.
Growth: each fixpoint round calls `instantiationsFor` for every unresolved
parameter, each of which scans implementations and supertraits; rounds are
proportional to bound-chain length, hence cubic on mutual-bounds.
Proposed fix: index traits by index instead of the `:53` linear find, and
replace the full rescan with a worklist of parameters whose inputs changed.
Keep the `JSON.stringify` keys (they did not show up in the profiles).
Expected gain: 20% on bound-heavy workloads (the `:34` rows carry 783.9 +
653.5 ms inclusive; even a worklist that halves re-scans plus an O(1) trait
lookup removes about a fifth of the profile).
Correctness risk: high; the fixpoint order affects which ambiguous bounds
resolve first. Covered by `perf:check` (growth ratios), the trait/bound
fixtures, and the portable typing suite.

### F5. `withStandardSource` rebuilds every std object per compile

File and line: `src/checker/standard-provenance.ts:54`: maps the joined
program object-by-object (`Object.fromEntries(Object.entries(value).map(...))`)
to stamp physical/logical spans.
Workload: tiny profile. Four `withStandardSource` nodes carry inclusive
107.6 + 61.3 + 121.4 + 30.0 = 320 ms of the 1934 ms profile (16.5%), while
the standalone join stage is only ~6 ms: the rebuild, not the join, is the
cost.
Growth: constant per compile (std is fixed size), so it is a floor under
every small compile, including each of the 2116 portable cases.
Proposed fix: stamp std spans once when std sources are first parsed (they
are static), and skip the per-compile tree rebuild. The joined program keeps
identical spans.
Expected gain: 10-15% on small compiles (most of the 16.5% inclusive share;
some entry/exit cost remains).
Correctness risk: medium; spans feed every diagnostic location. Covered by
diagnostic snapshot tests, `check --format json` tests, and the portable
suite (diagnostic assertions per fixture).

### F6. Implementation clash scan is quadratic in impl count, per compile

File and line: `src/checker/program-implementations.ts:764`
(`prepareImplementations`, inclusive 203.9 ms of the tiny profile, 10.5%),
with the pairwise overlap scan around `:256-260` (`targets.find` /
`heads.some` over all implementations, 220 of them on every compile).
Workload: tiny profile; calc shows the same node at 153.3 ms inclusive.
Growth: constant per compile today (220 impls -> ~48k pair checks), linear in
std impl count over time.
Proposed fix: skip the pairwise clash check between two standard
implementations (std does not change between compiles in one process; user
impls are still checked against everything). No cache of checking results,
only of the std-vs-std proof obligation.
Expected gain: 3-8% on small compiles (the clash scan is part of the 10.5%
node; preparation of declarations remains).
Correctness risk: medium; a missed overlap is a soundness hole. Covered by
the impl-overlap fixtures and the portable typing suite. Note: fully caching
prepared implementations would overlap the deferred std cache (section 4)
and is not proposed here.

### F7. `collectReadLocals` walks each function body after checking

File and line: `src/checker/context.ts:89-102` (generic
`Object.entries(node)` recursion), called once per checked function body at
`:349` for the `unused-local-binding` warning.
Workload: tiny profile, two nodes self 8.8 + 7.5 ms over 498 HIR functions
(one walk per body, so the count per compile equals the HIR function count:
498 for tiny, 883 for calc).
Growth: linear in total HIR size (dominated by the ~495 std functions on
small programs).
Proposed fix: record read locals during checking (mark on bind) instead of
re-walking each finished body. The warning output is unchanged.
Expected gain: ~1% on small compiles, growing linearly with program size.
Correctness risk: low; warning-only. Covered by `unused-local-binding`
fixtures.

### F8. `hasPanicDetail` walks the whole program per emit

File and line: `src/emitter/intrinsics.ts:90-99` (reflection walk over every
function body), called once per `compileToWat` at `:147`.
Workload: below the top-25 cutoff on all three profiles (sub-millisecond on
tiny), but it is one full-tree walk per compile by construction, over the
same ~500 HIR functions as F7.
Growth: linear in HIR size.
Proposed fix: set a flag when the checker produces a panic-with-message
node and read the flag at `:147` instead of walking.
Expected gain: under 1% on small compiles; removes a linear walk that grows
with every std addition.
Correctness risk: low; boolean must exactly match the walk. Covered by
emitter panic-detail tests and portable runtime fixtures.

### F9. Capture conversion rebuilds every function unconditionally

File and line: `src/checker/captured-cells-walk.ts:24`
(`mapCapturedFunction`), applied to all functions and closures at
`src/checker/captured-cells.ts:88-89` (498 + 40 objects on tiny).
Workload: tiny profile, inclusive 30.0 ms (1.6%).
Growth: linear in function count per compile.
Proposed fix: return the input unchanged when a function has no captures
and no closure content to rewrite (identity fast path).
Expected gain: ~1% on small compiles.
Correctness risk: low; identity must be reference-safe for downstream maps
(keyed by identity, not shape - verify at fix time). Covered by closure and
capture fixtures.

### F10. WAT is parsed twice after being generated

File and line: `src/emitter/emitter.ts:877` (`emitWat` ends with
`linkWat(...)`), `src/emitter/link-wat.ts:90` (`linkWat` re-parses the
generated WAT into forms), then `src/wasm.ts:41` (Binaryen `parseText`
parses it again).
Workload: calc. Standalone `linkWat` on calc's 474,086-char WAT is 16.2 ms
(median of 5: 15.9, 16.0, 16.2, 16.4, 26.4), which is 53% of calc's 30.61 ms
emit; calc assemble is another 85.96 ms, so post-WAT parsing totals ~102 ms
of the 301 ms compile (34%).
Growth: linear in WAT size (calc's WAT is ~59x its source).
Proposed fix: emit only reachable functions so the link pass has nothing to
remove, then delete the `linkWat` call; longer term, drive Binaryen's API
instead of WAT text. The first step keeps byte-identical output.
Expected gain: ~5% of calc-class compiles from removing `linkWat` (16.2 ms
measured directly); the Binaryen re-parse stays.
Correctness risk: medium; the linker also drops `elem declare` roots
incorrectly kept code may change instantiation. Covered by emitter link
tests and portable runtime fixtures (they execute the binary).

### F11. `refreshImplementations` refilters all impls per scope event

File and line: `src/checker/context.ts:264-274` (filters
`allImplementations`, 220 entries), called from `:237`, `:261`, `:447`.
Workload: calc profile, self 26.2 + 7.5 ms (1.3% of 2521 ms).
Growth: per scope event x 220 impls; more local-impl scopes means more
events.
Proposed fix: skip the refresh when the scope carries no local
implementations (the common case: the filter is then a no-op copy).
Expected gain: ~1% on mid-size compiles.
Correctness risk: low; the skip condition is exact (empty local set).
Covered by local-implementation scope fixtures.

### F12. Member lookup scans all traits linearly per lookup

File and line: `src/checker/member-lookup.ts:277` and `:457`
(`[...this.traitTypes.values()].find(...)` per member resolution),
`:259` (`inherentMethods.find` per call); nearby profile rows
`ambiguousProjection` (`associated-bindings.ts:165`, self ~25 ms) and
`traitsByIndex` (`:25`, self 11.3 ms) on tiny.
Growth: per lookup x trait count; constant today, linear in std traits.
Proposed fix: index `traitTypes` by index in a Map built once per check.
Expected gain: 1-2% (below top-25 self time; the rows above are the measured
share).
Correctness risk: low; lookup order must stay deterministic for
ambiguous-method errors (keep first-match semantics). Covered by member
resolution and ambiguity fixtures.

### F13. Portable harness caps workers at 8 on a 14-core box

File and line: `test/run-portable.ts:66`
(`Math.min(8, availableParallelism())`, overridable with `--jobs` /
`HD_TEST_JOBS`). The adapter (`test/hd-adapter.ts`) already does the right
thing: one worker pool, compiler imported once per worker, no process spawn
or `node_modules` reads per case.
Workload: `test:portable` median wall 45.29 s at user CPU ~455 s (10x
parallelism: 8 workers plus the driver). On this 14-core box 6 cores sit
idle.
Growth: suite wall scales as CPU/workers while cases stay CPU-bound.
Proposed fix: default to `availableParallelism()` instead of
`min(8, ...)`. One line; `--jobs` still overrides for small machines.
Expected gain: ~30% suite wall reduction here (455 s / 14 ~= 32 s plus
driver overhead, vs 45 s now). Estimated by division, not measured.
Correctness risk: negligible (scheduling only); worker memory x14 is the
only cost. Covered by the suite itself. The per-case ~75 ms std-check tax
(2116 x 75 ms ~= 159 s CPU, ~35% of suite CPU) is the deferred cache item,
not this fix.

Checked and cleared (looked for, not in any top-25 profile, not findings):
`JSON.stringify` as a key (`bound-inference.ts:110,129`,
`checker/shared.ts:697`, `emitter/value-comparison.ts:273,380,492`)
registers no top-25 self time, so key building is not today's cost;
`+=` string building in the emitter (`rg -n "\\+= " src/emitter/emitter.ts`
returns nothing; `link-wat.ts` keeps source slices); per-test-case module
loading or `node_modules` reads in `test/run-portable.ts` (workers import the
compiler once, cases run in-process); `implementationsFor` already has a
head-index cache (`implementation-index.ts:27-36`), so impl filtering is not
an unindexed linear scan.

## 4. Deferred: the checked-std cache (no cache now, per owner)

Measured cost of re-checking the standard library on every compile:

- Empty program (joins std, checks almost nothing of its own) compiles warm
in 74.7 ms vs tiny's 96.3 ms (sizeguard medians): ~78% of a small warm
compile is std, ~22% (21.6 ms) is the user's two lines. Command:
`node --experimental-strip-types /tmp/hd-perf-audit/sizeguard.mts`.
- Per compile, the checker walks 495 standard HIR functions out of 498
(counts from `/tmp/hd-perf-audit/counts.mts`; 220 implementations, 36 data
types, 40 closures ride along).
- Across the portable suite this is ~75 ms x 2116 cases ~= 159 s CPU, ~35%
of the suite's ~455 s CPU.
- CLI `hd check` on dogfood pays ~0.3 s of Node startup/type-stripping plus
the same per-compile std check (calc: 0.54 s wall vs 0.18 s in-process
check).

Not recommended now (owner direction "no cache now"); recorded here so the
number exists when the decision is revisited. Findings F5 and F6 reduce
parts of this cost without a checked-std cache and are fair game.

## 5. Suggested order of fix commits (biggest gain per line changed first)

Each fix is one commit and must report before/after numbers for
`test:portable`, `perf:check`, and the two size-guard programs (tiny cold +
warm, one-test cold + warm) in the commit message.

1. F2 `startsWith("generic:")` guard (`shared.ts:439`): 2 lines, ~5-8% on
inference workloads.
2. F5 stamp std spans once (`standard-provenance.ts:54`): one load-time
pass, ~10-15% on small compiles.
3. F9 capture-conversion identity fast path (`captured-cells-walk.ts:24`):
few lines, ~1%.
4. F8 panic-detail flag (`emitter/intrinsics.ts:90,147`): checker flag plus
one read, under 1% but removes a walk that grows with std.
5. F12 trait index map (`member-lookup.ts:277,457`): small, 1-2%.
6. F11 skip empty local-impl refresh (`checker/context.ts:264`): few lines,
~1%.
7. F7 mark-read-during-check (`checker/context.ts:89,349`): medium, ~1% and
linear.
8. F3 memoize type-text parsers (`types.ts`, `shared.ts:608`): small, 3-5%
on inference workloads.
9. F6 skip std-vs-std clash recheck (`program-implementations.ts:256-260`):
small, 3-8% on small compiles.
10. F10 drop the `linkWat` re-parse (`emitter.ts:877`): medium (emit
reachable-only first), ~5% on calc-class compiles, 16.2 ms measured.
11. F13 worker default to `availableParallelism()` (`run-portable.ts:66`):
one line, ~30% suite wall on many-core boxes (suite-only gain).
12. F4 bound-inference worklist + trait index (`bound-inference.ts:25-168`):
large, ~20% on bound-heavy workloads, highest risk of the algorithmic fixes.
13. F1 narrow speculation snapshot (`call-speculation.ts:24-80`): large,
~10% on inference-heavy workloads, highest correctness risk; last because a
mistake here silently keeps trial mutations.

Reproduction commands for every number: Phase 1 commands in section 1,
`node --experimental-strip-types /tmp/hd-perf-audit/phases.mts` for
section 2 (scripts are throwaway in `/tmp/hd-perf-audit/`, not committed),
and `node /tmp/hd-perf-audit/top25.mjs
/tmp/hd-perf-audit/prof/<profile>.cpuprofile` for section 3. Only
`audit/compiler/perf-audit.md` is committed by this task.



