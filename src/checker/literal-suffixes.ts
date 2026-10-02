import type { HirExpression } from "../hir.ts";
import type { Signature } from "./context.ts";

// A suffixed literal `Nx` is the call `x(N)` of a function marked
// `@num_suffix` (spec/lang/05-expressions.md#literal-suffixes), and a prefixed
// string `x"..."` the call `x(t)` of a function marked `@str_prefix`, with a
// `std.ops.Template` value `t` (#prefixed-strings). Both are plain call sugar
// (r-expr.literal-fn.ordinary-call): the call is an ordinary call, so generic
// functions and requirement rows follow the ordinary rules. The markers are
// typed facts, so a marked function's shape is checked at its decorator
// (r-annot.typed-fact.check, checker/typed-facts.ts).

/**
 * Checks the literal suffix `name`: `signature` is the module-scope function
 * of that name, if any, and `otherDeclaration` says whether the name is
 * something else in module scope (05-expressions.md#r-expr.literal-fn.not-marked).
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
  return call();
}

/**
 * Checks the string prefix `name`: `signature` is the module-scope function
 * of that name, if any, and `otherDeclaration` says whether the name is
 * something else in module scope (05-expressions.md#r-expr.literal-fn.not-marked).
 */
export function checkStringPrefixCall(
  name: string,
  signature: Signature | undefined,
  otherDeclaration: boolean,
  fail: (code: string, message: string) => never,
  call: () => HirExpression,
): HirExpression {
  if (!signature) {
    if (otherDeclaration)
      fail(
        "invalid-string-prefix",
        `string prefix '${name}' names something other than a function marked @str_prefix`,
      );
    fail("unknown-name", `unknown string prefix '${name}'`);
  }
  if (!signature.strPrefix)
    fail(
      "invalid-string-prefix",
      `string prefix '${name}' names a function that is not marked @str_prefix`,
    );
  return call();
}
