import type { ImplDecl, Program } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import { displayType } from "../types.ts";

const WORD = /[\p{ID_Start}_][\p{ID_Continue}]*/gu;

/**
 * Every type parameter of a generic implementation is constrained: the
 * trait's arguments or the target name it, or an associated-type binding in
 * the bound of a constrained parameter fixes it
 * (09-traits.md#r-trait.overlap.constrained-head, #r-trait.overlap.constrained-binding).
 */
export function unconstrainedImplementationParameters(program: Program): readonly Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  for (const implementation of program.implementations) {
    if (implementation.genericParameters.length === 0) continue;
    const free = unconstrained(implementation);
    if (free.length === 0) continue;
    diagnostics.push({
      code: "unconstrained-impl-parameter",
      severity: "error",
      message: `implementation parameter${free.length === 1 ? "" : "s"} ${free.map((name) => `'${displayType(name)}'`).join(", ")} appear${free.length === 1 ? "s" : ""} in neither the trait's arguments nor the target '${displayType(implementation.targetName)}'`,
      span: implementation.span,
    });
  }
  return diagnostics;
}

function unconstrained(implementation: ImplDecl): readonly string[] {
  const parameters = new Set(implementation.genericParameters);
  const constrained = new Set<string>();
  const mark = (text: string): boolean => {
    let added = false;
    for (const [word] of text.matchAll(WORD))
      if (parameters.has(word) && !constrained.has(word)) {
        constrained.add(word);
        added = true;
      }
    return added;
  };
  mark(implementation.targetName);
  if (implementation.traitName !== undefined) mark(implementation.traitName);
  let changed = true;
  while (changed) {
    changed = false;
    for (const bound of implementation.genericBounds) {
      if (!constrained.has(bound.parameter)) continue;
      for (const binding of bound.bindings ?? []) if (mark(binding.type.name)) changed = true;
    }
  }
  return implementation.genericParameters.filter((name) => !constrained.has(name));
}
