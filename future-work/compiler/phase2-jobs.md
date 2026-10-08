# New Compiler: Phase-2 Job Briefs

## Triage: Conformance Buckets To Jobs (T4, 2026-10-08)

Source: the Failure Buckets of `compiler/CONFORMANCE.md` at `561efcfb`
(497 pass, 1,114 fail, 1,327 unsupported of 2,938). Each `fail:` bucket is
the first diagnostic code; each `unsupported:` bucket is the pipeline
stage that first declined the case. The counts below sum to those
totals. A case can need more than one job; the table names the primary
fixing job, and shared reach is noted per job below.

| Bucket | Cases | Fixing job | Why |
| --- | ---: | --- | --- |
| `fail:unknown-import` | 777 | P2-5 | imports resolve only through package interfaces |
| `fail:type-mismatch` | 87 | P2-2 | unification, joins, and call checking |
| `fail:unknown-module` | 64 | P2-5 | module discovery and paths, chapters 10 and 03 |
| `fail:unsatisfied-trait-bound` | 31 | P2-3 | trait solving and evidence |
| `fail:unknown-method` | 29 | P2-2 | method and call resolution |
| `fail:unknown-data-field` | 19 | P2-4 | aggregate member lookup |
| `fail:syntax-error` | 10 | — | phase-1 parser gap: old syntax the new parser still accepts |
| `fail:argument-count` | 9 | P2-2 | call and constructor arity |
| `fail:identity-requires-references` | 9 | P2-1 | identity and reference rules in bodies |
| `fail:overlapping-impl` | 9 | P2-3 | coherence |
| `fail:unknown-name` | 9 | P2-1 | locals, closures, and defaults; 2 module-path cases also need P2-5 |
| `fail:missing-requirement` | 8 | P2-2 | requirement rows, mostly chapter 11 |
| `fail:nonexhaustive-match` | 8 | P2-1 | exhaustiveness |
| `fail:unknown-trait` | 8 | P2-3 | trait resolution, mostly chapter 11 |
| `fail:cannot-infer-type` | 7 | P2-2 | inference |
| `fail:missing-supertrait-implementation` | 4 | P2-3 | supertrait bounds |
| `fail:pipe-step-needs-placeholder` | 3 | P2-1 | pipe expressions |
| `fail:bang-call-outside-suspension` | 2 | P2-1 | suspension context in defaults |
| `fail:invalid-token` | 2 | — | phase-1 lexer gap |
| `fail:missing-required-field` | 2 | P2-4 | aggregate construction |
| `fail:orphan-impl` | 2 | P2-3 | coherence |
| `fail:pattern-arity` | 2 | P2-1 | patterns |
| `fail:tab-whitespace` | 2 | — | phase-1 lexer gap |
| `fail:trait-used-as-type` | 2 | P2-3 | trait positions |
| `fail:unknown-type` | 2 | P2-2 | type positions; 1 case also needs a P2-5 import |
| `fail:integer-literal-range` | 1 | P2-2 | literals |
| `fail:let-else-falls-through` | 1 | P2-1 | control flow |
| `fail:not-callable` | 1 | P2-2 | calls |
| `fail:placeholder-outside-pipe` | 1 | P2-1 | pipe expressions |
| `fail:type-used-as-value` | 1 | P2-4 | chapter 14 positions |
| `fail:unknown-variant` | 1 | P2-4 | enum variant resolution |
| `fail:unreachable-match-arm` | 1 | P2-1 | exhaustiveness |
| `unsupported:Collect` | 879 | P2-6 | collection into the emit pipeline; 83 chapter-11 cases also need P2-7, 85 harness cases also need P2-9 |
| `unsupported:Body` | 200 | P2-1 | body checking, chapters 05, 09, and 04 |
| `unsupported:TestOverlay` | 101 | P2-9 | test overlay and runner |
| `unsupported:CLI` | 99 | P2-10 | all CLI cases |
| `unsupported:FolderIface` | 35 | P2-5 | folder interfaces |
| `unsupported:Discover` | 13 | P2-5 | package discovery |

### Reach Per Job

Exclusive cases per primary job, with shared reach noted. Density uses
the midpoint of the size estimate below.

| Job | Exclusive cases | Also shares | kLOC | Cases per kLOC |
| --- | ---: | --- | ---: | ---: |
| P2-5 | 889 | — | 6.5 | 137 |
| P2-6 | 879 | — | 7.5 | 117 |
| P2-1 | 236 | — | 6.0 | 39 |
| P2-2 | 144 | — | 6.0 | 24 |
| P2-9 | 101 | 85 of Collect | 5.0 | 20 |
| P2-10 | 99 | — | 10.0 | 10 |
| P2-3 | 56 | — | 6.5 | 9 |
| P2-4 | 23 | — | 6.0 | 4 |
| P2-7 | 0 | 83 of Collect | 7.5 | shared |
| P2-8 | 0 | host runtime rows inside other buckets | 5.5 | shared |
| P2-11 | 0 | no REPL rows exist yet | 5.5 | 0 |
| P2-12 | 0 | — | 4.0 | 0 |
| Unowned | 14 | phase-1 parser and lexer gaps, no P2 brief covers them | — | — |

### Order Verdict

The dependency chain forces the sequence 1-2-3-4-5-6-7-8-9-10-11-12:
every job's prerequisites precede it, and no independent pair is
density-inverted. The order table below is unchanged; confirmed 2026-10-08.

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
| Exit test | All selected cases check with stable initialization groups and diagnostics; facts and generated instances appear once; debug/inspect uses only recorded evidence and TIR operations. |
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
