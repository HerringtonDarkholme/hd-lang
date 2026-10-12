// incremental-soundness: an incremental result always equals a clean one.
//
// A seeded random edit script on the generated small package. Each edit is
// one of: a private body edit, a public signature edit, a type error put
// into a function body or taken out again, a broken test expectation or its
// repair, and a new public function. After each edit:
// - the working copy, which keeps its own HD_CACHE and files across the
//   whole script, runs `hd check --tests --format json`;
// - a fresh copy of the same files, with an empty HD_CACHE, runs the same;
// - every 4th edit and the last, both also run `hd test --format json`.
// The runs match when they exit alike and report the same diagnostics
// (code, severity, file, line, column, message) and the same test outcomes,
// in any order. Paths are compared relative to each copy.
// An hd keeps an incremental mode when its check writes a compilation
// cache (lib/capability.ts findCompileCache). Without one, both runs are
// clean runs: the line is n/a ("no incremental mode detected"), but a short
// script of SAMPLE_EDITS still runs, so the script itself is exercised; a
// mismatch then still fails, since it shows nondeterminism.
// Target (gate): 100% of the edits match.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { findCompileCache } from "../lib/capability.ts";
import { copyPackage, freshCache, materialize } from "../lib/fixture.ts";
import { editBody, editSignature, makeRandom, type GeneratedPackage } from "../lib/gen.ts";
import { jsonLines, runHd, type HdCommand } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import { failed, judge, type Metric } from "../lib/metric.ts";
import { removeTempDir } from "../lib/tmp.ts";

const NAME = "incremental-soundness";
const LABEL = "incremental equals clean, after each edit";
const TARGET = "≥ 100.0%";
const EDITS = 16;
const SAMPLE_EDITS = 4;
const TEST_EVERY = 4;
const TIMEOUT_MS = 120_000;

export type EditKind =
  | "body"
  | "signature"
  | "break"
  | "repair"
  | "break-test"
  | "repair-test"
  | "add";

export interface EditState {
  readonly signed: Set<number>;
  readonly broken: Set<number>;
  readonly brokenTests: Set<number>;
  added: number;
}

export const newEditState = (): EditState => ({
  signed: new Set(),
  broken: new Set(),
  brokenTests: new Set(),
  added: 0,
});

/**
 * Applies one edit to module `k`'s text and returns the new text, or
 * undefined when that kind does not apply to the module now, as a repair
 * of a module that is not broken.
 */
export function applyEdit(
  kind: EditKind,
  k: number,
  text: string,
  state: EditState,
): string | undefined {
  switch (kind) {
    case "body":
      return editBody(text);
    case "signature":
      if (state.signed.has(k)) return undefined;
      state.signed.add(k);
      return editSignature(text);
    case "break": {
      if (state.broken.has(k)) return undefined;
      state.broken.add(k);
      return replaceOnce(text, /^ {4}x \+ bump$/m, "    x + bump + true");
    }
    case "repair":
      if (!state.broken.delete(k)) return undefined;
      return replaceOnce(text, /^ {4}x \+ bump \+ true$/m, "    x + bump");
    case "break-test":
      if (state.brokenTests.has(k)) return undefined;
      state.brokenTests.add(k);
      return replaceOnce(text, /(assert_equal\(total\d+\(items\), )6,/, "$17,");
    case "repair-test":
      if (!state.brokenTests.delete(k)) return undefined;
      return replaceOnce(text, /(assert_equal\(total\d+\(items\), )7,/, "$16,");
    case "add":
      state.added += 1;
      return `${text.trimEnd()}\n\n## An added function.\npub fn added${k}_${state.added}(x: i32) -> i32:\n    x + ${state.added}\n`;
  }
}

function replaceOnce(text: string, pattern: RegExp, replacement: string): string {
  const next = text.replace(pattern, replacement);
  if (next === text) throw new Error(`no edit site for ${String(pattern)}`);
  return next;
}

/**
 * The comparable view of a `--format json` run: its exit status, and its
 * diagnostics and test outcomes as sorted strings, with `dir` cut from
 * every path.
 */
export function outcome(status: number | null, stdout: string, dir: string): string[] {
  const clean = (value: unknown) => String(value ?? "").replaceAll(dir, "<pkg>");
  const lines = jsonLines(stdout).flatMap((record) => {
    if (record.kind === "diagnostic")
      return [
        [
          "diagnostic",
          record.code,
          record.severity,
          record.file,
          record.line,
          record.column,
          record.message,
        ]
          .map(clean)
          .join(" | "),
      ];
    if (record.kind === "test")
      return [["test", record.name, record.outcome].map(clean).join(" | ")];
    return [];
  });
  return [`status ${String(status)}`, ...lines.sort()];
}

/** The first difference between two outcomes, or undefined when they match. */
export function difference(
  incremental: readonly string[],
  clean: readonly string[],
): string | undefined {
  const only = (a: readonly string[], b: readonly string[]) =>
    a.filter((line) => !b.includes(line));
  const extra = only(incremental, clean);
  const missing = only(clean, incremental);
  if (extra.length === 0 && missing.length === 0) return undefined;
  return [
    extra.length ? `incremental only: ${extra[0]}` : "",
    missing.length ? `clean only: ${missing[0]}` : "",
  ]
    .filter(Boolean)
    .join("; ");
}

const KINDS: readonly EditKind[] = [
  "body",
  "body",
  "signature",
  "break",
  "repair",
  "break-test",
  "repair-test",
  "add",
];

async function compare(
  hd: HdCommand,
  args: readonly string[],
  working: { readonly dir: string; readonly env: Record<string, string> },
): Promise<string | undefined> {
  const fresh = copyPackage(working.dir, "clean");
  try {
    const run = (dir: string, env: Record<string, string>) =>
      runHd(hd, args, { cwd: dir, env, timeoutMs: TIMEOUT_MS });
    const incremental = await run(working.dir, working.env);
    const clean = await run(fresh, freshCache());
    if (incremental.timedOut || clean.timedOut) return `hd ${args.join(" ")} timed out`;
    return difference(
      outcome(incremental.status, incremental.stdout, working.dir),
      outcome(clean.status, clean.stdout, fresh),
    );
  } finally {
    removeTempDir(fresh);
  }
}

/** Runs an edit script of `edits` edits; returns the matches and the first mismatch. */
async function editScript(
  context: MetricContext,
  edits: number,
): Promise<{ readonly matched: number; readonly total: number; readonly mismatch?: string }> {
  const { dir, pkg } = materialize("small", NAME);
  const working = { dir, env: freshCache() };
  const random = makeRandom(`${NAME}-script`);
  const state = newEditState();
  const modules = (pkg as GeneratedPackage).modules;
  let matched = 0;
  let total = 0;
  let mismatch: string | undefined;
  // A check before any edit, so an incremental hd starts from a warm cache.
  await runHd(context.hd, ["check", "--tests"], {
    cwd: dir,
    env: working.env,
    timeoutMs: TIMEOUT_MS,
  });
  for (let step = 1; step <= edits; step++) {
    let applied: string | undefined;
    while (applied === undefined) {
      const kind = KINDS[Math.floor(random() * KINDS.length)]!;
      const k = Math.floor(random() * modules.length);
      const file = join(dir, modules[k]!.file);
      const next = applyEdit(kind, k, readFileSync(file, "utf8"), state);
      if (next === undefined) continue;
      writeFileSync(file, next);
      applied = `${kind} ${modules[k]!.name}`;
    }
    const runs = [["check", "--tests", "--format", "json"]];
    if (step % TEST_EVERY === 0 || step === edits) runs.push(["test", "--format", "json"]);
    for (const args of runs) {
      total += 1;
      const problem = await compare(context.hd, args, working);
      if (problem === undefined) matched += 1;
      else mismatch ??= `edit ${step} (${applied}), hd ${args[0]}: ${problem}`;
    }
  }
  return { matched, total, ...(mismatch === undefined ? {} : { mismatch }) };
}

export const incrementalSoundness: Metric = {
  name: NAME,
  pillar: "gate",
  summary: "random edit scripts: the incremental check and test results equal a clean run",
  async run(context) {
    const cache = await findCompileCache(context.hd);
    if (typeof cache === "string") return [failed(NAME, LABEL, TARGET, cache)];
    const incremental = cache !== undefined;
    const edits = incremental ? EDITS : SAMPLE_EDITS;
    context.log(
      `${NAME}: ${edits} random edits${incremental ? "" : " (no incremental mode: a sample)"}`,
    );
    try {
      const result = await editScript(context, edits);
      const note = `${result.matched} of ${result.total} runs matched over ${edits} edits${
        result.mismatch ? `; first mismatch: ${result.mismatch}` : ""
      }`;
      const judged = judge(NAME, LABEL, result.matched / result.total, 1, "%", "at-least", note);
      if (incremental || result.mismatch) return [judged];
      return [
        { ...judged, status: "n/a", note: `no incremental mode detected; the sample: ${note}` },
      ];
    } catch (error) {
      return [failed(NAME, LABEL, TARGET, error instanceof Error ? error.message : String(error))];
    }
  },
};
