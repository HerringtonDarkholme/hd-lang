import { readFile, readdir, stat } from "node:fs/promises";
import { join, resolve } from "node:path";

import { analyze } from "../compiler.ts";
import { DiagnosticReporter, Report, type OutputFormat } from "../diagnostic-report.ts";
import { moduleIdentity, SOURCE_ROOT } from "../package.ts";
import { parse } from "../parser/index.ts";
import { explainCode, loadSpecIndex, type SpecMention } from "../spec-index.ts";
import { workingDirectory, type CommandEnvironment, type CommandIo } from "./io.ts";
import { withDependencies } from "./dependencies.ts";
import { packageMode } from "./package-mode.ts";
import {
  formatDefinition,
  formatDocumentation,
  SymbolIndex,
  type InferredTypes,
  type SourceModule,
} from "../symbols.ts";

// The name-addressed query commands: `hd explain CODE`, `hd def NAME [PATH]`,
// and `hd doc NAME [PATH]`. src/README.md documents their output.

function uniqueMentions(mentions: readonly SpecMention[]): SpecMention[] {
  const seen = new Set<string>();
  return mentions.filter((mention) => {
    const key = `${mention.anchor} ${mention.rule ?? ""}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export interface ExplainArgs extends CommandEnvironment {
  readonly code: string;
  readonly format: OutputFormat;
}

/** `hd explain CODE`: what the specification says about a diagnostic code. */
export async function explainCommand(args: ExplainArgs, io: CommandIo): Promise<number> {
  const { code, format } = args;
  const index = await loadSpecIndex(args.specDir);
  const explanation = explainCode(index, code);
  if (format === "json") {
    io.out(
      JSON.stringify(
        explanation
          ? {
              code,
              known: true,
              category: explanation.category ?? null,
              meaning: explanation.meaning ?? null,
              meaningSource: explanation.meaning ? "spec/README.md#diagnostics" : null,
              rules: explanation.rules.map(({ id, anchor, file, line, text }) => ({
                id,
                anchor,
                file: `spec/${file}`,
                line,
                text,
              })),
              mentions: uniqueMentions(explanation.mentions).map((mention) => ({
                anchor: mention.anchor,
                heading: mention.heading,
                file: `spec/${mention.file}`,
                line: mention.line,
                rule: mention.rule ?? null,
              })),
              fixtures: explanation.fixtures,
            }
          : { code, known: false },
        null,
        2,
      ),
    );
    return explanation ? 0 : 1;
  }
  if (!explanation) {
    io.err(`hd explain: the specification names no diagnostic code '${code}'`);
    return 1;
  }
  const lines = [`${code}: ${explanation.category ?? "code"}`];
  if (explanation.meaning) lines.push(`  ${explanation.meaning}`, "  (spec/README.md#diagnostics)");
  if (explanation.rules.length > 0) {
    lines.push("", "rules:");
    for (const rule of explanation.rules) lines.push(`  ${rule.anchor}`, `    ${rule.text}`);
  }
  const mentions = uniqueMentions(explanation.mentions);
  if (mentions.length > 0) {
    lines.push("", "mentioned in:");
    for (const mention of mentions)
      lines.push(
        `  ${mention.anchor}${mention.rule ? ` (${mention.rule})` : ""}  line ${mention.line}`,
      );
  }
  if (explanation.fixtures.length > 0) {
    lines.push("", "fixtures:");
    for (const fixture of explanation.fixtures)
      lines.push(`  ${fixture.path}  ${fixture.phase} ${fixture.expectation}`);
  }
  io.out(lines.join("\n"));
  return 0;
}

function inferredTypes(source: string): InferredTypes | undefined {
  const { hir } = analyze(source);
  if (!hir) return undefined;
  return {
    functions: new Map(
      hir.functions
        .filter((declaration) => !declaration.synthetic)
        .map((declaration) => [
          declaration.name,
          { result: declaration.result, requirements: declaration.requirements },
        ]),
    ),
    globals: new Map(hir.globals.map((global) => [global.name, global.type])),
  };
}

interface Project {
  readonly modules: SourceModule[];
  readonly packageMode: boolean;
  readonly inferred?: InferredTypes;
  readonly failed: boolean;
}

/** Parses the FILE or package `args.target` names; messages name it as given. */
async function loadProject(args: LookupArgs, io: CommandIo): Promise<Project | undefined> {
  const { target, format } = args;
  const location = resolve(workingDirectory(args), target);
  let info;
  try {
    info = await stat(location);
  } catch {
    io.err(`hd: cannot read ${target}`);
    return undefined;
  }
  const report = new Report(format, io);
  const load = async (path: string, identity: string, display: string) => {
    const source = await readFile(path, "utf8");
    const result = parse(source);
    if (!result.program) {
      const index = format === "json" ? await loadSpecIndex(args.specDir) : undefined;
      const reporter = new DiagnosticReporter(report, display, source, index);
      for (const diagnostic of result.diagnostics) reporter.diagnostic(diagnostic);
      return { source, module: undefined };
    }
    return { source, module: { path: display, identity, source, program: result.program } };
  };
  if (info.isFile()) {
    const { source, module } = await load(location, "", target);
    if (!module) return { modules: [], packageMode: false, failed: true };
    return {
      modules: [module],
      packageMode: false,
      inferred: inferredTypes(source),
      failed: false,
    };
  }
  const root = join(location, SOURCE_ROOT);
  let entries: string[];
  try {
    entries = (await readdir(root, { recursive: true })).map(String);
  } catch {
    io.err(`hd: ${target} is not a package: it has no ${SOURCE_ROOT} directory`);
    return undefined;
  }
  const modules: SourceModule[] = [];
  let failed = false;
  for (const entry of entries.map((name) => name.replaceAll("\\", "/")).sort()) {
    if (!entry.endsWith(".hd")) continue;
    const path = `${SOURCE_ROOT}${entry}`;
    const identity = moduleIdentity(path);
    if (identity === undefined) continue;
    const { module } = await load(join(location, path), identity, join(target, path));
    if (module) modules.push(module);
    else failed = true;
  }
  return { modules, packageMode: true, failed };
}

export interface LookupArgs extends CommandEnvironment {
  /** The symbol, such as `main` or `Shape.area`. */
  readonly name: string;
  /** A FILE or a package directory. */
  readonly target: string;
  readonly format: OutputFormat;
}

/** `hd def NAME [FILE|PKG]`: where a symbol is defined, with its signature. */
export function defCommand(args: LookupArgs, io: CommandIo): Promise<number> {
  return lookup("def", args, io);
}

/** `hd doc NAME [FILE|PKG]`: a symbol's definition, documentation, and members. */
export function docCommand(args: LookupArgs, io: CommandIo): Promise<number> {
  return lookup("doc", args, io);
}

/**
 * The public modules of the dependency that `key` names, for a NAME that
 * starts with `dep.KEY` (spec/cli/command-line.md#r-cli.doc.name.dependency);
 * or the message of why there is none. The dependencies are selected and
 * fetched as for `hd check` (spec/cli/command-line.md#r-cli.dep.implicit-fetch).
 */
async function dependencyModules(
  args: LookupArgs,
  io: CommandIo,
  key: string,
): Promise<SourceModule[] | string> {
  const mode = await packageMode(resolve(workingDirectory(args), args.target));
  if (mode.kind !== "package") return `${args.target} is in no package, so it has no dependencies`;
  const pkg = await withDependencies(mode.package, args, (version) =>
    io.err(`hd: fetching ${version}`),
  );
  const problem = pkg.problems.find(
    ({ severity, path }) => severity === "error" && path.endsWith("hd.toml"),
  );
  if (problem)
    return `${problem.path}:${problem.line}: ${problem.code ?? "error"}: ${problem.message}`;
  const graph = pkg.dependencies;
  const id = graph?.dependencies[key] ?? graph?.devDependencies[key];
  const dependency = id === undefined ? undefined : graph?.packages[id];
  if (!dependency) return `package '${pkg.name}' has no dependency named '${key}'`;
  const modules: SourceModule[] = [];
  for (const [file, source] of Object.entries(dependency.files).sort()) {
    const identity = moduleIdentity(`${SOURCE_ROOT}${file}`);
    const program = parse(source).program;
    if (identity !== undefined && program)
      modules.push({ path: join(dependency.sourceRoot, file), identity, source, program });
  }
  return modules;
}

async function lookup(command: "def" | "doc", args: LookupArgs, io: CommandIo): Promise<number> {
  const { format } = args;
  let { name } = args;
  let project: Project | undefined;
  // `dep.KEY.ITEM` names an item of a dependency; only its pub items are found.
  const dependency = /^dep\.([^.]+)\.(.+)$/.exec(name);
  if (dependency) {
    const modules = await dependencyModules(args, io, dependency[1]!);
    if (typeof modules === "string") {
      io.err(`hd ${command}: ${modules}`);
      return 1;
    }
    project = { modules, packageMode: true, failed: false };
    name = `pkg.${dependency[2]!}`;
  } else project = await loadProject(args, io);
  if (!project) return 1;
  const index = new SymbolIndex(project.modules, project.packageMode, project.inferred);
  const found = index.lookup(name);
  const symbols = dependency ? found.symbols.filter((symbol) => symbol.public) : found.symbols;
  const suggestions = dependency ? [] : found.suggestions;
  name = args.name;
  if (format === "json") {
    io.out(JSON.stringify({ query: name, symbols, suggestions }, null, 2));
    return symbols.length > 0 ? 0 : 1;
  }
  if (symbols.length === 0) {
    const hint = suggestions.length > 0 ? `; candidates: ${suggestions.join(", ")}` : "";
    const partial = project.failed ? " (some modules did not parse)" : "";
    io.err(`hd ${command}: no symbol named ${name}${partial}${hint}`);
    return 1;
  }
  const render = command === "def" ? formatDefinition : formatDocumentation;
  io.out(symbols.map(render).join(command === "def" ? "\n" : "\n\n"));
  return 0;
}
