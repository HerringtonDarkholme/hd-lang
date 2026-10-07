# New Compiler: Work Estimate From The TypeScript Prototype

Status: This planning estimate defines no accepted behavior.

It audits frozen `src/` against the [overview](design-overview.md), [front half](syntax.md), [checker/TIR](checking-and-tir.md), [back half](codegen.md), [runtime](runtime-and-host.md), [commands](commands.md), and [build order](build-order.md).

## Measurement

| Item | Value |
| --- | --- |
| Coverage baseline | `54a4bad453c0c50ce93e0f0ca5bcedf1c422688e` (`54a4bad4`) |
| Frozen prototype | 202 TypeScript files; 76,220 lines under `src/` |
| Rust inspected | 23 crates; 24,942 lines under `compiler/crates/*/src` |
| Inspection | `ast-grep outline --lang rust --items exports --view names` run once per crate, plus source inspection |
| Estimate unit | One 2–2.5-hour agent job produces roughly 4–7k reviewed Rust lines |
| Counting rule | TS lines identify checklist scope; they are not a TS-to-Rust conversion ratio |

## Coverage Today

| Group | Status | Rust coverage at baseline | Rust lines |
| --- | --- | --- | ---: |
| Lexer and parser | partial | `hd_syntax`: lexer, layout, lossless green tree, parser forms, skim pass; typed views and symbol/highlight parity remain | 6,518 |
| Resolution and packages | partial | `hd_project`: source/package discovery model; `hd_resolve`: headers, interface codec, lowering, seeded interfaces, views | 4,639 |
| Checker | partial | `hd_types`: pools, unification, initial solver; `hd_check`: staged body walk; `hd_tir`: designed IR and wire form | 4,580 |
| Emitter | partial | `hd_mono`: layouts, passes, suspension plan; `hd_wasm`: deterministic binary emission and metadata skeleton | 2,063 |
| Runtime and host | partial | `hd_host_abi`: ABI records; `hd_run`: engine/journal/test model; Node/wasmtime/web adapters are skeletal | 1,151 |
| Test runner | partial | `hd_run::tests_model` and `hd_testkit` provide case records and harness helpers; execution/reporting remain | 278 |
| CLI commands | partial | `hd_cli` dispatch/disk/Node shell, `hd_driver` pipeline/reporting, and `hd_stdpack` embedding exist | 2,320 |
| REPL | partial | `hd_run::journal` has the durable record model; no input classifier, session compiler, or terminal loop | 180 |
| Docs | missing | `hd_doc` is only a 33-line crate shell | 33 |
| Other | partial | `hd_base`, `hd_intern`, `hd_diag`, `hd_cache`, `hd_sched`, `hd_fmt`: IDs/wire/diagnostics/cache/scheduling foundations | 3,526 |

Rust-line counts are gross lines in the named crates, not a completion percentage; a crate may support more than one group.

## Remaining Checklists

| Group | Constructs, cases, and diagnostics not yet covered |
| --- | --- |
| Lexer and parser | `InterpolatedStringValue`; numeric suffix families; decorator argument edge cases; `let`/pattern recovery; pipe placeholders; range precedence; postfix `?`; test-case registration; symbol/highlight index; all parser diagnostics and fix spans |
| Resolution and packages | TOML manifest parser; workspace/member rules; version requirements; dependency graph/lock sums; Git/cache fetch path; folder folding; package joins; visibility/re-export loops; folder cycles; orphan/nonlocal impls; private-type leaks; full prelude/std interface bootstrap |
| Checker — calls and inference | contextual inference; call speculation/rollback; named/default/vararg arguments; literal retry/defaulting/suffixes; least-common type; method references; operator calls; callable adapters; `cannot-infer`; ambiguity reporting |
| Checker — types and traits | row subsumption; permission weakening; variance; associated bindings/types; supertrait paths; impl candidate indexing; coherence; dynamic safety; generic patterns; requirement rows/keys; defaults; value categories; derived impl inventory |
| Checker — expressions and statements | every expression form; data/enum construction; control flow; comprehensions; statements; patterns; exhaustiveness; local declaration hoisting; iteration; maps; initialization; entry validation; termination and propagation |
| Checker — suspension and captures | suspending expression checks; capture views; shared captured cells; closure mapping; host capabilities; driver calls; cancellation/defer obligations |
| Checker — templates, facts, and diagnostics | template/tuple instances; typed facts; derivation models; `@error`; embedding; member-line facts; visibility; debug/inspect rewriting; name suggestions; written-type validation; diagnostic codes/fixes/notes |
| Emitter | roots/reachability; instance keys; scalar/data/enum/closure layouts; calls/operators/intrinsics; dictionaries and open instructions; rows/providers; counted loops; comparisons; panic sites; module init; suspension state machines; cancellation; deterministic section ordering |
| Runtime and host | exchange-buffer codec; argument streaming; boundary shape validation; host function table; imports; capability grants; resource limits; panic/backtrace decoding; Node/V8 engine; browser glue; embedding API |
| Test runner | unit/integration discovery; case init/reset; filters; parallel workers; property generation/shrinking/discards/regressions; panic/timeout handling; text/JSON reports; browser runner |
| CLI commands | complete argument grammar/help; package/file modes; check/build/run/test; `new`; add/remove/update; clean/cache; capabilities; profiles; queries; `fmt`/`fix`; doc tests; HTTP/process hosts; stable exits/JSON diagnostics |
| REPL | input classification/continuation indentation; session packages; shadowing; incremental link; journal framing; replay/divergence; terminal loop; refusal/recovery diagnostics |
| Docs | doc-comment extraction; doctest classification; compile-fail expectations; interface-backed pages; links/search; `hd doc` command flow |
| Other | driver orchestration; diagnostic renderers/JSON/fixes; HIR-to-TIR retirement; syntax highlighting; Unicode mixed-script checks; snapshots; spec index; cache persistence/eviction; worker pool; formatter; unsupported-stage accounting |

## Not Needed Or Replaced

| TS source | Lines | Disposition in the new design |
| --- | ---: | --- |
| `src/checker/gadt.ts`, `src/checker/gadt-checker.ts` | 663 | Drop: GADTs are removed; retain only ordinary enum exhaustiveness and trait dictionaries. |
| `src/emitter/link-wat.ts`, `src/wasm.ts`, `src/emitter/runtime/index.ts` | 350 | Replace: no WAT text linker/runtime blob; `wasm-encoder` emits deterministic binary sections. |
| `src/checker/program-lower.ts`, `src/hir.ts` | 1,354 | Replace machinery: checking writes designed TIR directly; use these only as semantic checklists. |
| `src/checker/standard-sources.ts`, prototype-only parts of `standard-bindings.ts` and `standard-library.ts` | 1,926 gross | Replace hard-coded source stubs with embedded std/interface seeds; keep only compiler-known lang/prelude/test items. |
| `src/toolchain-gate.ts` | 78 | Replace the WAT gate with the compiler cache/toolchain key and engine feature check. |
| `src/script-data.ts` | 1,189 | Generate Unicode script tables at build time; do not hand-port the generated table. |
| `src/checker/generated-source.ts` | 174 | Replace synthetic TypeScript source documents with typed generated TIR plus provenance spans. |
| Prototype orchestration in `src/compiler.ts` | 1,488 gross | Replace pass wiring with `hd_driver` tasks/cache keys; semantic boundary and replay cases remain in scope. |

“Gross” rows mix retained semantic cases with replaced machinery and therefore are not subtracted wholesale from the estimate.

## Estimate By Group

| Group | TS checklist lines in scope | New Rust estimate | Agent jobs | Primary risk |
| --- | ---: | ---: | ---: | --- |
| Lexer and parser | 7.9k | 1–3k | 0.5 | diagnostic/recovery parity |
| Resolution and packages | 4.0k | 5–8k | 1–2 | dependency and visibility edge cases |
| Checker | 37.5k | 24–35k | 5–7 | inference/traits plus diagnostic parity |
| Emitter | 8.8k | 12–18k | 2–4 | Wasm GC layouts and suspension |
| Runtime and host | 1.3k | 4–7k | 1 | ABI codecs and resource limits |
| Test runner | 0.7k | 4–6k | 1 | property tests and isolation |
| CLI commands | 6.0k | 8–12k | 2 | package/dependency workflows |
| REPL | 1.2k | 4–7k | 1 | incremental link and replay |
| Docs | 0.3k | 2–4k | 0.5–1 | doctest/source mapping |
| Other foundations | 5.0k | 5–8k | 1–2 | orchestration and diagnostic fidelity |
| **Total** | **72.7k** | **69–108k** | **15–22** | estimate excludes optimization tuning |

## Ordered Jobs

### Phase 1 — Make It Move

| Order | Job | Deliverable | TS checklist files |
| ---: | --- | --- | --- |
| 1 | M4a-1 | Scalar function bodies: names, locals, integers/bools/text, return, direct calls, basic TIR verification | `checker/checker.ts`, `checker/context*.ts`, `checker/expression-literals.ts`, `checker/statements.ts`, `checker/program-*.ts`, `types.ts`, `hir.ts` |
| 2 | M4a-2 | Data and control: constructors/fields, blocks, conditionals, matches, patterns, exhaustiveness | `checker/expression-data.ts`, `checker/expression-control.ts`, `checker/patterns.ts`, `checker/exhaustiveness.ts`, `checker/type-declarations.ts` |
| 3 | M4a-3 | Generics, calls, inference, rows, methods/operators, one trait call | `checker/calls.ts`, `checker/expression-calls.ts`, `checker/expression-operators.ts`, `checker/assignability.ts`, `checker/least-common-type.ts`, `checker/trait-*.ts`, `checker/program-implementations.ts` |
| 4 | M4a-4 | Closures/captures and first suspension form; complete vertical TIR printer/verifier path | `checker/capture-view.ts`, `checker/captured-cells*.ts`, `checker/expression-suspensions.ts`, `checker/host-capabilities.ts`, `checker/program-effects.ts` |
| 5 | M4b | Monomorphize the vertical slice: roots, instance keys, layouts, dictionaries, reachability | `emitter/reachability.ts`, `emitter/module-types.ts`, `emitter/callable-adapters.ts`, `emitter/scalars.ts`, `emitter/intrinsics.ts` |
| 6 | M4c | Emit deterministic Wasm GC and run scalar `println` through V8/Node | `emitter/emitter.ts`, `emitter/context.ts`, `emitter/function-body.ts`, `emitter/shared.ts`, `host-*.ts`, `runtime-interface.ts`, `wasm.ts` |

### Phase 2 — Make It Work

| Order | Job | Deliverable | TS checklist files |
| ---: | --- | --- | --- |
| 1 | P2-1 | Finish expression, statement, pattern, flow, exhaustiveness, and recovery coverage | `checker/expression-*.ts`, `checker/statements.ts`, `checker/patterns.ts`, `checker/exhaustiveness.ts`, `checker/termination.ts` |
| 2 | P2-2 | Finish inference, literals, rows, variance, generics, calls, methods, and diagnostics | `checker/calls.ts`, `checker/literal-*.ts`, `checker/row-rules.ts`, `checker/variance.ts`, `checker/written-type-validation.ts`, `checker/cannot-infer.ts` |
| 3 | P2-3 | Finish trait solving, associated types, coherence, defaults, derivation, and dictionaries | `checker/associated-bindings.ts`, `checker/program-implementations.ts`, `checker/supertrait-bounds.ts`, `checker/trait-*.ts`, `checker/typed-derivation.ts` |
| 4 | P2-4 | Templates, facts, tuples, embedding, init order, visibility, debug/inspect, fixes | `checker/template-instances.ts`, `checker/tuple-templates.ts`, `checker/*facts.ts`, `checker/program-embedding.ts`, `checker/module-initialization.ts`, `checker/debug-*.ts`, `checker/inspectable.ts` |
| 5 | P2-5 | Package discovery, manifests, dependencies, interfaces, privacy, ownership, std bootstrap | `package*.ts`, `manifest.ts`, `dependencies/*.ts`, `checker/module-paths.ts`, `checker/package-ownership.ts`, `checker/standard-*.ts` |
| 6 | P2-6 | Data/enum/closure/string/list/map codegen, module init, comparisons, panics | `emitter/data.ts`, `emitter/value-comparison.ts`, `emitter/iterator.ts`, `emitter/panic-sites.ts`, `emitter/host-providers.ts` |
| 7 | P2-7 | Full suspension state machines, stored suspension, `all!`/`race!`, cancellation/defer | `emitter/suspension.ts`, `emitter/stored-suspension.ts`, `checker/expression-suspensions.ts` |
| 8 | P2-8 | Host ABI, exchange buffer, capabilities, limits, Node/V8 and browser adapters | `host-arguments.ts`, `host-boundary.ts`, `host-functions.ts`, `host-values.ts`, `runtime-*.ts`, `web-host.ts` |
| 9 | P2-9 | Unit/integration/property runner with isolation, shrinking, snapshots, reports | `test-runner.ts`, `property-tests.ts`, `snapshots.ts`, `commands/doc-tests.ts` |
| 10 | P2-10 | CLI/package command surface, dependency edits, queries, hosts, exits, JSON output | `cli*.ts`, `commands/*.ts`, `diagnostic-report.ts` |
| 11 | P2-11 | REPL session, incremental link, journal/replay, terminal UX | `repl*.ts`, relevant replay paths in `compiler.ts` |
| 12 | P2-12 | Documentation, highlighting, spec index, doctests, formatter/fix completion | `doc-tests.ts`, `highlight.ts`, `spec-index.ts`, `unicode-scripts.ts` |

## TypeScript File Inventory

The responsibility column names the file's principal exported surface; private helpers remain part of that row's checklist.

### Lexer and parser

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/ast.ts` | 1,036 | ast: `TEMPLATE_PLACEHOLDER`, `TypeRef`, `GenericBound` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/lexer.ts` | 893 | lexer: `TokenKind`, `InterpolatedStringValue`, `Token` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/numeric.ts` | 94 | numeric: `NumericType`, `NUMERIC_TYPES`, `numericType` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/base.ts` | 838 | base: `ParseFailure`, `ExpressionParseResult`, `ParseOptions` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/decorators.ts` | 439 | generic parameters, bounds, and decorator parsing | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/expression.ts` | 1,275 | expression AST forms and expression parsing | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/index.ts` | 2 | parser public re-exports | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/let.ts` | 255 | `let` forms, bindings, and recovery | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/parser.ts` | 1,490 | parser: `ParseResult`, `parse` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/patterns.ts` | 272 | pattern forms, bindings, and spread validation | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/pipe-steps.ts` | 51 | pipe steps: `collectPipePlaceholders`, `isBarePipeStep` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/range.ts` | 95 | range: `RANGE_PRECEDENCE` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/test-cases.ts` | 495 | test cases: `ModuleItems`, `emptyModuleItems`, `registrationOf` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/parser/type-postfix.ts` | 18 | type postfix: `optionalTypeSuffix` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |
| `src/symbols.ts` | 643 | symbols: `SymbolInfo`, `SourceModule`, `InferredTypes` | `hd_syntax`; Syntax §§4.1–4.6; Data Structures §§3.11–3.14 |

### Resolution and packages

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/dependencies/cache.ts` | 228 | cache: `Variables`, `cacheDirectory`, `CacheEntry` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/dependencies/git.ts` | 197 | git: `GitError`, `maskCredentials`, `listTags` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/dependencies/manifest-edit.ts` | 152 | manifest edit: `setDependency`, `removeDependency`, `addWorkspaceMember` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/dependencies/requirement.ts` | 235 | requirement: `Version`, `parseVersion`, `compareVersions` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/dependencies/resolve.ts` | 689 | resolve: `Place`, `DependencyProblem`, `ResolveOptions` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/dependencies/sum.ts` | 158 | sum: `SUM_FILE`, `SumEntries`, `sumKey` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/manifest.ts` | 633 | manifest: `TomlValue`, `TomlTable`, `ManifestError` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/package-folders.ts` | 133 | package folders: `FolderModule`, `FolderUse`, `FolderEdge` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/package-join.ts` | 133 | package join: `isStandardUse`, `standardForeign`, `standardSpellings` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |
| `src/package.ts` | 1,443 | package: `SOURCE_ROOT`, `TEST_ROOT`, `TASK_ROOT` | `hd_project`, `hd_resolve`; Resolution §§4.7–4.12 |

### Checker — core and program

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/checker/checker-trial-state.ts` | 40 | checker trial state: `snapshotCheckerState` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/checker.ts` | 740 | checker: `FunctionChecker` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/context-types.ts` | 96 | context types: `CheckResult`, `Signature`, `InherentMethod` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/context.ts` | 1,499 | context: `CheckFailure`, `TEST_CASE_FUNCTIONS` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/index.ts` | 3 | checker public re-exports | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-context.ts` | 61 | program context: `ImplementationMethodPreparation`, `ImplementationPreparation`, `ProgramCheckContext` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-declarations.ts` | 587 | program declarations: `createProgramDeclarations` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-inference.ts` | 309 | program inference: `SignatureInference` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-lower.ts` | 284 | program lower: `lowerCheckedProgram` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-signatures.ts` | 497 | program signatures: `createProgramSignatures` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-types.ts` | 1,361 | program types: `declareProgramTypes`, `defineProgramData`, `defineProgramEnums` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program-validation.ts` | 205 | program validation: `validateProgram` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/program.ts` | 367 | program: `CheckOptions`, `check` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |
| `src/checker/shared.ts` | 1,477 | shared: `BindingExpressionFlow`, `iterableInfo`, `bindingExpressionFlow` | `hd_check`, `hd_tir`; Checking §§4.13–4.15 |

### Checker — calls and inference

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/checker/ambiguous-solutions.ts` | 37 | ambiguous solutions: `markAmbiguous`, `carryAmbiguous`, `ambiguousAmong` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/bound-inference.ts` | 224 | bound inference: `inferTypesThroughBounds` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/call-speculation.ts` | 147 | call speculation: `TRIAL_STATE`, `TrialSnapshot`, `TrialParticipant` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/calls.ts` | 1,490 | call signatures, argument planning, inference, and overload choice | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/cannot-infer.ts` | 160 | cannot infer: `InferredBinding`, `unresolvedTypeMessage`, `unresolvedEmptyListMessage` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/expression-calls.ts` | 1,498 | expression calls: `MemberCallExpression` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/expression-literals.ts` | 576 | expression literals: `integerLiteralTarget`, `integerTarget`, `floatLiteralTarget` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/expression-operators.ts` | 1,304 | unary/binary operators, rewrites, and diagnostics | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/least-common-type.ts` | 296 | least common type: `leastCommonType`, `rowUnionType` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/literal-arguments.ts` | 90 | literal arguments: `checkLiteralArgumentsLast` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/literal-join.ts` | 523 | literal join: `DEFAULTED_SPANS`, `markDefaultedLiteral`, `isDefaultedLiteral` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/literal-retry.ts` | 225 | literal retry: `ExpressionLiterals`, `expressionLiterals`, `hasExpressionLiterals` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/literal-suffixes.ts` | 67 | literal suffixes: `checkLiteralSuffixCall`, `checkStringPrefixCall` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/method-references.ts` | 574 | qualified and bound method reference resolution | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/operator-calls.ts` | 326 | operator-to-trait call checking | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |
| `src/checker/partial-expected.ts` | 36 | partial expected: `calleeOf`, `keepSolvedPositions` | `hd_check`, `hd_types`; Type Checking §§2–5, 8–10 |

### Checker — types and traits

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/checker/assignability.ts` | 222 | assignability: `functionVariancePairs`, `isRowSubsumption`, `isPermissionWeakening` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/associated-bindings.ts` | 188 | associated bindings: `associatedNames`, `bindingNameProblem`, `traitKeyParts` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/built-in-methods.ts` | 11 | built in methods: `BUILT_IN_METHODS`, `builtInMethodNames` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/dynamic-safety.ts` | 33 | dynamic safety: `traitIsDynamicallySafe` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/function-types.ts` | 178 | function types: `withFunctionTypeConstructors` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/gadt-checker.ts` | 282 | removed GADT arm refinements and equalities | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/gadt.ts` | 381 | gadt: `mentionsParameter`, `declarationArguments`, `variantShape` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/generic-patterns.ts` | 125 | generic patterns: `matchGenericTypePattern` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/generic-type.ts` | 42 | generic type: `containsGenericType` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/impl-parameters.ts` | 52 | impl parameters: `unconstrainedImplementationParameters` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/implementation-index.ts` | 41 | implementation index: `NOMINAL_HEAD`, `implementationsFor` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/intrinsic-dictionaries.ts` | 62 | intrinsic dictionaries: `intrinsicDictionaryPlan` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/known-types.ts` | 124 | known types: `isKnownType` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/numeric-family.ts` | 20 | numeric family: `familyHolds` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/program-implementations.ts` | 1,423 | program implementations: `implementationVisibleFrom`, `prepareImplementations` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/requirement-keys.ts` | 174 | requirement keys: `resolveRequirementKeyTypes`, `requirementKeyDiagnostics`, `requirementKeyDiagnosticsInType` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/requirement-rows.ts` | 16 | requirement rows: `rowParameterName`, `normalizedRequirements`, `sameRequirements` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/row-rules.ts` | 220 | row rules: `mismatchMessage`, `rowRuleDiagnostics`, `rowDiagnostic` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/supertrait-bounds.ts` | 172 | supertrait bounds: `builtInSupertraitHolds`, `boundsHold`, `implementationCategoryParameters` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/trait-calls.ts` | 455 | trait calls: `QualifiedCallExpression`, `QualifiedTrait` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/trait-paths.ts` | 142 | trait paths: `BoundProof`, `enclosingBoundProof`, `findSupertraitPath` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/type-defaults.ts` | 549 | type defaults: `withTypeDefaults`, `defaultBoundDiagnostics` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/type-parameter-names.ts` | 229 | type parameter names: `typeParameterRedeclarations` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/value-categories.ts` | 89 | value categories: `ValueCategory`, `traitImpliesValueCategory`, `typeSatisfiesValueCategory` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/variance.ts` | 338 | variance: `Declarations`, `conversionVariances`, `varianceDiagnostics` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |
| `src/checker/written-type-validation.ts` | 392 | written type validation: `traitTypeName`, `dynamicTraitProblemInType`, `restElementProblem` | `hd_types`, `hd_check`; Type Checking §§3–7; Trait Solver §§1–15 |

### Checker — expressions and statements

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/checker/exhaustiveness.ts` | 275 | exhaustiveness: `IntegerInterval`, `integerPatternInterval`, `patternsExhaustive` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/expression-comprehensions.ts` | 394 | expression comprehensions: `FOR_PATTERN_ITEM` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/expression-control.ts` | 1,095 | blocks, conditionals, matches, loops, and propagation | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/expression-data.ts` | 904 | records, enums, tuples, member access, and updates | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/iteration.ts` | 156 | iterable resolution and loop item typing | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/local-declarations.ts` | 227 | local declarations: `hoistLocalDeclarations` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/map-keys.ts` | 93 | map keys: `mapKeyKind`, `implementsTrait`, `registerHashableKeyTypes` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/patterns.ts` | 974 | pattern typing, bindings, reachability, and narrowing | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/statements.ts` | 1,046 | statements: `STATEMENT_IFS` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/termination.ts` | 158 | termination: `InferredPropagation`, `mismatchedPropagation`, `resultFailure` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |
| `src/checker/type-declarations.ts` | 670 | type declarations: `NEWTYPE_FIELD`, `words`, `substitute` | `hd_check`, `hd_tir`; Type Checking §2; Checking §4.13 |

### Checker — captures and suspension

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/checker/capture-view.ts` | 103 | capture view: `captureSources`, `isCaptureSource` | `hd_check`, `hd_tir`; Type Checking §§2.6, 12; Suspension §14 |
| `src/checker/captured-cells-walk.ts` | 496 | captured cells walk: `CaptureCellMapper`, `mapCapturedFunction` | `hd_check`, `hd_tir`; Type Checking §§2.6, 12; Suspension §14 |
| `src/checker/captured-cells.ts` | 125 | captured cells: `cellType`, `cellInner`, `shareCapturedLocals` | `hd_check`, `hd_tir`; Type Checking §§2.6, 12; Suspension §14 |
| `src/checker/expression-suspensions.ts` | 364 | suspending calls, result flow, and child checks | `hd_check`, `hd_tir`; Type Checking §§2.6, 12; Suspension §14 |
| `src/checker/host-capabilities.ts` | 181 | host capabilities: `validateHostCapabilities` | `hd_check`, `hd_tir`; Type Checking §§2.6, 12; Suspension §14 |
| `src/checker/program-effects.ts` | 89 | program effects: `findDriverCall`, `driverStartingFunctionNames`, `deferredDriverCalls` | `hd_check`, `hd_tir`; Type Checking §§2.6, 12; Suspension §14 |

### Checker — facts, templates, std, and diagnostics

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/checker/debug-print-calls.ts` | 194 | debug-call discovery and printable-site planning | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/debug-print.ts` | 644 | debug print: `DBG_INTRINSIC`, `DBG_TEXT_INTRINSIC`, `DebugPrinter` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/declaration-facts.ts` | 50 | declaration facts: `checkDuplicateDeclarationFacts` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/decorators.ts` | 283 | decorators: `withBareMarkerCalls`, `markerFunctions`, `withSuffixMarkers` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/derivation-models.ts` | 114 | derivation models: `MemberModel`, `VariantModel`, `effectiveFacts` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/derive-aliases.ts` | 39 | derive aliases: `nullaryTypeAliases`, `expandTypeAlias`, `deriveMissing` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/derive-intrinsics.ts` | 213 | derive intrinsics: `DerivedOrigins`, `registerDerivedOrigins`, `isDerivedImplementation` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/display-names.ts` | 52 | display names: `displayName` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/entry-error.ts` | 98 | entry error: `ENTRY_ERROR_REPORT`, `STD_ENTRY_REPORT`, `renderedEntry` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/error-derivation.ts` | 406 | error derivation: `withErrorDerivation` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/error-generation.ts` | 249 | error generation: `ErrorMember`, `ErrorCase`, `ErrorType` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/expression-inspect.ts` | 498 | expression inspection plans and downcast handling | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/fact-patterns.ts` | 45 | fact patterns: `matchFactPattern` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/function-facts.ts` | 79 | function facts: `FACTS_OF_INTRINSIC`, `STRUCTURE_FACT`, `factsOfBuilderName` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/generated-source.ts` | 174 | generated source: `Source_`, `ZERO_SPAN` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/import-bindings.ts` | 41 | import bindings: `ImportBindingMap` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/inspectable.ts` | 247 | inspectable: `InspectEnvironment`, `HANDLE_TYPE`, `usesStandardInspect` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/member-lines.ts` | 366 | member lines: `Target`, `isSpreadFact`, `lineFacts` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/member-lookup.ts` | 609 | member lookup: `traitDefaultDeclarations` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/member-visibility.ts` | 123 | member visibility and ownership checks | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/module-initialization.ts` | 343 | module initialization: `checkModuleInitialization` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/module-paths.ts` | 177 | module paths: `withModulePaths` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/name-suggestions.ts` | 106 | name suggestions: `closestNames`, `didYouMean`, `similarMethods` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/package-ownership.ts` | 79 | package ownership: `ROOT_PACKAGE`, `PackageOwnership`, `packageOwnership` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/prelude-names.ts` | 116 | prelude names: `PRELUDE_ORIGINS`, `PRELUDE_NAMES` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/program-embedding.ts` | 260 | program embedding: `checkEmbeddedMemberConflicts`, `checkEmbeddingLimits` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/self-ref.ts` | 143 | self ref: `SelfRef`, `SelfRefScope`, `selfRefScope` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/standard-bindings.ts` | 875 | standard bindings: `FOREIGN`, `patternBindings`, `renameStandardBindings` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/standard-library.ts` | 965 | standard library: `standardSubmoduleFunctionIdentity`, `CHECK_EQUAL`, `standardDeclarationNames` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/standard-provenance.ts` | 70 | standard provenance: `withStandardSource` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/standard-sources.ts` | 86 | standard sources: `STANDARD_MODULES`, `StandardModule`, `standardDocument` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/standard-traits.ts` | 109 | standard traits: `STANDARD_FROM`, `TUPLE_TRAIT`, `ALL_COMBINATOR` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/standard-uses.ts` | 183 | standard uses: `isStandardModulePath`, `declares`, `standardExporters` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/template-instances.ts` | 807 | template instances: `STRUCTURE`, `TUPLE_REST`, `isTypeRef` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/test-tier-notes.ts` | 119 | test tier notes: `TestTierNames`, `withTestTierNotes`, `testTierNames` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/tuple-templates.ts` | 415 | tuple templates: `TupleInstance`, `tupleShapesInJoinedProgram`, `localTupleName` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/typed-derivation.ts` | 1,470 | typed derivation: `STRUCTURE_MISMATCH`, `STRUCTURE_AS_DECLARED`, `STRUCTURE_WITNESS` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |
| `src/checker/typed-facts.ts` | 402 | typed facts: `withTypedFacts` | `hd_check`, `hd_resolve`, `hd_diag`; Checking §§4.13–4.14; Codegen §12.3 |

### Emitter

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/emitter/callable-adapters.ts` | 294 | callable adapters: `CALLABLE_STORAGE_TYPES` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/context.ts` | 875 | context: `CleanupFrame`, `EmittedArguments`, `EmitterContext` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/data.ts` | 140 | data construction, field storage, and access emission | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/emitter.ts` | 1,306 | emitter: `EmitOptions`, `emitWat` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/function-body.ts` | 1,450 | statement/expression body emission and cleanup frames | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/host-providers.ts` | 877 | host providers: `emitHostProviders` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/index.ts` | 3 | emitter public re-exports | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/intrinsics.ts` | 178 | intrinsics: `isRuntimePrimitive`, `emitIntrinsicBody`, `emitHostFunctionImports` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/iterator.ts` | 298 | iterator sources, loop temporaries, and element emission | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/link-wat.ts` | 153 | link wat: `linkWat` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/module-types.ts` | 158 | module types: `collectModuleTypes` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/panic-sites.ts` | 140 | panic sites: `OpenSite`, `withoutSiteLines`, `PanicSiteTable` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/reachability.ts` | 292 | reachability: `traitMethodKey`, `calledTraitMethods`, `emissionReachability` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/runtime/index.ts` | 9 | index: `RUNTIME_WAT`, `MAP_RUNTIME_WAT`, `BOUNDARY_RUNTIME_WAT` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/scalars.ts` | 46 | scalars: `scalarWasm`, `boxScalar`, `voidThen` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/shared.ts` | 124 | shared: `indent`, `andThen`, `matchTestTag` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/sized-numeric.ts` | 263 | sized numeric: `SizedNumericContext`, `isSizedNumeric`, `integerConstant` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/stored-suspension.ts` | 262 | stored suspension: `STORED_SUSPENSION_TYPES`, `STORED_SUSPENSION_RUNTIME`, `storedSuspensionAdapterReferences` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/suspension.ts` | 1,287 | suspension: `SuspensionOperation`, `SuspensionTerminator`, `SuspensionPlan` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/emitter/value-comparison.ts` | 789 | structural equality/order emission and nested values | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |
| `src/wasm.ts` | 188 | wasm: `WasmArtifact`, `SiteMap`, `assembleWat` | `hd_mono`, `hd_wasm`; Codegen §§11–13; Suspension §14; Wasm Layout §15 |

### Runtime and host

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/host-arguments.ts` | 170 | host arguments: `ArgumentBuffers`, `argumentBuffers`, `streamedArgumentImports` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |
| `src/host-boundary.ts` | 281 | host boundary: `payloadlessSingletonEnum`, `isBoundaryScalar`, `boundaryShape` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |
| `src/host-functions.ts` | 290 | host functions: `HostFunction`, `HOST_FUNCTIONS`, `HOST_PROVIDERS` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |
| `src/host-values.ts` | 104 | host values: `hostArgumentValue`, `checkedHostValue` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |
| `src/runtime-interface.ts` | 256 | runtime interface: `FunctionIdentity`, `HostMethodInterface`, `HostTraitInterface` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |
| `src/runtime-panic.ts` | 152 | runtime panic: `RUNTIME_PANIC_NAMES`, `RuntimePanicName`, `FallbackBinding` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |
| `src/web-host.ts` | 75 | web host: `WEB_HOST_TRAITS`, `wait`, `WEB_HOST_ANSWERS` | `hd_host_abi`, `hd_run`, `hd_run_wasmtime`, `hd_web`; Runtime §§16–18 |

### Test runner

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/property-tests.ts` | 274 | property tests: `PropertyDiscard`, `RegressionStore`, `CaseResult` | `hd_run`, `hd_testkit`; Engines/Test Runner §19 |
| `src/test-runner.ts` | 390 | test runner: `RunnableFunction`, `TestReporting`, `TempDirs` | `hd_run`, `hd_testkit`; Engines/Test Runner §19 |

### CLI commands

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/cli-args.ts` | 601 | cli args: `UsageError`, `ParsedCommand`, `overviewHelp` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/cli.ts` | 195 | cli: `main` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/capabilities.ts` | 280 | capabilities: `Grant`, `CapabilityGrants`, `UNLIMITED` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/clean.ts` | 80 | clean: `CleanArgs`, `cleanCommand` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/compile.ts` | 360 | compile: `parseCommand`, `CheckArgs`, `checkCommand` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/default-profile.ts` | 366 | default profile: `DefaultProfileHost`, `DEFAULT_PROFILE_TRAITS`, `defaultProfileAnswer` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/dependencies.ts` | 442 | dependencies: `withDependencies`, `DependencyArgs`, `AddArgs` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/doc-tests.ts` | 292 | doc tests: `TestTally`, `DocTestModule`, `moduleDocTests` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/execute.ts` | 1,099 | execute: `FileArgs`, `fileCommand`, `ModuleArgs` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/help.ts` | 110 | help: `HelpArgs`, `helpCommand`, `replCommand` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/http-host.ts` | 188 | http host: `sendRequest` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/index.ts` | 59 | command dispatch contracts and runner options | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/io.ts` | 169 | io: `CommandIo`, `processIo`, `EXIT_HD_FAILURE` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/new.ts` | 201 | new: `PackageKind`, `NewArgs`, `newCommand` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/package-mode.ts` | 337 | package mode: `MANIFEST_FILE`, `BUILD_DIRECTORY`, `Executable` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/processes.ts` | 128 | processes: `coversProgram`, `PROCESS_NOT_GRANTED`, `runHostProgram` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/profiles.ts` | 144 | profiles: `RUNTIME_PROFILE_NAMES`, `RUNTIME_SCENARIO_NAMES`, `RuntimeScenario` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/queries.ts` | 268 | queries: `ExplainArgs`, `explainCommand`, `LookupArgs` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/source.ts` | 673 | source: `RuntimeProfileName`, `TestLayout`, `PackageTree` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |
| `src/commands/test-host.ts` | 45 | test host: `TEST_TEMP_DIRS`, `integrationTestHost` | `hd_cli`, `hd_driver`, `hd_stdpack`; Commands §§7, 20 |

### REPL

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/repl-input.ts` | 233 | repl input: `InputKind`, `classifyInput`, `INDENT_UNIT` | `hd_cli`, `hd_run`; Live Execution; Commands §20.5 |
| `src/repl-terminal.ts` | 179 | repl terminal: `ReplIo`, `runRepl` | `hd_cli`, `hd_run`; Live Execution; Commands §20.5 |
| `src/repl.ts` | 749 | repl: `ReplPackage`, `ReplHost`, `ReplRefusal` | `hd_cli`, `hd_run`; Live Execution; Commands §20.5 |

### Docs

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/doc-tests.ts` | 270 | doc tests: `DocTest`, `docTests`, `filePosition` | `hd_doc`; Commands §7.7 |

### Other

| TS file | Lines | Responsibility | Rust/design owner |
| --- | ---: | --- | --- |
| `src/compiler.ts` | 1,488 | compiler: `Compilation`, `CompileOptions`, `HostArgumentValue` | `hd_driver`, `hd_cache`, `hd_sched`; Overview §§1.1–1.4 |
| `src/diagnostic-report.ts` | 402 | diagnostic report: `OutputFormat`, `JsonSpan`, `JsonFix` | `hd_diag`, `hd_driver`; Data Structures §3.8; Checking §4.14 |
| `src/diagnostics.ts` | 153 | diagnostics: `SourceDocument`, `SourceOrigin`, `SOURCE_ORIGIN` | `hd_diag`; Data Structures §§3.7–3.8 |
| `src/highlight.ts` | 251 | highlight: `TokenClass`, `classify`, `highlight` | `hd_fmt`, `hd_syntax`; Syntax §§4.1–4.4 |
| `src/hir.ts` | 1,070 | hir: `ValueType`, `HirTypeSubstitution`, `HirDataField` | `hd_tir`; Data Structures §§3.10, 3.18 |
| `src/panic-locator.ts` | 110 | panic locator: `panicLocator`, `stackExhaustionPanics` | `hd_run`, `hd_wasm`; Wasm Layout §15.5 |
| `src/script-data.ts` | 1,189 | script data: `SCRIPT_RANGES`, `SCRIPT_AUGMENT_OVERRIDES`, `SCRIPT_NAMES` | `hd_syntax`; Syntax §4.1 |
| `src/snapshots.ts` | 91 | snapshots: `snapshotModule`, `snapshotRun`, `regressionStore` | `hd_testkit`, `hd_run`; Test Runner §§19.3–19.5 |
| `src/spec-index.ts` | 348 | spec index: `SpecMention`, `SpecIndex`, `namedCodes` | `hd_doc`, `hd_diag`; Commands §7.7 |
| `src/toolchain-gate.ts` | 78 | toolchain gate: `TOOLCHAIN_GATE_WAT`, `buildToolchainGate`, `runToolchainGate` | `hd_driver`, `hd_run`; Cache §5.3; Engines §18 |
| `src/types.ts` | 751 | types: `CURSOR_TYPE`, `ResultParts`, `FunctionParts` | `hd_types`, `hd_tir`; Data Structures §§3.4, 3.18 |
| `src/unicode-scripts.ts` | 142 | unicode scripts: `mixedScriptWarning` | `hd_syntax`, `hd_diag`; Syntax §4.1 |
