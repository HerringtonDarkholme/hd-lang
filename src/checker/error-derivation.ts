import type {
  DataDecl,
  DataField,
  Decorators,
  EnumDecl,
  Expression,
  MethodDecl,
  Parameter,
  Program,
  UseDecl,
} from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { readonlyType, typeSourceText } from "../types.ts";
import {
  type ErrorCase,
  type ErrorMember,
  type ErrorNames,
  type ErrorType,
  errorHelpers,
  generateErrorType,
} from "./error-generation.ts";

// Error derivation (spec/lang/14-annotations.md#error-derivation). `@error` is a
// compiler intrinsic, whatever a binding named `error` means
// (annot.error.intrinsic, annot.error.name). This pass runs before any
// decorator is resolved: it removes every `@error` form and, inside an error
// type, every `@from` and `@source` marker, checks their placement, and
// generates the `Display`, `Error`, and `From` implementations as ordinary
// hd source (error-generation.ts).

const HIDDEN_ERROR = "__std_error_Error";
const HIDDEN_FROM = "__std_convert_From";

type Form =
  | { readonly kind: "bare" }
  | { readonly kind: "message"; readonly message: Expression }
  | { readonly kind: "transparent" }
  | { readonly kind: "invalid" };

interface Located<T> {
  readonly value: T;
  readonly span: SourceSpan;
}

/** The `@error` form a decorator value writes, or undefined for another value. */
function formOf(fact: Expression): Form | undefined {
  if (fact.kind === "name") return fact.name === "error" ? { kind: "bare" } : undefined;
  if (fact.kind !== "call" || fact.callee.kind !== "name" || fact.callee.name !== "error")
    return undefined;
  const [argument] = fact.arguments;
  const plain = !fact.argumentNames?.some(Boolean) && !fact.argumentSpreads?.some(Boolean);
  if (fact.arguments.length !== 1 || !argument || !plain) return { kind: "invalid" };
  if (argument.kind === "string" || argument.kind === "interpolated-string")
    return { kind: "message", message: argument };
  if (argument.kind === "name" && argument.name === "transparent") return { kind: "transparent" };
  return { kind: "invalid" };
}

/** The `@from` or `@source` marker a member decorator writes (annot.error.marker). */
function markerOf(fact: Expression): Located<{ name: string; arguments: boolean }> | undefined {
  const callee = fact.kind === "call" ? fact.callee : fact;
  if (callee.kind !== "name" || (callee.name !== "from" && callee.name !== "source"))
    return undefined;
  return { value: { name: callee.name, arguments: fact.kind === "call" }, span: fact.span };
}

/** The local name of an imported standard item, if the module imports it. */
function importedName(uses: Program["uses"], module: string, name: string): string | undefined {
  for (const declaration of uses)
    for (const imported of declaration.names)
      if (declaration.module === module && imported.name === name)
        return imported.alias ?? imported.name;
  return undefined;
}

function headName(type: string): string {
  return readonlyType(type).split("[")[0]!;
}

function compact(type: string): string {
  return type.replace(/\s+/g, "");
}

export function withErrorDerivation(program: Program): {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
} {
  const diagnostics: Diagnostic[] = [];
  const error = (code: string, message: string, span: SourceSpan): void => {
    diagnostics.push({ code, message, span });
  };
  // Splits `@error` forms off a list of decorator values. A form on a target
  // that `allowed` rejects is misplaced (annot.error.form.misplaced); a form
  // with a wrong argument is `invalid-error-marker` (annot.error.form.argument).
  const split = (
    facts: readonly Expression[] | undefined,
    target: string,
    allowed: (form: Form) => boolean = () => false,
  ): { forms: Located<Form>[]; rest: readonly Expression[] | undefined } => {
    if (!facts) return { forms: [], rest: facts };
    const forms: Located<Form>[] = [];
    const rest: Expression[] = [];
    for (const fact of facts) {
      const form = formOf(fact);
      if (!form) rest.push(fact);
      else if (form.kind === "invalid")
        error(
          "invalid-error-marker",
          "an @error line takes one message string or 'transparent'",
          fact.span,
        );
      else if (!allowed(form))
        error(
          "decorator-target-kind",
          `this @error form cannot be written before ${target}`,
          fact.span,
        );
      else if (forms.length > 0)
        error("invalid-error-marker", `${target} has more than one @error line`, fact.span);
      else forms.push({ value: form, span: fact.span });
    }
    return { forms, rest };
  };
  const stripDecorators = <T extends { readonly decorators?: Decorators }>(
    item: T,
    target: string,
    allowed?: (form: Form) => boolean,
  ): { item: T; forms: Located<Form>[] } => {
    if (!item.decorators) return { item, forms: [] };
    const { forms, rest } = split(item.decorators.facts, target, allowed);
    return { item: { ...item, decorators: { ...item.decorators, facts: rest! } }, forms };
  };
  const stripMetadata = <T extends { readonly metadata?: readonly Expression[] }>(
    item: T,
    target: string,
  ): T => {
    if (!item.metadata) return item;
    return { ...item, metadata: split(item.metadata, target).rest };
  };
  const parameters = (list: readonly Parameter[]): Parameter[] =>
    list.map((parameter) => stripMetadata(parameter, "a parameter"));
  const methods = (list: readonly MethodDecl[]): MethodDecl[] =>
    list.map((method) => {
      const stripped = stripDecorators(method, "a method").item;
      return { ...stripped, parameters: parameters(stripped.parameters) };
    });

  // Inside an error type, `@from` and `@source` mark only payload members and
  // fields (annot.error.form.other, annot.error.form.misplaced).
  const withoutMarkers = (
    facts: readonly Expression[] | undefined,
    target: string,
  ): readonly Expression[] | undefined =>
    facts?.filter((fact) => {
      const marker = markerOf(fact);
      if (marker)
        error(
          "decorator-target-kind",
          `@${marker.value.name} cannot be written before ${target}`,
          marker.span,
        );
      return !marker;
    });
  const errorTypes: ErrorType[] = [];
  // Reads the cause markers of one variant's payload or one data type's fields.
  const caseOf = (
    fields: readonly DataField[],
    form: Located<Form> | undefined,
    genericParameters: readonly string[],
    name: string | undefined,
  ): { fields: DataField[]; errorCase: ErrorCase } => {
    let unnamed = 0;
    const members: ErrorMember[] = [];
    let cause: number | undefined;
    const kept = fields.map((field, index) => {
      const binding = field.positional ? `_${unnamed++}` : field.name;
      let marker: string | undefined;
      const metadata: Expression[] = [];
      for (const fact of field.metadata ?? []) {
        const found = markerOf(fact);
        const formFound = formOf(fact);
        if (formFound) {
          split([fact], "a member");
          continue;
        }
        if (!found) {
          metadata.push(fact);
          continue;
        }
        if (found.value.arguments)
          error("invalid-error-marker", `@${found.value.name} takes no arguments`, found.span);
        else if (found.value.name === "from" && fields.length !== 1)
          error(
            "decorator-target-kind",
            "@from marks only the only payload member of a variant or the only field of a data type",
            found.span,
          );
        else if (cause !== undefined || marker)
          error(
            "invalid-error-marker",
            "a variant or data type has at most one @from or @source member",
            field.span,
          );
        else if (found.value.name === "from" && genericParameters.includes(field.type.name))
          error(
            "invalid-error-marker",
            `@from cannot mark a member whose type is the type parameter '${field.type.name}'; convert it explicitly`,
            field.span,
          );
        else marker = found.value.name;
      }
      if (marker) cause = index;
      members.push({ field, binding, type: field.type.name, ...(marker ? { marker } : {}) });
      return { ...field, metadata };
    });
    const value = form?.value;
    if (value?.kind === "transparent" && fields.length !== 1 && form)
      error(
        "decorator-target-kind",
        "@error(transparent) needs exactly one payload member or field",
        form.span,
      );
    return {
      fields: kept,
      errorCase: {
        ...(name !== undefined ? { name } : {}),
        members,
        ...(value?.kind === "message" ? { message: value.message } : {}),
        transparent: value?.kind === "transparent" && fields.length === 1,
      },
    };
  };

  const data = program.data.map((declaration): DataDecl => {
    const { item, forms } = stripDecorators(
      declaration,
      "a data type",
      (form) => form.kind !== "bare",
    );
    const form = forms[0];
    if (!form) {
      const fields = item.fields.map((field) => stripMetadata(field, "a field"));
      return { ...item, fields };
    }
    const { fields, errorCase } = caseOf(item.fields, form, item.genericParameters, undefined);
    errorTypes.push({ kind: "data", declaration: item, cases: [errorCase], shared: [] });
    return { ...item, fields };
  });
  const enums = program.enums.map((declaration): EnumDecl => {
    const { item, forms } = stripDecorators(declaration, "an enum", (form) => form.kind === "bare");
    const errorEnum = forms.length > 0;
    const sharedFields = item.sharedFields.map((field) => stripMetadata(field, "shared data"));
    const cases: ErrorCase[] = [];
    const variants = item.variants.map((variant) => {
      const { forms: variantForms, rest } = split(
        variant.metadata,
        errorEnum ? "a variant" : "a variant of an enum without a bare @error",
        (form) => errorEnum && form.kind !== "bare",
      );
      if (!errorEnum)
        return {
          ...variant,
          metadata: rest,
          fields: variant.fields.map((field) => stripMetadata(field, "a payload member")),
        };
      const { fields, errorCase } = caseOf(
        variant.fields,
        variantForms[0],
        item.genericParameters,
        variant.name,
      );
      cases.push(errorCase);
      return { ...variant, metadata: withoutMarkers(rest, "a variant"), fields };
    });
    const result = {
      ...item,
      sharedFields: errorEnum
        ? sharedFields.map((field) => ({
            ...field,
            metadata: withoutMarkers(field.metadata, "shared data"),
          }))
        : sharedFields,
      variants,
    };
    if (errorEnum) {
      const shared = sharedFields
        .filter((field) => !field.positional)
        .map((field): ErrorMember => ({ field, binding: field.name, type: field.type.name }));
      errorTypes.push({ kind: "enum", declaration: result, cases, shared });
    }
    return result;
  });
  const functions = program.functions.map((declaration) => {
    const stripped = stripDecorators(declaration, "a function").item;
    return { ...stripped, parameters: parameters(stripped.parameters) };
  });
  const traits = program.traits.map((declaration) => {
    const stripped = stripDecorators(declaration, "a trait").item;
    return { ...stripped, methods: methods(stripped.methods) };
  });
  const implementations = program.implementations.map((declaration) => {
    const stripped = stripDecorators(declaration, "an implementation").item;
    return { ...stripped, methods: methods(stripped.methods) };
  });
  const types = program.types?.map(
    (declaration) =>
      stripDecorators(declaration, declaration.base ? "a newtype" : "a type alias").item,
  );

  const importedError = importedName(program.uses, "std.error", "Error");
  const importedFrom = importedName(program.uses, "std.convert", "From");
  const names: ErrorNames = {
    error: importedError ?? HIDDEN_ERROR,
    from: importedFrom ?? HIDDEN_FROM,
  };
  checkConversions(errorTypes, error);
  checkHandWritten(program, errorTypes, names, error);
  if (errorTypes.length === 0 || diagnostics.length > 0)
    return {
      program: { ...program, data, enums, functions, traits, implementations, types },
      diagnostics,
    };

  // `@error` needs no import of `std.error.Error` (annot.error.no-use): the
  // prototype imports it under its hidden name when the module does not. The
  // prelude uses `std.convert`, so `From` is declared, under its hidden name
  // when the module does not import it.
  const uses: UseDecl[] = [...program.uses];
  if (!importedError)
    uses.push({
      kind: "use",
      module: "std.error",
      names: [{ name: "Error", alias: HIDDEN_ERROR }],
      span: program.span,
    });
  const generated = errorTypes.map((type, index) => generateErrorType(type, index, names));
  return {
    program: {
      ...program,
      uses,
      data,
      enums,
      traits,
      types,
      implementations: [...implementations, ...generated.flatMap((item) => item.implementations)],
      functions: [
        ...functions,
        ...generated.flatMap((item) => item.functions),
        ...errorHelpers(names),
      ],
    },
    diagnostics,
  };
}

/** Two `@from` members of one type generate overlapping `From` (annot.error.from.same-type). */
function checkConversions(
  errorTypes: readonly ErrorType[],
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  for (const type of errorTypes) {
    const seen = new Set<string>();
    for (const item of type.cases)
      for (const member of item.members) {
        if (member.marker !== "from") continue;
        const key = compact(member.type);
        if (seen.has(key))
          error(
            "overlapping-impl",
            `a second @from member of type '${typeSourceText(member.type)}' generates another From[${typeSourceText(member.type)}] for '${type.declaration.name}'`,
            member.field.span,
          );
        seen.add(key);
      }
  }
}

/**
 * A hand-written `Display`, `Error`, or generated `From[P]` for an error type
 * overlaps a generated one (annot.error.hand-written).
 */
function checkHandWritten(
  program: Program,
  errorTypes: readonly ErrorType[],
  names: ErrorNames,
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  const byName = new Map(errorTypes.map((type) => [type.declaration.name, type] as const));
  for (const implementation of program.implementations) {
    const trait = implementation.traitName;
    const type = byName.get(headName(implementation.targetName));
    if (!trait || !type || implementation.standard) continue;
    const froms = type.cases.flatMap((item) =>
      item.members
        .filter((member) => member.marker === "from")
        .map((member) => compact(`${names.from}[${member.type}]`)),
    );
    const traitHead = headName(trait);
    if (traitHead === "Display" || traitHead === names.error || froms.includes(compact(trait)))
      error(
        "overlapping-impl",
        `'${type.declaration.name}' already implements ${trait} through @error`,
        implementation.span,
      );
  }
}
