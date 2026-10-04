# Job 5 audit: dead code in src/

> Posted in chat 2026-10-04 during the read-only audit session; logged here verbatim by owner request. HEAD c38d0d3f. Body below is the posted report, unedited.

Job 5 done. No repo files changed. HEAD `c38d0d3f`, all line numbers below are at that commit.

## Method

- Exports: all `export function/const/class/interface/type/enum` in `src/` (711 total) cross-checked against every `import`, `export…from`, and `import()` in `src/`, `test/`, `bin/`, `website/`, with specifiers resolved (`./x`, `../x`, index files) and `export *` / `export type {…} from` / `import {a as b}` / `import * as ns` + `ns.name` chains traced. Each of the 37 survivors was re-verified with a direct grep, and also searched in `spec/`, `lib/`, `.agents/`, `website/assets/` (no users anywhere).
- Files: every `src/` file is imported by something in scope — none unimported. (`test/*.test.ts` entries are out of scope for this question, which asks about `src/`.)
- Long functions: `function_declaration`, `method_definition`, `arrow_function`, `function_expression` spans via ast-grep, kept when over 150 lines.

## Dead exports (37): symbol | location | evidence

Evidence shorthand: `0 imports` = no import/export-from/dynamic-import in scope binds it (out-of-scope dirs also clean).

| symbol | location | evidence |
| --- | --- | --- |
| `factsOfName` | `src/checker/function-facts.ts:35` | 0 imports; used at :31, :54 same file |
| `compactKey` | `src/checker/inspectable.ts:199` | 0 imports; used at :178 same file |
| `TypeDeclarations` | `src/checker/least-common-type.ts:13` | 0 imports; same-file annotations only |
| `LeastCommonType` | `src/checker/least-common-type.ts:18` | 0 imports; same-file annotations only |
| `containsGenericParameter` | `src/checker/shared.ts:617` | 0 imports; recursive self-calls only |
| `CompilerModule` | `src/checker/standard-sources.ts:56` | 0 imports; used at :62, :71, :77 same file |
| `TYPE_KEYS` | `src/checker/template-instances.ts:28` | 0 imports (`local-declarations.ts:27`, `type-defaults.ts:90` declare their own) |
| `TYPE_LIST_KEYS` | `src/checker/template-instances.ts:29` | 0 imports (separate copies in two other files) |
| `renameTypes` | `src/checker/template-instances.ts:47` | 0 imports |
| `localType` | `src/checker/template-instances.ts:100` | 0 imports (`standard-bindings.test.ts:117` is a test-local) |
| `protocolError` | `src/checker/template-instances.ts:132` | 0 imports |
| `TemplateSite` | `src/checker/template-instances.ts:160` | 0 imports |
| `TEMPLATE_SELF` | `src/checker/template-instances.ts:188` | 0 imports |
| `InstanceInput` | `src/checker/template-instances.ts:375` | 0 imports |
| `forwardingAllowed` | `src/checker/template-instances.ts:537` | 0 imports |
| `newtypeHelper` | `src/checker/template-instances.ts:553` | 0 imports |
| `TEST_FUNCTION` | `src/checker/termination.ts:10` | 0 imports |
| `terminationTraitName` | `src/checker/termination.ts:42` | 0 imports |
| `runnableEntryResult` | `src/checker/termination.ts:53` | 0 imports |
| `terminates` | `src/checker/termination.ts:70` | 0 imports |
| `TupleShape` | `src/checker/tuple-templates.ts:33` | 0 imports; 27 same-file mentions only |
| `tupleShapes` | `src/checker/tuple-templates.ts:54` | 0 imports; same file only |
| `tupleTarget` | `src/checker/tuple-templates.ts:112` | 0 imports; same file only |
| `elementBound` | `src/checker/tuple-templates.ts:197` | 0 imports; same file only |
| `restBound` | `src/checker/tuple-templates.ts:213` | 0 imports; same file only |
| `implementsRest` | `src/checker/tuple-templates.ts:247` | 0 imports; same file only |
| `ValueCategoryEnvironment` | `src/checker/value-categories.ts:34` | 0 imports; used at :45 same file |
| `FlagSpec` | `src/cli-args.ts:6` | 0 imports; used at :31, :54+ same file |
| `CommandSpec` | `src/cli-args.ts:23` | 0 imports; used at :94, :246+ same file |
| `COMMANDS` | `src/cli-args.ts:94` | 0 imports; used at :253 same file |
| `WatCompilation` | `src/compiler.ts:29` | 0 imports |
| `InstantiateOptions` | `src/compiler.ts:228` | 0 imports |
| `CallableStorageAdapter` | `src/emitter/context.ts:81` | 0 imports; used at :114, :329, :645 same file |
| `EmissionReachability` | `src/emitter/reachability.ts:77` | 0 imports; used at :82 same file |
| `PayloadlessEnumValue` | `src/host-boundary.ts:5` | 0 imports; used at :17 same file |
| `BoundaryShape` | `src/host-boundary.ts:48` | 0 imports; used at :64 same file |
| `GenericKinds` | `src/parser/base.ts:17` | 0 imports; used at :62, :324+ same file |

Whole-module clusters: `template-instances.ts` (9 dead), `tuple-templates.ts` (6), `termination.ts` (4). Three early candidates (`CheckResult`, `CheckOptions`, plus re-export chains) were excluded after tracing `export type {…} from` barrels.

## Files no other file imports: none

All 153 `src/` modules have at least one in-scope importer (including side-effect and dynamic imports).

## Functions over 150 lines (53): function | span

| function | span |
| --- | --- |
| `inferTypesThroughBounds` | `src/checker/bound-inference.ts:25-219` (195) |
| `checkSignatureArguments` | `src/checker/calls.ts:793-1018` (226) |
| `mapExpression` | `src/checker/captured-cells-walk.ts:135-406` (272) |
| `checkClosureExpression` | `src/checker/checker.ts:323-583` (261) |
| `coerce` | `src/checker/context.ts:452-633` (182) |
| `withErrorDerivation` | `src/checker/error-derivation.ts:84-354` (271) |
| `checkBuiltInMemberCall` | `src/checker/expression-calls.ts:185-354` (170) |
| `checkDynamicMemberCall` | `src/checker/expression-calls.ts:356-610` (255) |
| `checkImplementedMemberCall` | `src/checker/expression-calls.ts:612-882` (271) |
| `callCandidate` callback | `src/checker/expression-calls.ts:662-812` (151) |
| `checkNamedIntrinsicCall` | `src/checker/expression-calls.ts:930-1165` (236) |
| `checkQualifiedCall` | `src/checker/expression-calls.ts:1311-1494` (184) |
| `checkControlExpression` | `src/checker/expression-control.ts:86-263` (178) |
| `checkMatchArm` | `src/checker/expression-control.ts:583-879` (297) |
| `checkDataExpression` | `src/checker/expression-data.ts:81-385` (305) |
| `checkAccessExpression` | `src/checker/expression-data.ts:635-900` (266) |
| `checkLiteralExpression` | `src/checker/expression-literals.ts:207-413` (207) |
| `checkOperatorExpression` | `src/checker/expression-operators.ts:184-491` (308) |
| `checkSuspendingCallExpression` | `src/checker/expression-suspensions.ts:89-333` (245) |
| `hoistLocalDeclarations` | `src/checker/local-declarations.ts:54-226` (173) |
| `checkNestedPattern` | `src/checker/patterns.ts:425-671` (247) |
| `createEnumVariantDeclarations` | `src/checker/program-declarations.ts:160-369` (210) |
| `prepareInherentImplementation` | `src/checker/program-implementations.ts:428-579` (152) |
| `prepareImplementations` | `src/checker/program-implementations.ts:764-1074` (311) |
| `lowerCheckedProgram` | `src/checker/program-lower.ts:68-252` (185) |
| `createProgramSignatures` | `src/checker/program-signatures.ts:109-419` (311) |
| per-declaration closure | `src/checker/program-signatures.ts:115-417` (303) |
| `defineProgramTraits` | `src/checker/program-types.ts:409-679` (271) |
| `validateDeclaredTypes` | `src/checker/program-types.ts:714-876` (163) |
| `validateProgram` | `src/checker/program-validation.ts:14-166` (153) |
| `inferGenericType` | `src/checker/shared.ts:784-939` (156) |
| `expression` | `src/checker/standard-bindings.ts:385-550` (166) |
| `checkStatement` | `src/checker/statements.ts:71-353` (283) |
| `checkBindingStatement` | `src/checker/statements.ts:639-792` (154) |
| `compileTemplate` | `src/checker/template-instances.ts:190-372` (183) |
| `withTypeDeclarations` | `src/checker/type-declarations.ts:510-681` (172) |
| `withTypedDerivation` | `src/checker/typed-derivation.ts:337-644` (308) |
| `generateDerivation` | `src/checker/typed-derivation.ts:1082-1412` (331) |
| `withTypedFacts` | `src/checker/typed-facts.ts:152-399` (248) |
| `instantiate` | `src/compiler.ts:1031-1334` (304) |
| `emitCfgSuspensionSupport` | `src/emitter/emitter.ts:300-514` (215) |
| `emitReachableWat` | `src/emitter/emitter.ts:954-1230` (277) |
| `emitValueExpression` | `src/emitter/function-body.ts:176-424` (249) |
| `emitCallExpression` | `src/emitter/function-body.ts:470-762` (293) |
| `emissionReachability` | `src/emitter/reachability.ts:82-256` (175) |
| `lowerExpression` | `src/emitter/suspension.ts:421-707` (287) |
| `scanQuoted` | `src/lexer.ts:520-688` (169) |
| `linkPackage` | `src/package.ts:198-529` (332) |
| `parseExpression` | `src/parser/expression.ts:77-298` (222) |
| `parsePrefix` | `src/parser/expression.ts:429-634` (206) |
| `parsePattern` | `src/parser/expression.ts:1275-1442` (168) |
| `parseType` | `src/parser/parser.ts:1003-1171` (169) |
| `parseStatement` | `src/parser/parser.ts:1222-1386` (165) |

No rewrites proposed. Ready for Job 6.