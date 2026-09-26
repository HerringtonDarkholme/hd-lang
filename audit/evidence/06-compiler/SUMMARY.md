# Phase 6: Compiler Architecture

Commit `bd985d7`, 2026-09-25. The worker could not write report files. The
coordinator wrote this file from the worker's final report. The evidence
files it cites are in this directory.

`hd` means `node --experimental-strip-types bin/hd.js`. Scripts are in
`audit/scripts/compiler/` and pass `npx oxlint audit/scripts/compiler`.

## Headline

- **HIR:** fully typed and resolved. The emitter never imports the AST or
  the checker. But types are strings: the emitter re-parses them at about 60
  sites and looks up data and enum types by bare name.
- **Folder boundaries:** 0 imports of `parser/`, `checker/`, or `emitter/`
  internals from outside the folder.
- **Rows:** inference is local to closures, not a whole-program fixpoint.
  Named functions must declare rows. Changing one leaf's row in a
  31-function tree gave 1 error and needed 4 hand edits up the chain.
- **Incremental builds:** there is no cache. A one-line edit costs the same as
  a cold build (ratio 0.93 to 1.25). Binaryen assembly takes 85 to 90% of
  compile time (6.6 s for 34k lines). Inserting one function changes 308 of
  628 WAT functions.
- **Error recovery:** the parser reports 1 of 3 errors and the checker 5 of 6
  (one per function). A signature error hides every body error.
- **Code health:** 13 functions are at 250 to 299 lines, against a 300 cap.
  Duplication is 9.8% overall and 31.5% within `emitter.ts`.

## Commands and evidence

| Step                  | Command                                                     | Evidence                                   |
| --------------------- | ----------------------------------------------------------- | ------------------------------------------ |
| structure             | `ast-grep outline` on core files                           | read only                                  |
| HIR shape             | `hd dump-hir` on `desugar.hd`, `varargs-named.hd`; `hir-shape.ts` | `desugar-hir.md`                       |
| rows                  | `hd check`, `hd explain-requirements` on row probes         | `row-omission.md`, `explain-requirements.md` |
| row ripple            | `row-ripple.ts audit/probes/compiler/row-ripple`            | `row-ripple.md`                            |
| error recovery        | `hd check probes/compiler/errors-*.hd`                      | `error-recovery.md`                        |
| import graph          | `import-graph.ts src`                                       | `module-structure.md`                      |
| function lengths      | `npx oxlint -c audit/scripts/compiler/oxlint-fnlen.json -f unix src` | `function-lengths.md`             |
| duplication           | `duplication.ts 6`                                          | `duplication.md`                           |
| scale and edit timing | `scale.ts <dir> 250,500,1000,2000`                          | `scale.md`                                 |
| WAT stability         | `wat-stability.ts probes/compiler/scale-100.hd`             | `wat-stability.md`                         |
| closure nesting       | `nested-closures.ts <dir> 20`                               | `nested-closures.md`                       |
| module initialization | `init-dag.ts`; `hd check` and `hd run` on `init-*.hd`       | `init-dag.md`, `module-initialization.md`  |
| interpolation spans   | `hd check interp-span*.hd`                                  | `interpolation-span.md`                    |
| replay identity       | `hd record` and `hd replay` on 3 formatting variants        | `replay-identity.md`                       |
| frame liveness        | `hd build --wat frame-liveness.hd`                          | `frame-liveness.md`                        |

## 6.1 Pipeline and HIR

### Stages

| Stage    | Module      | Input to output    | Notes                                                                                                 |
| -------- | ----------- | ------------------ | ----------------------------------------------------------------------------------------------------- |
| lex      | `lexer.ts`  | source to tokens   | parsing stops if the lexer reports anything (`parser.ts:1217`)                                         |
| parse    | `parser/`   | tokens to AST      | interpolation fragments are re-lexed as separate sources (`parser.ts:136-140`), losing spans (F-606); trailing blocks become a closure argument (`parser.ts:1150`) |
| check    | `checker/`  | AST to `HirProgram` | serial passes (`program.ts:21-45`): validation, types, data, enums, traits, host capabilities, implementations, declarations, signatures, bodies, module initialization |
| emit     | `emitter/`  | HIR to WAT text    | read-only prepass `collectModuleTypes` (`emitter.ts:1093`), then string emission per function          |
| assemble | `wasm.ts`   | WAT to bytes       | Binaryen `parseText` and validation                                                                   |

### Typed HIR

The HIR is fully typed and resolved:

- every node has a type;
- calls carry the function index, explicit provider expressions, bound
  dictionaries, and erased types;
- the emitter imports only `hir.ts`, `types.ts`, and `runtime-panic.ts`, so
  it never sees the AST and never infers.

The weakness is that `ValueType = string` (`hir.ts:3`), for example
`fn!(string)->Result[User?,DbError]$Database`. The emitter re-parses these
strings and resolves data and enum types through `dataByName` and
`enumByName` (`emitter/context.ts:190-200`). Export and test-export naming
depends on a `$` name prefix and a name regex (`emitter.ts:105`,
`emitter/shared.ts:16-19`), not on the HIR `synthetic` flag (F-607).

### Desugaring location

| Sugar               | Lowered in                                                                | Still in HIR                     |
| ------------------- | ------------------------------------------------------------------------- | -------------------------------- |
| parameter defaults  | checker creates a synthetic default function; emitter inserts the call (`function-body.ts:1036-1085`) | partly (`defaultArguments`) |
| field defaults      | checker (synthetic `$default.*` call)                                     | no                               |
| named arguments     | emitter reorders using `argumentParameterIndices`                         | yes, as metadata                 |
| varargs and spread  | checker (list literal, or the passed local)                               | no                               |
| copy-update         | emitter (`function-body.ts:779`, `suspension.ts:625`)                     | yes (`data.spread`)              |
| comprehensions      | emitter (`iterator.ts:131`, `suspension.ts:729`)                          | yes                              |
| `for`               | emitter (`function-body.ts:939`, `suspension.ts:732`)                     | yes                              |
| trailing blocks     | parser                                                                    | no                               |
| test blocks         | checker creates suspending `$test.N`; emitter derives the export by regex | no                               |
| interpolation       | parser, then checker (`string-build` and `display`)                       | lowered to built-ins             |

Desugaring is spread across three stages (F-609).

### Missing middle IR

- `SuspensionPlan` (`emitter/suspension.ts`) is a partial control-flow IR,
  but only for suspending functions. Its operations still hold unlowered HIR
  expressions, and it has no liveness.
- In the frame-liveness probe, all 6 locals are stored at all 5 suspension
  points, including 3 that are dead (this confirms F-552).
- Boxing, callable adapters, and trait adapters are decided while WAT text is
  written (`function-body.ts:1045-1060`, `1195-1255`).

A lowered IR is needed; `SuspensionPlan` is the natural seed.

### Requirements in HIR

Rows are carried on function types (`$Logger`, `$row:r`) and in
`HirFunction.requirements`. `explain-requirements` reads those rows but
rebuilds call paths with a separate hand-written walker, which misses cases
(F-600, F-601, F-612).

### Module structure

| File                          | Lines | Fan-in | Share of the 1,500-line cap |
| ----------------------------- | ----- | ------ | --------------------------- |
| `checker/shared.ts`           | 1,225 | 16     | 82%                         |
| `checker/context.ts`          | 1,181 | 15     | 79%                         |
| `checker/expression-calls.ts` | 1,313 | 1      | 88%                         |
| `emitter/function-body.ts`    | 1,487 | n/a    | 99%                         |
| `emitter/emitter.ts`          | 1,460 | n/a    | 97%                         |
| `parser/parser.ts`            | 1,219 | n/a    | 81%                         |
| `emitter/suspension.ts`       | 1,191 | n/a    | 79%                         |

- The checker is one class split across files by inheritance: a 12-class
  chain from `CheckerContext` to `FunctionChecker`. It has about 30 mutable
  fields (`context.ts:117-148`) and a 20-argument positional constructor. The
  emitter uses the same pattern, 5 levels deep.
- Commit `58262b7` split a 4,115-line `checker.ts` into 22 files and a
  2,735-line `emitter.ts` into 6. The resulting files are grouped by topic.
  The `??` dispatch chains that connect them lose exhaustiveness checking
  (F-610).
- `context.ts` and `shared.ts` share 68 duplicated 6-line windows. That
  suggests code was copied rather than moved during the split; this is
  `UNVERIFIED` beyond the count.

### Error recovery

| Probe                               | Planted | Reported |
| ----------------------------------- | ------- | -------- |
| type errors in separate functions   | 6       | 5        |
| parse errors                        | 3       | 1        |
| delimiter errors plus a parse error | 3       | 2 (lexer errors hide the parse error) |
| signature error plus body error     | 2       | 1 (`program.ts:43`)                   |

Filed as F-605.

## 6.2 Parallelism Readiness

Shared mutable state written while checking function bodies:

- the `closures` array (`checker/checker.ts:120-121`, `173`, `180`, `234`):
  a closure's index is its global push order;
- the `globals` map (`statements.ts:367`, `480`, `533`): the module body must
  be checked first (`program-lower.ts:87-94`);
- `diagnostics` (`program-lower.ts:119`, `127`): append-only, easy to merge.

Shared emitter state: `callableAdapters` (`function-body.ts:1100-1103`),
`providerKeys` (`context.ts:233-240`), 5 feature flags (`context.ts:74-78`),
and a loop counter.

| Stage                 | Readiness                                                           |
| --------------------- | ------------------------------------------------------------------- |
| lex and parse         | ready                                                               |
| signature collection  | ready as a cheap serial prepass                                     |
| body checking         | needs refactor (closure numbering, `globals`)                       |
| module-init check     | needs refactor (per-function read-set summaries); the whole-module graph is a language rule (`spec/10-modules.md:228-233`) |
| per-function emission | needs refactor (global registries and flags)                        |
| assembly              | blocked by design (an implementation choice): one WAT string for the module, and 85 to 90% of the time |

## 6.3 Incremental Readiness

- Named functions, including local `fn`, must declare rows. Omitting one
  gives `missing-requirement`. Only closures infer rows.
- In the ripple probe, the compiler reported 1 error. Restoring the build
  needed 4 hand edits; afterward 5 named rows and 4 closure rows had changed.

| Units | Cold    | One-line edit | Assembly share |
| ----- | ------- | ------------- | -------------- |
| 250   | 594 ms  | 554 ms        | 85%            |
| 1000  | 2115 ms | 2641 ms       | 91%            |
| 2000  | 6595 ms | 6337 ms       | 90%            |

- There is no cache or query layer.
- WAT stability, out of 628 functions: changing one literal changes 1;
  inserting a function changes 308; inserting a closure changes 204.
- Replay identity breaks on a comment or whitespace edit inside a function
  (F-611).

Design points that block query-based or incremental compilation. Each is an
implementation choice unless marked as a language rule:

1. Global sequential numbering of functions, closures, signatures, and
   adapters (F-608).
2. String types with bare-name identity (F-607).
3. The module body must be checked first, and a binding is visible only after
   its source position. **Language rule**, but `10-modules.md:222-226` is
   ambiguous.
4. Transitive initialization checking. **Language rule**
   (`10-modules.md:228-233`). The unmemoized walk behind F-602 is an
   implementation choice.
5. Trait resolution scans every implementation. The coherence requirement is
   a **language rule** (`09-traits.md:190-227`); the linear scan is not.
6. Closure inference checks each body twice (F-604).
7. A signature error stops all body checking.
8. The whole module is assembled from one WAT string.
9. Replay identity hashes raw source text. The spec defers code identity
   (`07-functions.md:258`).
10. Explicit rows on named functions. **Language rule**, and a helpful one: an
    edit invalidates only direct callers.

## 6.4 Code Health

- **Folder boundaries:** 0 violations across `src/`, `test/`, and `spec/`.
- **Function length:** 27 functions exceed 150 lines; 13 are at 250 to 299.
  Longest: `emitValueExpression` (299), `createProgramDeclarations` (296),
  `lowerExpression` (288), `checkOperatorExpression` (287), `instantiate`
  (287), `emitCallExpression` (285).
- **Duplication:** 9.8% of 15,330 lines overall. It is 31.5% in `emitter.ts`
  (the control-flow and linear suspension paths) and 35.3% in
  `expression-suspensions.ts`. The match-condition code in
  `function-body.ts:1358-1386` and `emitter.ts:640-669` is nearly identical.
- **HIR walkers:** `function-body.ts` covers 80 of 80 node kinds,
  `suspension.ts` 79, `requirements.ts` 57, `checker/shared.ts` 25, plus one
  reflective walker. Nothing checks walker exhaustiveness.

## Verdict: Does The HIR Design Make Sense?

Mostly yes, for a single-file MVP. The HIR is a real typed and resolved
boundary, with explicit provider and dictionary passing and clean layer
separation.

Three things weaken it:

- types are strings;
- about half the desugaring happens in the emitter, repeated across three
  lowering paths;
- the only lower IR covers suspending functions and has no liveness.

Before multi-module or incremental work, it needs interned structured types,
stable declaration identities, and one shared lowered IR.

## Findings

| ID    | Severity | Title                                                                         |
| ----- | -------- | ----------------------------------------------------------------------------- |
| F-600 | minor    | `explain-requirements` skips 23 of 80 HIR kinds and reports real uses as "declared" |
| F-601 | minor    | `explain-requirements` shows closure-routed requirements as a direct `$.use`  |
| F-602 | major    | the module-initialization check takes exponential time (8.6 s at call depth 22) |
| F-603 | major    | the initialization check misses bound and dynamic trait dispatch; an accepted program silently reads a zero global |
| F-604 | minor    | nested unannotated closures double check time per level (3.2 s at depth 20)   |
| F-605 | minor    | error recovery limits                                                         |
| F-606 | minor    | errors inside `${...}` are reported at 1:1                                    |
| F-607 | note     | string-typed HIR with name-based type identity                                |
| F-608 | note     | program-wide numbering makes WAT unstable under edits                         |
| F-609 | note     | desugaring split between checker and emitter; control flow lowered three times |
| F-610 | note     | `??` dispatch chains defeat exhaustiveness checking                           |
| F-611 | note     | replay code identity breaks on whitespace or comment edits                    |
| F-612 | note     | `explain-requirements` output grows exponentially                             |

## Phase-7 Questions

1. Named functions already must declare rows. Should the spec state that a
   missing clause means an empty row? Or should private functions infer rows,
   at the cost of a whole-module fixpoint?
2. Does "functions reached by trait dispatch" (module initialization)
   include bounded and dynamic dispatch? Every implementation in scope, or
   only those whose type can reach that point?
3. In `10-modules.md:225`, does "later function bodies" mean source order or
   execution order?
4. Should replay code identity be normalized, over tokens or HIR?
5. Should the conformance contract require independent errors from
   different declarations to be reported?
6. Should `explain-requirements` print one witness path per key?

## Not Done

- Timings were taken on a contended CPU and are order-of-magnitude. The
  roughly 2x-per-level ratios held across two runs.
- A full replay audit belongs to 4.1.
- The shingle duplication method uses exact matches, so it undercounts
  renamed near-duplicates.
- Parallel checking was not prototyped, because `src/` may not be modified.
  The shared writes were located by reading source.
