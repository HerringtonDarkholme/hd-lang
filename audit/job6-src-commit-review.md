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

## Standing watch (first pass)

Next Job 6 pass triggers on the next `src/`-touching commit on
`origin/main` after `c38d0d3f`.

---

# Job 6 audit, second pass

> Reviewed 2026-10-04 at HEAD `7c4f4c13`, covering every
> `src/`-touching commit in `c38d0d3f..HEAD`. No repo files were changed
> during the review; all evidence comes from commands run against the
> checkout. Verdict: **no findings** — details per commit below.
> `9f67cd00`, `3edd4e36`, `40a1f4bd` are self-authored (Jobs A–C of this
> session); their landing evidence is restated, not re-derived.

## Scope

```
$ git log --oneline c38d0d3f..HEAD -- src/
aaa26b22 Spec batch 2026-10-04c (task #288)
40a1f4bd Job C: drop export from 37 file-local symbols (audit/job5)
3edd4e36 Job B: genericTypeName returns early (perf F2)
9f67cd00 Job A: rename error codes to spec spelling
4856cb9e Spec: hd doc and module docs (task #263)
0a04a489 Conformance: CLI-tier cases and the pending-first-poll scenario (task #283)
```

(`06886d18`, `494f29aa`, `f86395db`, `7c4f4c13` touch no `src/` path
and are out of scope.)

## `0a04a489` — CLI-tier cases (task #283)

- The only `src/` path touched is `src/KNOWN_ISSUES.md` (counts + 6 new
  CLI tags). No compiler code to review.
- Counts verify: `test/portable/cases.tsv` 2,237 rows +
  `KNOWN_FAILURES.tsv` 175 rows = 2,412 claimed; tier join against
  `spec/conformance/cases.tsv` gives selected 1966 lang / 269 std /
  2 CLI and KF 145 / 10 / 20, exactly as written. Tag rows sum to 175;
  `CLI-ENTRY` counts 12 rows, matching the 11→12 bump.
- All 33 added KF rows point at fixtures added in the same commit; only
  indexes and READMEs are modified. The 13 `pending-first-poll` rows
  are runtime/valid cases for a scenario the CLI has no hook for, which
  is what the new PENDING-FIRST-POLL tag says.

## `4856cb9e` — hd doc and module docs (task #263)

- Again `src/KNOWN_ISSUES.md` only: 2,238 + 184 = 2,422; tiers 1967 /
  269 / 2 and 146 / 10 / 28 verify by the same join; tag rows sum to
  184. New tags CLI-DOC (5) and MODULE-DOC (1) match added `cli/doc-*`
  and `parse/valid/module-documentation.hd` rows.
- MODULE-DOC's reason (lexer reports `doc-comment-without-target` for
  a file's first `##` block) is consistent with the tour sweep in Job D,
  where no such warning appeared only because no snippet opens with a
  bare `##` block.

## `9f67cd00` — Job A renames (self-authored)

- Restated landing evidence: `grep -rn
  'mismatched-delimiter\|duplicate-data-field' src test
  --include='*.ts'` is empty on this tree (re-run here, no hits); the
  two live probes print the spec codes; size guards identical
  (16,350 B / 23 fns, 3,143 B / 6 fns); `pnpm run check` 726 pass.
- The fixture rename (`duplicate-data-field.hd` →
  `duplicate-field.hd`) touched the do-not-touch list under explicit
  owner direction; manifests were the only other files changed.

## `3edd4e36` — Job B early return (self-authored)

- One line in `genericTypeName`; behavior probed identical on 8 input
  shapes before landing. `perf:check` 11/11 both sides; the ~4 ms
  mutual-bounds median drop was reported as noise, not a win.
- Still present on this tree (verified in `origin/main` at landing
  time; this tree contains that commit).

## `40a1f4bd` — Job C de-exports (self-authored)

- 37/37 rows re-verified in-session before landing (own-file-only use;
  cross-file lookalikes are separate declarations, property accesses,
  or prose); a 7-symbol sample re-grepped here across all of `src/`
  shows zero remaining exports. The committed diff is 37 export
  removals plus one formatter-mandated signature join; tiny-program
  WAT byte-identical; `pnpm run check` 726 pass.

## `aaa26b22` — spec batch 2026-10-04c (task #288)

- `src/` touch is `src/KNOWN_ISSUES.md` only: 2,268 + 189 = 2,457;
  tiers 1991 / 275 / 2 and 149 / 10 / 30 verify by the same join; tag
  rows sum to 189.
- Two fixture *contents* are edited (not just added):
  `bound-inference-several-impls.hd` (`cannot-infer-type` →
  `ambiguous-type`) and `bounds-disagree.hd` (rule renumber), with
  matching `cases.tsv` expectation changes. This is spec-driven, not
  edits-made-to-pass: the batch renames the rule to
  `bound.no-default.ambiguous`, and the compiler still emits
  `cannot-infer-type` — verified live here — so both fixtures land as
  new BOUND-AMBIGUOUS KF rows (186–187), whose reason text matches the
  observed output exactly. Same for RESERVE-PKG (188): `check` exits 0
  on `module-named-pkg.hd`, as the row says, and `reserved-module-name`
  is unknown to `src/`.

## Standing watch (second pass)

Next Job 6 pass triggers on the next `src/`-touching commit on
`origin/main` after `7c4f4c13`.
