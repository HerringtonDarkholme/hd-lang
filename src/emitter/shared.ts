import type {
  HirExpression,
  HirFunction,
  HirProgram,
  HirStatement,
  HirTrait,
  HirTraitMethod,
  ValueType,
} from "../hir.ts";
import { nominalGenericParts, readonlyType } from "../types.ts";

export const indent = (text: string, spaces = 2): string => {
  const prefix = " ".repeat(spaces);
  return text
    .split("\n")
    .map((line) => prefix + line)
    .join("\n");
};

/**
 * Match-test conjunction that evaluates `right` only when `left` holds: a later
 * test may read an erased payload that exists only when earlier tags matched.
 */
export const andThen = (left: string, right: string): string =>
  `(if (result i32) ${left} (then ${right}) (else (i32.const 0)))`;
/** A match test's tag read; `-1` marks the erased optional/`Result` carrier. */
export const matchTestTag = (enumIndex: number, value: string): string =>
  enumIndex === -1
    ? `(struct.get $hd.variant $hd.variant-tag ${value})`
    : `(struct.get $e${enumIndex} $e${enumIndex}tag ${value})`;
export const exportName = (name: string): string => JSON.stringify(name);
export const functionName = (index: number): string => `$f${index}`;
/** A string literal: a fresh `$hd.string` over the UTF-8 bytes of `text`. */
export const stringLiteral = (text: string | readonly number[]): string => {
  const bytes = typeof text === "string" ? [...new TextEncoder().encode(text)] : text;
  const array = bytes.length
    ? `(array.new_fixed $hd.bytes ${bytes.length} ${bytes.map((byte) => `(i32.const ${byte})`).join(" ")})`
    : `(array.new_fixed $hd.bytes 0)`;
  return `(struct.new $hd.string ${array} (i32.const 0) (i32.const ${bytes.length}))`;
};
export const localName = (index: number): string => `$l${index}`;
export const globalName = (index: number): string => `$g${index}`;
export const testExportName = (name: string): string | undefined => {
  const match = /^\$test\.(\d+)$/.exec(name);
  return match ? `__hd_test_${match[1]}` : undefined;
};
export const isGenericValueType = (type: ValueType): boolean =>
  /^\*?generic:([^?[\](),]+)$/.test(type);
export const containsGenericValueType = (type: ValueType): boolean => type.includes("generic:");
export const isRowRequirement = (requirement: string): boolean => requirement.startsWith("row:");

export type HirSuspendDrive = Extract<
  HirExpression,
  { kind: "suspend-drive" | "trait-suspend-drive" | "suspension-drive" }
>;

export interface LinearSuspensionSite {
  readonly index: number;
  readonly statementIndex: number;
  readonly statement: HirStatement;
  readonly drive: HirSuspendDrive;
  readonly cleanups: readonly (readonly HirStatement[])[];
}

function exactStatementDrive(statement: HirStatement): HirSuspendDrive | undefined {
  const expression =
    statement.kind === "binding" ||
    statement.kind === "assignment" ||
    statement.kind === "global-binding" ||
    statement.kind === "global-assignment" ||
    statement.kind === "discard"
      ? statement.value
      : statement.kind === "return" || statement.kind === "break"
        ? statement.value
        : statement.kind === "expression"
          ? statement.expression
          : undefined;
  return expression?.kind === "suspend-drive" ||
    expression?.kind === "trait-suspend-drive" ||
    expression?.kind === "suspension-drive"
    ? expression
    : undefined;
}

export const traitSuspensionName = (traitIndex: number, methodIndex: number): string =>
  `$ts${traitIndex}_${methodIndex}`;
export const traitSuspensionPollName = (traitIndex: number, methodIndex: number): string =>
  `$tspoll${traitIndex}_${methodIndex}`;
export const traitSuspensionCancelName = (traitIndex: number, methodIndex: number): string =>
  `$tscancel${traitIndex}_${methodIndex}`;
export const traitSuspensionResultName = (traitIndex: number, methodIndex: number): string =>
  `$tsresult${traitIndex}_${methodIndex}`;
export const traitSuspensionDriveName = (traitIndex: number, methodIndex: number): string =>
  `$tsdrive${traitIndex}_${methodIndex}`;
export const suspensionWrapperPollAdapterName = (functionIndex: number): string =>
  `$swpoll${functionIndex}`;
export const suspensionWrapperCancelAdapterName = (functionIndex: number): string =>
  `$swcancel${functionIndex}`;
export const suspensionWrapperResultAdapterName = (functionIndex: number): string =>
  `$swresult${functionIndex}`;
export const traitSuspensionWrapperPollAdapterName = (
  traitIndex: number,
  methodIndex: number,
): string => `$tswpoll${traitIndex}_${methodIndex}`;
export const traitSuspensionWrapperCancelAdapterName = (
  traitIndex: number,
  methodIndex: number,
): string => `$tswcancel${traitIndex}_${methodIndex}`;
export const traitSuspensionWrapperResultAdapterName = (
  traitIndex: number,
  methodIndex: number,
): string => `$tswresult${traitIndex}_${methodIndex}`;

export function suspensionFrameTypeName(drive: HirSuspendDrive): string {
  if (drive.kind === "suspend-drive") return `$s${drive.functionIndex}`;
  if (drive.kind === "trait-suspend-drive")
    return traitSuspensionName(drive.traitIndex, drive.methodIndex);
  return "$hd.suspension";
}

export function suspensionPoll(drive: HirSuspendDrive, frame: string): string {
  if (drive.kind === "suspend-drive") return `(call $poll${drive.functionIndex} ${frame})`;
  if (drive.kind === "trait-suspend-drive")
    return `(call ${traitSuspensionPollName(drive.traitIndex, drive.methodIndex)} ${frame})`;
  return `(call $hd.suspension_poll ${frame})`;
}

export function suspensionCancel(drive: HirSuspendDrive, frame: string): string {
  if (drive.kind === "suspend-drive") return `(call $cancel${drive.functionIndex} ${frame})`;
  if (drive.kind === "trait-suspend-drive")
    return `(call ${traitSuspensionCancelName(drive.traitIndex, drive.methodIndex)} ${frame})`;
  return `(call $hd.suspension_cancel ${frame})`;
}

export function linearSuspensionSites(declaration: HirFunction): readonly LinearSuspensionSite[] {
  const sites: LinearSuspensionSite[] = [];
  const cleanups: Array<readonly HirStatement[]> = [];
  declaration.body.forEach((statement, statementIndex) => {
    if (statement.kind === "defer") {
      cleanups.push(statement.body);
      return;
    }
    const drive = exactStatementDrive(statement);
    if (drive)
      sites.push({
        index: sites.length,
        statementIndex,
        statement,
        drive,
        cleanups: [...cleanups],
      });
  });
  return sites;
}
export const providerWatType = (
  requirement: string,
  traits: ReadonlyMap<string, HirTrait>,
): string => {
  if (isRowRequirement(requirement)) return "(ref null $hd.providers)";
  const trait = traits.get(nominalGenericParts(requirement)?.name ?? requirement);
  return trait ? `(ref null $trait${trait.index})` : "externref";
};

export const traitTypeBase = (type: ValueType): string => {
  const key = readonlyType(type).slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
};

/** Dictionary parameters for a trait method's method-level bounds, named `$<prefix><n>`. */
export const methodBoundParameters = (method: HirTraitMethod, prefix?: string): string[] =>
  (method.genericBounds ?? []).map(
    (bound, index) =>
      `(param ${prefix ? `$${prefix}${index} ` : ""}(ref null $trait${bound.traitIndex}))`,
  );

/** One canonical instance per fieldless data type (05-expressions.md). */
export const dataSingletons = (program: HirProgram): string[] =>
  program.data
    .filter((declaration) => declaration.fields.length === 0)
    .map(({ index }) => `  (global $d${index}c (ref $d${index}) (struct.new $d${index}))`);
