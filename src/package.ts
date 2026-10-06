import type {
  ForeignName,
  InitGroupStart,
  ModuleScope,
  NamespaceModule,
  PackageScopes,
  Program,
  UseDecl,
} from "./ast.ts";
import { PRELUDE_NAMES } from "./checker/prelude-names.ts";
import {
  isStandardUse,
  joinedText,
  standardForeign,
  standardSpellings,
  wrapTestModule,
} from "./package-join.ts";
import type { ParseOptions } from "./parser/base.ts";
import type { Diagnostic, SourcePosition, SourceSpan } from "./diagnostics.ts";
import { physicalDiagnostic, sourceDocument } from "./diagnostics.ts";
import { KEYWORDS } from "./lexer.ts";
import { parse } from "./parser/index.ts";
import { directoryOf, folderLoops, folders, stronglyConnected } from "./package-folders.ts";
import { nominalGenericParts } from "./types.ts";

// Package linking for the prototype (10-modules.md). Every source file of one
// package is a module named by its path under `src/`, and every file under
// `tests/` is an integration test module named `tests.<path>`. The linker resolves the
// `pkg`, `self`, and `super` uses between those modules and then joins the
// modules reachable from the entry module into one source text, in module
// initialization order, with the package uses removed. That text is an
// ordinary single-module program for the rest of the pipeline.
//
// Joining modules puts their top-level names in one namespace. Each module
// keeps its own names through a scope (`ModuleScope`) that travels beside
// the joined source: a declaration whose name another linked module also
// declares joins under a hidden spelling, and the scope maps the module's
// own spellings, its renamed (`as`) uses, and its module namespace uses
// (`use pkg.user.types`) to the joined spellings. The scope also lists the
// other linked modules' top-level names, and the names their std uses bind,
// that the module neither declares nor imports, so that naming one is an
// error, as it would be if the modules were compiled apart
// (03-names-and-scopes.md#r-names.module.other-module). Std use lines join
// once (package-join.ts).
// The checker applies the scopes before anything else (checker/module-paths.ts).
//
// A `*_test.hd` file is a test module. It joins as a `tests:` block, so the
// joined source may hold several `tests:` blocks and is parsed with
// `joinedModules`. A test build (`LinkOptions.tests`) links every test module.
// Test case names share the joined namespace too, so two modules must not
// name a test case alike.
//
// An integration test module (spec/lang/10-modules.md#r-module.test.integration)
// is test code too. Each file directly under the test root is an integration
// test program: its own program, linked only with the library and shared
// test modules it uses, separately from the other programs
// (spec/lang/10-modules.md#r-module.test.integration.program). A use of one
// program from another module is `unknown-module`
// (spec/lang/10-modules.md#r-module.test.integration.program-use). A module
// in a subdirectory of the test root is shared test code that any program
// may use through `self`. Its scope hides a library module's top-level
// names as any module's does.

export const SOURCE_ROOT = "src/";
/** The default test root, which holds the integration test modules. */
export const TEST_ROOT = "tests/";
/** The directory of the package's tasks and shared task modules (spec/cli/command-line.md#tasks). */
export const TASK_ROOT = "tasks/";
/** The package root module, which `pkg` names (spec/lang/10-modules.md#r-module.path.lib-file). */
export const LIB_FILE = "src/lib.hd";
/**
 * The default executable's entry module, its own program and never part of
 * the library (spec/lang/10-modules.md#r-module.path.main-file).
 */
export const MAIN_FILE = "src/main.hd";

export interface PackageDiagnostic extends Diagnostic {
  /** The package file the diagnostic points into. */
  readonly path: string;
}

interface PackageModule {
  /**
   * The module's file: a package path, such as `src/user.hd`, in the root
   * package, and an absolute path in a dependency package.
   */
  readonly path: string;
  /** The dotted module identity in its package, `""` for `src/lib.hd`. */
  readonly identity: string;
  readonly program?: Program;
  /** The dependency package the module belongs to; absent in the root package. */
  readonly dependency?: DependencyPackage;
}

/**
 * A package that the root package depends on, linked as its own package
 * (spec/lang/10-modules.md#name-resolution-across-packages): its modules
 * keep their own `pkg`, `self`, and `super`, and other packages reach only
 * their `pub` declarations through `dep.NAME`.
 */
export interface DependencyPackage {
  /** Unique among the linked packages, such as `github.com/acme/json@2.1.0`. */
  readonly id: string;
  /** How messages name the package, such as `dep.json`. */
  readonly shown: string;
  /** The package's source root, an absolute path with `/`, which holds `lib.hd`. */
  readonly sourceRoot: string;
  /** Its library's source files, by their paths under the source root, such as `lib.hd`. */
  readonly files: Readonly<Record<string, string>>;
  /** Its own dependencies: each `dep.NAME` name to a package id. */
  readonly dependencies: Readonly<Record<string, string>>;
  /**
   * Set for a version fetched for a version requirement, as its host path
   * and version, such as `github.com/acme/json@2.1.0`. A workspace member or
   * a path requirement's package is the user's own code and leaves it unset
   * (spec/lang/10-modules.md#r-module.dbg.own-code).
   */
  readonly fetched?: string;
}

/** The packages a link may reach through `dep.NAME` (spec/lang/10-modules.md#use-roots). */
export interface PackageDependencies {
  /** The root package's dependencies: each `dep.NAME` name to a package id. */
  readonly dependencies: Readonly<Record<string, string>>;
  /**
   * The root package's dev dependencies, which only its test code and tasks
   * may use (spec/lang/10-modules.md#r-module.test.dev-dependency).
   */
  readonly devDependencies: Readonly<Record<string, string>>;
  /** Every package the root package reaches, by id. */
  readonly packages: Readonly<Record<string, DependencyPackage>>;
}

interface LinkSegment {
  readonly path: string;
  /** First line of the module in the linked source, 1-based. */
  readonly firstLine: number;
  readonly lineCount: number;
  /** Columns the linker indented the module by: 4 for a test module. */
  readonly indent: number;
  /**
   * The columns the linker deleted at the start of a module line, keyed by
   * its 0-based line within the module: a test module's top-level `pub `.
   */
  readonly deleted: ReadonlyMap<number, number>;
}

interface LinkOptions {
  /**
   * A test build (spec/lang/10-modules.md#r-module.test.code): every test module
   * is linked, not only those the entry module reaches.
   */
  readonly tests?: boolean;
  /**
   * The package paths of the executables' entry modules. Each is its own
   * program, which no other module may use
   * (spec/cli/command-line.md#r-cli.exe.entry-no-use). The default is
   * `src/main.hd` alone.
   */
  readonly programs?: readonly string[];
  /** The packages that `dep.NAME` uses reach; without it, the package has no dependencies. */
  readonly dependencies?: PackageDependencies;
  /**
   * A package key for the entry module alone, so that the checker sees it
   * as code of another package: a doc test sees the package as a dependent
   * does (spec/lang/10-modules.md#r-module.test.doc.view).
   */
  readonly entryPackage?: string;
}

/** Whether a package path is an integration test module (spec/lang/10-modules.md#r-module.test.integration). */
function isIntegrationTestPath(path: string): boolean {
  return path.startsWith(TEST_ROOT);
}

/** Whether a package path is a task or a shared task module (spec/cli/command-line.md#tasks). */
function isTaskPath(path: string): boolean {
  return path.startsWith(TASK_ROOT);
}

/**
 * Whether a package path is a program root of its own: an integration test
 * program, or a task, such as `tasks/seed.hd`
 * (spec/cli/command-line.md#r-cli.task.file).
 */
function isRootProgram(path: string): boolean {
  const root = [TEST_ROOT, TASK_ROOT].find((prefix) => path.startsWith(prefix));
  return root !== undefined && !path.slice(root.length).includes("/");
}

/**
 * Whether a package path is a test module or an integration test module
 * (spec/lang/10-modules.md#r-module.test.module): its top level is test code.
 */
function isTestModulePath(path: string): boolean {
  return path.endsWith("_test.hd") || isIntegrationTestPath(path);
}

export interface LinkedPackage {
  /** The joined single-module source; absent when linking failed. */
  readonly source?: string;
  /**
   * Initialization-group starts, as joined-source line numbers in linker
   * order; empty when linking failed. The parser maps them to statement
   * indices on `Program.initGroups` under `joinedModules`.
   */
  readonly initGroups: readonly InitGroupStart[];
  /** Linked modules in initialization order. */
  readonly modules: readonly PackageModule[];
  /**
   * Each linked module's scope over `source`, which the parser puts on the
   * program under `joinedModules`; absent when linking failed.
   */
  readonly packageScopes?: PackageScopes;
  readonly diagnostics: readonly PackageDiagnostic[];
  /**
   * The line of `source` where the entry module starts. Outside a test build
   * the entry module is initialized last, so it runs to the end of `source`,
   * and its lines keep their numbers relative to this one.
   */
  readonly entryLine?: number;
  /**
   * The entry module is a script: top-level statements and no `main`
   * (spec/lang/10-modules.md#r-module.init.script). Never set for a test
   * build, whose entry module is not the last to initialize.
   */
  readonly scriptEntry?: true;
  /**
   * The source of each linked dependency module, by its absolute path, so a
   * diagnostic that points into a dependency can show its line.
   */
  readonly dependencySources: Readonly<Record<string, string>>;
  /** Maps a diagnostic on the linked source back to its package file. */
  locate(diagnostic: Diagnostic): PackageDiagnostic;
}

const IDENTIFIER = /^[\p{ID_Start}_][\p{ID_Continue}_]*$/u;

/**
 * The module identity of a package path, or undefined when it names none. A
 * file under `tests/` is the integration test module `tests.<path>`
 * (spec/lang/10-modules.md#r-module.test.integration); `tests` is a
 * reserved word, so no library module identity starts with it. `src/lib.hd`
 * is the package root module, whose identity is `""`
 * (spec/lang/10-modules.md#r-module.path.lib-file); `src/mod.hd` names no
 * module (spec/lang/10-modules.md#r-module.path.no-root-mod).
 */
export function moduleIdentity(path: string): string | undefined {
  if (path === LIB_FILE) return "";
  const root = [SOURCE_ROOT, TEST_ROOT, TASK_ROOT].find((prefix) => path.startsWith(prefix));
  if (root === undefined || !path.endsWith(".hd")) return undefined;
  const parts = path.slice(root.length, -".hd".length).split("/");
  if (parts.at(-1) === "mod") parts.pop();
  if (parts.length === 0) return undefined;
  const valid = parts.every(
    (part) => IDENTIFIER.test(part) && part.normalize("NFC") === part && !KEYWORDS.has(part),
  );
  if (!valid) return undefined;
  if (root === TEST_ROOT) return ["tests", ...parts].join(".");
  // The tasks root has no name in source (cli.task.root-file), so a task
  // module's identity starts with a word that no use path can spell.
  return root === TASK_ROOT ? [TASKS_IDENTITY, ...parts].join(".") : parts.join(".");
}

/** The first part of a task module's identity; no identifier spells it. */
const TASKS_IDENTITY = "<tasks>";

/**
 * A module's name in a message: its identity, `pkg` for the root module, or
 * its path for a task. A dependency's module starts with the dependency's
 * name, as `dep.json.text`.
 */
function shown(module: PackageModule): string {
  if (module.dependency)
    return [module.dependency.shown, module.identity].filter((part) => part !== "").join(".");
  if (isTaskPath(module.path)) return module.path;
  return module.identity === "" ? "pkg" : module.identity;
}

/**
 * A module's key among every linked module: its identity in the root
 * package, and the package id and identity in a dependency, which no
 * identity spells.
 */
function moduleKey(dependency: DependencyPackage | undefined, identity: string): string {
  return dependency ? `<dep ${dependency.id}>${identity}` : identity;
}

function keyOf(module: PackageModule): string {
  return moduleKey(module.dependency, module.identity);
}

/** The text a module's hidden spellings start from: its identity, after its package's id. */
function hiddenBase(module: PackageModule): string {
  return module.dependency ? `dep_${module.dependency.id}_${module.identity}` : module.identity;
}

/** The `test-only-use` error of a dev dependency used from non-test code (cli.dep.dev-use.remove-first). */
interface TestOnlyUse {
  readonly testOnly: string;
}

/**
 * The package that `dep.NAME` names from `module`, or the message of the
 * `unknown-module` error, or `test-only-use` for a dev dependency. A root module sees the root package's
 * dependencies, and its dev dependencies only from test code or a task
 * (spec/lang/10-modules.md#r-module.test.dev-dependency); a dependency's
 * module sees that package's own dependencies.
 */
function dependencyNamed(
  dependencies: PackageDependencies | undefined,
  module: PackageModule,
  name: string,
): DependencyPackage | TestOnlyUse | string {
  const own = module.dependency;
  const id = own
    ? own.dependencies[name]
    : (dependencies?.dependencies[name] ??
      (isTestModulePath(module.path) || isTaskPath(module.path)
        ? dependencies?.devDependencies[name]
        : undefined));
  const found = id === undefined ? undefined : dependencies?.packages[id];
  if (found) return found;
  if (!own && dependencies?.devDependencies[name] !== undefined)
    return {
      testOnly: `'dep.${name}' is a dev dependency, which only test code and tasks may use; to use it here, run hd remove ${name} first, then hd add ${name} PATH@VERSION`,
    };
  if (own) return `${own.shown} has no dependency named '${name}'`;
  return dependencies === undefined ||
    Object.keys(dependencies.dependencies).length +
      Object.keys(dependencies.devDependencies).length ===
      0
    ? `the package has no dependencies; add '${name}' with hd add ${name} PATH@VERSION`
    : `the package has no dependency named '${name}'; add it with hd add ${name} PATH@VERSION`;
}

/** The `unknown-module` message for a use of a module that `dependency`, or the root package, lacks. */
function missingModule(dependency: DependencyPackage | undefined, identity: string): string {
  if (dependency)
    return identity === ""
      ? `${dependency.shown} has no library: its src/lib.hd does not exist`
      : `${dependency.shown} has no module '${identity}'`;
  return identity === ""
    ? `the package has no root module: 'pkg' names ${LIB_FILE}, which does not exist`
    : `no package module '${identity}'`;
}

/** The missing-module diagnostic for one use, with integration-test guidance at its root. */
function missingUseDiagnostic(
  module: PackageModule,
  declaration: UseDecl,
  dependency: DependencyPackage | undefined,
  identity: string,
  span: SourceSpan,
): PackageDiagnostic {
  const integrationRoot =
    dependency === undefined &&
    isRootProgram(module.path) &&
    declaration.module === "self" &&
    identity === "tests";
  if (!integrationRoot)
    return {
      path: module.path,
      code: "unknown-module",
      message: missingModule(dependency, identity),
      span,
    };
  const names = declaration.names
    .map(({ name, alias }) => (alias ? `${name} as ${alias}` : name))
    .join(", ");
  return {
    path: module.path,
    code: "unknown-module",
    message: missingModule(undefined, module.path),
    notes: [
      `an integration test sees the package as a dependent does: write \`use pkg.{${names}}\``,
    ],
    span,
  };
}

/** Where a package use leads: a package, and a module path in it. */
interface UseTarget {
  /** The package; absent for the root package. */
  readonly dependency: DependencyPackage | undefined;
  readonly path: string[];
  /** `use dep.json`, which names the root module of `json` itself. */
  readonly namespaceOnly: boolean;
}

/**
 * The package and module path a package use names; a message when it names
 * none; undefined for a `std` use. A use through `dep.NAME` names a module
 * of that dependency, whose root module is its src/lib.hd
 * (spec/lang/10-modules.md#use-roots).
 */
function useTarget(
  module: PackageModule,
  declaration: UseDecl,
  dependencies: PackageDependencies | undefined,
): UseTarget | TestOnlyUse | string | undefined {
  const [root, name, ...rest] = declaration.module.split(".");
  if (root !== "dep") {
    const path = useModulePath(module, declaration);
    return Array.isArray(path)
      ? { dependency: module.dependency, path, namespaceOnly: false }
      : path;
  }
  const found = dependencyNamed(dependencies, module, name ?? declaration.names[0]!.name);
  if (typeof found === "string" || "testOnly" in found) return found;
  return { dependency: found, path: rest, namespaceOnly: name === undefined };
}

/** `useTarget`, reporting the error a use names (`unknown-module` or `test-only-use`). */
function reportedUseTarget(
  module: PackageModule,
  declaration: UseDecl,
  dependencies: PackageDependencies | undefined,
  report: (path: string, code: string, message: string, span?: SourceSpan) => void,
): UseTarget | undefined {
  const used = useTarget(module, declaration, dependencies);
  if (typeof used === "string") report(module.path, "unknown-module", used, declaration.span);
  else if (used !== undefined && "testOnly" in used)
    report(module.path, "test-only-use", used.testOnly, declaration.span);
  return typeof used === "object" && !("testOnly" in used) ? used : undefined;
}

/**
 * The library modules of every dependency package, added to `modules` with
 * their absolute paths under each package's source root; the parse
 * diagnostics name those paths too. Returns each module's source by path.
 */
function addDependencyModules(
  dependencies: PackageDependencies | undefined,
  modules: Map<string, PackageModule>,
  diagnostics: PackageDiagnostic[],
  report: (path: string, code: string, message: string, span?: SourceSpan) => void,
): Record<string, string> {
  const sources: Record<string, string> = {};
  for (const dependency of Object.values(dependencies?.packages ?? {})) {
    const own = parseDependencyModules(dependency, diagnostics, report);
    // Implementation modules are checked per package (09-traits.md#r-trait.own.module).
    reportNonlocalImplementations(own, report);
    for (const module of own) {
      modules.set(keyOf(module), module);
      sources[module.path] = dependency.files[module.path.slice(dependency.sourceRoot.length + 1)]!;
    }
  }
  return sources;
}

function parseDependencyModules(
  dependency: DependencyPackage,
  diagnostics: PackageDiagnostic[],
  report: (path: string, code: string, message: string, span?: SourceSpan) => void,
): PackageModule[] {
  // The modules take package paths under `src/` for parsing.
  const absolute = (path: string): string =>
    `${dependency.sourceRoot}/${path.slice(SOURCE_ROOT.length)}`;
  const files = Object.fromEntries(
    Object.entries(dependency.files).map(([path, text]) => [`${SOURCE_ROOT}${path}`, text]),
  );
  const own: PackageDiagnostic[] = [];
  const parsed = parsePackageModules(files, own, (path, ...rest) =>
    report(absolute(path), ...rest),
  );
  for (const diagnostic of own)
    diagnostics.push({ ...diagnostic, path: absolute(diagnostic.path) });
  return [...parsed.values()].map((module) => ({
    ...module,
    path: absolute(module.path),
    dependency,
  }));
}

function fold(identity: string): string {
  return identity.toUpperCase().toLowerCase().normalize("NFC");
}

// The unknown-module message for a use of an integration test program from
// another module, or undefined when the use is allowed
// (spec/lang/10-modules.md#r-module.test.integration.program-use).
function programUseMessage(
  module: PackageModule,
  target: PackageModule,
  identity: string,
): string | undefined {
  if (!isRootProgram(target.path) || module.path === target.path) return undefined;
  // A use of a task from another module is an error (cli.task.program-use).
  if (isTaskPath(target.path))
    return `'${target.path}' is a task, which is its own program and cannot be used from another module`;
  return `'${identity}' is an integration test program, which is its own program and cannot be used from another module`;
}

// The unknown-module message for a use of an executable's entry module, its
// own program, or undefined when the use is allowed
// (spec/lang/10-modules.md#r-module.path.main-no-use,
// spec/cli/command-line.md#r-cli.exe.entry-no-use).
function entryUseMessage(
  module: PackageModule,
  target: PackageModule,
  programs: readonly string[],
): string | undefined {
  if (!programs.includes(target.path) || module.path === target.path) return undefined;
  return `'${target.path}' is an executable's entry module, which is its own program and no module can use`;
}

function fileStart(): SourceSpan {
  const position: SourcePosition = { offset: 0, line: 1, column: 1 };
  return { start: position, end: position };
}

function topLevelNames(program: Program): Map<string, SourceSpan> {
  const names = new Map<string, SourceSpan>();
  for (const declaration of [
    ...program.functions,
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...(program.types ?? []),
  ])
    if (!names.has(declaration.name)) names.set(declaration.name, declaration.span);
  for (const statement of program.statements)
    if (statement.kind === "binding" && !names.has(statement.name))
      names.set(statement.name, statement.span);
  return names;
}

/** The span of a program's first top-level declaration, if it has one. */
function firstDeclarationSpan(program: Program): SourceSpan | undefined {
  return [...topLevelNames(program).values()].sort(
    (left, right) => left.start.offset - right.start.offset,
  )[0];
}

function isPublic(program: Program, name: string): boolean {
  return [
    ...program.functions,
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...(program.types ?? []),
  ].some((declaration) => declaration.name === name && declaration.public === true);
}

/** The source module a relative use starts from (10-modules.md#relative-uses). */
function relativeBase(module: PackageModule): string[] {
  if (module.path === MAIN_FILE || module.path === LIB_FILE) return [];
  // Each file directly under the test root or `tasks` is an independent
  // program root, whose lookup starts at that root (cli.task.root-file).
  if (isRootProgram(module.path)) return [isTaskPath(module.path) ? TASKS_IDENTITY : "tests"];
  return module.identity === "" ? [] : module.identity.split(".");
}

/**
 * The module path a package use names, as identity parts; a message when it
 * moves above its root; undefined for a `std` or `dep` use.
 */
function useModulePath(module: PackageModule, declaration: UseDecl): string[] | string | undefined {
  const [root, ...rest] = declaration.module.split(".");
  if (root === "pkg") return rest;
  // There is no `tests` use root (spec/lang/10-modules.md#r-module.test.no-tests-root).
  if (root === "tests")
    return "there is no 'tests' use root; reach the test root through 'self' or 'super'";
  if (root !== "self" && root !== "super") return undefined;
  const base = relativeBase(module);
  const path = [root, ...rest];
  if (path[0] === "self") path.shift();
  // Relative uses in an integration test module stay under the test root,
  // and in a task module under `tasks` (cli.task.super, cli.task.above-root).
  const top = isIntegrationTestPath(module.path) || isTaskPath(module.path) ? 1 : 0;
  while (path[0] === "super") {
    if (base.length === top)
      return top === 0
        ? "'super' moves above the package root"
        : `'super' moves above the ${isTaskPath(module.path) ? "tasks" : "test"} root`;
    base.pop();
    path.shift();
  }
  return [...base, ...path];
}

interface ResolvedUse {
  readonly declaration: UseDecl;
  readonly target: PackageModule;
  /** The declarations a use selects, each with its local name: `as` renames it. */
  readonly names: readonly ImportedName[];
  /** A module namespace use, as `use pkg.words`, binds the module itself to this name. */
  readonly namespace?: string;
}

interface ImportedName {
  readonly name: string;
  readonly local: string;
}

/** A package declaration: its module and its name there. */
interface Declared {
  readonly module: PackageModule;
  readonly name: string;
}

// Implementation Modules (09-traits.md#r-trait.own.module): a trait
// implementation must be declared in the module that declares the trait,
// the target's outer constructor, or an outer constructor of a trait
// argument; any other module of the owning package is `nonlocal-impl`. The
// checker only sees the joined program, so the linker reports it from the
// per-module programs, where each declaration's module is still known.
// Names the package owns but no package module declares (standard or
// primitive names) resolve to no home and are left to the checker's
// package-ownership codes.
function reportNonlocalImplementations(
  modules: Iterable<PackageModule>,
  report: (path: string, code: string, message: string, span?: SourceSpan) => void,
): void {
  const listed = [...modules];
  const traitHomes = new Map<string, PackageModule>();
  const typeHomes = new Map<string, PackageModule>();
  for (const module of listed) {
    const program = module.program;
    if (!program) continue;
    for (const declaration of program.traits)
      if (!traitHomes.has(declaration.name)) traitHomes.set(declaration.name, module);
    for (const declaration of [...program.data, ...program.enums, ...(program.types ?? [])])
      if (!typeHomes.has(declaration.name)) typeHomes.set(declaration.name, module);
  }
  const typeHome = (type: string): PackageModule | undefined =>
    typeHomes.get(nominalGenericParts(type)?.name ?? type);
  for (const module of listed) {
    for (const implementation of module.program?.implementations ?? []) {
      if (implementation.traitName === undefined || implementation.standard) continue;
      const trait = nominalGenericParts(implementation.traitName);
      const homes = new Set<PackageModule>();
      const traitHome = traitHomes.get(trait?.name ?? implementation.traitName);
      if (traitHome) homes.add(traitHome);
      const targetHome = typeHome(implementation.targetName);
      if (targetHome) homes.add(targetHome);
      for (const argument of trait?.arguments ?? []) {
        const argumentHome = typeHome(argument);
        if (argumentHome) homes.add(argumentHome);
      }
      if (homes.size === 0 || homes.has(module)) continue;
      const legal = [...homes].map((home) => `'${home.path}'`).join(", ");
      report(
        module.path,
        "nonlocal-impl",
        `implementation of trait '${trait?.name ?? implementation.traitName}' for '${implementation.targetName}' must be declared in a module that declares the trait, the target, or a trait argument (${legal}), not in '${module.path}'`,
        implementation.span,
      );
    }
  }
}

function parsePackageModules(
  files: Readonly<Record<string, string>>,
  diagnostics: PackageDiagnostic[],
  report: (path: string, code: string, message: string, span?: SourceSpan) => void,
): Map<string, PackageModule> {
  const modules = new Map<string, PackageModule>();
  const folded = new Map<string, string>();
  for (const path of Object.keys(files).sort()) {
    const identity = moduleIdentity(path);
    if (path === `${SOURCE_ROOT}mod.hd`) {
      report(
        path,
        "reserved-module-name",
        "'src/mod.hd' is not a module, since the source root is no directory module; rename it 'src/lib.hd', the package root module",
      );
      continue;
    }
    if (identity === undefined) {
      report(
        path,
        "invalid-module-path",
        `'${path}' is not a module path: files are 'src/<identifier>/.../<identifier>.hd', or under 'tests/' for integration tests and 'tasks/' for tasks`,
      );
      continue;
    }
    // A program file directly under the test root or `tasks`, beside a
    // directory of the same name, is invalid
    // (spec/lang/10-modules.md#r-module.test.integration.beside-dir,
    // spec/cli/command-line.md#r-cli.task.beside-dir).
    const directory = path.slice(0, -".hd".length);
    if (
      isRootProgram(path) &&
      Object.keys(files).some((other) => other.startsWith(`${directory}/`))
    ) {
      report(
        path,
        "duplicate-module-name",
        `'${path}' lies beside the directory '${directory}/', so it is no program; move it to '${directory}/mod.hd'`,
      );
      continue;
    }
    const clash = folded.get(fold(identity));
    if (clash !== undefined) {
      report(path, "duplicate-module-name", `'${path}' and '${clash}' name the same module`);
      continue;
    }
    folded.set(fold(identity), path);
    const parsed = parse(files[path]!, { testModule: isTestModulePath(path) });
    for (const diagnostic of parsed.diagnostics) diagnostics.push({ ...diagnostic, path });
    // `pkg` names the root module, so no module directly under the source
    // root takes its name (spec/lang/10-modules.md#r-module.path.reserved-pkg).
    if (identity === "pkg" && path.startsWith(SOURCE_ROOT)) {
      report(
        path,
        "reserved-module-name",
        `'${path}' cannot be a module: 'pkg' names the package root module, src/lib.hd; rename the file`,
        parsed.program && firstDeclarationSpan(parsed.program),
      );
      continue;
    }
    modules.set(identity, {
      path,
      identity,
      ...(parsed.program ? { program: parsed.program } : {}),
    });
  }
  return modules;
}

/**
 * Maps a diagnostic on a linked source back to its package file
 * (`LinkedPackage.locate`), with its fix-it's edits when they lie in that
 * file: `sourceOf` gives a file's text, which places each moved edit.
 */
function segmentLocator(
  segments: readonly LinkSegment[],
  entry: string,
  sourceOf: (path: string) => string | undefined,
): (diagnostic: Diagnostic) => PackageDiagnostic {
  return (diagnostic) => {
    // REPL transports source-qualified diagnostics as text, so callers that
    // parse that transport can retain its explicit non-package file here.
    const explicitFile = (diagnostic as Diagnostic & { readonly file?: unknown }).file;
    if (typeof explicitFile === "string") return { ...diagnostic, path: explicitFile };
    const document = sourceDocument(diagnostic.span);
    if (document) return { ...physicalDiagnostic(diagnostic), path: document.file };
    const segmentOf = (position: SourcePosition): LinkSegment | undefined =>
      segments.findLast(({ firstLine }) => firstLine <= position.line) ?? segments[0];
    const segment = segmentOf(diagnostic.span.start);
    if (!segment) return { ...diagnostic, path: entry };
    const move = (position: SourcePosition): SourcePosition => {
      const line = Math.min(Math.max(1, position.line - segment.firstLine + 1), segment.lineCount);
      const deleted = segment.deleted.get(line - 1) ?? 0;
      return {
        ...position,
        line,
        column: Math.max(1, position.column - segment.indent + deleted),
      };
    };
    const { fix, ...rest } = diagnostic;
    const located: PackageDiagnostic = {
      ...rest,
      path: segment.path,
      span: { start: move(diagnostic.span.start), end: move(diagnostic.span.end) },
    };
    // A fix-it's edits move with it, and it stays only when every edit is in
    // the diagnostic's own file, whose text gives each moved edit its offset.
    const text = sourceOf(segment.path);
    if (!fix || text === undefined) return located;
    if (
      fix.edits.some(
        ({ span }) => segmentOf(span.start) !== segment || segmentOf(span.end) !== segment,
      )
    )
      return located;
    const lineOffsets = [0];
    for (let index = text.indexOf("\n"); index !== -1; index = text.indexOf("\n", index + 1))
      lineOffsets.push(index + 1);
    const placed = (position: SourcePosition): SourcePosition => {
      const moved = move(position);
      return { ...moved, offset: (lineOffsets[moved.line - 1] ?? text.length) + moved.column - 1 };
    };
    const edits = fix.edits.map(({ span, replacement }) => ({
      span: { start: placed(span.start), end: placed(span.end) },
      replacement,
    }));
    return { ...located, fix: { message: fix.message, edits } };
  };
}

/**
 * Follows `pub use` re-exports to the declaration that `name` names in
 * `target`; "loop" when the chain returns to a module it passed
 * (spec/lang/10-modules.md#r-module.pub-use.chain.loop).
 */
function exporterOf(
  resolvedUses: ReadonlyMap<PackageModule, readonly ResolvedUse[]>,
  target: PackageModule,
  name: string,
  seen: Set<PackageModule>,
): Declared | "private" | "loop" | undefined {
  const program = target.program!;
  if (topLevelNames(program).has(name))
    return isPublic(program, name) ? { module: target, name } : "private";
  if (seen.has(target)) return "loop";
  seen.add(target);
  for (const use of resolvedUses.get(target) ?? []) {
    const exported = use.declaration.public
      ? use.names.find(({ local }) => local === name)
      : undefined;
    if (exported) return exporterOf(resolvedUses, use.target, exported.name, seen);
  }
  return undefined;
}

/** Links the package `files` (path to source) whose entry module is `entry`. */
export function linkPackage(
  files: Readonly<Record<string, string>>,
  entry: string,
  options: LinkOptions = {},
): LinkedPackage {
  const diagnostics: PackageDiagnostic[] = [];
  const report = (path: string, code: string, message: string, span = fileStart()): void => {
    diagnostics.push({ path, code, message, span });
  };
  const modules = parsePackageModules(files, diagnostics, report);
  reportNonlocalImplementations(modules.values(), report);
  // Each module's source, by its path: a dependency's under its source root.
  const dependencySources = addDependencyModules(
    options.dependencies,
    modules,
    diagnostics,
    report,
  );
  const sources: Record<string, string> = { ...files, ...dependencySources };
  const byPath = new Map([...modules.values()].map((module) => [module.path, module]));
  const entryModule = byPath.get(entry);
  if (!entryModule && !diagnostics.some((diagnostic) => diagnostic.path === entry))
    report(entry, "unknown-module", `entry module '${entry}' is not a package source file`);

  const resolvedUses = new Map<PackageModule, ResolvedUse[]>();

  for (const module of modules.values()) {
    const uses: ResolvedUse[] = [];
    resolvedUses.set(module, uses);
    for (const declaration of module.program?.uses ?? []) {
      const root = declaration.module.split(".")[0];
      if (root === "std") continue;
      const span = declaration.span;
      const [first] = declaration.names;
      const used = reportedUseTarget(module, declaration, options.dependencies, report);
      if (used === undefined) continue;
      const { dependency: targetPackage, path: modulePath, namespaceOnly } = used;
      // A single use whose last segment is a module names that module's
      // namespace, as `use pkg.words` (spec/lang/10-modules.md#r-module.use.single).
      const grouped = sources[module.path]!.slice(span.start.offset, span.end.offset).includes("{");
      const namespacePath = [...modulePath, first!.name];
      const namespace =
        namespaceOnly ||
        (!grouped && modules.has(moduleKey(targetPackage, namespacePath.join("."))));
      const path = namespace && !namespaceOnly ? namespacePath : modulePath;
      const identity = path.join(".");
      const target = modules.get(moduleKey(targetPackage, identity));
      if (!target) {
        diagnostics.push(missingUseDiagnostic(module, declaration, targetPackage, identity, span));
        continue;
      }
      const programUse =
        programUseMessage(module, target, identity) ??
        entryUseMessage(module, target, options.programs ?? [MAIN_FILE]);
      if (programUse !== undefined) {
        report(module.path, "unknown-module", programUse, span);
        continue;
      }
      // In an integration test module, `pkg` names only the library modules
      // (spec/lang/10-modules.md#r-module.test.integration.pkg-root).
      if (root === "pkg" && isIntegrationTestPath(module.path) && isTestModulePath(target.path)) {
        report(
          module.path,
          "unknown-module",
          `'pkg' names only library modules in an integration test module, not the test module '${identity}'`,
          span,
        );
        continue;
      }
      if (!target.program) continue;
      // A single use that names both a module and a declaration of its
      // parent, as `use pkg.words`, is ambiguous
      // (spec/lang/10-modules.md#r-module.use.ambiguous).
      if (!grouped && namespace && !namespaceOnly) {
        const parent = modules.get(moduleKey(targetPackage, modulePath.join(".")));
        const clash =
          parent?.program === undefined
            ? undefined
            : exporterOf(resolvedUses, parent, first!.name, new Set());
        if (clash !== undefined && clash !== "loop") {
          report(
            module.path,
            "ambiguous-import",
            `'${first!.name}' names both the module '${identity}' and a declaration of module '${shown(parent!)}'; rename one`,
            span,
          );
          continue;
        }
      }
      if (namespace) {
        uses.push({ declaration, target, names: [], namespace: first!.alias ?? first!.name });
        continue;
      }
      const names = declaration.names.map(({ name, alias }) => ({ name, local: alias ?? name }));
      uses.push({ declaration, target, names });
    }
  }

  // Imported names must be public declarations of the target (or re-exported).
  const edges = new Map<PackageModule, Set<PackageModule>>();
  // What each imported local name names, by module.
  const importedNames = new Map<PackageModule, Map<string, Declared>>();
  // Uses outside test code, for the folder graph (spec/lang/10-modules.md#r-module.cycle.test-code).
  const folderUses: { module: PackageModule; use: ResolvedUse }[] = [];
  for (const [module, uses] of resolvedUses) {
    const local = module.program ? topLevelNames(module.program) : new Map();
    const imported = new Set<string>();
    const resolved = new Map<string, Declared>();
    importedNames.set(module, resolved);
    const targets = new Set<PackageModule>();
    edges.set(module, targets);
    const testNames = new Set(module.program?.testOnlyNames ?? []);
    for (const use of uses) {
      targets.add(use.target);
      const locals = use.namespace !== undefined ? [use.namespace] : use.names.map((n) => n.local);
      // Only test code may use a test module (spec/lang/10-modules.md#r-module.test.non-test-use).
      const testCode = isTestModulePath(module.path) || locals.every((name) => testNames.has(name));
      if (!testCode && use.target.dependency === module.dependency)
        folderUses.push({ module, use });
      if (isTestModulePath(use.target.path) && !testCode)
        report(
          module.path,
          "test-only-use",
          `only test code may use the test module '${shown(use.target)}'`,
          use.declaration.span,
        );
      if (use.namespace !== undefined) {
        if (local.has(use.namespace) || imported.has(use.namespace))
          report(
            module.path,
            "duplicate-module-name",
            `imported name '${use.namespace}' is declared more than once`,
            use.declaration.span,
          );
        imported.add(use.namespace);
      }
      for (const { name, local: localName } of use.names) {
        const found = exporterOf(resolvedUses, use.target, name, new Set());
        // A plain use into a pub use loop has the loop's code
        // (spec/lang/10-modules.md#r-module.pub-use.chain.loop-use).
        if (found === "loop")
          report(
            module.path,
            "re-export-loop",
            use.declaration.public
              ? `'pub use' of '${name}' leads back to itself through module '${shown(use.target)}'; a pub use chain must end at a declaration`
              : `'use' of '${name}' leads into a pub use loop through module '${shown(use.target)}'; a pub use chain must end at a declaration`,
            use.declaration.span,
          );
        else if (found === undefined)
          report(
            module.path,
            "unknown-import",
            `module '${shown(use.target)}' declares no '${name}'`,
            use.declaration.span,
          );
        else if (found === "private")
          report(
            module.path,
            "private-import",
            `'${name}' is private to module '${shown(use.target)}'; mark it 'pub'`,
            use.declaration.span,
          );
        else {
          resolved.set(localName, found);
          if (found.module !== use.target) targets.add(found.module);
        }
        if (local.has(localName) || imported.has(localName))
          report(
            module.path,
            "duplicate-module-name",
            `imported name '${localName}' is declared more than once`,
            use.declaration.span,
          );
        imported.add(localName);
      }
    }
  }

  // The folder graph must be acyclic (spec/lang/10-modules.md#r-module.cycle.acyclic).
  // Files of one folder may use each other in a loop.
  const folderOf = folders(modules.values());
  for (const loop of folderLoops(folderUses, folderOf)) {
    // Moving a file to `x/mod.hd` makes it a folder of its own, unless it
    // already is the folder of its directory or of its child modules.
    const fix = loop.find(
      ({ use }) =>
        folderOf(use.target) === directoryOf(use.target.path) &&
        !use.target.path.endsWith("/mod.hd") &&
        use.target.path !== LIB_FILE,
    );
    const at = fix ?? loop[0]!;
    const steps = loop.map(({ from, to, module, use }) => {
      const { line } = use.declaration.span.start;
      const text = sources[module.path]!.split("\n")[line - 1]!.trim();
      return `  ${from}/ -> ${to}/: ${module.path}:${line}: ${text}`;
    });
    const help = fix
      ? `move ${fix.use.target.path} to ${fix.use.target.path.replace(/\.hd$/, "/mod.hd")}; ` +
        `its module name '${shown(fix.use.target)}' and every use line stay the same`
      : "move the shared declarations into a leaf folder that uses none of these folders";
    report(
      at.module.path,
      "folder-cycle",
      [
        `folders depend on each other in a loop (the tangle has ${loop.tangle} folders):`,
        ...steps,
        `  help: ${help}`,
      ].join("\n"),
      at.use.declaration.span,
    );
  }

  // Collect the modules reachable from the entry.
  const reachable = new Set<PackageModule>();
  const visit = (module: PackageModule): void => {
    if (reachable.has(module)) return;
    reachable.add(module);
    for (const target of edges.get(module) ?? []) visit(target);
  };
  if (entryModule) visit(entryModule);
  // A test build links every unit test module. Each integration test program
  // links separately through its own uses, never in bulk.
  if (options.tests)
    for (const module of modules.values())
      if (isTestModulePath(module.path) && !isIntegrationTestPath(module.path)) visit(module);

  const { order, groups } = initializationOrder(reachable, edges);

  const { joinedName, standardName } = joinedNames(order, entryModule);
  const foreignOf = foreignNamesOf(order, importedNames, resolvedUses);

  const segments: LinkSegment[] = [];
  const locate = segmentLocator(segments, entry, (path) => files[path] ?? dependencySources[path]);
  if (diagnostics.some(({ severity }) => severity !== "warning") || !entryModule)
    return { modules: order, diagnostics, locate, initGroups: [], dependencySources };

  // Join the modules. Package uses are dropped and a standard use keeps only
  // the names no earlier module imported under the same spelling; every
  // other line stays in place.
  const importedStd = new Set<string>();
  let source = "";
  let line = 1;
  // An initialization-group start opens each group's statement block, so the
  // checker can tell a group of one module (source order,
  // 10-modules.md#r-module.init.source-order-single) from a larger group
  // (dependency order, 10-modules.md#order-inside-a-group). The starts travel
  // beside the source as line numbers, never in it, so user comments cannot
  // collide with them and diagnostic mapping needs no hole for marker lines.
  const initGroups: InitGroupStart[] = [];
  // Each module's scope, and the members of each module a namespace use
  // names (spec/lang/05-expressions.md#r-expr.name.qualified).
  const scopes: ModuleScope[] = [];
  const namespaceModules: Record<string, NamespaceModule> = {};
  const moduleScope = (module: PackageModule, firstLine: number, lastLine: number): ModuleScope => {
    const names: Record<string, string> = {};
    const [namespaces, imports]: [Record<string, string>, string[]] = [{}, []];
    for (const name of module.program ? topLevelNames(module.program).keys() : []) {
      const joined = joinedName({ module, name });
      if (joined !== name) names[name] = joined;
    }
    for (const [local, declared] of importedNames.get(module) ?? []) {
      const joined = joinedName(declared);
      imports.push(joined); // every joined name the module imports
      if (joined !== local) names[local] = joined;
    }
    // A std name another module binds differently joins under a hidden spelling.
    for (const { names: used } of (module.program?.uses ?? []).filter(isStandardUse))
      for (const local of used.map(({ name, alias }) => alias ?? name))
        if (standardName(module, local) !== local) names[local] = standardName(module, local);
    for (const use of resolvedUses.get(module) ?? []) {
      if (use.namespace === undefined) continue;
      namespaces[use.namespace] = keyOf(use.target);
      namespaceModules[keyOf(use.target)] ??= namespaceMembers(
        use.target,
        modules,
        resolvedUses,
        joinedName,
      );
    }
    const owner =
      module === entryModule && options.entryPackage !== undefined
        ? { package: options.entryPackage }
        : {};
    return {
      firstLine,
      lastLine,
      names,
      namespaces,
      ...ownershipFields(module, imports),
      ...foreignOf(module),
      ...owner,
    };
  };
  const multiBefore = new Map<PackageModule, boolean>();
  for (const group of groups) multiBefore.set(group[0]!, group.length > 1);
  for (const module of order) {
    const multi = multiBefore.get(module);
    // The start line precedes the group's text, matching the old marker
    // line: the parser opens the group at the first statement after it.
    if (multi !== undefined) initGroups.push({ line: line - 1, multi });
    const spelling = (local: string): string => standardName(module, local);
    let text = joinedText(module.program!, sources[module.path]!, importedStd, spelling);
    if (!text.endsWith("\n")) text += "\n";
    const lineCount = text.split("\n").length - 1;
    // A unit test module joins as a `tests:` block; its top-level `pub` is
    // deleted, since a `tests:` item cannot be `pub`. An integration test
    // module joins as ordinary top-level source from its own AST: never
    // re-indented, no `pub` stripped, so multiline strings survive and
    // top-level `it` calls parse as test cases via `joinedModules`.
    const deleted = new Map<number, number>();
    const wrapAsTestsBlock = isTestModulePath(module.path) && !isIntegrationTestPath(module.path);
    if (wrapAsTestsBlock) {
      text = wrapTestModule(text, deleted);
      line += 1;
    }
    const indent = wrapAsTestsBlock ? 4 : 0;
    segments.push({ path: module.path, firstLine: line, lineCount, indent, deleted });
    scopes.push(moduleScope(module, line, line + lineCount - 1));
    source += text;
    line += lineCount;
  }
  return {
    source,
    modules: order,
    diagnostics,
    locate,
    initGroups,
    packageScopes: { scopes, modules: namespaceModules },
    dependencySources,
    entryLine: segments.find(({ path }) => path === entry)?.firstLine,
    // A test build runs no entry behavior, so an entry module with a
    // `tests:` block initializes its top level requirement-free; one
    // without test code is still a script there
    // (spec/lang/10-modules.md#r-module.init.tests.requirement-free).
    ...(isScript(entryModule.program) &&
    !isTestModulePath(entryModule.path) &&
    !(options.tests && (entryModule.program?.tests.length ?? 0) > 0)
      ? { scriptEntry: true as const }
      : {}),
  };
}

/**
 * For each linked module, the names of the other linked modules (`order`)
 * that it must not name (03-names-and-scopes.md#r-names.module.other-module):
 * their top-level names, and the names their std uses bind
 * (r-names.module.use-own-module), that it neither declares nor binds
 * through a use. A std use still joins once for every module; only its
 * names stay in their module. A prelude name is in every module's scope.
 */
function foreignNamesOf(
  order: readonly PackageModule[],
  importedNames: ReadonlyMap<PackageModule, ReadonlyMap<string, Declared>>,
  resolvedUses: ReadonlyMap<PackageModule, readonly ResolvedUse[]>,
): (module: PackageModule) => Pick<ModuleScope, "foreign"> {
  const declarers = new Map<string, PackageModule[]>();
  const standard = new Map<string, ForeignName>();
  for (const module of order) {
    for (const name of module.program ? topLevelNames(module.program).keys() : [])
      declarers.set(name, [...(declarers.get(name) ?? []), module]);
    for (const use of module.program?.uses ?? [])
      if (isStandardUse(use))
        for (const { name, alias } of use.names)
          if (!PRELUDE_NAMES.has(alias ?? name) && !standard.has(alias ?? name))
            standard.set(alias ?? name, standardForeign(use.module, name, alias));
  }
  return (module) => {
    const program = module.program;
    if (!program) return {};
    const visible = new Set([
      ...topLevelNames(program).keys(),
      ...(importedNames.get(module)?.keys() ?? []),
      ...(resolvedUses.get(module) ?? []).flatMap(({ namespace }) => namespace ?? []),
    ]);
    for (const use of program.uses)
      if (isStandardUse(use)) for (const { name, alias } of use.names) visible.add(alias ?? name);
    const foreign: Record<string, ForeignName> = {};
    for (const [name, modules] of declarers) {
      if (visible.has(name)) continue;
      const others = modules.filter((other) => other !== module);
      const target =
        others.find((other) => importPath(module, other) && isPublic(other.program!, name)) ??
        others[0];
      if (!target) continue;
      const trait = target.program!.traits.some((declaration) => declaration.name === name);
      foreign[name] = { ...(trait ? { trait } : {}), hint: foreignHint(module, target, name) };
    }
    for (const [name, found] of standard) if (!visible.has(name)) foreign[name] ??= found;
    return Object.keys(foreign).length > 0 ? { foreign } : {};
  };
}

/** The path a use in `from` names `target` by, as `pkg.cart`; undefined when no use can. */
function importPath(from: PackageModule, target: PackageModule): string | undefined {
  // A program root is no module's import (module.path.main-no-use,
  // module.test.integration.program-use), and only test code may use a
  // test module (module.test.non-test-use.test-module).
  if (target.path === MAIN_FILE || isRootProgram(target.path)) return undefined;
  if (isTestModulePath(target.path) && !isTestModulePath(from.path)) return undefined;
  if (from.dependency === target.dependency)
    return target.identity === "" ? "pkg" : `pkg.${target.identity}`;
  return from.dependency ? undefined : shown(target);
}

/** What a diagnostic says about `name`, which `target` declares and `from` names bare. */
function foreignHint(from: PackageModule, target: PackageModule, name: string): string {
  const where = `module '${shown(target)}'`;
  const program = target.program!;
  const path = importPath(from, target);
  if (path === undefined) return `${where} declares it, and this module can't use that module`;
  // A top-level binding can't be `pub` (module.package.no-pub-binding).
  if (program.statements.some((item) => item.kind === "binding" && item.name === name))
    return `it is a top-level binding of ${where}, private to that module`;
  const use = `\`use ${path}.{${name}}\``;
  if (isPublic(program, name)) return `${where} declares it; import it with ${use}`;
  if (from.dependency !== target.dependency) return `it is private to ${where}`;
  return `it is private to ${where}; mark it 'pub' and import it with ${use}`;
}

/** What a module path may select in `target`, the module a namespace use names. */
function namespaceMembers(
  target: PackageModule,
  modules: ReadonlyMap<string, PackageModule>,
  resolvedUses: ReadonlyMap<PackageModule, readonly ResolvedUse[]>,
  joinedName: (declared: Declared) => string,
): NamespaceModule {
  const members: Record<string, string | null> = {};
  for (const name of topLevelNames(target.program!).keys())
    members[name] = isPublic(target.program!, name) ? joinedName({ module: target, name }) : null;
  for (const use of resolvedUses.get(target) ?? [])
    if (use.declaration.public)
      for (const { local } of use.names) {
        const found = exporterOf(resolvedUses, target, local, new Set());
        if (typeof found === "object") members[local] = joinedName(found);
      }
  // A path cannot reach a child module through its parent
  // (10-modules.md#r-module.path.no-child-import): each direct child with
  // the `use` path that imports it, for the diagnostic.
  const children: Record<string, string> = {};
  const prefix = target.identity === "" ? "" : `${target.identity}.`;
  for (const candidate of modules.values()) {
    if (candidate.dependency !== target.dependency) continue;
    if (!candidate.identity.startsWith(prefix)) continue;
    const rest = candidate.identity.slice(prefix.length);
    if (rest === "" || rest.includes(".")) continue;
    children[rest] = candidate.dependency ? shown(candidate) : `pkg.${candidate.identity}`;
  }
  return { shown: shown(target), members, children };
}

/** A script: top-level statements and no `main` (spec/lang/10-modules.md#r-module.init.script). */
function isScript(program: Program | undefined): boolean {
  return (
    program !== undefined &&
    program.statements.length > 0 &&
    !program.functions.some(({ name }) => name === "main")
  );
}

/**
 * What a module's scope says about packages: the dependency package it
 * belongs to and the declarations its uses import, which the checker reads
 * (checker/package-ownership.ts).
 */
function ownershipFields(
  module: PackageModule,
  imports: readonly string[],
): Pick<ModuleScope, "package" | "imports" | "fetched"> {
  return {
    ...(module.dependency ? { package: module.dependency.id } : {}),
    // A fetched package's `dbg` calls print nothing
    // (spec/lang/10-modules.md#r-module.dbg.dependency).
    ...(module.dependency?.fetched ? { fetched: module.dependency.fetched } : {}),
    ...(imports.length > 0 ? { imports } : {}),
  };
}

/**
 * The hidden spelling of a package declaration that shares its name with
 * another linked module's, as `__pkg_user_types_origin` for `origin` in
 * `user.types`. Like the std loader's `__std_` names, no source spells it.
 */
function hiddenPackageName(identity: string, name: string): string {
  const path = identity.replace(/[^\p{ID_Continue}]+/gu, "_").replace(/^_+/, "");
  return `__pkg_${path === "" ? "" : `${path}_`}${name}`;
}

/**
 * How a linked package's joined source parses: as joined modules, with the
 * linker's initialization-group starts and module scopes beside it.
 */
export function linkedParseOptions(linked: LinkedPackage): ParseOptions {
  return {
    joinedModules: true,
    initGroupStarts: linked.initGroups,
    ...(linked.scriptEntry ? { scriptEntry: true } : {}),
    ...(linked.packageScopes ? { packageScopes: linked.packageScopes } : {}),
  };
}

// Initialization order (spec/lang/10-modules.md#initialization-order): modules
// that use each other form one group; a group is ready once every group it
// uses is done, and ready groups go by their least identity. Test modules
// go after the others, so a standard use that a test module shares with
// library code stays outside the test module's `tests:` block. Inside a
// group the prototype joins modules by identity; it does not interleave
// their statements by dependency (src/README.md, Implemented Surface).
function initializationOrder(
  reachable: ReadonlySet<PackageModule>,
  edges: ReadonlyMap<PackageModule, ReadonlySet<PackageModule>>,
): { readonly order: readonly PackageModule[]; readonly groups: readonly PackageModule[][] } {
  const groups = stronglyConnected([...reachable], (module) =>
    [...(edges.get(module) ?? [])].filter((target) => reachable.has(target)),
  ).map((group) => group.sort(byIdentity));
  const groupOf = new Map<PackageModule, PackageModule[]>();
  for (const group of groups) for (const module of group) groupOf.set(module, group);
  const libraryGroup = (group: PackageModule[]): boolean =>
    group.some(({ path }) => !isTestModulePath(path));
  const order: PackageModule[] = [];
  const emitted: PackageModule[][] = [];
  const pending = new Set(groups);
  while (pending.size > 0) {
    const ready = [...pending]
      .filter((group) =>
        group.every((module) =>
          [...(edges.get(module) ?? [])].every((target) => {
            const other = groupOf.get(target);
            return other === undefined || other === group || !pending.has(other);
          }),
        ),
      )
      .sort((left, right) => byIdentity(left[0]!, right[0]!));
    const library = [...pending].some(libraryGroup);
    const next = (library ? ready.find(libraryGroup) : undefined) ?? ready[0] ?? [...pending][0]!;
    order.push(...next);
    emitted.push(next);
    pending.delete(next);
  }
  return { order, groups: emitted };
}

function byIdentity(left: PackageModule, right: PackageModule): number {
  return left.identity < right.identity ? -1 : left.identity > right.identity ? 1 : 0;
}

// The joined program has one namespace for every linked module. Each
// module keeps its own names through its scope (`ModuleScope`): a
// declaration whose name another linked module also declares, or imports
// from std, joins under a hidden spelling, except in the entry module.
// Standard uses join once for every module; a module that binds a local
// name to another std declaration than an earlier module does binds it
// under a hidden spelling (`standardName`, package-join.ts).
function joinedNames(
  order: readonly PackageModule[],
  entryModule: PackageModule | undefined,
): {
  readonly joinedName: (declared: Declared) => string;
  readonly standardName: (module: PackageModule, local: string) => string;
} {
  const standardBinders = new Map<string, PackageModule[]>();
  const declarers = new Map<string, PackageModule[]>();
  for (const module of order) {
    const program = module.program;
    if (!program) continue;
    for (const name of topLevelNames(program).keys())
      declarers.set(name, [...(declarers.get(name) ?? []), module]);
    for (const declaration of program.uses)
      if (isStandardUse(declaration))
        for (const { name, alias } of declaration.names)
          standardBinders.set(alias ?? name, [
            ...(standardBinders.get(alias ?? name) ?? []),
            module,
          ]);
  }
  const spellings = new Set(declarers.keys());
  const hiddenNames = new Map<string, string>();
  const hidden = (module: PackageModule, name: string): string => {
    const key = `${module.path}\0${name}`;
    let found = hiddenNames.get(key);
    if (found === undefined) {
      const base = hiddenPackageName(hiddenBase(module), name);
      found = base;
      for (let index = 2; spellings.has(found); index += 1) found = `${base}_${index}`;
      spellings.add(found);
      hiddenNames.set(key, found);
    }
    return found;
  };
  const joinedName = ({ module, name }: Declared): string => {
    // A `main` outside the entry module is an ordinary function
    // (spec/cli/command-line.md#r-cli.exe.unselected-main), so it never
    // joins as the program's `main`.
    if (name === "main" && module !== entryModule) return hidden(module, name);
    const shared =
      (declarers.get(name) ?? []).some((other) => other !== module) ||
      (standardBinders.get(name) ?? []).some((binder) => binder !== module);
    if (!shared || (module === entryModule && !standardBinders.has(name))) return name;
    return hidden(module, name);
  };
  return { joinedName, standardName: standardSpellings(order, hidden) };
}
