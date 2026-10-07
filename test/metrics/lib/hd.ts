// Running the `hd` under test: command parsing, timing, CPU time and peak RSS.
//
// The implementation under test is a command prefix, such as `hd` or
// `node --experimental-strip-types bin/hd.js`. Nothing here imports the
// prototype; every measurement goes through a child process.

import { execFile, spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";
import { platform } from "node:process";
import { performance } from "node:perf_hooks";

import { makeTempDir } from "./tmp.ts";

export interface HdCommand {
  /** The program and the leading arguments; the action and its flags follow. */
  readonly argv: readonly string[];
  /** The command as the user wrote it, for reports. */
  readonly display: string;
}

/** Splits a command line on spaces, honoring single and double quotes. */
export function splitCommand(text: string): string[] {
  const words: string[] = [];
  let current = "";
  let quote: string | null = null;
  let started = false;
  for (const char of text) {
    if (quote) {
      if (char === quote) quote = null;
      else current += char;
    } else if (char === "'" || char === '"') {
      quote = char;
      started = true;
    } else if (/\s/.test(char)) {
      if (started) words.push(current);
      current = "";
      started = false;
    } else {
      current += char;
      started = true;
    }
  }
  if (quote) throw new Error(`unclosed ${quote} in the hd command: ${text}`);
  if (started) words.push(current);
  return words;
}

/**
 * Parses the `--hd` value. A word that names an existing file relative to
 * `base` becomes absolute, so the command still works when a script runs it
 * from a temporary directory.
 */
export function parseHdCommand(text: string, base = process.cwd()): HdCommand {
  const words = splitCommand(text);
  if (words.length === 0) throw new Error("the hd command is empty");
  const argv = words.map((word) =>
    !isAbsolute(word) && !word.startsWith("-") && existsSync(join(base, word))
      ? resolve(base, word)
      : word,
  );
  return { argv, display: text };
}

export interface RunOptions {
  readonly cwd: string;
  /** Kill the run after this long; the result then has `timedOut`. Default 60 s. */
  readonly timeoutMs?: number;
  /** Measure CPU time and peak RSS with `/usr/bin/time`. Default false. */
  readonly measure?: boolean;
  /** Extra environment variables, such as `HD_CACHE`. */
  readonly env?: Readonly<Record<string, string>>;
  /** Text written to the child's standard input. */
  readonly input?: string;
}

export interface RunResult {
  /** The exit status; null when a signal ended the run. */
  readonly status: number | null;
  readonly stdout: string;
  readonly stderr: string;
  readonly wallMs: number;
  /** Wall time from the start to the first byte of standard output; undefined with no output. */
  readonly firstOutputMs?: number;
  /** User plus system CPU time; undefined when not measured. */
  readonly cpuMs?: number;
  /** Peak resident set size in bytes; undefined when not measured. */
  readonly rssBytes?: number;
  readonly timedOut: boolean;
}

const TIME = "/usr/bin/time";

/** Process groups of the runs still alive, so an interrupted runner can end them. */
const live = new Set<number>();

/** Kills every run this process started that is still alive. */
export function killLiveRuns(): void {
  for (const pid of live) {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {
      // Already gone.
    }
  }
  live.clear();
}

/** Whether `/usr/bin/time` can report CPU time and peak RSS on this host. */
export const canMeasure = (): boolean =>
  existsSync(TIME) && (platform === "darwin" || platform === "linux");

/** Parses the report of `/usr/bin/time -l` (macOS) or `-v` (GNU, Linux). */
export function parseTimeReport(
  text: string,
): { readonly cpuMs: number; readonly rssBytes: number } | undefined {
  const bsd = /([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys/.exec(text);
  const bsdRss = /(\d+)\s+maximum resident set size/.exec(text);
  if (bsd && bsdRss)
    return {
      cpuMs: (Number(bsd[2]) + Number(bsd[3])) * 1000,
      rssBytes: Number(bsdRss[1]),
    };
  const user = /User time \(seconds\):\s*([\d.]+)/.exec(text);
  const sys = /System time \(seconds\):\s*([\d.]+)/.exec(text);
  const gnuRss = /Maximum resident set size \(kbytes\):\s*(\d+)/.exec(text);
  if (user && sys && gnuRss)
    return {
      cpuMs: (Number(user[1]) + Number(sys[1])) * 1000,
      rssBytes: Number(gnuRss[1]) * 1024,
    };
  return undefined;
}

/** Runs `argv` as a child process in its own process group. */
export function runProcess(argv: readonly string[], options: RunOptions): Promise<RunResult> {
  const timeoutMs = options.timeoutMs ?? 60_000;
  const reportDir = options.measure && canMeasure() ? makeTempDir("time") : undefined;
  const reportFile = reportDir ? join(reportDir, "time.txt") : undefined;
  const full = reportFile
    ? [TIME, platform === "darwin" ? "-l" : "-v", "-o", reportFile, ...argv]
    : [...argv];
  return new Promise((done, fail) => {
    const start = performance.now();
    const child = spawn(full[0]!, full.slice(1), {
      cwd: options.cwd,
      env: { ...process.env, NO_COLOR: "1", ...options.env },
      detached: true,
      stdio: ["pipe", "pipe", "pipe"],
    });
    if (child.pid !== undefined) live.add(child.pid);
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    let firstOutputMs: number | undefined;
    child.stdout.on("data", (chunk: Buffer) => {
      firstOutputMs ??= performance.now() - start;
      out.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => err.push(chunk));
    child.stdin.on("error", () => {});
    child.stdin.end(options.input ?? "");
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      // The group holds only processes this run started: the child and,
      // when measuring, the hd under `/usr/bin/time`.
      try {
        process.kill(-child.pid!, "SIGKILL");
      } catch {
        child.kill("SIGKILL");
      }
    }, timeoutMs);
    child.on("error", (error) => {
      clearTimeout(timer);
      fail(error);
    });
    child.on("close", (status) => {
      clearTimeout(timer);
      if (child.pid !== undefined) live.delete(child.pid);
      const wallMs = performance.now() - start;
      let usage: ReturnType<typeof parseTimeReport>;
      if (reportFile && existsSync(reportFile))
        usage = parseTimeReport(readFileSync(reportFile, "utf8"));
      if (reportDir) rmSync(reportDir, { recursive: true, force: true });
      done({
        status,
        stdout: Buffer.concat(out).toString("utf8"),
        stderr: Buffer.concat(err).toString("utf8"),
        wallMs,
        ...(firstOutputMs === undefined ? {} : { firstOutputMs }),
        ...usage,
        timedOut,
      });
    });
  });
}

/** Runs one `hd` action, as `hd check --format json`, with `args` after the prefix. */
export const runHd = (
  hd: HdCommand,
  args: readonly string[],
  options: RunOptions,
): Promise<RunResult> => runProcess([...hd.argv, ...args], options);

/** Whether this `hd` has COMMAND: `hd help COMMAND` succeeds (cli.command.help.command). */
export async function supportsCommand(hd: HdCommand, command: string, cwd: string) {
  const result = await runHd(hd, ["help", command], { cwd, timeoutMs: 30_000 });
  return result.status === 0;
}

/** Whether a command takes FLAG: its `hd help COMMAND` text names it. */
export async function supportsFlag(hd: HdCommand, command: string, flag: string, cwd: string) {
  const result = await runHd(hd, ["help", command], { cwd, timeoutMs: 30_000 });
  return result.status === 0 && new RegExp(`(^|\\s)${flag}(\\s|=|,|$)`, "m").test(result.stdout);
}

/** The JSON lines of a `--format json` run; non-JSON lines are skipped. */
export function jsonLines(text: string): Record<string, unknown>[] {
  const records: Record<string, unknown>[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("{")) continue;
    try {
      const value: unknown = JSON.parse(trimmed);
      if (value && typeof value === "object" && !Array.isArray(value))
        records.push(value as Record<string, unknown>);
    } catch {
      // Not a JSON object line: some other output.
    }
  }
  return records;
}

/** The diagnostic objects of a `--format json` run (cli.json.kind). */
export const diagnosticsOf = (text: string): Record<string, unknown>[] =>
  jsonLines(text).filter((record) => record.kind === "diagnostic");

/** The summary object of a `--format json` run, the last one (cli.json.summary.result). */
export const summaryOf = (text: string): Record<string, unknown> | undefined =>
  jsonLines(text)
    .filter((record) => record.kind === "summary")
    .at(-1);

/**
 * The summed resident set size, in bytes, of the processes in group
 * `pgid`, from the output of `ps -A -o pgid=,rss=` (kilobytes, on macOS and
 * Linux alike). Undefined when no process of the group is listed.
 */
export function parsePsRss(text: string, pgid: number): number | undefined {
  let total: number | undefined;
  for (const line of text.split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s*$/.exec(line);
    if (match && Number(match[1]) === pgid) total = (total ?? 0) + Number(match[2]) * 1024;
  }
  return total;
}

/** The current resident set size of process group `pgid`, by `ps`; undefined when unknown. */
export function groupRssBytes(pgid: number): Promise<number | undefined> {
  return new Promise((done) => {
    execFile("ps", ["-A", "-o", "pgid=,rss="], { timeout: 10_000 }, (error, stdout) =>
      done(error ? undefined : parsePsRss(stdout, pgid)),
    );
  });
}

export interface SampledRun {
  /** The resident set size of the process group at each sample, with its time in ms. */
  readonly samples: readonly { readonly atMs: number; readonly rssBytes: number | undefined }[];
  readonly stdout: string;
  readonly stderr: string;
  /** The exit status when the process ended before the duration; undefined when it was stopped. */
  readonly exited?: number | null;
}

/**
 * Runs a long-lived process for `durationMs`, samples the RSS of its
 * process group every `intervalMs` with `ps`, then kills the group. A
 * process that exits early ends the run, with `exited` set.
 */
export function runSampled(
  argv: readonly string[],
  options: {
    readonly cwd: string;
    readonly durationMs: number;
    readonly intervalMs: number;
    readonly env?: Readonly<Record<string, string>>;
  },
): Promise<SampledRun> {
  return new Promise((done, fail) => {
    const start = performance.now();
    const child = spawn(argv[0]!, argv.slice(1), {
      cwd: options.cwd,
      env: { ...process.env, NO_COLOR: "1", ...options.env },
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const pid = child.pid;
    if (pid !== undefined) live.add(pid);
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => (stdout += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => (stderr += chunk));
    const samples: { atMs: number; rssBytes: number | undefined }[] = [];
    let stopped = false;
    let sampling = false;
    const sampler = setInterval(() => {
      if (sampling || pid === undefined) return;
      sampling = true;
      const atMs = performance.now() - start;
      void groupRssBytes(pid).then((rssBytes) => {
        sampling = false;
        if (!stopped) samples.push({ atMs, rssBytes });
      });
    }, options.intervalMs);
    const stop = setTimeout(() => {
      stopped = true;
      try {
        process.kill(-pid!, "SIGKILL");
      } catch {
        child.kill("SIGKILL");
      }
    }, options.durationMs);
    child.on("error", (error) => {
      clearInterval(sampler);
      clearTimeout(stop);
      fail(error);
    });
    child.on("close", (status) => {
      clearInterval(sampler);
      clearTimeout(stop);
      if (pid !== undefined) live.delete(pid);
      const early = !stopped;
      stopped = true;
      done({ samples, stdout, stderr, ...(early ? { exited: status } : {}) });
    });
  });
}

export interface SessionStep {
  /** Text written to the session's standard input. */
  readonly input: string;
  /** Holds once the session's standard output shows that the input was handled. */
  readonly done: (stdout: string) => boolean;
}

export interface SessionResult {
  /** The resident set size of the session's process group after each completed step. */
  readonly rssBytes: readonly (number | undefined)[];
  /** Why the session stopped early: a timeout, or an exit before a step was done. */
  readonly problem?: string;
  readonly stdout: string;
  readonly stderr: string;
}

/**
 * Runs one long-lived process, such as a REPL, fed in steps. Each step's
 * input is written; the run then waits until the step's `done` holds for the
 * standard output so far, and samples the resident set size of the process
 * group. Standard input closes after the last step. The whole session shares
 * one timeout, and waiting is woken by output, not by polling.
 */
export function runSession(
  argv: readonly string[],
  options: {
    readonly cwd: string;
    readonly env?: Readonly<Record<string, string>>;
    readonly timeoutMs: number;
    readonly steps: readonly SessionStep[];
  },
): Promise<SessionResult> {
  const child = spawn(argv[0]!, argv.slice(1), {
    cwd: options.cwd,
    env: { ...process.env, NO_COLOR: "1", ...options.env },
    detached: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const pid = child.pid;
  if (pid !== undefined) live.add(pid);
  let stdout = "";
  let stderr = "";
  let exit: number | null | undefined;
  let wake: (() => void) | undefined;
  child.stdout.setEncoding("utf8").on("data", (chunk: string) => {
    stdout += chunk;
    wake?.();
  });
  child.stderr.setEncoding("utf8").on("data", (chunk: string) => (stderr += chunk));
  child.stdin.on("error", () => {});
  const closed = new Promise<void>((done) => {
    child.on("error", () => {
      exit ??= null;
      wake?.();
      done();
    });
    child.on("close", (status) => {
      exit = status;
      if (pid !== undefined) live.delete(pid);
      wake?.();
      done();
    });
  });
  const started = performance.now();
  const deadline = started + options.timeoutMs;
  const waitFor = async (test: () => boolean): Promise<boolean> => {
    while (!test() && exit === undefined) {
      const left = deadline - performance.now();
      if (left <= 0) return false;
      await new Promise<void>((done) => {
        const timer = setTimeout(done, left);
        wake = () => {
          clearTimeout(timer);
          done();
        };
      });
      wake = undefined;
    }
    return test();
  };
  const kill = (): void => {
    try {
      process.kill(-pid!, "SIGKILL");
    } catch {
      child.kill("SIGKILL");
    }
  };
  return (async () => {
    const rssBytes: (number | undefined)[] = [];
    let problem = pid === undefined ? "the session did not start" : undefined;
    for (const [index, step] of options.steps.entries()) {
      if (problem) break;
      child.stdin.write(step.input);
      if (await waitFor(() => step.done(stdout))) {
        rssBytes.push(await groupRssBytes(pid!));
        continue;
      }
      const seconds = ((performance.now() - started) / 1000).toFixed(0);
      const where = `step ${index + 1} of ${options.steps.length}`;
      problem =
        exit === undefined
          ? `timed out after ${seconds} s at ${where}`
          : `exited ${String(exit)} before ${where} was done`;
    }
    child.stdin.end();
    if (exit === undefined) {
      const left = problem ? 0 : Math.max(deadline - performance.now(), 1000);
      const timer = setTimeout(kill, left);
      await closed;
      clearTimeout(timer);
    }
    return { rssBytes, ...(problem === undefined ? {} : { problem }), stdout, stderr };
  })();
}
