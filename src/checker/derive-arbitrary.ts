import type { DataField, EnumDecl, Expression, ImplDecl, Program, UseDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { fieldsSelfRef, selfRefScope, type SelfRefScope } from "./self-ref.ts";
import { Source_ } from "./generated-source.ts";
import type { Target } from "./member-lines.ts";

// Derived `Arbitrary` (spec/10-modules.md#derived-arbitrary) for a target
// with an `arbitrary.with` fact, and the `std.testing.arbitrary` module.
//
// Every other derived `Arbitrary` instantiates the `std.testing` template
// (lib/std/testing.hd). A template cannot draw a tuned member: its
// `Source` strengthens `member[F]` to `F < Arbitrary`, which every built
// member must satisfy (r-annot.walker.obligation), while a tuned member need
// not implement `Arbitrary` and must instead be inspectable, reported at
// the fact (r-module.testing.arbitrary.with.inspectable). So the checker
// generates such a derivation directly. The generated `arbitrary` draws
// each member with `c.draw[F]()`, or with its `arbitrary.with(gen)` fact's
// generator, whose drawn value it downcasts to `F`. An enum draws a variant
// index first, with the simplest variant at index 0, so a spent draw budget
// picks it; the simplest is the first variant whose `self_ref` is not
// `.Required`, as in the template (self-ref.ts).

/** The hidden name of `std.testing.arbitrary.with` (lib/std/arbitrary.hd). */
export const ARBITRARY_WITH = "__std_arbitrary_with";
/** One value that a `Generator` draws (lib/std/arbitrary.hd). */
const DRAW_WITH = "__std_arbitrary_draw_with";
/** `std.inspect.downcast_val`, as the generated code imports it. */
const DOWNCAST = "hd__arbitrary_downcast";

function transform(node: unknown, callback: (value: Record<string, unknown>) => unknown): unknown {
  if (Array.isArray(node)) return node.map((item) => transform(item, callback));
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(node)) result[key] = transform(value, callback);
  return callback(result);
}

/**
 * `use std.testing.arbitrary` imports a module, which the prototype has no
 * values for: each `arbitrary.with(gen)` becomes a call of `with` by its
 * hidden name, and `std.inspect`'s `Inspectable` and `TypeId`, which the
 * module and the derived code name, are imported.
 */
export function withArbitraryModule(program: Program): Program {
  const aliases = new Set<string>();
  const uses = program.uses.flatMap((use) => {
    if (use.module !== "std.testing") return [use];
    const names = use.names.filter((name) => {
      if (name.name !== "arbitrary") return true;
      aliases.add(name.alias ?? name.name);
      return false;
    });
    if (names.length === use.names.length) return [use];
    return names.length > 0 ? [{ ...use, names }] : [];
  });
  if (aliases.size === 0) return program;
  const rewritten = transform({ ...program, uses }, (value) => {
    const callee = value.callee as Expression | undefined;
    if (
      value.kind === "call" &&
      callee?.kind === "member" &&
      callee.name === "with" &&
      callee.receiver.kind === "name" &&
      aliases.has(callee.receiver.name)
    )
      return { ...value, callee: { kind: "name", name: ARBITRARY_WITH, span: callee.span } };
    return value;
  }) as Program;
  const imported = new Set(
    program.uses
      .filter((use) => use.module === "std.inspect")
      .flatMap((use) => use.names.map((name) => name.name)),
  );
  const inspect: UseDecl = {
    kind: "use",
    module: "std.inspect",
    names: [
      ...["Inspectable", "TypeId"].filter((name) => !imported.has(name)).map((name) => ({ name })),
      { name: "downcast_val", alias: DOWNCAST },
    ],
    span: program.span,
  };
  return { ...rewritten, uses: [...rewritten.uses, inspect] };
}

/** The local name of a `std.testing` declaration, or its hidden name. */
export function testingName(program: Program, name: string): string | undefined {
  for (const use of program.uses)
    if (use.module === "std.testing")
      for (const imported of use.names)
        if (imported.name === name) return imported.alias ?? imported.name;
  return undefined;
}

export interface ArbitraryDerivation {
  readonly target: Target;
  /** The facts this derivation sees for a data type's member, by name. */
  readonly facts: (member: string) => {
    readonly facts: readonly Expression[];
    readonly omitted: boolean;
  };
  readonly span: SourceSpan;
}

/** The `arbitrary.with(gen)` fact among `facts` (r-module.testing.arbitrary.with). */
function generatorFact(facts: readonly Expression[]): Expression | undefined {
  return facts.find(
    (fact) =>
      fact.kind === "call" && fact.callee.kind === "name" && fact.callee.name === ARBITRARY_WITH,
  );
}

/** Whether a member of `target` has an `arbitrary.with` fact. */
export function hasGeneratorFact(
  target: Target,
  factsOf: (member: string) => readonly Expression[],
): boolean {
  if (target.kind === "data")
    return target.declaration.fields.some((field) => generatorFact(factsOf(field.name)));
  return target.declaration.variants.some((variant) =>
    variant.fields.some((field) => generatorFact(field.metadata ?? [])),
  );
}

/** The ordinary `impl Arbitrary for T` of one derivation. */
function deriveOne(
  item: ArbitraryDerivation,
  index: number,
  names: { readonly arbitrary: string; readonly choices: string },
  scope: SelfRefScope,
): { readonly implementation: ImplDecl; readonly program: Program } {
  const { target } = item;
  const declaration = target.declaration;
  const parameters = declaration.genericParameters;
  const out = new Source_();
  const T = out.type(
    parameters.length > 0 ? `${declaration.name}[${parameters.join(",")}]` : declaration.name,
  );
  const bounds = parameters.map((parameter) => `${parameter} < ${names.arbitrary}`).join(", ");
  out.add(`impl${parameters.length > 0 ? `[${bounds}]` : ""} ${names.arbitrary} for ${T}:`);
  out.add(`    fn arbitrary(c: mut ${names.choices}) -> ${T}:`);
  const helpers: string[][] = [];
  const helperSpans: SourceSpan[] = [];
  // One member's value: its generator's value downcast to its type, or its
  // type's own `Arbitrary` (r-module.testing.arbitrary.with.downcast,
  // .with.no-fallback).
  const draw = (field: DataField, owner: string, facts: readonly Expression[]): string => {
    const F = out.type(field.type.name);
    const fact = generatorFact(facts);
    if (!fact) return `c.draw[${F}]()`;
    const helper = `hd__arbitrary${index}_${helpers.length}`;
    const member = `${owner}.${field.positional ? `_${field.name}` : field.name}`;
    helpers.push([
      `fn ${helper}${parameters.length > 0 ? `[${parameters.join(", ")}]` : ""}(c: mut ${names.choices}) -> ${F}:`,
      `    drawn := ${DRAW_WITH}(${out.expression(fact)}, c)`,
      `    match ${DOWNCAST}[${F}](drawn):`,
      `        .Some(value) => value`,
      `        .None => panic("arbitrary.with on member ${member} of type \${TypeId::of[${F}]()} drew a value of type \${drawn.runtime_type()}")`,
    ]);
    helperSpans.push(fact.span);
    return `${helper}(c)`;
  };
  const recursive = (fields: readonly DataField[]): boolean =>
    fieldsSelfRef(fields, declaration.name, scope).variant === "Required";
  const noFinite = `        panic(${out.string(`${declaration.name} has no finite value: every way to build it needs another ${declaration.name}`)})`;
  if (target.kind === "data") {
    const fields = target.declaration.fields;
    if (recursive(fields)) out.add(noFinite);
    else {
      const values = fields.map((field) => {
        const { facts, omitted } = item.facts(field.name);
        if (omitted && field.default) return `${field.name}: ${out.expression(field.default)}`;
        return `${field.name}: ${draw(field, declaration.name, facts)}`;
      });
      out.add(`        ${declaration.name} { ${values.join(", ")} }`);
    }
  } else {
    const enumDeclaration: EnumDecl = target.declaration;
    const variants = enumDeclaration.variants;
    // The simplest choice is the first non-recursive variant
    // (r-module.testing.arbitrary.derive.simplest); the others follow in
    // declaration order.
    const simplest = variants.findIndex((variant) => !recursive(variant.fields));
    const ordered =
      simplest < 0 ? [] : [variants[simplest]!, ...variants.filter((_, at) => at !== simplest)];
    const build = (variant: (typeof variants)[number]): string => {
      const owner = `${declaration.name}.${variant.name}`;
      if (variant.fields.length === 0) return owner;
      return `${owner}(${variant.fields.map((field) => draw(field, owner, field.metadata ?? [])).join(", ")})`;
    };
    if (ordered.length === 0) out.add(noFinite);
    else if (ordered.length === 1) out.add(`        ${build(ordered[0]!)}`);
    else {
      out.add(`        match c.int(0, ${ordered.length - 1}):`);
      ordered.forEach((variant, position) =>
        out.add(
          `            ${position === ordered.length - 1 ? "_" : position} => ${build(variant)}`,
        ),
      );
    }
  }
  helpers.forEach((lines, position) => {
    for (const line of lines) out.add(line, helperSpans[position]);
  });
  const program = out.program(item.span);
  return {
    implementation: program.implementations[0]!,
    program: { ...program, implementations: [] },
  };
}

/** The implementations and helper functions of the derived `Arbitrary` implementations. */
export function deriveArbitrary(
  program: Program,
  items: readonly ArbitraryDerivation[],
  arbitrary: string,
): { readonly implementations: ImplDecl[]; readonly functions: Program["functions"][number][] } {
  if (items.length === 0) return { implementations: [], functions: [] };
  const names = {
    arbitrary,
    choices: testingName(program, "Choices") ?? "__std_testing_Choices",
  };
  const scope = selfRefScope(program);
  const generated = items.map((item, index) => deriveOne(item, index, names, scope));
  return {
    implementations: generated.map((item) => item.implementation),
    functions: generated.flatMap((item) => item.program.functions),
  };
}
