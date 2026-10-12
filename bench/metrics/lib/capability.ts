// What the `hd` under test offers, found from its documented help text and
// from what its runs write. Pillar 2 metrics use these probes to report
// `n/a` for a feature this hd lacks. Each probe runs once per hd command.

import { freshCache, materialize, runProblem } from "./fixture.ts";
import { runHd, type HdCommand } from "./hd.ts";
import { listTree, makeTempDir } from "./tmp.ts";

/** The commands whose help text the probes read. */
const HELP_COMMANDS = ["check", "test", "build", "clean", "add", "fetch", "repl"];

const helpTexts = new Map<string, Promise<{ readonly all: string; readonly check: string }>>();

/**
 * The text of `hd help` and of `hd help COMMAND` for the commands the
 * probes read; a command whose help fails adds nothing. `check` is the
 * text of `hd help check` alone.
 */
export function helpText(hd: HdCommand): Promise<{ readonly all: string; readonly check: string }> {
  let found = helpTexts.get(hd.display);
  if (!found) {
    found = (async () => {
      const cwd = makeTempDir("help");
      const texts: string[] = [];
      let check = "";
      for (const command of [undefined, ...HELP_COMMANDS]) {
        const result = await runHd(hd, command ? ["help", command] : ["help"], {
          cwd,
          timeoutMs: 30_000,
        });
        if (result.status !== 0) continue;
        texts.push(result.stdout);
        if (command === "check") check = result.stdout;
      }
      return { all: texts.join("\n"), check };
    })();
    helpTexts.set(hd.display, found);
  }
  return found;
}

/** How to run `hd check` with a given number of threads. */
export interface ThreadControl {
  /** The flag or variable, for a report. */
  readonly name: string;
  readonly args: (threads: number) => string[];
  readonly env: (threads: number) => Record<string, string>;
}

/**
 * The documented thread control of `hd check`: a `--jobs`, `--threads` or
 * `-j` flag in `hd help check`, or an `HD_THREADS`, `HD_JOBS` or
 * `HD_PARALLELISM` variable named in any help text. Undefined when none is
 * documented.
 */
export function findThreadControl(checkHelp: string, allHelp: string): ThreadControl | undefined {
  const named = (pattern: string) =>
    new RegExp(`(?:^|\\s)(${pattern})(?=[\\s=,]|$)`, "m").exec(checkHelp)?.[1];
  const flag = named("--jobs|--threads") ?? named("-j");
  if (flag)
    return {
      name: flag,
      args: (threads) => (flag === "-j" ? [`-j${threads}`] : [`${flag}=${threads}`]),
      env: () => ({}),
    };
  const variable = /\b(HD_(?:THREADS|JOBS|PARALLELISM|NUM_THREADS))\b/.exec(allHelp)?.[1];
  if (variable)
    return { name: variable, args: () => [], env: (threads) => ({ [variable]: String(threads) }) };
  return undefined;
}

/**
 * A documented size cap of the cache: an environment variable named in the
 * help text as `HD_CACHE_..._MAX`, `..._CAP` or `..._LIMIT`. The metrics set
 * it to a byte count. Undefined when none is documented.
 */
export function findCacheCap(allHelp: string): string | undefined {
  return /\b(HD_CACHE_[A-Z_]*?(?:MAX|CAP|LIMIT)[A-Z_]*)\b/.exec(allHelp)?.[1];
}

/** Where a compilation cache lives, as found by `findCompileCache`. */
export interface CompileCache {
  /** `HD_CACHE`, or `package` for files the check wrote inside the package. */
  readonly where: "HD_CACHE" | "package";
  /** The paths the check wrote, relative to that directory. */
  readonly paths: readonly string[];
}

const compileCaches = new Map<string, Promise<CompileCache | string | undefined>>();

/**
 * Whether `hd check` keeps a compilation cache: the files a check of the
 * small package writes under a fresh `HD_CACHE`, or inside the package.
 * The package has no dependencies, so nothing there is a fetched version.
 * Returns undefined when the check writes nothing, and a reason when the
 * check fails.
 */
export function findCompileCache(hd: HdCommand): Promise<CompileCache | string | undefined> {
  let found = compileCaches.get(hd.display);
  if (!found) {
    found = (async () => {
      const { dir, pkg } = materialize("small", "compile-cache-probe");
      const env = freshCache();
      const result = await runHd(hd, ["check"], { cwd: dir, env, timeoutMs: 60_000 });
      const problem = runProblem(result, 60_000, "the cache probe's hd check");
      if (problem) return problem;
      const cached = listTree(env.HD_CACHE!);
      if (cached.length) return { where: "HD_CACHE", paths: cached };
      const written = listTree(dir).filter((path) => !pkg.files.has(path));
      if (written.length) return { where: "package", paths: written };
      return undefined;
    })();
    compileCaches.set(hd.display, found);
  }
  return found;
}

const repls = new Map<string, Promise<boolean>>();

/**
 * Whether this hd has a REPL that reads standard input: `hd help repl`
 * succeeds, and `hd repl` fed `1 + 2` shows 3 and exits 0.
 */
export function hasStdinRepl(hd: HdCommand): Promise<boolean> {
  let found = repls.get(hd.display);
  if (!found) {
    found = (async () => {
      const cwd = makeTempDir("repl-probe");
      const help = await runHd(hd, ["help", "repl"], { cwd, timeoutMs: 30_000 });
      if (help.status !== 0) return false;
      const result = await runHd(hd, ["repl"], { cwd, timeoutMs: 30_000, input: "1 + 2\n" });
      return result.status === 0 && /(^|\D)3(\D|$)/m.test(result.stdout);
    })();
    repls.set(hd.display, found);
  }
  return found;
}
