// The hd programs of the metrics, which live as files under
// bench/metrics/fixtures/, never as strings in a script.
//
// - A program fixture is a `.hd` file that checks as it stands. A line that
//   ends in `# param: NAME` holds one integer literal outside its strings,
//   which a metric may replace, as a loop count: `withParams`. The literal
//   written there is the default.
// - A template, `.hd.tmpl`, is the hd text of a generator (lib/gen.ts,
//   lib/pathological.ts): `render` fills in `{{name}}`, `{{name ARG...}}`
//   for a function value, and the line blocks `{{#each name}}`,
//   `{{#if name}}` and `{{#unless name}}`, each closed by `{{/each}}`,
//   `{{/if}}` or `{{/unless}}`. A line that is only `{{! text }}` is a
//   comment.
// - `PROGRAMS` names every package a metric builds from program fixtures:
//   which fixture becomes which file, and the capabilities its hd.toml
//   grants. `programFiles` writes one, with its params.
//
// check-fixtures.ts checks every fixture, template output and corpus
// program with the hd under test, so a language change that breaks one
// fails CI (crates/hd_cli/tests/all/metrics_fixtures.rs).

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

import { makeTempDir, writeTree } from "./tmp.ts";

/** The fixture directory. */
export const FIXTURES = join(import.meta.dirname, "..", "fixtures");

/** The files of a package, by path relative to its root. */
export type Files = ReadonlyMap<string, string>;

const texts = new Map<string, string>();

/** The text of a fixture, by path relative to the fixture directory. */
export function fixture(path: string): string {
  let text = texts.get(path);
  if (text === undefined) {
    text = readFileSync(join(FIXTURES, path), "utf8");
    texts.set(path, text);
  }
  return text;
}

/** Every file under the fixture directory, relative to it, sorted. */
export function fixturePaths(dir = FIXTURES): string[] {
  return readdirSync(dir)
    .flatMap((entry) => {
      const path = join(dir, entry);
      return statSync(path).isDirectory() ? fixturePaths(path) : [path];
    })
    .map((path) => (dir === FIXTURES ? relative(FIXTURES, path) : path))
    .sort();
}

// ------------------------------------------------------------- params

/** Integer values for the `# param: NAME` lines of a fixture. */
export type Params = Readonly<Record<string, number>>;

const PARAM_MARK = /\s+#\s*param:\s*([a-z][a-z0-9-]*)\s*$/;
const STRING = /"(?:[^"\\]|\\.)*"/g;
const INTEGER = /(?<![\w.$])\d[\d_]*(?![\w.])/g;

interface ParamLine {
  readonly name: string;
  readonly code: string;
  readonly mark: string;
  /** Where the one integer literal of `code` starts, and its text. */
  readonly at: number;
  readonly literal: string;
}

function paramLine(line: string, where: string): ParamLine | undefined {
  const mark = PARAM_MARK.exec(line);
  if (!mark) return undefined;
  const code = line.slice(0, mark.index);
  const blanked = code.replaceAll(STRING, (text) => " ".repeat(text.length));
  const literals = [...blanked.matchAll(INTEGER)];
  if (literals.length !== 1)
    throw new Error(
      `${where}: a \`# param: ${mark[1]}\` line needs exactly one integer literal outside strings, found ${literals.length}`,
    );
  return {
    name: mark[1]!,
    code,
    mark: line.slice(mark.index),
    at: literals[0]!.index,
    literal: literals[0]![0],
  };
}

/** The params of a fixture text, with their default values. */
export function paramsOf(text: string, where = "fixture"): Map<string, number> {
  const found = new Map<string, number>();
  for (const line of text.split("\n")) {
    const param = paramLine(line, where);
    if (param) found.set(param.name, Number(param.literal.replaceAll("_", "")));
  }
  return found;
}

/**
 * The text with each `# param: NAME` line's literal replaced by
 * `params[NAME]`; params left out keep their defaults. A param the text
 * does not have is an error.
 */
export function withParams(text: string, params: Params, where = "fixture"): string {
  const unused = new Set(Object.keys(params));
  const lines = text.split("\n").map((line) => {
    const param = paramLine(line, where);
    if (!param || !(param.name in params)) return line;
    unused.delete(param.name);
    const value = params[param.name]!;
    if (!Number.isSafeInteger(value) || value < 0)
      throw new Error(`${where}: param ${param.name} must be a non-negative integer, not ${value}`);
    return `${param.code.slice(0, param.at)}${value}${param.code.slice(param.at + param.literal.length)}${param.mark}`;
  });
  if (unused.size > 0) throw new Error(`${where}: no param ${[...unused].join(", ")}`);
  return lines.join("\n");
}

// ---------------------------------------------------------- templates

/** A value a template can name. A list is the scopes of an `{{#each}}` block. */
export type TemplateValue =
  | string
  | number
  | boolean
  | ((...args: string[]) => string | number)
  | readonly TemplateScope[];

export interface TemplateScope {
  readonly [name: string]: TemplateValue;
}

const BLOCK = /^\s*\{\{([#/])(each|if|unless)(?:\s+([a-z][a-zA-Z0-9]*))?\}\}\s*$/;
const COMMENT = /^\s*\{\{!.*\}\}\s*$/;
const FIELD = /\{\{([a-z][a-zA-Z0-9]*)((?:\s+[^\s{}]+)*)\}\}/g;

function lookup(scope: TemplateScope, name: string, where: string): TemplateValue {
  if (!(name in scope)) throw new Error(`${where}: the template names {{${name}}}, which is unset`);
  return scope[name]!;
}

function renderLines(lines: readonly string[], scope: TemplateScope, where: string): string[] {
  const out: string[] = [];
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index]!;
    if (COMMENT.test(line)) continue;
    const block = BLOCK.exec(line);
    if (block) {
      if (block[1] === "/" || !block[3])
        throw new Error(`${where}:${index + 1}: unexpected ${line.trim()}`);
      const [, , kind, name] = block as unknown as [string, string, string, string];
      let depth = 1;
      let end = index + 1;
      for (; end < lines.length; end++) {
        const inner = BLOCK.exec(lines[end]!);
        if (inner?.[2] !== kind) continue;
        depth += inner[1] === "#" ? 1 : -1;
        if (depth === 0) break;
      }
      if (end === lines.length)
        throw new Error(`${where}:${index + 1}: {{#${kind}}} is not closed`);
      const body = lines.slice(index + 1, end);
      const value = lookup(scope, name, where);
      if (kind === "each") {
        if (!Array.isArray(value)) throw new Error(`${where}: {{#each ${name}}} needs a list`);
        for (const item of value as readonly TemplateScope[])
          out.push(...renderLines(body, { ...scope, ...item }, where));
      } else if (Boolean(value) === (kind === "if")) out.push(...renderLines(body, scope, where));
      index = end;
      continue;
    }
    out.push(
      line.replaceAll(FIELD, (_, name: string, args: string) => {
        const value = lookup(scope, name, where);
        if (typeof value === "function") return String(value(...args.trim().split(/\s+/)));
        if (args.trim()) throw new Error(`${where}: {{${name}}} is not a function`);
        if (typeof value === "object") throw new Error(`${where}: {{${name}}} is a list`);
        return String(value);
      }),
    );
  }
  return out;
}

/** The text of a template with its fields and blocks filled in from `scope`. */
export function render(template: string, scope: TemplateScope, where = "template"): string {
  return renderLines(template.split("\n"), scope, where).join("\n");
}

/** A template fixture, rendered. */
export const renderFixture = (path: string, scope: TemplateScope): string =>
  render(fixture(path), scope, path);

// ----------------------------------------------------------- packages

export interface ManifestOptions {
  /** Host capabilities granted in `[capabilities]`, each `true`. */
  readonly capabilities?: readonly string[];
  /** Capabilities granted to tests and tasks only, in `[dev-capabilities]`. */
  readonly devCapabilities?: readonly string[];
}

/** The hd.toml of a package: its name and capability grants (cli.cap.table). */
export function manifest(name: string, options: ManifestOptions = {}): string {
  const table = (title: string, names: readonly string[] | undefined): string[] =>
    names?.length ? ["", `[${title}]`, ...names.map((key) => `${key} = true`)] : [];
  return [
    "[package]",
    `name = "${name}"`,
    ...table("capabilities", options.capabilities),
    ...table("dev-capabilities", options.devCapabilities),
    "",
  ].join("\n");
}

/** Writes a package into a fresh temporary directory and returns it. */
export function writePackage(label: string, files: Files): string {
  const dir = makeTempDir(label);
  writeTree(dir, files);
  return dir;
}

// ------------------------------------------------------------ programs

/** The package name of every built program; its executable takes the name. */
export const PACKAGE = "bench";

export interface ProgramSpec extends ManifestOptions {
  /** The package name. Default PACKAGE, which names a built program. */
  readonly name?: string;
  /** Each file of the package, by its path, from the fixture that holds it. */
  readonly files: Readonly<Record<string, string>>;
}

const CONSOLE = { capabilities: ["Console"] } as const;
const main = (fixture: string): Record<string, string> => ({ "src/main.hd": fixture });

const hostCall = (fixture: string): ProgramSpec => ({
  capabilities: ["Console", "FsRead"],
  files: {
    "src/main.hd": `host-call-overhead/${fixture}.hd`,
    // Three more modules, so list_dir! of src/ returns 4 entries.
    "src/a.hd": "host-call-overhead/filler.hd",
    "src/b.hd": "host-call-overhead/filler.hd",
    "src/c.hd": "host-call-overhead/filler.hd",
  },
});

/** Every package a metric builds from program fixtures, by name. */
export const PROGRAMS = {
  "allocations/counted": { ...CONSOLE, files: main("allocations/counted.hd") },
  "allocations/list": { ...CONSOLE, files: main("allocations/list.hd") },
  "allocations/map": { ...CONSOLE, files: main("allocations/map.hd") },
  "allocations/chain": { ...CONSOLE, files: main("allocations/chain.hd") },
  "check-cost": {
    ...CONSOLE,
    name: "loop",
    files: { "src/main.hd": "check-cost/main.hd", "src/sum.hd": "check-cost/sum.hd" },
  },
  "dead-code/std-free": { files: main("dead-code/std-free.hd") },
  "dead-code/unused-uses": { files: main("dead-code/unused-uses.hd") },
  "dev-speed/empty-test": { name: "case", files: main("dev-speed/empty-test.hd") },
  "host-call-overhead/println": hostCall("println"),
  "host-call-overhead/println-empty": hostCall("println-empty"),
  "host-call-overhead/read-text": hostCall("read-text"),
  "host-call-overhead/read-text-empty": hostCall("read-text-empty"),
  "host-call-overhead/list-dir": hostCall("list-dir"),
  "host-call-overhead/list-dir-empty": hostCall("list-dir-empty"),
  "long-run-memory/service": { ...CONSOLE, files: main("long-run-memory/service.hd") },
  "proptest-perf/suite": {
    name: "props",
    files: { "src/lib.hd": "proptest-perf/suite.hd", "src/shop.hd": "proptest-perf/shop.hd" },
  },
  "proptest-perf/failing": {
    name: "props",
    files: { "src/lib.hd": "proptest-perf/failing.hd", "src/shop.hd": "proptest-perf/shop.hd" },
  },
  "runtime/baseline": { files: main("runtime/baseline.hd") },
  "serde-throughput/encode": {
    ...CONSOLE,
    files: {
      "src/main.hd": "serde-throughput/encode.hd",
      "src/orders.hd": "serde-throughput/orders.hd",
    },
  },
  "serde-throughput/decode": {
    ...CONSOLE,
    files: {
      "src/main.hd": "serde-throughput/decode.hd",
      "src/orders.hd": "serde-throughput/orders.hd",
    },
  },
  "size-startup-heap/tiny": { ...CONSOLE, files: main("size-startup-heap/tiny.hd") },
  "suspension-overhead/empty": { ...CONSOLE, files: main("suspension-overhead/empty.hd") },
  "suspension-overhead/await": { ...CONSOLE, files: main("suspension-overhead/await.hd") },
  "suspension-overhead/all": { ...CONSOLE, files: main("suspension-overhead/all.hd") },
  "suspension-overhead/race": { ...CONSOLE, files: main("suspension-overhead/race.hd") },
  "text-throughput/build": { ...CONSOLE, files: main("text-throughput/build.hd") },
  "text-throughput/split": { ...CONSOLE, files: main("text-throughput/split.hd") },
  "text-throughput/regex": { ...CONSOLE, files: main("text-throughput/regex.hd") },
} as const satisfies Record<string, ProgramSpec>;

export type ProgramName = keyof typeof PROGRAMS;

/**
 * The files of a program package, hd.toml included, with `params` applied
 * to its fixtures. Each param must exist in at least one of them.
 */
export function programFiles(name: ProgramName, params: Params = {}): Map<string, string> {
  const spec: ProgramSpec = PROGRAMS[name];
  const files = new Map<string, string>([["hd.toml", manifest(spec.name ?? PACKAGE, spec)]]);
  const unused = new Set(Object.keys(params));
  for (const [path, source] of Object.entries(spec.files)) {
    const text = fixture(source);
    const own = paramsOf(text, source);
    const mine = Object.fromEntries(Object.entries(params).filter(([key]) => own.has(key)));
    for (const key of Object.keys(mine)) unused.delete(key);
    files.set(path, withParams(text, mine, source));
  }
  if (unused.size > 0) throw new Error(`${name}: no param ${[...unused].join(", ")}`);
  return files;
}

/** Writes a program package into a fresh temporary directory and returns it. */
export const writeProgram = (
  name: ProgramName,
  params: Params = {},
  label: string = name,
): string => writePackage(label.replaceAll("/", "-"), programFiles(name, params));
