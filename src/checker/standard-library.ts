import {
  TEMPLATE_PLACEHOLDER,
  type DataDecl,
  type EnumDecl,
  type FunctionDecl,
  type ImplDecl,
  type MethodDecl,
  type Program,
  type UseDecl,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { STANDARD_MODULES, standardSource, type StandardModule } from "./standard-sources.ts";

// Joins the toy standard library (`lib/std/*.hd`) into the one module the
// prototype compiles.
//
// - A module's top-level declarations are declared when the program imports
//   one of its names, or when another included module uses it. Each is
//   declared under the importing program's local name, or under the hidden
//   name `__std_<module>_<Name>`.
// - An inherent implementation on a built-in type (`impl string:`,
//   `impl[T] T?:`) needs no `use` (09-traits.md#r-trait.own.inherent.std).
//   Only its methods whose names the program selects with `.name` are
//   declared, so an unrelated program compiles as before.
//
// Every added declaration's span points at the `use` that brought it in, or
// at the program when there is none.

const BUILT_IN_TARGETS = new Set([
  "string",
  "bool",
  "char",
  "i32",
  "i8",
  "i16",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "f32",
  "f64",
  "List",
  "Map",
]);

interface ParsedModule {
  readonly name: StandardModule;
  readonly program: Program;
  /** Top-level declaration names, which the loader renames. */
  readonly names: readonly string[];
  /** `use std.<module>.<Name>` lines of the module itself. */
  readonly uses: readonly { readonly module: StandardModule; readonly name: string }[];
  /**
   * `use` lines of a std module that the compiler provides rather than
   * `lib/std`, such as `use std.task.block_on`. The joined program imports
   * each under its hidden name, so the name keeps its ordinary meaning.
   */
  readonly compilerUses: readonly { readonly module: string; readonly name: string }[];
  /**
   * The `use` lines whose names only the module's templates and their
   * protocol implementations mention, such as `std.testing`'s use of
   * `std.testing.arbitrary.Generator`. They join nothing by themselves: a
   * derivation that instantiates the template brings them in
   * (`standardTemplate`).
   */
  readonly templateUses: readonly { readonly module: StandardModule; readonly name: string }[];
  readonly templateCompilerUses: readonly { readonly module: string; readonly name: string }[];
}

const parsedModules = new Map<StandardModule, ParsedModule>();
/** Renamed module sources, parsed once each; most programs rename nothing they import. */
const renamedModules = new Map<string, Program>();
/** A std module's implementations, by renamed source, for its templates. */
const templateModules = new Map<string, readonly ImplDecl[]>();

/** The prelude names that a std module declares in hd, by module. */
const PRELUDE_DECLARATIONS: readonly (readonly [StandardModule, string])[] = [
  ["cmp", "Eq"],
  ["cmp", "PartialOrd"],
  ["cmp", "Ord"],
  ["cmp", "Ordering"],
  ["console", "Console"],
  ["console", "println"],
  ["format", "Display"],
  ["format", "Debug"],
  ["format", "debug"],
  ["hash", "Hash"],
  ["hash", "Hasher"],
  ["iter", "Iterator"],
  ["iter", "Iterable"],
];

/**
 * Methods that declare a std prelude type. A program that selects one, as in
 * `values.iter().filter(keep)`, declares `Iterator` even when it never names
 * it (spec/std/iter.md#iterator-adapters).
 */
const PRELUDE_TYPE_METHODS: readonly (readonly [StandardModule, string, readonly string[]])[] = [
  ["iter", "Iterator", ["iter", "take", "enumerate", "fold", "collect"]],
  // A std type's text, as `(1, "a").to_string()` (05-expressions.md#r-expr.interp.std.tuple.template).
  ["format", "Display", ["to_string"]],
];

function isStandardModule(name: string): name is StandardModule {
  return (STANDARD_MODULES as readonly string[]).includes(name);
}

/** The hidden name of a `std` declaration that the program did not import. */
function hiddenStandardName(module: string, name: string): string {
  return `__std_${module}_${name}`;
}

/** What a checked `assert_equal` or `snapshot` call runs (lib/std/testing.hd). */
export const CHECK_EQUAL = hiddenStandardName("testing", "check_equal");

/**
 * Compiler-provided names that the prototype declares only under their
 * standard names, since it supports no alias for them
 * (checker/standard-traits.ts): a std module's use of one keeps the name.
 */
const UNALIASED_COMPILER_NAMES = new Set(["inspect.Inspectable", "inspect.TypeId"]);

/** The name a std module's use of a compiler-provided name has in the joined program. */
function compilerUseName(module: string, name: string): string {
  return UNALIASED_COMPILER_NAMES.has(`${module}.${name}`)
    ? name
    : hiddenStandardName(module, name);
}

/** A `use` of a compiler-provided name, under the name `compilerUseName` gives it. */
function compilerUse(
  used: { readonly module: string; readonly name: string },
  span: SourceSpan,
): UseDecl {
  const alias = compilerUseName(used.module, used.name);
  return {
    kind: "use",
    module: `std.${used.module}`,
    names: [alias === used.name ? { name: used.name } : { name: used.name, alias }],
    span,
  };
}

/** Whether the program already imports `name` from `module` under `local`. */
function imports(program: Program, module: string, name: string, local: string): boolean {
  return program.uses.some(
    (declaration) =>
      declaration.module === module &&
      declaration.names.some(
        (imported) => imported.name === name && (imported.alias ?? imported.name) === local,
      ),
  );
}

function builtInBase(implementation: ImplDecl): boolean {
  const target = implementation.targetName;
  const base = target.endsWith("?") ? "?" : (target.split("[")[0] ?? target);
  return (
    base === "?" || base === "Result" || tupleTarget(implementation) || BUILT_IN_TARGETS.has(base)
  );
}

/**
 * A tuple type, such as `(A, B)`. Its std implementations, up to 12
 * elements, are declared only for code that mentions a tuple
 * (`mentionedNames`), so a program without tuples does not compile them.
 */
function tupleTarget(implementation: ImplDecl): boolean {
  return implementation.targetName.startsWith("(");
}

function builtInTarget(implementation: ImplDecl): boolean {
  return implementation.traitName === undefined && builtInBase(implementation);
}

/**
 * A std trait implementation for a built-in type, such as the primitive
 * `impl Add[i32] for i32` of `std.ops`. It is declared only when the program,
 * or a std declaration it gets, names the trait, so that a program which
 * imports some other `std.ops` name does not compile every operator
 * implementation.
 */
function builtInTraitImplementation(implementation: ImplDecl): boolean {
  return implementation.traitName !== undefined && builtInBase(implementation);
}

/**
 * The name in a std function's `@intrinsic("name")` line. The line is
 * prototype-internal and std-only: the loader turns it into
 * `FunctionDecl.intrinsic`, so the checker never sees it, while the same line
 * in user code stays a rejected function decorator (src/README.md,
 * Compiler/library boundary).
 */
function intrinsicName(declaration: FunctionDecl): string | undefined {
  const decorators = declaration.decorators;
  const fact = decorators?.facts[0];
  if (!decorators || decorators.derives.length > 0 || decorators.facts.length !== 1) return;
  if (fact?.kind !== "call" || fact.callee.kind !== "name" || fact.callee.name !== "intrinsic")
    return;
  const [argument] = fact.arguments;
  return fact.arguments.length === 1 && argument?.kind === "string" ? argument.value : undefined;
}

function parseModule(name: StandardModule, source: string): Program {
  const parsed = parse(source, { standardLibrary: true });
  if (!parsed.program || parsed.diagnostics.some((d) => d.severity !== "warning"))
    throw new Error(
      `std.${name} does not parse: ${parsed.diagnostics.map((d) => `${d.code}@${d.span.start.line}: ${d.message}`).join("; ")}`,
    );
  const functions = parsed.program.functions.map((declaration) => {
    const intrinsic = intrinsicName(declaration);
    if (!intrinsic) return { ...declaration, standard: true };
    const { decorators: _decorators, ...rest } = declaration;
    return { ...rest, intrinsic, standard: true };
  });
  // A std declaration may be a prelude name (spec/lang/10-modules.md#prelude).
  const standard = <T>(items: readonly T[]): T[] =>
    items.map((item) => ({ ...item, standard: true }));
  return {
    ...parsed.program,
    data: standard(parsed.program.data),
    enums: standard(parsed.program.enums),
    traits: standard(parsed.program.traits),
    functions,
  };
}

const declaredModules = new Map<
  StandardModule,
  { readonly program: Program; readonly names: readonly string[] }
>();

/** A module's parsed source and the names it declares. */
function declaredModule(name: StandardModule): {
  readonly program: Program;
  readonly names: readonly string[];
} {
  const cached = declaredModules.get(name);
  if (cached) return cached;
  const program = parseModule(name, standardSource(name));
  const names = [
    ...program.data.map((declaration) => declaration.name),
    ...program.enums.map((declaration) => declaration.name),
    ...program.traits.map((declaration) => declaration.name),
    ...(program.types ?? []).map((declaration) => declaration.name),
    ...program.functions.map((declaration) => declaration.name),
  ];
  const declared = { program, names };
  declaredModules.set(name, declared);
  return declared;
}

function standardModule(name: StandardModule): ParsedModule {
  const cached = parsedModules.get(name);
  if (cached) return cached;
  const { program, names } = declaredModule(name);
  const uses: { module: StandardModule; name: string }[] = [];
  const compilerUses: { module: string; name: string }[] = [];
  const templateUses: { module: StandardModule; name: string }[] = [];
  const templateCompilerUses: { module: string; name: string }[] = [];
  // The names that the module's declarations and ordinary implementations
  // mention; any other imported name is used only by a template part.
  const structure = new Set(
    program.uses
      .filter((declaration) => declaration.module === `std.${STRUCTURE}`)
      .flatMap((declaration) => declaration.names.map((imported) => imported.name)),
  );
  const outside = new Set<string>();
  mentionedNames(
    [
      ...(program.types ?? []),
      ...program.data,
      ...program.enums,
      ...program.traits,
      ...program.functions,
      ...program.implementations.filter(
        (implementation) => !isTemplatePart(implementation, structure),
      ),
    ],
    outside,
  );
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std."))
      throw new Error(`std.${name} uses '${declaration.module}', which is not a std module`);
    for (const imported of declaration.names)
      // A compiler-provided name, such as `std.testing.assert` or
      // `std.task.block_on`, is a compiler use: its module does not declare it.
      if (isStandardModule(module) && declaredModule(module).names.includes(imported.name))
        (outside.has(imported.name) ? uses : templateUses).push({ module, name: imported.name });
      else if (module === STRUCTURE || outside.has(imported.name))
        compilerUses.push({ module, name: imported.name });
      else templateCompilerUses.push({ module, name: imported.name });
  }
  const module = { name, program, names, uses, compilerUses, templateUses, templateCompilerUses };
  parsedModules.set(name, module);
  return module;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Renames whole identifiers that are not member names (`.name`) or associated
 * names (`Type::name`), nor a string literal's first word, such as the
 * name in `@intrinsic("string_lower")`. The std sources keep their
 * top-level names distinct from their fields, parameters, and locals, so a
 * textual rename is exact.
 */
function renameSource(source: string, renames: ReadonlyMap<string, string>): string {
  if (renames.size === 0) return source;
  const pattern = new RegExp(
    `(?<![\\w."]|::)(${[...renames.keys()].map(escapeRegExp).join("|")})(?!\\w)`,
    "g",
  );
  return source.replace(pattern, (name) => renames.get(name) ?? name);
}

function respan<T>(value: T, span: SourceSpan): T {
  if (Array.isArray(value)) return value.map((item) => respan(item, span)) as T;
  if (!value || typeof value !== "object") return value;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(value))
    result[key] = key === "span" ? span : respan(child, span);
  return result as T;
}

/**
 * Every `.name` or `Owner::name` the node selects: the member names a
 * program may call, such as `string::from_utf8`.
 */
function memberNames(node: unknown, names: Set<string>): void {
  if (Array.isArray(node)) {
    for (const item of node) memberNames(item, names);
    return;
  }
  if (!node || typeof node !== "object") return;
  const record = node as Record<string, unknown>;
  if (
    (record.kind === "member" || record.kind === "qualified-name") &&
    typeof record.name === "string"
  )
    names.add(record.name);
  for (const [key, child] of Object.entries(record)) if (key !== "span") memberNames(child, names);
}

/**
 * The traits a comparison operator calls (05-expressions.md#r-expr.eq.calls-eq,
 * #r-expr.ord.partial-cmp), and `assert_equal`'s `Eq` bound. `PartialOrd`
 * extends `Eq`.
 */
const OPERATOR_TRAITS = new Map<string, readonly string[]>([
  ...["==", "!=", "assert_equal"].map((name): [string, string[]] => [name, ["Eq"]]),
  ...["<", "<=", ">", ">="].map((name): [string, string[]] => [name, ["Eq", "PartialOrd"]]),
]);

/**
 * Every identifier-like string in the node, a superset of the names it
 * mentions. A comparison operator mentions the trait it calls, and a tuple
 * type, such as `(i32, string)` or `List[(K, V)]`, mentions `tuple`, as a
 * tuple expression or pattern (`kind: "tuple"`) does. A map type, literal,
 * or comprehension mentions `Hash`, which its key type's bound
 * `Map[K < Eq & Hash, V]` checks (04-type-system.md#map-key-types), and
 * `tuple`, since iterating a map yields `(K, V)` pairs.
 */
const MAP_MENTIONS = ["Hash", "tuple"];

export function mentionedNames(node: unknown, names: Set<string>): void {
  if (Array.isArray(node)) {
    for (const item of node) mentionedNames(item, names);
    return;
  }
  if (typeof node === "string") {
    for (const word of node.match(/\w+/g) ?? []) {
      names.add(word);
      if (word === "Map") for (const name of MAP_MENTIONS) names.add(name);
    }
    for (const trait of OPERATOR_TRAITS.get(node) ?? []) names.add(trait);
    if (/(^|\W)\(/.test(node)) names.add("tuple");
    return;
  }
  if (!node || typeof node !== "object") return;
  const kind = (node as { readonly kind?: unknown }).kind;
  if (kind === "map" || kind === "map-comprehension")
    for (const name of MAP_MENTIONS) names.add(name);
  // An interpolated value is written through `Display`
  // (05-expressions.md#string-interpolation).
  if (kind === "interpolated-string") names.add("Display");
  for (const [key, child] of Object.entries(node)) if (key !== "span") mentionedNames(child, names);
}

type ModuleDeclaration = { readonly name: string };

/** Top-level declarations of a (renamed) module, in declaration order. */
function declarationsOf(program: Program): readonly ModuleDeclaration[] {
  return [
    ...(program.types ?? []),
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...program.functions,
  ];
}

function baseName(type: string): string {
  return type.split("[")[0] ?? type;
}

/**
 * Records each data type's qualified name, such as `std.annotation.Annotate`,
 * which the compiler recognizes whatever local name it has
 * (spec/lang/14-annotations.md#r-annot.target.recognized). A rename keeps the
 * declaration order.
 */
function withStandardNames(renamed: Program, original: ParsedModule): Program {
  return {
    ...renamed,
    data: renamed.data.map((declaration, index) => ({
      ...declaration,
      standardName: `std.${original.name}.${original.program.data[index]!.name}`,
    })),
    traits: renamed.traits.map((declaration, index) => ({
      ...declaration,
      standardName: `std.${original.name}.${original.program.traits[index]!.name}`,
    })),
  };
}

/**
 * The local names of the program's `std` imports that name a function with
 * no parameters, which a bare decorator calls
 * (spec/lang/14-annotations.md#r-annot.decorator.bare-call).
 */
export function importedMarkerFunctions(program: Program): Set<string> {
  const markers = new Set<string>();
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const functions = standardModule(module).program.functions;
    for (const imported of declaration.names)
      if (functions.some((item) => item.name === imported.name && item.parameters.length === 0))
        markers.add(imported.alias ?? imported.name);
  }
  return markers;
}

/** The `std` module and declared name of a name the program uses, local or hidden. */
function standardOrigin(
  program: Program,
  local: string,
): { readonly module: StandardModule; readonly name: string } | undefined {
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const imported = declaration.names.find((item) => (item.alias ?? item.name) === local);
    if (imported) return { module, name: imported.name };
  }
  for (const module of STANDARD_MODULES)
    if (local.startsWith(hiddenStandardName(module, "")))
      return { module, name: local.slice(hiddenStandardName(module, "").length) };
  return undefined;
}

/** Renames a std module's text into the program's names. */
function programNames(program: Program, module: StandardModule): (text: string) => string {
  const localNames = standardLocalNames(program);
  const nameOf = (owner: StandardModule, name: string): string =>
    localNames.get(`${owner}.${name}`) ?? hiddenStandardName(owner, name);
  const renames = moduleRenames(standardModule(module), nameOf);
  return (text) => renameSource(text, renames);
}

/**
 * The result type's declaration of the `std` function that the program
 * calls by `callee`, its local or hidden name, with that declaration's name
 * and bound texts in the program's names. Typed facts read it
 * (spec/lang/14-annotations.md#member-typed-facts).
 */
export function standardResultDeclaration(
  program: Program,
  callee: string,
): { readonly declaration: DataDecl | EnumDecl; readonly name: string } | undefined {
  const found = standardOrigin(program, callee);
  if (!found) return undefined;
  const parsed = standardModule(found.module);
  const fn = parsed.program.functions.find((item) => item.name === found.name);
  if (!fn) return undefined;
  const base = baseName(fn.result.name);
  const declaration = [...parsed.program.data, ...parsed.program.enums].find(
    (item) => item.name === base,
  );
  if (!declaration) return undefined;
  const rename = programNames(program, found.module);
  const bounds = (declaration.genericBounds ?? []).map((bound) => ({
    ...bound,
    traits: bound.traits.map(rename),
  }));
  return { declaration: { ...declaration, genericBounds: bounds }, name: rename(base) };
}

/**
 * The supertraits without arguments of the `std` trait that the program
 * names `local`, in the program's names, as `Num` for `std.num.Integer`.
 */
export function standardSupertraits(program: Program, local: string): readonly string[] {
  const found = standardOrigin(program, local);
  if (!found) return [];
  const trait = standardModule(found.module).program.traits.find(
    (item) => item.name === found.name,
  );
  const rename = programNames(program, found.module);
  return (trait?.supertraits ?? [])
    .filter((supertrait) => !supertrait.name.includes("["))
    .map((supertrait) => rename(supertrait.name));
}

/** How a module's source is renamed into the program: its names and the names it uses. */
function moduleRenames(
  parsed: ParsedModule,
  nameOf: (module: StandardModule, name: string) => string,
  structureNames: ReadonlyMap<string, string> = new Map(),
): Map<string, string> {
  const renames = new Map<string, string>();
  for (const name of parsed.names) renames.set(name, nameOf(parsed.name, name));
  for (const used of [...parsed.uses, ...parsed.templateUses])
    renames.set(used.name, nameOf(used.module, used.name));
  for (const used of [...parsed.compilerUses, ...parsed.templateCompilerUses])
    if (used.module !== STRUCTURE) renames.set(used.name, compilerUseName(used.module, used.name));
    else if (structureNames.has(used.name)) renames.set(used.name, structureNames.get(used.name)!);
  return renames;
}

/** The node with each `name` equal to `from` replaced by `to`. */
function renamed<T>(node: T, from: string, to: string): T {
  if (Array.isArray(node)) return node.map((item) => renamed(item, from, to)) as T;
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(node))
    result[key] = key === "name" && child === from ? to : renamed(child, from, to);
  return result as T;
}

/**
 * A desugared prefixed string names `std.ops.Template` by its hidden name
 * (`TEMPLATE_PLACEHOLDER`), which is declared only when the program does not
 * import `Template`; an import renames it to the local name
 * (spec/lang/05-expressions.md#r-expr.literal-fn.no-marker-import).
 */
function withTemplateName(program: Program): Program {
  for (const declaration of program.uses)
    for (const imported of declaration.names)
      if (declaration.module === "std.ops" && imported.name === "Template")
        return renamed(program, TEMPLATE_PLACEHOLDER, imported.alias ?? imported.name);
  return program;
}

/** `std.structure`, which the typed-derivation pass declares (checker/typed-derivation.ts). */
const STRUCTURE = "structure";

/** The `std.structure` names a module imports. */
function structureNamesOf(module: ParsedModule): Set<string> {
  return new Set(
    module.compilerUses.filter((used) => used.module === STRUCTURE).map((used) => used.name),
  );
}

/**
 * A module's templates, and its implementations of the `std.structure`
 * protocol traits, which only a derivation instantiates
 * (spec/lang/14-annotations.md#templates).
 */
function isTemplatePart(implementation: ImplDecl, structure: ReadonlySet<string>): boolean {
  return (
    implementation.byStructure !== undefined ||
    (implementation.traitName !== undefined && structure.has(baseName(implementation.traitName)))
  );
}

/**
 * A renamed module without its template parts: the typed-derivation pass
 * instantiates them through `standardTemplate`, and they name
 * `std.structure`, which the program declares only when it derives.
 */
function withoutTemplates(renamed: Program, original: ParsedModule): Program {
  const structure = structureNamesOf(original);
  return withStandardNames(
    {
      ...renamed,
      implementations: renamed.implementations.filter(
        (implementation) => !isTemplatePart(implementation, structure),
      ),
    },
    original,
  );
}

/** The program's local names of the `std` declarations it imports, and the prelude names. */
function standardLocalNames(program: Program): Map<string, string> {
  const localNames = new Map<string, string>();
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const declared = standardModule(module).names;
    for (const imported of declaration.names)
      if (declared.includes(imported.name))
        localNames.set(`${module}.${imported.name}`, imported.alias ?? imported.name);
  }
  for (const [module, name] of PRELUDE_DECLARATIONS) localNames.set(`${module}.${name}`, name);
  return localNames;
}

/**
 * The template of the `std` trait that the program imports as `trait`,
 * with the module's `std.structure` protocol implementations, written with
 * the program's names: a name it imports by its local name, any other `std`
 * name by its hidden name, and `std.structure` names as the typed-derivation
 * pass declares them (spec/lang/14-annotations.md#templates).
 */
export interface StandardTemplate {
  readonly template: ImplDecl;
  readonly support: readonly ImplDecl[];
  /** The compiler-provided names that only the template parts use. */
  readonly uses: readonly UseDecl[];
  /** Every implementation of the trait's module, in the program's names. */
  readonly implementations: readonly ImplDecl[];
}

export function standardTemplate(
  program: Program,
  trait: string,
  /** The names of `std.structure` items the program renames (typed-derivation.ts). */
  structureNames: ReadonlyMap<string, string>,
  /** The trait's tuple template, `impl[T < Tuple] Trait for T by Structure`, rather than its template. */
  tuple = false,
): StandardTemplate | undefined {
  // The trait's module: one the program imports it from, or, for a prelude
  // trait such as `Eq` or `Hash`, the module that declares it.
  const candidates: { readonly module: StandardModule; readonly span: SourceSpan }[] = [];
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const imported = declaration.names.find((name) => (name.alias ?? name.name) === trait);
    if (imported && standardModule(module).names.includes(imported.name))
      candidates.push({ module, span: declaration.span });
  }
  for (const [module, name] of PRELUDE_DECLARATIONS)
    if (name === trait) candidates.push({ module, span: program.span });
  for (const { module, span } of candidates) {
    const parsed = standardModule(module);
    const localNames = standardLocalNames(program);
    const nameOf = (owner: StandardModule, name: string): string =>
      localNames.get(`${owner}.${name}`) ?? hiddenStandardName(owner, name);
    const renames = moduleRenames(parsed, nameOf, structureNames);
    const source = renameSource(standardSource(module).replace(/^use .*$/gm, ""), renames);
    const structure = new Set(
      [...structureNamesOf(parsed)].map((name) => structureNames.get(name) ?? name),
    );
    let implementations = templateModules.get(source);
    if (!implementations) {
      implementations = parseModule(module, source).implementations;
      templateModules.set(source, implementations);
    }
    // The template, or the tuple template, whose parameter is bounded by
    // `Tuple` (annot.template.tuple.separate).
    const template = implementations.find(
      (implementation) =>
        implementation.byStructure !== undefined &&
        implementation.genericBounds.length === (tuple ? 1 : 0) &&
        baseName(implementation.traitName ?? "") === trait,
    );
    if (!template) return undefined;
    const support = implementations.filter(
      (implementation) =>
        implementation.byStructure === undefined && isTemplatePart(implementation, structure),
    );
    const uses = parsed.templateCompilerUses
      .map((used) => compilerUse(used, span))
      .filter((use) => {
        const imported = use.names[0]!;
        return !imports(program, use.module, imported.name, imported.alias ?? imported.name);
      });
    return { template, support, uses, implementations };
  }
  return undefined;
}

/**
 * The local names of the `std` traits with a tuple template that the
 * program sees: prelude traits, such as `Eq`, and imported ones, such as
 * `std.ops.Default` (spec/lang/14-annotations.md#tuple-templates).
 */
export function standardTupleTraits(program: Program): readonly string[] {
  const localNames = standardLocalNames(program);
  const traits: string[] = [];
  for (const module of STANDARD_MODULES)
    for (const implementation of declaredModule(module).program.implementations) {
      if (implementation.byStructure === undefined || implementation.genericBounds.length !== 1)
        continue;
      const local = localNames.get(`${module}.${baseName(implementation.traitName ?? "")}`);
      if (local !== undefined && !traits.includes(local)) traits.push(local);
    }
  return traits;
}

/** Declares the `std` modules and built-in methods that the program uses. */
export function withStandardLibrary(source: Program): Program {
  const program = withTemplateName(source);
  // Local names of the program's own std imports, and where each module came in.
  const localNames = new Map<string, string>();
  const spans = new Map<StandardModule, SourceSpan>();
  // Modules declared whole: imported ones and their dependencies.
  const included = new Set<StandardModule>();
  // Single declarations (by declared name) that a built-in method reaches.
  const reached = new Set<string>();
  const include = (module: StandardModule, span: SourceSpan): void => {
    if (!spans.has(module)) spans.set(module, span);
    if (included.has(module)) return;
    included.add(module);
    for (const dependency of standardModule(module).uses) include(dependency.module, span);
  };
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const declared = standardModule(module).names;
    for (const imported of declaration.names) {
      if (!declared.includes(imported.name)) continue;
      localNames.set(`${module}.${imported.name}`, imported.alias ?? imported.name);
      include(module, declaration.span);
    }
  }
  // A test `timeout` is checked against `Duration`, so it declares `std.time`
  // (spec/lang/10-modules.md#test-cases).
  const timed = program.tests.find((test) => test.timeout);
  if (timed) include("time", timed.span);
  // A checked `assert_equal` or `snapshot` call runs `check_equal`.
  const asserting = program.uses.find(
    (declaration) =>
      declaration.module === "std.testing" &&
      declaration.names.some(({ name }) => name === "assert_equal" || name === "snapshot"),
  );
  if (asserting) {
    reached.add(CHECK_EQUAL);
    if (!spans.has("testing")) spans.set("testing", asserting.span);
  }
  // An `all!` call drives the frame that `all_frame` builds
  // (checker/expression-suspensions.ts).
  const awaitingAll = program.uses.find(
    (declaration) =>
      declaration.module === "std.task" && declaration.names.some(({ name }) => name === "all"),
  );
  if (awaitingAll) {
    reached.add(hiddenStandardName("task", "all_frame"));
    if (!spans.has("task")) spans.set("task", awaitingAll.span);
  }

  // Prelude names that std declares keep their names
  // (spec/lang/10-modules.md#prelude).
  for (const [module, name] of PRELUDE_DECLARATIONS) localNames.set(`${module}.${name}`, name);
  const nameOf = (module: StandardModule, name: string): string =>
    localNames.get(`${module}.${name}`) ?? hiddenStandardName(module, name);
  const modules = new Map<StandardModule, Program>();
  const moduleProgram = (module: StandardModule): Program => {
    let renamed = modules.get(module);
    if (renamed) return renamed;
    const parsed = standardModule(module);
    const renames = moduleRenames(parsed, nameOf);
    const source = renameSource(standardSource(module).replace(/^use .*$/gm, ""), renames);
    renamed = renamedModules.get(source) ?? withoutTemplates(parseModule(module, source), parsed);
    renamedModules.set(source, renamed);
    modules.set(module, renamed);
    return renamed;
  };
  // Every std declaration by its declared name, and the module it belongs to.
  const owners = new Map<string, StandardModule>();
  for (const module of STANDARD_MODULES)
    for (const name of standardModule(module).names) owners.set(nameOf(module, name), module);
  const declared = (name: string): boolean => {
    const module = owners.get(name);
    return module === undefined || included.has(module) || reached.has(name);
  };
  // An implementation on a std type, or of a std trait, is declared with them.
  const implementationDeclared = (implementation: ImplDecl): boolean =>
    declared(baseName(implementation.targetName)) &&
    (implementation.traitName === undefined || declared(baseName(implementation.traitName)));

  // A program that mentions a std-declared prelude name gets its declaration,
  // unless it declares that name itself, which is a prelude-name-shadow error.
  const mentionedByProgram = new Set<string>();
  mentionedNames(program, mentionedByProgram);
  // Compiler-generated code, such as a lowered `it_prop`, names a std
  // declaration by its hidden name.
  for (const name of mentionedByProgram) {
    const module = owners.get(name);
    if (module === undefined || !name.startsWith("__std_") || included.has(module)) continue;
    reached.add(name);
    if (!spans.has(module)) spans.set(module, program.span);
  }
  const selectedByProgram = new Set<string>();
  memberNames(program, selectedByProgram);
  for (const [, name, methods] of PRELUDE_TYPE_METHODS)
    if (methods.some((method) => selectedByProgram.has(method))) mentionedByProgram.add(name);
  for (const [module, name] of PRELUDE_DECLARATIONS) {
    if (!mentionedByProgram.has(name) || included.has(module)) continue;
    if (
      [...program.functions, ...program.data, ...program.enums, ...program.traits].some(
        (declaration) => declaration.name === name,
      )
    )
      continue;
    reached.add(name);
    if (!spans.has(module)) spans.set(module, program.span);
  }

  // Built-in methods: the methods of `impl` blocks on built-in types whose
  // names are selected, to a fixed point. A selected method reaches the std
  // declarations it mentions, and each reached declaration reaches those it
  // mentions, so a method call declares only the helpers it needs.
  const selected = new Set<string>();
  memberNames(program, selected);
  const chosen = new Map<ImplDecl, MethodDecl[]>();
  const scanned = new Set<unknown>();
  // A whole module reaches only the prelude names it mentions; it `use`s the rest.
  const prelude = new Set(PRELUDE_DECLARATIONS.map(([, name]) => name));
  const reach = (node: unknown, preludeOnly = false): void => {
    const mentioned = new Set<string>();
    mentionedNames(node, mentioned);
    for (const name of mentioned) {
      const module = owners.get(name);
      if (module === undefined || included.has(module) || reached.has(name)) continue;
      if (preludeOnly && !prelude.has(name)) continue;
      reached.add(name);
      if (!spans.has(module)) spans.set(module, program.span);
    }
  };
  let changed = true;
  while (changed) {
    changed = false;
    for (const module of STANDARD_MODULES) {
      if (!spans.has(module)) continue;
      const renamed = moduleProgram(module);
      const whole = included.has(module);
      const nodes = [
        ...declarationsOf(renamed).filter((item) => whole || reached.has(item.name)),
        ...renamed.implementations.filter(
          (item) => !builtInTarget(item) && (whole || implementationDeclared(item)),
        ),
      ];
      for (const node of nodes) {
        if (scanned.has(node)) continue;
        scanned.add(node);
        changed = true;
        memberNames(node, selected);
        reach(node, whole);
      }
    }
    for (const module of STANDARD_MODULES) {
      for (const implementation of moduleProgram(module).implementations) {
        if (!builtInTarget(implementation)) continue;
        const methods = chosen.get(implementation) ?? [];
        for (const method of implementation.methods) {
          if (methods.includes(method) || !selected.has(method.name)) continue;
          methods.push(method);
          chosen.set(implementation, methods);
          changed = true;
          memberNames(method, selected);
          reach(method);
        }
      }
    }
  }
  if (spans.size === 0 && chosen.size === 0) return program;

  // The trait names that decide which built-in trait implementations are
  // declared: those the program mentions, and those the kept std
  // declarations mention, apart from each trait's own name.
  const traitMentions = new Set(mentionedByProgram);
  for (const module of STANDARD_MODULES) {
    if (!spans.has(module)) continue;
    const renamed = moduleProgram(module);
    const whole = included.has(module);
    for (const item of declarationsOf(renamed)) {
      if (!whole && !reached.has(item.name)) continue;
      const { name: _name, ...rest } = item;
      mentionedNames(rest, traitMentions);
    }
    for (const implementation of renamed.implementations)
      if (
        !builtInTarget(implementation) &&
        !builtInTraitImplementation(implementation) &&
        (whole || implementationDeclared(implementation))
      )
        mentionedNames(implementation, traitMentions);
  }
  const keepImplementation = (implementation: ImplDecl, whole: boolean): boolean =>
    !builtInTarget(implementation) &&
    (whole || implementationDeclared(implementation)) &&
    (!builtInTraitImplementation(implementation) ||
      (traitMentions.has(baseName(implementation.traitName!)) &&
        (!tupleTarget(implementation) || traitMentions.has("tuple"))));

  // Module declarations, each respanned to the use that included the module.
  const types = [...(program.types ?? [])];
  const data = [...program.data];
  const enums = [...program.enums];
  const traits = [...program.traits];
  const functions = [...program.functions];
  const implementations = [...program.implementations];
  for (const module of STANDARD_MODULES) {
    const span = spans.get(module);
    if (!span) continue;
    const renamed = moduleProgram(module);
    const whole = included.has(module);
    const keep = <T extends ModuleDeclaration>(items: readonly T[]): T[] =>
      respan(
        items.filter((item) => whole || reached.has(item.name)),
        span,
      );
    types.push(...keep(renamed.types ?? []));
    data.push(...keep(renamed.data));
    enums.push(...keep(renamed.enums));
    traits.push(...keep(renamed.traits));
    functions.push(...keep(renamed.functions));
    implementations.push(
      ...respan(
        renamed.implementations
          .filter((implementation) => keepImplementation(implementation, whole))
          // `std` owns its prelude traits, so its impls are never orphans.
          .map((implementation) => ({ ...implementation, standard: true })),
        span,
      ),
    );
  }
  for (const [implementation, methods] of chosen)
    implementations.push(respan({ ...implementation, methods, standard: true }, program.span));
  // Compiler-provided names that a joined module uses, under hidden names.
  const uses = [...program.uses];
  const imported = new Set<string>();
  for (const module of STANDARD_MODULES) {
    if (!spans.has(module)) continue;
    for (const used of standardModule(module).compilerUses) {
      const alias = compilerUseName(used.module, used.name);
      if (imported.has(alias) || imports(program, `std.${used.module}`, used.name, alias)) continue;
      imported.add(alias);
      uses.push(compilerUse(used, program.span));
    }
  }
  return { ...program, uses, types, data, enums, traits, functions, implementations };
}
