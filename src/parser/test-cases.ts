import type {
  DataDecl,
  EnumDecl,
  Expression,
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

type Fail = (code: string, message: string, span: SourceSpan) => never;

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

// The stable panic categories an `expect_panic` option may name
// (spec/lang/06-control-flow.md#r-flow.panic.category-names).
const PANIC_CATEGORIES: ReadonlySet<string> = new Set([
  "assertion-failed",
  "explicit-panic",
  "integer-overflow",
  "integer-division-by-zero",
  "invalid-shift",
  "index-out-of-bounds",
  "iterator-invalidated",
  "structure-variant-mismatch",
  "suspension-competing-driver",
  "suspension-reentrant-poll",
  "suspension-invalid-state",
  "stack-exhausted",
]);

const TEST_OPTIONS = ["ignore", "expect_panic", "timeout"];

type Call = Extract<Expression, { kind: "call" }>;
type Closure = Extract<Expression, { kind: "closure" }>;

interface TestArguments {
  readonly name: string;
  readonly body: Closure;
  readonly options: Readonly<Record<string, string>>;
  readonly timeout?: Expression;
  /** The positional arguments after the name, such as `it_each`'s rows. */
  readonly positional: readonly Expression[];
  /** Named arguments beyond `it`'s options, such as `it_prop`'s `cases`. */
  readonly named: Readonly<Record<string, Expression>>;
}

/** What a test-case function takes beyond `it`'s options. */
interface TestSignature {
  /** The final function's parameter name: `body`, or `prop` for a property. */
  readonly bodyName: string;
  /** Leading arguments that may also be passed by name. */
  readonly named: readonly string[];
  /** Named options beyond `it`'s. */
  readonly options?: readonly string[];
}

const IT_SIGNATURE: TestSignature = { bodyName: "body", named: [] };

// Reads a test-case call as an ordinary call of `it` or `it_each`
// (spec/lang/10-modules.md#test-cases): the name and any other leading
// positional arguments, then literal named options, then the body as a
// trailing block or `body=` (07-functions.md#r-fn.default.final-function).
function testArguments(
  call: Call,
  callee: string,
  positional: number,
  fail: Fail,
  signature: TestSignature = IT_SIGNATURE,
): TestArguments {
  const names = call.argumentNames ?? [];
  const nameArgument = call.arguments[0];
  if (nameArgument === undefined || names[0] !== undefined)
    fail("argument-count", `${callee}(...) needs the test name first`, call.span);
  if (nameArgument.kind !== "string")
    fail(
      "non-literal-test-argument",
      "a test name must be a string literal without interpolation",
      nameArgument.span,
    );
  const leading: Expression[] = [];
  const named: Record<string, Expression> = {};
  const options: Record<string, string> = {};
  let timeout: Expression | undefined;
  let body: Closure | undefined;
  for (let index = 1; index < call.arguments.length; index += 1) {
    const argument = call.arguments[index]!;
    const option = names[index];
    if (option === undefined) {
      if (argument.kind === "closure" && argument.trailing === true) {
        body = argument;
        continue;
      }
      if (leading.length + 1 < positional) {
        leading.push(argument);
        continue;
      }
      fail(
        argument.kind === "closure" ? "type-mismatch" : "argument-count",
        `${callee}(...) takes its options by name, and its body as a trailing block or body=`,
        argument.span,
      );
    }
    if (signature.named.includes(option) || signature.options?.includes(option)) {
      named[option] = argument;
      continue;
    }
    if (option === signature.bodyName) {
      if (argument.kind !== "closure")
        fail("type-mismatch", `the body of ${callee}(...) is a fn! closure`, argument.span);
      body = argument;
      continue;
    }
    if (!TEST_OPTIONS.includes(option))
      fail(
        "unknown-named-argument",
        `${callee}(...) has no option '${option}'; use ignore, expect_panic, or timeout`,
        argument.span,
      );
    // `timeout` takes any `Duration` value, evaluated when the test case runs
    // (std/testing.md#r-std-testing.option.timeout-any-duration); see
    // `withTimeout`.
    if (option === "timeout") {
      timeout = argument;
      continue;
    }
    if (argument.kind !== "string")
      fail(
        "non-literal-test-argument",
        `the ${option} option takes a string literal without interpolation`,
        argument.span,
      );
    if (option === "expect_panic" && !PANIC_CATEGORIES.has(argument.value))
      fail("unknown-panic-category", `'${argument.value}' is not a panic category`, argument.span);
    options[option] = argument.value;
  }
  // A leading argument may also be passed by name, as `it_prop_with`'s `gen=`.
  const namedLeading = signature.named.filter((option) => named[option] !== undefined).length;
  if (leading.length + namedLeading + 1 < positional)
    fail("argument-count", `${callee}(...) is missing an argument`, call.span);
  if (!body) fail("argument-count", `${callee}(...) needs a body as its final argument`, call.span);
  if (body.trailing !== true && body.suspending !== true)
    fail("type-mismatch", `the body of ${callee}(...) is a fn! closure`, body.span);
  return {
    name: nameArgument.value,
    body,
    named,
    options,
    ...(timeout ? { timeout } : {}),
    positional: leading,
  };
}

function optionFields(options: Readonly<Record<string, string>>): Partial<TestDecl> {
  return {
    ...(options.ignore !== undefined ? { ignore: options.ignore } : {}),
    ...(options.expect_panic !== undefined ? { expectPanic: options.expect_panic } : {}),
  };
}

/** A call of a hidden `std.testing` function, `lib/std/testing.hd`. */
function testingCall(
  name: string,
  arguments_: readonly Expression[],
  span: SourceSpan,
  named: Readonly<Record<string, Expression | undefined>> = {},
): Expression {
  const extra = Object.entries(named).filter(
    (entry): entry is [string, Expression] => entry[1] !== undefined,
  );
  return {
    kind: name.endsWith("_case") ? "suspend-call" : "call",
    callee: { kind: "name", name: `__std_testing_${name}`, span },
    arguments: [...arguments_, ...extra.map(([, value]) => value)],
    ...(extra.length > 0
      ? { argumentNames: [...arguments_.map(() => undefined), ...extra.map(([key]) => key)] }
      : {}),
    span,
  };
}

// A `timeout` value is any `Duration`, evaluated when the test case runs
// (spec/std/testing.md#r-std-testing.option.timeout-at-run): the test
// function first passes it to `case_timeout`, which reports it to the
// runner's `TestRunner`.
function withTimeout(timeout: Expression | undefined, body: readonly Statement[]): Statement[] {
  if (!timeout) return [...body];
  const call = testingCall("case_timeout", [timeout], timeout.span);
  return [{ kind: "expression", expression: call, span: timeout.span }, ...body];
}

// A timed `it` test function reports its `timeout`, then runs the written
// body as a closure with the empty row, so the runner's `TestRunner` never
// covers the body (spec/std/testing.md#runner-capabilities). The closure
// keeps the body's result: the written one, or the fixed `void` of a
// trailing block, or `Result[void, Error]` when it uses `?`.
function timedBody(test: TestDecl): TestDecl {
  if (!test.timed || test.table || test.property) return test;
  const [report, ...body] = test.body;
  const span = test.span;
  const closure: Closure = {
    kind: "closure",
    suspending: true,
    parameters: [],
    requirements: [],
    body,
    ...(test.result
      ? { result: test.result }
      : test.explicit
        ? {}
        : { result: { name: "void", span } }),
    span,
  };
  const run = testingCall("run_case", [closure], span);
  return { ...test, body: [report!, { kind: "expression", expression: run, span }] };
}

// A top-level statement of a `tests:` block must be a call of the prelude
// function `it` with a literal name, its options, and a body
// (spec/lang/10-modules.md#test-cases).
export function testCase(statement: Statement, fail: Fail): TestDecl {
  const call = statement.kind === "expression" ? statement.expression : undefined;
  if (!call || call.kind !== "call" || call.callee.kind !== "name" || call.callee.name !== "it")
    fail(
      "invalid-test-statement",
      "every top-level statement of a tests block must be an it(...) call",
      statement.span,
    );
  const { name, body, options, timeout } = testArguments(call, "it", 1, fail);
  const explicit = body.trailing !== true;
  if (explicit && body.parameters.length > 0)
    fail("argument-count", "a test body takes no parameters", body.span);
  return {
    kind: "test",
    name,
    body: withTimeout(timeout, body.body),
    ...(timeout ? { timed: true } : {}),
    ...(explicit
      ? { explicit: true, ...(body.result ? { result: body.result } : {}) }
      : usesPropagation(body.body)
        ? { propagates: true }
        : {}),
    ...optionFields(options),
    span: statement.span,
  };
}

// A test case whose body is one call of a `std.testing` function with the
// written body: the `it_each`, `it_prop`, or `it_prop_with` case body in
// lib/std/testing.hd. A body without a written result that uses `?` returns
// `Result[void, Error]`, as the test case does
// (spec/std/testing.md#r-std-testing.try.test.row-body). A body without a
// written row gets the empty row, which a test body must have
// (spec/lang/10-modules.md#r-module.testing.unit-row): the runner's
// capabilities cover the case function around it, never the body.
function libraryCase(
  written: Closure,
  run: (closure: Closure) => Expression,
  timeout: Expression | undefined,
  errorName: string,
): Partial<TestDecl> & Pick<TestDecl, "body"> {
  const body: Closure = { ...written, requirements: written.requirements ?? [] };
  const propagates = !body.result && usesPropagation(body.body);
  const closure: Closure = propagates
    ? { ...body, result: { name: `Result[void,${errorName}]`, span: body.span } }
    : body;
  const expression = run(closure);
  return {
    body: withTimeout(timeout, [{ kind: "expression", expression, span: expression.span }]),
    ...(timeout ? { timed: true } : {}),
    ...(body.result ? { explicit: true, result: body.result } : {}),
    ...(propagates ? { propagates: true } : {}),
  };
}

// `std.testing.it_each(name, rows, ..., body=)` registers one case per row
// (spec/std/testing.md#table-test-rows). The prototype compiles one test
// function that the runner calls once per row, each in a fresh instance; its
// body is `each_case!(rows, body)`.
function tableTest(
  statement: Statement,
  aliases: ReadonlySet<string>,
  errorName: string,
  fail: Fail,
): TestDecl {
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
  const { name, body, options, timeout, positional } = testArguments(call, "it_each", 2, fail);
  const rows = positional[0]!;
  if (body.trailing === true || body.parameters.length !== 1)
    fail("argument-count", "an it_each body is a fn! closure with one parameter", body.span);
  const run = (closure: Closure): Expression =>
    testingCall("each_case", [rows, closure], call.span);
  return {
    kind: "test",
    name,
    ...libraryCase(body, run, timeout, errorName),
    table: true,
    ...optionFields(options),
    span: statement.span,
  };
}

// `std.testing.it_prop(name, ..., cases=, shrink=, prop=)` and
// `it_prop_with(name, gen, ...)` register one property test case
// (spec/std/testing.md#property-tests). The prototype compiles one test
// function that the runner calls once per generated case, each in a fresh
// instance (src/property-tests.ts); its body is `prop_case!` or
// `prop_with_case!` of lib/std/testing.hd.
function propertyTest(
  statement: Statement,
  withGenerator: boolean,
  errorName: string,
  fail: Fail,
): TestDecl {
  const call = (statement as Extract<Statement, { kind: "expression" }>).expression as Call;
  const callee = withGenerator ? "it_prop_with" : "it_prop";
  const signature = {
    bodyName: "prop",
    named: withGenerator ? ["gen"] : [],
    options: ["cases", "shrink", "examples"],
  };
  const { name, body, options, timeout, positional, named } = testArguments(
    call,
    callee,
    withGenerator ? 2 : 1,
    fail,
    signature,
  );
  const generator = positional[0] ?? named.gen;
  if (withGenerator !== (generator !== undefined))
    fail("argument-count", `${callee}(...) ${withGenerator ? "needs" : "takes no"} gen`, call.span);
  const parameter = body.parameters[0];
  if (body.trailing === true || body.parameters.length !== 1)
    fail("argument-count", `a ${callee} prop is a fn! closure with one parameter`, body.span);
  if (!withGenerator && !parameter!.type)
    fail(
      "closure-parameter-needs-annotation",
      "an it_prop parameter needs a type, whose Arbitrary draws it",
      parameter!.span,
    );
  const span = call.span;
  const integer = (value: number): Expression => ({ kind: "integer", value: BigInt(value), span });
  const caps = [named.cases ?? integer(100), named.shrink ?? integer(500)];
  const run = (closure: Closure): Expression =>
    generator
      ? testingCall("prop_with_case", [...caps, generator, closure], span, {
          examples: named.examples,
        })
      : testingCall("prop_case", [...caps, closure], span, { examples: named.examples });
  return {
    kind: "test",
    name,
    ...libraryCase(body, run, timeout, errorName),
    property: true,
    ...optionFields(options),
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
  // A test body that uses `?` returns `Result[void, Error]`
  // (spec/lang/05-expressions.md#r-expr.try.test.with-try). The prototype declares
  // the erased `Error` through an implicit `use std.error.Error` when the
  // module does not import it.
  const imported = items.uses
    .filter((use) => use.module === "std.error")
    .flatMap((use) => use.names)
    .find((name) => name.name === "Error");
  const errorName = imported ? (imported.alias ?? imported.name) : "Error";
  const importedAs = (name: string): Set<string> =>
    new Set(
      items.uses
        .filter((use) => use.module === "std.testing")
        .flatMap((use) => use.names)
        .filter((entry) => entry.name === name)
        .map((entry) => entry.alias ?? entry.name),
    );
  const properties = [importedAs("it_prop"), importedAs("it_prop_with")] as const;
  const calleeOf = (statement: Statement): string | undefined =>
    statement.kind === "expression" &&
    statement.expression.kind === "call" &&
    statement.expression.callee.kind === "name"
      ? statement.expression.callee.name
      : undefined;
  const tables = items.pendingEach
    .filter((statement) => !properties.some((names) => names.has(calleeOf(statement) ?? "")))
    .map((statement) => tableTest(statement, aliases, errorName, fail));
  const propertyTests = items.pendingEach
    .filter((statement) => properties.some((names) => names.has(calleeOf(statement) ?? "")))
    .map((statement) =>
      propertyTest(statement, properties[1].has(calleeOf(statement)!), errorName, fail),
    );
  items.tests.push(...propertyTests);
  items.tests.push(...tables);
  const later = (left: TestDecl, right: TestDecl): TestDecl =>
    left.span.start.offset > right.span.start.offset ? left : right;
  const seen = new Map<string, TestDecl>();
  for (const test of items.tests) {
    const earlier = seen.get(test.name);
    if (earlier)
      fail(
        "duplicate-test-name",
        `a test case named '${test.name}' already exists in this module`,
        later(earlier, test).span,
      );
    seen.set(test.name, test);
  }
  // An `it_each` case is named `name[i]`, so no other test case may use
  // such a name (spec/std/testing.md#r-std-testing.it-each.name-clash).
  for (const table of tables)
    for (const test of items.tests)
      if (test !== table && isRowName(test.name, table.name))
        fail(
          "duplicate-test-name",
          `'${test.name}' names a row of it_each("${table.name}", ...)`,
          later(table, test).span,
        );
  const propagating = items.tests.filter((test) => test.propagates);
  if (propagating.length > 0 && !imported)
    items.uses.push({
      kind: "use",
      module: "std.error",
      names: [{ name: "Error" }],
      span: propagating[0]!.span,
    });
  items.tests = items.tests.map((test) =>
    timedBody(
      test.propagates
        ? { ...test, result: { name: `Result[void,${errorName}]`, span: test.span } }
        : test,
    ),
  );
}

function isRowName(candidate: string, table: string): boolean {
  if (!candidate.startsWith(`${table}[`) || !candidate.endsWith("]")) return false;
  return /^[0-9]+$/.test(candidate.slice(table.length + 1, -1));
}
