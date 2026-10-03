import type { TypeRef } from "../ast.ts";
import type { SourcePosition } from "../diagnostics.ts";
import { optionalType } from "../types.ts";

/** Apply every postfix constructor after a primary type has been parsed. */
export function optionalTypeSuffix(
  primary: TypeRef,
  enabled: boolean,
  next: () => SourcePosition | undefined,
): TypeRef {
  let result = primary;
  while (enabled) {
    const end = next();
    if (!end) break;
    result = { name: optionalType(result.name), span: { ...result.span, end } };
  }
  return result;
}
