// Phase 4.1: round-trip f64 special values and multi-byte strings through host providers.
// Usage: node --experimental-strip-types audit/scripts/runtime/host-values.ts
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const root = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const directory = "audit/probes/runtime/host-values";

function hd(args: readonly string[]): string {
  const result = spawnSync("node", ["--experimental-strip-types", "bin/hd.js", ...args], {
    cwd: root,
    encoding: "utf8",
    timeout: 30_000,
  });
  if (result.error) return "TIMEOUT";
  const stdout = result.stdout
    .split("\n")
    .filter((line) => line && !line.endsWith(".replay.json"))
    .join(" ");
  const stderr = result.stderr
    .split("\n")
    .find((line) => /^\w*Error|runtime panic|^\S+:\d+:\d+:/.test(line));
  return `exit${result.status} ${stdout}${stderr ? ` | ${stderr}` : ""}`.trim();
}

interface Encoded {
  readonly kind: string;
  readonly bits?: string;
  readonly utf8?: string;
}

const rows = ["probe\tlive_run\trecord\treplay\trecorded_argument\trecorded_result"];
for (const name of readdirSync(join(root, directory))
  .filter((file) => file.endsWith(".hd"))
  .sort()) {
  const path = `${directory}/${name}`;
  const profile = ["--profile", name.startsWith("f64") ? "ready-float" : "ready-text"];
  const live = hd(["run", ...profile, path]);
  const recorded = hd(["record", ...profile, path]);
  const replayed = hd(["replay", ...profile, path]);
  let argument = "-";
  let value = "-";
  try {
    const events = JSON.parse(readFileSync(join(root, `${path}.replay.json`), "utf8")) as {
      operation: string;
      encodedArguments: Encoded[];
      encodedValue?: Encoded;
    }[];
    const host = events.find((event) => event.operation === "provider-poll");
    const show = (encoded: Encoded | undefined): string =>
      encoded ? `${encoded.kind}:${encoded.bits ?? encoded.utf8}` : "none";
    argument = host ? host.encodedArguments.map(show).join(",") : "no provider event";
    value = host ? show(host.encodedValue) : "-";
  } catch {
    argument = "no sidecar";
  }
  rows.push([name, live, recorded, replayed, argument, value].join("\t"));
}
const header = `# commit bd985d7, ${new Date().toISOString()}, node ${process.version}\n# command: node --experimental-strip-types audit/scripts/runtime/host-values.ts\n`;
writeFileSync(join(root, "audit/evidence/04-runtime/host-values.tsv"), header + rows.join("\n") + "\n");
console.log(rows.join("\n"));
