# F-609: Desugaring is split between checker and emitter, and control flow is lowered three times
Severity: note
Area: architecture
Evidence: audit/evidence/06-compiler/desugar-hir.md, audit/evidence/06-compiler/duplication.md
Effect: The checker fully lowers varargs, field defaults, trailing blocks, and
  test blocks. The emitter still lowers named-argument order
  (`argumentParameterIndices`), parameter defaults (`defaultArguments`),
  copy-update (`spread`), `for`, comprehensions, generic boxing, callable
  adapters, and trait adapters. Each of these must be handled once per
  emission path. There are three: direct (function-body.ts), linear suspension,
  and CFG suspension (emitter.ts plus suspension.ts, which handles 79 of 80
  kinds). Match-condition code is copied almost verbatim at
  function-body.ts:1358-1386 and emitter.ts:640-669. 31.5% of emitter.ts lines
  sit in windows duplicated within the file.
Recommendation: OPEN_ISSUES question. Should a lowered IR sit between HIR and
  WAT, with explicit argument order, defaults, loops, boxing, and a CFG?
  One lowering could then serve every emission path.
