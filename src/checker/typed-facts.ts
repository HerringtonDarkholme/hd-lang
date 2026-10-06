import type {
  DataDecl,
  DataField,
  EnumDecl,
  Expression,
  FunctionDecl,
  GenericBound,
  MemberLine,
  Program,
  TypeRef,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { functionResultText, readonlyType } from "../types.ts";
import { Source_ } from "./generated-source.ts";
import {
  standardResultDeclaration,
  standardSubmoduleFunctionIdentity,
  standardSupertraits,
} from "./standard-library.ts";

// Typed facts (spec/lang/14-annotations.md#member-typed-facts). A data type or
// enum declared with `@annotate::[Q](...)` is a typed fact type `D`, and the
// pattern `Q`, a type over `D`'s parameters such as `F`, `List[T]`, or
// `fn(T) -> R`, describes its target's type. This pass moves `Q` onto the
// declaration (`factPattern`) and rewrites each value `v` on a field or a
// module-level function to the call `hd__typed_fact_N(v)` of a generated
// identity function `fn hd__typed_fact_N[P1, P2](fact: D[P1, P2]) -> D[P1, P2]`,
// marked with the target's monomorphic type `X`. The checker matches `Q`
// against `X` as a call's argument infers its parameter's type
// (annot.typed-fact.infer), which gives the call the arguments `Q` mentions;
// the ordinary call checking then gives `v` its expected type, infers the
// other slots, and checks `D`'s bounds, all at the decorator or member line
// (annot.typed-fact.check, .check.other-params, .check.inferred,
// .check.bounds, .check.reported). Every later reader of the fact, such as a
// derivation's facts, sees the typed value. It runs after trait-less blocks
// are folded into the declarations, before any derivation reads them.

type Report = (code: string, message: string, span: SourceSpan) => void;

interface TypedFactType {
  /** The fact type's name in the program. */
  readonly name: string;
  readonly declaration: DataDecl | EnumDecl;
}

/** A target's type and the generic scope its type arguments need. */
interface TargetType {
  readonly type: string;
  readonly scope: string;
}

const FACT_HELPER = "hd__typed_fact_";
const SCOPE_PREFIX = "HdFact";

function baseName(type: string): string {
  return readonlyType(type).split("[")[0]!;
}

function boundText(bound: GenericBound): string {
  return bound.traits.join(" & ");
}

/**
 * The local names under which the program imports `std.annotation.annotate`,
 * which the compiler recognizes by its qualified name (annot.typed-fact.declare).
 */
function annotateNames(program: Program): Set<string> {
  const names = new Set(
    program.functions
      .filter((declaration) => declaration.standardName === "std.annotation.annotate")
      .map((declaration) => declaration.name),
  );
  for (const use of program.uses)
    if (use.module === "std.annotation")
      for (const imported of use.names)
        if (imported.name === "annotate") names.add(imported.alias ?? imported.name);
  return names;
}

/**
 * A target's type made monomorphic (annot.typed-fact.monomorphic): each
 * bounded type parameter becomes a fresh parameter of the check's scope with
 * its bounds, one fixed type that satisfies them (.monomorphic.bound), and
 * each unbounded one becomes `Any`. A requirement-row parameter stays a row.
 */
function monomorphic(
  type: string,
  parameters: readonly string[],
  bounds: readonly GenericBound[],
  rows: ReadonlySet<string>,
): TargetType {
  const renames = new Map<string, string>();
  for (const parameter of parameters)
    renames.set(
      parameter,
      bounds.some((bound) => bound.parameter === parameter) || rows.has(parameter)
        ? `${SCOPE_PREFIX}${parameter}`
        : "Any",
    );
  const rename = (text: string): string =>
    text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word) => renames.get(word) ?? word);
  const scoped = parameters.filter((parameter) => renames.get(parameter) !== "Any");
  const scope = scoped.map((parameter) => {
    const traits = bounds
      .filter((bound) => bound.parameter === parameter)
      .map((bound) => rename(boundText(bound)));
    return traits.length > 0
      ? `${SCOPE_PREFIX}${parameter} < ${traits.join(" & ")}`
      : `${SCOPE_PREFIX}${parameter}`;
  });
  return { type: rename(type), scope: scope.length > 0 ? `[${scope.join(", ")}]` : "" };
}

/**
 * A function's signature as a function type, with its `!` and requirement
 * row (annot.typed-fact.declared-type). Defaults are not part of it.
 */
function signatureType(declaration: FunctionDecl): string {
  const parameters = declaration.parameters.map((parameter) =>
    parameter.variadic ? `${parameter.type.name}...` : parameter.type.name,
  );
  const requirements = declaration.writtenRequirements ?? declaration.requirements;
  const row = requirements.length > 0 ? `$${[...requirements].sort().join("+")}` : "";
  // The parser's type text: no spaces, and the row's keys sorted.
  return `fn${declaration.suspending ? "!" : ""}(${parameters.join(",")})->${functionResultText(declaration.result.name)}${row}`;
}

type Expand = (bounds: readonly GenericBound[]) => readonly GenericBound[];

function functionTarget(declaration: FunctionDecl, expand: Expand): TargetType | undefined {
  if (declaration.resultOmitted) return undefined;
  const rows = new Set(declaration.rowParameters ?? []);
  return monomorphic(
    signatureType(declaration),
    declaration.genericParameters,
    expand(declaration.genericBounds),
    rows,
  );
}

function fieldTarget(owner: DataDecl | EnumDecl, field: DataField, expand: Expand): TargetType {
  return monomorphic(
    field.type.name,
    owner.genericParameters,
    expand(owner.genericBounds ?? []),
    new Set(),
  );
}

/**
 * Finds the typed fact types, checks each `@annotate` type argument, and
 * rewrites each typed fact value to its checking call.
 */
export function withTypedFacts(program: Program, error: Report): Program {
  const annotate = annotateNames(program);
  const local = new Map<string, TypedFactType>();

  // Declarations: move `annotate`'s type argument, the pattern, onto the
  // declaration. Without one, it is the default `Any`, an untyped fact type
  // (annot.typed-fact.untyped).
  const declare = <T extends DataDecl | EnumDecl>(declaration: T): T => {
    const decorators = declaration.decorators;
    if (!decorators || annotate.size === 0) return declaration;
    let changed = false;
    let pattern: TypeRef | undefined;
    const facts = decorators.facts.map((fact) => {
      if (
        fact.kind !== "call" ||
        fact.callee.kind !== "name" ||
        !annotate.has(fact.callee.name) ||
        !fact.typeArguments ||
        fact.typeArguments.length === 0
      )
        return fact;
      changed = true;
      if (fact.typeArguments.length > 1)
        error(
          "argument-count",
          `annotate takes one type argument, received ${fact.typeArguments.length}`,
          fact.span,
        );
      else {
        pattern = fact.typeArguments[0]!;
        local.set(declaration.name, { name: declaration.name, declaration });
      }
      const { typeArguments: _typeArguments, ...call } = fact;
      return call;
    });
    if (!changed) return declaration;
    return {
      ...declaration,
      decorators: { ...decorators, facts },
      ...(pattern ? { factPattern: pattern } : {}),
    };
  };
  const declared: Program = {
    ...program,
    data: program.data.map(declare),
    enums: program.enums.map(declare),
  };

  const functions = new Map(declared.functions.map((item) => [item.name, item] as const));
  const importOrigins = new Map<string, string>();
  for (const use of declared.uses)
    for (const imported of use.names)
      importOrigins.set(imported.alias ?? imported.name, `${use.module}.${imported.name}`);
  // A bound's supertraits are bounds too, so the fixed type of a parameter
  // bounded by `Integer` meets a bound `Num` (annot.typed-fact.monomorphic.bound).
  const traits = new Map(declared.traits.map((item) => [item.name, item] as const));
  const supertraits = (trait: string): readonly string[] => {
    const local = traits.get(trait);
    return local
      ? local.supertraits.map((item) => item.name).filter((name) => !name.includes("["))
      : standardSupertraits(declared, trait);
  };
  const expand: Expand = (bounds) =>
    bounds.map((bound) => {
      const all = new Set<string>();
      const add = (trait: string): void => {
        if (all.has(trait)) return;
        all.add(trait);
        if (!trait.includes("[")) for (const item of supertraits(trait)) add(item);
      };
      for (const trait of bound.traits) add(trait);
      return { ...bound, traits: [...all] };
    });
  const factTypeOf = (fact: Expression): TypedFactType | undefined => {
    if (fact.kind === "data") return local.get(fact.name);
    if (fact.kind !== "call") return undefined;
    const identity =
      fact.callee.kind === "member" && fact.callee.receiver.kind === "name"
        ? standardSubmoduleFunctionIdentity(
            importOrigins.get(fact.callee.receiver.name),
            fact.callee.name,
          )
        : undefined;
    const callee =
      fact.callee.kind === "name"
        ? fact.callee.name
        : identity === undefined
          ? undefined
          : [...functions.values()].find((declaration) => declaration.standardName === identity)
              ?.name;
    if (!callee) return undefined;
    const declaration = functions.get(callee);
    if (declaration?.resultOmitted) return undefined;
    const declaredFact = declaration && local.get(baseName(declaration.result.name));
    if (declaredFact) return declaredFact;
    // After std is joined, a hidden std function is present in `functions`
    // while its @annotate declaration is intentionally not a user import.
    // Fall through to the declaration's qualified std identity.
    const standard = standardResultDeclaration(declared, callee);
    if (!standard) return undefined;
    const call = standard.declaration.decorators?.facts.find(
      (item) =>
        item.kind === "call" &&
        item.callee.kind === "name" &&
        item.callee.name === "annotate" &&
        (item.typeArguments?.length ?? 0) > 0,
    );
    return call?.kind === "call"
      ? { name: standard.name, declaration: standard.declaration }
      : undefined;
  };

  // One identity function per fact type, its parameters renamed apart from
  // any name in a target's type.
  const helpers = new Map<string, string>();
  const helperSources: string[] = [];
  const helperOf = (fact: TypedFactType): string => {
    let name = helpers.get(fact.name);
    if (name) return name;
    name = `${FACT_HELPER}${helpers.size}`;
    helpers.set(fact.name, name);
    const parameters = fact.declaration.genericParameters;
    const renames = new Map(parameters.map((parameter, index) => [parameter, `HdP${index}`]));
    const rename = (text: string): string =>
      text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word) => renames.get(word) ?? word);
    const list = parameters.map((parameter) => {
      const traits = (fact.declaration.genericBounds ?? [])
        .filter((bound) => bound.parameter === parameter)
        .map((bound) => rename(boundText(bound)));
      return traits.length > 0
        ? `${renames.get(parameter)} < ${traits.join(" & ")}`
        : renames.get(parameter)!;
    });
    // A pattern such as `i32` may need no parameters (annot.typed-fact.pattern.concrete).
    const type =
      parameters.length > 0
        ? `${fact.name}[${parameters.map((parameter) => renames.get(parameter)).join(", ")}]`
        : fact.name;
    const generics = list.length > 0 ? `[${list.join(", ")}]` : "";
    helperSources.push(`fn ${name}${generics}(fact: ${type}) -> ${type}:`, `    fact`);
    return name;
  };

  const wrap = (fact: Expression, target: TargetType | undefined): Expression => {
    const typed = factTypeOf(fact);
    if (!typed || !target) return fact;
    const span = fact.span;
    return {
      kind: "call",
      callee: { kind: "name", name: helperOf(typed), span },
      arguments: [fact],
      span,
      typedFact: { target: target.type, factType: typed.name },
      ...(target.scope ? { typedFactScope: target.scope } : {}),
    };
  };
  // A typed fact attaches only to a field or a module-level function
  // (annot.typed-fact.targets).
  const misplaced = (facts: readonly Expression[] | undefined, what: string): void => {
    for (const fact of facts ?? [])
      if (factTypeOf(fact))
        error(
          "decorator-target-kind",
          `a typed fact attaches only to a field or a module-level function, not to ${what}`,
          fact.span,
        );
  };
  const wrapFields = <T extends DataField>(owner: DataDecl | EnumDecl, field: T): T =>
    field.metadata
      ? {
          ...field,
          metadata: field.metadata.map((fact) => wrap(fact, fieldTarget(owner, field, expand))),
        }
      : field;

  for (const declaration of [...declared.data, ...declared.enums])
    misplaced(declaration.decorators?.facts, `a ${declaration.kind} declaration`);
  for (const declaration of declared.enums)
    for (const variant of declaration.variants) misplaced(variant.metadata, "a variant");
  for (const declaration of declared.functions)
    for (const parameter of declaration.parameters) misplaced(parameter.metadata, "a parameter");
  for (const declaration of [...declared.traits, ...declared.implementations]) {
    misplaced(declaration.decorators?.facts, "a trait or implementation");
    for (const method of declaration.methods) {
      misplaced(method.decorators?.facts, "a method");
      for (const parameter of method.parameters) misplaced(parameter.metadata, "a parameter");
    }
  }
  for (const declaration of declared.types ?? [])
    misplaced(declaration.decorators?.facts, "a type declaration");

  const data = new Map(declared.data.map((item) => [item.name, item] as const));
  // A derivation block's member line on a data field (annot.line.typed).
  const wrapLine = (owner: DataDecl, line: MemberLine): MemberLine => {
    const field = owner.fields.find((item) => item.name === line.name);
    if (!field || line.value?.kind !== "list") return line;
    const value = line.value;
    return {
      ...line,
      value: {
        ...value,
        elements: value.elements.map((element, index) =>
          value.spreads?.[index] ? element : wrap(element, fieldTarget(owner, field, expand)),
        ),
      },
    };
  };

  const rewritten: Program = {
    ...declared,
    data: declared.data.map((declaration) => ({
      ...declaration,
      fields: declaration.fields.map((field) => wrapFields(declaration, field)),
    })),
    enums: declared.enums.map((declaration) => ({
      ...declaration,
      variants: declaration.variants.map((variant) => ({
        ...variant,
        fields: variant.fields.map((field) => wrapFields(declaration, field)),
      })),
    })),
    functions: declared.functions.map((declaration) =>
      declaration.decorators
        ? {
            ...declaration,
            decorators: {
              ...declaration.decorators,
              facts: declaration.decorators.facts.map((fact) =>
                wrap(fact, functionTarget(declaration, expand)),
              ),
            },
          }
        : declaration,
    ),
    implementations: declared.implementations.map((implementation) => {
      const owner = data.get(baseName(implementation.targetName));
      if (!implementation.memberLines || !implementation.traitName || !owner) return implementation;
      return {
        ...implementation,
        memberLines: implementation.memberLines.map((line) => wrapLine(owner, line)),
      };
    }),
  };
  if (helperSources.length === 0) return rewritten;
  const generated = new Source_();
  for (const line of helperSources) generated.add(line, program.span);
  const helperFunctions = generated.program(program.span).functions;
  return { ...rewritten, functions: [...rewritten.functions, ...helperFunctions] };
}
