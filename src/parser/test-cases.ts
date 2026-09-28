import type {
  DataDecl,
  EnumDecl,
  FunctionDecl,
  ImplDecl,
  Statement,
  TestDecl,
  TraitDecl,
  TypeDecl,
  UseDecl,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";

/** The items of a module, including those of its `tests:` block. */
export interface ModuleItems {
  uses: UseDecl[];
  functions: FunctionDecl[];
  data: DataDecl[];
  enums: EnumDecl[];
  traits: TraitDecl[];
  implementations: ImplDecl[];
  tests: TestDecl[];
  types: TypeDecl[];
  statements: Statement[];
  testOnlyNames: Set<string>;
  /** Top-level `tests:` statements that are not `it` calls, resolved after the uses. */
  pendingEach: Statement[];
}

export type Fail = (code: string, message: string, span: SourceSpan) => never;

export function emptyModuleItems(): ModuleItems {
  return {
    uses: [],
    functions: [],
    data: [],
    enums: [],
    traits: [],
    implementations: [],
    tests: [],
    types: [],
    statements: [],
    testOnlyNames: new Set(),
    pendingEach: [],
  };
}

export function isCallOf(statement: Statement, name: string): boolean {
  return (
    statement.kind === "expression" &&
    statement.expression.kind === "call" &&
    statement.expression.callee.kind === "name" &&
    statement.expression.callee.name === name
  );
}

// Whether a test body uses `?` outside any nested closure or local function.
function usesPropagation(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  if (Array.isArray(value)) return value.some(usesPropagation);
  const node = value as { kind?: unknown; localFunction?: unknown };
  if (node.kind === "propagate") return true;
  if (node.kind === "closure" || node.kind === "local-declaration" || node.localFunction)
    return false;
  return Object.entries(node).some(([key, child]) => key !== "span" && usesPropagation(child));
}

// A top-level statement of a `tests:` block must be a call of the prelude
// intrinsic `it` with a literal name, literal options, and a body
// (spec/10-modules.md#test-cases).
export function testCase(statement: Statement, fail: Fail): TestDecl {
  const call = statement.kind === "expression" ? statement.expression : undefined;
  if (!call || call.kind !== "call" || call.callee.kind !== "name" || call.callee.name !== "it")
    fail(
      "invalid-test-statement",
      "every top-level statement of a tests block must be an it(...) call",
      statement.span,
    );
  const names = call.argumentNames ?? [];
  const body = call.arguments.at(-1);
  if (!body || body.kind !== "closure" || names[call.arguments.length - 1] !== undefined)
    fail("argument-count", "it(...) needs a body as its final argument", call.span);
  const nameArgument = call.arguments[0];
  if (call.arguments.length < 2 || names[0] !== undefined || nameArgument === undefined)
    fail("argument-count", "it(...) needs the test name first", call.span);
  if (nameArgument.kind !== "string")
    fail(
      "non-literal-test-argument",
      "a test name must be a string literal without interpolation",
      nameArgument.span,
    );
  const options: Record<string, string> = {};
  for (let index = 1; index < call.arguments.length - 1; index += 1) {
    const argument = call.arguments[index]!;
    const option = names[index];
    if (option === undefined)
      fail("argument-count", "it(...) takes one positional name", argument.span);
    if (!["ignore", "expect_panic", "timeout"].includes(option))
      fail(
        "unknown-named-argument",
        `it(...) has no option '${option}'; use ignore, expect_panic, or timeout`,
        argument.span,
      );
    if (argument.kind !== "string")
      fail(
        "non-literal-test-argument",
        `the ${option} option takes a string literal without interpolation`,
        argument.span,
      );
    options[option] = argument.value;
  }
  const explicit = body.trailing !== true;
  if (explicit && body.parameters.length > 0)
    fail("argument-count", "a test body takes no parameters", body.span);
  if (explicit && body.suspending !== true)
    fail("type-mismatch", "a test body has type fn!() -> T", body.span);
  return {
    kind: "test",
    name: nameArgument.value,
    body: body.body,
    ...(explicit
      ? { explicit: true, ...(body.result ? { result: body.result } : {}) }
      : usesPropagation(body.body)
        ? { propagates: true }
        : {}),
    ...(options.ignore !== undefined ? { ignore: options.ignore } : {}),
    ...(options.expect_panic !== undefined ? { expectPanic: options.expect_panic } : {}),
    ...(options.timeout !== undefined ? { timeout: options.timeout } : {}),
    span: statement.span,
  };
}

// `std.testing.it_each(name, rows, body)` registers one case per row
// (spec/10-modules.md#table-tests). The prototype runs the rows as one test
// case, a loop that bang-calls the body with each row.
function tableTest(statement: Statement, aliases: ReadonlySet<string>, fail: Fail): TestDecl {
  const call = statement.kind === "expression" ? statement.expression : undefined;
  if (
    !call ||
    call.kind !== "call" ||
    call.callee.kind !== "name" ||
    !aliases.has(call.callee.name)
  )
    fail(
      "invalid-test-statement",
      "every top-level statement of a tests block must be a call of it or std.testing.it_each",
      statement.span,
    );
  const [nameArgument, rows, body] = call.arguments;
  const named = (call.argumentNames ?? []).some((name) => name !== undefined);
  if (call.arguments.length !== 3 || named || !nameArgument || !rows || !body)
    fail("argument-count", "it_each(...) takes a name, rows, and a body", call.span);
  if (nameArgument.kind !== "string")
    fail(
      "non-literal-test-argument",
      "a test name must be a string literal without interpolation",
      nameArgument.span,
    );
  const span = call.span;
  const row = "$each.row";
  const callback = "$each.body";
  const bangCall: Statement = {
    kind: "expression",
    expression: {
      kind: "suspend-call",
      callee: { kind: "name", name: callback, span },
      arguments: [{ kind: "name", name: row, span }],
      span,
    },
    span,
  };
  return {
    kind: "test",
    name: nameArgument.value,
    body: [
      { kind: "binding", name: callback, mutable: false, value: body, span: body.span },
      {
        kind: "expression",
        expression: {
          kind: "for",
          bindings: [{ name: row, span: rows.span }],
          iterable: rows,
          body: [bangCall],
          elseBody: [],
          span,
        },
        span,
      },
    ],
    span: statement.span,
  };
}

/** Resolves `it_each` calls, checks name uniqueness, and fixes `?` bodies' results. */
export function finishTestCases(items: ModuleItems, fail: Fail): void {
  const aliases = new Set(
    items.uses
      .filter((use) => use.module === "std.testing")
      .flatMap((use) => use.names)
      .filter((name) => name.name === "it_each")
      .map((name) => name.alias ?? name.name),
  );
  for (const statement of items.pendingEach) items.tests.push(tableTest(statement, aliases, fail));
  const seen = new Set<string>();
  for (const test of items.tests) {
    if (seen.has(test.name))
      fail(
        "duplicate-test-name",
        `a test case named '${test.name}' already exists in this module`,
        test.span,
      );
    seen.add(test.name);
  }
  // A trailing test body that uses `?` returns `Result[void, Error]`
  // (spec/05-expressions.md#r-expr.try.test.with-try). The prototype declares
  // the erased `Error` through an implicit `use std.error.Error` when the
  // module does not import it.
  const propagating = items.tests.filter((test) => test.propagates);
  if (propagating.length === 0) return;
  const imported = items.uses
    .filter((use) => use.module === "std.error")
    .flatMap((use) => use.names)
    .find((name) => name.name === "Error");
  const errorName = imported ? (imported.alias ?? imported.name) : "Error";
  if (!imported)
    items.uses.push({
      kind: "use",
      module: "std.error",
      names: [{ name: "Error" }],
      span: propagating[0]!.span,
    });
  items.tests = items.tests.map((test) =>
    test.propagates
      ? { ...test, result: { name: `Result[void,${errorName}]`, span: test.span } }
      : test,
  );
}
