// Stored counts of earlier runs. A run's counts depend on the module's bytes, the
// program name it gets, and the host that runs it, so these name the stored result.
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { countEntry } from "./baseline.ts";
import type { Entry } from "./baseline.ts";

export function runnerDigest(root: string): string {
  const hash = createHash("sha256");
  hash.update(`${process.version}\0${process.env.NODE_OPTIONS ?? ""}\0`);
  const host = readdirSync(join(root, "host"), { recursive: true, encoding: "utf8" })
    .filter((name) => name.endsWith(".ts"))
    .map((name) => join("host", name));
  for (const name of [...host, "bench/counts/measure.ts", "bench/counts/baseline.ts"].sort()) {
    hash
      .update(`${name}\0`)
      .update(readFileSync(join(root, name)))
      .update("\0");
  }
  return hash.digest("hex");
}

export function resultKey(runner: string, program: string, module: Uint8Array): string {
  return createHash("sha256").update(`${runner}\0${program}\0`).update(module).digest("hex");
}

// A missing or unreadable result is a miss: the program runs again.
export function readResult(directory: string, key: string, program: string): Entry | undefined {
  const path = join(directory, `${key}.json`);
  if (!existsSync(path)) {
    return undefined;
  }
  try {
    const row = JSON.parse(readFileSync(path, "utf8")) as Entry;
    const whole = countEntry(program, [row.retained, row.released, row.uniques].map(BigInt));
    return row.program === program && row.total === whole.total && /^\d+$/.test(row.noOps ?? "")
      ? row
      : undefined;
  } catch {
    return undefined;
  }
}

export function writeResult(directory: string, key: string, row: Entry): void {
  mkdirSync(directory, { recursive: true });
  const partial = join(directory, `${key}.${process.pid}.partial`);
  writeFileSync(partial, JSON.stringify(row));
  renameSync(partial, join(directory, `${key}.json`));
}
