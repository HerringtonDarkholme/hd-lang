# Job 6 audit: new `src/` commit review (first pass)

> Reviewed 2026-10-04 at HEAD `c38d0d3f`, covering every `src/`-touching
> commit since session start (`d082975f..HEAD`). No repo files were changed
> during the review; all evidence comes from commands run against the
> checkout. Verdict: **no findings** — details per commit below.

## Scope

```
$ git log --oneline d082975f..HEAD -- src/
c38d0d3f Spec batch 2026-10-04b (task #282)
4636f546 fix: a literal argument waits for another argument to solve its type parameter (task #266)
4fba4be5 fix: bound inference revisits only parameters with fresh inputs
```

## `4636f546` — literal arguments wait for later solvers

- Cited rule `types.literal.open.decide.generic` exists:
  `spec/lang/04-type-system.md:385` ("an argument whose type parameter
  another argument solves, as in `biggest(i, items.len())`"). The new
  `src/checker/literal-arguments.ts` implements `types.generic.infer.join.literal`
  ("takes the type solved from the other arguments as its expected type, in
  any position", Inference From Several Arguments §6). Citation valid.
- Not a special-case patch: a general plan-ordering mechanism plus a
  duplicate-argument check shared across five call sites. Re-verified each
  refactored span reports the same code at the same argument span
  (`src/checker/calls.ts` diff).
- The waiting logic reads a live map: `substitutions` is created once as
  `new Map` at `src/checker/calls.ts:806` and mutated by `checkEntry`, so
  `substitutions.has(parameter)` inside `checkLiteralArgumentsLast` sees
  solvers checked earlier in plan order. No stale snapshot.
- No fixture or KNOWN_FAILURES edits (diff touches only `src/` + two test
  files), so the message's "no known-failure fixture changes status" holds.
- Tests, re-run here:
  `node --test --experimental-strip-types test/contextual-numeric-literals.test.ts`
  → 8 pass, 0 fail; `test/dogfood.test.ts` → 8 pass, 0 fail.
- Probes (scratch only): `fold(0, fn(acc: usize, n): acc + n)` (mixed
  annotated/unannotated closure, the untested combination) checks clean,
  and `fold(3000000000, ...)` with both mixed and fully-annotated closures
  checks clean — the early-flush path does not pin the literal to `i32`
  (no eager range check, no spurious conflict). The hypothesized
  mixed-closure ordering hazard does not reproduce; no item filed.
- `wc -l src/checker/calls.ts` → 1483: the "under the 1500-line lint
  limit" claim checks out.

## `4fba4be5` — dirty-flag bound inference

- No spec rule cited; the message claims an identical fixpoint with fewer
  visits. Read-set analysis supports it: `inferGenericType`
  (`src/checker/shared.ts:802-811`) reads seed entries only for
  formal-side (pattern) names, which are subterms of the bound's trait
  arguments and therefore members of `inputsOf`; the solvability gate reads
  `bound.parameter`, also an input. Newly solved non-inputs cannot change a
  skipped check's outcome. `boundsMentioning` uses the same predicate as
  the old per-round filter (`mentionedGenericParameters` mirrors
  `containsGenericParameter`; `src/checker/shared.ts:650-656`).
- No fixtures, spec, or KNOWN_FAILURES touched (diff is two `src/` files
  only). No new tests; correctness rests on the unchanged conformance
  suite plus the `obligation-cascade` perf shape, which Job 1 corroborates
  (cascade now far faster than the brief states at the fixed commit).

## `c38d0d3f` — spec batch (task #282)

- `git diff c38d0d3f~1 c38d0d3f -- 'src/*.ts' 'src/**/*.ts'` is empty: the
  only `src/` path touched is `src/KNOWN_ISSUES.md` (docs). No compiler
  code to review.
- The 9 added KNOWN_FAILURES rows are genuine: ran all 16 new fixtures
  through `spec/tools/run-conformance.ts --adapter test/hd-adapter.ts`
  → `conformance: 7 passed, 9 failed, 16 selected`; every new KF row fails
  exactly as its reason text says, and all 7 new portable cases (including
  the 2 invalid variance rejections) pass. Not edits-made-to-pass.
- New KF tags (`SHADOW-TPARAM`, `VARIANCE-MUT-SELF`, `ALIAS-MISSING`,
  `PRIVATE-STD`, `DERIVE-MISSING`, `AMBIGUOUS-TYPE`) are documented with
  migration rationale in the same commit's `test/MIGRATED.md` diff.

## Standing watch

Next Job 6 pass triggers on the next `src/`-touching commit on
`origin/main` after `c38d0d3f`.
