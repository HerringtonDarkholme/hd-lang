# 1.1 Specification Edit Review and 1.2 Fixture Marker Review

Commit `bd985d7`, base `4cc1312`, 2026-09-25. The worker could not write
report files; the coordinator wrote this file from the worker's final
report. The coordinator confirmed the chapter diff size independently.

## Headline

- 7 specification hunks, all in `1e55fc9`, the commit that added `src/`:
  - 4 clarifications;
  - 2 new rules;
  - 1 editorial;
  - 0 relaxed, 0 tightened.
- 179 fixtures in the window (22 added, 157 changed); 0 weakened.
- Marker code matches `cases.tsv` for 179 of 179 fixtures, and matches the
  old header code for 157 of 157.
- For all 109 fixtures the suite runs, the marker line equals the line the
  compiler reports.
- No rule was bent to legalize code the base spec rejected.

## MVP Window

- The first commit touching `src/` is `1e55fc9` (2026-09-25 08:34, 16 added
  files). The base is its parent, `4cc1312`, a spec-consistency pass
  (`AUDIT_CHANGES.md`) with no implementation references.
- The window is `1e55fc9`, `58262b7`, `78af8aa`, `bd985d7`.
- Several rules tested by new runtime fixtures first appear in `4cc1312`,
  before `src/` existed (`git log -S`):
  - float display;
  - map insertion order;
  - `nil` ordering first;
  - the closed panic list.

  The implementation did not invent them.
- Correction to the audit baseline: see
  `audit/evidence/00-baseline/CORRECTIONS.md`.

## 1.1 Results

The diff touches 4 chapter files (+25/-10). Patch: `chapter-diff.patch`.
Table: `hunks.tsv`.

| Class          | Count | Hunks                                                   |
| -------------- | ----- | ------------------------------------------------------- |
| clarification  | 4     | H1a, H1b, H4b, H5                                       |
| new rule       | 2     | H2 (provider values escape), H3 (`all!`/`race!` intrinsics) |
| editorial      | 1     | H4a                                                     |
| rule relaxed   | 0     |                                                         |
| rule tightened | 0     |                                                         |

Hunks the implementation depends on, confirmed by probes:

- **H2 (chapter 11).** The compiler accepts a provider that escapes through a
  return value, a data field, and a module list, then is used with an empty
  row (`h2-provider-escape-field-return.hd`, `h2-provider-escape-global.hd`).
  Base design documents already described this as current behavior. The same
  commit deleted the open issue that recommended the opposite (F-100).
- **H1a/H5 (chapters 07 and 14).** The rule itself is unchanged: a
  function-value call in a default is still rejected
  (`h1-purity-function-value-default.hd`, 8:30).

Hunks it does not depend on:

- **H3/H4b.** The compiler rejects `all!` and `race!` with
  `unsupported-task-combinator`, a code missing from the spec inventory
  (`h3-*.hd`).
- **H1b** ("does not claim referential transparency"). The compiler is
  stricter than the spec and rejects top-level reads in defaults (F-101, 5
  probes).

## 1.2 Results

179 `.hd` files; 21 rows added to `cases.tsv`, 0 existing rows changed.
Table: `../01-fixtures/markers.tsv`; raw data in `markers-raw.tsv` and
`impl-lines.tsv`.

| Check                                                      | Result  |
| ---------------------------------------------------------- | ------- |
| marker code equals `cases.tsv`                             | 179/179 |
| inline code equals base header code                        | 157/157 |
| real body changes                                          | 2, both fixes to spec-invalid fixtures |
| tested fixtures with the marker on the reported line       | 109/109 |
| untested fixtures (compiler lacks the code; line unchecked) | 37     |
| panic markers whose line nothing checks                    | 18 (F-155) |
| new fixtures whose cited section holds the rule (by reading) | 22/22 |

- No fixture was weakened: no reject became accept, and no code became more
  generic.
- The two body edits fixed fixtures that were invalid at the base (see the
  `base-*.hd` probes):
  - `requirements-and-suspension.hd` drove a readonly `Suspend` binding;
  - `cancellation-runs-defer.hd` leaked a private trait through `pub main`.
- One citation is off: `partial-ordering-dispatch.hd` cites
  `09#comparison-traits`, but the rule it tests is in
  `05#unary-and-binary-operators`.

## Findings

| ID    | Severity | Title                                                                                  |
| ----- | -------- | -------------------------------------------------------------------------------------- |
| F-100 | minor    | the first implementation commit closed three open design issues without fixtures        |
| F-101 | minor    | parameter and data-field defaults reject any read of a top-level binding               |
| F-102 | minor    | marker lines are normative, but the spec has no location rule; every tested line equals this compiler's span |

F-100 covers three issues:

- provider escape was closed against its own recommendation;
- the recommended `pure` qualifier was dropped;
- `all!`/`race!` became intrinsics, which the compiler still rejects.

Related and not refiled: F-155 (panic lines are unchecked).

## Phase-7 Questions

- **H2** (codifies current compiler behavior): does the owner confirm the
  reworded-claim option over second-class providers? Should "remains usable
  after its provider scope ends" be qualified while NonEscapable providers
  remain an open design question?
- **H3** (a real limit, not yet exercised): sealed `Suspend` blocks
  user-written combinators. Should the spec register a code for "combinator
  signature unresolved"?
- **H1a/H5:** was dropping the `pure` qualifier intended, or should it
  return to `OPEN_ISSUES.md`?
- **H1b:** should defaults be allowed to read reassignable top-level
  bindings? Should the spec name a code for impure parameter defaults? The
  registry has none; the compiler invents `impure-parameter-default`.
- **Markers:** should the spec define diagnostic locations, or should
  runners accept any line within the construct's span?

## Commands

- `git log --reverse -- src/`; `git show --numstat` per commit;
  `git diff -U4 4cc1312 bd985d7 -- 'spec/[0-9][0-9]-*.md'`;
  `git show 4cc1312:future-work/OPEN_ISSUES.md`; `git log -S`.
- `markers.ts`, `impl-lines.ts`, and
  `awk -F'\t' -f join.awk impl-lines.tsv markers-raw.tsv`.
- `hd check` and `hd test` on `audit/probes/spec-edits/*.hd`, logged in
  `probe-runs.log`; `npx oxlint`.

## Not Done

- Lines for the 37 untested fixtures cannot be checked by execution.
- The 157 relocated markers were reviewed by reading. With no location rule
  in the spec, their lines cannot be re-derived from it.
- The TypeScript reference-parser port was not compared against the deleted
  Python parser.
