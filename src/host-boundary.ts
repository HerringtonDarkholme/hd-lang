import type { HirEnum, ValueType } from "./hir.ts";
import { nominalGenericParts } from "./types.ts";

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
