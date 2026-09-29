import type { HirExpression } from "../hir.ts";
import type { Signature } from "./context.ts";

// The suffix reader: a suffixed literal `Nx` is the call `x(N)` of a function
// marked `@num_suffix` (spec/05-expressions.md#literal-suffixes). The call is
// checked as an ordinary call first, so a parameter the literal cannot fill
// is an ordinary call error; then the reader checks the function's shape at
// the literal (#r-expr.suffix.fn-shape.reader).

const SUFFIX_PARAMETER_TYPES: ReadonlySet<string> = new Set([
  "i8",
  "i16",
  "i32",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "f32",
  "f64",
]);

/** What makes a function unusable as a suffix (05-expressions.md#r-expr.suffix.fn-shape). */
function suffixShapeProblem(signature: Signature): string | undefined {
  if (signature.genericParameters.length > 0 || signature.rowParameters.length > 0)
    return "must declare no type parameters";
  if (signature.parameters.length !== 1 || signature.variadic)
    return "must take exactly one parameter";
  if (!SUFFIX_PARAMETER_TYPES.has(signature.parameters[0]!))
    return "must take a primitive integer or floating-point parameter";
  if (signature.requirements.length > 0) return "must need no providers";
  if (signature.suspending) return "must not suspend";
  return undefined;
}

/**
 * Checks the literal suffix `name`: `signature` is the module-scope function
 * of that name, if any, and `otherDeclaration` says whether the name is
 * something else in module scope (05-expressions.md#r-expr.suffix.not-marked).
 */
export function checkLiteralSuffixCall(
  name: string,
  signature: Signature | undefined,
  otherDeclaration: boolean,
  fail: (code: string, message: string) => never,
  call: () => HirExpression,
): HirExpression {
  if (!signature) {
    if (otherDeclaration)
      fail(
        "invalid-literal-suffix",
        `literal suffix '${name}' names something other than a function marked @num_suffix`,
      );
    fail("unknown-name", `unknown literal suffix '${name}'`);
  }
  if (!signature.numSuffix)
    fail(
      "invalid-literal-suffix",
      `literal suffix '${name}' names a function that is not marked @num_suffix`,
    );
  const checked = call();
  const problem = suffixShapeProblem(signature);
  if (problem) fail("invalid-literal-suffix", `suffix function '${name}' ${problem}`);
  return checked;
}
