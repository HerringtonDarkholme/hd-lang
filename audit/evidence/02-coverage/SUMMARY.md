# Phase 2: Coverage Against the Specification

Commit `bd985d7`, 2026-09-25. The worker could not write report files; the
coordinator wrote this file from the worker's final report.

## Headline

- **Corpus:** 286 conformance cases, 209 selected. All 209 pass under
  `audit/scripts/coverage/runner.ts`, which copies the logic of
  `test/run-portable.ts`. Its run over the selected manifest reproduces the
  portable result.
- **Unselected (77):** 51 deferred, 25 in scope and failing, 1 passing
  (`typing/valid/traits.hd`).
- **MVP goal 5 (structured rejection of deferred features)** is not met. Of
  the 51 deferred cases:
  - 6 get an `unsupported-*` code first;
  - 1 gets one, but not first;
  - 34 get generic codes (`expected-expression` 21, `expected-token` 9,
    `unknown-trait` 2, `unknown-type` 1, `unknown-name` 1);
  - 10 get `decorator-not-top-level` on decorators that are at top level.
- **Claim ledger:** 150 claims; 141 verified, 9 contradicted, 0 without
  evidence.
- **Probes:** 103 run. 93 pass, 7 fail as contradictions, and 3 negative
  controls (k01, k13, k73) fail as intended.
- **Spec examples:** of 141 accept or mixed examples, 85 meet their
  expectation and compile to Wasm, 50 have an entry, and 82 run without a
  panic.

## Commands

All scripts are in `audit/scripts/coverage/` and pass oxlint.

- **2.1:** `node --experimental-strip-types audit/scripts/coverage/runner.ts
  conformance audit/evidence/02-coverage/unselected-manifest.tsv
  audit/evidence/02-coverage/unselected.tsv`, output
  `unselected-classified.tsv`. Phase commands match the portable runner:
  parse runs `hd parse`, type runs `hd check`, and runtime runs `hd check`
  then `hd test` with the fixture's profile, scenario, and pending-function
  directives.
- **Reference parser:** `refparse.ts` accepts all 5 failing `parse/valid`
  cases.
- **S0:** `ast-snapshot.ts` run twice, then `cmp`.
- **S1:** `hd build --wat` on the 35 selected runtime cases, output
  `s1-wasm-gc.tsv`.
- **S3:** `runner.ts conformance audit/probes/gates/manifest.tsv
  audit/evidence/02-coverage/gate-probes.tsv`.
- **S5:** `s5-replay.ts audit/evidence/02-coverage/s5-replay.tsv`.
- **2.3:** `runner.ts fixtures test/fixtures` (342 of 342 pass);
  `runner.ts fixtures audit/probes/claims`; `host-token.ts` (Node API);
  `ledger.ts`. CLI checks are in `02-claims/cli.log`, reproductions in
  `02-claims/repros.log` and `f64-relational-wat.txt`.
- **2.4:** `examples-pipeline.ts audit/evidence/02-coverage/examples`.

## 2.1 In-Scope Failures

| Finding | Cause                                   | Cases |
| ------- | --------------------------------------- | ----- |
| F-253   | sized numeric types                     | 8     |
| F-252   | parser gaps                             | 4     |
| F-254   | `type` and local declarations           | 4     |
| F-255   | prelude names                           | 3     |
| F-256   | row-kinded arguments                    | 2     |
| F-251   | unsound accept                          | 1     |
| F-257   | `mut fn`                                | 1     |
| F-258   | wrong code                              | 1     |
| F-259   | missing profile (exit 2 with usage text) | 1    |

## Per-Chapter Coverage

| Chapter   | Fixtures | Selected | Selected passing | Unselected passing | Deferred | In-scope failing |
| --------- | -------: | -------: | ---------------: | -----------------: | -------: | ---------------: |
| 01        | 11       | 10       | 10               | 0                  | 0        | 1                |
| 02        | 17       | 12       | 12               | 0                  | 1        | 4                |
| 03        | 8        | 7        | 7                | 0                  | 0        | 1                |
| 04        | 46       | 30       | 30               | 0                  | 6        | 10               |
| 05        | 30       | 28       | 28               | 0                  | 0        | 2                |
| 06        | 32       | 32       | 32               | 0                  | 0        | 0                |
| 07        | 18       | 16       | 16               | 0                  | 0        | 2                |
| 08        | 18       | 18       | 18               | 0                  | 0        | 0                |
| 09        | 25       | 16       | 16               | 1                  | 7        | 1                |
| 10        | 17       | 14       | 14               | 0                  | 0        | 3                |
| 11        | 26       | 25       | 25               | 0                  | 1        | 0                |
| 12        | 6        | 0        | 0                | 0                  | 6        | 0                |
| 13        | 3        | 0        | 0                | 0                  | 2        | 1                |
| 14        | 29       | 1        | 1                | 0                  | 28       | 0                |
| **Total** | 286      | 209      | 209              | 1                  | 51       | 25               |

The chapter-13 failure is `variant-result-owner.hd`, which fails on its
unnamed payload `Value(T)`.

## 2.2 Slice Gates

| Slice | Verdict     | Evidence                                                                          |
| ----- | ----------- | --------------------------------------------------------------------------------- |
| S0    | met         | 31 of 31 selected parse cases pass; AST hashes identical across two processes; the core AST matches the checked-in snapshot. But 4 core `parse/valid` cases the reference parser accepts still fail (F-252). |
| S1    | met         | 35 of 35 runtime cases pass and all emit GC structs. The curated set misses f64 relational comparison (F-160). |
| S2    | met         | missing-row, subtraction, absent-warning, and key-collision cases pass; all 10 chapter-11 examples run; `explain-requirements` prints transitive paths. |
| S3    | met         | new probes in `audit/probes/gates/`: 3-frame cancellation logs `"imo"` (inner, middle, outer); LIFO within frames gives `"badce"`; unreached defers do not run; nested provider capture passes; the negative control expecting `"omi"` fails as intended. |
| S4    | met         | 16 of 16 chapter-09 cases and 35 of 35 collection, generic, and trait fixtures pass. Caveat: F-263. |
| S5    | partly met  | Works: round trip; rejection of a changed suspending function or `main`; survival of inserted functions and unexecuted changes; rejection of missing or extra events, a changed configuration, and changed or tampered host arguments; a tampered host value replays without calling the host. Fails: a changed executed non-suspending helper replays silently and returns 44 (F-401); a comment edit is rejected (F-264). |

## 2.3 Claim Ledger

Output: `../02-claims/ledger.tsv`.

- The 9 contradicted claims are C034, C040, C127, C130, C131, C138, C140,
  C147, and C150.
- Of these, C040, C131, C140, C147, and C150 test spec rules the claims
  depend on, rather than the literal claim sentences.
- Some verified rows rest on `expect: accept` fixtures, which only
  type-check.
- The two evaluation-order fixtures turned out to be weak (F-266).
- The `bool`/`char` part of C076, and C078, were checked through the Node
  API.

## 2.4 Specification Examples

| Chapter   | Examples | Expectation holds | Compiles to Wasm | Has entry | Runs cleanly |
| --------- | -------: | ----------------: | ---------------: | --------: | -----------: |
| 01        | 5        | 4                 | 4                | 4         | 4            |
| 03        | 12       | 10                | 10               | 6         | 10           |
| 04        | 14       | 6                 | 6                | 1         | 6            |
| 05        | 13       | 12                | 12               | 5         | 12           |
| 06        | 7        | 7                 | 7                | 3         | 7            |
| 07        | 20       | 15                | 15               | 12        | 12           |
| 08        | 14       | 10                | 10               | 9         | 10           |
| 09        | 13       | 10                | 10               | 0         | 10           |
| 10        | 8        | 1                 | 1                | 1         | 1            |
| 11        | 10       | 10                | 10               | 9         | 10           |
| 12        | 3        | 0                 | 0                | 0         | 0            |
| 13        | 5        | 0                 | 0                | 0         | 0            |
| 14        | 17       | 0                 | 0                | 0         | 0            |
| **Total** | 141      | 85                | 85               | 50        | 82           |

The 3 chapter-07 examples that do not run cleanly map to
`explicit-generic-method.hd`, which panics by design (F-268).

## Findings

| ID    | Severity | Title                                                                  | Duplicates   |
| ----- | -------- | ---------------------------------------------------------------------- | ------------ |
| F-250 | major    | deferred features get generic or false diagnostics                     |              |
| F-251 | major    | a plain closure may mutate a captured list, and it runs                | F-351        |
| F-252 | major    | the parser rejects single-line `if`/`else`, `use self.`/`super.`, and unnamed positional enum payloads |  |
| F-253 | minor    | sized numeric types are unimplemented and not listed as deferred       |              |
| F-254 | minor    | `type` declarations and local declarations fail to parse               |              |
| F-255 | minor    | prelude `Any`, `Eq`, `Hash`, and `Hasher` are unknown                  |              |
| F-256 | minor    | `Job[$()]` fails to parse                                              |              |
| F-257 | minor    | `mut fn` closures are missing; captured assignment reports `unknown-name` | F-352, F-357 |
| F-258 | minor    | `no-common-type` is reported instead of `no-least-common-type`         |              |
| F-259 | minor    | the `disposed-file` profile is missing                                 |              |
| F-260 | note     | `traits.hd` passes but is unselected                                   |              |
| F-261 | major    | `is` rejects `mut` operands                                            | F-353        |
| F-262 | blocker (worker's rating) | any f64 `<`, `<=`, `>`, `>=` crashes code generation   | F-160        |
| F-263 | major    | primitives fail `PartialEq`, `PartialOrd`, and `Display` bounds        | F-350, F-500 |
| F-264 | minor    | a comment edit breaks replay                                           | F-611        |
| F-265 | minor    | replay and CLI errors surface as uncaught stack traces                 | F-162        |
| F-266 | minor    | the evaluation-order fixtures cannot observe order                     | F-210        |
| F-267 | minor    | a later-parameter default reports `impure-parameter-default`           | F-356        |
| F-268 | note     | `explicit-generic-method.hd` panics when run                           | F-405 (per worker) |

Each duplicate file carries a `Duplicates:` line. The final report
de-duplicates these.

## Phase-7 Questions

1. Should the spec define a portable "unsupported feature" category?
2. Are sized numeric types, `type` declarations, and local declarations in
   the MVP, or deferred?
3. Which code applies to a default that names a later parameter? Should
   `impure-parameter-default` join the inventory?
4. Should replay identity use a normalized form, and cover every executed
   function?
5. When does `no-least-common-type` apply, as opposed to `no-common-type`?
6. Should `examples.tsv` mark examples that are type-only?
7. Is mixing positional varargs with a spread (`total(1, rest...)`)
   rejected, and with which code? The MVP reports `duplicate-argument`.
8. Should implementation-only codes (`type-mismatch`, `missing-display`,
   `argument-count`, and others) be promoted to the inventory?

## Not Done

- True concurrent host calls (C078) could not be tested, because `all!` and
  `race!` are unsupported.
- Documentation-comment AST retention (C002) is covered only by a TypeScript
  test, so it is `UNVERIFIED` from the command line.
- The cause given for F-264 comes from reading source and is `UNVERIFIED`.
- The `examples/core.hd` edit and the root `case.wasm` did not come from this
  worker; see the coordinator's note in `audit/REPORT.md`.
