import type { FunctionDecl, Program, TypeRef } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { Source_ } from "./generated-source.ts";

// When an entry point returns `.Err(e)`, the host writes the report of `e` to
// standard error and exits with status 1
// (spec/lang/10-modules.md#r-module.entry.err-stderr). The report is the
// error's `Display` text, then a `caused by: ` line per cause when its type
// implements `Error` (r-module.entry.err-render-chain,
// r-module.entry.err-render-display). A test case's `.Err` is rendered the
// same way, for the runner's own report
// (spec/lang/10-modules.md#r-module.testing.err-print). The checker adds one
// generated function per such entry point or test case; its wrapper calls it
// on an `.Err` and hands the text to the host (emitter/context.ts,
// emitEntryReport).

/** The generated function that renders an entry point's `.Err` result. */
const ENTRY_ERROR_RENDERER = "hd__entry_error";

/** The generated function that renders the N-th test case's `.Err` result. */
const TEST_ERROR_RENDERER = /^hd__entry_error_test_(\d+)$/;

/**
 * The checker intrinsic its body calls: `std.error`'s report for a type that
 * implements `Error`, else the `Display` text (checker/expression-calls.ts).
 */
export const ENTRY_ERROR_REPORT = "hd__entry_report";

/** `std.error`'s hidden `entry_report`, which renders the cause chain. */
export const STD_ENTRY_REPORT = "__std_error_entry_report";

/** A written result type that is a `Result`; an alias of one is not seen. */
const RESULT_TYPE = /^(mut\s+)?Result\s*\[/;

/**
 * The function whose `.Err` the generated renderer `name` renders: `main`,
 * or the synthetic `$test.N` of the N-th test case
 * (checker/program-declarations.ts). Undefined for any other name.
 */
export function renderedEntry(name: string): string | undefined {
  if (name === ENTRY_ERROR_RENDERER) return "main";
  const test = TEST_ERROR_RENDERER.exec(name);
  return test ? `$test.${test[1]}` : undefined;
}

/** The entry point `main` or `main!`, when its written result is a `Result`. */
function resultEntry(program: Program): FunctionDecl | undefined {
  return program.functions.find(
    (declaration) =>
      declaration.name === "main" &&
      declaration.public === true &&
      declaration.parameters.length === 0 &&
      declaration.genericParameters.length === 0 &&
      declaration.testOnly !== true &&
      declaration.resultOmitted !== true &&
      RESULT_TYPE.test(declaration.result.name),
  );
}

/** The renderer `name` of a `.Err` held by a `result`, declared at `span`. */
function renderer(
  name: string,
  result: TypeRef,
  span: SourceSpan,
  testOnly: boolean,
): FunctionDecl[] {
  const out = new Source_();
  out.add(`fn ${name}(hd__result: ${out.type(result.name)}) -> string:`);
  out.add("    match hd__result:");
  out.add(`        .Err(hd__error) => ${ENTRY_ERROR_REPORT}(hd__error)`);
  out.add('        _ => ""');
  return out.program(span).functions.map((declaration): FunctionDecl => ({
    ...declaration,
    compilerGenerated: true,
    privateAccess: true,
    // A test case's result may name a type of its `tests:` block.
    ...(testOnly ? { testOnly: true } : {}),
  }));
}

/**
 * Adds the entry renderer when the program's entry point returns a `Result`,
 * and one for each test case whose written result is a `Result`.
 */
export function withEntryErrorRenderer(program: Program): Program {
  const entry = resultEntry(program);
  const functions = [
    ...(entry ? renderer(ENTRY_ERROR_RENDERER, entry.result, entry.span, false) : []),
    ...program.tests.flatMap((test, index) =>
      test.result && RESULT_TYPE.test(test.result.name)
        ? renderer(`hd__entry_error_test_${index}`, test.result, test.span, true)
        : [],
    ),
  ];
  return functions.length === 0
    ? program
    : { ...program, functions: [...program.functions, ...functions] };
}
