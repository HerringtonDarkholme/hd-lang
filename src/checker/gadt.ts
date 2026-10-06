import type { EnumDecl, EnumVariant, GenericBound, TypeRef } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type {
  HirDataField,
  HirEnum,
  HirEnumVariant,
  HirGenericBound,
  HirTrait,
  ValueType,
} from "../hir.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
  displayType,
} from "../types.ts";
import { resolveGenericType, substituteGenericType } from "./shared.ts";

// GADT-style enum variants (spec/lang/13-gadts.md): a variant's explicit
// result, its variant-local parameters, construction, and the arm-local
// refinement that matching a variant introduces.

type VariantGadt = NonNullable<HirEnumVariant["gadt"]>;

/** Whether `type` mentions the type parameter `name` anywhere. */
export function mentionsParameter(type: ValueType, name: string): boolean {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`generic:${escaped}(?![A-Za-z0-9_])`).test(type);
}

/**
 * The declaration's own parameters as type arguments: the result of a
 * variant without an explicit result (13-gadts.md#r-gadt.result.default).
 */
export function declarationArguments(declaration: HirEnum): readonly ValueType[] {
  return declaration.genericParameters.map((parameter) => `generic:${parameter}`);
}

/** A variant's constructor type variables and its result's type arguments over them. */
export function variantShape(
  declaration: HirEnum,
  variant: HirEnumVariant,
): { readonly variables: readonly string[]; readonly resultArguments: readonly ValueType[] } {
  return variant.gadt
    ? variant.gadt
    : {
        variables: declaration.genericParameters,
        resultArguments: declarationArguments(declaration),
      };
}

/** The type a variant constructs once its variables are solved. */
export function variantResultType(
  declaration: HirEnum,
  variant: HirEnumVariant,
  substitutions: ReadonlyMap<string, ValueType>,
): ValueType {
  const { resultArguments } = variantShape(declaration, variant);
  return declaration.genericParameters.length > 0
    ? nominalGenericType(
        declaration.name,
        resultArguments.map((argument) => substituteGenericType(argument, substitutions)),
      )
    : declaration.name;
}

/** The type arguments of a subject of enum `declaration`'s type, or undefined for another type. */
export function subjectArguments(
  declaration: HirEnum,
  subject: ValueType,
): readonly ValueType[] | undefined {
  const plain = readonlyType(subject);
  if (plain === declaration.name) return [];
  const nominal = nominalGenericParts(plain);
  return nominal?.name === declaration.name ? nominal.arguments : undefined;
}

interface VariantResolution {
  readonly declaration: EnumDecl;
  readonly variant: EnumVariant;
  /** The variant's resolved payload fields. */
  readonly fields: readonly HirDataField[];
  readonly traitTypes: ReadonlyMap<string, HirTrait>;
  readonly diagnostics: Diagnostic[];
  /** Resolves a written type with these generic parameters in scope. */
  readonly resolve: (type: TypeRef, generics: ReadonlySet<string>) => ValueType | undefined;
  /** Adds a hidden enum field of `type` and returns its index. */
  readonly addField: (name: string, type: ValueType, span: TypeRef["span"]) => number;
}

/**
 * The GADT shape of a variant, or undefined for a variant that constructs
 * its enum with the declaration's own arguments (13-gadts.md#variant-result-types).
 * A result whose outer type is another type is reported as
 * `variant-result-owner` where shared data is planned, so it has no shape.
 */
export function resolveVariantGadt(resolution: VariantResolution): VariantGadt | undefined {
  const { declaration, variant } = resolution;
  const local = variant.genericParameters ?? [];
  const declared = declaration.genericParameters;
  const scope = new Set([...declared, ...local]);
  let resultArguments: readonly ValueType[];
  // A result naming the enum without type arguments, as `Box(1)`, constructs
  // it with the declaration's own arguments (13-gadts.md#r-gadt.result.default).
  if (variant.resultType && variant.resultType.name.trim() !== declaration.name) {
    const resolved = resolution.resolve(variant.resultType, scope);
    if (resolved === undefined) return undefined;
    const plain = readonlyType(resolved);
    const nominal = nominalGenericParts(plain);
    const owner = nominal?.name ?? plain;
    if (owner !== declaration.name) return undefined;
    resultArguments = nominal?.arguments ?? [];
    if (resultArguments.length !== declared.length) return undefined;
  } else {
    if (local.length === 0) return undefined;
    resultArguments = declared.map((parameter) => `generic:${parameter}`);
  }
  const shadowed = new Set(local);
  const exact = resultArguments.every(
    (argument, index) =>
      argument === `generic:${declared[index]!}` && !shadowed.has(declared[index]!),
  );
  if (exact && local.length === 0) return undefined;
  const mentioned = (name: string): boolean =>
    resultArguments.some((argument) => mentionsParameter(argument, name)) ||
    resolution.fields.some((field) => mentionsParameter(field.type, name));
  const variables = [
    ...declared.filter((parameter) => !shadowed.has(parameter) && mentioned(parameter)),
    ...local,
  ];
  const bounds = resolveVariantBounds(
    variant.genericBounds ?? [],
    new Set(variables),
    resolution.traitTypes,
    resolution.diagnostics,
  );
  const existential = new Set(
    local.filter(
      (parameter) => !resultArguments.some((argument) => mentionsParameter(argument, parameter)),
    ),
  );
  const evidence = bounds.flatMap((bound, index) =>
    existential.has(bound.parameter)
      ? [
          {
            bound: index,
            fieldIndex: resolution.addField(
              `$evidence.${variant.name}.${index}`,
              boundTraitType(bound),
              variant.span,
            ),
          },
        ]
      : [],
  );
  return { variables, localParameters: local, resultArguments, bounds, evidence };
}

/** The trait value type of a bound's dictionary, as `trait:Display`. */
export function boundTraitType(bound: HirGenericBound): ValueType {
  return `trait:${
    bound.traitArguments.length > 0
      ? nominalGenericType(bound.traitName, bound.traitArguments)
      : bound.traitName
  }`;
}

function resolveVariantBounds(
  bounds: readonly GenericBound[],
  parameters: ReadonlySet<string>,
  traitTypes: ReadonlyMap<string, HirTrait>,
  diagnostics: Diagnostic[],
): HirGenericBound[] {
  return bounds.flatMap((bound) =>
    bound.traits.flatMap((written): HirGenericBound[] => {
      const inner = mutableInner(written);
      const key = inner ?? written;
      const application = nominalGenericParts(key);
      const name = application?.name ?? key;
      if (name === "Any" || name === "AnyRef" || name === "AnyVal") return [];
      const trait = traitTypes.get(name);
      if (!trait) {
        diagnostics.push({
          code: "unknown-trait",
          message: `unknown trait '${displayType(name)}'`,
          span: bound.span,
        });
        return [];
      }
      return [
        {
          parameter: bound.parameter,
          traitName: trait.name,
          traitIndex: trait.index,
          traitArguments: (application?.arguments ?? []).map((argument) =>
            resolveGenericType(argument, parameters),
          ),
          mutable: inner !== undefined,
        },
      ];
    }),
  );
}

/** The arm-local facts of a matched GADT variant (13-gadts.md#refinement-algorithm). */
export interface VariantRefinement {
  /** Each variable of the variant's constructor, solved for the payload types. */
  readonly substitutions: ReadonlyMap<string, ValueType>;
  /** Equalities on the type parameters in scope, as `T = i64`. */
  readonly equalities: ReadonlyMap<string, ValueType>;
  /** The fresh parameters of the variant's unsolved variables, by variable. */
  readonly existentials: ReadonlyMap<string, string>;
}

const VARIABLE = "%";

interface TypeShape {
  readonly head: string;
  readonly children: readonly ValueType[];
}

function shapeOf(type: ValueType): TypeShape {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return { head: "mut", children: [mutable] };
  const tuple = tupleParts(type);
  if (tuple !== undefined) return { head: `tuple/${tuple.length}`, children: tuple };
  const optional = optionalInner(type);
  if (optional !== undefined) return { head: "optional", children: [optional] };
  const result = resultParts(type);
  if (result) return { head: "result", children: [result.ok, result.error] };
  const callable = functionParts(type);
  if (callable)
    return {
      head: `fn/${callable.suspending}/${callable.variadic}/${callable.parameters.length}/${callable.requirements.join("+")}`,
      children: [...callable.parameters, callable.result],
    };
  const nominal = nominalGenericParts(type);
  if (nominal)
    return { head: `${nominal.name}/${nominal.arguments.length}`, children: nominal.arguments };
  return { head: type, children: [] };
}

function parameterName(type: ValueType): string | undefined {
  const match = /^generic:([^?[\](),]+)$/.exec(type);
  return match?.[1];
}

/**
 * First-order nominal unification of a subject's type arguments with a
 * variant's result arguments (13-gadts.md#r-gadt.unify.first-order). The
 * variant's variables and the subject's type parameters may be solved; a
 * solved subject parameter becomes an arm-local equality. Returns undefined
 * when they cannot unify, so the variant cannot inhabit the subject's type.
 */
export function refineVariant(
  subject: readonly ValueType[],
  shape: { readonly variables: readonly string[]; readonly resultArguments: readonly ValueType[] },
  fresh: (name: string) => string,
): VariantRefinement | undefined {
  const renamed = new Map(
    shape.variables.map((variable) => [variable, `generic:${VARIABLE}${variable}`]),
  );
  const bound = new Map<string, ValueType>();
  const root = (type: ValueType): ValueType => {
    let current = type;
    for (let depth = 0; depth < 256; depth += 1) {
      const name = parameterName(current);
      const next = name === undefined ? undefined : bound.get(name);
      if (next === undefined) return current;
      current = next;
    }
    return current;
  };
  const resolved = (type: ValueType, depth = 0): ValueType => {
    if (depth > 64) return type;
    const substitutions = new Map(
      [...bound].map(([name, value]) => [name, resolved(value, depth + 1)] as const),
    );
    return substituteGenericType(type, substitutions);
  };
  const unify = (left: ValueType, right: ValueType): boolean => {
    const a = root(left);
    const b = root(right);
    if (a === b) return true;
    const aName = parameterName(a);
    const bName = parameterName(b);
    // A variant variable binds first, so a subject parameter stays itself.
    const variable = aName?.startsWith(VARIABLE)
      ? aName
      : bName?.startsWith(VARIABLE)
        ? bName
        : (aName ?? bName);
    if (variable !== undefined) {
      const other = variable === aName ? b : a;
      if (mentionsParameter(resolved(other), variable)) return false;
      bound.set(variable, other);
      return true;
    }
    const aShape = shapeOf(a);
    const bShape = shapeOf(b);
    if (aShape.head !== bShape.head || aShape.children.length !== bShape.children.length)
      return false;
    return aShape.children.every((child, index) => unify(child, bShape.children[index]!));
  };
  const variantArguments = shape.resultArguments.map((argument) =>
    substituteGenericType(argument, renamed),
  );
  if (subject.length !== variantArguments.length) return undefined;
  if (!subject.every((argument, index) => unify(argument, variantArguments[index]!)))
    return undefined;
  const existentials = new Map<string, string>();
  for (const variable of shape.variables) {
    const value = resolved(renamed.get(variable)!);
    const name = parameterName(value);
    if (name?.startsWith(VARIABLE) && !existentials.has(name.slice(1)))
      existentials.set(name.slice(1), fresh(name.slice(1)));
  }
  const closing = new Map(
    [...existentials].map(([variable, name]) => [`${VARIABLE}${variable}`, `generic:${name}`]),
  );
  const final = (type: ValueType): ValueType => substituteGenericType(resolved(type), closing);
  const substitutions = new Map(
    shape.variables.map((variable) => [variable, final(renamed.get(variable)!)] as const),
  );
  const equalities = new Map<string, ValueType>();
  for (const name of bound.keys())
    if (!name.startsWith(VARIABLE)) equalities.set(name, final(`generic:${name}`));
  return { substitutions, equalities, existentials };
}

/** Whether written type `text` names `name` as a whole word. */
function namesWord(text: string, name: string): boolean {
  return new RegExp(`(?<![A-Za-z0-9_.])${name}(?![A-Za-z0-9_])`).test(text);
}

/** Written type `text` with each whole-word name in `map` replaced. */
export function substituteWritten(text: string, map: ReadonlyMap<string, string>): string {
  return text.replace(/(?<![A-Za-z0-9_.])[A-Za-z_][A-Za-z0-9_]*/g, (word) => map.get(word) ?? word);
}

/**
 * The GADT shape of a variant as written, before types resolve: its
 * constructor's variables and its result's written type arguments. Undefined
 * for a variant that constructs its enum with the declaration's own
 * arguments (13-gadts.md#r-gadt.result.default).
 */
export function writtenVariantGadt(
  declaration: EnumDecl,
  variant: EnumVariant,
):
  | { readonly variables: readonly string[]; readonly resultArguments: readonly string[] }
  | undefined {
  const local = variant.genericParameters ?? [];
  const declared = declaration.genericParameters;
  const written = variant.resultType?.name.replace(/\s+/g, " ").trim();
  const nominal = written === undefined ? undefined : nominalGenericParts(written);
  const resultArguments =
    written === undefined || nominal === undefined
      ? declared
      : nominal.arguments.map((argument) => argument.trim());
  const shadowed = new Set(local);
  const exact =
    resultArguments.length === declared.length &&
    resultArguments.every(
      (argument, index) => argument === declared[index] && !shadowed.has(argument),
    );
  if (exact && local.length === 0) return undefined;
  const texts = [...resultArguments, ...variant.fields.map((field) => field.type.name)];
  const variables = [
    ...declared.filter(
      (parameter) => !shadowed.has(parameter) && texts.some((text) => namesWord(text, parameter)),
    ),
    ...local,
  ];
  return { variables, resultArguments };
}
