import type { DataDecl, EnumDecl, Expression, ImplDecl, MemberLine, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { nominalGenericParts, readonlyType } from "../types.ts";

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

// A member line's right side may be any list-typed expression
// (annot.line.right-typed). A right side that is not a list literal becomes
// one spread fact element, emitted as `value...` in the facts list.
const spreadFacts = new WeakSet<Expression>();
const spreadIds = new WeakMap<Expression, number>();
let nextSpreadId = 0;

function spreadId(fact: Expression): number {
  let id = spreadIds.get(fact);
  if (id === undefined) {
    id = nextSpreadId++;
    spreadIds.set(fact, id);
  }
  return id;
}

/** Whether a fact element stands for a whole list, spread into the facts. */
export function isSpreadFact(fact: Expression): boolean {
  return spreadFacts.has(fact);
}

/** The fact elements that a member line's list-typed right side adds. */
export function lineFacts(value: Expression): readonly Expression[] {
  if (value.kind !== "list") {
    spreadFacts.add(value);
    return [value];
  }
  value.elements.forEach((element, index) => {
    if (value.spreads?.[index]) spreadFacts.add(element);
  });
  return value.elements;
}

const NOT_A_LIST = new Set([
  "boolean",
  "character",
  "closure",
  "data",
  "float",
  "integer",
  "interpolated-string",
  "map",
  "map-comprehension",
  "string",
  "text",
  "tuple",
]);

/**
 * Whether a right side can be list-typed: a list, or an expression whose known
 * type is a list. Anything else, such as `name = 5`, is `invalid-member-line`
 * (annot.line.right.error, annot.line.right.not-list).
 */
export function listValued(
  value: Expression,
  knownType: (value: Expression) => string | undefined,
): boolean {
  if (value.kind === "list" || value.kind === "list-comprehension") return true;
  if (NOT_A_LIST.has(value.kind)) return false;
  const type = knownType(value);
  return type === undefined || readonlyType(type).startsWith("List[");
}

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
  knownType: (value: Expression) => string | undefined,
  error: Report,
): boolean {
  let valid = true;
  // A spread list's element types are not known here, so it never duplicates.
  const typeOf = (fact: Expression): string =>
    isSpreadFact(fact) ? `spread:${spreadId(fact)}` : factType(fact);
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
    if (!line.value || !listValued(line.value, knownType)) {
      fail(
        "invalid-member-line",
        "a member line's right side must be a list-typed expression or pass",
        line.span,
      );
      continue;
    }
    const before = current.get(line.name) ?? declarationFacts(target, line.name).map(typeOf);
    const added = lineFacts(line.value).map(typeOf);
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
    if (line.name === name && line.value)
      result =
        line.operator === "+=" ? [...result, ...lineFacts(line.value)] : [...lineFacts(line.value)];
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
 * declaration facts of its target (at most one block per type), and drops the block: it
 * implements nothing (annot.traitless.*).
 */
export function withTraitLessBlocks(
  program: Program,
  structureVisible: boolean,
  factType: (fact: Expression) => string,
  knownType: (value: Expression) => string | undefined,
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
  const seen = new Set<string>();
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
    // The header binds the declaration's parameters in order, under any names
    // and without bounds (annot.traitless.generic, annot.traitless.generic-rename).
    const declared = target.declaration.genericParameters;
    const arguments_ = nominalGenericParts(readonlyType(block.targetName))?.arguments ?? [];
    const parameters = block.genericParameters;
    if (
      block.genericBounds.length > 0 ||
      parameters.length !== declared.length ||
      arguments_.length !== declared.length ||
      arguments_.some((argument, index) => argument !== parameters[index])
    ) {
      error(
        "misplaced-derivation",
        declared.length === 0
          ? `a trait-less derivation block for '${name}' takes no type parameters`
          : `a trait-less derivation block must apply '${name}' to its own ${declared.length} type parameter(s), in order and without bounds`,
        block.span,
      );
      continue;
    }
    // One trait-less block per type (annot.traitless.unique).
    if (seen.has(name)) {
      error(
        "overlapping-impl",
        `type '${name}' already has a trait-less derivation block`,
        block.span,
      );
      continue;
    }
    seen.add(name);
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
    if (!valid || !checkMemberLines(target, lines, factType, knownType, error)) continue;
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
