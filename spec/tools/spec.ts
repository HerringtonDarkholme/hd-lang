// Spec text tools: one entry point for counting, auditing, and citing rules.
//
//   npm run spec -- counts [--by chapter|prefix|topic|kind] [--json]
//   npm run spec -- audit [--strict] [--list] [--json]
//   npm run spec -- refs RULE-ID [--json]
//   npm run spec -- refs --dead [--brief] [--all] [--json]
//   npm run spec -- rewrite BASE [HEAD] [--json] [--fail-on KINDS]
//   npm run spec -- glossary [--markdown] [--json]
//
// The tools read spec text, fixtures, and records; they import only Node
// built-ins and spec/, never src/. Usage is documented in spec/tools/README.md.
//
// Exit status: 0 on success; 1 when `audit --strict` finds a warning or
// `refs --dead` finds a failing citation (a dead citation in spec/, a
// fixture, guide/, or lib/std that does not record history), or when
// `rewrite --fail-on` names a kind the rewrite trips; 2 on a usage error.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { audit, auditReport, auditTotals } from "./spec-audit.ts";
import { loadCorpus, loadCorpusAt, REPO_ROOT, SPEC_ROOT } from "./spec-corpus.ts";
import { counts, countsReport, type CountsView } from "./spec-counts.ts";
import { glossary, glossaryMarkdown, glossaryReport } from "./spec-glossary.ts";
import { buildIndex, deadCitations, deadReport, historicalIds, refsReport } from "./spec-refs.ts";
import { FAIL_KINDS, failures, type FailKind, rewrite, rewriteReport } from "./spec-rewrite.ts";

const USAGE = `usage: spec.ts counts [--by chapter|prefix|topic|kind] [--json]
       spec.ts audit [--strict] [--list] [--json]
       spec.ts refs RULE-ID [--json]
       spec.ts refs --dead [--brief] [--all] [--json]
       spec.ts rewrite BASE [HEAD] [--json] [--fail-on lost-codes,lost-examples,reused-ids]
       spec.ts glossary [--markdown] [--json]`;

const VIEWS: readonly CountsView[] = ["chapter", "prefix", "topic", "kind"];

class UsageError extends Error {}

interface Parsed {
  readonly flags: Set<string>;
  readonly values: Map<string, string>;
  readonly positional: string[];
}

function parse(
  args: readonly string[],
  flags: readonly string[],
  valued: readonly string[],
): Parsed {
  const result: Parsed = { flags: new Set(), values: new Map(), positional: [] };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index]!;
    if (valued.includes(arg)) {
      const value = args[index + 1];
      if (value === undefined) throw new UsageError(`${arg} needs a value`);
      result.values.set(arg, value);
      index += 1;
    } else if (flags.includes(arg)) result.flags.add(arg);
    else if (arg.startsWith("--")) throw new UsageError(`unknown option ${arg}`);
    else result.positional.push(arg);
  }
  return result;
}

const json = (value: unknown): string => `${JSON.stringify(value, null, 2)}\n`;

/** Runs one command; returns its output and exit status. */
export function run(
  args: readonly string[],
  roots: { spec: string; repo: string } = { spec: SPEC_ROOT, repo: REPO_ROOT },
): { status: number; stdout: string } {
  const [command, ...rest] = args;
  if (command === "counts") {
    const options = parse(rest, ["--json"], ["--by"]);
    const view = (options.values.get("--by") ?? "chapter") as CountsView;
    if (!VIEWS.includes(view) || options.positional.length > 0)
      throw new UsageError(`unknown counts view ${view}`);
    const result = counts(loadCorpus(roots.spec));
    return {
      status: 0,
      stdout: options.flags.has("--json") ? json(result) : countsReport(result, view),
    };
  }
  if (command === "audit") {
    const options = parse(rest, ["--strict", "--list", "--json"], []);
    if (options.positional.length > 0) throw new UsageError("audit takes no arguments");
    const warnings = audit(loadCorpus(roots.spec));
    const stdout = options.flags.has("--json")
      ? json({ totals: auditTotals(warnings), warnings })
      : auditReport(warnings, options.flags.has("--list"));
    return { status: options.flags.has("--strict") && warnings.length > 0 ? 1 : 0, stdout };
  }
  if (command === "refs") {
    const options = parse(rest, ["--dead", "--brief", "--all", "--json"], []);
    const dead = options.flags.has("--dead");
    if (dead ? options.positional.length !== 0 : options.positional.length !== 1)
      throw new UsageError(dead ? "refs --dead takes no rule ID" : "refs needs one rule ID");
    const index = buildIndex(loadCorpus(roots.spec), roots.repo);
    if (dead) {
      const found = deadCitations(index);
      const stdout = options.flags.has("--json")
        ? json(found)
        : deadReport(found, {
            brief: options.flags.has("--brief"),
            all: options.flags.has("--all"),
          });
      return { status: found.some((citation) => citation.failing) ? 1 : 0, stdout };
    }
    const id = options.positional[0]!.replace(/^#?r-/, "");
    const stdout = options.flags.has("--json")
      ? json({
          id,
          defined: index.live.get(id) ?? null,
          citations: index.citations.filter((c) => c.id === id),
        })
      : refsReport(index, id);
    return { status: 0, stdout };
  }
  if (command === "rewrite") {
    const options = parse(rest, ["--json"], ["--fail-on"]);
    if (options.positional.length < 1 || options.positional.length > 2)
      throw new UsageError("rewrite needs a base revision and at most one head revision");
    const failOn = (options.values.get("--fail-on") ?? "").split(",").filter((kind) => kind !== "");
    for (const kind of failOn)
      if (!(FAIL_KINDS as readonly string[]).includes(kind))
        throw new UsageError(`unknown --fail-on kind ${kind}`);
    const [baseRev, headRev] = options.positional as [string, string | undefined];
    const at = (rev: string) => {
      try {
        return loadCorpusAt(roots.repo, rev);
      } catch {
        throw new UsageError(`git cannot read revision ${rev}`);
      }
    };
    const result = rewrite({
      base: at(baseRev),
      head: headRev === undefined ? loadCorpus(roots.spec) : at(headRev),
      baseLabel: baseRev,
      headLabel: headRev ?? "working tree",
      repoRoot: roots.repo,
      earlierIds: historicalIds(roots.repo, baseRev),
    });
    const tripped = failures(result, failOn as FailKind[]);
    const stdout = options.flags.has("--json")
      ? json({ ...result, failed: tripped })
      : rewriteReport(result) + (tripped.length > 0 ? `Failed: ${tripped.join(", ")}\n` : "");
    return { status: tripped.length > 0 ? 1 : 0, stdout };
  }
  if (command === "glossary") {
    const options = parse(rest, ["--markdown", "--json"], []);
    if (options.positional.length > 0) throw new UsageError("glossary takes no arguments");
    const result = glossary(loadCorpus(roots.spec));
    const stdout = options.flags.has("--json")
      ? json(result)
      : options.flags.has("--markdown")
        ? glossaryMarkdown(result)
        : glossaryReport(result);
    return { status: 0, stdout };
  }
  throw new UsageError(command ? `unknown command ${command}` : "missing command");
}

function main(args: readonly string[]): number {
  try {
    const { status, stdout } = run(args);
    process.stdout.write(stdout);
    return status;
  } catch (error) {
    if (!(error instanceof UsageError)) throw error;
    console.error(`${error.message}\n${USAGE}`);
    return 2;
  }
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) process.exitCode = main(process.argv.slice(2));
