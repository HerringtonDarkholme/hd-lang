import type { HirData, HirEnum, HirProgram, ValueType } from "./hir.ts";
import { NUMERIC_TYPES } from "./numeric.ts";
import { nominalGenericParts, optionalInner, tupleParts } from "./types.ts";

interface PayloadlessEnumValue {
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
type BoundaryShape =
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
  /** `Result[T, E]`: tag 0 with a `T` payload, or tag 1 with an `E` payload; a `void` side has none. */
  | { readonly kind: "result"; readonly ok: ValueType; readonly err: ValueType }
  /** A non-generic enum without shared fields: its variant's tag and that variant's fields. */
  | { readonly kind: "enum"; readonly declaration: HirEnum }
  | { readonly kind: "other"; readonly type: ValueType };

export function boundaryShape(
  type: ValueType,
  lookupData: (name: string) => HirData | undefined,
  lookupEnum: (name: string) => HirEnum | undefined = () => undefined,
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
  const sides = resultSides(type);
  if (sides) return { kind: "result", ok: sides[0], err: sides[1] };
  const declaration = lookupData(nominal?.name ?? type);
  if (declaration) return { kind: "data", declaration, arguments: nominal?.arguments ?? [] };
  const enumeration = nominal ? undefined : lookupEnum(type);
  if (
    enumeration &&
    enumeration.genericParameters.length === 0 &&
    enumeration.sharedFields.length === 0
  )
    return { kind: "enum", declaration: enumeration };
  return { kind: "other", type };
}

/**
 * Whether `Result[T, E]` crosses the scalar way, as a tag and a scalar or
 * string payload (src/compiler.ts): its `T` is `void`, a scalar, or a
 * `string`, and its `E` is one of those or a payload-free singleton enum.
 * Any other `Result`, such as `Result[string, FsError]`, crosses as a
 * boundary node tree, so an error with a payload reaches hd code.
 */
export function scalarResult(
  type: ValueType,
  lookupEnum: (name: string) => HirEnum | undefined = () => undefined,
): boolean {
  const sides = resultSides(type);
  if (!sides) return false;
  const [ok, err] = sides;
  const simple = (side: ValueType): boolean =>
    side === "void" || side === "string" || isBoundaryScalar(side);
  const enumeration = lookupEnum(err);
  const singleton =
    enumeration !== undefined &&
    enumeration.variants.length === 1 &&
    enumeration.sharedFields.length === 0 &&
    enumeration.variants[0]!.fields.length === 0;
  return simple(ok) && (simple(err) || singleton);
}

/**
 * The direction a value crosses a boundary in: `out` of hd, as a host call's
 * argument, or `in` to hd, as its result (module.boundary.consent.direction).
 */
export type BoundaryDirection = "out" | "in";

/** The `std.serde` trait whose implementation is consent for a direction. */
export const CONSENT_TRAITS: Readonly<Record<BoundaryDirection, string>> = {
  out: "std.serde.Serialize",
  in: "std.serde.Deserialize",
};

/** An implementation, by its trait's std name and its target. */
export interface ConsentImplementation {
  readonly traitStandardName?: string;
  readonly targetType: string;
}

/** The head names of the types that implement the std trait `trait`. */
export function consentingTypes(
  implementations: readonly ConsentImplementation[],
  trait: string,
): ReadonlySet<string> {
  return new Set(
    implementations
      .filter((implementation) => implementation.traitStandardName === trait)
      .map(
        (implementation) =>
          nominalGenericParts(implementation.targetType)?.name ?? implementation.targetType,
      ),
  );
}

/** A checked program's implementations, by their traits' std names. */
export function programImplementations(
  program: Pick<HirProgram, "implementations" | "traits">,
): ConsentImplementation[] {
  return program.implementations.map((implementation) => {
    const trait = program.traits.find((item) => item.index === implementation.traitIndex);
    return {
      ...(trait?.standardName ? { traitStandardName: trait.standardName } : {}),
      targetType: implementation.targetType,
    };
  });
}

/**
 * Whether the fields of `data` may cross a boundary. A data type whose
 * fields are all public crosses; one with a private field crosses only when
 * it consents for the direction, by implementing `std.serde.Serialize` to go
 * out or `std.serde.Deserialize` to come in (module.boundary.consent.out, .in). A
 * newtype crosses as its base value.
 */
export function boundaryFieldsVisible(data: HirData, consenting: ReadonlySet<string>): boolean {
  return (
    data.newtype === true ||
    data.fields.every((field) => field.public || field.embedded) ||
    consenting.has(data.name)
  );
}

/**
 * Whether a host capability argument of `type` crosses as a boundary value
 * tree (emitter/host-providers.ts, host-arguments.ts): any type but a
 * scalar, a `string`, or a `List[string]`, which keep their own encodings.
 */
export function structuralHostArgument(type: ValueType): boolean {
  return !isBoundaryScalar(type) && type !== "string" && !isStringListArgument(type);
}

/** The program's lookups of a boundary data or enum type by name. */
export function programLookups(program: {
  readonly data: readonly HirData[];
  readonly enums: readonly HirEnum[];
}): readonly [(name: string) => HirData | undefined, (name: string) => HirEnum | undefined] {
  return [
    (name) => program.data.find((item) => item.name === name),
    (name) => program.enums.find((item) => item.name === name),
  ];
}

/**
 * Whether `type` crosses as a list of strings: the one structured argument
 * type a host capability method takes, as `Process.run!`'s `args`
 * (spec/lang/10-modules.md#processes).
 */
export function isStringListArgument(type: ValueType): boolean {
  const nominal = nominalGenericParts(type);
  return (
    nominal?.name === "List" && nominal.arguments.length === 1 && nominal.arguments[0] === "string"
  );
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
  const lookups = programLookups(program);
  const shape = boundaryShape(type, ...lookups);
  return (
    shape.kind === "optional" ||
    shape.kind === "tuple" ||
    shape.kind === "list" ||
    shape.kind === "data" ||
    shape.kind === "enum" ||
    (shape.kind === "result" && !scalarResult(type, lookups[1]))
  );
}
