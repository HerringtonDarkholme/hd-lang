# Compiler Speed: Open Findings

Status: the open findings of the 2026-10-04 speed audit. Fixed and removed:
F2 (3edd4e36), F4 (4fba4be5), F8 (2f6dcc50), F9 (06b12ff7), F11 (0bea744a),
F12 (ad85d9e5), F13 (b44e48c0). Baselines, phase breakdown and full
profiles are in git history (`git show a3f8e3b3:audit/compiler/perf-audit.md`).
Slow-compile profiles per perf case: [../job1-slow-compile-profiles.md](../job1-slow-compile-profiles.md).
F3 is task #255 (type interning); F5, F6 and the std cache are task #262
(deferred by the owner).

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

## Deferred: the checked-std cache (no cache now, per owner)

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
