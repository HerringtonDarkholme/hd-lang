import type { Expression, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";

// Declaration facts: the decorators on items, members, variants, and
// parameters (spec/14-annotations.md#member-metadata), and the literal facts that the
// unused-fact warning skips (#r-annot.fact.unused-std).

/**
 * One member, variant, or parameter holds at most one declaration fact of
 * each concrete type (annot.metadata.duplicate); the later one is reported.
 */
export function checkDuplicateDeclarationFacts(
  program: Program,
  factType: (fact: Expression) => string,
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  const check = (metadata: readonly Expression[] | undefined, holder = "member"): void => {
    const seen = new Set<string>();
    for (const fact of metadata ?? []) {
      const type = factType(fact);
      if (seen.has(type))
        error("duplicate-fact", `this ${holder} already holds a fact of the same type`, fact.span);
      seen.add(type);
    }
  };
  // Two type-level decorators of one fact type (annot.fact.duplicate-decorator).
  for (const declaration of [...program.data, ...program.enums])
    check(declaration.decorators?.facts, "declaration");
  for (const declaration of program.data)
    for (const field of declaration.fields) check(field.metadata);
  for (const declaration of program.enums)
    for (const variant of declaration.variants) {
      check(variant.metadata);
      for (const field of variant.fields) check(field.metadata);
    }
  for (const declaration of program.functions) {
    // Two values of one type before a function, trait, implementation,
    // newtype, or method (annot.decorator.duplicate).
    check(declaration.decorators?.facts, "declaration");
    for (const parameter of declaration.parameters) check(parameter.metadata);
  }
  for (const declaration of [...program.traits, ...program.implementations]) {
    check(declaration.decorators?.facts, "declaration");
    for (const method of declaration.methods) {
      check(method.decorators?.facts, "method");
      for (const parameter of method.parameters) check(parameter.metadata);
    }
  }
  for (const declaration of program.types ?? [])
    check(declaration.decorators?.facts, "declaration");
}

/** A literal fact has a primitive type, so it never warns as unused. */
export function isLiteralFact(fact: Expression): boolean {
  return ["string", "interpolated-string", "integer", "float", "boolean", "character"].includes(
    fact.kind,
  );
}
