import type {
  DataDecl,
  DataField,
  EnumDecl,
  EnumVariant,
  FunctionDecl,
  ImplDecl,
  MethodDecl,
  Parameter,
  Program,
  Statement,
  TraitDecl,
} from "./ast.ts";
import type { SourcePosition, SourceSpan } from "./diagnostics.ts";
import { jsonSpan, type JsonSpan } from "./diagnostic-report.ts";

// Name-addressed symbol lookup for `hd def` and `hd doc`. It reads parsed
// modules only, so it answers for a project whose types do not check yet.
//
// A name is a module path and an item path. In a package, `pkg.user.User`
// is `User` in `src/user.hd` (module `user`), `pkg.User` is `User` in
// `src/mod.hd`, and the `pkg.` root may be left out. In a single file the
// item path is the whole name. An item path is `Item`, `Item.member`, or
// `Enum.Variant.field`; `Type::function` is accepted for `Type.function`.
// Without a module path, every module is searched.

export type SymbolKind =
  | "function"
  | "data"
  | "enum"
  | "trait"
  | "binding"
  | "field"
  | "variant"
  | "method"
  | "associated-function"
  | "associated-type";

export interface SymbolParameter {
  readonly name: string;
  readonly type: string;
  readonly variadic: boolean;
  readonly default: string | null;
}

export interface SymbolTrait {
  readonly trait: string;
  /** The implementation's target as written, such as `Box[T]`. */
  readonly target: string;
  readonly file: string;
  readonly span: JsonSpan;
}

export interface SymbolInfo {
  /** The qualified name that addresses this symbol. */
  readonly name: string;
  readonly kind: SymbolKind;
  /** Dotted module identity; `""` for the root module or a single file. */
  readonly module: string;
  readonly file: string;
  readonly span: JsonSpan;
  readonly public: boolean;
  /** The declaration header as written, without its body or comments. */
  readonly signature: string;
  readonly doc: string | null;
  /** The qualified name of the enclosing type or trait, for members. */
  readonly owner?: string;
  /** The trait a method implements or declares. */
  readonly trait?: string;
  /** A field's, binding's, or associated type's type, when known. */
  readonly type?: string | null;
  readonly embedded?: boolean;
  readonly parameters?: readonly SymbolParameter[];
  readonly result?: string | null;
  readonly requirements?: readonly string[] | null;
  readonly suspending?: boolean;
  /**
   * Which of `result`, `requirements`, and `type` the source omits. Their
   * values then come from the checker, or are null when it did not run.
   */
  readonly omitted?: readonly ("result" | "requirements" | "type")[];
  /** A trait method with a default body. */
  readonly hasDefault?: boolean;
  /** Fields, variants, methods, and associated types of a type or trait. */
  readonly members?: readonly SymbolInfo[];
  /** Traits a data type or enum implements; for a trait, its implementations. */
  readonly implementations?: readonly SymbolTrait[];
  /** A trait's supertraits as written. */
  readonly supertraits?: readonly string[];
  /** The name the lookup matched when it reached this symbol through a `pub use`. */
  readonly via?: string;
}

export interface SourceModule {
  /** Path as reported, such as `src/user.hd` or the file given on the command line. */
  readonly path: string;
  readonly identity: string;
  readonly source: string;
  readonly program: Program;
}

/** Types the checker inferred for a declaration that omits them. */
export interface InferredTypes {
  readonly functions: ReadonlyMap<
    string,
    { readonly result: string; readonly requirements: readonly string[] }
  >;
  readonly globals: ReadonlyMap<string, string>;
}

export interface LookupResult {
  readonly symbols: readonly SymbolInfo[];
  /** Qualified names that share the query's last segment, when nothing matched. */
  readonly suggestions: readonly string[];
}

type Item =
  | { readonly kind: "function"; readonly decl: FunctionDecl }
  | { readonly kind: "data"; readonly decl: DataDecl }
  | { readonly kind: "enum"; readonly decl: EnumDecl }
  | { readonly kind: "trait"; readonly decl: TraitDecl }
  | { readonly kind: "binding"; readonly decl: Extract<Statement, { kind: "binding" }> };

function lineStarts(source: string): number[] {
  const starts = [0];
  for (let index = 0; index < source.length; index += 1)
    if (source[index] === "\n") starts.push(index + 1);
  return starts;
}

function positionAt(starts: readonly number[], offset: number): SourcePosition {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const middle = (low + high + 1) >> 1;
    if (starts[middle]! <= offset) low = middle;
    else high = middle - 1;
  }
  return { offset, line: low + 1, column: offset - starts[low]! + 1 };
}

/**
 * The declaration header starting at `offset`: everything up to the first
 * line break outside brackets and strings, or in `suite` mode up to the `:`
 * that opens the body, with comments dropped and whitespace collapsed.
 */
export function headerAt(source: string, offset: number, mode: "suite" | "line" = "suite"): string {
  let depth = 0;
  let text = "";
  for (let index = offset; index < source.length; index += 1) {
    const char = source[index]!;
    if (char === "#") {
      while (index < source.length && source[index] !== "\n") index += 1;
      if (depth === 0) break;
      text += " ";
      continue;
    }
    if (char === '"' || char === "'") {
      let end = index + 1;
      while (end < source.length && source[end] !== char && source[end] !== "\n")
        end += source[end] === "\\" ? 2 : 1;
      text += source.slice(index, end + 1);
      index = end;
      continue;
    }
    if (char === "(" || char === "[" || char === "{") depth += 1;
    else if (char === ")" || char === "]" || char === "}") depth -= 1;
    else if (
      depth === 0 &&
      (char === "\n" || (mode === "suite" && char === ":" && source[index + 1] !== "="))
    )
      break;
    text += char;
  }
  return text
    .replaceAll(/\s+/g, " ")
    .replaceAll(/([([{]) /g, "$1")
    .replaceAll(/ ?,? ([)\]}])/g, "$1")
    .replaceAll(/,([)\]}])/g, "$1")
    .trim();
}

function baseName(target: string): string {
  return target
    .replace(/^mut\s+/, "")
    .replace(/\[.*$/s, "")
    .trim();
}

class ModuleView {
  readonly module: SourceModule;
  private readonly starts: number[];
  private readonly qualifier: string;

  constructor(module: SourceModule, packageMode: boolean) {
    this.module = module;
    this.starts = lineStarts(module.source);
    this.qualifier = packageMode
      ? module.identity === ""
        ? "pkg."
        : `pkg.${module.identity}.`
      : module.identity === ""
        ? ""
        : `${module.identity}.`;
  }

  qualified(item: string): string {
    return `${this.qualifier}${item}`;
  }

  text(span: SourceSpan): string {
    return this.module.source.slice(span.start.offset, span.end.offset);
  }

  /**
   * The declaration's span, widened to a `pub` marker before it on the same
   * line and without trailing whitespace.
   */
  span(span: SourceSpan): JsonSpan {
    const source = this.module.source;
    const lineStart = this.starts[span.start.line - 1] ?? span.start.offset;
    const marker = /(?:^|\s)(pub\s+)$/.exec(source.slice(lineStart, span.start.offset));
    const start = marker ? span.start.offset - marker[1]!.length : span.start.offset;
    let end = span.end.offset;
    while (end > start && /\s/.test(source[end - 1]!)) end -= 1;
    return jsonSpan({ start: positionAt(this.starts, start), end: positionAt(this.starts, end) });
  }

  header(
    span: SourceSpan,
    isPublic: boolean | undefined,
    mode: "suite" | "line" = "suite",
  ): string {
    const header = headerAt(this.module.source, span.start.offset, mode);
    return isPublic && !header.startsWith("pub ") ? `pub ${header}` : header;
  }
}

function omitted(decl: MethodDecl | FunctionDecl): ("result" | "requirements")[] {
  return [
    ...(decl.resultOmitted ? ["result" as const] : []),
    ...(decl.requirementsOmitted ? ["requirements" as const] : []),
  ];
}

function parameters(view: ModuleView, list: readonly Parameter[]): SymbolParameter[] {
  return list.map((parameter) => ({
    name: parameter.name,
    type: parameter.type.name,
    variadic: parameter.variadic === true,
    default: parameter.default ? view.text(parameter.default.span).trim() : null,
  }));
}

function fieldSymbol(view: ModuleView, owner: string, field: DataField): SymbolInfo {
  const defaultText = field.default ? ` = ${view.text(field.default.span).trim()}` : "";
  return {
    name: `${owner}.${field.name}`,
    kind: "field",
    module: view.module.identity,
    file: view.module.path,
    span: view.span(field.span),
    public: field.embedded === true || field.public === true,
    signature: field.embedded
      ? field.type.name
      : `${field.public ? "pub " : ""}${field.name}: ${field.type.name}${defaultText}`,
    doc: field.doc ?? null,
    owner,
    type: field.type.name,
    embedded: field.embedded === true,
  };
}

function variantSymbol(view: ModuleView, owner: string, variant: EnumVariant): SymbolInfo {
  const name = `${owner}.${variant.name}`;
  return {
    name,
    kind: "variant",
    module: view.module.identity,
    file: view.module.path,
    span: view.span(variant.span),
    public: true,
    signature: view.header(variant.span, false),
    doc: variant.doc ?? null,
    owner,
    members: variant.fields.map((field) => fieldSymbol(view, name, field)),
  };
}

function methodSymbol(
  view: ModuleView,
  owner: string,
  method: MethodDecl,
  trait: string | undefined,
  inTrait: boolean,
): SymbolInfo {
  const receiver = method.parameters[0]?.name === "self";
  return {
    name: `${owner}.${method.name}`,
    kind: receiver ? "method" : "associated-function",
    module: view.module.identity,
    file: view.module.path,
    span: view.span(method.span),
    public: method.public === true || trait !== undefined,
    signature: view.header(method.span, method.public),
    doc: method.doc ?? null,
    owner,
    ...(trait ? { trait } : {}),
    parameters: parameters(view, method.parameters),
    result: method.resultOmitted ? null : method.result.name,
    requirements: method.requirementsOmitted ? null : method.requirements,
    suspending: method.suspending,
    omitted: omitted(method),
    ...(inTrait ? { hasDefault: method.body !== undefined } : {}),
  };
}

interface ImplSite {
  readonly view: ModuleView;
  readonly decl: ImplDecl;
}

export class SymbolIndex {
  private readonly views: readonly ModuleView[];
  private readonly impls: readonly ImplSite[];
  private readonly inferred: InferredTypes | undefined;

  constructor(modules: readonly SourceModule[], packageMode: boolean, inferred?: InferredTypes) {
    this.views = modules.map((module) => new ModuleView(module, packageMode));
    this.impls = this.views.flatMap((view) =>
      view.module.program.implementations.map((decl) => ({ view, decl })),
    );
    this.inferred = inferred;
  }

  private items(view: ModuleView): Map<string, Item> {
    const items = new Map<string, Item>();
    const program = view.module.program;
    const add = (name: string, item: Item): void => {
      if (!items.has(name)) items.set(name, item);
    };
    for (const decl of program.functions) add(decl.name, { kind: "function", decl });
    for (const decl of program.data) add(decl.name, { kind: "data", decl });
    for (const decl of program.enums) add(decl.name, { kind: "enum", decl });
    for (const decl of program.traits) add(decl.name, { kind: "trait", decl });
    for (const decl of program.statements)
      if (decl.kind === "binding") add(decl.name, { kind: "binding", decl });
    return items;
  }

  /** The module a `pub use` path names from `view`, following 10-modules.md#use-roots. */
  private useTarget(view: ModuleView, path: string): ModuleView | undefined {
    const parts = path.split(".");
    let base: string[];
    if (parts[0] === "pkg") base = [];
    else if (parts[0] === "self" || parts[0] === "super") {
      const identity = view.module.identity === "" ? [] : view.module.identity.split(".");
      base = view.module.path.endsWith("/mod.hd") ? identity : identity.slice(0, -1);
      if (parts[0] === "super") base = base.slice(0, -1);
    } else return undefined;
    const identity = [...base, ...parts.slice(1)].join(".");
    return this.views.find((candidate) => candidate.module.identity === identity);
  }

  /** Items `view` declares or re-exports under `name`. */
  private resolveItem(
    view: ModuleView,
    name: string,
    depth = 0,
  ): { view: ModuleView; item: Item; via?: string } | undefined {
    const item = this.items(view).get(name);
    if (item) return { view, item };
    if (depth > 8) return undefined;
    for (const use of view.module.program.uses) {
      if (!use.public) continue;
      const entry = use.names.find((candidate) => (candidate.alias ?? candidate.name) === name);
      if (!entry) continue;
      const target = this.useTarget(view, use.module);
      const found = target && this.resolveItem(target, entry.name, depth + 1);
      if (found) return { ...found, via: found.via ?? view.qualified(name) };
    }
    return undefined;
  }

  private implementationsFor(view: ModuleView, name: string): ImplSite[] {
    return this.impls.filter(({ view: site, decl }) => {
      if (baseName(decl.targetName) !== name) return false;
      if (site === view) return true;
      // An impl in another module targets this type unless that module
      // declares its own type of the same name.
      return !this.items(site).has(name);
    });
  }

  private typeMembers(view: ModuleView, owner: string, name: string): SymbolInfo[] {
    const members: SymbolInfo[] = [];
    for (const { view: site, decl } of this.implementationsFor(view, name))
      for (const method of decl.methods)
        members.push(methodSymbol(site, owner, method, decl.traitName, false));
    return members;
  }

  private traitRefs(view: ModuleView, name: string): SymbolTrait[] {
    return this.implementationsFor(view, name)
      .filter(({ decl }) => decl.traitName !== undefined)
      .map(({ view: site, decl }) => ({
        trait: decl.traitName!,
        target: decl.targetName,
        file: site.module.path,
        span: site.span(decl.span),
      }));
  }

  private itemSymbol(view: ModuleView, item: Item, via?: string): SymbolInfo {
    const name = view.qualified(item.decl.name);
    const base = {
      name,
      kind: item.kind,
      module: view.module.identity,
      file: view.module.path,
      span: view.span(item.decl.span),
      ...(via && via !== name ? { via } : {}),
    };
    switch (item.kind) {
      case "function": {
        const { decl } = item;
        const inferred = this.inferred?.functions.get(decl.name);
        return {
          ...base,
          public: decl.public === true,
          signature: view.header(decl.span, decl.public),
          doc: decl.doc ?? null,
          parameters: parameters(view, decl.parameters),
          result: decl.resultOmitted ? (inferred?.result ?? null) : decl.result.name,
          requirements: decl.requirementsOmitted
            ? (inferred?.requirements ?? null)
            : decl.requirements,
          suspending: decl.suspending,
          omitted: omitted(decl),
        };
      }
      case "binding": {
        const { decl } = item;
        return {
          ...base,
          public: false,
          signature: view.header(decl.span, false, "line"),
          doc: null,
          type: decl.annotation?.name ?? this.inferred?.globals.get(decl.name) ?? null,
          omitted: decl.annotation ? [] : ["type"],
        };
      }
      case "data": {
        const { decl } = item;
        return {
          ...base,
          public: decl.public === true,
          signature: view.header(decl.span, decl.public),
          doc: decl.doc ?? null,
          members: [
            ...decl.fields.map((field) => fieldSymbol(view, name, field)),
            ...this.typeMembers(view, name, decl.name),
          ],
          implementations: this.traitRefs(view, decl.name),
        };
      }
      case "enum": {
        const { decl } = item;
        return {
          ...base,
          public: decl.public === true,
          signature: view.header(decl.span, decl.public),
          doc: decl.doc ?? null,
          members: [
            ...decl.sharedFields.map((field) => fieldSymbol(view, name, field)),
            ...decl.variants.map((variant) => variantSymbol(view, name, variant)),
            ...this.typeMembers(view, name, decl.name),
          ],
          implementations: this.traitRefs(view, decl.name),
        };
      }
      case "trait": {
        const { decl } = item;
        return {
          ...base,
          public: decl.public === true,
          signature: view.header(decl.span, decl.public),
          doc: decl.doc ?? null,
          supertraits: decl.supertraits.map((supertrait) => supertrait.name),
          members: [
            ...decl.associatedTypes.map((associated): SymbolInfo => ({
              name: `${name}.${associated.name}`,
              kind: "associated-type",
              module: view.module.identity,
              file: view.module.path,
              span: view.span(associated.span),
              public: decl.public === true,
              signature: `type ${associated.name}${associated.value ? ` = ${associated.value.name}` : ""}`,
              doc: associated.doc ?? null,
              owner: name,
              type: associated.value?.name ?? null,
            })),
            ...decl.methods.map((method) => methodSymbol(view, name, method, decl.name, true)),
          ],
          implementations: this.impls
            .filter(({ decl: impl }) => impl.traitName && baseName(impl.traitName) === decl.name)
            .map(({ view: site, decl: impl }) => ({
              trait: impl.traitName!,
              target: impl.targetName,
              file: site.module.path,
              span: site.span(impl.span),
            })),
        };
      }
    }
  }

  /** Symbols in `view` addressed by the item path `path`. */
  private lookupIn(view: ModuleView, path: readonly string[]): SymbolInfo[] {
    const [first, ...rest] = path;
    if (!first) return [];
    const found = this.resolveItem(view, first);
    if (!found) return [];
    let current = [this.itemSymbol(found.view, found.item, found.via)];
    for (const segment of rest)
      current = current.flatMap((symbol) =>
        (symbol.members ?? []).filter((member) => member.name.endsWith(`.${segment}`)),
      );
    return current;
  }

  lookup(query: string): LookupResult {
    const segments = query.replaceAll("::", ".").split(".").filter(Boolean);
    const rooted = segments[0] === "pkg";
    const names = rooted ? segments.slice(1) : segments;
    const results: SymbolInfo[] = [];
    const seen = new Set<string>();
    const add = (symbols: readonly SymbolInfo[]): void => {
      for (const symbol of symbols) {
        const key = `${symbol.file}:${symbol.span.start.offset}:${symbol.name}`;
        if (!seen.has(key)) {
          seen.add(key);
          results.push(symbol);
        }
      }
    };
    // Module-qualified reading: the longest module path that yields a match.
    for (let split = names.length - 1; split >= 0; split -= 1) {
      const identity = names.slice(0, split).join(".");
      const view = this.views.find((candidate) => candidate.module.identity === identity);
      if (!view) continue;
      const symbols = this.lookupIn(view, names.slice(split));
      if (symbols.length > 0) {
        add(symbols);
        break;
      }
    }
    // Unqualified reading: an item path searched in every module.
    if (!rooted && results.length === 0)
      for (const view of this.views) add(this.lookupIn(view, names));
    const last = names.at(-1);
    const suggestions =
      results.length > 0 || !last ? [] : this.allNames().filter((name) => name.endsWith(last));
    return { symbols: results, suggestions: [...new Set(suggestions)].sort() };
  }

  /** Every qualified item and member name, for suggestions. */
  allNames(): string[] {
    const names: string[] = [];
    const walk = (symbol: SymbolInfo): void => {
      names.push(symbol.name);
      for (const member of symbol.members ?? []) walk(member);
    };
    for (const view of this.views)
      for (const item of this.items(view).values()) walk(this.itemSymbol(view, item));
    return names;
  }
}

function location(symbol: { readonly file: string; readonly span: JsonSpan }): string {
  return `${symbol.file}:${symbol.span.start.line}:${symbol.span.start.column}`;
}

/** `hd def` text: one line per symbol with its location, kind, name, and signature. */
export function formatDefinition(symbol: SymbolInfo): string {
  const via = symbol.via ? ` (as ${symbol.via})` : "";
  return `${location(symbol)}: ${symbol.kind} ${symbol.name}${via}\n  ${symbol.signature}`;
}

function docLines(doc: string | null, indent: string): string[] {
  return doc ? doc.split("\n").map((line) => `${indent}${line}`.trimEnd()) : [];
}

/** `hd doc` text: the definition, its documentation, members, and traits. */
export function formatDocumentation(symbol: SymbolInfo): string {
  const lines = [`${symbol.kind} ${symbol.name}`, `  ${location(symbol)}`, "", symbol.signature];
  if (symbol.via) lines.splice(2, 0, `  re-exported as ${symbol.via}`);
  if (symbol.doc) lines.push("", ...docLines(symbol.doc, ""));
  const inferred = (value: string | null | undefined): string =>
    value === null || value === undefined ? "inferred" : `${value} (inferred)`;
  const facts: string[] = [];
  if (symbol.omitted?.includes("result")) facts.push(`result: ${inferred(symbol.result)}`);
  if (symbol.omitted?.includes("requirements"))
    facts.push(
      `requirements: ${inferred(symbol.requirements?.length === 0 ? "$()" : symbol.requirements && `$ ${symbol.requirements.join(", ")}`)}`,
    );
  if (symbol.omitted?.includes("type")) facts.push(`type: ${inferred(symbol.type)}`);
  if (facts.length > 0) lines.push("", ...facts);
  if (symbol.supertraits && symbol.supertraits.length > 0)
    lines.push("", `supertraits: ${symbol.supertraits.join(", ")}`);
  const groups: [string, SymbolKind[]][] = [
    ["fields", ["field"]],
    ["variants", ["variant"]],
    ["associated types", ["associated-type"]],
    ["methods", ["method", "associated-function"]],
  ];
  for (const [title, kinds] of groups) {
    const members = (symbol.members ?? []).filter((member) => kinds.includes(member.kind));
    if (members.length === 0) continue;
    lines.push("", `${title}:`);
    for (const member of members) {
      const origin = member.trait && symbol.kind !== "trait" ? `  [${member.trait}]` : "";
      const fallback = member.hasDefault ? "  [default]" : "";
      lines.push(`  ${member.signature}${origin}${fallback}  (${location(member)})`);
      lines.push(...docLines(member.doc, "    "));
    }
  }
  if (symbol.implementations && symbol.implementations.length > 0) {
    lines.push("", symbol.kind === "trait" ? "implementations:" : "traits:");
    for (const implementation of symbol.implementations)
      lines.push(
        `  ${symbol.kind === "trait" ? implementation.target : implementation.trait}  (${location(implementation)})`,
      );
  }
  return lines.join("\n");
}
