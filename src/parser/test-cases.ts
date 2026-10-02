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
// (spec/06-control-flow.md#r-flow.panic.category-names).
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
// (spec/10-modules.md#test-cases): the name and any other leading
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
    // (std/testing.md#r-std-testing.option.timeout-any-duration); the
    // checker types it.
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

function optionFields(
  options: Readonly<Record<string, string>>,
  timeout: Expression | undefined,
): Partial<TestDecl> {
  return {
    ...(options.ignore !== undefined ? { ignore: options.ignore } : {}),
    ...(options.expect_panic !== undefined ? { expectPanic: options.expect_panic } : {}),
    ...(timeout ? { timeout } : {}),
  };
}

// A top-level statement of a `tests:` block must be a call of the prelude
// function `it` with a literal name, its options, and a body
// (spec/10-modules.md#test-cases).
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
    body: body.body,
    ...(explicit
      ? { explicit: true, ...(body.result ? { result: body.result } : {}) }
      : usesPropagation(body.body)
        ? { propagates: true }
        : {}),
    ...optionFields(options, timeout),
    span: statement.span,
  };
}

// `std.testing.it_each(name, rows, ..., body=)` registers one case per row
// (spec/std/testing.md#table-test-rows). The prototype compiles one test function
// that the runner calls once per row, each in a fresh instance: it evaluates
// `rows`, reports their count, and runs the body with the selected row. With
// no rows, row 0 panics with `index-out-of-bounds` after reporting count 0.
// A body without a written result that uses `?` returns `Result[void, Error]`
// (spec/std/testing.md#r-std-testing.try.test.row-body).
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
  const propagates = !body.result && usesPropagation(body.body);
  const span = call.span;
  const local = (name: string): Expression => ({ kind: "name", name, span });
  const bind = (name: string, value: Expression): Statement => ({
    kind: "binding",
    name,
    mutable: false,
    value,
    span,
  });
  const invoke = (callee: Expression, arguments_: Expression[] = []): Expression => ({
    kind: "call",
    callee,
    arguments: arguments_,
    span,
  });
  const count = invoke({ kind: "member", receiver: local("$each.rows"), name: "len", span });
  const bangCall: Expression = {
    kind: "suspend-call",
    callee: local("$each.body"),
    arguments: [
      { kind: "index", receiver: local("$each.rows"), index: local("$each.index"), span },
    ],
    span,
  };
  const closure: Closure = propagates
    ? { ...body, result: { name: `Result[void,${errorName}]`, span: body.span } }
    : body;
  const statementOf = (expression: Expression): Statement => ({
    kind: "expression",
    expression,
    span,
  });
  return {
    kind: "test",
    name,
    body: [
      { ...bind("$each.body", closure), span: body.span },
      bind("$each.rows", rows),
      bind("$each.index", invoke(local("$each-row-index"))),
      statementOf(invoke(local("$each-row-count"), [count])),
      statementOf(propagates ? { kind: "propagate", operand: bangCall, span } : bangCall),
      ...(propagates
        ? [statementOf(invoke({ kind: "contextual-variant", name: "Ok", span }))]
        : []),
    ],
    ...(body.result ? { explicit: true, result: body.result } : {}),
    table: true,
    ...(propagates ? { propagates: true } : {}),
    ...optionFields(options, timeout),
    span: statement.span,
  };
}

// `std.testing.it_prop(name, ..., cases=, shrink=, prop=)` and
// `it_prop_with(name, gen, ...)` register one property test case
// (spec/std/testing.md#property-tests). The prototype compiles one test
// function that the runner calls once per generated case, each in a fresh
// instance: it reports `cases` and `shrink` to the runner, takes a
// runner-created `Choices`, draws the input with `gen` or the parameter
// type's `Arbitrary`, reports the input's `Debug` text (so `T < Debug`,
// spec/std/testing.md#r-std-testing.prop.debug), and runs `prop` with it
// (src/property-tests.ts).
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
  const propagates = !body.result && usesPropagation(body.body);
  const span = call.span;
  const local = (text: string): Expression => ({ kind: "name", name: text, span });
  const integer = (value: number): Expression => ({ kind: "integer", value: BigInt(value), span });
  const invoke = (callee_: Expression, arguments_: Expression[] = []): Expression => ({
    kind: "call",
    callee: callee_,
    arguments: arguments_,
    span,
  });
  const bind = (text: string, value: Expression): Statement => ({
    kind: "binding",
    name: text,
    mutable: false,
    value,
    span,
  });
  const statementOf = (expression: Expression): Statement => ({
    kind: "expression",
    expression,
    span,
  });
  // With `examples`, a helper takes the example the runner selects or
  // draws a value; without, the case still asks which example to run, so
  // the runner learns there is none (src/property-tests.ts).
  const examples = named.examples;
  const draw: Expression = withGenerator
    ? examples
      ? invoke(local(PROPERTY_VALUE), [examples, local("$prop.gen"), local("$prop.choices")])
      : invoke(local("$prop.gen"), [local("$prop.choices")])
    : examples
      ? {
          kind: "call",
          callee: local(PROPERTY_EXAMPLE_OR_DRAW),
          typeArguments: [parameter!.type!],
          arguments: [examples, local("$prop.choices")],
          span,
        }
      : {
          kind: "call",
          callee: { kind: "member", receiver: local("$prop.choices"), name: "draw", span },
          typeArguments: [parameter!.type!],
          arguments: [],
          span,
        };
  const bangCall: Expression = {
    kind: "suspend-call",
    callee: local("$prop.body"),
    arguments: [local("$prop.value")],
    span,
  };
  const closure: Closure = propagates
    ? { ...body, result: { name: `Result[void,${errorName}]`, span: body.span } }
    : body;
  return {
    kind: "test",
    name,
    body: [
      { ...bind("$prop.body", closure), span: body.span },
      ...(generator ? [bind("$prop.gen", generator)] : []),
      statementOf(
        invoke(local(PROPERTY_CONFIG), [named.cases ?? integer(100), named.shrink ?? integer(500)]),
      ),
      {
        kind: "binding",
        name: "$prop.choices",
        mutable: true,
        mutableAccess: true,
        value: invoke(local(PROPERTY_CHOICES)),
        span,
      },
      ...(examples
        ? []
        : [
            {
              kind: "discard",
              value: invoke(local(PROPERTY_EXAMPLE), [integer(0)]),
              span,
            } as Statement,
          ]),
      bind("$prop.value", draw),
      statementOf(invoke(local(PROPERTY_INPUT), [local("$prop.value")])),
      statementOf(propagates ? { kind: "propagate", operand: bangCall, span } : bangCall),
      ...(propagates
        ? [statementOf(invoke({ kind: "contextual-variant", name: "Ok", span }))]
        : []),
    ],
    ...(body.result ? { explicit: true, result: body.result } : {}),
    property: true,
    ...(propagates ? { propagates: true } : {}),
    ...optionFields(options, timeout),
    span: statement.span,
  };
}

/** The hidden `std.testing` functions a lowered property test calls. */
const PROPERTY_CONFIG = "__std_testing_prop_config";
const PROPERTY_CHOICES = "__std_testing_prop_choices";
/** The example the runner selects, or -1; see `examples` in lib/std/testing.hd. */
const PROPERTY_EXAMPLE = "__std_testing_prop_example";
const PROPERTY_VALUE = "__std_testing_prop_value";
const PROPERTY_EXAMPLE_OR_DRAW = "__std_testing_prop_example_or_draw";
/** Reports the input's `Debug` text, so `T` must implement `Debug`. */
const PROPERTY_INPUT = "__std_testing_prop_input";

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
  // (spec/05-expressions.md#r-expr.try.test.with-try). The prototype declares
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
  if (propagating.length === 0) return;
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

function isRowName(candidate: string, table: string): boolean {
  if (!candidate.startsWith(`${table}[`) || !candidate.endsWith("]")) return false;
  return /^[0-9]+$/.test(candidate.slice(table.length + 1, -1));
}
