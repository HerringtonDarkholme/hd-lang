import type { HirExpression, HirFunction, HirStatement, HirTrait, ValueType } from "../hir.ts";
import { nominalGenericParts } from "../types.ts";

export const indent = (text: string, spaces = 2): string => {
  const prefix = " ".repeat(spaces);
  return text
    .split("\n")
    .map((line) => prefix + line)
    .join("\n");
};

export const exportName = (name: string): string => JSON.stringify(name);
export const functionName = (index: number): string => `$f${index}`;
export const localName = (index: number): string => `$l${index}`;
export const globalName = (index: number): string => `$g${index}`;
export const testExportName = (name: string): string | undefined => {
  const match = /^\$test\.(\d+)$/.exec(name);
  return match ? `__hd_test_${match[1]}` : undefined;
};
export const isGenericValueType = (type: ValueType): boolean =>
  /^generic:([^?[\](),]+)$/.test(type);
export const isRowRequirement = (requirement: string): boolean => requirement.startsWith("row:");

export type HirSuspendDrive = Extract<
  HirExpression,
  { kind: "suspend-drive" | "trait-suspend-drive" }
>;

export interface LinearSuspensionSite {
  readonly index: number;
  readonly statementIndex: number;
  readonly statement: HirStatement;
  readonly drive: HirSuspendDrive;
  readonly cleanups: readonly (readonly HirStatement[])[];
}

export function exactStatementDrive(statement: HirStatement): HirSuspendDrive | undefined {
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
  return expression?.kind === "suspend-drive" || expression?.kind === "trait-suspend-drive"
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

export function suspensionFrameTypeName(drive: HirSuspendDrive): string {
  return drive.kind === "suspend-drive"
    ? `$s${drive.functionIndex}`
    : traitSuspensionName(drive.traitIndex, drive.methodIndex);
}

export function suspensionPoll(drive: HirSuspendDrive, frame: string): string {
  return drive.kind === "suspend-drive"
    ? `(call $poll${drive.functionIndex} ${frame})`
    : `(call ${traitSuspensionPollName(drive.traitIndex, drive.methodIndex)} ${frame})`;
}

export function suspensionCancel(drive: HirSuspendDrive, frame: string): string {
  return drive.kind === "suspend-drive"
    ? `(call $cancel${drive.functionIndex} ${frame})`
    : `(call ${traitSuspensionCancelName(drive.traitIndex, drive.methodIndex)} ${frame})`;
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
  const key = type.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
};
