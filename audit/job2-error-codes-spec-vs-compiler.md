# Job 2 audit: error codes, spec vs compiler

> Posted in chat 2026-10-04 during the read-only audit session; logged here verbatim by owner request. HEAD 4636f546. Body below is the posted report, unedited.

Job 2 done. No repo files changed. HEAD `4636f546`, stable through this job.

## Method (all from commands I ran)

- Spec side: backticked kebab-case tokens in `spec/README.md` (214), `spec/lang` + `spec/cli` + `spec/std` prose (250), `# diagnostic:` fixture markers (193 unique), `cases.tsv` `reject:` expectations (193 unique). Union: 259 tokens.
- Compiler side: every string that reaches a diagnostic `code` position in `src/`: `code: "…"` literals, first args of `fail(`/`report(`, second args of path-first `report(path, "…")` (`src/package.ts:204`), first args of `error(`/`warn(`, and both branches of `cond ? "…" : "…"` ternaries in those positions (11 sites, each verified by reading). Excluded: internal LCT markers `none`/`ambiguous` (`src/checker/least-common-type.ts:24`, never emitted). Total: **269 codes emitted**.
- Emitter census: only `fail`, `report`, `error`, `warn` take codes; no computed/concatenated codes exist; CLI commands emit none.

## List 1 — spec names it, `src/` never emits (15)

All 15 are normative `spec/README.md` Diagnostics-table codes with zero `src/` hits (`grep -rn` count 0):

| code | named at |
| --- | --- |
| `boundary-cycle` | `spec/README.md:91` |
| `boundary-decoder-panic` | `spec/README.md:91` |
| `confusable-identifier` | `spec/README.md:89` |
| `cyclic-test-dependency` | `spec/README.md:86` |
| `impossible-gadt-pattern` | `spec/README.md:86` |
| `invalid-default-variant` | `spec/README.md:86` |
| `missing-entry-point` | `spec/README.md:86` |
| `mixed-script-identifier` | `spec/README.md:89` |
| `nonlocal-impl` | `spec/README.md:86` |
| `package-cycle` | `spec/README.md:86` |
| `qualified-string-prefix` | `spec/README.md:86` |
| `unconstrained-impl-parameter` | `spec/README.md:86` |
| `unmatched-delimiter` | `spec/README.md:88` |
| `unselected-main` | `spec/README.md:89` |
| `unsigned-comparison-always` | `spec/README.md:86` |

Rule-level cites I verified: `missing-entry-point` (`spec/cli/command-line.md:101`), `package-cycle` (`spec/lang/10-modules.md:580`), `cyclic-test-dependency` (`spec/lang/10-modules.md:309`). Note `src/KNOWN_ISSUES.md:49,51,136` openly records three of these as unimplemented (`qualified-string-prefix`, `unsigned-comparison-always`, `unconstrained-impl-parameter`).

Deliberately excluded from this list: 13 runtime-panic categories (they are panics, not diagnostics; all 13 are in `src/runtime-panic.ts:2-15`, e.g. `stack-exhausted`, `integer-overflow`) and 30 prose non-codes (`std-*` module names, `my-app`, `a-z`, `build-finished`, `date-time`, `dry-run`, `go-import`, `num-traits`).

## List 2 — `src/` emits it, spec never names it (70)

Emission site is the `file:line` of the `code:`/`fail(`/`report(`/`error(`/`warn(` call:

| code | emitted at |
| --- | --- |
| `ambiguous-bound-method` | `src/checker/expression-calls.ts:375` |
| `associated-function-needs-target` | `src/checker/expression-calls.ts:1346` |
| `associated-type-mismatch` | `src/checker/calls.ts:1447` |
| `bare-carriage-return` | `src/lexer.ts:177` |
| `closure-result-needs-annotation` | `src/checker/checker.ts:454` |
| `continue-outside-loop` | `src/checker/statements.ts:321` |
| `duplicate-data-field` | `src/checker/program-types.ts:193` |
| `duplicate-generic-parameter` | `src/parser/decorators.ts:81` |
| `duplicate-impl-member` | `src/checker/program-implementations.ts:727` |
| `duplicate-module-path` | `src/package.ts:221` |
| `duplicate-mutable-permission` | `src/parser/parser.ts:1065` |
| `duplicate-supertrait` | `src/checker/program-types.ts:448` |
| `duplicate-variant-pattern-field` | `src/checker/expression-control.ts:688` |
| `embedded-field-default` | `src/parser/parser.ts:864` |
| `empty-enum` | `src/parser/parser.ts:988` |
| `empty-match` | `src/parser/expression.ts:987` |
| `empty-suite` | `src/parser/parser.ts:1195` |
| `entry-point-parameters` | `src/checker/program-signatures.ts:375` |
| `expected-comprehension-clause` | `src/parser/expression.ts:942` |
| `expected-expression` | `src/parser/expression.ts:633` |
| `expected-interpolation-end` | `src/parser/parser.ts:247` |
| `expected-pattern` | `src/parser/expression.ts:1312` |
| `extra-trait-member` | `src/checker/program-implementations.ts:734` |
| `extra-trait-method` | `src/checker/program-implementations.ts:915` |
| `generic-arity` | `src/checker/expression-calls.ts:1332` |
| `generic-entry-point` | `src/checker/program-signatures.ts:276` |
| `generic-kind-conflict` | `src/checker/program-signatures.ts:176` |
| `identifier-not-nfc` | `src/lexer.ts:342` |
| `internal-enum-literal` | `src/checker/patterns.ts:338` |
| `internal-error` | `src/repl.ts:346` |
| `interpolated-pattern` | `src/parser/expression.ts:1324` |
| `invalid-character-literal` | `src/lexer.ts:671` |
| `invalid-module-path` | `src/package.ts:212` |
| `invalid-unary-operand` | `src/checker/expression-operators.ts:628` |
| `member-on-non-data` | `src/checker/expression-data.ts:510` |
| `mismatched-delimiter` | `src/lexer.ts:772` |
| `missing-associated-type` | `src/checker/program-implementations.ts:744` |
| `missing-method-body` | `src/parser/parser.ts:664` |
| `missing-variant-result` | `src/checker/program-declarations.ts:134` |
| `named-argument-needs-declaration` | `src/checker/expression-calls.ts:893` |
| `package-name-collision` | `src/package.ts:429` |
| `pattern-type-mismatch` | `src/checker/expression-control.ts:638` |
| `provider-type-mismatch` | `src/checker/calls.ts:1132` |
| `qualified-receiver-position` | `src/checker/expression-calls.ts:1358` |
| `recursive-trait-dictionary` | `src/checker/context.ts:711` |
| `reserved-name` | `src/parser/parser.ts:311` |
| `runtime-panic` | `src/repl.ts:345` |
| `tuple-binding-annotation` | `src/checker/statements.ts:404` |
| `unexpected-character` | `src/lexer.ts:756` |
| `unexpected-type-arguments` | `src/checker/calls.ts:514` |
| `uninhabited-binding` | `src/checker/expression-operators.ts:913` |
| `unknown-associated-function` | `src/checker/expression-calls.ts:1486` |
| `unknown-tuple-member` | `src/checker/expression-data.ts:698` |
| `unsupported-bound-associated-call` | `src/checker/trait-calls.ts:127` |
| `unsupported-context-operation` | `src/parser/expression.ts:1169` |
| `unsupported-derivation` | `src/checker/template-instances.ts:248` |
| `unsupported-gadt-result` | `src/checker/program-declarations.ts:140` |
| `unsupported-generic-impl` | `src/checker/program-implementations.ts:461` |
| `unsupported-host-provider-signature` | `src/checker/host-capabilities.ts:76` |
| `unsupported-local-default` | `src/parser/expression.ts:1056` |
| `unsupported-local-generic-function` | `src/parser/expression.ts:1037` |
| `unsupported-local-vararg` | `src/parser/expression.ts:1050` |
| `unsupported-match-subject` | `src/checker/expression-control.ts:387` |
| `unsupported-nested-variant-pattern` | `src/checker/patterns.ts:666` |
| `unsupported-package-use` | `src/package.ts:279` |
| `unsupported-type-form` | `src/parser/parser.ts:1165` |
| `unterminated-string-interpolation` | `src/lexer.ts:656` |
| `void-binding` | `src/checker/expression-comprehensions.ts:188` |
| `void-data-field` | `src/checker/program-types.ts:223` |
| `void-parameter` | `src/checker/context.ts:291` |

Each was also searched across all of `spec/` (any file type, no backtick requirement): the only hits are the fixture filename `duplicate-data-field.hd` (whose `reject:` expectation is `duplicate-field` — see list 3) and prose mentions of the runtime-panics section. `internal-error` and `runtime-panic` are REPL-only (`src/repl.ts:345-346`).

## List 3 — same failure, different spelling (2 pairs)

| spec | compiler | evidence |
| --- | --- | --- |
| `unmatched-delimiter` | `mismatched-delimiter` | Spec (`spec/README.md:103`): a closing delimiter with no matching open, or closing a different kind. Compiler (`src/lexer.ts:772`): `unexpected '…' delimiter`. Same failure, different code. |
| `duplicate-field` | `duplicate-data-field` | Spec (`spec/lang/08-data-and-enums.md:29`, `r[data.field.unique]`): duplicate declared fields are `duplicate-field`. Compiler (`src/checker/program-types.ts:193`): declarations get `duplicate-data-field`; only literals get `duplicate-field` (`src/checker/expression-data.ts:96`, matching fixture `typing/invalid/duplicate-data-field.hd` which expects `duplicate-field`). |

No other respellings exist: letter-stripped normalization over all 322 codes finds zero further collisions, and no edit-distance ≤2 pairs span the two sets. Near-misses that are genuinely different (not listed): `ambiguous-method`/`ambiguous-bound-method` (different ambiguity conditions), `break-outside-loop`/`continue-outside-loop` (different keywords), `generic-arity`/`partial-generic-arguments` (both emitted by `src/`).

Consistency notes: every fixture/`cases.tsv` code is inside the README table (0 outliers), and 21 table codes have no fixture. The brief said "`# error:` headers" — the actual format is `# diagnostic:` markers plus `cases.tsv` `reject:` expectations; both were collected.

Ready for Job 3.