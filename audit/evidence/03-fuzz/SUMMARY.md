# 3.2 Fuzzing

Commit `bd985d7`, 2026-09-25. The worker could not write report files; the
coordinator wrote this file from the worker's final report. The coordinator
reproduced F-304 independently: `audit/fuzz/findings/F-304-impl-result-payload-unchecked.hd`
passes `hd check`, then `hd run` fails Wasm validation (`struct.new operand
0 must have proper type`).

## The Tool

`audit/fuzz/` imports only Node built-ins, its own files, and `spec/`.

- `check-imports.ts` passes; a planted `src/` import makes it exit 1.
- `npx oxlint audit/fuzz` passes. Evidence: `lint-and-imports.txt`.
- Files: `fuzz.ts`, `mutate.ts`, `generate.ts` (parses the chapter-02 EBNF
  fences itself), `oracles/`, `adapters/wasm.ts`, `minimize.ts`,
  `grammar-check.ts`, `forward.sh`, `CONTRACT.md`, `README.md`, `findings/`.
- `README.md` documents how to point it at another implementation.

## Commands

- Round 1: `node --experimental-strip-types audit/fuzz/fuzz.ts --seed r1
  --cases 5000 --jobs 10 --adapter wasm --minimize --min-tests 300 --out
  audit/evidence/03-fuzz/round1`
- Round 2: the same command with `--seed r2`.
- Cross-implementation: `--seed x1 --cases 1000 --fuzzer cross --compiler
  "node --experimental-strip-types bin/hd.js" --compiler audit/fuzz/forward.sh`
- Negative control: the same, with `--compiler true` as the second compiler.
- Generator check: `grammar-check.ts --seed g1 --cases 4000`.
- Replay: `fuzz.ts --replay <fixture>` for each filed fixture.

Logs: `round1.log`, `round2.log`, `cross.log`,
`cross-negative-control.log`, `grammar-check.txt`, `replay.txt`, and
`roundN/<fuzzer>/` (`signatures.tsv`, `examples/*.min.hd`, `outcomes.tsv`,
`uninventoried-codes.tsv`, `run.json`).

## Counts

Every fuzzer ran its full 5,000 cases; none reached the 20-signature cap.
Load average was 7 to 70.

| Run              | Fuzzer   | Cases | Signatures | Wall time |
| ---------------- | -------- | ----- | ---------- | --------- |
| r1               | parse    | 5000  | 19         | 302 s     |
| r1               | contract | 5000  | 13         | 820 s     |
| r1               | phase    | 5000  | 0          | 24 s      |
| r1               | wasm     | 5000  | 3          | 240 s     |
| r2               | parse    | 5000  | 18         | 369 s     |
| r2               | contract | 5000  | 11         | 1081 s    |
| r2               | phase    | 5000  | 0          | 26 s      |
| r2               | wasm     | 5000  | 3          | 561 s     |
| x1               | cross    | 1000  | **0**      | 616 s     |
| negative control | cross    | 20    | 20         | 7 s       |

- Total wall time, including minimization: r1 27m43s, r2 39m55s, cross
  10m17s.
- The cross run was reduced to 1,000 cases, because each case doubles the
  command-line calls at load 60.
- In the r1 contract run, `check` accepted 1,192 of the 5,000 inputs that the
  reference parser accepts.
- Phase consistency: 0 violations in 10,000 inputs.
- 13 panic categories appeared, all from chapter 06.
- `wasm-tools` is not installed, so only `wasm-opt` validated modules.

## Uninventoried Codes

58.5% of `check` rejections (2,229 of 5,000 inputs) carry a code missing from
the `spec/README.md` inventory. There are 48 distinct codes. The most common:

| Code                  | Count |
| --------------------- | ----- |
| `expected-expression` | 1036  |
| `unknown-name`        | 749   |
| `expected-token`      | 554   |
| `unknown-type`        | 528   |
| `type-mismatch`       | 311   |

## Triage

Counts are r1/r2.

**Parse agreement:**

| Signature                                                        | Counts  | Class                  |
| ---------------------------------------------------------------- | ------- | ---------------------- |
| `expected-expression`                                            | 431/395 | F-315 (local declarations) |
| `decorator-not-top-level`                                        | 105/98  | F-312                  |
| `expected-token`                                                 | 362/325 | F-315                  |
| reference rejects, implementation accepts: `syntax-error`        | 14/17   | F-310                  |
| reference rejects, implementation accepts: `trailing-block-position` | 4/4 | F-310                  |
| `invalid-escape`                                                 | 7/2     | F-301                  |
| `expected-newline`                                               | 15/16   | F-309, F-315           |
| `missing-impl-body`                                              | 20/11   | F-315                  |
| `invalid-string-interpolation`                                   | 6/4     | F-303                  |
| `unexpected-indentation`                                         | 1/0     | F-311                  |
| `argument-order`                                                 | 1/0     | F-302                  |
| `doc-comment-without-target`                                     | 0/2     | F-316                  |
| `unsupported-*` (5 signatures)                                   |         | discarded: correct handling of unsupported features |
| `reserved-name`, `nonfinal-positional-spread`, `duplicate-mutable-permission`, `invalid-assignment-target` | | discarded; parse-or-check phase is a spec question |
| `nonfinal-vararg`                                                |         | spec question          |

**Contract:**

| Signature                                   | Counts             | Class |
| ------------------------------------------- | ------------------ | ----- |
| uninventoried code (check, run, test)       | 2229/2227          | F-313 |
| no `main` (run 810/821; test 672/679)       |                    | F-306 |
| unparseable WAT                             | 12/15              | F-305 |
| "main has no runnable export"               | 8/10               | F-307 |
| `TypeError`                                 | 1/0                | F-308 |
| `struct.new` operand type                   | 1/1                | F-304 |

**Wasm validity:** F-305 and F-304 again. Two signatures were discarded:

- "unexpected end of input": a fuzzer race, since fixed;
- 6 build timeouts in r2: CPU contention; the build takes 1.9 s when rerun
  alone.

**Round 2** repeated 16 of the 19 parse classes and added 2. Every r2
contract and Wasm class was already known. The common classes are exhausted;
the rare ones were only sampled.

**Generator check:** 768 of 4,000 EBNF derivations are rejected by the
reference parser. Among the smallest 40, about half are F-314 (a comma inside
a same-line suite, or a nested binding ending in a suite), and about 15 are
F-300. One form was not filed: `where fn() -> T: Bound` is rejected because
the reference lexer treats `fn` there as a suite header.

## Findings

All are confirmed by replay; fixtures are in `audit/fuzz/findings/`.

| ID    | Severity       | Title                                                                              | Duplicate of |
| ----- | -------------- | ---------------------------------------------------------------------------------- | ------------ |
| F-300 | major (ref)    | reference parser rejects `fn() -> fn() -> T` without a `$` clause (Earley nullable-completion bug) |  |
| F-301 | major (ref)    | reference parser rejects `\u{...}` escapes                                         | F-206        |
| F-302 | major (ref)    | argument order is enforced on context entries, rejecting the chapter-11 example    | F-360        |
| F-303 | minor (ref)    | a stray `$` is accepted, and no inventory code exists for it                       | part of F-206 |
| F-304 | major          | `check` does not check `Ok`/`Err` payload types; the program crashes in Wasm validation |          |
| F-305 | major          | `_ := f()?` on `Result[void, E]` emits unparseable WAT                             | extends F-559 |
| F-306 | minor          | `run`/`test` crash with a stack trace when there is no entry                       | F-162        |
| F-307 | major          | a `main` returning `Result[void, E]` passes `check` but cannot run                 |              |
| F-308 | minor          | a non-`pub` `main` with a non-host requirement passes `check`, then `run` throws a `TypeError` |  |
| F-309 | major          | same-line `if c: a else: b` is rejected                                            | F-252        |
| F-310 | minor          | a line starting with `:` becomes a trailing block of the previous statement        |              |
| F-311 | minor          | a nested suite inside brackets is accepted without deeper indentation              |              |
| F-312 | minor          | every decorator is reported as `decorator-not-top-level`                           | F-405        |
| F-313 | major (spec)   | the code inventory omits 48 codes hd emits and 8 reference-parser codes            | extends F-205 |
| F-314 | note (spec)    | the grammar derives same-line-suite forms that layout cannot produce               |              |
| F-315 | note           | grammar-valid forms are rejected with generic codes: `A()`, `If[T]`, bodyless `impl P`, local declarations | F-254 (partly) |
| F-316 | minor          | `pub fn` in an inherent impl is rejected with `doc-comment-without-target`         |              |

## Fixes to the Fuzzer Between Rounds

- The Wasm adapter now builds in a fresh directory per call. Earlier smoke
  runs had written `case.wasm`, and a probe `x.wasm`, to the repository
  root. The worker deleted both. This explains the stray root file noticed
  during the audit.
- The label for outputs with no code was improved. It affects grouping
  only, never verdicts.

## Phase-7 Questions

1. Should `CONTRACT.md` become normative: exit statuses, the located-line
   format, panic lines, and "never exit 1 without a code"?
2. Which diagnostics may `parse` report, as opposed to `check`?
3. How is a non-final `...` in a function type diagnosed?
4. Should parse, name, and type codes be inventoried? Should there be an
   `unsupported-*` code family?
5. Is a non-`pub` `main` an entry point? What should `run`/`test` do on a
   library file?
6. Which code applies to a stray `$`?
7. Which of the F-314 forms are valid?
8. Should the fuzzer move to `spec/tools/`, with a seeded smoke run in
   `spec/check.sh`?

## Not Done

- The cross-implementation run used 1,000 cases instead of 5,000.
- There is no stdout oracle for `run`.
- `wasm-tools` was not used.
- The `src/` mechanisms behind F-304 and F-305 are `UNVERIFIED`.
