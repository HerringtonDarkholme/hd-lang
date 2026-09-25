import type { HirExpression, HirFunction, HirProgram, HirStatement } from "./hir.ts";

export interface RequirementPath {
  readonly key: string;
  readonly path: readonly string[];
}

export interface FunctionRequirementExplanation {
  readonly functionName: string;
  readonly declared: readonly string[];
  readonly paths: readonly RequirementPath[];
}

export function explainRequirements(
  program: HirProgram,
): readonly FunctionRequirementExplanation[] {
  const functions = new Map(
    program.functions.map((declaration) => [declaration.index, declaration]),
  );
  return program.functions.map((declaration) => ({
    functionName: declaration.name,
    declared: declaration.requirements,
    paths: declaration.requirements.flatMap((key) => {
      const paths: string[][] = [];
      visitFunction(declaration, key, [declaration.name], new Set(), functions, paths);
      return paths.length > 0
        ? paths.map((path) => ({ key, path }))
        : [{ key, path: [declaration.name, "declared"] }];
    }),
  }));
}

function visitFunction(
  declaration: HirFunction,
  key: string,
  path: readonly string[],
  active: ReadonlySet<string>,
  functions: ReadonlyMap<number, HirFunction>,
  output: string[][],
): void {
  const identity = `${declaration.index}:${key}`;
  if (active.has(identity)) {
    output.push([...path, "recursive"]);
    return;
  }
  const nextActive = new Set(active);
  nextActive.add(identity);
  const before = output.length;
  visitStatements(declaration.body, key, path, nextActive, functions, output);
  if (output.length === before) output.push([...path, "declared"]);
}

function visitStatements(
  statements: readonly HirStatement[],
  key: string,
  path: readonly string[],
  active: ReadonlySet<string>,
  functions: ReadonlyMap<number, HirFunction>,
  output: string[][],
): void {
  for (const statement of statements) {
    switch (statement.kind) {
      case "defer":
        visitStatements(statement.body, key, path, active, functions, output);
        break;
      case "binding":
      case "assignment":
      case "global-binding":
      case "global-assignment":
      case "discard":
        visitExpression(statement.value, key, path, active, functions, output);
        break;
      case "return":
        if (statement.value) visitExpression(statement.value, key, path, active, functions, output);
        break;
      case "expression":
        visitExpression(statement.expression, key, path, active, functions, output);
        break;
      case "break":
        if (statement.value) visitExpression(statement.value, key, path, active, functions, output);
        break;
      case "continue":
      case "pass":
        break;
    }
  }
}

function visitExpression(
  expression: HirExpression,
  key: string,
  path: readonly string[],
  active: ReadonlySet<string>,
  functions: ReadonlyMap<number, HirFunction>,
  output: string[][],
): void {
  switch (expression.kind) {
    case "provider-use":
      if (expression.key === key) output.push([...path, `$.use(${key})`]);
      return;
    case "provider-pack":
      expression.bases.forEach((base) =>
        visitExpression(base, key, path, active, functions, output),
      );
      expression.providers.forEach((provider) =>
        visitExpression(provider, key, path, active, functions, output),
      );
      return;
    case "call":
    case "suspend-construct": {
      expression.arguments.forEach((argument) =>
        visitExpression(argument, key, path, active, functions, output),
      );
      expression.bounds?.forEach((bound) =>
        visitExpression(bound, key, path, active, functions, output),
      );
      const callee = functions.get(expression.functionIndex);
      if (!callee) return;
      expression.providers.forEach((provider, index) => {
        if (provider.kind === "provider-use" && provider.key === key) {
          visitFunction(
            callee,
            callee.requirements[index]!,
            [...path, callee.name],
            active,
            functions,
            output,
          );
        } else if (provider.kind === "provider-pack" && provider.keys.includes(key)) {
          visitFunction(
            callee,
            callee.requirements[index]!,
            [...path, callee.name],
            active,
            functions,
            output,
          );
        }
      });
      return;
    }
    case "suspend-drive":
    case "suspend-cancel":
    case "trait-suspend-drive":
    case "trait-suspend-cancel":
      visitExpression(expression.suspension, key, path, active, functions, output);
      return;
    case "trait-suspend-construct":
      visitExpression(expression.receiver, key, path, active, functions, output);
      expression.arguments.forEach((argument) =>
        visitExpression(argument, key, path, active, functions, output),
      );
      expression.providers.forEach((provider) =>
        visitExpression(provider, key, path, active, functions, output),
      );
      return;
    case "variant-wrap":
      if (expression.payload)
        visitExpression(expression.payload, key, path, active, functions, output);
      return;
    case "list":
      expression.elements.forEach((element) =>
        visitExpression(element, key, path, active, functions, output),
      );
      return;
    case "map":
      expression.entries.forEach((entry) => {
        visitExpression(entry.key, key, path, active, functions, output);
        visitExpression(entry.value, key, path, active, functions, output);
      });
      return;
    case "propagate":
      visitExpression(expression.operand, key, path, active, functions, output);
      return;
    case "unary":
      visitExpression(expression.operand, key, path, active, functions, output);
      return;
    case "binary":
    case "value-equality":
    case "value-ordering":
      visitExpression(expression.left, key, path, active, functions, output);
      visitExpression(expression.right, key, path, active, functions, output);
      return;
    case "assert-equal":
    case "assert":
      expression.arguments.forEach((argument) =>
        visitExpression(argument, key, path, active, functions, output),
      );
      return;
    case "closure":
      expression.captures.forEach((capture) =>
        visitExpression(capture, key, path, active, functions, output),
      );
      return;
    case "closure-call":
      visitExpression(expression.callee, key, path, active, functions, output);
      expression.arguments.forEach((argument) =>
        visitExpression(argument, key, path, active, functions, output),
      );
      expression.providers.forEach((provider) =>
        visitExpression(provider, key, path, active, functions, output),
      );
      return;
    case "trait-wrap":
      visitExpression(expression.value, key, path, active, functions, output);
      return;
    case "trait-dictionary":
    case "trait-bound-dictionary":
      return;
    case "trait-bound":
      visitExpression(expression.value, key, path, active, functions, output);
      return;
    case "trait-call":
      visitExpression(expression.receiver, key, path, active, functions, output);
      expression.arguments.forEach((argument) =>
        visitExpression(argument, key, path, active, functions, output),
      );
      expression.providers.forEach((provider) =>
        visitExpression(provider, key, path, active, functions, output),
      );
      return;
    case "provider-context":
      expression.entries.forEach((entry) =>
        visitExpression(entry.value, key, path, active, functions, output),
      );
      return;
    case "provider-with":
      expression.entries.forEach((entry) =>
        visitExpression(entry.value, key, path, active, functions, output),
      );
      visitStatements(expression.body, key, path, active, functions, output);
      return;
    case "data":
      if (expression.spread)
        visitExpression(expression.spread, key, path, active, functions, output);
      expression.fields.forEach((field) =>
        visitExpression(field, key, path, active, functions, output),
      );
      return;
    case "enum":
      expression.fields.forEach((field) => {
        if (field) visitExpression(field, key, path, active, functions, output);
      });
      return;
    case "member":
    case "enum-member":
    case "variant-tag":
    case "variant-payload":
    case "string-length":
    case "list-length":
    case "list-iterator":
    case "iterator-next":
    case "map-iterator":
    case "map-length":
      visitExpression(expression.receiver, key, path, active, functions, output);
      return;
    case "list-index":
      visitExpression(expression.receiver, key, path, active, functions, output);
      visitExpression(expression.index, key, path, active, functions, output);
      return;
    case "map-index":
    case "map-remove":
      visitExpression(expression.receiver, key, path, active, functions, output);
      visitExpression(expression.key, key, path, active, functions, output);
      return;
    case "panic":
      visitExpression(expression.message, key, path, active, functions, output);
      return;
    case "if":
      visitExpression(expression.condition, key, path, active, functions, output);
      visitStatements(expression.thenBody, key, path, active, functions, output);
      visitStatements(expression.elseBody, key, path, active, functions, output);
      return;
    case "while":
      visitExpression(expression.condition, key, path, active, functions, output);
      visitStatements(expression.body, key, path, active, functions, output);
      visitStatements(expression.elseBody, key, path, active, functions, output);
      return;
    case "match":
      visitExpression(expression.subject, key, path, active, functions, output);
      expression.arms.forEach((arm) => {
        if (arm.literal) visitExpression(arm.literal, key, path, active, functions, output);
        if (arm.guard) visitExpression(arm.guard, key, path, active, functions, output);
        visitStatements(arm.body, key, path, active, functions, output);
      });
      return;
    case "function-value":
    case "closure-self":
      return;
    case "integer":
    case "float":
    case "string":
    case "character":
    case "boolean":
    case "local":
    case "global":
    case "capture":
      return;
  }
}
