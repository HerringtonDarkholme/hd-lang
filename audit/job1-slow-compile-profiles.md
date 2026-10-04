# Job 1 audit: slow-compile profiles

> Posted in chat 2026-10-04 during the read-only audit session; logged here verbatim by owner request. HEAD 4636f546. Body below is the posted report, unedited.

Job 1 done. All numbers below are from commands I ran; no repo files were changed (`git status --short` is clean).

## Method

- Command: `node --stack-size=8000 --experimental-strip-types bin/hd.js check FILE` (`--stack-size=8000` is the perf harness's own value, `test/perf/inference/spawn.ts:17`). Machine: Apple M3 Max arm64, darwin, node v24.19.0.
- Base wall time = median of 3 timed runs after 1 warm-up; 2×/4× = 1 timed run after warm-up. Profiles: same command plus `--cpu-prof`, self time per function from `timeDeltas`, 1-based `file:line` verified against the tree.
- Generators: the four `gen.ts` cases via `test/perf/inference/gen.ts`; the three kept-out cases via a verbatim copy of their generators from git history (`828e1d0e:test/perf/inference/gen.ts`, kept as scratch `/tmp/muse-job1/gen-keptout.ts`).
- HEAD moved twice mid-job via background `pull --ff-only` (`d082975f` → `94f1c3ab` → `4636f546`). Timings taken at `94f1c3ab`; every base re-run once at `4636f546` (+0–7%, i.e. noise), and all profiles below re-taken at `4636f546`.

## Headline findings (brief is stale in two places)

- `obligation-cascade` no longer times out: depth 1000 checks in 1243 ms (brief: >45 s). Commit `4fba4be5` ("bound inference revisits only parameters with fresh inputs") fixed it; its message quotes "x100 over 45 s to 0.9 s", matching my 1243 ms wall (≈0.9 s + ~0.3 s startup).
- `mutual-bounds` is likewise ~15× faster than the brief's numbers (scale 200 in 2.2 s), still ~quadratic in the measured range.
- `list-nest` needs the harness 8 MB stack: with node defaults it crashes at depth ≥1000 in the checker and ≥2000 in the parser (details in its table).

## 1. nested-tuple (depth = nesting; base depth 26, 2× = 52, 4× = 104)

| size | wall |
| --- | --- |
| depth 26 | 5429 / 5433 / 5442 ms, median **5433** |
| depth 52 (2×) | **crash**, 1236 ms |
| depth 104 (4×) | **crash**, 481 ms |

Crash boundary is between depth 26 (5.4 s) and 28 (crash). Diagnostic at base: `type-mismatch: expected u8, found i32` at 30:21.

Top 10 by self time (base, 5552 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 4784.2 | 86.2 | `tupleParts` | `src/types.ts:68` |
| 127.5 | 2.3 | `mutableInner` | `src/types.ts:43` |
| 126.9 | 2.3 | `tupleType` | `src/types.ts:92` |
| 66.3 | 1.2 | (garbage collector) | runtime |
| 32.5 | 0.6 | `getCompileCacheEntry` | runtime |
| 25.4 | 0.5 | `read` | runtime |
| 21.8 | 0.4 | `ModuleWrap` | runtime |
| 21.5 | 0.4 | `compileForInternalLoader` | runtime |
| 12.0 | 0.2 | (program) | runtime root |
| 9.6 | 0.2 | `traitWithMember` | `src/checker/member-lookup.ts:274` |

Crash stack (`hd check`, depth 32, exit 1, reproduced at current HEAD):

```
RangeError: Invalid string length
    at Array.join (<anonymous>)
    at tupleType (file:///Users/hd/code/test/hd-lang/src/types.ts:93:23)
    at FunctionChecker.checkLiteralExpression (file:///Users/hd/code/test/hd-lang/src/checker/expression-literals.ts:347:17)
    at FunctionChecker.checkExpressionRaw (file:///Users/hd/code/test/hd-lang/src/checker/checker.ts:48:12)
    at FunctionChecker.checkExpression (file:///Users/hd/code/test/hd-lang/src/checker/checker.ts:26:24)
    at FunctionChecker.checkBindingStatement (file:///Users/hd/code/test/hd-lang/src/checker/statements.ts:731:20)
    at FunctionChecker.checkStatement (file:///Users/hd/code/test/hd-lang/src/checker/statements.ts:95:21)
    at FunctionChecker.checkStatements (file:///Users/hd/code/test/hd-lang/src/checker/context.ts:430:29)
    at FunctionChecker.check (file:///Users/hd/code/test/hd-lang/src/checker/context.ts:306:25)
    at SignatureInference.run (file:///Users/hd/code/test/hd-lang/src/checker/program-inference.ts:231:7)
```

The type text doubles per level (`tupleType` joining `elements` at `src/types.ts:93`); past depth 26 it exceeds the max string length. One function (`tupleParts`, 86%) dominates: the checker re-decomposes the exponentially growing tuple type per level.

## 2. tuple-destructure (slots; base 100, 2× = 200, 4× = 400)

| size | wall |
| --- | --- |
| 100 slots | 2273 / 2273 / 2264 ms, median **2273** |
| 200 slots (2×) | **8712** (3.8×) |
| 400 slots (4×) | **42069** (4.8×) |

Diagnostic: `type-mismatch: expected u32, found i32` at 3:19.

Top 10 (2331 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 235.8 | 10.1 | `resolveType` | `src/checker/context.ts:1237` |
| 201.0 | 8.6 | `tupleParts` | `src/types.ts:68` |
| 155.8 | 6.7 | `nominalGenericParts` | `src/types.ts:141` |
| 124.1 | 5.3 | `functionParts` | `src/types.ts:340` |
| 105.6 | 4.5 | `traitsByIndex` | `src/checker/associated-bindings.ts:25` |
| 98.7 | 4.2 | (program) | runtime root |
| 64.9 | 2.8 | `substituteGenericType` | `src/checker/shared.ts:362` |
| 63.9 | 2.7 | (garbage collector) | runtime |
| 55.4 | 2.4 | `inferTypesThroughBounds` | `src/checker/bound-inference.ts:25` |
| 51.1 | 2.2 | `ambiguousProjection` | `src/checker/associated-bindings.ts:165` |

No single hotspot: cost is spread over type resolving, tuple/nominal/function decomposition, and trait-association lookup per slot.

## 3. obligation-cascade (chain depth; base 2000, 2× = 4000, 4× = 8000)

| size | wall |
| --- | --- |
| depth 2000 | 3395 / 3439 / 3368 ms, median **3395** |
| depth 4000 (2×) | **12585** (3.7×) |
| depth 8000 (4×) | **51725** (4.1×) |

Also: depth 1000 → 1243 ms (brief: >45 s — fixed, see headline). Result is `ok` (accept) at all sizes.

Top 10 (3426 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 268.2 | 7.8 | filter closure in `instantiationsFor` | `src/checker/bound-inference.ts:127` |
| 254.6 | 7.4 | `children` | `src/checker/shared.ts:483` |
| 172.1 | 5.0 | `unifyTypes` | `src/checker/shared.ts:520` |
| 165.2 | 4.8 | `find` predicate in `registerImplementationPair` | `src/checker/program-implementations.ts:257` |
| 158.6 | 4.6 | `tupleType` | `src/types.ts:92` |
| 155.4 | 4.5 | `filter` predicate in `implementationsFor` | `src/checker/implementation-index.ts:34` |
| 150.7 | 4.4 | `candidatesFor` | `src/checker/bound-inference.ts:134` |
| 130.0 | 3.8 | RegExp typename match | runtime |
| 125.8 | 3.7 | `inferTypesThroughBounds` | `src/checker/bound-inference.ts:25` |
| 125.6 | 3.7 | `matchGenericTypePattern` | `src/checker/generic-patterns.ts:25` |

Still ~quadratic in this range, but the fixpoint loop is no longer the wall: cost is spread over instantiation filtering, unification, and impl-index scans.

## 4. mutual-bounds (gen scale; pairs = 5×scale/2; base 200 = 500 pairs)

| size | wall |
| --- | --- |
| scale 200 | 2240 / 2189 / 2174 ms, median **2189** |
| scale 400 (2×) | **8988** (4.1×) |
| scale 800 (4×) | **35240** (3.9×) |

Result `ok` at all sizes.

Top 10 (2291 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 596.8 | 26.1 | `inferTypesThroughBounds` | `src/checker/bound-inference.ts:25` |
| 195.5 | 8.5 | `inputsOf` | `src/checker/bound-inference.ts:170` |
| 79.4 | 3.5 | `genericTypeName` | `src/checker/shared.ts:439` |
| 67.3 | 2.9 | (program) | runtime root |
| 66.8 | 2.9 | `checkEntry` | `src/checker/calls.ts:874` |
| 64.2 | 2.8 | (garbage collector) | runtime |
| 58.9 | 2.6 | `children` | `src/checker/shared.ts:483` |
| 57.7 | 2.5 | RegExp `generic:` match | runtime |
| 54.6 | 2.4 | `visit` | `src/checker/shared.ts:658` |
| 46.6 | 2.0 | filter closure in `instantiationsFor` | `src/checker/bound-inference.ts:127` |

Bound inference is still the core cost (35% in `inferTypesThroughBounds` + `inputsOf`), but no longer cubic in the measured range.

## 5. add-many-types (gen scale; types = scale/2; base 60 = 30 types)

| size | wall |
| --- | --- |
| scale 60 | 3582 / 3531 / 3473 ms, median **3531** |
| scale 120 (2×) | **13565** (3.8×) |
| scale 240 (4×) | **60547** (4.5×) |

Diagnostic: `type-mismatch: 'i32' does not widen implicitly to 'i64'` at 1147:21.

Top 10 (3487 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 1598.5 | 45.8 | restore closure in `speculate`/`snapshot` | `src/checker/call-speculation.ts:58` |
| 1351.6 | 38.8 | `snapshot` closure in `speculate` | `src/checker/call-speculation.ts:27` |
| 56.0 | 1.6 | (garbage collector) | runtime |
| 27.0 | 0.8 | (program) | runtime root |
| 22.7 | 0.7 | `compileForInternalLoader` | runtime |
| 17.6 | 0.5 | `snapshotCheckerState` | `src/checker/checker-trial-state.ts:21` |
| 12.6 | 0.4 | restore closure, Map branch | `src/checker/call-speculation.ts:38` |
| 10.8 | 0.3 | `scan` | `src/lexer.ts:152` |
| 10.3 | 0.3 | `rewriteTypes` | `src/checker/type-declarations.ts:361` |
| 10.0 | 0.3 | `callCandidate` | `src/checker/expression-calls.ts:662` |

85% of self time is trial snapshot/restore in `call-speculation.ts`: each open-operand `+` speculates over candidates and copies the checker state.

## 6. list-nest (gen scale; depth = 2×scale; base 3000 = depth 6000)

| size | wall |
| --- | --- |
| depth 6000 | 5391 / 5365 / 5371 ms, median **5371** |
| depth 12000 (2×) | **23688** (4.4×; needs `--stack-size=24000`, crashes at 8000-stack) |
| depth 24000 (4×) | **96134** (4.1×; same stack note) |

Diagnostic: `type-mismatch: expected u16, found i32` at 3:22. Stack note: at the harness 8000-stack, depth 12000 crashes in `fillNode` (`src/checker/type-defaults.ts:225`, `RangeError: Maximum call stack size exceeded`, 358 ms). With node defaults (no `--stack-size`), depth 1000 already crashes in `fillNode` and depth 2000 in the parser (`Parser.advance`, `src/parser/base.ts:588` ← `Parser.matchText` `:560` ← `parsePrefix` `src/parser/expression.ts:511` ← `parseExpression` `:97`, repeating). So the harness flag is load-bearing for this case.

Top 10 (5429 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 2278.8 | 42.0 | `nominalGenericParts` | `src/types.ts:141` |
| 1784.3 | 32.9 | `checkAccessExpression` | `src/checker/expression-data.ts:635` |
| 847.5 | 15.6 | (program) | runtime root |
| 86.9 | 1.6 | (garbage collector) | runtime |
| 20.2 | 0.4 | `compileForInternalLoader` | runtime |
| 15.9 | 0.3 | `parsePrefix` | `src/parser/expression.ts:429` |
| 15.1 | 0.3 | `functionParts` | `src/types.ts:340` |
| 11.0 | 0.2 | `rewriteTypes` | `src/checker/type-declarations.ts:361` |
| 10.7 | 0.2 | `parseExpression` | `src/parser/expression.ts:77` |
| 10.5 | 0.2 | `checkLiteralExpression` | `src/checker/expression-literals.ts:207` |

75% sits in `nominalGenericParts` + `checkAccessExpression`: each of the 6000 `[0]` reads re-decomposes the `List[List[…]]` type. Clean quadratic.

## 7. closures-shared-var (gen scale; closures = 10×scale; base 600 = 6000)

| size | wall |
| --- | --- |
| 6000 closures | 3382 / 3215 / 3153 ms, median **3215** |
| 12000 (2×) | **13000** (4.0×) |
| 24000 (4×) | **61917** (4.8×) |

Diagnostic: `type-mismatch: expected u32, found i32` at 18003:22.

Top 10 (3171 ms total self):

| self ms | % | function | file:line |
| --- | --- | --- | --- |
| 2265.3 | 71.4 | `visibleCaptureSources` | `src/checker/context.ts:1305` |
| 139.6 | 4.4 | (garbage collector) | runtime |
| 59.0 | 1.9 | `checkClosureExpression` | `src/checker/checker.ts:323` |
| 38.6 | 1.2 | `check` | `src/checker/context.ts:277` |
| 30.8 | 1.0 | (program) | runtime root |
| 28.3 | 0.9 | `scan` | `src/lexer.ts:152` |
| 27.7 | 0.9 | `CheckerContext` constructor | `src/checker/context.ts:203` |
| 26.9 | 0.8 | `visit` | `src/checker/template-instances.ts:72` |
| 26.3 | 0.8 | `rewriteTypes` | `src/checker/type-declarations.ts:361` |
| 21.7 | 0.7 | `collectReadLocals` | `src/checker/context.ts:89` |

The single most concentrated hotspot of all seven cases: 71% in `visibleCaptureSources` — every closure re-scans for visible captures of the shared open variable. Growth here measures ~n² (4.0×/4.8× per 2×), steeper than the brief's ~n^1.6.

## Growth summary (measured ratios per 2× input)

| case | base → 2× | 2× → 4× |
| --- | --- | --- |
| nested-tuple | 5.4 s → crash | crash |
| tuple-destructure | 3.8× | 4.8× |
| obligation-cascade | 3.7× | 4.1× |
| mutual-bounds | 4.1× | 3.9× |
| add-many-types | 3.8× | 4.5× |
| list-nest | 4.4× | 4.1× |
| closures-shared-var | 4.0× | 4.8× |

Ready for Job 2 when you are.