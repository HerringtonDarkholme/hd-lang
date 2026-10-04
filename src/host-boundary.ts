import type { HirData, HirEnum, HirProgram, ValueType } from "./hir.ts";
import { NUMERIC_TYPES } from "./numeric.ts";
import { nominalGenericParts, optionalInner, tupleParts } from "./types.ts";

export interface PayloadlessEnumValue {
  readonly enumIndex: number;
  readonly tag: number;
}

/**
 * Resolve an enum whose one possible value needs no host payload. The host
 * result tag selects the Result side; this identity supplies the nested enum.
 */
export function payloadlessSingletonEnum(
  enums: readonly HirEnum[],
  type: ValueType,
): PayloadlessEnumValue | undefined {
  const name = nominalGenericParts(type)?.name ?? type;
  const declaration = enums.find((item) => item.name === name);
  const variant = declaration?.variants.length === 1 ? declaration.variants[0] : undefined;
  return declaration &&
    variant &&
    declaration.sharedFields.length === 0 &&
    variant.fields.length === 0
    ? { enumIndex: declaration.index, tag: variant.tag }
    : undefined;
}

/**
 * The scalar leaf of a host boundary type: a `bool`, `char`, or numeric
 * type. `string` is not one: it crosses as UTF-8 bytes, so the shape
 * resolver below gives it its own kind.
 */
const BOUNDARY_SCALARS = new Set<ValueType>(["bool", "char", ...NUMERIC_TYPES.keys()]);

export function isBoundaryScalar(type: ValueType): boolean {
  return BOUNDARY_SCALARS.has(type);
}

/**
 * One step of the host boundary type walk (scalar, `string`, `T?`, tuple,
 * `List`, data over a declaration lookup, anything else): the single
 * dispatch shared by the checker's capability validation, the runtime's
 * check/encode/decode, and the emitter's structural decoder. Generic
 * substitution over a data shape's `arguments` stays with the caller, next
 * to the recursion each site already owns.
 */
export type BoundaryShape =
  | { readonly kind: "scalar"; readonly type: ValueType }
  | { readonly kind: "string" }
  | { readonly kind: "optional"; readonly inner: ValueType }
  | { readonly kind: "tuple"; readonly elements: readonly ValueType[] }
  | { readonly kind: "list"; readonly element: ValueType }
  | {
      readonly kind: "data";
      readonly declaration: HirData;
      readonly arguments: readonly ValueType[];
    }
  | { readonly kind: "other"; readonly type: ValueType };

export function boundaryShape(
  type: ValueType,
  lookupData: (name: string) => HirData | undefined,
): BoundaryShape {
  if (isBoundaryScalar(type)) return { kind: "scalar", type };
  if (type === "string") return { kind: "string" };
  const optional = optionalInner(type);
  if (optional !== undefined) return { kind: "optional", inner: optional };
  const tuple = tupleParts(type);
  if (tuple) return { kind: "tuple", elements: tuple };
  const nominal = nominalGenericParts(type);
  if (nominal?.name === "List" && nominal.arguments.length === 1)
    return { kind: "list", element: nominal.arguments[0]! };
  const declaration = lookupData(nominal?.name ?? type);
  if (declaration) return { kind: "data", declaration, arguments: nominal?.arguments ?? [] };
  return { kind: "other", type };
}

/** The `[T, E]` of a `Result[T, E]` boundary result. */
export function resultSides(type: ValueType): readonly [ValueType, ValueType] | undefined {
  const parts = nominalGenericParts(type);
  return parts?.name === "Result" && parts.arguments.length === 2
    ? [parts.arguments[0]!, parts.arguments[1]!]
    : undefined;
}

/** Whether a host result of `type` crosses as a boundary node tree. */
export function structuralHostResult(program: HirProgram, type: ValueType): boolean {
  const shape = boundaryShape(type, (name) => program.data.find((item) => item.name === name));
  return (
    shape.kind === "optional" ||
    shape.kind === "tuple" ||
    shape.kind === "list" ||
    shape.kind === "data"
  );
}
