// Shared plumbing for the implementation-neutral fuzzer.
// Allowed imports: Node built-ins and `spec/` only (enforced by check-imports.ts).
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

export const repoRoot = resolve(import.meta.dirname, "../..");
export const specRoot = resolve(repoRoot, "spec");

export type Action = "build" | "check" | "parse" | "run" | "test";

export interface CommandResult {
  readonly code: number | null;
  readonly ms: number;
  readonly signal: string | null;
  readonly stderr: string;
  readonly stdout: string;
  readonly timedOut: boolean;
}

export type OutcomeKind = "accept" | "panic" | "reject" | "violation";

export interface Outcome {
  /** First diagnostic or panic code; for violations, the violation kind. */
  readonly code: string;
  /** Free-form detail: uninventoried code, normalized first output line, etc. */
  readonly detail: string;
  readonly kind: OutcomeKind;
  readonly ms: number;
  /** A located code outside the spec inventory (check/run/test only). */
  readonly unknownCode?: string;
}

export function splitCommand(value: string): string[] {
  const parts: string[] = [];
  const pattern = /"([^"]*)"|'([^']*)'|([^\s]+)/g;
  for (const match of value.matchAll(pattern)) parts.push(match[1] ?? match[2] ?? match[3]!);
  return parts;
}

export function invoke(
  command: readonly string[],
  action: Action,
  path: string,
  options: readonly string[],
  timeoutMs: number,
  cwd: string = repoRoot,
): Promise<CommandResult> {
  const started = performance.now();
  return new Promise((complete) => {
    const child = spawn(command[0]!, [...command.slice(1), action, ...options, path], {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => {
      if (stdout.length < 200_000) stdout += chunk;
    });
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => {
      if (stderr.length < 200_000) stderr += chunk;
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      complete({
        code: 127,
        ms: performance.now() - started,
        signal: null,
        stderr: String(error),
        stdout,
        timedOut,
      });
    });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      complete({ code, ms: performance.now() - started, signal, stderr, stdout, timedOut });
    });
  });
}

export async function mapParallel<T, U>(
  values: readonly T[],
  jobs: number,
  operation: (value: T, index: number) => Promise<U>,
): Promise<U[]> {
  const results: U[] = [];
  let next = 0;
  async function worker(): Promise<void> {
    while (next < values.length) {
      const index = next;
      next += 1;
      results[index] = await operation(values[index]!, index);
    }
  }
  await Promise.all(Array.from({ length: Math.min(jobs, values.length) }, worker));
  return results;
}

/** Deterministic PRNG (mulberry32) seeded from an arbitrary string. */
export class Rng {
  private state: number;

  constructor(seed: string) {
    const digest = createHash("sha256").update(seed).digest();
    this.state = digest.readUInt32LE(0) || 1;
  }

  next(): number {
    this.state = (this.state + 0x6d2b79f5) | 0;
    let value = this.state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  }

  int(limit: number): number {
    return Math.floor(this.next() * limit);
  }

  pick<T>(values: readonly T[]): T {
    return values[this.int(values.length)]!;
  }

  chance(probability: number): boolean {
    return this.next() < probability;
  }
}

export function hash(text: string): string {
  return createHash("sha256").update(text).digest("hex").slice(0, 12);
}

export interface Inventory {
  /** Error, warning, and boundary codes from the normative table in spec/README.md. */
  readonly diagnostics: ReadonlySet<string>;
  /** Stable panic categories from spec/06-control-flow.md. */
  readonly panics: ReadonlySet<string>;
  /** Codes the reference parser can emit (read from its source text, not imported). */
  readonly referenceParser: ReadonlySet<string>;
  readonly warnings: ReadonlySet<string>;
}

function backticked(text: string): string[] {
  return [...text.matchAll(/`([a-z0-9-]+)`/g)].map((match) => match[1]!);
}

export function loadInventory(): Inventory {
  const readme = readFileSync(resolve(specRoot, "README.md"), "utf8");
  const diagnostics = new Set<string>();
  const warnings = new Set<string>();
  for (const line of readme.split("\n")) {
    const row = /^\| (Error|Warning|Boundary failure) \| (.*) \|$/.exec(line);
    if (!row) continue;
    for (const code of backticked(row[2]!)) {
      diagnostics.add(code);
      if (row[1] === "Warning") warnings.add(code);
    }
  }
  const control = readFileSync(resolve(specRoot, "06-control-flow.md"), "utf8");
  const sentence = /Stable panic categories are exactly([\s\S]*?)\.\s/.exec(control);
  if (!sentence) throw new Error("spec/06-control-flow.md: panic category sentence not found");
  const panics = new Set(backticked(sentence[1]!));
  const referenceParser = new Set<string>();
  const parserDirectory = resolve(specRoot, "reference-parser");
  for (const name of readdirSync(parserDirectory)) {
    if (!name.endsWith(".ts")) continue;
    const text = readFileSync(resolve(parserDirectory, name), "utf8");
    for (const match of text.matchAll(/(?:diagnostic\(|code: )"([a-z0-9-]+)"/g))
      referenceParser.add(match[1]!);
  }
  if (diagnostics.size < 50 || panics.size < 10)
    throw new Error("spec inventory looks truncated; check spec/README.md and chapter 06");
  return { diagnostics, panics, referenceParser, warnings };
}

export function specFiles(directory: string, extension = ".hd"): string[] {
  const result: string[] = [];
  for (const name of readdirSync(directory).sort()) {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) result.push(...specFiles(path, extension));
    else if (name.endsWith(extension)) result.push(path);
  }
  return result;
}

function escapeRegExp(text: string): string {
  return text.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Normalizes a free-text line into a stable, path- and number-free label. */
export function normalizeLine(line: string, path: string): string {
  return line
    .replaceAll(path, "FILE")
    .replaceAll(/(?:file:\/\/)?(?<![\w.])\/[^\s:)]+/g, "PATH")
    .replaceAll(/\d+/g, "N")
    .replaceAll(/'[^']*'/g, "'_'")
    .trim()
    .slice(0, 100);
}

/**
 * A grouping label for output that carries no located code. It is only used to
 * split signatures, never to decide a verdict. Preference: the first line that
 * reads like an error message ("Something...: text"), else the first line that
 * does not look like a file location or source excerpt.
 */
export function messageLabel(output: string, path: string): string {
  const lines = output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const message = lines.find((line) =>
    /^(?:[\p{L}][\w.' -]*?)?(?:error|exception|panic\w*|fatal|abort\w*)\s*:/iu.test(line),
  );
  const plain = lines.find((line) => !/(?:^|\s)(?:file:\/\/)?\/|^\^+$|^at\s/.test(line));
  return normalizeLine(message ?? plain ?? lines[0] ?? "", path);
}

/**
 * Classifies one command result under the command contract (audit/fuzz/CONTRACT.md).
 * Only exit status, signals, and `PATH:LINE:COL: CODE:` / `CODE:` lines are read.
 */
export function classify(
  result: CommandResult,
  action: Action,
  path: string,
  inventory: Inventory,
): Outcome {
  const ms = result.ms;
  const output = `${result.stdout}\n${result.stderr}`;
  if (result.timedOut) return { code: "timeout", detail: "", kind: "violation", ms };
  if (result.signal) return { code: "signal", detail: result.signal, kind: "violation", ms };
  if (result.code === 0) return { code: "", detail: "", kind: "accept", ms };
  const label = messageLabel(output, path);
  if (result.code !== 1)
    return { code: "bad-exit", detail: `exit ${result.code}: ${label}`, kind: "violation", ms };
  const base = path.split("/").at(-1)!;
  const located = new RegExp(
    `^(?:\\S*?${escapeRegExp(base)}):(\\d+):(\\d+): (warning: )?([a-z0-9]+(?:-[a-z0-9]+)*):`,
    "m",
  );
  const lines = output.split("\n");
  const errors: string[] = [];
  for (const line of lines) {
    const match = located.exec(line);
    if (match && !match[3]) errors.push(match[4]!);
  }
  if (action === "run" || action === "test") {
    for (const line of lines) {
      const panic = /^(?:\S+:\d+:\d+: )?([a-z0-9]+(?:-[a-z0-9]+)*): /.exec(line.trim());
      if (panic && inventory.panics.has(panic[1]!) && !errors.includes(panic[1]!))
        return { code: panic[1]!, detail: "", kind: "panic", ms };
    }
  }
  if (errors.length === 0) return { code: "no-located-code", detail: label, kind: "violation", ms };
  const unknownCode =
    action === "parse" || action === "build"
      ? undefined
      : errors.find(
          (code) => !inventory.diagnostics.has(code) && !inventory.referenceParser.has(code),
        );
  return { code: errors[0]!, detail: errors.join(","), kind: "reject", ms, unknownCode };
}

export function outcomeLabel(outcome: Outcome): string {
  if (outcome.kind === "accept") return "accept";
  return `${outcome.kind}:${outcome.code}`;
}
