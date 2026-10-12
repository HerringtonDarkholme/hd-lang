// One run of the mistake corpus, shared by `mistakes`, `diag-location` and
// `fixit-safety`, so a full metrics run checks each program once.
//
// For each program, in its own temporary directory:
// 1. `hd check --tests --format json FILE` gives the diagnostic objects;
// 2. `hd check --tests FILE` gives the text an agent reads by default;
// 3. when the diagnostic for the mistake carries one fix-it, the fix is
//    applied to a copy and step 1 runs again on it.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { loadMistakes, type Mistake } from "./corpus.ts";
import { fixOf, applyEdits, hasFixField } from "./fixit.ts";
import { diagnosticsOf, runHd, type HdCommand } from "./hd.ts";
import type { MetricContext } from "./metric.ts";
import { withTempDir } from "./tmp.ts";

export interface MistakeOutcome {
  readonly mistake: Mistake;
  /** Diagnostic objects, warnings included. */
  readonly diagnostics: readonly Record<string, unknown>[];
  /** stdout and stderr of the text-format check. */
  readonly text: string;
  /** The diagnostic judged to report the mistake: the expected code's, else the first. */
  readonly reported?: Record<string, unknown>;
  /** Whether any diagnostic object had a `fix` or `fixes` field. */
  readonly fixFieldSeen: boolean;
  /** Whether the reported diagnostic had one fix-it to apply. */
  readonly fixApplied: boolean;
  /** Diagnostics after applying the fix-it; undefined when none was applied. */
  readonly afterFix?: readonly Record<string, unknown>[];
  /** Why a run could not complete, such as a timeout. */
  readonly problem?: string;
}

const TIMEOUT_MS = 30_000;
const POOL = 4;

const cache = new Map<string, Promise<MistakeOutcome[]>>();

async function checkJson(hd: HdCommand, dir: string, file: string) {
  const result = await runHd(hd, ["check", "--tests", "--format", "json", file], {
    cwd: dir,
    timeoutMs: TIMEOUT_MS,
  });
  if (result.timedOut) throw new Error(`hd check timed out after ${TIMEOUT_MS / 1000} s`);
  return diagnosticsOf(result.stdout + result.stderr);
}

async function runOne(hd: HdCommand, mistake: Mistake): Promise<MistakeOutcome> {
  return withTempDir("mistake", async (dir) => {
    const file = `${mistake.name}.hd`;
    writeFileSync(join(dir, file), mistake.text);
    try {
      const diagnostics = await checkJson(hd, dir, file);
      const textRun = await runHd(hd, ["check", "--tests", file], {
        cwd: dir,
        timeoutMs: TIMEOUT_MS,
      });
      const reported = diagnostics.find((d) => d.code === mistake.code) ?? diagnostics[0];
      const fixFieldSeen = diagnostics.some(hasFixField);
      const edits = reported ? fixOf(reported) : undefined;
      const base = {
        mistake,
        diagnostics,
        text: textRun.stdout + textRun.stderr,
        ...(reported ? { reported } : {}),
        fixFieldSeen,
      };
      if (!edits || edits.length === 0) return { ...base, fixApplied: false };
      writeFileSync(join(dir, file), applyEdits(mistake.text, edits));
      return { ...base, fixApplied: true, afterFix: await checkJson(hd, dir, file) };
    } catch (error) {
      return {
        mistake,
        diagnostics: [],
        text: "",
        fixFieldSeen: false,
        fixApplied: false,
        problem: error instanceof Error ? error.message : String(error),
      };
    }
  });
}

async function runAll(context: MetricContext): Promise<MistakeOutcome[]> {
  const corpus = loadMistakes(join(context.repoRoot, "test", "metrics", "mistakes"));
  context.log(`mistake corpus: checking ${corpus.length} programs`);
  const outcomes: MistakeOutcome[] = Array.from({ length: corpus.length });
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < corpus.length) {
      const index = next++;
      outcomes[index] = await runOne(context.hd, corpus[index]!);
    }
  };
  await Promise.all(Array.from({ length: POOL }, worker));
  return outcomes;
}

/** The corpus outcomes for this `hd`, computed once per runner process. */
export function mistakeOutcomes(context: MetricContext): Promise<MistakeOutcome[]> {
  let found = cache.get(context.hd.display);
  if (!found) {
    found = runAll(context);
    cache.set(context.hd.display, found);
  }
  return found;
}

/** Error diagnostics only. */
export const errorsOf = (diagnostics: readonly Record<string, unknown>[]) =>
  diagnostics.filter((d) => d.severity !== "warning");
