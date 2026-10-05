import type {
  DataDecl,
  EnumDecl,
  FunctionDecl,
  GenericBound,
  ImplDecl,
  MethodDecl,
  Program,
  TraitDecl,
  TypeDecl,
  TypeRef,
} from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import {
  contextKeys,
  contextType,
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  rowArgumentKeys,
  rowArgumentType,
  splitTypeBindings,
  tupleParts,
  tupleType,
  displayType,
} from "../types.ts";
import type { ProgramCheckContext } from "./program-context.ts";
import {
  collectRowParameterReferences,
  matchTraitImplementation,
  resolveGenericType,
  resolveTraitType,
} from "./shared.ts";
import { displayName } from "./display-names.ts";
import { INSPECTABLE } from "./standard-traits.ts";
import { substitute, words } from "./type-declarations.ts";
import { ImportBindingMap } from "./import-bindings.ts";

// Type-argument defaults in written types (04-type-system.md#type-argument-defaults).
// A written type, such as an annotation, a signature, a field, a bound, or an
// implementation header, infers nothing: each trailing slot it omits takes
// its default, with the earlier arguments and `Self` substituted
// (types.generic.default.written). This pass fills those slots before the
// checker runs, so the rest of the checker sees complete argument lists.
// A use site that infers, such as a call, applies defaults after inference
// instead (checker/calls.ts).

/** A generic declaration's parameters and the defaults written for them. */
interface GenericShape {
  readonly parameters: readonly string[];
  readonly defaults: ReadonlyMap<string, string>;
}

/** The built-in generic types, which declare no defaults. */
const BUILTIN_SHAPES: ReadonlyMap<string, GenericShape> = new Map([
  ["List", { parameters: ["T"], defaults: new Map() }],
  ["Map", { parameters: ["K", "V"], defaults: new Map() }],
  ["Suspend", { parameters: ["T"], defaults: new Map() }],
  ["Result", { parameters: ["T", "E"], defaults: new Map() }],
]);

type GenericDeclaration = DataDecl | EnumDecl | TraitDecl | TypeDecl;

function shapeOf(declaration: {
  readonly genericParameters: readonly string[];
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
}): GenericShape {
  return {
    parameters: declaration.genericParameters,
    defaults: new Map(
      Object.entries(declaration.genericDefaults ?? {}).map(([name, type]) => [name, type.name]),
    ),
  };
}

function isTypeRef(value: unknown): value is TypeRef {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    typeof (value as { name?: unknown }).name === "string" &&
    keys.includes("span") &&
    keys.every((key) => key === "name" || key === "span" || key === "written")
  );
}

const TYPE_KEYS = new Set(["type", "result", "annotation", "value", "alias", "base"]);
const TYPE_LIST_KEYS = new Set(["typeArguments", "ownerTypeArguments"]);

class DefaultFiller {
  private readonly shapes: ReadonlyMap<string, GenericShape>;
  readonly diagnostics: Diagnostic[] = [];

  constructor(shapes: ReadonlyMap<string, GenericShape>) {
    this.shapes = shapes;
  }

  private mentions(type: string, scope: ReadonlySet<string>): boolean {
    return words(type).some((word) => this.shapes.has(word) && !scope.has(word));
  }

  /**
   * A written type with every omitted trailing slot filled. `self` is the
   * type `Self` stands for in a default, or undefined where none is known,
   * as in a trait value type (trait.dyn.generic-trait.self-default). A
   * generic parameter in `scope` shadows a declaration of its name.
   */
  type(type: string, span: SourceSpan, scope: ReadonlySet<string>, self?: string): string {
    if (!this.mentions(type, scope)) return type;
    const visit = (inner: string): string => this.type(inner, span, scope);
    const mutable = mutableInner(type);
    if (mutable !== undefined) return mutableType(this.type(mutable, span, scope, self));
    const rowArgument = rowArgumentKeys(type);
    if (rowArgument) return rowArgumentType(rowArgument.map(visit));
    const context = contextKeys(type);
    if (context) return contextType(context.map(visit));
    const optional = optionalInner(type);
    if (optional !== undefined) return optionalType(visit(optional));
    const tuple = tupleParts(type);
    if (tuple !== undefined) return tupleType(tuple.map(visit));
    const callable = functionParts(type);
    if (callable)
      return functionType(
        callable.parameters.map(visit),
        visit(callable.result),
        callable.requirements.map(visit),
        callable.variadic,
        callable.suspending,
      );
    const nominal = nominalGenericParts(type);
    const name = nominal?.name ?? type;
    const shape = scope.has(name) ? undefined : this.shapes.get(name);
    const { positional, bindings } = splitTypeBindings(nominal?.arguments ?? []);
    const written = positional.map(visit);
    const bound = bindings.map((binding) => `${binding.name}=${visit(binding.type)}`);
    if (!shape) return nominal ? nominalGenericType(name, [...written, ...bound]) : type;
    if (written.length > shape.parameters.length) {
      this.diagnostics.push({
        code: "argument-count",
        message: `'${name}' has ${shape.parameters.length} generic parameters, but ${written.length} type arguments are written`,
        span,
      });
      return type;
    }
    const filled = [...written];
    for (let index = written.length; index < shape.parameters.length; index += 1) {
      const parameter = shape.parameters[index]!;
      const fallback = shape.defaults.get(parameter);
      const needsSelf = fallback !== undefined && words(fallback).includes("Self");
      if (fallback === undefined || (needsSelf && self === undefined)) {
        this.diagnostics.push({
          code: "partial-generic-arguments",
          message:
            fallback === undefined
              ? `'${displayName(name)}' needs a type argument for '${displayType(parameter)}', which has no default`
              : `'${displayName(name)}' omits '${displayType(parameter)}', whose default names Self, and no Self is known here`,
          span,
        });
        return type;
      }
      const substitutions = new Map(
        shape.parameters.slice(0, index).map((earlier, position) => [earlier, filled[position]!]),
      );
      if (self !== undefined) substitutions.set("Self", self);
      filled.push(visit(substitute(fallback, substitutions)));
    }
    return filled.length + bound.length === 0
      ? name
      : nominalGenericType(name, [...filled, ...bound]);
  }
}

/** Fills the bound traits of one parameter, where `Self` is that parameter. */
function fillBound(
  bound: GenericBound,
  filler: DefaultFiller,
  span: SourceSpan,
  scope: ReadonlySet<string>,
): GenericBound {
  const renamed = new Map<string, string>();
  const traits = bound.traits.map((trait) => {
    const inner = mutableInner(trait);
    const filled = filler.type(inner ?? trait, bound.span ?? span, scope, bound.parameter);
    renamed.set(inner ?? trait, filled);
    return inner === undefined ? filled : mutableType(filled);
  });
  return {
    ...bound,
    traits,
    ...(bound.bindings
      ? {
          bindings: bound.bindings.map((binding) => ({
            ...binding,
            trait: renamed.get(binding.trait) ?? binding.trait,
            type: {
              ...binding.type,
              name: filler.type(binding.type.name, binding.type.span, scope),
            },
          })),
        }
      : {}),
  };
}

/** The scope inside a node: a declaration's generic parameters shadow type names. */
function innerScope(
  record: Record<string, unknown>,
  scope: ReadonlySet<string>,
): ReadonlySet<string> {
  const parameters = record.genericParameters;
  return Array.isArray(parameters) && parameters.length > 0
    ? new Set([...scope, ...(parameters as string[])])
    : scope;
}

function fillNode<T>(
  node: T,
  filler: DefaultFiller,
  span?: SourceSpan,
  outer: ReadonlySet<string> = new Set(),
): T {
  if (Array.isArray(node)) return node.map((item) => fillNode(item, filler, span, outer)) as T;
  if (!node || typeof node !== "object") return node;
  const record = node as Record<string, unknown>;
  const own = (record.span as SourceSpan | undefined) ?? span;
  const scope = innerScope(record, outer);
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(record)) {
    if (key === "span" || key === "genericDefaults") result[key] = value;
    else if (TYPE_KEYS.has(key) && isTypeRef(value))
      result[key] = { ...value, name: filler.type(value.name, value.span, scope) };
    else if (TYPE_LIST_KEYS.has(key) && Array.isArray(value))
      result[key] = value.map((item) =>
        isTypeRef(item) ? { ...item, name: filler.type(item.name, item.span, scope) } : item,
      );
    else if (key === "genericBounds" && Array.isArray(value) && own)
      result[key] = (value as GenericBound[]).map((bound) => fillBound(bound, filler, own, scope));
    else if (key === "requirements" && Array.isArray(value) && own)
      result[key] = (value as string[]).map((requirement) => filler.type(requirement, own, scope));
    else if (
      key === "key" &&
      typeof value === "string" &&
      own &&
      (record.kind === "provider-use" || record.kind === "binding")
    )
      result[key] = filler.type(value, own, scope);
    else result[key] = fillNode(value, filler, own, scope);
  }
  return result as T;
}

/** A trait's supertrait list, where `Self` is the trait's own `Self`. */
function fillTrait(declaration: TraitDecl, filler: DefaultFiller): TraitDecl {
  const scope = new Set(declaration.genericParameters);
  const renamed = new Map<string, string>();
  const supertraits = declaration.supertraits.map((supertrait) => {
    const name = filler.type(supertrait.name, supertrait.span, scope, "Self");
    renamed.set(supertrait.name, name);
    return { ...supertrait, name };
  });
  const filled = fillNode({ ...declaration, supertraits: [] }, filler);
  return {
    ...filled,
    supertraits,
    ...(declaration.supertraitBindings
      ? {
          supertraitBindings: declaration.supertraitBindings.map((binding) => ({
            ...binding,
            trait: renamed.get(binding.trait) ?? binding.trait,
            type: {
              ...binding.type,
              name: filler.type(binding.type.name, binding.type.span, scope),
            },
          })),
        }
      : {}),
  };
}

/** An implementation header, where `Self` in the trait's defaults is the target. */
function fillImplementation(declaration: ImplDecl, filler: DefaultFiller): ImplDecl {
  const filled = fillNode(declaration, filler);
  const scope = new Set(declaration.genericParameters);
  return {
    ...filled,
    targetName: filler.type(declaration.targetName, declaration.span, scope),
    ...(declaration.traitName !== undefined
      ? {
          traitName: filler.type(
            declaration.traitName,
            declaration.span,
            scope,
            declaration.targetName,
          ),
        }
      : {}),
  };
}

interface DefaultedList {
  readonly parameters: readonly string[];
  readonly defaults?: Readonly<Record<string, TypeRef>>;
  /** Types and rows of the declaration that decide a parameter's kind. */
  readonly kindTypes: readonly string[];
  readonly requirements?: readonly string[];
  readonly span: SourceSpan;
}

function defaultedLists(program: Program): DefaultedList[] {
  const lists: DefaultedList[] = [];
  const callable = (declaration: FunctionDecl | MethodDecl): void => {
    lists.push({
      parameters: declaration.genericParameters,
      defaults: declaration.genericDefaults,
      kindTypes: [
        ...declaration.parameters.map((parameter) => parameter.type.name),
        declaration.result.name,
      ],
      requirements: declaration.requirements,
      span: declaration.span,
    });
  };
  const declared = (declaration: GenericDeclaration, kindTypes: readonly string[]): void => {
    lists.push({
      parameters: declaration.genericParameters,
      defaults: declaration.genericDefaults,
      kindTypes,
      span: declaration.span,
    });
  };
  program.functions.forEach(callable);
  program.implementations.forEach((implementation) => implementation.methods.forEach(callable));
  program.traits.forEach((trait) => {
    trait.methods.forEach(callable);
    declared(trait, []);
  });
  program.data.forEach((data) =>
    declared(
      data,
      data.fields.map((field) => field.type.name),
    ),
  );
  program.enums.forEach((declaration) =>
    declared(declaration, [
      ...declaration.sharedFields.map((field) => field.type.name),
      ...declaration.variants.flatMap((variant) => variant.fields.map((field) => field.type.name)),
    ]),
  );
  (program.types ?? []).forEach((declaration) =>
    declared(declaration, [
      ...(declaration.alias ? [declaration.alias.name] : []),
      ...(declaration.base ? [declaration.base.name] : []),
    ]),
  );
  return lists.filter((list) => list.defaults !== undefined);
}

/**
 * Default order, the names a default may use, and its kind
 * (types.generic.default.order, .later, .kind).
 */
function defaultDeclarationDiagnostics(program: Program): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  for (const list of defaultedLists(program)) {
    const defaults = list.defaults ?? {};
    const firstDefault = list.parameters.findIndex((parameter) => parameter in defaults);
    const missing = list.parameters.findIndex(
      (parameter, index) => index > firstDefault && !(parameter in defaults),
    );
    if (firstDefault >= 0 && missing >= 0) {
      diagnostics.push({
        code: "default-order",
        message: `generic parameter '${list.parameters[missing]}' follows a defaulted parameter, so it needs a default too`,
        span: list.span,
      });
      continue;
    }
    const rows = new Set<string>();
    for (const requirement of list.requirements ?? [])
      if (list.parameters.includes(requirement)) rows.add(requirement);
    list.kindTypes.forEach((type) =>
      collectRowParameterReferences(type, new Set(list.parameters), rows),
    );
    list.parameters.forEach((parameter, index) => {
      const fallback = defaults[parameter];
      if (!fallback) return;
      const later = words(fallback.name).find((word) =>
        list.parameters.slice(index).includes(word),
      );
      if (later !== undefined) {
        diagnostics.push({
          code: "binding-not-yet-visible",
          message: `the default of '${displayType(parameter)}' names '${later}', which is not declared before it`,
          span: fallback.span,
        });
        return;
      }
      if (rowArgumentKeys(fallback.name) !== undefined && !rows.has(parameter))
        diagnostics.push({
          code: "generic-kind-mismatch",
          message: `'${displayType(parameter)}' is a type parameter, so its default must be a type, not the row '${displayType(fallback.name)}'`,
          span: fallback.span,
        });
    });
  }
  return diagnostics;
}

/**
 * Fills the defaults a written type omits and checks each declared default's
 * order, names, and kind. Row aliases take no defaults in the prototype.
 */
export function withTypeDefaults(
  program: Program,
  aliases: ReadonlyMap<string, string> = new Map(),
): {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
} {
  const shapes = new ImportBindingMap<GenericShape>(aliases, [...BUILTIN_SHAPES]);
  const declarations: GenericDeclaration[] = [
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...(program.types ?? []).filter((declaration) => declaration.row === undefined),
  ];
  for (const declaration of declarations)
    if (declaration.genericParameters.length > 0)
      shapes.set(declaration.name, shapeOf(declaration));
  const diagnostics = defaultDeclarationDiagnostics(program);
  if (diagnostics.length > 0) return { program, diagnostics };
  const filler = new DefaultFiller(shapes);
  const { traits, implementations, ...rest } = program;
  const filled: Program = {
    ...fillNode(rest, filler),
    traits: traits.map((trait) => fillTrait(trait, filler)),
    implementations: implementations.map((implementation) =>
      fillImplementation(implementation, filler),
    ),
  } as Program;
  return { program: filled, diagnostics: filler.diagnostics };
}

interface BoundedDefaults {
  readonly generics: ReadonlySet<string>;
  readonly bounds: readonly GenericBound[];
  readonly defaults: Readonly<Record<string, TypeRef>>;
}

function boundedDefaults(program: Program): BoundedDefaults[] {
  const lists: BoundedDefaults[] = [];
  const add = (
    declaration: {
      readonly genericParameters: readonly string[];
      readonly genericBounds?: readonly GenericBound[];
      readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
    },
    enclosing: readonly string[] = [],
  ): void => {
    if (!declaration.genericDefaults || !declaration.genericBounds?.length) return;
    lists.push({
      generics: new Set([...enclosing, ...declaration.genericParameters]),
      bounds: declaration.genericBounds,
      defaults: declaration.genericDefaults,
    });
  };
  program.functions.forEach((declaration) => add(declaration));
  program.implementations.forEach((implementation) =>
    implementation.methods.forEach((method) => add(method, implementation.genericParameters)),
  );
  program.traits.forEach((trait) => {
    add(trait);
    trait.methods.forEach((method) => add(method, trait.genericParameters));
  });
  [...program.data, ...program.enums].forEach((declaration) => add(declaration));
  return lists;
}

/**
 * A default must satisfy its parameter's bounds for every instantiation of
 * the earlier parameters, checked once at the declaration
 * (types.generic.default.bound, types.generic.default.checked-once). The
 * prototype reports a declared data or enum default that no implementation
 * of a bound trait matches; other defaults are checked where they apply.
 */
export function defaultBoundDiagnostics(context: ProgramCheckContext): Diagnostic[] {
  const { dataTypes, enumTypes, traitTypes, implementationPreparations } = context;
  const diagnostics: Diagnostic[] = [];
  for (const list of boundedDefaults(context.program)) {
    for (const bound of list.bounds) {
      const fallback = list.defaults[bound.parameter];
      if (!fallback) continue;
      const type = resolveTraitType(resolveGenericType(fallback.name, list.generics), traitTypes);
      const head = nominalGenericParts(type)?.name ?? type;
      if (!dataTypes.has(head) && !enumTypes.has(head)) continue;
      for (const written of bound.traits) {
        const traitKey = mutableInner(written) ?? written;
        const application = nominalGenericParts(traitKey);
        const trait = traitTypes.get(application?.name ?? traitKey);
        if (!trait || trait.name === INSPECTABLE) continue;
        const traitArguments = (application?.arguments ?? []).map((argument) =>
          resolveTraitType(resolveGenericType(argument, list.generics), traitTypes),
        );
        const implemented = implementationPreparations.some((candidate) =>
          Boolean(matchTraitImplementation(candidate, trait.index, type, traitArguments)),
        );
        if (!implemented)
          diagnostics.push({
            code: "unsatisfied-trait-bound",
            message: `the default '${displayType(fallback.name)}' of '${displayType(bound.parameter)}' does not implement ${displayType(traitKey)}`,
            span: fallback.span,
          });
      }
    }
  }
  return diagnostics;
}
