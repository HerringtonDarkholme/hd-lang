// Runs measure.ts in a child process, so each measurement starts from a
// fresh heap and a crash or a hang stays contained.
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

import type { Report } from "./measure.ts";

export type Outcome =
  | { readonly status: "ok"; readonly report: Report }
  | { readonly status: "timeout" | "crash"; readonly detail?: string };

const measureScript = resolve(import.meta.dirname, "measure.ts");

export const measureInChild = (args: readonly string[], timeoutSeconds: number): Outcome => {
  const result = spawnSync(
    process.execPath,
    ["--experimental-strip-types", "--stack-size=8000", measureScript, ...args],
    { encoding: "utf8", timeout: timeoutSeconds * 1000, maxBuffer: 1 << 26 },
  );
  if (result.error && (result.error as NodeJS.ErrnoException).code === "ETIMEDOUT")
    return { status: "timeout" };
  if (result.status !== 0)
    return {
      status: "crash",
      detail: (result.stderr || result.stdout).trim().split("\n").slice(-3).join(" | "),
    };
  const line = result.stdout.trim().split("\n").at(-1)!;
  return { status: "ok", report: JSON.parse(line) as Report };
};

export const formatMs = (value: number | undefined): string =>
  value === undefined ? "-" : value < 10 ? value.toFixed(1) : String(Math.round(value));
