import type { DataDecl, EnumDecl, Expression, ImplDecl, MemberLine, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { readonlyType } from "../types.ts";

// Member lines (spec/14-annotations.md#member-lines) and trait-less
// derivation blocks (#trait-less-derivation-blocks). A trait-less block
// `impl X by Structure:` writes shared metadata of `X`; this pass checks it,
// folds its lines into the declaration facts of `X`, and removes it, so every
// later derivation and shape sees the result as if it were written with
// decorators.

export type Target =
  | { readonly kind: "data"; readonly declaration: DataDecl }
  | { readonly kind: "enum"; readonly declaration: EnumDecl };

type Report = (code: string, message: string, span: SourceSpan) => void;

export function directMembers(target: Target): string[] {
  return target.kind === "data"
    ? target.declaration.fields.map((field) => field.name)
    : target.declaration.variants.map((variant) => variant.name);
}

export function declarationFacts(target: Target, name: string): readonly Expression[] {
  if (name === "Self") return target.declaration.decorators?.facts ?? [];
  if (target.kind === "data")
    return target.declaration.fields.find((field) => field.name === name)?.metadata ?? [];
  return target.declaration.variants.find((variant) => variant.name === name)?.metadata ?? [];
}

export function checkMemberLines(
  target: Target,
  lines: readonly MemberLine[],
  factType: (fact: Expression) => string,
  error: Report,
): boolean {
  let valid = true;
  const fail = (code: string, message: string, span: SourceSpan): void => {
    error(code, message, span);
    valid = false;
  };
  const members = new Set(directMembers(target));
  const current = new Map<string, string[]>();
  for (const line of lines) {
    if (line.name !== "Self" && !members.has(line.name)) {
      fail(
        "unknown-annotation-member",
        target.kind === "enum"
          ? `'${line.name}' is not a variant of '${target.declaration.name}'; a member line names a whole variant`
          : `'${line.name}' is not a member of '${target.declaration.name}'`,
        line.nameSpan,
      );
      continue;
    }
    if (line.pass) {
      if (line.operator === "+=" || line.name === "Self" || target.kind === "enum") {
        fail(
          "invalid-member-line",
          line.name === "Self"
            ? "Self takes a fact list, not pass"
            : target.kind === "enum"
              ? "a whole variant cannot be omitted"
              : "pass follows only '='",
          line.span,
        );
        continue;
      }
      if (target.kind === "data") {
        const field = target.declaration.fields.find((item) => item.name === line.name);
        if (field && !field.default) {
          fail(
            "omitted-member-without-default",
            `member '${line.name}' has no default, so it cannot be omitted`,
            line.span,
          );
          continue;
        }
      }
      continue;
    }
    if (!line.value || line.value.kind !== "list") {
      fail("invalid-member-line", "a member line's right side must be a list or pass", line.span);
      continue;
    }
    const before = current.get(line.name) ?? declarationFacts(target, line.name).map(factType);
    const added = line.value.elements.map(factType);
    const next = line.operator === "+=" ? [...before, ...added] : added;
    if (new Set(next).size !== next.length) {
      fail(
        "duplicate-fact",
        `member '${line.name}' would hold two facts of the same concrete type; use '=' to change it`,
        line.span,
      );
      continue;
    }
    current.set(line.name, next);
  }
  return valid;
}

/** Whether an implementation header has `by` and no trait (annot.traitless.form). */
export function isTraitLess(implementation: ImplDecl): boolean {
  return (
    implementation.traitName === undefined &&
    (implementation.byStructure !== undefined || implementation.delegate !== undefined)
  );
}

/** The facts of `name` after the lines of one block (annot.traitless.after-decorators). */
function applied(
  facts: readonly Expression[],
  lines: readonly MemberLine[],
  name: string,
): readonly Expression[] {
  let result = facts;
  for (const line of lines)
    if (line.name === name && line.value?.kind === "list")
      result =
        line.operator === "+=" ? [...result, ...line.value.elements] : [...line.value.elements];
  return result;
}

function withLines(target: Target, lines: readonly MemberLine[], span: SourceSpan): Target {
  const named = new Set(lines.map((line) => line.name));
  const decorators = named.has("Self")
    ? {
        derives: [],
        span,
        ...target.declaration.decorators,
        facts: applied(target.declaration.decorators?.facts ?? [], lines, "Self"),
      }
    : target.declaration.decorators;
  const member = <M extends { readonly name: string; readonly metadata?: readonly Expression[] }>(
    item: M,
  ): M =>
    named.has(item.name)
      ? { ...item, metadata: applied(item.metadata ?? [], lines, item.name) }
      : item;
  if (target.kind === "data")
    return {
      kind: "data",
      declaration: {
        ...target.declaration,
        ...(decorators ? { decorators } : {}),
        fields: target.declaration.fields.map(member),
      },
    };
  return {
    kind: "enum",
    declaration: {
      ...target.declaration,
      ...(decorators ? { decorators } : {}),
      variants: target.declaration.variants.map(member),
    },
  };
}

/**
 * Checks each module-level trait-less block, applies its lines to the
 * declaration facts of its target in source order, and drops the block: it
 * implements nothing (annot.traitless.*).
 */
export function withTraitLessBlocks(
  program: Program,
  structureVisible: boolean,
  factType: (fact: Expression) => string,
  error: Report,
): Program {
  const blocks = program.implementations.filter(isTraitLess);
  if (blocks.length === 0) return program;
  const newtypes = new Set(
    (program.types ?? []).filter((item) => item.base !== undefined).map((item) => item.name),
  );
  const targets = new Map<string, Target>([
    ...program.data.map((item) => [item.name, { kind: "data", declaration: item }] as const),
    ...program.enums.map((item) => [item.name, { kind: "enum", declaration: item }] as const),
  ]);
  for (const block of blocks) {
    // A header without a trait never delegates (trait.by.trait-less.error).
    if (block.delegate) {
      error(
        "invalid-delegation",
        `a header without a trait never delegates; only 'by Structure' may follow '${block.targetName}'`,
        block.delegate.span,
      );
      continue;
    }
    if (!structureVisible) {
      error(
        "unknown-trait",
        "unknown trait 'Structure'; import it with use std.structure.Structure",
        block.byStructure!,
      );
      continue;
    }
    const name = readonlyType(block.targetName).split("[")[0]!;
    const target = newtypes.has(name) ? undefined : targets.get(name);
    if (!target) {
      error(
        "misplaced-derivation",
        newtypes.has(name)
          ? "a trait-less derivation block cannot target a newtype"
          : "a trait-less derivation block must target a data type or enum declared in this module",
        block.span,
      );
      continue;
    }
    let valid = true;
    for (const member of [...block.methods, ...block.associatedTypes]) {
      error(
        "misplaced-derivation",
        "a trait-less derivation block holds only member lines",
        member.span,
      );
      valid = false;
    }
    const lines = block.memberLines ?? [];
    for (const line of lines)
      if (line.pass) {
        error(
          "invalid-member-line",
          "a trait-less derivation block writes only metadata, so it cannot omit a member",
          line.span,
        );
        valid = false;
      }
    if (!valid || !checkMemberLines(target, lines, factType, error)) continue;
    targets.set(name, withLines(target, lines, block.span));
  }
  const declaration = <T extends DataDecl | EnumDecl>(item: T): T =>
    targets.get(item.name)!.declaration as T;
  return {
    ...program,
    data: program.data.map(declaration),
    enums: program.enums.map(declaration),
    implementations: program.implementations.filter((item) => !isTraitLess(item)),
  };
}
