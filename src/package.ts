import type { Program, UseDecl } from "./ast.ts";
import type { Diagnostic, SourcePosition, SourceSpan } from "./diagnostics.ts";
import { KEYWORDS } from "./lexer.ts";
import { parse } from "./parser/index.ts";

// Package linking for the prototype (10-modules.md). Every source file of one
// package is a module named by its path under `src/`, and every file under
// `tests/` is an integration test module named `tests.<path>`. The linker resolves the
// `pkg`, `self`, and `super` uses between those modules and then joins the
// modules reachable from the entry module into one source text, in module
// initialization order, with the package uses removed. That text is an
// ordinary single-module program for the rest of the pipeline.
//
// Joining modules puts their top-level names in one namespace, so the linker
// rejects a name that two linked modules declare (`package-name-collision`)
// and does not stop one module from naming another's declaration without a
// `use`. Module namespace uses (`use pkg.user.types`) and renaming uses
// (`as`) of package declarations are `unsupported-package-use`.
//
// A `*_test.hd` file is a test module. It joins as a `tests:` block, so the
// joined source may hold several `tests:` blocks and is parsed with
// `joinedModules`. A test build (`LinkOptions.tests`) links every test module.
// Test case names share the joined namespace too, so two modules must not
// name a test case alike.
//
// An integration test module (spec/10-modules.md#r-module.test.integration)
// is test code too, so it links like a test module. It reaches the library
// through `pkg` uses, which see only public declarations, and other
// integration test modules through the `tests` root or `self`. The joined
// program shares one namespace, so it does not hide a library module's
// private names or test code from an integration test module.

export const SOURCE_ROOT = "src/";
/** The default test root, which holds the integration test modules. */
export const TEST_ROOT = "tests/";

export interface PackageDiagnostic extends Diagnostic {
  /** The package file the diagnostic points into. */
  readonly path: string;
}

interface PackageModule {
  readonly path: string;
  /** The dotted module identity, `""` for `src/mod.hd`. */
  readonly identity: string;
  readonly program?: Program;
}

interface LinkSegment {
  readonly path: string;
  /** First line of the module in the linked source, 1-based. */
  readonly firstLine: number;
  readonly lineCount: number;
  /** Columns the linker indented the module by: 4 for a test module. */
  readonly indent: number;
}

interface LinkOptions {
  /**
   * A test build (spec/10-modules.md#r-module.test.code): every test module
   * is linked, not only those the entry module reaches.
   */
  readonly tests?: boolean;
}

/** Whether a package path is an integration test module (spec/10-modules.md#r-module.test.integration). */
function isIntegrationTestPath(path: string): boolean {
  return path.startsWith(TEST_ROOT);
}

/**
 * Whether a package path is a test module or an integration test module
 * (spec/10-modules.md#r-module.test.module): its top level is test code.
 */
function isTestModulePath(path: string): boolean {
  return path.endsWith("_test.hd") || isIntegrationTestPath(path);
}

export interface LinkedPackage {
  /** The joined single-module source; absent when linking failed. */
  readonly source?: string;
  /** Linked modules in initialization order. */
  readonly modules: readonly PackageModule[];
  readonly diagnostics: readonly PackageDiagnostic[];
  /**
   * The line of `source` where the entry module starts. Outside a test build
   * the entry module is initialized last, so it runs to the end of `source`,
   * and its lines keep their numbers relative to this one.
   */
  readonly entryLine?: number;
  /** Maps a diagnostic on the linked source back to its package file. */
  locate(diagnostic: Diagnostic): PackageDiagnostic;
}

const IDENTIFIER = /^[\p{ID_Start}_][\p{ID_Continue}_]*$/u;

/**
 * The module identity of a package path, or undefined when it names none. A
 * file under `tests/` is the integration test module `tests.<path>`
 * (spec/10-modules.md#r-module.test.integration.tests-root); `tests` is a
 * reserved word, so no library module identity starts with it.
 */
export function moduleIdentity(path: string): string | undefined {
  const root = [SOURCE_ROOT, TEST_ROOT].find((prefix) => path.startsWith(prefix));
  if (root === undefined || !path.endsWith(".hd")) return undefined;
  const parts = path.slice(root.length, -".hd".length).split("/");
  if (parts.at(-1) === "mod") parts.pop();
  const valid = parts.every(
    (part) => IDENTIFIER.test(part) && part.normalize("NFC") === part && !KEYWORDS.has(part),
  );
  if (!valid) return undefined;
  return root === TEST_ROOT ? ["tests", ...parts].join(".") : parts.join(".");
}

function fold(identity: string): string {
  return identity.toUpperCase().toLowerCase().normalize("NFC");
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

function isPublic(program: Program, name: string): boolean {
  return [
    ...program.functions,
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...(program.types ?? []),
  ].some((declaration) => declaration.name === name && declaration.public === true);
}

/** The directory module a relative use starts from (10-modules.md#use-roots). */
function relativeBase(module: PackageModule): string[] {
  const parts = module.identity === "" ? [] : module.identity.split(".");
  return module.path.endsWith("/mod.hd") ? parts : parts.slice(0, -1);
}

/**
 * The module path a package use names, as identity parts; a message when it
 * moves above its root; undefined for a `std` or `dep` use.
 */
function useModulePath(module: PackageModule, declaration: UseDecl): string[] | string | undefined {
  const [root, ...rest] = declaration.module.split(".");
  if (root === "pkg") return rest;
  // The parser accepts the `tests` root only in an integration test module.
  if (root === "tests") return [root, ...rest];
  if (root !== "self" && root !== "super") return undefined;
  const base = relativeBase(module);
  const path = [root, ...rest];
  if (path[0] === "self") path.shift();
  // Relative uses in an integration test module stay under the test root.
  const top = isIntegrationTestPath(module.path) ? 1 : 0;
  while (path[0] === "super") {
    if (base.length === top)
      return top === 0
        ? "'super' moves above the package root"
        : "'super' moves above the test root";
    base.pop();
    path.shift();
  }
  return [...base, ...path];
}

interface ResolvedUse {
  readonly declaration: UseDecl;
  readonly target: PackageModule;
  readonly names: readonly string[];
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
  const modules = new Map<string, PackageModule>();
  const folded = new Map<string, string>();
  for (const path of Object.keys(files).sort()) {
    const identity = moduleIdentity(path);
    if (identity === undefined) {
      report(
        path,
        "invalid-module-path",
        `'${path}' is not a module path: files are 'src/<identifier>/.../<identifier>.hd', or under 'tests/' for integration tests`,
      );
      continue;
    }
    const clash = folded.get(fold(identity));
    if (clash !== undefined) {
      report(path, "duplicate-module-path", `'${path}' and '${clash}' name the same module`);
      continue;
    }
    folded.set(fold(identity), path);
    const parsed = parse(files[path]!, {
      testModule: isTestModulePath(path),
      integrationTest: isIntegrationTestPath(path),
    });
    for (const diagnostic of parsed.diagnostics) diagnostics.push({ ...diagnostic, path });
    modules.set(identity, {
      path,
      identity,
      ...(parsed.program ? { program: parsed.program } : {}),
    });
  }
  const byPath = new Map([...modules.values()].map((module) => [module.path, module]));
  const entryModule = byPath.get(entry);
  if (!entryModule && !diagnostics.some((diagnostic) => diagnostic.path === entry))
    report(entry, "unknown-module", `entry module '${entry}' is not a package source file`);

  const resolvedUses = new Map<PackageModule, ResolvedUse[]>();
  // Follows `pub use` re-exports to the module that declares `name`; "loop"
  // when the chain returns to a module it passed
  // (spec/10-modules.md#r-module.pub-use.chain.loop).
  const exporter = (
    target: PackageModule,
    name: string,
    seen: Set<PackageModule>,
  ): PackageModule | "private" | "loop" | undefined => {
    const program = target.program!;
    if (topLevelNames(program).has(name)) return isPublic(program, name) ? target : "private";
    if (seen.has(target)) return "loop";
    seen.add(target);
    for (const use of resolvedUses.get(target) ?? [])
      if (use.declaration.public && use.names.includes(name))
        return exporter(use.target, name, seen);
    return undefined;
  };

  for (const module of modules.values()) {
    const uses: ResolvedUse[] = [];
    resolvedUses.set(module, uses);
    for (const declaration of module.program?.uses ?? []) {
      const root = declaration.module.split(".")[0];
      if (root === "std") continue;
      const span = declaration.span;
      if (root === "dep") {
        report(module.path, "unknown-module", "the package has no dependencies", span);
        continue;
      }
      const path = useModulePath(module, declaration);
      if (typeof path === "string") report(module.path, "unknown-module", path, span);
      if (!Array.isArray(path)) continue;
      const identity = path.join(".");
      const target = modules.get(identity);
      const grouped = files[module.path]!.slice(span.start.offset, span.end.offset).includes("{");
      const namespace = modules.has([...path, declaration.names[0]!.name].join("."));
      if (!grouped && namespace) {
        report(
          module.path,
          "unsupported-package-use",
          "module namespace uses are not supported; use selected declarations: `use pkg.m.{Name}`",
          span,
        );
        continue;
      }
      if (!target) {
        report(module.path, "unknown-module", `no package module '${identity}'`, span);
        continue;
      }
      // In an integration test module, `pkg` names only the library modules
      // (spec/10-modules.md#r-module.test.integration.pkg-root).
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
      const renamed = declaration.names.find(({ name, alias }) => alias && alias !== name);
      if (renamed) {
        report(
          module.path,
          "unsupported-package-use",
          `renaming '${renamed.name}' with 'as' is not supported for package declarations`,
          span,
        );
        continue;
      }
      uses.push({ declaration, target, names: declaration.names.map(({ name }) => name) });
    }
  }

  // Imported names must be public declarations of the target (or re-exported).
  const edges = new Map<PackageModule, Set<PackageModule>>();
  // Uses outside test code, for the folder graph (spec/10-modules.md#r-module.cycle.test-code).
  const folderUses: { module: PackageModule; use: ResolvedUse }[] = [];
  for (const [module, uses] of resolvedUses) {
    const local = module.program ? topLevelNames(module.program) : new Map();
    const imported = new Set<string>();
    const targets = new Set<PackageModule>();
    edges.set(module, targets);
    const testNames = new Set(module.program?.testOnlyNames ?? []);
    for (const use of uses) {
      targets.add(use.target);
      // Only test code may use a test module (spec/10-modules.md#r-module.test.non-test-use).
      const testCode =
        isTestModulePath(module.path) ||
        use.declaration.names.every(({ name, alias }) => testNames.has(alias ?? name));
      if (!testCode) folderUses.push({ module, use });
      if (isTestModulePath(use.target.path) && !testCode)
        report(
          module.path,
          "test-only-use",
          `only test code may use the test module '${use.target.identity}'`,
          use.declaration.span,
        );
      for (const name of use.names) {
        const found = exporter(use.target, name, new Set());
        // A plain use into a pub use loop has the loop's code
        // (spec/10-modules.md#r-module.pub-use.chain.loop-use).
        if (found === "loop")
          report(
            module.path,
            "re-export-loop",
            use.declaration.public
              ? `'pub use' of '${name}' leads back to itself through module '${use.target.identity}'; a pub use chain must end at a declaration`
              : `'use' of '${name}' leads into a pub use loop through module '${use.target.identity}'; a pub use chain must end at a declaration`,
            use.declaration.span,
          );
        else if (found === undefined)
          report(
            module.path,
            "unknown-import",
            `module '${use.target.identity}' declares no '${name}'`,
            use.declaration.span,
          );
        else if (found === "private")
          report(
            module.path,
            "private-import",
            `'${name}' is private to module '${use.target.identity}'; mark it 'pub'`,
            use.declaration.span,
          );
        else if (found !== use.target) targets.add(found);
        if (local.has(name) || imported.has(name))
          report(
            module.path,
            "duplicate-module-name",
            `imported name '${name}' is declared more than once`,
            use.declaration.span,
          );
        imported.add(name);
      }
    }
  }

  // The folder graph must be acyclic (spec/10-modules.md#r-module.cycle.acyclic).
  // Files of one folder may use each other in a loop.
  for (const loop of folderLoops(folderUses)) {
    const fix = loop.find(({ use }) => !use.target.path.endsWith("/mod.hd"));
    const at = fix ?? loop[0]!;
    const steps = loop.map(({ from, to, module, use }) => {
      const { line } = use.declaration.span.start;
      const text = files[module.path]!.split("\n")[line - 1]!.trim();
      return `  ${from}/ -> ${to}/: ${module.path}:${line}: ${text}`;
    });
    const help = fix
      ? `move ${fix.use.target.path} to ${fix.use.target.path.replace(/\.hd$/, "/mod.hd")}; ` +
        `its module name '${fix.use.target.identity}' and every use line stay the same`
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
  if (options.tests)
    for (const module of modules.values()) if (isTestModulePath(module.path)) visit(module);

  const order = initializationOrder(reachable, edges);

  // The joined program has one namespace for every linked module: a name is
  // either one module's declaration or one standard-library declaration.
  const namespace = new Map<string, { module: PackageModule; std?: string }>();
  for (const module of order) {
    const program = module.program;
    if (!program) continue;
    for (const [name, span] of topLevelNames(program)) {
      const owner = namespace.get(name);
      if (owner && owner.module !== module)
        report(
          module.path,
          "package-name-collision",
          `'${name}' is also declared in module '${owner.module.identity}'; linked modules share one namespace, so top-level names must differ`,
          span,
        );
      else namespace.set(name, { module });
    }
    const main = program.functions.find(({ name }) => name === "main");
    if (module !== entryModule && main)
      report(
        module.path,
        "package-name-collision",
        "only the entry module may declare 'main' in a linked package",
        main.span,
      );
    for (const declaration of program.uses) {
      if (!isStandardUse(declaration)) continue;
      for (const { name, alias } of declaration.names) {
        const local = alias ?? name;
        const std = `${declaration.module}.${name}`;
        const owner = namespace.get(local);
        if (owner && owner.module !== module && owner.std !== std)
          report(
            module.path,
            "package-name-collision",
            `'${local}' names a different declaration in module '${owner.module.identity}'`,
            declaration.span,
          );
        else if (!owner) namespace.set(local, { module, std });
      }
    }
  }

  const segments: LinkSegment[] = [];
  const locate = (diagnostic: Diagnostic): PackageDiagnostic => {
    const segment =
      segments.findLast(({ firstLine }) => firstLine <= diagnostic.span.start.line) ?? segments[0];
    if (!segment) return { ...diagnostic, path: entry };
    const move = (position: SourcePosition): SourcePosition => ({
      ...position,
      line: Math.min(Math.max(1, position.line - segment.firstLine + 1), segment.lineCount),
      column: Math.max(1, position.column - segment.indent),
    });
    return {
      ...diagnostic,
      path: segment.path,
      span: { start: move(diagnostic.span.start), end: move(diagnostic.span.end) },
    };
  };
  if (diagnostics.some(({ severity }) => severity !== "warning") || !entryModule)
    return { modules: order, diagnostics, locate };

  // Join the modules. Package uses are dropped and a standard use keeps only
  // the names no earlier module imported; every other line stays in place.
  const importedStd = new Set<string>();
  let source = "";
  let line = 1;
  for (const module of order) {
    let text = joinedText(module, files[module.path]!, importedStd);
    if (!text.endsWith("\n")) text += "\n";
    const lineCount = text.split("\n").length - 1;
    // A test module's top level is test position, so it joins the linked
    // source as a `tests:` block (spec/10-modules.md#test-modules). Its
    // top-level `pub` is dropped, since a `tests:` item cannot be `pub`;
    // the linked modules share one namespace anyway.
    if (isTestModulePath(module.path)) {
      text = `tests:\n${text
        .replace(/^pub(?=\s+(?:fn|data|enum|trait|type|use)\b)/gm, "   ")
        .replace(/^(?=.)/gm, "    ")}`;
      line += 1;
    }
    const indent = isTestModulePath(module.path) ? 4 : 0;
    segments.push({ path: module.path, firstLine: line, lineCount, indent });
    source += text;
    line += lineCount;
  }
  return {
    source,
    modules: order,
    diagnostics,
    locate,
    entryLine: segments.find(({ path }) => path === entry)?.firstLine,
  };
}

function isStandardUse(declaration: UseDecl): boolean {
  return declaration.module.split(".")[0] === "std";
}

// A module's text in the joined source: package uses are dropped, and a
// standard use keeps only the names that no earlier module imported
// (`importedStd`, which this adds to). Every other line stays in place.
function joinedText(module: PackageModule, source: string, importedStd: Set<string>): string {
  let text = source;
  const edits: { readonly start: number; readonly end: number; readonly text: string }[] = [];
  const moduleStd = new Set<string>();
  for (const declaration of module.program!.uses) {
    // A `pub use` span starts at `use`; the edit covers the `pub` too.
    const end = declaration.span.end.offset;
    let start = declaration.span.start.offset;
    if (declaration.public) start = text.lastIndexOf("pub", start);
    const newlines = text.slice(start, end).replace(/[^\n]/g, "");
    if (!isStandardUse(declaration)) {
      edits.push({ start, end, text: newlines });
      continue;
    }
    const kept = declaration.names.filter(({ name, alias }) => {
      const key = `${alias ?? name}=${declaration.module}.${name}`;
      moduleStd.add(key);
      return !importedStd.has(key);
    });
    if (kept.length === declaration.names.length) continue;
    const names = kept.map(({ name, alias }) => (alias ? `${name} as ${alias}` : name));
    const pub = declaration.public ? "pub " : "";
    edits.push({
      start,
      end,
      text:
        kept.length === 0
          ? newlines
          : `${pub}use ${declaration.module}.{${names.join(", ")}}${newlines}`,
    });
  }
  for (const key of moduleStd) importedStd.add(key);
  for (const edit of edits.toReversed())
    text = text.slice(0, edit.start) + edit.text + text.slice(edit.end);
  return text;
}

// Initialization order (spec/10-modules.md#initialization-order): modules
// that use each other form one group; a group is ready once every group it
// uses is done, and ready groups go by their least identity. Test modules
// go after the others, so a standard use that a test module shares with
// library code stays outside the test module's `tests:` block. Inside a
// group the prototype joins modules by identity; it does not interleave
// their statements by dependency (src/README.md, Implemented Surface).
function initializationOrder(
  reachable: ReadonlySet<PackageModule>,
  edges: ReadonlyMap<PackageModule, ReadonlySet<PackageModule>>,
): PackageModule[] {
  const groups = stronglyConnected([...reachable], (module) =>
    [...(edges.get(module) ?? [])].filter((target) => reachable.has(target)),
  ).map((group) => group.sort(byIdentity));
  const groupOf = new Map<PackageModule, PackageModule[]>();
  for (const group of groups) for (const module of group) groupOf.set(module, group);
  const libraryGroup = (group: PackageModule[]): boolean =>
    group.some(({ path }) => !isTestModulePath(path));
  const order: PackageModule[] = [];
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
    pending.delete(next);
  }
  return order;
}

function byIdentity(left: PackageModule, right: PackageModule): number {
  return left.identity < right.identity ? -1 : left.identity > right.identity ? 1 : 0;
}

interface FolderEdge {
  readonly from: string;
  readonly to: string;
  readonly module: PackageModule;
  readonly use: ResolvedUse;
}

/** The folder that holds a package file (spec/10-modules.md#r-module.folder.directory). */
function folderOf(path: string): string {
  return path.slice(0, path.lastIndexOf("/"));
}

// One shortest loop per strongly connected component of the folder graph
// (spec/10-modules.md#cycle-diagnostic). Each edge is carried by the first use
// that makes it; `tangle` is the component's size.
function folderLoops(
  uses: readonly { module: PackageModule; use: ResolvedUse }[],
): (FolderEdge[] & { tangle: number })[] {
  const out = new Map<string, Map<string, FolderEdge>>();
  for (const { module, use } of uses) {
    const from = folderOf(module.path);
    const to = folderOf(use.target.path);
    if (from === to) continue;
    const targets = out.get(from) ?? new Map<string, FolderEdge>();
    out.set(from, targets);
    if (!targets.has(to)) targets.set(to, { from, to, module, use });
  }
  const successors = (folder: string): FolderEdge[] =>
    [...(out.get(folder)?.values() ?? [])].sort((left, right) => (left.to < right.to ? -1 : 1));
  const folders = [
    ...new Set([...out.keys(), ...[...out.values()].flatMap((targets) => [...targets.keys()])]),
  ].sort();
  const loops: (FolderEdge[] & { tangle: number })[] = [];
  for (const component of stronglyConnected(folders, (folder) =>
    successors(folder).map(({ to }) => to),
  )) {
    if (component.length < 2) continue;
    const inside = new Set(component);
    let best: FolderEdge[] | undefined;
    for (const start of [...component].sort()) {
      // Breadth-first search back to `start`, staying inside the component.
      const previous = new Map<string, FolderEdge>();
      const queue = [start];
      let closing: FolderEdge | undefined;
      for (let index = 0; index < queue.length && !closing; index++) {
        for (const edge of successors(queue[index]!)) {
          if (!inside.has(edge.to)) continue;
          if (edge.to === start) {
            closing = edge;
            break;
          }
          if (previous.has(edge.to)) continue;
          previous.set(edge.to, edge);
          queue.push(edge.to);
        }
      }
      if (!closing) continue;
      const path = [closing];
      for (let edge = previous.get(closing.from); edge; edge = previous.get(edge.from))
        path.unshift(edge);
      if (!best || path.length < best.length) best = path;
    }
    if (best) loops.push(Object.assign(best, { tangle: component.length }));
  }
  return loops;
}

// Tarjan's algorithm: the strongly connected components of a graph.
function stronglyConnected<T>(nodes: readonly T[], next: (node: T) => readonly T[]): T[][] {
  const index = new Map<T, number>();
  const low = new Map<T, number>();
  const stack: T[] = [];
  const onStack = new Set<T>();
  const components: T[][] = [];
  const connect = (node: T): void => {
    index.set(node, index.size);
    low.set(node, index.get(node)!);
    stack.push(node);
    onStack.add(node);
    for (const target of next(node)) {
      if (!index.has(target)) {
        connect(target);
        low.set(node, Math.min(low.get(node)!, low.get(target)!));
      } else if (onStack.has(target)) low.set(node, Math.min(low.get(node)!, index.get(target)!));
    }
    if (low.get(node) !== index.get(node)) return;
    const component: T[] = [];
    for (;;) {
      const member = stack.pop()!;
      onStack.delete(member);
      component.push(member);
      if (member === node) break;
    }
    components.push(component);
  };
  for (const node of nodes) if (!index.has(node)) connect(node);
  return components;
}
