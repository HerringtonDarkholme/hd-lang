# Q23: Mistake corpus and diagnostic location (`6699bd49`)

From `audit/hd-writing-log.md` (~200 rows) and Q21's retry report,
`compiler/bench/mistakes/` holds one small `.hd` file per logged
mistake kind (40 cases). Each file carries `# code:` (the diagnostic
code the mistake should produce) and `# line:` (the source line holding
the mistake) headers. Rerunnable: `node compiler/bench/mistakes/run.mjs`
runs `hd check --format json` on each and takes the first diagnostic.

- code match: **27/40 (67.5%)**
- line match: **29/40 (72.5%; goal >= 95%)** — goal missed
- both code and line: 25/40

## Worst offenders

**`hd check` does not check `tests:` blocks in single-file mode.**
`nosuchfn()` inside an `it` body, and even directly in test position,
both pass `hd check` silently (status 0). This explains three corpus
misses at once: `second-tests-block` (no `duplicate-tests-block`),
`test-trailer-ok` (no `invalid-test-statement`), and it means every
test-body mistake kind is invisible to the check command. The codes
`DuplicateTestsBlock` and `PrivateMain` are never constructed anywhere
outside the codes enum (dead codes). `hd test FILE.hd` does run test
bodies (Q21), so this is a check-only coverage gap, and the largest
single drag on both shares.

**Silent accepts of former mistakes (6 cases, no diagnostic at all):**

| Case | Old (prototype) behavior | New behavior |
| --- | --- | --- |
| contextual-some | `missing-contextual-enum-type` on `x == .Some(3)` | accepted; contextual variants now resolve through `==` |
| integer-literal-range | bare `9223372036854775807` rejected as i32 | accepted; the literal takes the expected `i64` |
| method-default | `syntax-error` on `fill: char = ' '` | accepted, per spec `fn.default.allowed` |
| data-colon-pass | `syntax-error: expected a data field name` | accepted — `pass` reads as an empty body; needs spec eyes (is an empty data type legal?) |
| private-main | silent exit 0 (nothing ran) | silent, but the new compiler RUNS a single-file non-`pub` main — the mistake evaporated |
| loop-keyword | `unknown-name 'loop'` | `hd check` silent (0 modules flagged); `hd run` says `unsupported: TrailingCallExpr` at the right line — check/run divergence |

**Wrong code, right line (5):**

| Case | Expected | Got | Note |
| --- | --- | --- | --- |
| list-plus | `unsatisfied-trait-bound` | `type-mismatch` | new message names `Add` for the operand; arguably as helpful |
| mixed-signedness | `mixed-signedness` | `type-mismatch` | the `MixedSignedness` code exists but is never constructed (dead code) |
| tuple-index-bound | `unknown-method` | `unknown-data-field` | new, more precise code (`no field '_2' on (i32, i32)`) |
| reverse-ctor | `syntax-error` | `trailing-block-position` | new, more precise code for `Name(k: v)` |
| cannot-infer-empty | `cannot-infer-type` | `mutable-receiver-required` (next line) | `[]` becomes `List[?]` and the later `push` fails first; the log's suggested annotation (`let pieces: mut List[string]`) is still what the user needs, but no message says so |

**Right code, nearby line (2):** `non-reassignable-binding` points at
the reassignment (line 6), not the `:=` binding (line 5); the joined
`.Ok(())` in `trailing-dot-ok` points at the expression start (line 6),
not the `.Ok` line (7). Both conventions are defensible — the report
names the statement that fails rather than the declaration that caused
it. Counting either convention, line share stays far from 95% because
of the tests-block gap above.

## Diagnostics per mistake (the 25 clean hits)

All 13 syntax-error cases hit code and line (parse errors place
precisely), as do `missing-let`, `missing-requirement` ($ Console),
`bang-call-outside-suspension`, `invalid-result-propagation`,
`mut-on-primitive`, `mutable-receiver-required`,
`readonly-argument-to-mutable-parameter`, `readonly-root`,
`discarded-must-use-value`, `argument-count`, `unknown-name`
(`assert` without import), `unknown-method` (leading-dot join),
`unsatisfied-trait-bound` (optional in interpolation), and
`integer-literal-range`'s neighbors. Parse-time location is good;
the misses are all semantic-coverage or dead-code issues, not
off-by-N column bugs.

## Suggested next steps (for the orchestrator, not this report)

1. Check `tests:` bodies in `hd check` single-file mode (covers 3
   misses plus every future test-body mistake).
2. Either construct `MixedSignedness` or delete it; same for
   `DuplicateTestsBlock` / `PrivateMain` (wire them or drop the codes).
3. Decide `data X:` + `pass` and the `loop:` check/run divergence.
4. `cannot-infer-type` for `[]` should fire at the binding before the
   later `push` fails, and name the `mut` form when the value is
   mutated (`let pieces: mut List[string]`).
