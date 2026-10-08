# Q25: `hd check --tests` and the mistake corpus (`b4283960`)

Q23 found plain `hd check FILE` silent on mistakes inside `tests:`
blocks and asked whether `hd check --tests` covers them. Answer: there
is no `--tests` flag. `hd check --tests FILE` exits 101 with
`` `hd check` has no option `--tests` ``, in single-file and
package mode alike; plain `hd check` stays silent (status 0) on both
test-code mistake kinds, which is correct per `cli.check.default`.
Rerunnable: `node compiler/bench/mistakes/run.mjs` now passes `--tests`
for cases with a `tests:` block (2 of 40) and records the mode per row.

- code match: **27/40 (67.5%)**
- line match: **29/40 (72.5%; goal >= 95%)** — goal still missed
- both code and line: 25/40

Unchanged shares from Q23 (`mistakes-6699bd49.md`, deleted in this
commit; git history keeps it): the two test-code cases moved from
"silent under plain check" to "flag rejected", still misses.

## What `--tests` does today: nothing, everywhere

| Probe (new compiler, this commit) | Result |
| --- | --- |
| `hd check --format json second-tests-block.hd` | status 0, summary only — silent, correct per `cli.check.default` |
| `hd check --format json test-trailer-ok.hd` | status 0, summary only — silent, correct per `cli.check.default` |
| `hd check --tests --format json second-tests-block.hd` | exit 101, `` `hd check` has no option `--tests` `` |
| package `hd check --tests --format json` (duplicate block in `src/main.hd`) | exit 101, same usage error |
| package plain `hd check --format json` on the same package | status 0 — silent on the duplicate block too |

## Compiler gap lines

- **Compiler gap: `hd check` has no `--tests` option** (spec
  `cli.check.tests`, file and package modes, plus `cli.check.tests.doc`
  for doc tests). Test code is unchecked by `hd check` in every mode,
  so `duplicate-tests-block` and `invalid-test-statement` can never
  surface there. This is the whole 2-case `--tests` miss column and
  every future test-body mistake kind.
- **Compiler gap: `duplicate-tests-block` is never reported.**
  `Code::DuplicateTestsBlock` exists in the generated codes enum but is
  constructed nowhere — dead code. No second-`tests:`-block detection
  exists in any check path.
- **Half-wired: `invalid-test-statement` is constructed but
  unreachable from check** (`hd_check/src/tests.rs` reports it for
  non-registration statements in a `tests:` block, yet `hd check`
  skips tests blocks in all modes, so only a tests-checking entry
  point could reach it).

## The 15 misses (mode-aware)

| Case | Mode | Expected | Got |
| --- | --- | --- | --- |
| second-tests-block | --tests | duplicate-tests-block | flag rejected, no diagnostic |
| test-trailer-ok | --tests | invalid-test-statement | flag rejected, no diagnostic |
| contextual-some | check | missing-contextual-enum-type | no diagnostic (now resolves through `==`) |
| integer-literal-range | check | integer-literal-range | no diagnostic (takes expected `i64`) |
| method-default | check | syntax-error | no diagnostic (per `fn.default.allowed`) |
| data-colon-pass | check | syntax-error | no diagnostic (`pass` reads as empty body) |
| private-main | check | private-main | no diagnostic (single-file non-`pub` main now runs) |
| loop-keyword | check | unknown-name | no diagnostic under check; `hd run` reports `unsupported` (check/run divergence) |
| list-plus | check | unsatisfied-trait-bound | `type-mismatch` (names `Add`; arguably as helpful) |
| mixed-signedness | check | mixed-signedness | `type-mismatch` (`MixedSignedness` still never constructed — dead code) |
| tuple-index-bound | check | unknown-method | `unknown-data-field` (more precise) |
| reverse-ctor | check | syntax-error | `trailing-block-position` (more precise) |
| cannot-infer-empty | check | cannot-infer-type | `mutable-receiver-required` one line later |
| non-reassignable | check | non-reassignable-binding | right code, points at the reassignment not the binding |
| trailing-dot-ok | check | unknown-method | right code, points at the expression start not `.Ok` |

The non-`--tests` rows match Q23's taxonomy (silent accepts of
spec-blessed behavior, dead codes, more-precise codes, nearby lines);
see that report's history for the per-case notes. The line-share goal
(>= 95%) stays missed almost entirely because of coverage gaps
(`--tests`, dead codes), not off-by-N locations.

## Suggested next steps (for the orchestrator, not this report)

1. Add `hd check --tests` (file + package + doc tests per
   `cli.check.tests` / `cli.check.tests.doc`); wire
   `DuplicateTestsBlock` or drop the code.
2. Either construct `MixedSignedness` or delete it (carried from Q23).
3. Decide `data X:` + `pass` and the `loop:` check/run divergence
   (carried from Q23).
