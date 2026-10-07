// File-access tracing for io-per-check: `strace` on Linux, run as the
// current user. macOS's `fs_usage` and `dtruss` need root, so they are never
// used, and the metric is n/a there.

import { spawnSync } from "node:child_process";
import { platform } from "node:process";

export interface FileAccess {
  /** Paths opened for reading, by a call that did not fail. */
  readonly reads: ReadonlySet<string>;
  /** Paths stat'ed, checked or opened in any other way, including failed calls. */
  readonly stats: ReadonlySet<string>;
}

const OPENS = new Set(["open", "openat", "openat2", "creat"]);

/**
 * Parses an `strace -f -e trace=%file` log: one call per line, maybe after
 * a pid. A call's first quoted argument is its path; an empty path, as
 * `fstatat(3, "", ..., AT_EMPTY_PATH)`, names no file.
 */
export function parseStrace(text: string): FileAccess {
  const reads = new Set<string>();
  const stats = new Set<string>();
  for (const line of text.split("\n")) {
    const match =
      /^(?:\[pid\s+\d+\]\s*|\d+\s+)?(\w+)\((?:AT_FDCWD|-?\d+)?(?:,\s*)?"((?:[^"\\]|\\.)*)"(.*)$/.exec(
        line,
      );
    if (!match || match[2] === "") continue;
    const [, call, path, rest] = match as unknown as [string, string, string, string];
    const failedCall = /\)\s*=\s*-1\b/.test(rest);
    if (OPENS.has(call) && !/O_WRONLY/.test(rest) && call !== "creat" && !failedCall)
      reads.add(path);
    else stats.add(path);
  }
  return { reads, stats };
}

/**
 * The argv prefix that traces a command's file calls into `output`, or the
 * reason tracing is not possible here without elevated privileges.
 */
export function fileTracer(output: string): readonly string[] | string {
  if (platform === "darwin") return "fs_usage and dtruss need root on macOS";
  if (platform !== "linux") return `no file tracer on ${platform}`;
  const argv = ["strace", "-f", "-qq", "-e", "trace=%file", "-e", "signal=none", "-o"];
  const probe = spawnSync(argv[0]!, [...argv.slice(1), "/dev/null", "--", "true"], {
    encoding: "utf8",
  });
  if (probe.error) return "no strace on PATH";
  if (probe.status !== 0) return `strace cannot trace here: ${probe.stderr.trim().slice(0, 200)}`;
  return [...argv, output, "--"];
}
