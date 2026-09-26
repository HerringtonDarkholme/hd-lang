# Decision Sheet: Conformance Blockers

**Status:** decided by the owner on 2026-09-25 (all 13). Originally Written 2026-09-25 from the audit of
commit `bd985d7`. Each entry gives the question, the options, the evidence,
a recommendation, and what the answer unblocks in
[`CONFORMANCE_PLAN.md`](CONFORMANCE_PLAN.md).

The recommendations are the auditor's reading of the evidence. Nothing here
is decided until the owner marks it. Mark each entry by replacing
`Decision: _` with the chosen option.

Scope: only questions that block making the conformance suite portable
(steps 1 and 2 of the audit's next steps). Language-design questions (B, C,
and D in the decision log) are out of scope here; they go to
`OPEN_ISSUES.md`.

References: `[OPEN-n]` markers are in
[`fixture-format.md`](fixture-format.md), and A, B, C, D rows are in
[`../evidence/07-spec-feedback/DECISION_LOG.md`](../evidence/07-spec-feedback/DECISION_LOG.md).

## Summary

| #   | Question                                        | Recommendation                                              | Unblocks       |
| --- | ----------------------------------------------- | ----------------------------------------------------------- | -------------- |
| D1  | How are runtime results checked?                | retire `expect-result`; use `assert_equal` in test blocks   | W1, W6         |
| D2  | Which functions may a fixture execute?          | only spec entry points and test blocks                      | W1, W6         |
| D3  | Where must a diagnostic be located?             | one general anchor rule in `spec/README.md`                 | W1, W3, W6     |
| D4  | How is the reported path matched?               | same file, not the same bytes; no working-directory rule    | W3             |
| D5  | May a reject case report other errors?          | no other errors; warnings allowed                           | W3, W6         |
| D6  | How are panics judged?                          | exact category; no `runtime-error`; line not judged         | W1, W3, W6     |
| D7  | Which diagnostic codes are in the inventory?    | add a small general-purpose set; fixtures mark only inventoried codes | W2, W6, W7 |
| D8  | Must a rejection happen in the listed phase?    | the phase is the latest point; earlier is allowed           | W3             |
| D9  | Is there a portable "unsupported" result?       | yes: `unsupported-feature` (decided: **no**, manifests only) | W1, W2, W3     |
| D10 | How do test blocks execute?                     | fresh instance per test block; `main` does not run first    | W1, W6         |
| D11 | Which same-line-suite forms are valid?          | only forms the layout rules produce; tighten the EBNF       | W5             |
| D12 | Where does the fuzzer live?                     | `spec/tools/fuzz/`, with a seeded smoke run in `spec/check.sh` | W8          |
| D13 | Timeout and exit rules for runners              | adopt the fuzz contract: 10 s timeout, exit 0/1 only, never exit 1 without a located code | W3 |

Deferred items, which do not block: the affected fixtures stay out of
`spec/conformance/` until they are decided.

| #  | Question                                                        | Fixtures held back |
| -- | --------------------------------------------------------------- | ------------------ |
| N1 | `disposed-file` profile surface ([OPEN-8])                      | 1                  |
| N2 | MVP `ready-*` profiles ([OPEN-9]); recommendation: keep them as implementation tests | the fixtures using them |
| N3 | competing drivers and re-entrant poll in source terms ([OPEN-10], A7) | 2            |
| N4 | synthetic packages for `fixture-package-role` ([OPEN-11])       | 2                  |
| N5 | stdout expectations ([OPEN-15])                                 | console fixtures   |
| N6 | panic category for a negative exponent (B8)                     | 1                  |

---

## D1. How are runtime results checked?

**Consequence of choosing A:** every runtime result now rests on
`assert_equal`, so the suite needs an oracle self-test layer first (work item
W3a in the plan). Each value kind gets a pass fixture and an
`assertion-failed` panic fixture, so a no-op `assert_equal` cannot pass.
`assert_equal` is cross-checked against `assert` on plain `bool`
conditions. NaN and -0 are tested through `x != x`, identity, or bits,
because `PartialEq` treats NaN as unequal to itself and -0 as equal to +0.


`[OPEN-1]`, `[OPEN-3]`, F-202, F-156.

Today `# expect-result: main = 42` compares the host's JavaScript formatting
of the Wasm return value. An `f64` `3.0` prints `3`, and a `char` prints its
scalar number. 169 directives in 140 fixtures depend on it, and program
output can satisfy the check.

| Option | Meaning | Cost |
| ------ | ------- | ---- |
| A | Retire `expect-result`. Results are checked with `assert_equal` inside named test blocks, which the language already specifies. | 140 fixtures are converted mechanically: `# expect-result: f = V` becomes a test block asserting `f()` equals `V`. |
| B | Keep it, and specify the rendering as the value's `Display` output on a marked `result: VALUE` line. | Every runner must implement `Display` rendering at the host boundary. Values without `Display` cannot be checked. |

**Recommendation: A.** It leaves nothing to specify at the host boundary,
works for any value with `PartialEq`, and removes the formatting coupling
entirely. The runner only needs "test blocks pass".

Decision: A (retire `expect-result`; use `assert_equal` in test blocks), 2026-09-25.

## D2. Which functions may a fixture execute?

`[OPEN-2]`, F-201.

125 fixtures run a non-`pub` `main` returning `i32` through `run --entry`,
and 13 carry a requirement row that no profile supplies.

| Option | Meaning |
| ------ | ------- |
| A | Only entry points as defined in chapter 10, plus named test blocks. |
| B | Any zero-parameter function named by the fixture. |

**Recommendation: A.** With D1-A, fixtures call other functions from test
blocks, so no runner option is needed to select an entry. It also removes the
F-201 pattern of requirement rows that nothing supplies.

Decision: A (entry points and test blocks only), 2026-09-25.

## D3. Where must a diagnostic be located?

A3, `[OPEN-5]` (second half), F-102.

Marker lines are normative, but the spec has no location rule. Every checked
line today equals wherever this compiler reports.

| Option | Meaning |
| ------ | ------- |
| A | One general rule in `spec/README.md`: a diagnostic is reported on the line where the smallest construct that violates the rule begins. Fixture authors place the marker there. |
| B | A location rule per diagnostic code. |
| C | Runners accept any line within the violating construct's span. This needs span markers in fixtures. |

**Recommendation: A.** One sentence covers nearly every fixture. When the
audit neutralized marked lines, the code disappeared in 260 of 267 cases. B
is precise but large, and C needs new fixture syntax.

Decision: A (one general anchor rule), 2026-09-25.

## D4. How is the reported path matched?

`[OPEN-5]` (first half), `[OPEN-12]`, F-207.

Today the runner requires the argv path echoed byte for byte, and forces the
working directory to the repository root. A compiler that prints relative
paths fails 239 of 551 cases.

| Option | Meaning |
| ------ | ------- |
| A | The reported `PATH` must name the same file. It may be relative to the working directory, or absolute. No working-directory requirement. |
| B | Keep the byte-for-byte match. |

**Recommendation: A.** The byte-for-byte match is pure coupling to this
compiler.

Decision: A (same file; no working-directory rule), 2026-09-25.

## D5. May a reject case report other errors?

A4, `[OPEN-6]`, F-150, F-208.

Today a reject case passes even when unrelated errors appear. The audit
found 5 selected conformance cases and 1 fixture that do this.

| Option | Meaning |
| ------ | ------- |
| A | The marked code must appear on its line, and no other error may appear. Warnings are allowed. |
| B | Other errors are allowed. |

**Recommendation: A.** With B, a checker that rejects too much still passes.
The 6 offending fixtures get fixed.

Decision: A (no other errors; warnings allowed), 2026-09-25.

## D6. How are panics judged?

A5, A6, `[OPEN-4]`, `[OPEN-14]`, F-153, F-155, F-204.

`# panic: runtime-error` accepts any crash, and it hid a raw trap. No
implementation reports a panic location, and chapter 06 requires one only
"when one is available".

| Option | Meaning |
| ------ | ------- |
| A | The panic category must match a chapter-06 category exactly. `runtime-error` is removed. The panic line is documentation and is not judged. |
| B | As A, but the panic line is judged too. |

**Recommendation: A.** B makes locations mandatory, which chapter 06 does
not. The marker still documents the intended line. The negative-exponent
fixture waits for N6.

Decision: A (exact category; no `runtime-error`; line not judged), 2026-09-25.

## D7. Which diagnostic codes are in the inventory?

A8, F-205, F-313.

hd emits 48 codes that are missing from `spec/README.md`. The reference
parser uses 8 more. 69 of 140 fixture markers use them, and so do 58.5% of
fuzzed `check` rejections.

| Option | Meaning |
| ------ | ------- |
| A | Add every code any implementation emits. |
| B | Add a small general-purpose set that every checker needs. Fixtures may mark only inventoried codes. Finer codes stay implementation-specific, and fixtures using them stay in `test/`. |
| C | Keep the inventory closed; rewrite or drop fixtures that use other codes. |

**Recommendation: B.** A freezes this compiler's internal distinctions into
the language contract. C drops real coverage of name resolution, arity, and
type errors, which every checker has.

The proposed additions are drawn from the observed codes. The owner edits
the list.

- **Lexical and syntax** (from the reference parser): `syntax-error`,
  `invalid-token`, `unexpected-indentation`, `invalid-dedent`,
  `unclosed-delimiter`, `unmatched-delimiter`, `unterminated-string`.
- **Names:** `unknown-name`, `unknown-type`, `unknown-trait`,
  `unknown-method`, `unknown-data-field`, `unknown-variant`.
- **Duplicates:** `duplicate-binding`, `duplicate-type`, `duplicate-variant`.
  `duplicate-field` already exists; consider whether `duplicate-data-field`
  is the same thing.
- **Types and calls:** `type-mismatch`, `argument-count`, `not-callable`.
- **Control flow:** `break-outside-loop`.

Codes deliberately left out, as too implementation-shaped:
`expected-expression`, `expected-token`, `expected-newline` (use
`syntax-error`); `unsupported-*` (see D9); `generic-type-mismatch`,
`match-arm-type`, `if-branch-type`, `pattern-type-mismatch` (use
`type-mismatch`).

Separate small question: is `impure-parameter-default` a new code, or the
existing `impure-data-default` generalized? (A8, B6.)

Decision: B (small general-purpose set; fixtures mark only inventoried codes), 2026-09-25.

## D8. Must a rejection happen in the listed phase?

A13.

Some rejections could reasonably happen in `parse` or in `check`:
`reserved-name`, `nonfinal-positional-spread`,
`duplicate-mutable-permission`, `invalid-assignment-target`.

| Option | Meaning |
| ------ | ------- |
| A | The phase in `cases.tsv` is the latest point at which the rejection must appear. A `type`-phase reject may already be reported by `parse`; a `parse`-phase reject must be reported by `parse`. |
| B | The phase is exact. |

**Recommendation: A.** It is portable, and it does not force a front-end
architecture.

Decision: A (listed phase is the latest allowed), 2026-09-25.

## D9. Is there a portable "unsupported" result?

A9, F-250.

Today a deferred feature falls into generic parse errors: only 7 of 51
deferred cases get an `unsupported-*` code. Runners cannot tell a deferral
from a wrong rejection, and implementations must maintain selection
manifests.

| Option | Meaning |
| ------ | ------- |
| A | The spec defines `unsupported-feature`, located at the construct. A conformance run reports pass, fail, and skip. A case that reports `unsupported-feature` is skipped, never passed, and a skip on an `accept` case is not a failure. |
| B | No special result; implementations select a subset with manifests, as `test/portable/cases.tsv` does today. |

**Recommendation: A**, keeping manifests optional. It makes partial
implementations honest and measurable, and it is exactly MVP goal 5.

Decision: B (no special result; implementations keep selection manifests), 2026-09-25. The recommendation was not taken.

## D10. How do test blocks execute?

`[OPEN-7]`, B5, F-403.

Chapter 02 says each test gets its own instance. The MVP runs `main` and
all test blocks in one shared instance, and 4 fixtures (`suspension/32`,
`33`, `35`, `36`) read state that `main` wrote.

| Option | Meaning |
| ------ | ------- |
| A | Keep the spec: a fresh instance per test block, after module initialization, and `main` does not run. |
| B | Change the spec to match the MVP. |

**Recommendation: A.** Isolated tests are the spec's existing rule and the
portable choice. The 4 fixtures get rewritten.

Decision: A (fresh instance per test block; `main` does not run), 2026-09-25.

## D11. Which same-line-suite forms are valid?

A14, F-314.

The chapter-02 EBNF derives forms the layout rules cannot produce, such as
a comma inside a same-line suite, or a nested binding that ends in a suite.
768 of 4,000 generated derivations are rejected by the reference parser.

| Option | Meaning |
| ------ | ------- |
| A | Layout wins: tighten the EBNF so it derives only forms layout can produce. |
| B | The grammar wins: extend layout to allow these forms. |

**Recommendation: A.** It changes no accepted program, and it makes the
EBNF usable for generation.

Decision: A (layout wins; tighten the EBNF), 2026-09-25.

## D12. Where does the fuzzer live?

A15.

| Option | Meaning |
| ------ | ------- |
| A | Move `audit/fuzz/` to `spec/tools/fuzz/`. Add a seeded smoke run (for example 200 cases per fuzzer, about 1 minute) to `spec/check.sh`, plus the `check-imports` gate. |
| B | Leave it in `audit/` as a manual tool. |

**Recommendation: A.** It already imports only Node built-ins and `spec/`,
and its value comes from running against every implementation.

Decision: A (`spec/tools/fuzz/` with a smoke run in `spec/check.sh`), 2026-09-25.

## D13. Timeout and exit rules for runners

`[OPEN-13]`.

**Recommendation:** adopt the fuzz contract. A 10 s timeout per command;
exit status 0 or 1 only; exit 1 always carries at least one located code; a
signal or timeout fails the case.

Decision: adopt the fuzz contract, 2026-09-25.
