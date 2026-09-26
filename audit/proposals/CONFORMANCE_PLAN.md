# Conformance Plan: Making the Suite Outlive the MVP

**Status:** ready to run. All 13 decisions in
[`DECISION_SHEET.md`](DECISION_SHEET.md) were made on 2026-09-25. Written 2026-09-25 from the audit
of commit `bd985d7`.

**Goal:** after this plan, `spec/` alone defines the conformance suite and
how to run it. The suite contains the audit's results as permanent cases, and
any implementation can be measured against it, selecting its supported
subset with a manifest (D9). No language feature is added.

**Out of scope:** fixing the MVP compiler. Cases the MVP fails are
deselected in its manifest, and the list of those failures becomes the
input to the MVP fix step (step 3 of the next steps).

## Ground Rules

- This plan edits `spec/` and `test/`. That is a new phase of work, separate
  from the read-only audit.
- Every work item ends with a command that proves it.
- `npm run check` stays green after every work item. New cases that the MVP
  fails are deselected in `test/portable/cases.tsv`, never weakened.
- Fixture moves use `git mv`, so history follows the file.
- Each work item is one commit.

## Work Items

| Item | Title                                          | Needs decisions            | Can start |
| ---- | ---------------------------------------------- | -------------------------- | --------- |
| W0   | Record the decisions                           | all of D1 to D13 (done)    | first     |
| W1   | Apply the fixture format to the spec           | D1, D2, D3, D6, D9, D10    | now       |
| W2   | Update the diagnostic inventory                | D7                         | now       |
| W3   | Make the runner follow the contract            | D3 to D6, D8, D9, D13      | after W1, W2 |
| W3a  | Self-test the `assert_equal` oracle            | D1                         | after W3  |
| W4   | Fix reference-parser bugs                      | none                       | now       |
| W5   | Reconcile the EBNF with layout                 | D11                        | now       |
| W6   | Promote fixtures from `test/fixtures`          | D1, D2, D3, D5, D6, D7, D10 | after W3a |
| W7   | Add audit-derived cases                        | D7                         | after W3, W4 |
| W8   | Move the fuzzer into `spec/tools/`             | D12                        | after W4  |
| W9   | Regenerate the MVP selection and failure list  | none                       | last      |

W0 is done. W1, W2, W4, and W5 can start now.

```text
W0 ──┬─ W1 ─┐
     ├─ W2 ─┼─ W3 ─┬─ W3a ─ W6 ─┐
     └─ W5  │      └─ W7 ───────┼─ W9
W4 ─────────┴─ W8 ──────────────┘
```

---

### W0. Record the decisions

- Mark each `Decision: _` in `DECISION_SHEET.md`.
- For each deferred item N1 to N6, add a question to
  `future-work/OPEN_ISSUES.md` with its evidence link.
- Add the language-design questions from `DECISION_LOG.md` sections B, C,
  and D to `OPEN_ISSUES.md` as questions (26 are listed in
  `audit/REPORT.md` section 8).

**Done when:** no `Decision: _` remains, and `spec/check.sh` passes. The
check bans unresolved markers only in numbered chapters, so
`OPEN_ISSUES.md` entries are fine.

### W1. Apply the fixture format to the spec

- Start from the proposed text in `audit/proposals/fixture-format.md`,
  between its `---8<---` markers.
- Replace each `[OPEN-n]` with the decided text, and delete the text a
  decision retires (for example `expect-result`, if D1 is A).
- Replace `spec/conformance/README.md` with the result, including:
  - the command contract (from `audit/fuzz/CONTRACT.md`, adjusted by D4 and
    D13);
  - the anchor rule (D3);
  - panic judging (D6);
  - subset selection by manifest (D9): a manifest lists the cases an
    implementation claims; unlisted cases are not run, and there is no
    skip result;
  - per-test instances (D10).
- Reduce `test/README.md` to implementation-test notes, pointing at the spec
  for the format.
- Extend `spec/check.sh` to reject any directive not defined in the new
  README.

**Done when:** every directive used under `spec/conformance/` is defined in
`spec/conformance/README.md`. Check this by rerunning
`audit/scripts/test-quality/inventory.ts` and confirming its directives
table has no "outside `spec/`" rows for conformance files. `spec/check.sh`
also passes.

### W2. Update the diagnostic inventory

- Add the D7 code set to the table in `spec/README.md`. Each code gets a
  one-line meaning. `unsupported-*` codes stay implementation-specific (D9).
- Resolve the `impure-parameter-default` sub-question from D7.
- Where the reference parser emits a code, it must now be in the inventory
  (`syntax-error` and the lexical codes).

**Done when:** `spec/check.sh` accepts every code used by
`spec/conformance/`. Rerunning the fuzz contract fuzzer
(`--fuzzer contract --cases 500`) shows the uninventoried share falling to
roughly the implementation-shaped codes D7 left out.

### W3. Make the runner follow the contract

Rewrite the conformance path of `test/run-portable.ts` as a neutral runner,
`spec/tools/run-conformance.ts`, that imports nothing from `src/`. It must:

- match paths by file identity and set no working directory (D4);
- fail reject cases with any extra error, while allowing warnings (D5);
- require the exact panic category, with no `runtime-error` (D6);
- treat `cases.tsv` phases as latest-allowed (D8);
- run only the cases in an optional selection manifest, and report pass and
  fail for those (D9);
- apply the 10 s timeout and exit rules (D13).

`test/run-portable.ts` keeps only the `test/fixtures` implementation path.
It calls the neutral runner for conformance cases.

**Done when:**

1. **The planted failures are all caught.** Rerun the 35 plants in
   `audit/probes/harness/` against the new runner. All 35 must be caught,
   versus 25 today; the 6 controls must pass.
2. **The stub run shows no coupling.** Run
   `audit/scripts/test-quality/stub-hd.ts` in both absolute and relative
   path modes, with a manifest selecting the parse cases. All selected
   cases must pass, with 0 coupling failures (versus 239 today with
   relative paths). Without a manifest, every non-parse case must fail
   cleanly with a located code.
3. **The MVP result is unchanged.** Every conformance case the MVP passes
   today either still passes, or fails for a reason the decisions intend.
   List each change.

### W3a. Self-test the `assert_equal` oracle

D1 makes `assert_equal` the oracle for every runtime result, so it must be
proven before W6 converts 140 fixtures onto it.

- For each value kind, add a pair: equal values pass (`runtime/valid`), and
  unequal values panic with `assertion-failed` (`runtime/panic`). Kinds:
  `i32`, `f64`, `bool`, `char`, `string`, tuple, `list[T]`, `map[K, V]`
  (including order-independence, if the spec states it), `T?`,
  `Result[T, E]`, a nominal type with an explicit `PartialEq`
  implementation, and a generic `T: PartialEq` value.
- Cross-check against `assert` on plain `bool` conditions that do not use
  `PartialEq`, such as comparing lengths or matching on variants.
- Pin the `PartialEq` edge values: NaN is unequal to itself, and -0 equals
  +0. Fixtures about bit-exact NaN or -0 must use `x != x`, identity, or
  bits, never `assert_equal`.
- Cover the harness side: a test block whose `assert_equal` fails must fail
  the case (already shown for `assert` in audit step 1.3).

**Done when:** every pair behaves as specified under the neutral runner, and
a planted no-op `assert_equal` (a stub implementation that always passes)
fails every panic half. The known MVP gap F-500 (primitives under generic
`PartialEq`) shows up here as listed MVP failures, not as weakened fixtures.

### W4. Fix reference-parser bugs

These need no decisions. Fix in `spec/reference-parser/`:

| Finding         | Bug                                                          |
| --------------- | ------------------------------------------------------------ |
| F-206 (F-301)   | `\u{...}` escapes are rejected; `\{` and `\ ` are over-accepted |
| F-300           | `fn() -> fn() -> T` is rejected without a `$` clause (Earley nullable completion) |
| F-358           | string literals inside `${...}` are rejected                 |
| F-359           | nested named arguments are flagged as `argument-order`       |
| F-360 (F-302)   | a spread after an explicit `$.with` binding is rejected      |
| F-303           | a stray `$` is accepted (the code comes from D7 when decided) |

Add one `parse/valid` or `parse/invalid` fixture per bug. The minimized
inputs are in `audit/fuzz/findings/F-30*-ref-*.hd` and
`audit/probes/blind-triage/refparser/`.

**Done when:**

- the new fixtures pass the reference parser in `spec/check.sh`;
- a parse-agreement fuzz rerun (`--fuzzer parse --seed r1 --cases 5000`)
  shows no reference-side signatures for these bugs;
- the blind fixtures AMB-01 to AMB-04 can be written in their original form.

### W5. Reconcile the EBNF with layout

- Apply D11 to `spec/02-grammar.md`.
- Add `parse/invalid` fixtures for the forms that are now excluded, taken
  from the smallest F-314 derivations in
  `audit/evidence/03-fuzz/grammar-check.txt`.

**Done when:** `grammar-check.ts --seed g1 --cases 4000` shows no
F-314-class rejections. It showed about half of the smallest 40 before.

### W6. Promote fixtures from `test/fixtures`

Input: `audit/evidence/01-test-quality/inventory.tsv`, which lists 327
language-behavior files, each with a promotion target.

1. **Move the 107 ready files now.** Use `git mv` into the target
   `spec/conformance/` directory and add a `cases.tsv` row with the target
   spec section.
2. **Convert the 126 files that depend on D1 or D2.** Rewrite
   `expect-result` as test blocks with `assert_equal`, and non-entry
   execution as test-block calls. Write a converter script and review its
   output by hand.
3. **Handle the 94 blocked files by reason:**
   - a non-spec code: re-mark with a D7 code, or leave in `test/`
     (including `unsupported-*` codes, per D9);
   - an undeclared requirement key (F-201): fix the fixture;
   - an MVP-only profile (N2): leave in `test/`;
   - host formatting: covered by step 2;
   - `runtime-error`: re-mark with a chapter-06 category, or hold for N6.
4. **Clean up while moving:**
   - split multi-marker files into one case per marker;
   - give each case a unique name (F-211);
   - fix the 6 extra-error cases (D5);
   - fix the 3 misplaced panic markers (F-204);
   - rewrite the 4 shared-instance tests (D10);
   - fix the weak oracles listed in F-210.
5. **Delete duplicates.** Drop the 5 identical cross-set duplicates in
   `audit/evidence/01-test-quality/near-duplicates.tsv`, keeping the
   conformance copy.

**Done when:**

- `spec/check.sh` passes;
- `test/fixtures` holds only the 15 implementation-detail files plus the
  deliberately held-back ones, each with a one-line reason in
  `test/README.md`;
- rerunning `oracle.ts` over `spec/conformance/` shows no persisting
  neutralized code and no extra errors.

### W7. Add audit-derived cases

Add only cases whose expected outcome is settled by the spec or by a D
decision. Anything that depends on an open question waits.

| Source                                   | Cases                                                                  |
| ---------------------------------------- | ---------------------------------------------------------------------- |
| `audit/fuzz/findings/`                   | the implementation-bug fixtures F-304 to F-316, where the spec settles the expectation (F-304: payload type mismatch; F-305, F-307: runnable `Result[void, E]` programs) |
| `audit/blind/fixtures/`                  | all 60, after triage: pass as written, except the 3 ambiguity-dependent ones (AMB-08, 15, 17) |
| `audit/probes/gaps/`                     | the 19 high-risk mechanism probes, including all four `f64` ordering operators and NaN |
| `audit/probes/gates/`                    | 3-frame cancellation order, LIFO within a frame, unreached `defer`     |
| `audit/probes/coordinator/`              | `f64-lt.hd`, `prim-partialeq.hd`                                       |
| `audit/probes/compiler/init-dispatch-miss.hd` | holds for B2: the expected outcome depends on the trait-dispatch question |

Also add fixtures for the most important of the 60 uncovered spec sections
(A12, `audit/evidence/01-test-quality/coverage.tsv`). Start with chapter 05
evaluation order and propagation, chapter 07 methods and receivers, and
chapter 10 packages.

**Done when:** each added case has a `cases.tsv` row and passes
`spec/check.sh`. The MVP failures among them are listed in W9.

### W8. Move the fuzzer into `spec/tools/`

- `git mv audit/fuzz spec/tools/fuzz`; update paths in its `README.md`.
- Add to `spec/check.sh`:
  - the `check-imports` gate, which fails on any `src/` import;
  - a seeded smoke run of about 200 cases per fuzzer, with the reference
    parser as the only oracle for the parse fuzzer. The smoke run does not
    judge an implementation, so `spec/check.sh` stays implementation-free.
- Implementation smoke runs go in `npm run check`, using
  `--compiler "node --experimental-strip-types bin/hd.js"`.
- Minimized fixtures from `audit/fuzz/findings/` moved in W7; the folder
  keeps only the tooling.

**Done when:** `spec/check.sh` runs the gate and the smoke run, and a
planted `src/` import fails it.

### W9. Regenerate the MVP selection and failure list

- Run the neutral runner over all of `spec/conformance/` with the MVP.
- Write `test/portable/cases.tsv` as the passing subset, so
  `npm run check` stays green.
- Write `test/portable/KNOWN_FAILURES.tsv`: each failing case with its audit
  finding ID (for example F-160, F-304, F-351, F-500, F-603). This is the
  input to step 3, fixing the MVP bugs that distort semantic answers.

**Done when:** `npm run check` passes, and every failing case maps to a
finding or a D/N item.

## Expected End State

| Measure                                              | Today             | After              |
| ---------------------------------------------------- | ----------------- | ------------------ |
| directives defined outside `spec/`                   | 8 of 19           | 0                  |
| planted runner failures caught                       | 25 of 35          | 35 of 35           |
| stub-runner coupling failures (relative paths)       | 239               | 0                  |
| language fixtures outside `spec/conformance/`        | 327               | held-back only, each with a reason |
| conformance markers using non-inventoried codes      | 69 of 140 (fixture set) | 0            |
| reference-parser bugs known                          | 6                 | 0                  |
| fuzzer location                                      | `audit/`          | `spec/tools/`, in `spec/check.sh` |
| MVP failures tracked                                 | scattered in findings | `KNOWN_FAILURES.tsv` |

## Effort Guide

W3a, W4, W5, W8, and W9 are small and mechanical. W1 and W2 are mostly writing,
once the decisions exist. W3 is a focused rewrite of one runner. W6 is the
largest item, because of the 126 conversions and the cleanup; write the
converter first. W7 depends on triage that the audit has mostly done
already.
