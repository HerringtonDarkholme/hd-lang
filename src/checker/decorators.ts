import type {
  DataDecl,
  Decorators,
  EnumDecl,
  Expression,
  FunctionDecl,
  MethodDecl,
  Parameter,
  Program,
} from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import { factType } from "./typed-derivation.ts";

// Decorators as plain values (spec/14-annotations.md#prefix-decorators and
// #target-kinds). Two passes over the attached values:
//
// - `withBareMarkerCalls`: a bare decorator name that resolves to a function
//   with no parameters is called (annot.decorator.bare-call). It runs before
//   typed derivation and shapes copy the values, and again after the
//   standard library is joined, for `std`'s own decorators.
// - `checkDecoratorTargets`: a value whose type carries a
//   `std.annotation.Annotate` fact may be attached only to the target kinds
//   it lists (annot.target.limit.error). The check reads the `@annotate`
//   arguments syntactically, as the prototype's other fact passes do.

/** The qualified names the compiler recognizes (annot.target.recognized). */
export const STANDARD_ANNOTATE = "std.annotation.Annotate";

/** A target kind: a variant of `std.annotation.Target`, or none for a newtype. */
type TargetKind =
  | "Fn"
  | "Data"
  | "Enum"
  | "Field"
  | "Variant"
  | "Param"
  | "Trait"
  | "Impl"
  | "Method"
  | undefined;

interface Attached {
  readonly fact: Expression;
  readonly kind: TargetKind;
}

/** Every attached value in the program, with its target's kind (annot.target.kind.*). */
function attachedValues(program: Program): Attached[] {
  const result: Attached[] = [];
  const add = (facts: readonly Expression[] | undefined, kind: TargetKind): void => {
    for (const fact of facts ?? []) result.push({ fact, kind });
  };
  const parameters = (list: readonly Parameter[]): void => {
    for (const parameter of list) add(parameter.metadata, "Param");
  };
  const methods = (list: readonly MethodDecl[]): void => {
    for (const method of list) {
      add(method.decorators?.facts, "Method");
      parameters(method.parameters);
    }
  };
  for (const declaration of program.functions) {
    add(declaration.decorators?.facts, "Fn");
    parameters(declaration.parameters);
  }
  for (const declaration of program.data) {
    add(declaration.decorators?.facts, declaration.newtype ? undefined : "Data");
    for (const field of declaration.fields) add(field.metadata, "Field");
  }
  for (const declaration of program.enums) {
    add(declaration.decorators?.facts, "Enum");
    for (const variant of declaration.variants) {
      add(variant.metadata, "Variant");
      for (const field of variant.fields) add(field.metadata, "Field");
    }
  }
  for (const declaration of program.traits) {
    add(declaration.decorators?.facts, "Trait");
    methods(declaration.methods);
  }
  for (const declaration of program.implementations) {
    add(declaration.decorators?.facts, "Impl");
    methods(declaration.methods);
  }
  for (const declaration of program.types ?? [])
    if (declaration.base) add(declaration.decorators?.facts, undefined);
  return result;
}

// ---------------------------------------------------------------------------
// Bare markers (annot.decorator.bare-call).

function called(fact: Expression, markers: ReadonlySet<string>): Expression {
  if (fact.kind !== "name" || !markers.has(fact.name)) return fact;
  return { kind: "call", callee: fact, arguments: [], span: fact.span };
}

function calledAll(
  facts: readonly Expression[] | undefined,
  markers: ReadonlySet<string>,
): readonly Expression[] | undefined {
  return facts?.map((fact) => called(fact, markers));
}

function withDecorators<T extends { readonly decorators?: Decorators }>(
  item: T,
  markers: ReadonlySet<string>,
): T {
  const decorators = item.decorators;
  if (!decorators) return item;
  return { ...item, decorators: { ...decorators, facts: calledAll(decorators.facts, markers)! } };
}

function withParameters<T extends { readonly parameters: readonly Parameter[] }>(
  item: T,
  markers: ReadonlySet<string>,
): T {
  return {
    ...item,
    parameters: item.parameters.map((parameter) =>
      parameter.metadata
        ? { ...parameter, metadata: calledAll(parameter.metadata, markers) }
        : parameter,
    ),
  };
}

function withMethods<T extends { readonly methods: readonly MethodDecl[] }>(
  item: T,
  markers: ReadonlySet<string>,
): T {
  return {
    ...item,
    methods: item.methods.map((method) => withParameters(withDecorators(method, markers), markers)),
  };
}

/**
 * Calls every bare decorator name in `markers`, the names of functions with
 * no parameters (annot.decorator.bare-call). Nothing outside a decorator
 * changes (annot.decorator.bare-call.only).
 */
export function withBareMarkerCalls(program: Program, markers: ReadonlySet<string>): Program {
  if (markers.size === 0) return program;
  const fields = <T extends DataDecl["fields"][number]>(field: T): T =>
    field.metadata ? { ...field, metadata: calledAll(field.metadata, markers) } : field;
  return {
    ...program,
    functions: program.functions.map((item) =>
      withParameters(withDecorators(item, markers), markers),
    ),
    data: program.data.map((item) => ({
      ...withDecorators(item, markers),
      fields: item.fields.map(fields),
    })),
    enums: program.enums.map((item): EnumDecl => ({
      ...withDecorators(item, markers),
      variants: item.variants.map((variant) => ({
        ...variant,
        ...(variant.metadata ? { metadata: calledAll(variant.metadata, markers) } : {}),
        fields: variant.fields.map(fields),
      })),
    })),
    traits: program.traits.map((item) => withMethods(withDecorators(item, markers), markers)),
    implementations: program.implementations.map((item) =>
      withMethods(withDecorators(item, markers), markers),
    ),
    ...(program.types ? { types: program.types.map((item) => withDecorators(item, markers)) } : {}),
  };
}

/** The program's own functions that take no parameters. */
export function markerFunctions(functions: readonly FunctionDecl[]): Set<string> {
  return new Set(
    functions
      .filter((declaration) => declaration.parameters.length === 0)
      .map((declaration) => declaration.name),
  );
}

// ---------------------------------------------------------------------------
// Target kinds (annot.target.*).

function baseTypeName(type: string): string {
  return type.split("[")[0] ?? type;
}

/** The kinds that an `@annotate(...)` value lists, read from its written arguments. */
function listedKinds(fact: Expression): Set<string> | undefined {
  const kinds = new Set<string>();
  const add = (argument: Expression): boolean => {
    if (argument.kind === "contextual-variant") kinds.add(argument.name);
    else if (argument.kind === "member") kinds.add(argument.name);
    else if (argument.kind === "list") return argument.elements.every(add);
    else return false;
    return true;
  };
  if (fact.kind === "call") return fact.arguments.every(add) ? kinds : undefined;
  if (fact.kind === "data") {
    const field = fact.fields.find((item) => item.name === "kinds");
    return field && add(field.value) ? kinds : undefined;
  }
  return undefined;
}

/**
 * `decorator-not-annotator` for each value attached to a kind of target
 * that its type's `@annotate` fact does not list (annot.target.limit.error,
 * annot.target.newtype).
 */
export function checkDecoratorTargets(program: Program): Diagnostic[] {
  const functions = new Map(program.functions.map((item) => [item.name, item] as const));
  const types = new Map<string, DataDecl | EnumDecl>([
    ...program.data.map((item) => [item.name, item] as const),
    ...program.enums.map((item) => [item.name, item] as const),
  ]);
  const typeOf = (fact: Expression): DataDecl | EnumDecl | undefined =>
    types.get(baseTypeName(factType(fact, functions)));
  const limits = new Map<string, Set<string> | undefined>();
  const limitOf = (declaration: DataDecl | EnumDecl): Set<string> | undefined => {
    if (limits.has(declaration.name)) return limits.get(declaration.name);
    limits.set(declaration.name, undefined);
    let found: Set<string> | undefined;
    for (const fact of declaration.decorators?.facts ?? []) {
      const type = typeOf(fact);
      if (type?.kind !== "data" || type.standardName !== STANDARD_ANNOTATE) continue;
      found = listedKinds(fact);
    }
    limits.set(declaration.name, found);
    return found;
  };
  const diagnostics: Diagnostic[] = [];
  for (const { fact, kind } of attachedValues(program)) {
    const type = typeOf(fact);
    if (!type) continue;
    const limit = limitOf(type);
    if (!limit || (kind !== undefined && limit.has(kind))) continue;
    const listed = [...limit].map((item) => `.${item}`).join(", ");
    const name = type.kind === "data" ? (type.standardName ?? type.name) : type.name;
    diagnostics.push({
      code: "decorator-not-annotator",
      message:
        kind === undefined
          ? `a '${name}' value may be attached only to ${listed}; a newtype has no target kind`
          : `a '${name}' value may be attached only to ${listed}, not to a .${kind} target`,
      span: fact.span,
    });
  }
  return diagnostics;
}
