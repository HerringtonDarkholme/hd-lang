import type { FunctionDecl } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type { HirExpression } from "../hir.ts";
import type { Signature } from "./context.ts";

// A suffixed literal `Nx` is the call `x(N)` of a function marked
// `@num_suffix` (spec/05-expressions.md#literal-suffixes), and a prefixed
// string `x"..."` the call `x(t)` of a function marked `@str_prefix`, with a
// `std.ops.Template` value `t` (#prefixed-strings). Both are plain call sugar
// (Literal Suffixes L20): the call is an ordinary call, so generic functions
// and requirement rows follow the ordinary rules. The compiler, as the
// markers' reader, checks the marked function's shape once, at its
// definition (L21 and L22, #r-expr.suffix.fn-shape.definition,
// #r-expr.prefix.fn-shape.definition).

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

/**
 * What makes a marked function unusable as a suffix or prefix: exactly one
 * parameter of the right type, even when others have defaults (L22), and no
 * suspension. `fits` says whether the parameter's written type is right; a
 * vararg's type is a list, so it never fits.
 */
function markerShapeProblem(
  declaration: FunctionDecl,
  what: string,
  fits: (type: string) => boolean,
): string | undefined {
  const parameters = declaration.parameters;
  if (parameters.length !== 1) return `must take exactly one parameter, of ${what}`;
  const [parameter] = parameters;
  if (parameter!.variadic || !fits(parameter!.type.name))
    return `must take its parameter as ${what}`;
  if (declaration.suspending) return "must not suspend";
  return undefined;
}

/**
 * The definition-site shape errors of marked functions, as `type-mismatch`
 * at each marked definition. `templates` are the local names of
 * `std.ops.Template`.
 */
export function markerShapeDiagnostics(
  functions: readonly FunctionDecl[],
  templates: ReadonlySet<string>,
): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  for (const declaration of functions) {
    const problems = [
      declaration.numSuffix
        ? markerShapeProblem(declaration, "a primitive integer or floating-point type", (type) =>
            SUFFIX_PARAMETER_TYPES.has(type),
          )
        : undefined,
      declaration.strPrefix
        ? markerShapeProblem(declaration, "type std.ops.Template[T]", (type) =>
            templates.has(type.split("[")[0] ?? type),
          )
        : undefined,
    ];
    const marker = declaration.numSuffix ? "@num_suffix" : "@str_prefix";
    for (const problem of problems)
      if (problem)
        diagnostics.push({
          code: "type-mismatch",
          message: `${marker} function '${declaration.name}' ${problem}`,
          span: declaration.span,
        });
  }
  return diagnostics;
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
  return call();
}

/**
 * Checks the string prefix `name`: `signature` is the module-scope function
 * of that name, if any, and `otherDeclaration` says whether the name is
 * something else in module scope (05-expressions.md#r-expr.prefix.not-marked).
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
