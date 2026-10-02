import type { DataDecl, DataField, EnumDecl, Expression, FunctionDecl, ImplDecl } from "../ast.ts";
import { Source_, ZERO_SPAN } from "./generated-source.ts";

// The implementations that `@error` generates (spec/lang/14-annotations.md#error-derivation),
// as ordinary hd source. A message becomes a helper function whose parameters
// are the members it names, so it sees exactly those members and never
// `self` (annot.error.message.scope, annot.error.message.no-self). A line that
// needs a member to implement `Error` or `Display` calls a bounded helper and
// carries the member's span, so an unmet bound is reported at the member
// (annot.error.cause.type, annot.error.transparent.type).

export interface ErrorMember {
  readonly field: DataField;
  /** The member's name in a message: its name, or `_0`, `_1`, ... when unnamed. */
  readonly binding: string;
  readonly type: string;
  readonly marker?: string;
}

export interface ErrorCase {
  /** The variant; absent for a data type. */
  readonly name?: string;
  readonly members: readonly ErrorMember[];
  readonly message?: Expression;
  readonly transparent: boolean;
}

export type ErrorType =
  | {
      readonly kind: "data";
      readonly declaration: DataDecl;
      readonly cases: readonly ErrorCase[];
      readonly shared: readonly ErrorMember[];
    }
  | {
      readonly kind: "enum";
      readonly declaration: EnumDecl;
      readonly cases: readonly ErrorCase[];
      /** The enum's named shared fields (annot.error.message.shared). */
      readonly shared: readonly ErrorMember[];
    };

/** The local names of `std.error.Error` and `std.convert.From`. */
export interface ErrorNames {
  readonly error: string;
  readonly from: string;
}

const DISPLAY = "hd__error_display";
const CAUSE = "hd__error_cause";
const OPTIONAL_CAUSE = "hd__error_optional_cause";
const FORWARD = "hd__error_forward";

/** The names an expression reads. */
function namesIn(node: unknown, found: Set<string>): Set<string> {
  if (Array.isArray(node)) for (const item of node) namesIn(item, found);
  else if (node && typeof node === "object") {
    const record = node as Record<string, unknown>;
    if (record.kind === "name" && typeof record.name === "string") found.add(record.name);
    for (const [key, value] of Object.entries(record)) if (key !== "span") namesIn(value, found);
  }
  return found;
}

function mentions(type: string, parameter: string): boolean {
  return new RegExp(`(^|[^A-Za-z0-9_])${parameter}($|[^A-Za-z0-9_])`).test(type);
}

function causeMember(item: ErrorCase): ErrorMember | undefined {
  return item.members.find((member) => member.marker !== undefined);
}

/** The members a case's message names: its own, then the enum's named shared fields. */
function interpolated(type: ErrorType, item: ErrorCase): ErrorMember[] {
  if (!item.message) return [];
  const names = namesIn(item.message, new Set());
  return [...item.members, ...type.shared].filter((member) => names.has(member.binding));
}

/**
 * The generated bounds on each type parameter (annot.error.bound.*): `Display`
 * gets `Display` for an interpolated or transparent parameter; `Error` gets
 * `Error` for a cause or transparent one, `Display & Inspectable` for one
 * only interpolated, and `Inspectable` for one only carried.
 */
function bounds(type: ErrorType): { display: string[]; error: string[] } {
  const members = (pick: (item: ErrorCase) => readonly ErrorMember[]): Set<string> =>
    new Set(
      type.cases.flatMap((item) => pick(item).map((member) => member.type.replace(/\?$/, ""))),
    );
  const shown = members((item) => interpolated(type, item));
  const forwarded = members((item) => (item.transparent ? item.members : []));
  const caused = members((item) => {
    const member = causeMember(item);
    return member ? [member] : [];
  });
  const display: string[] = [];
  const error: string[] = [];
  for (const parameter of type.declaration.genericParameters) {
    display.push(
      shown.has(parameter) || forwarded.has(parameter) ? `${parameter} < Display` : parameter,
    );
    if (caused.has(parameter) || forwarded.has(parameter)) error.push(`${parameter} < ERROR`);
    else if (shown.has(parameter)) error.push(`${parameter} < Display & Inspectable`);
    else error.push(`${parameter} < Inspectable`);
  }
  return { display, error };
}

function generic(parts: readonly string[]): string {
  return parts.length > 0 ? `[${parts.join(", ")}]` : "";
}

/** The generated implementations and message helpers of one error type. */
export function generateErrorType(
  type: ErrorType,
  index: number,
  names: ErrorNames,
): { implementations: ImplDecl[]; functions: FunctionDecl[] } {
  const { name, genericParameters: parameters } = type.declaration;
  const out = new Source_();
  const T = out.type(parameters.length > 0 ? `${name}[${parameters.join(",")}]` : name);
  const memberType = (member: ErrorMember): string => out.type(member.type);
  const { display, error } = bounds(type);
  const errorBounds = error.map((bound) => bound.replace(/ERROR$/, names.error));
  // How a case reads a member: a pattern binding, or a field of `self`.
  const reader = (item: ErrorCase, used: ReadonlySet<ErrorMember>): Map<ErrorMember, string> => {
    const read = new Map<ErrorMember, string>();
    item.members.forEach((member, position) => {
      if (type.kind === "data") read.set(member, `self.${member.field.name}`);
      else if (used.has(member)) read.set(member, `hd_m${position}`);
    });
    for (const member of type.shared) read.set(member, `self.${member.field.name}`);
    return read;
  };
  const arm = (item: ErrorCase, read: ReadonlyMap<ErrorMember, string>): string => {
    if (item.members.length === 0) return `${name}.${item.name}`;
    const bindings = item.members.map((member) => read.get(member) ?? "_");
    return `${name}.${item.name}(${bindings.join(", ")})`;
  };
  const cases = (
    method: string,
    body: (item: ErrorCase, read: ReadonlyMap<ErrorMember, string>) => string | undefined,
    used: (item: ErrorCase) => readonly ErrorMember[],
    spanOf: (item: ErrorCase) => DataField["span"] | undefined,
    fallback?: string,
  ): void => {
    out.add(`    ${method}:`);
    if (type.kind === "data") {
      const item = type.cases[0]!;
      out.add(`        ${body(item, reader(item, new Set()))}`, spanOf(item));
      return;
    }
    const arms: [string, DataField["span"] | undefined][] = [];
    for (const item of type.cases) {
      const read = reader(item, new Set(used(item)));
      const value = body(item, read);
      if (value !== undefined)
        arms.push([`            ${arm(item, read)} => ${value}`, spanOf(item)]);
    }
    if (arms.length === 0) {
      out.add(`        ${fallback ?? out.string(name)}`);
      return;
    }
    out.add(`        match self:`);
    for (const [line, span] of arms) out.add(line, span);
    if (arms.length < type.cases.length && fallback) out.add(`            _ => ${fallback}`);
  };

  // Display (annot.error.message.*, annot.error.transparent.display).
  const helpers: string[] = [];
  const message = (item: ErrorCase, read: ReadonlyMap<ErrorMember, string>): string => {
    if (item.transparent) return `${DISPLAY}(${read.get(item.members[0]!)})`;
    if (!item.message) return out.string(item.name ?? name);
    const shown = interpolated(type, item);
    const helper = `hd__error_message_${index}_${helpers.length}`;
    const used = parameters.filter((parameter) =>
      shown.some((member) => mentions(member.type, parameter)),
    );
    const helperBounds = used.map((parameter) =>
      shown.some((member) => member.type === parameter) ? `${parameter} < Display` : parameter,
    );
    const list = shown.map((member) => `${member.binding}: ${memberType(member)}`);
    helpers.push(
      `fn ${helper}${generic(helperBounds)}(${list.join(", ")}) -> string:`,
      `    ${out.expression(item.message)}`,
    );
    return `${helper}(${shown.map((member) => read.get(member)).join(", ")})`;
  };
  out.add(`impl${generic(display)} Display for ${T}:`);
  cases(
    "fn to_string(self) -> string",
    message,
    (item) => (item.transparent ? item.members : interpolated(type, item)),
    (item) => (item.transparent ? item.members[0]!.field.span : undefined),
  );

  // Error, with `cause` (annot.error.cause.*, annot.error.transparent.cause).
  const causeOf = (item: ErrorCase, read: ReadonlyMap<ErrorMember, string>): string | undefined => {
    if (item.transparent) return `${FORWARD}(${read.get(item.members[0]!)})`;
    const member = causeMember(item);
    if (!member) return undefined;
    return `${member.type.endsWith("?") ? OPTIONAL_CAUSE : CAUSE}(${read.get(member)})`;
  };
  const causing = type.cases.some((item) => causeOf(item, reader(item, new Set(item.members))));
  out.add(`impl${generic(errorBounds)} ${names.error} for ${T}${causing ? ":" : ""}`);
  if (causing)
    cases(
      `fn cause(self) -> ${names.error}?`,
      causeOf,
      (item) => (item.transparent ? item.members : [causeMember(item)!].filter(Boolean)),
      (item) => (item.transparent ? item.members[0] : causeMember(item))?.field.span,
      ".None",
    );

  // One From per `@from` member (annot.error.from.generate, annot.error.from.data).
  for (const item of type.cases) {
    const member = item.members.find((candidate) => candidate.marker === "from");
    if (!member) continue;
    const P = memberType(member);
    out.add(`impl${generic(parameters)} ${names.from}[${P}] for ${T}:`);
    out.add(`    fn from(value: ${P}) -> ${T}:`);
    out.add(
      type.kind === "data"
        ? `        ${name} { ${member.field.name}: value }`
        : `        ${name}.${item.name}(value)`,
    );
  }
  for (const line of helpers) out.add(line);
  const program = out.program(type.declaration.span);
  return { implementations: [...program.implementations], functions: [...program.functions] };
}

/** The bounded helpers that the generated implementations call. */
export function errorHelpers(names: ErrorNames): FunctionDecl[] {
  const E = names.error;
  const out = new Source_();
  out.add(`fn ${DISPLAY}[T < Display](value: T) -> string:`);
  out.add(`    value.to_string()`);
  out.add(`fn ${CAUSE}[T < ${E}](value: T) -> ${E}?:`);
  out.add(`    .Some(value)`);
  out.add(`fn ${OPTIONAL_CAUSE}[T < ${E}](value: T?) -> ${E}?:`);
  out.add(`    match value:`);
  out.add(`        .Some(inner) => .Some(inner)`);
  out.add(`        .None => .None`);
  out.add(`fn ${FORWARD}[T < ${E}](value: T) -> ${E}?:`);
  out.add(`    value.cause()`);
  return [...out.program(ZERO_SPAN).functions];
}
