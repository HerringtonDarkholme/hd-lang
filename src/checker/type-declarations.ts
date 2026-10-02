import type { DataDecl, ImplDecl, Program, TypeDecl, TypeRef } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import {
  bindingParts,
  bindingType,
  contextKeys,
  contextType,
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  rowArgumentKeys,
  rowArgumentType,
  tupleParts,
  tupleType,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";

// Type declarations (04-type-system.md#transparent-aliases-and-newtypes). A
// transparent alias is expanded wherever a type is written, so the rest of
// the checker never sees it. A row alias, `type AppRow = $ Db + Cache`, is
// expanded wherever a row is written: headers, function types, row type
// arguments, and contexts (11-requirements-and-suspension.md#row-aliases).
// A newtype `type Name(Base)` becomes a data type whose one field, which
// source cannot name, holds the base value; `Name(value)` constructs it and
// `Base(name)` unwraps it.

/** The field holding a newtype's base value. */
export const NEWTYPE_FIELD = "$value";

interface Alias {
  readonly parameters: readonly string[];
  /** The parameters declared `$R` (11-requirements-and-suspension.md#r-req.row.alias.generic.marked). */
  readonly rows: ReadonlySet<string>;
  readonly target: string;
}

interface RowAlias {
  readonly parameters: readonly string[];
  readonly rows: ReadonlySet<string>;
  readonly keys: readonly string[];
}

const TYPE_KEYS = new Set(["type", "result", "annotation", "value"]);
const TYPE_LIST_KEYS = new Set(["typeArguments", "ownerTypeArguments", "supertraits"]);

function isTypeRef(value: unknown): value is TypeRef {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    typeof (value as { name?: unknown }).name === "string" &&
    keys.includes("span") &&
    keys.every((key) => key === "name" || key === "span")
  );
}

export function words(type: string): readonly string[] {
  return type.split(/[^\p{ID_Continue}#]+/u).filter(Boolean);
}

/**
 * The parameters of an ordinary alias used as requirement keys in its
 * target or declared `$R`, which take row arguments (11-requirements-and-suspension.md#r-req.row.param.marked).
 */
function rowKindedParameters(alias: Alias): ReadonlySet<string> {
  const rows = new Set<string>();
  const visit = (type: string): void => {
    const inner = mutableInner(type) ?? optionalInner(type);
    if (inner !== undefined) return visit(inner);
    const tuple = tupleParts(type);
    if (tuple) return tuple.forEach(visit);
    const callable = functionParts(type);
    if (callable) {
      callable.requirements.forEach((key) => rows.add(key));
      callable.parameters.forEach(visit);
      return visit(callable.result);
    }
    const context = contextKeys(type);
    if (context) return context.forEach((key) => rows.add(key));
    const row = rowArgumentKeys(type);
    if (row) return row.forEach((key) => rows.add(key));
    nominalGenericParts(type)?.arguments.forEach(visit);
  };
  visit(alias.target);
  return new Set(
    alias.parameters.filter((parameter) => rows.has(parameter) || alias.rows.has(parameter)),
  );
}

/** Replaces alias parameters, written by name, with their arguments. */
export function substitute(type: string, substitutions: ReadonlyMap<string, string>): string {
  if (substitutions.size === 0) return type;
  const direct = substitutions.get(type);
  if (direct !== undefined) return direct;
  const visit = (inner: string): string => substitute(inner, substitutions);
  const binding = bindingParts(type);
  if (binding) return bindingType(binding, visit(binding.type));
  const row = (keys: readonly string[]): string[] =>
    keys.flatMap((key) => {
      const argument = substitutions.get(key);
      if (argument === undefined) return [visit(key)];
      return rowArgumentKeys(argument) ?? [argument];
    });
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(visit(mutable));
  const rowArgument = rowArgumentKeys(type);
  if (rowArgument) return rowArgumentType(row(rowArgument));
  const context = contextKeys(type);
  if (context) return contextType(row(context));
  const optional = optionalInner(type);
  if (optional !== undefined) return optionalType(visit(optional));
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map(visit));
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map(visit),
      visit(callable.result),
      row(callable.requirements),
      callable.variadic,
      callable.suspending,
    );
  const nominal = nominalGenericParts(type);
  return nominal ? nominalGenericType(nominal.name, nominal.arguments.map(visit)) : type;
}

/** Expands ordinary and row aliases in types, rows, and single keys. */
class AliasExpander {
  private readonly aliases: ReadonlyMap<string, Alias>;
  private readonly rows: ReadonlyMap<string, RowAlias>;
  private readonly diagnostics: Diagnostic[];

  constructor(
    aliases: ReadonlyMap<string, Alias>,
    rows: ReadonlyMap<string, RowAlias>,
    diagnostics: Diagnostic[],
  ) {
    this.aliases = aliases;
    this.rows = rows;
    this.diagnostics = diagnostics;
  }

  mentions(text: string): boolean {
    return words(text).some((word) => this.aliases.has(word) || this.rows.has(word));
  }

  /** True when `name`, directly or through ordinary aliases, is a row alias. */
  isRow(name: string, depth = 0): boolean {
    if (this.rows.has(name)) return true;
    const alias = this.aliases.get(name);
    return (
      depth < 64 &&
      alias !== undefined &&
      alias.parameters.length === 0 &&
      this.isRow(alias.target, depth + 1)
    );
  }

  private isRowArgument(argument: string): boolean {
    return rowArgumentKeys(argument) !== undefined || this.isRow(argument);
  }

  private kindMismatch(message: string, span: SourceSpan): void {
    this.diagnostics.push({ code: "generic-kind-mismatch", message, span });
  }

  /** A bare key or row alias where a row parameter takes a row (r-req.row.slot.bare). */
  private bareRow(name: string, parameter: string, argument: string, span: SourceSpan): void {
    this.kindMismatch(
      `'${name}' takes a requirement row for '${parameter}', written after '$', as in '$ ${argument.replace(/^trait:/, "")}'`,
      span,
    );
  }

  /** The arguments of an ordinary alias, with a row given for a type-kinded parameter reported. */
  private aliasArguments(
    name: string,
    alias: Alias,
    arguments_: readonly string[],
    span: SourceSpan,
  ): ReadonlyMap<string, string> | undefined {
    const rowKinded = rowKindedParameters(alias);
    for (const [index, argument] of arguments_.entries()) {
      const parameter = alias.parameters[index]!;
      if (!rowKinded.has(parameter) && this.isRowArgument(argument)) {
        this.kindMismatch(
          `'${name}' takes a type, not the requirement row '${argument}', for '${parameter}'`,
          span,
        );
        return undefined;
      }
      // A row parameter's argument is a row slot, written after `$`
      // (11-requirements-and-suspension.md#r-req.row.slot.bare).
      // The bare key stands in as its row after the one kind error, so no
      // unknown-type error follows.
      if (rowKinded.has(parameter) && rowArgumentKeys(argument) === undefined) {
        this.bareRow(name, parameter, argument, span);
        return new Map(
          alias.parameters.map((other, position) => [
            other,
            position === index ? rowArgumentType([argument]) : arguments_[position]!,
          ]),
        );
      }
    }
    return new Map(alias.parameters.map((parameter, index) => [parameter, arguments_[index]!]));
  }

  /** The requirement keys one written key stands for. */
  key(key: string, span: SourceSpan, depth = 0): readonly string[] {
    if (depth > 64) return [key];
    const nominal = nominalGenericParts(key);
    const name = nominal?.name ?? key;
    const arguments_ = nominal?.arguments ?? [];
    const row = this.rows.get(name);
    if (row && row.parameters.length === arguments_.length) {
      const substitutions = new Map(
        row.parameters.map((parameter, index) => [parameter, arguments_[index]!]),
      );
      for (const [index, parameter] of row.parameters.entries()) {
        const argument = arguments_[index]!;
        if (row.rows.has(parameter) && rowArgumentKeys(argument) === undefined) {
          this.bareRow(name, parameter, argument, span);
          return [];
        }
      }
      return row.keys.flatMap((inner) => {
        const argument = substitutions.get(inner);
        const replaced =
          argument === undefined
            ? [substitute(inner, substitutions)]
            : (rowArgumentKeys(argument) ?? [argument]);
        return replaced.flatMap((expanded) => this.key(expanded, span, depth + 1));
      });
    }
    const alias = this.aliases.get(name);
    if (alias && alias.parameters.length === arguments_.length) {
      const substitutions = this.aliasArguments(name, alias, arguments_, span);
      // The key is dropped after its one kind error.
      if (!substitutions) return [];
      const target = substitute(alias.target, substitutions);
      // A key has no `mut`: access follows the trait
      // (11-requirements-and-suspension.md#r-req.row.alias.no-mut).
      if (mutableInner(target) !== undefined) {
        this.diagnostics.push({
          code: "syntax-error",
          message: `alias '${name}' is '${target.replace(/^mut:/, "mut ")}', and a requirement key has no 'mut'; the trait's 'mut self' methods decide the access`,
          span,
        });
        return [key];
      }
      return this.key(target, span, depth + 1);
    }
    return [
      nominal
        ? nominalGenericType(
            name,
            arguments_.map((argument) => this.type(argument, span)),
          )
        : key,
    ];
  }

  /** A written row with every alias expanded, as a sorted set of keys. */
  row(keys: readonly string[], span: SourceSpan): readonly string[] {
    if (!keys.some((key) => this.mentions(key))) return keys;
    return [...new Set(keys.flatMap((key) => this.key(key, span)))].sort();
  }

  /** One key where a single key is required, as in `$.use(Key)` or `Key=value`. */
  singleKey(key: string, span: SourceSpan): string {
    if (!this.mentions(key)) return key;
    const name = nominalGenericParts(key)?.name ?? key;
    if (this.isRow(name)) {
      this.kindMismatch(
        `'${name}' is a requirement row alias, not one key; name each key instead`,
        span,
      );
      return key;
    }
    return this.key(key, span)[0] ?? key;
  }

  /**
   * An explicit type argument with every alias expanded. A bare row alias
   * there is a type, which the kind checks reject
   * (11-requirements-and-suspension.md#r-req.row.slot.bare).
   */
  typeArgument(type: string, span: SourceSpan, _list: string): string {
    return this.type(type, span);
  }

  /** A written type with every alias expanded. */
  type(type: string, span: SourceSpan, depth = 0): string {
    if (depth > 64 || !this.mentions(type)) return type;
    const expand = (inner: string): string => this.type(inner, span, depth + 1);
    const binding = bindingParts(type);
    if (binding) return bindingType(binding, expand(binding.type));
    const mutable = mutableInner(type);
    if (mutable !== undefined) return mutableType(expand(mutable));
    const rowArgument = rowArgumentKeys(type);
    if (rowArgument) return rowArgumentType(this.row(rowArgument, span));
    const context = contextKeys(type);
    if (context) return contextType(this.row(context, span));
    const optional = optionalInner(type);
    if (optional !== undefined) return optionalType(expand(optional));
    const tuple = tupleParts(type);
    if (tuple !== undefined) return tupleType(tuple.map(expand));
    const callable = functionParts(type);
    if (callable)
      return functionType(
        callable.parameters.map(expand),
        expand(callable.result),
        this.row(callable.requirements, span),
        callable.variadic,
        callable.suspending,
      );
    const nominal = nominalGenericParts(type);
    const name = nominal?.name ?? type;
    const rowAlias = this.rows.get(name);
    if (rowAlias) {
      this.kindMismatch(
        `'${name}' is a requirement row alias, not a type; where a row goes, write '$ ${name}'`,
        span,
      );
      // Its first key stands in, so the one kind error is not followed by
      // an unknown-type error.
      return this.key(rowAlias.keys[0] ?? "void", span).at(0) ?? "void";
    }
    const arguments_ = nominal?.arguments ?? [];
    const alias = this.aliases.get(name);
    if (alias && alias.parameters.length === arguments_.length) {
      const substitutions = this.aliasArguments(name, alias, arguments_, span);
      if (!substitutions) return type;
      return expand(substitute(alias.target, substitutions));
    }
    return nominal ? nominalGenericType(name, arguments_.map(expand)) : type;
  }
}

function rewriteTypes<T>(node: T, expander: AliasExpander, span?: SourceSpan): T {
  if (Array.isArray(node)) return node.map((item) => rewriteTypes(item, expander, span)) as T;
  if (!node || typeof node !== "object") return node;
  const record = node as Record<string, unknown>;
  const own = (record.span as SourceSpan | undefined) ?? span;
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(record)) {
    if (TYPE_KEYS.has(key) && isTypeRef(value))
      result[key] = expander.mentions(value.name)
        ? { ...value, name: expander.type(value.name, value.span) }
        : value;
    else if (TYPE_LIST_KEYS.has(key) && Array.isArray(value))
      result[key] = value.map((item) =>
        isTypeRef(item)
          ? expander.mentions(item.name)
            ? { ...item, name: expander.typeArgument(item.name, item.span, key) }
            : item
          : rewriteTypes(item, expander, own),
      );
    // A declaration's or closure's written row (11-requirements-and-suspension.md#r-req.row.alias.expand).
    else if (key === "requirements" && Array.isArray(value) && own) {
      const expanded = expander.row(value as string[], own);
      result[key] = expanded;
      if (expanded !== value && record.kind === "function") result.writtenRequirements = value;
    }
    // `$.use(Key)` and a binding `Key=value` name one key.
    else if (
      key === "key" &&
      typeof value === "string" &&
      own &&
      (record.kind === "provider-use" || record.kind === "binding")
    )
      result[key] = expander.singleKey(value, own);
    // A bound names traits, never a row (11-requirements-and-suspension.md#r-req.row.alias.type-or-key).
    else if (
      key === "traits" &&
      Array.isArray(value) &&
      typeof record.parameter === "string" &&
      own
    )
      result[key] = (value as string[]).map((trait) => {
        const inner = mutableInner(trait);
        const name = nominalGenericParts(inner ?? trait)?.name ?? inner ?? trait;
        if (expander.isRow(name)) expander.singleKey(name, own);
        return trait;
      });
    // A span keeps its identity, so tables keyed by span, such as a derived
    // field's diagnostic (checker/derive-intrinsics.ts), still find it.
    else if (key === "span") result[key] = value;
    else result[key] = rewriteTypes(value, expander, own);
  }
  return result as T;
}

function newtypeData(declaration: TypeDecl, base: TypeRef): DataDecl {
  return {
    kind: "data",
    ...(declaration.public ? { public: true } : {}),
    name: declaration.name,
    genericParameters: declaration.genericParameters,
    fields: [{ name: NEWTYPE_FIELD, type: base, span: base.span }],
    doc: declaration.doc,
    newtype: true,
    span: declaration.span,
  };
}

/**
 * Reports each alias cycle once, on its declaration that comes first in the
 * source (04-type-system.md#r-types.alias.cycle), and returns the names in
 * cycles, which are then not expanded.
 */
function aliasCycles(
  declarations: readonly TypeDecl[],
  targets: ReadonlyMap<string, readonly string[]>,
  diagnostics: Diagnostic[],
): ReadonlySet<string> {
  const cyclic = new Set<string>();
  const reaches = (from: string, to: string, seen: Set<string>): boolean => {
    for (const next of targets.get(from) ?? []) {
      if (next === to) return true;
      if (seen.has(next)) continue;
      seen.add(next);
      if (reaches(next, to, seen)) return true;
    }
    return false;
  };
  for (const declaration of declarations) {
    if (cyclic.has(declaration.name) || !targets.has(declaration.name)) continue;
    if (!reaches(declaration.name, declaration.name, new Set())) continue;
    const members = [...targets.keys()].filter(
      (name) =>
        name === declaration.name ||
        (reaches(declaration.name, name, new Set()) && reaches(name, declaration.name, new Set())),
    );
    members.forEach((name) => cyclic.add(name));
    diagnostics.push({
      code: "alias-cycle",
      message: `alias '${declaration.name}' expands to itself through ${members.join(", ")}`,
      span: declaration.span,
    });
  }
  return cyclic;
}

/** A row alias's right side without `$`, with a fix-it that adds it. */
function missingRowDollar(name: string, span: SourceSpan, diagnostics: Diagnostic[]): void {
  diagnostics.push({
    code: "generic-kind-mismatch",
    message: `the right side of row alias '${name}' is a requirement row, written after '$'`,
    span,
    fix: {
      message: "add '$' before the row",
      edits: [{ span: { start: span.start, end: span.start }, replacement: "$ " }],
    },
  });
}

/** Expands aliases and lowers newtypes to data declarations. */
export function withTypeDeclarations(program: Program): {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
} {
  const declarations = program.types ?? [];
  if (declarations.length === 0) return { program, diagnostics: [] };
  const diagnostics: Diagnostic[] = [];
  const taken = new Set<string>([
    ...program.data.map((declaration) => declaration.name),
    ...program.enums.map((declaration) => declaration.name),
    ...program.traits.map((declaration) => declaration.name),
  ]);
  const aliases = new Map<string, Alias>();
  const rows = new Map<string, RowAlias>();
  const newtypes: TypeDecl[] = [];
  const accepted: TypeDecl[] = [];
  for (const declaration of declarations) {
    if (PRELUDE_NAMES.has(declaration.name)) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' shadows a prelude name`,
        span: declaration.span,
      });
      continue;
    }
    if (taken.has(declaration.name)) {
      diagnostics.push({
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      continue;
    }
    taken.add(declaration.name);
    accepted.push(declaration);
    if (declaration.row)
      rows.set(declaration.name, {
        parameters: declaration.genericParameters,
        rows: new Set(declaration.rowParameters ?? []),
        keys: declaration.row,
      });
    else if (declaration.alias)
      aliases.set(declaration.name, {
        parameters: declaration.genericParameters,
        rows: new Set(declaration.rowParameters ?? []),
        target: declaration.alias.name,
      });
    else newtypes.push(declaration);
  }
  const names = new Set([...aliases.keys(), ...rows.keys()]);
  const targets = new Map<string, readonly string[]>();
  for (const [name, alias] of aliases)
    targets.set(
      name,
      words(alias.target).filter((word) => names.has(word) && !alias.parameters.includes(word)),
    );
  for (const [name, row] of rows)
    targets.set(
      name,
      row.keys.flatMap(words).filter((word) => names.has(word) && !row.parameters.includes(word)),
    );
  for (const name of aliasCycles(accepted, targets, diagnostics)) {
    aliases.delete(name);
    rows.delete(name);
  }
  const expander = new AliasExpander(aliases, rows, diagnostics);
  // A row alias's right side is a row, written after `$`
  // (11-requirements-and-suspension.md#r-req.row.alias.dollar.missing).
  for (const declaration of accepted) {
    const target = declaration.alias;
    const bare =
      declaration.bareRow ??
      (target && expander.isRow(nominalGenericParts(target.name)?.name ?? target.name)
        ? target.span
        : undefined);
    if (bare) missingRowDollar(declaration.name, bare, diagnostics);
  }
  const { types: _types, ...rest } = program;
  const lowered: Program = {
    ...rest,
    data: [
      ...program.data,
      ...newtypes.map((declaration) => newtypeData(declaration, declaration.base!)),
    ],
  };
  const rewritten = rewriteTypes(lowered, expander);
  // An inherent implementation cannot target a transparent alias
  // (09-traits.md#r-trait.own.inherent.tuple-alias).
  for (const implementation of rewritten.implementations) {
    const head = nominalGenericParts(implementation.targetName)?.name ?? implementation.targetName;
    if (implementation.traitName === undefined && aliases.has(head))
      diagnostics.push({
        code: "invalid-impl-target",
        message: `an inherent implementation cannot target the transparent alias '${head}'; implement its target type where that type is declared`,
        span: implementation.span,
      });
  }
  const implementations = rewritten.implementations.map((implementation): ImplDecl =>
    expander.mentions(implementation.targetName)
      ? {
          ...implementation,
          targetName: expander.type(implementation.targetName, implementation.span),
        }
      : implementation,
  );
  return { program: { ...rewritten, implementations }, diagnostics };
}
