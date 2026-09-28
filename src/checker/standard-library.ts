import type { FunctionDecl, ImplDecl, MethodDecl, Program } from "../ast.ts";
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
}

const parsedModules = new Map<StandardModule, ParsedModule>();
/** Renamed module sources, parsed once each; most programs rename nothing they import. */
const renamedModules = new Map<string, Program>();

/** The prelude names that a std module declares in hd, by module. */
const PRELUDE_DECLARATIONS: readonly (readonly [StandardModule, string])[] = [
  ["console", "println"],
  ["format", "debug"],
  ["hash", "Hash"],
  ["hash", "Hasher"],
  // The shape surface (spec/14-annotations.md#common-shape-representation).
  ...[
    "DeclarationId",
    "SourcePosition",
    "DeclarationKind",
    "PrimitiveKind",
    "TypeShape",
    "FieldShape",
    "DataShape",
    "VariantShape",
    "EnumShape",
    "ParamShape",
    "FnShape",
    "ShapeMetadata",
  ].map((name) => ["annotation", name] as const),
];

/** std declarations that a prelude trait names, declared when a program mentions the trait. */
const TRAIT_DECLARATIONS: readonly (readonly [StandardModule, string, string])[] = [
  ["format", "DebugWriter", "Debug"],
];

function isStandardModule(name: string): name is StandardModule {
  return (STANDARD_MODULES as readonly string[]).includes(name);
}

/** The hidden name of a `std` declaration that the program did not import. */
export function hiddenStandardName(module: string, name: string): string {
  return `__std_${module}_${name}`;
}

function builtInTarget(implementation: ImplDecl): boolean {
  if (implementation.traitName !== undefined) return false;
  const target = implementation.targetName;
  const base = target.endsWith("?") ? "?" : (target.split("[")[0] ?? target);
  return base === "?" || base === "Result" || BUILT_IN_TARGETS.has(base);
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
  const parsed = parse(source);
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
  // A std declaration may be a prelude name (spec/10-modules.md#prelude).
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

function standardModule(name: StandardModule): ParsedModule {
  const cached = parsedModules.get(name);
  if (cached) return cached;
  const program = parseModule(name, standardSource(name));
  const names = [
    ...program.data.map((declaration) => declaration.name),
    ...program.enums.map((declaration) => declaration.name),
    ...program.traits.map((declaration) => declaration.name),
    ...(program.types ?? []).map((declaration) => declaration.name),
    ...program.functions.map((declaration) => declaration.name),
  ];
  const uses: { module: StandardModule; name: string }[] = [];
  const compilerUses: { module: string; name: string }[] = [];
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std."))
      throw new Error(`std.${name} uses '${declaration.module}', which is not a std module`);
    for (const imported of declaration.names)
      if (isStandardModule(module)) uses.push({ module, name: imported.name });
      else compilerUses.push({ module, name: imported.name });
  }
  const module = { name, program, names, uses, compilerUses };
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

/** Every `.name` the node selects: the member names a program may call. */
function memberNames(node: unknown, names: Set<string>): void {
  if (Array.isArray(node)) {
    for (const item of node) memberNames(item, names);
    return;
  }
  if (!node || typeof node !== "object") return;
  const record = node as Record<string, unknown>;
  if (record.kind === "member" && typeof record.name === "string") names.add(record.name);
  for (const [key, child] of Object.entries(record)) if (key !== "span") memberNames(child, names);
}

/** Every identifier-like string in the node, a superset of the names it mentions. */
function mentionedNames(node: unknown, names: Set<string>): void {
  if (Array.isArray(node)) {
    for (const item of node) mentionedNames(item, names);
    return;
  }
  if (typeof node === "string") {
    for (const word of node.match(/\w+/g) ?? []) names.add(word);
    return;
  }
  if (!node || typeof node !== "object") return;
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

/** Declares the `std` modules and built-in methods that the program uses. */
export function withStandardLibrary(program: Program): Program {
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
  // (spec/10-modules.md#test-cases).
  const timed = program.tests.find((test) => test.timeout);
  if (timed) include("time", timed.span);

  // Prelude names that std declares keep their names
  // (spec/10-modules.md#prelude).
  for (const [module, name] of PRELUDE_DECLARATIONS) localNames.set(`${module}.${name}`, name);
  const nameOf = (module: StandardModule, name: string): string =>
    localNames.get(`${module}.${name}`) ?? hiddenStandardName(module, name);
  const modules = new Map<StandardModule, Program>();
  const moduleProgram = (module: StandardModule): Program => {
    let renamed = modules.get(module);
    if (renamed) return renamed;
    const parsed = standardModule(module);
    const renames = new Map<string, string>();
    for (const name of parsed.names) renames.set(name, nameOf(module, name));
    for (const used of parsed.uses) renames.set(used.name, nameOf(used.module, used.name));
    for (const used of parsed.compilerUses)
      renames.set(used.name, hiddenStandardName(used.module, used.name));
    const source = renameSource(standardSource(module).replace(/^use .*$/gm, ""), renames);
    renamed = renamedModules.get(source) ?? parseModule(module, source);
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
  // The prelude `Debug` names `DebugWriter`, so a program that mentions
  // `Debug` declares it, with the builders and implementations it reaches.
  for (const [module, name, trait] of TRAIT_DECLARATIONS) {
    if (included.has(module) || !mentionedByProgram.has(trait)) continue;
    reached.add(nameOf(module, name));
    if (!spans.has(module)) spans.set(module, program.span);
  }
  // Compiler-generated code, such as a lowered `it_prop`, names a std
  // declaration by its hidden name.
  for (const name of mentionedByProgram) {
    const module = owners.get(name);
    if (module === undefined || !name.startsWith("__std_") || included.has(module)) continue;
    reached.add(name);
    if (!spans.has(module)) spans.set(module, program.span);
  }
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
  const reach = (node: unknown): void => {
    const mentioned = new Set<string>();
    mentionedNames(node, mentioned);
    for (const name of mentioned) {
      const module = owners.get(name);
      if (module === undefined || included.has(module) || reached.has(name)) continue;
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
        if (!whole) reach(node);
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
          .filter(
            (implementation) =>
              !builtInTarget(implementation) && (whole || implementationDeclared(implementation)),
          )
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
      const alias = hiddenStandardName(used.module, used.name);
      if (imported.has(alias)) continue;
      imported.add(alias);
      uses.push({
        kind: "use",
        module: `std.${used.module}`,
        names: [{ name: used.name, alias }],
        span: program.span,
      });
    }
  }
  return { ...program, uses, types, data, enums, traits, functions, implementations };
}
