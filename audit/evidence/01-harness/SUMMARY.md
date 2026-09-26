# 1.3 to 1.5: Harness, Gap Probes, Special-Casing

Commit `bd985d7`, 2026-09-25. The audit worker could not write report
files, so the coordinator wrote this summary from the worker's report and the
evidence files below. The coordinator reproduced F-160 independently
(`audit/probes/coordinator/f64-lt.hd`: `1.0 < 2.0` crashes in `src/wasm.ts`).

## Evidence

| File                                               | Content                                              |
| -------------------------------------------------- | ---------------------------------------------------- |
| `audit/scripts/harness/run-portable-copy.ts`        | harness copy with only the five root constants changed |
| `harness-copy.diff`                                | `diff -u` against `test/run-portable.ts`             |
| `plants.tsv`, `plant-run.log`                      | 41 planted cases (35 plants, 6 controls) and verdicts |
| `existing-unexpected-diagnostics.tsv`              | real selected cases that also emit unrelated errors   |
| `../01-gap-probes/results.tsv`                     | 19 gap probes over 7 mechanisms                      |
| `../01-special-casing/*.log`                       | grep inventory, renamed variants, crash triggers      |

## 1.3 Harness sensitivity

Plan-table rows:

| Plan row                                          | Result                                                    |
| ------------------------------------------------- | --------------------------------------------------------- |
| right code, wrong line                            | caught                                                    |
| wrong code, right line                            | caught                                                    |
| extra unexpected error in a reject case           | **missed** (F-150)                                        |
| accept case that emits a warning                  | passes; decided: keep (3 selected accept cases emit correct `unused-local-binding` warnings) |
| panic case ending in generic `runtime-error`      | caught in the conformance suite, **missed** in the fixture suite (F-153) |
| `expect-result` with the wrong value              | caught                                                    |
| runtime case that never runs its test blocks      | caught, except scenario cases (F-154)                     |
| crash instead of a diagnostic                     | caught                                                    |

Across all 35 plants, 10 were missed. They cluster into F-150 to F-154:

- a `# diagnostic:` marker satisfied by a warning with the same code;
- panic codes matched anywhere in stdout or stderr, including program output;
- `# panic: runtime-error` accepting any crash.

Type and runtime phases really type-check and execute.

## 1.4 Gap probes

19 probes over the 7 high-risk mechanisms, with controls: 16 pass.

| Mechanism                       | Result                                                         |
| ------------------------------- | -------------------------------------------------------------- |
| permission and readonly checks  | pass                                                           |
| argument evaluation order       | pass                                                           |
| `defer` on cancellation         | pass                                                           |
| one-shot suspension state       | pass                                                           |
| row subtraction                 | pass                                                           |
| `f64` NaN ordering              | **fail**: every f64 `<`, `<=`, `>`, `>=` crashes emission (F-160) |
| iterator version check          | pass                                                           |

No existing fixture orders an `f64` at runtime, so the suite is green while
the operator is unusable.

## 1.5 Special-casing scan

- No fixture names or paths are special-cased in `src/`. Six renamed fixture
  variants behave identically.
- Name-bound runtime profiles live in `src/cli.ts`. `pending-gate` and
  `cleanup_ran` are documented in `spec/conformance/README.md`; the
  `ready-*` profiles are not. The documented `disposed-file` profile is not
  implemented, and its runtime case is unselected.
- 63 `throw new Error` sites; none is converted into a structured
  diagnostic. Six crash paths were triggered (F-161, F-162, and the F-160
  crash). There are no `TODO` markers.

## Findings

| ID    | Severity | Title                                                                  |
| ----- | -------- | ---------------------------------------------------------------------- |
| F-150 | minor    | reject cases pass even when unrelated errors are also reported         |
| F-151 | minor    | a `# diagnostic:` marker is satisfied by a same-code warning            |
| F-152 | minor    | panic codes match anywhere in output, including program output          |
| F-153 | major    | `# panic: runtime-error` accepts crashes; hides a raw trap for `2 ** -1` |
| F-154 | minor    | scenario cases never run their test blocks                             |
| F-155 | minor    | panics carry no source location, so marker lines are unchecked          |
| F-156 | note     | `expect-result` compares all of stdout                                  |
| F-160 | major    | f64 relational operators crash code generation                          |
| F-161 | minor    | unbounded recursion ends in a raw `RangeError`, not `stack-exhausted`   |
| F-162 | minor    | `hd run`/`hd test` crash with stack traces on entry-shape errors        |
| F-163 | major    | `xs == [1, 2]` is rejected with `type-mismatch`                         |

## Phase-7 questions

- Should the conformance format define that a reject case must produce only
  its marked diagnostic?
- Should `runtime-error` be removed as a panic expectation, so every
  conformance panic names a specified category?
- Should panic reports carry a source location that fixtures can check?
