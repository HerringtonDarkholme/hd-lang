# New Compiler: Phase-2 Job Briefs

## Triage: Conformance Buckets To Jobs (T4b, 2026-10-08)

Source: the Failure Buckets of `compiler/CONFORMANCE.md` regenerated at
`99f3d038` (1,055 pass, 1,061 fail, 826 unsupported of 2,942). Each
`fail:` bucket is the first diagnostic code; each `unsupported:` bucket
is the pipeline stage that first declined the case. P2-1a collapsed
`fail:unknown-import` from 777 to 29 and `unsupported:Collect` from 879
to 98; the largest buckets are now `fail:no-diagnostic` (491, split by
expected code below), `unsupported:Emit` (270), `unsupported:Body`
(264), and `fail:type-mismatch` (126). A case can need more than one
job; the table names the primary fixing job, and shared reach is noted
per job below.

| Bucket | Cases | Fixing job | Why |
| --- | ---: | --- | --- |
| `fail:no-diagnostic` | 491 | split below | checker accepts what the spec rejects; biggest groups are trait (152), inference (145), and body (98) checks |
| `unsupported:Emit` | 270 | P2-6 | emit pipeline; 19 ch-11 cases also need P2-7, 11 testing cases also need P2-9 |
| `unsupported:Body` | 264 | P2-1 | body checking, chapters 05, 09, and 04; 18 ch-11 cases also need P2-7, 4 testing cases also need P2-9 |
| `fail:type-mismatch` | 126 | P2-2 | unification, joins, and call checking |
| `unsupported:CLI` | 101 | P2-10 | all CLI cases |
| `unsupported:Collect` | 98 | P2-6 | collection into the emit pipeline; 14 ch-11 cases also need P2-7, 7 testing cases also need P2-9 |
| `fail:runtime-exit` | 72 | P2-6 | wrong exit code at runtime is codegen, not diagnosis |
| `fail:unknown-module` | 67 | P2-5 | module discovery and paths, chapters 10 and 03 |
| `fail:unknown-method` | 49 | P2-2 | method and call resolution |
| `unsupported:RunCase` | 40 | P2-8 | engine run of emitted Wasm; 22 ch-11 cases also need P2-7; needs P2-6 emit first |
| `fail:unsatisfied-trait-bound` | 35 | P2-3 | trait solving and evidence |
| `unsupported:FolderIface` | 35 | P2-5 | folder interfaces |
| `fail:unknown-data-field` | 31 | P2-4 | aggregate member lookup |
| `fail:unknown-import` | 29 | P2-5 | leftover package-interface imports P2-1a did not reach |
| `fail:argument-count` | 17 | P2-2 | call and constructor arity |
| `fail:nonexhaustive-match` | 16 | P2-1 | exhaustiveness |
| `unsupported:Discover` | 13 | P2-5 | package discovery |
| `fail:cannot-infer-type` | 12 | P2-2 | inference |
| `fail:missing-requirement` | 11 | P2-2 | requirement rows, mostly chapter 11 |
| `fail:syntax-error` | 10 | — | phase-1 parser gap: old syntax the new parser still accepts |
| `fail:identity-requires-references` | 9 | P2-1 | identity and reference rules in bodies |
| `fail:missing-return-value` | 9 | P2-1 | control-flow value rules |
| `fail:overlapping-impl` | 9 | P2-3 | coherence |
| `fail:unknown-name` | 9 | P2-1 | locals, closures, and defaults |
| `fail:unknown-trait` | 7 | P2-3 | trait resolution, mostly chapter 11 |
| `fail:stdout` | 7 | P2-6 | wrong console output is codegen, not diagnosis |
| `fail:pipe-step-needs-placeholder` | 5 | P2-1 | pipe expressions |
| `unsupported:TestCase` | 4 | P2-9 | test-case execution |
| `fail:missing-supertrait-implementation` | 4 | P2-3 | supertrait bounds |
| `fail:bare-variant-pattern` | 3 | P2-1 | patterns |
| `fail:bang-call-outside-suspension` | 2 | P2-1 | suspension context in bodies |
| `fail:invalid-result-propagation` | 2 | P2-1 | `?` outside a propagating body is control-flow checking |
| `fail:invalid-token` | 2 | — | phase-1 lexer gap |
| `fail:missing-entry-point` | 2 | P2-10 | entry-point rules |
| `fail:missing-required-field` | 2 | P2-4 | aggregate construction |
| `fail:not-callable` | 2 | P2-2 | calls |
| `fail:orphan-impl` | 2 | P2-3 | coherence |
| `fail:pattern-arity` | 2 | P2-1 | patterns |
| `fail:tab-whitespace` | 2 | — | phase-1 lexer gap |
| `fail:trait-used-as-type` | 2 | P2-3 | trait positions |
| `fail:type-used-as-value` | 2 | P2-4 | chapter 14 positions |
| `fail:unknown-type` | 2 | P2-2 | type positions |
| `fail:discarded-must-use-value` | 1 | P2-1 | must-use discards in bodies |
| `fail:duplicate-data-pattern-field` | 1 | P2-1 | patterns |
| `fail:integer-literal-range` | 1 | P2-2 | literals |
| `fail:invalid-test-statement` | 1 | P2-9 | test layout |
| `fail:let-else-falls-through` | 1 | P2-1 | control flow |
| `fail:placeholder-outside-pipe` | 1 | P2-1 | pipe expressions |
| `fail:suspension-forbidden-context` | 1 | P2-1 | suspension context in bodies |
| `fail:unknown-named-argument` | 1 | P2-2 | calls |
| `fail:unknown-variant` | 1 | P2-4 | enum variant resolution |
| `unsupported:Link` | 1 | P2-6 | linking the emitted module |

### `fail:no-diagnostic` By Expected Code (T4b)

Expected code is the `reject:CODE` column of `spec/conformance/cases.tsv`.
Top codes per owning job; the tail follows in the same row. Judgment
calls: mutability codes (`readonly-*`, `mut-on-*`,
`mutable-receiver-required`) sit with body checking (P2-1) while
`mutable-upgrade` sits with rows (P2-2); `prelude-name-shadow` and
`ambiguous-method` sit with scope resolution (P2-1);
`invalid-result-propagation` sits with control flow (P2-1).

| Job | Cases | Top expected codes (chapter) |
| --- | ---: | --- |
| P2-3 | 152 | `unsatisfied-trait-bound` 41 (04: 11, 09: 10, 10: 7, 14: 7, 05: 4, std: 2), `trait-method-signature` 12, `invalid-error-marker` 7, `duplicate-trait-member` 6, `misplaced-derivation` 6, `missing-trait-method` 5, `missing-supertrait-implementation` 5, `overlapping-impl` 5, `invalid-delegation` 4, `derive-field-missing-trait` 4, `member-not-derivable` 7 incl. std, plus 50 across 23 codes |
| P2-2 | 145 | `type-mismatch` 58 (04: 18, 14: 14, 11: 5, 05: 4, 09: 4, std: 8, other: 5), `invalid-variance` 22 (04: 21), `mutable-upgrade` 15, `missing-requirement` 10 (11: 6), `generic-requirement-key-collision` 5, plus 35 across 20 codes |
| P2-1 | 98 | `readonly-root` 13, `mutable-receiver-required` 12, `ambiguous-promoted-member` 9, `suspension-forbidden-context` 6, `unknown-name` 6, `prelude-name-shadow` 4, `mut-on-tuple` 4, `mut-on-primitive` 3, `readonly-argument-to-mutable-parameter` 3, `duplicate-module-name` 3, warnings (`redundant-let-mut`, `unused-local-binding`, `unsigned-comparison-always`, others) 13, plus 22 across 14 codes |
| P2-4 | 66 | `embedded-*` 14, `decorator-target-kind` 11, `private-type-leak` 6, `duplicate-fact` 5, `duplicate-field` 4, `boundary-private-field` 3, `invalid-member-line` 3, `inspectable-requirement` 3, plus 17 across 10 codes |
| P2-9 | 12 | test-layout codes (`duplicate-test-name`, `non-literal-test-argument`, `misplaced-test-case`, `misplaced-tests-block` ×2 each) 8, `public-test-item`, `test-only-use`, `duplicate-tests-block`, `invalid-test-statement` ×1 each |
| P2-5 | 10 | `unknown-module` 3, `duplicate-module-name` 2, `reserved-module-name` 2, `invalid-module-path`, `template-names-binding`, `unknown-import` ×1 each, all chapter 10 |
| P2-10 | 4 | `entry-point-parameters` 2, `private-main` (warn) 2 |
| P2-12 | 2 | `mixed-script-identifier`, `derivation-line-drift` (both warn) |
| P2-8 | 1 | `nonhost-entry-requirement` (11) |
| P2-6 | 1 | `unknown-panic-category` (10) |

The single `syntax-error [11]` no-diagnostic case parsed clean and stayed
silent through check; if the parser should reject it, it joins the
Unowned parser gaps instead of P2-2.

### Reach Per Job

Exclusive cases per primary job, with shared reach noted. Density uses
the midpoint of the size estimate below.

| Job | Exclusive cases | Also shares | kLOC | Cases per kLOC |
| --- | ---: | --- | ---: | ---: |
| P2-1 | 424 | — | 6.0 | 71 |
| P2-2 | 366 | — | 6.0 | 61 |
| P2-6 | 449 | — | 7.5 | 60 |
| P2-3 | 211 | — | 6.5 | 32 |
| P2-5 | 154 | — | 6.5 | 24 |
| P2-4 | 102 | — | 6.0 | 17 |
| P2-10 | 107 | — | 10.0 | 11 |
| P2-8 | 41 | RunCase needs P2-6 emit first | 5.5 | 7 |
| P2-9 | 17 | 22 testing cases inside Body/Emit/Collect | 5.0 | 3 |
| P2-12 | 2 | — | 4.0 | 1 |
| P2-7 | 0 | 73 ch-11 cases inside Body/Emit/Collect/RunCase | 7.5 | shared |
| P2-11 | 0 | no REPL rows exist yet | 5.5 | 0 |
| Unowned | 14 | phase-1 parser and lexer gaps, no P2 brief covers them | — | — |

### Order Verdict

The dependency chain forces the sequence 1-2-3-4-5-6-7-8-9-10-11-12,
and the density ranking agrees with it everywhere a reorder is possible;
reconfirmed on the T4b numbers 2026-10-08. Each dependency below is a code
path, not a phase label: P2-2's call checking consumes P2-1's checked
expression TIR; P2-3's solver normalizes the associated types P2-2
instantiates; P2-4's template instances carry P2-1 bodies and P2-3
evidence; P2-5's folder interfaces resolve the trait bounds and aggregate
shapes P2-3 and P2-4 define; P2-6's collect/mono runs over verified TIR
behind those interfaces; P2-7 lowers into P2-6's emit pipeline; P2-8's
engines execute P2-6/P2-7 Wasm; P2-9's runner dispatches through P2-8;
P2-10's CLI dispatches to the P2-5 package and P2-9 test commands;
P2-11 reuses the P2-8 engines behind the P2-10 command shell; P2-12
indexes every other job's output. By density the order would be
P2-1 (71), P2-2 (61), P2-6 (60), P2-3 (32), P2-5 (24), P2-4 (17),
P2-10 (11), P2-8 (7), P2-9 (3), P2-12 (1): P2-6's 60 cannot move ahead
of P2-3–P2-5 because emit consumes their checking, and P2-10 cannot
move ahead of P2-8–P2-9 because `hd test` runs through the engines and
runner. No independent pair is density-inverted. The order table below
is unchanged.

Phase 2 is the "make it work" pass after the compiler skeleton. The order
below is the ordered list from [the work estimate](work-estimate.md), refined
so that prerequisites come first and, among independent jobs, the job that
unblocks more conformance cases comes first. TypeScript files are behavioral
checklists only: preserve the specified behavior, but do not port their
structure.

The case counts are the current rows of `spec/conformance/cases.tsv` whose
`specification` column names a chapter. They are an indication of reach, not
an exclusive assignment: a case can need more than one phase-2 job. Every
exit test includes `cargo test --workspace` and the relevant selection of the
portable conformance suite.

| Order | Job | Primary dependency | Candidate chapter rows | Estimated new Rust |
| ---: | --- | --- | ---: | ---: |
| 1 | P2-1 Core bodies and flow | M4a body-checker skeleton | 655 | 5–7 kLOC |
| 2 | P2-2 Inference, calls, and rows | P2-1 | 741 | 5–7 kLOC |
| 3 | P2-3 Traits and evidence | P2-2 | 512 | 5–8 kLOC |
| 4 | P2-4 Aggregate and module semantics | P2-1–P2-3 | 533 | 5–7 kLOC |
| 5 | P2-5 Packages and interfaces | P2-3–P2-4 | 256 plus CLI cases | 5–8 kLOC |
| 6 | P2-6 Synchronous code generation | P2-1–P2-5 | up to 1,032 runtime rows | 6–9 kLOC |
| 7 | P2-7 Suspension lowering | P2-6 | 260 plus panic cases | 6–9 kLOC |
| 8 | P2-8 Host ABI and engines | P2-6–P2-7 | host/capability cases | 4–7 kLOC |
| 9 | P2-9 Test runner | P2-6–P2-8 | testing rows plus CLI cases | 4–6 kLOC |
| 10 | P2-10 CLI and package commands | P2-5–P2-9 | all CLI cases | 8–12 kLOC |
| 11 | P2-11 REPL | P2-6–P2-8, P2-10 | REPL CLI cases | 4–7 kLOC |
| 12 | P2-12 Tooling and documentation | P2-1–P2-11 | examples and tooling cases | 3–5 kLOC |

## P2-1. Core Bodies And Flow

| Field | Brief |
| --- | --- |
| Scope | Complete expression, statement, pattern, control-flow, exhaustiveness, termination, and poison-based recovery checking, producing verified TIR. |
| TypeScript checklist | `src/checker/expression-*.ts`, `statements.ts`, `patterns.ts`, `exhaustiveness.ts`, `termination.ts`, `local-declarations.ts` |
| Specification | `lang/03-names-and-scopes.md`, `lang/05-expressions.md`, `lang/06-control-flow.md`, and the pattern/matching parts of `lang/08-data-and-enums.md` |
| Design | `type-checking.md` §§2.2–2.3, 6–8, 10; `checking-and-tir.md` §§4.13.3, 4.13.6–4.13.8, 4.13.11, 4.14 |
| Cases to turn green | Type and runtime rows for language chapters 03, 05, and 06; chapter-08 rows for construction, access, and matching. These selectors currently reach 655 rows. |
| Exit test | Those selected rows have the specified accept/diagnostic/runtime result; malformed bodies recover to the next independent statement; the TIR verifier accepts every successful body. |
| Size estimate | 5–7 kLOC of Rust; first of the work estimate's 5–7 checker jobs. |

## P2-2. Inference, Calls, Literals, Rows, And Generics

| Field | Brief |
| --- | --- |
| Scope | Finish unification-driven inference, literals, coercions and joins, requirement rows, variance, generic instantiation, calls, methods, and their diagnostics. |
| TypeScript checklist | `src/checker/calls.ts`, `expression-calls.ts`, `literal-*.ts`, `row-rules.ts`, `requirement-rows.ts`, `variance.ts`, `written-type-validation.ts`, `cannot-infer.ts`, `bound-inference.ts`, `generic-type.ts` |
| Specification | `lang/04-type-system.md`, `lang/07-functions.md`, `lang/11-requirements-and-suspension.md` requirement-row sections, and `lang/12-variadic-generics.md` |
| Design | `type-checking.md` §§2.4–2.8, 3–5, 6.2, 9–10; `checking-and-tir.md` §§4.13.2, 4.13.4–4.13.5, 4.13.7, 4.14 |
| Cases to turn green | Type and runtime rows for chapters 04, 07, 11, and 12, excluding cases assigned specifically to suspension lowering. The first three chapters currently account for 741 rows. |
| Exit test | The selected cases infer the same public types and diagnostic codes on cold and incremental checks; successful calls leave no unresolved inference variables and pass TIR verification. |
| Size estimate | 5–7 kLOC of Rust. |

## P2-3. Traits, Associated Types, And Evidence

| Field | Brief |
| --- | --- |
| Scope | Implement trait selection, associated-type normalization, coherence, defaults, derivation, trait values, and the evidence consumed by code generation. |
| TypeScript checklist | `src/checker/associated-bindings.ts`, `program-implementations.ts`, `supertrait-bounds.ts`, `trait-*.ts`, `typed-derivation.ts`, `implementation-index.ts`, `impl-parameters.ts`, `ambiguous-solutions.ts` |
| Specification | `lang/09-traits.md`; typed derivation and error derivation in `lang/14-annotations.md` |
| Design | `trait-solver.md` §§1–12 and 14; `type-checking.md` §1.6; `checking-and-tir.md` §4.13.9; `codegen.md` §13.5 |
| Cases to turn green | All chapter-09 rows and chapter-14 rows whose rules cover typed derivation, error derivation, or generated implementations; candidate reach is at most 512 current rows. Include `spec/conformance/trees/{dyn-inherent-nonlocal,nonlocal-impl,money-ops}`. |
| Exit test | Trait selections and failures are deterministic, coherence rejects every forbidden overlap, projections normalize at the three designed points, and every successful call records verifier-valid evidence. |
| Size estimate | 5–8 kLOC of Rust. |

## P2-4. Aggregates, Facts, Initialization, And Inspection

| Field | Brief |
| --- | --- |
| Scope | Complete data/enum templates, facts, tuples, embedding, module initialization, visibility, debug/inspect behavior, and supported fixes. |
| TypeScript checklist | `src/checker/template-instances.ts`, `tuple-templates.ts`, `*facts.ts`, `program-embedding.ts`, `module-initialization.ts`, `member-visibility.ts`, `debug-*.ts`, `inspectable.ts`, `decorators.ts` |
| Specification | Aggregate rules in `lang/08-data-and-enums.md`; initialization and visibility in `lang/10-modules.md`; annotations and derivation in `lang/14-annotations.md` |
| Design | `checking-and-tir.md` §§4.13.8–4.13.10; `type-checking.md` §§1.7, 8; `resolution-and-interfaces.md` §§4.10–4.12; `codegen.md` §12.3 |
| Cases to turn green | Chapter-08, chapter-10 initialization/visibility, and chapter-14 fact/template/debug rows. Include `spec/conformance/trees/{init-cycle,init-group,init-order,shared-enum-init,visibility}`; candidate reach is at most 533 rows. |
| Exit test | All selected cases check with stable module cycles and diagnostics; facts and generated instances appear once; debug/inspect uses only recorded evidence and TIR operations. |
| Size estimate | 5–7 kLOC of Rust. |

## P2-5. Packages, Discovery, And Interfaces

| Field | Brief |
| --- | --- |
| Scope | Implement package and folder discovery, manifests and dependencies, interface construction/loading, privacy, ownership, and standard-library bootstrap. |
| TypeScript checklist | `src/package.ts`, `manifest.ts`, `dependencies/*.ts`, `checker/module-paths.ts`, `package-ownership.ts`, `standard-*.ts`, `import-bindings.ts`, `standard-library.ts` |
| Specification | `lang/03-names-and-scopes.md`, `lang/10-modules.md`, package ownership in `lang/09-traits.md`, and `cli/command-line.md` package/dependency sections |
| Design | `resolution-and-interfaces.md` §§4.7–4.12; `cache.md` §§5.1–5.5; `commands.md` §§7.1–7.2; `data-structures.md` §§3.14–3.17, 3.20 |
| Cases to turn green | Chapter-10 rows; every `spec/conformance/trees/` and `packages/` case; CLI families `dep-*`, `dev-dependency-*`, `exe-*`, `member-unlisted`, and `typeid-package-name`. |
| Exit test | Cold and cached checks resolve the same graph and diagnostics; all package-tree cases pass; a body-only dependency edit preserves its dependent interface key while a public-signature edit invalidates it. |
| Size estimate | 5–8 kLOC of Rust; the work estimate's 1–2 resolution/package jobs. |

## P2-6. Synchronous Code Generation

| Field | Brief |
| --- | --- |
| Scope | Emit executable Wasm for data, enums, closures, strings, collections, module initialization, comparisons, iterators, and panics. |
| TypeScript checklist | `src/emitter/data.ts`, `value-comparison.ts`, `iterator.ts`, `panic-sites.ts`, `host-providers.ts`, `function-body.ts`, `intrinsics.ts`, `runtime/*.wat` |
| Specification | Runtime semantics across language chapters 04–10 and 14, especially `lang/06-control-flow.md#runtime-panics` and `lang/10-modules.md#module-initialization` |
| Design | `codegen.md` §§11–13; `wasm-layout.md` §§15.1–15.6; `representation-runtime.md` §§2–10; `runtime-and-host.md` §§16.1–16.4 |
| Cases to turn green | All `runtime/valid/` and `runtime/panic/` cases not requiring suspension, host I/O, or the test-runner services; this is the base for the suite's 1,032 current runtime rows. |
| Exit test | Each selected program validates as Wasm and has the specified result under Node/V8; panics preserve category and source span; two clean builds produce identical bytes. |
| Size estimate | 6–9 kLOC of Rust; first half of the work estimate's 12–18 kLOC emitter group. |

## P2-7. Suspension, Concurrency, And Cancellation

| Field | Brief |
| --- | --- |
| Scope | Lower suspending functions and stored suspensions to state machines, including `all!`, `race!`, cancellation, and `defer` cleanup. |
| TypeScript checklist | `src/emitter/suspension.ts`, `stored-suspension.ts`, `checker/expression-suspensions.ts`, `checker/captured-cells*.ts` |
| Specification | Suspending functions, protocol, cancellation, and runtime boundary in `lang/11-requirements-and-suspension.md`; deferred cleanup in `lang/06-control-flow.md` |
| Design | `suspension.md` §§14.1–14.9; `codegen.md` §§12.5, 13.6; `wasm-layout.md` §15.2; `representation-runtime.md` §10.1 |
| Cases to turn green | Chapter-11 runtime rows involving `Suspend`, `await`, `all!`, `race!`, or cancellation; `runtime/panic/*suspension*`, `*cancel*`, `race-empty-at-run-time.hd`, and `defer-*.hd`. |
| Exit test | Selected cases pass under Node/V8, cancellation runs each required cleanup exactly once, and the suspension/frame verifier accepts every emitted state machine. |
| Size estimate | 6–9 kLOC of Rust; completes the estimated emitter group. |

## P2-8. Host ABI, Capabilities, And Engines

| Field | Brief |
| --- | --- |
| Scope | Implement the generated host ABI, exchange buffer, structured values, capabilities and limits, plus Node/V8 and browser adapters. |
| TypeScript checklist | `src/host-arguments.ts`, `host-boundary.ts`, `host-functions.ts`, `host-values.ts`, `runtime-*.ts`, `web-host.ts`, `checker/host-capabilities.ts` |
| Specification | Wasm boundary in `lang/10-modules.md`; runtime boundary in `lang/11-requirements-and-suspension.md`; capabilities and limits in `cli/command-line.md`; applicable `spec/std/{host,console,fs,net,http,sys}.md` APIs |
| Design | `runtime-and-host.md` §§16–17; `engines-and-test-runner.md` §18; `commands.md` §§20.2–20.4; `representation-runtime.md` §§7.5, 10.4 |
| Cases to turn green | Host-dependent runtime rows; `runtime/panic/host-result-out-of-range.hd` and `println-console-closed.hd`; CLI families `cap-*`, `wasm-cap-flags-only`, `wasm-run-built`, and `entry-err-*`. |
| Exit test | The same artifact has the specified behavior through Node/V8 and the browser adapter; imports exactly match granted capabilities; malformed host results fail with the specified panic rather than corrupting Wasm memory. |
| Size estimate | 4–7 kLOC of Rust/JavaScript glue; the work estimate's runtime job. |

## P2-9. Test Runner

| Field | Brief |
| --- | --- |
| Scope | Build and run unit, integration, table, property, panic, timeout, and snapshot tests with deterministic listing and reports. |
| TypeScript checklist | `src/test-runner.ts`, `property-tests.ts`, `snapshots.ts`, `commands/doc-tests.ts` |
| Specification | `lang/10-modules.md#standard-testing`, `spec/std/testing.md`, and `cli/command-line.md` test/machine-output/exit-status rules |
| Design | `engines-and-test-runner.md` §§19.1–19.6; `commands.md` §§7.3–7.4, 20.1 |
| Cases to turn green | Language and stdlib testing rows; CLI families `test-*`, `json-test-*`, `release-test-checked`, and the `trees/{integration-shared,test-edges,test-only,test-relative}` package trees. |
| Exit test | Listing and execution order are deterministic; every selected CLI `expect.txt` matches; property replay, snapshots, panics, timeouts, and test-only dependency boundaries behave as specified. |
| Size estimate | 4–6 kLOC of Rust. |

## P2-10. CLI And Package Commands

| Field | Brief |
| --- | --- |
| Scope | Complete command parsing and all package, file, build, run, check, dependency, documentation, formatting, creation, cache, and diagnostic-report flows. |
| TypeScript checklist | `src/cli*.ts`, `commands/*.ts`, `diagnostic-report.ts`, `package.ts`, `manifest.ts` |
| Specification | All of `spec/cli/command-line.md`, plus entry-point and Wasm-boundary rules in `lang/10-modules.md` |
| Design | `commands.md` §§7.1–7.7, 20.1–20.4; `cli-forms.md`; `cache.md`; `engines-and-test-runner.md` §§18.1–18.3 |
| Cases to turn green | Every row of `spec/conformance/cli-cases.tsv` except the REPL cases reserved for P2-11; all directories under `spec/conformance/cli/` named by those rows. |
| Exit test | The CLI suite matches every selected `expect.txt`, exit code, output mode, and filesystem effect from clean and warm caches; `hd check`, `hd run`, `hd build`, and `hd FILE.hd` work in package and single-file modes. |
| Size estimate | 8–12 kLOC of Rust, preferably split into command shell/reporting and package/dependency sub-jobs. |

## P2-11. REPL

| Field | Brief |
| --- | --- |
| Scope | Implement incremental REPL sessions, shadowing, journal/replay, panic recovery, Ctrl-C behavior, and browser-worker execution. |
| TypeScript checklist | `src/repl*.ts` and the replay/session paths in `src/compiler.ts` |
| Specification | `cli/command-line.md#repl`, plus the expression, scope, module, panic, suspension, and capability rules exercised by entered forms |
| Design | `live-execution.md` §§2–10; `commands.md` §§7.8, 20.5; `engines-and-test-runner.md` §18.4 |
| Cases to turn green | No dedicated REPL row exists in `spec/conformance/cli-cases.tsv` yet. Add scripted session cases for expression evaluation, declarations and shadowing, multiline input, panic recovery, Ctrl-C, journal replay, and browser-worker stop. |
| Exit test | Scripted sessions match specified stdout/stderr and status; definitions shadow without mutating old values; journal replay is deterministic; panic and Ctrl-C leave the session usable. |
| Size estimate | 4–7 kLOC of Rust/host glue. |

## P2-12. Docs, Highlighting, Formatting, And Fixes

| Field | Brief |
| --- | --- |
| Scope | Finish documentation tests and output, syntax highlighting, spec indexing, Unicode-script checks, formatting, and machine-applicable fixes. |
| TypeScript checklist | `src/doc-tests.ts`, `commands/doc-tests.ts`, `highlight.ts`, `spec-index.ts`, `unicode-scripts.ts`, and formatting/fix paths under `src/commands/` |
| Specification | All language chapters as doctest inputs; documentation, rewriting, and machine-output sections of `cli/command-line.md` |
| Design | `commands.md` §§7.6–7.7; `syntax.md` §§4.1–4.6; `checking-and-tir.md` §4.14; `live-execution.md` §8 for browser highlighting |
| Cases to turn green | `spec/conformance/examples.tsv`; `spec/conformance/trees/doc-tests`; CLI families `doc-*`, `fmt-*`, `json-diagnostic-fixes`, and `derivation-lines-agree`. |
| Exit test | Every indexed spec example is extracted and checked at its declared phase; doc and formatting CLI cases match exactly; a second format/fix pass is a no-op; all emitted edits have valid UTF-8 boundaries. |
| Size estimate | 3–5 kLOC of Rust/JavaScript, consistent with the work estimate's 2–4 kLOC docs group plus formatter/fix integration. |
