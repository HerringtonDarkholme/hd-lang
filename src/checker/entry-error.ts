import type { FunctionDecl, Program } from "../ast.ts";
import { Source_ } from "./generated-source.ts";

// When an entry point returns `.Err(e)`, the host writes the report of `e` to
// standard error and exits with status 1
// (spec/lang/10-modules.md#r-module.entry.err-stderr). The report is the
// error's `Display` text, then a `caused by: ` line per cause when its type
// implements `Error` (r-module.entry.err-render-chain,
// r-module.entry.err-render-display). The checker adds one generated
// function that renders an entry result's error; the entry wrapper calls it
// on an `.Err` and hands the text to the host (emitter/context.ts,
// emitEntryReport).

/** The generated function that renders an entry point's `.Err` result. */
export const ENTRY_ERROR_RENDERER = "hd__entry_error";

/**
 * The checker intrinsic its body calls: `std.error`'s report for a type that
 * implements `Error`, else the `Display` text (checker/expression-calls.ts).
 */
export const ENTRY_ERROR_REPORT = "hd__entry_report";

/** `std.error`'s hidden `entry_report`, which renders the cause chain. */
export const STD_ENTRY_REPORT = "__std_error_entry_report";

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
      /^(mut\s+)?Result\s*\[/.test(declaration.result.name),
  );
}

/** Adds the entry renderer when the program's entry point returns a `Result`. */
export function withEntryErrorRenderer(program: Program): Program {
  const entry = resultEntry(program);
  if (!entry) return program;
  const out = new Source_();
  out.add(`fn ${ENTRY_ERROR_RENDERER}(hd__result: ${out.type(entry.result.name)}) -> string:`);
  out.add("    match hd__result:");
  out.add(`        .Err(hd__error) => ${ENTRY_ERROR_REPORT}(hd__error)`);
  out.add('        _ => ""');
  const functions = out.program(entry.span).functions.map((declaration): FunctionDecl => ({
    ...declaration,
    compilerGenerated: true,
    privateAccess: true,
  }));
  return { ...program, functions: [...program.functions, ...functions] };
}
