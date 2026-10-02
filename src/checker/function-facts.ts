import type { Expression, FunctionDecl, Program } from "../ast.ts";
import { Source_ } from "./generated-source.ts";
import { visit } from "./template-instances.ts";

// `facts_of(f)` (spec/14-annotations.md#function-facts). `lib/std/annotation.hd`
// declares `facts_of` with `@intrinsic("facts_of")`. Before checking, this
// pass gives each module-level function that a `facts_of` call names a
// builder, `hd__facts_of_f() -> Facts`, over `std.structure`'s `Facts`; the
// checker lowers a call whose argument names that function to a call of its
// builder (`factsOfCall`), and rejects any other argument.

const ANNOTATION_MODULE = "std.annotation";

/** The `@intrinsic` name of `std.annotation.facts_of`. */
export const FACTS_OF_INTRINSIC = "facts_of";

/**
 * The checker intrinsic `hd__structure_fact(v)`: a fact value erased to
 * `Inspectable` whatever its type, since the prototype's `Facts` holds
 * `Inspectable` values where the specification has `Any`. Every fact list
 * that this pass and typed derivation generate wraps its values in it.
 */
export const STRUCTURE_FACT = "hd__structure_fact";

export function factsOfBuilderName(functionName: string): string {
  return `hd__facts_of_${functionName}`;
}

/** Whether the program imports `facts_of`, whose calls return `std.structure`'s `Facts`. */
export function importsFactsOf(program: Program): boolean {
  return factsOfName(program) !== undefined;
}

/** The local name of an imported `std.annotation.facts_of`, if any. */
export function factsOfName(program: Program): string | undefined {
  for (const use of program.uses) {
    if (use.module !== ANNOTATION_MODULE) continue;
    const imported = use.names.find((name) => name.name === "facts_of");
    if (imported) return imported.alias ?? imported.name;
  }
  return undefined;
}

/**
 * One builder per module-level function that a `facts_of` call names. `fact`
 * gives the value a builder holds for one decorator fact, and `facts` is the
 * name under which the program declares `std.structure`'s `Facts`.
 */
export function factsOfBuilders(
  program: Program,
  facts: string,
  fact: (expression: Expression) => Expression,
): readonly FunctionDecl[] {
  const name = factsOfName(program);
  if (name === undefined) return [];
  const targets = new Set<string>();
  visit(program, (value) => {
    if (value.kind !== "call") return;
    const call = value as unknown as Extract<Expression, { kind: "call" }>;
    const [argument] = call.arguments;
    if (call.callee.kind === "name" && call.callee.name === name && argument?.kind === "name")
      targets.add(argument.name);
  });
  const out = new Source_();
  for (const declaration of program.functions) {
    if (!targets.has(declaration.name) || declaration.name.startsWith("hd__")) continue;
    const items = (declaration.decorators?.facts ?? []).map(
      (item) => `${STRUCTURE_FACT}(${out.expression(fact(item))})`,
    );
    out.add(`fn ${factsOfBuilderName(declaration.name)}() -> ${facts}:`);
    out.add(`    ${facts} { items: [${items.join(", ")}] }`);
  }
  return out.lines.length === 0 ? [] : out.program(program.span).functions;
}
