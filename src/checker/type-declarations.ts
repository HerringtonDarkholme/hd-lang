import type { DataDecl, ImplDecl, Program, TypeDecl, TypeRef } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import {
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  substituteTypeParameters,
  tupleParts,
  tupleType,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";

// Type declarations (04-type-system.md#transparent-aliases-and-newtypes). A
// transparent alias is expanded wherever a type is written, so the rest of
// the checker never sees it. A newtype `type Name(Base)` becomes a data type
// whose one field, which source cannot name, holds the base value;
// `Name(value)` constructs it and `Base(name)` unwraps it.

/** The field holding a newtype's base value. */
export const NEWTYPE_FIELD = "$value";

interface Alias {
  readonly parameters: readonly string[];
  readonly target: string;
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

function expandAliases(type: string, aliases: ReadonlyMap<string, Alias>, depth = 0): string {
  if (depth > 64) return type;
  const expand = (inner: string): string => expandAliases(inner, aliases, depth + 1);
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(expand(mutable));
  const optional = optionalInner(type);
  if (optional !== undefined) return optionalType(expand(optional));
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map(expand));
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map(expand),
      expand(callable.result),
      callable.requirements,
      callable.variadic,
      callable.suspending,
    );
  const nominal = nominalGenericParts(type);
  const name = nominal?.name ?? type;
  const arguments_ = (nominal?.arguments ?? []).map(expand);
  const alias = aliases.get(name);
  if (alias && alias.parameters.length === arguments_.length)
    return expand(
      substituteTypeParameters(
        alias.target,
        new Map(alias.parameters.map((parameter, index) => [parameter, arguments_[index]!])),
      ),
    );
  return nominal ? nominalGenericType(name, arguments_) : type;
}

function mentionsAlias(type: string, aliases: ReadonlyMap<string, Alias>): boolean {
  return type.split(/[^\p{ID_Continue}#]+/u).some((word) => aliases.has(word));
}

function rewriteTypes<T>(node: T, expand: (type: TypeRef) => TypeRef): T {
  if (Array.isArray(node)) return node.map((item) => rewriteTypes(item, expand)) as T;
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(node)) {
    if (TYPE_KEYS.has(key) && isTypeRef(value)) result[key] = expand(value);
    else if (TYPE_LIST_KEYS.has(key) && Array.isArray(value))
      result[key] = value.map((item) =>
        isTypeRef(item) ? expand(item) : rewriteTypes(item, expand),
      );
    else result[key] = rewriteTypes(value, expand);
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
  const newtypes: TypeDecl[] = [];
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
    if (declaration.alias)
      aliases.set(declaration.name, {
        parameters: declaration.genericParameters,
        target: declaration.alias.name,
      });
    else newtypes.push(declaration);
  }
  const expand = (type: TypeRef): TypeRef =>
    mentionsAlias(type.name, aliases) ? { ...type, name: expandAliases(type.name, aliases) } : type;
  const { types: _types, ...rest } = program;
  const lowered: Program = {
    ...rest,
    data: [
      ...program.data,
      ...newtypes.map((declaration) => newtypeData(declaration, declaration.base!)),
    ],
  };
  const rewritten = rewriteTypes(lowered, expand);
  const implementations = rewritten.implementations.map((implementation): ImplDecl =>
    mentionsAlias(implementation.targetName, aliases)
      ? { ...implementation, targetName: expandAliases(implementation.targetName, aliases) }
      : implementation,
  );
  return { program: { ...rewritten, implementations }, diagnostics };
}
