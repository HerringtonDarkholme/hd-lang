# 1.6 Test Case Quality and Portability

Commit `bd985d7`, 2026-09-25. The worker could not write report files; the
coordinator wrote this file from the worker's final report. The proposal
`audit/proposals/fixture-format.md` was written by the worker: it contains 15
`[OPEN-n]` questions and decides nothing.

## Headline

- **Misplaced language tests:** 327 of 342 `test/fixtures` files, and 33 of
  173 TypeScript tests, test language behavior outside `spec/conformance/`.
- **Stub runner:** 62 of 551 cases pass, all of them parse cases. One parse
  case fails because of a reference-parser bug. The other 488 fail cleanly.
- **Paths:** the real compiler, printing relative paths, fails 239 of 551
  cases.
- **Undefined codes:** 69 of 140 fixture markers use codes missing from
  `spec/README.md`.
- **Marker isolation:** when the marked line is neutralized, the code
  disappears in 123 of 127 selected conformance cases and 137 of 140 fixtures.
- **Coverage gaps:** 60 substantive spec sections have no fixture in either
  set.

## Commands

All scripts are in `audit/scripts/test-quality/`, run from the repository
root, and pass `npx oxlint`.

| Command                                                                                      | Output                                  |
| -------------------------------------------------------------------------------------------- | --------------------------------------- |
| `inventory.ts`                                                                               | `inventory.tsv`, `selfcontain.tsv`      |
| `HD_TEST_COMMAND="node --experimental-strip-types audit/scripts/test-quality/stub-hd.ts" node --experimental-strip-types test/run-portable.ts` | `stub-runner.log` |
| the same, with `--suite conformance --phase parse`                                           | 31 passed                               |
| the same, with `STUB_MODE=relative-hd`                                                       | `stub-relative-hd.log`                  |
| `oracle.ts --jobs 6` (copies in `audit/probes/oracle/`)                                      | `oracle-runs.tsv`                       |
| `oracle-summary.ts`                                                                          | `oracle.tsv`                            |
| `coverage.ts`                                                                                | `coverage.tsv`, `near-duplicates.tsv`   |
| `bash audit/scripts/test-quality/probes.sh`                                                  | `probes.log`                            |

## Inventory

Output: `inventory.tsv`.

- **`test/fixtures`:** 327 language behavior, 15 implementation detail. All
  342 files were read; the 15 were chosen by hand. The spec section comes
  from a path-rule table written after reading.
- **TypeScript tests:** 140 implementation detail (71 of which also assert
  language behavior) and 33 pure language behavior. All 33 duplicate a
  portable fixture. Classes come from assertion regexes (heuristic); the 33
  were checked by hand.
- **Promotion readiness of the 327:**
  - 107 can move now;
  - 126 need `expect-result` or `expect: test` defined in `spec/` first;
  - 94 are blocked: a non-spec code, an undeclared key, an MVP-only profile,
    host formatting, or `runtime-error`.

## Directives

Output: `directives.tsv`, 20 rows.

Defined only outside `spec/`:

- `# test:` (342 uses), `# expect-result:` (169), and
  `# expect: accept|parse|test` (62). `accept` and `parse` are documented
  nowhere.
- The one-marker rule, the phase-to-command mapping, the `--entry`,
  `--profile`, `--scenario`, and `--pending-function` options, exit statuses,
  the located-line format, and the `runtime-error` wildcard.

Hooks buildable from their description: `cancellation-cleanup` and
`pending-function`.

Hooks that are not buildable from their description:

- `disposed-file`: its trait surface is unstated, and the MVP exits 2 on it;
- `competing-drivers` and `reentrant-poll`: undefined in source terms; they
  panic even when `main!` is `pass`;
- `fixture-package-role`: the packages exist nowhere, and no runner reads it;
- four `ready-*` profiles exist only in `src/cli.ts`.

## Self-Containment and Determinism

Output: `selfcontain.tsv`. 231 fixtures carry at least one flag:

| Flag                                        | Count      |
| ------------------------------------------- | ---------- |
| non-`pub` `main` returning a value          | 125        |
| non-spec diagnostic code                    | 69 markers |
| `run --entry` on a non-entry function       | 44         |
| undeclared `Clock`, `Logger`, or `Backup`   | 29         |
| requirement row with fabricated providers   | 13         |
| MVP-only profile                            | 4          |
| `f64` or `char` host formatting             | 3          |
| `runtime-error`                             | 1          |

Map iteration (5 fixtures) is fine, because insertion order is specified. No
fixture depends on message text, timing, or file layout.

## Stub Runner

- **62 pass:** 31 conformance parse cases and 31 `expect: parse` fixtures.
- **1 fails:** `frontend/14-...-6`, because the reference parser rejects
  `\u{1F600}`.
- **488 fail cleanly** with the located `unsupported-feature`.
- There are no false passes, and no case inspects WAT or HIR.

Hidden coupling:

- the located-diagnostic check needs the verbatim argv path; with relative
  paths, 239 of 551 fail (F-207);
- parse-level fixtures run through `check`; 5 match the reference parser
  exactly yet fail (F-209);
- failures are reported by name, and 36 names cover 98 reports (F-211);
- `expect-result` depends on command-line stdout formatting (F-202);
- the runner forces its working directory to the repository root and needs
  `test/portable/cases.tsv`.

## Oracle Strength

Output: `oracle.tsv`. 304 marked cases, 628 fixtures in total.

**(a) Neutralize the marked line:**

| Verdict                                  | Selected conformance | Fixtures           |
| ---------------------------------------- | -------------------- | ------------------ |
| code disappears, copy accepted           | 75                   | 95                 |
| code disappears, another error remains   | 48                   | 42                 |
| code persists                            | 4                    | 2 (+1 legitimate)  |

The code persists for two reasons:

- repeated constructs: discarded-result, key-collision, duplicate-bound;
- incidental markers: `compiler/46` and two scenario cases.

The two tab fixtures were rerun by hand with spaces.

**(b) Extra errors:** 5 selected conformance cases and 1 fixture.

**(c) Runtime observation:** all 170 runtime accept cases observe something.
Weak cases:

- 3 return a constant literal;
- 4 `expect: accept` fixtures claim runtime effects but only type-check;
- `suspension/35` cannot see a lost -0.0 sign.

**(d) Accept twins (heuristic):** 121 marked cases (97 of them conformance)
lack an accept fixture with token similarity of at least 0.5; 92 have no
accept case in the same group.

**(e) One behavior per fixture:**

- no fixture has multiple markers;
- 118 share names;
- 10 have more than two result entries;
- 6 chain three or more facts with "and".

## Redundancy and Gaps

Output: `coverage.tsv`.

- 60 substantive sections have no primary fixture. Examples:
  - chapter 05 evaluation order and propagation;
  - chapter 07 methods and receivers;
  - most of chapters 13 and 14;
  - chapter 10 packages.
- 17 sections are covered only by `test/fixtures`.
- 53 near-duplicate pairs; 14 cross the two sets, and 5 of those are
  identical.

## Findings

| ID    | Severity | Title                                                                          |
| ----- | -------- | ------------------------------------------------------------------------------ |
| F-200 | major    | fixture directives and the runner command contract are defined outside `spec/` |
| F-201 | major    | 29 fixtures use undeclared requirement keys, which the compiler wrongly accepts |
| F-202 | major    | `expect-result` compares JavaScript formatting of Wasm return values, not a specified rendering |
| F-204 | minor    | three panic fixtures mark a line that does not cause the panic                 |
| F-205 | major    | half the `test/fixtures` reject markers use codes `spec/README.md` does not define |
| F-206 | minor    | the reference parser disagrees with chapter 01 on four lexical rules           |
| F-207 | minor    | the located-diagnostic check needs the runner's absolute path echoed verbatim  |
| F-208 | minor    | four reject cases repeat their code on other lines                             |
| F-209 | note     | `test/fixtures` has no phase, so lexer and parser rejections run through `check` |
| F-210 | minor    | 19 fixtures claim behavior their oracle cannot observe                         |
| F-211 | note     | 118 fixtures share `# test:` names, so failures cannot be traced to a file     |
| F-212 | minor    | two harness hooks cannot be built from their description; one is implemented nowhere |

F-203 was withdrawn as a duplicate of F-153. F-150, F-155, and F-156 overlap
F-208, F-204, and F-202, but cover different aspects.

## Phase-7 Questions

1. Should `spec/conformance/README.md` hold the whole fixture format? See
   `audit/proposals/fixture-format.md`.
2. Should expected values use `Display` rendering, or be replaced by
   `assert_equal`?
3. May result entries be any function, and who supplies their requirement
   rows?
4. Should there be codes for type mismatch, unknown names and types, and
   syntax errors?
5. Which panic category does a negative exponent raise?
6. How are the scenario hooks defined in source terms, and where are the
   package sources for `fixture-package-role`?
7. Should panic markers be judged by line?
8. May reject cases report extra errors?
9. Do test blocks run after `main` in the same instance?
10. Should there be a stdout expectation?

## Not Done

- Neutralization is mechanical; structural lines are flagged, not rewritten.
- The twin check and the TypeScript-test classification are heuristic.
- The 37 unselected conformance cases were not analyzed further; they
  mostly show unimplemented features (see phase 2).
- No fixtures and no `spec/` files were edited.
