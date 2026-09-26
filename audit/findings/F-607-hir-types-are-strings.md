# F-607: HIR types are strings that the emitter re-parses, with nominal types keyed by bare name
Severity: note
Area: architecture
Evidence: audit/evidence/06-compiler/desugar-hir.md ("distinct HIR type strings"),
  `hd dump-hir spec/conformance/typing/valid/requirements-and-suspension.hd`
  (types such as `fn!(string)->Result[User?,DbError]$Database`,
  `suspend(1):...`, `provider-row:r`, `mut:UserId`)
Effect: `ValueType = string` (src/hir.ts:3). The emitter calls 11 string
  parsers from src/types.ts at about 60 call sites, for example `functionParts` and
  `nominalGenericParts`. It resolves `Point` through `dataByName` and
  `enumByName` maps (src/emitter/context.ts:190-200). Two modules that declare
  the same type name cannot be told apart. Each type test re-parses text, and
  the compiler cannot check that a type string is well formed.
Recommendation: OPEN_ISSUES question. Should HIR carry interned structured
  types with declaration IDs before multi-module work starts?
