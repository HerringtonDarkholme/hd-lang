import type { ImplDecl, MethodDecl, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { STANDARD_MODULES, standardSource, type StandardModule } from "../std/index.ts";

// Joins the toy standard library (`src/std/*.hd`) into the one module the
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
  "i64",
  "u8",
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
}

const parsedModules = new Map<StandardModule, ParsedModule>();
/** Renamed module sources, parsed once each; most programs rename nothing they import. */
const renamedModules = new Map<string, Program>();

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

function parseModule(name: StandardModule, source: string): Program {
  const parsed = parse(source);
  if (!parsed.program || parsed.diagnostics.some((d) => d.severity !== "warning"))
    throw new Error(
      `std.${name} does not parse: ${parsed.diagnostics.map((d) => `${d.code}@${d.span.start.line}: ${d.message}`).join("; ")}`,
    );
  return parsed.program;
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
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module))
      throw new Error(`std.${name} uses '${declaration.module}', which is not a std module`);
    for (const imported of declaration.names) uses.push({ module, name: imported.name });
  }
  const module = { name, program, names, uses };
  parsedModules.set(name, module);
  return module;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Renames whole identifiers that are not member names (`.name`) or associated
 * names (`Type::name`). The std sources keep their top-level names distinct
 * from their fields, parameters, and locals, so a textual rename is exact.
 */
function renameSource(source: string, renames: ReadonlyMap<string, string>): string {
  if (renames.size === 0) return source;
  const pattern = new RegExp(
    `(?<![\\w.]|::)(${[...renames.keys()].map(escapeRegExp).join("|")})(?!\\w)`,
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

/** Declares the `std` modules and built-in methods that the program uses. */
export function withStandardLibrary(program: Program): Program {
  // Local names of the program's own std imports, and where each module came in.
  const localNames = new Map<string, string>();
  const spans = new Map<StandardModule, SourceSpan>();
  const included = new Set<StandardModule>();
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
    const source = renameSource(standardSource(module).replace(/^use .*$/gm, ""), renames);
    renamed = renamedModules.get(source) ?? parseModule(module, source);
    renamedModules.set(source, renamed);
    modules.set(module, renamed);
    return renamed;
  };

  // Built-in methods: the methods of `impl` blocks on built-in types whose
  // names are selected, to a fixed point. A selected method that mentions a
  // module declaration includes that module.
  const selected = new Set<string>();
  memberNames(program, selected);
  const chosen = new Map<ImplDecl, MethodDecl[]>();
  let changed = true;
  while (changed) {
    changed = false;
    for (const module of included) {
      const renamed = moduleProgram(module);
      memberNames(renamed.functions, selected);
      memberNames(
        renamed.implementations.filter((item) => !builtInTarget(item)),
        selected,
      );
    }
    for (const module of STANDARD_MODULES) {
      const parsed = standardModule(module);
      for (const implementation of moduleProgram(module).implementations) {
        if (!builtInTarget(implementation)) continue;
        const methods = chosen.get(implementation) ?? [];
        for (const method of implementation.methods) {
          if (methods.includes(method) || !selected.has(method.name)) continue;
          methods.push(method);
          chosen.set(implementation, methods);
          changed = true;
          memberNames(method, selected);
          const mentioned = new Set<string>();
          mentionedNames(method, mentioned);
          if (parsed.names.some((name) => mentioned.has(nameOf(module, name))))
            include(module, program.span);
        }
      }
    }
  }
  if (included.size === 0 && chosen.size === 0) return program;

  // Module declarations, each respanned to the use that included the module.
  const types = [...(program.types ?? [])];
  const data = [...program.data];
  const enums = [...program.enums];
  const traits = [...program.traits];
  const functions = [...program.functions];
  const implementations = [...program.implementations];
  for (const module of STANDARD_MODULES) {
    if (!included.has(module)) continue;
    const renamed = moduleProgram(module);
    const span = spans.get(module)!;
    types.push(...respan(renamed.types ?? [], span));
    data.push(...respan(renamed.data, span));
    enums.push(...respan(renamed.enums, span));
    traits.push(...respan(renamed.traits, span));
    functions.push(...respan(renamed.functions, span));
    implementations.push(
      ...respan(
        renamed.implementations.filter((implementation) => !builtInTarget(implementation)),
        span,
      ),
    );
  }
  for (const [implementation, methods] of chosen)
    implementations.push(respan({ ...implementation, methods, standard: true }, program.span));
  return { ...program, types, data, enums, traits, functions, implementations };
}
