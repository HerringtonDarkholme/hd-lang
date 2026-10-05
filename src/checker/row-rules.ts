import type { DataField, FunctionDecl, MethodDecl, Program } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import {
  contextKeys,
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  rowArgumentKeys,
  tupleParts,
  displayType,
} from "../types.ts";

export { isRowSubsumption } from "./assignability.ts";

// Which declarations take row parameters, and how many unknown row
// parameters one pattern may hold (Requirement Reuse RU3 and RU4,
// 11-requirements-and-suspension.md#row-parameters and #least-row-solutions).
// The checks read written types after alias expansion.

/**
 * `expected X, found Y`, naming the row keys of a function value that the
 * expected row lacks (11-requirements-and-suspension.md#r-req.row.subsume.missing).
 */
export function mismatchMessage(actual: string, expected: string): string {
  const wide = functionParts(expected)?.requirements;
  const extra = functionParts(actual)?.requirements.filter((key) => !wide?.includes(key));
  const lacks =
    wide && extra?.length ? `; the expected row lacks ${extra.map(displayType).join(", ")}` : "";
  return `expected ${displayType(expected)}, found ${displayType(actual)}${lacks}`;
}

/**
 * A note for a requirement or mismatch error in a callable whose written row
 * names a row alias: the row as written and expanded
 * (11-requirements-and-suspension.md#r-req.row.alias.diagnostics.expanded).
 */
function aliasedRowNote(code: string, declaration: FunctionDecl): string {
  const written = declaration.writtenRequirements;
  if (!written || (code !== "missing-requirement" && code !== "type-mismatch")) return "";
  const expanded = declaration.requirements.map(displayType).join(" + ") || "()";
  return `; the row '$ ${written.join(" + ")}' expands to '$ ${expanded}'`;
}

interface WrittenRow {
  readonly keys: readonly string[];
  /** The row of a `$.Context[...]` type. */
  readonly context: boolean;
}

/** Every row written in `type`: function-type rows, row type arguments, and context rows. */
function writtenRows(type: string, output: WrittenRow[] = []): WrittenRow[] {
  const inner = mutableInner(type) ?? optionalInner(type);
  if (inner !== undefined) return writtenRows(inner, output);
  const row = rowArgumentKeys(type);
  if (row) {
    output.push({ keys: row, context: false });
    return output;
  }
  const context = contextKeys(type);
  if (context) {
    output.push({ keys: context, context: true });
    return output;
  }
  const tuple = tupleParts(type);
  if (tuple) {
    tuple.forEach((element) => writtenRows(element, output));
    return output;
  }
  const callable = functionParts(type);
  if (callable) {
    output.push({ keys: callable.requirements, context: false });
    callable.parameters.forEach((parameter) => writtenRows(parameter, output));
    writtenRows(callable.result, output);
    return output;
  }
  nominalGenericParts(type)?.arguments.forEach((argument) => writtenRows(argument, output));
  return output;
}

function mentionsParameter(type: string, parameters: ReadonlySet<string>): boolean {
  return writtenRows(type).some((row) => row.keys.some((key) => parameters.has(key)));
}

/** A data type, enum, or trait declares no row parameter (r-req.row.param.no-data.error). */
function typeDeclarationDiagnostics(program: Program, diagnostics: Diagnostic[]): void {
  const report = (owner: string, parameters: ReadonlySet<string>, span: SourceSpan): void => {
    diagnostics.push({
      code: "generic-kind-mismatch",
      message: `'${owner}' takes no row parameter: its generic parameters ${[...parameters].join(", ")} are types, so one cannot be used in a requirement row`,
      span,
    });
  };
  const fields = (
    owner: string,
    parameters: ReadonlySet<string>,
    list: readonly DataField[],
  ): void => {
    const field = list.find((candidate) => mentionsParameter(candidate.type.name, parameters));
    if (field) report(owner, parameters, field.span);
  };
  for (const declaration of program.data) {
    if (declaration.standard || declaration.genericParameters.length === 0) continue;
    fields(declaration.name, new Set(declaration.genericParameters), declaration.fields);
  }
  for (const declaration of program.enums) {
    if (declaration.standard || declaration.genericParameters.length === 0) continue;
    const parameters = new Set(declaration.genericParameters);
    fields(declaration.name, parameters, [
      ...declaration.sharedFields,
      ...declaration.variants.flatMap((variant) => variant.fields),
    ]);
  }
  for (const declaration of program.traits) {
    if (declaration.standard || declaration.genericParameters.length === 0) continue;
    const parameters = new Set(declaration.genericParameters);
    for (const method of declaration.methods) {
      if (
        method.requirements.some((key) => parameters.has(key)) ||
        method.parameters.some((parameter) => mentionsParameter(parameter.type.name, parameters)) ||
        mentionsParameter(method.result.name, parameters)
      ) {
        report(declaration.name, parameters, method.span);
        break;
      }
    }
  }
}

/** Context-row and one-unknown checks of one callable (r-req.row.param.context, r-req.row.least.ambiguous). */
function callableDiagnostics(
  callable: FunctionDecl | MethodDecl,
  enclosing: readonly string[],
  diagnostics: Diagnostic[],
): void {
  const generics = new Set([...enclosing, ...callable.genericParameters]);
  if (generics.size === 0) return;
  const types = [...callable.parameters.map((parameter) => parameter.type), callable.result];
  for (const type of types) {
    const row = writtenRows(type.name).find(
      (candidate) => candidate.context && candidate.keys.some((key) => generics.has(key)),
    );
    if (row)
      diagnostics.push({
        code: "row-parameter-in-context",
        message: `'$.Context[...]' takes a concrete row, not the row parameter '${row.keys.find((key) => generics.has(key))}'`,
        span: type.span,
      });
  }
  // Each parameter's row patterns, as their sets of row parameters.
  const own = new Set(callable.genericParameters);
  const patterns = callable.parameters.map((parameter) => ({
    span: parameter.span,
    rows: writtenRows(parameter.type.name)
      .filter((row) => !row.context)
      .map((row) => new Set(row.keys.filter((key) => own.has(key)))),
  }));
  const fixed = new Set<string>();
  let changed = true;
  while (changed) {
    changed = false;
    for (const pattern of patterns)
      for (const row of pattern.rows) {
        const unknown = [...row].filter((name) => !fixed.has(name));
        if (unknown.length === 1) {
          fixed.add(unknown[0]!);
          changed = true;
        }
      }
  }
  for (const pattern of patterns) {
    const row = pattern.rows.find(
      (candidate) => [...candidate].filter((name) => !fixed.has(name)).length > 1,
    );
    if (row)
      diagnostics.push({
        code: "ambiguous-row-pattern",
        message: `the row pattern lists the row parameters ${[...row].filter((name) => !fixed.has(name)).join(", ")}, and no other parameter fixes all but one of them`,
        span: pattern.span,
      });
  }
}

export function rowRuleDiagnostics(program: Program): readonly Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  typeDeclarationDiagnostics(program, diagnostics);
  for (const declaration of program.functions)
    if (!declaration.standard) callableDiagnostics(declaration, [], diagnostics);
  for (const implementation of program.implementations)
    if (!implementation.standard)
      for (const method of implementation.methods)
        callableDiagnostics(method, implementation.genericParameters, diagnostics);
  for (const trait of program.traits)
    if (!trait.standard)
      for (const method of trait.methods) callableDiagnostics(method, [], diagnostics);
  return diagnostics;
}

/**
 * A diagnostic as the checker reports it. In a parameter default, which
 * runs with an empty row and outside any driver, the ordinary row and bang
 * checks decide requirement-freedom from callee signatures alone; only the
 * reported code differs. A row mismatch also lists aliased rows' keys.
 */
export function rowDiagnostic(
  code: string,
  message: string,
  declaration: FunctionDecl,
): { readonly code: string; readonly message: string } {
  if (declaration.defaultContext) {
    if (code === "missing-requirement") {
      code = "requirement-in-default";
      message = `a default must be requirement-free: ${message}`;
    } else if (code === "bang-call-outside-suspension") {
      code = "suspension-forbidden-context";
      message = "a default must not suspend";
    }
  }
  return { code, message: message + aliasedRowNote(code, declaration) };
}
