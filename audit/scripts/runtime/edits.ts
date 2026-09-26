// Phase 4.1 edit experiments: record base.hd, then replay its history against each edited copy.
// Usage: node --experimental-strip-types audit/scripts/runtime/edits.ts
import { spawnSync } from "node:child_process";
import { copyFileSync, readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const root = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const directory = "audit/probes/runtime/edits";
const profile = ["--profile", "ready-counter"];

interface Event {
  readonly siteId: string;
  readonly functionName: string;
  readonly functionCodeId: string;
}

function hd(args: readonly string[]): { code: number | null; out: string } {
  const result = spawnSync("node", ["--experimental-strip-types", "bin/hd.js", ...args], {
    cwd: root,
    encoding: "utf8",
  });
  const errorLine =
    result.stderr.split("\n").find((line) => /^Error|: runtime panic|^\S+:\d+:\d+:/.test(line)) ?? "";
  return { code: result.status, out: `${result.stdout.trim()}${errorLine ? ` | ${errorLine}` : ""}` };
}

function events(path: string): Event[] {
  return JSON.parse(readFileSync(join(root, path), "utf8")) as Event[];
}

const base = `${directory}/base.hd`;
const recorded = hd(["record", ...profile, base]);
if (recorded.code !== 0) throw new Error(`base record failed: ${recorded.out}`);
const baseEvents = events(`${base}.replay.json`);
const rows = ["variant\tlive_run\towned_ids_changed\treplay_with_base_history\tverdict_hint"];
const variants = readdirSync(join(root, directory))
  .filter((name) => /^v\d+.*\.hd$/.test(name))
  .sort();
for (const name of variants) {
  const variant = `${directory}/${name}`;
  const live = hd(["run", ...profile, variant]);
  const own = hd(["record", ...profile, variant]);
  let changed = "-";
  if (own.code === 0) {
    renameSync(join(root, `${variant}.replay.json`), join(root, `${variant}.own.json`));
    const ownEvents = events(`${variant}.own.json`);
    changed =
      ownEvents
        .map((event, index) => {
          const before = baseEvents[index];
          if (!before) return `+${event.siteId}`;
          const parts: string[] = [];
          if (before.siteId !== event.siteId) parts.push(`site ${before.siteId}->${event.siteId}`);
          if (before.functionCodeId !== event.functionCodeId)
            parts.push(`code(${event.functionName})`);
          return parts.join(" ");
        })
        .filter(Boolean)
        .join("; ") || "none";
  }
  copyFileSync(join(root, `${base}.replay.json`), join(root, `${variant}.replay.json`));
  const replayed = hd(["replay", ...profile, variant]);
  const hint =
    replayed.code === 0
      ? live.out === replayed.out
        ? "accepted"
        : `accepted-DIFFERENT-OUTPUT(base=42)`
      : "rejected";
  rows.push([name, live.out, changed, `exit${replayed.code}: ${replayed.out}`, hint].join("\t"));
}
const header = `# commit bd985d7, ${new Date().toISOString()}, node ${process.version}\n# command: node --experimental-strip-types audit/scripts/runtime/edits.ts (base history: ${base}.replay.json, profile ready-counter)\n`;
writeFileSync(join(root, "audit/evidence/04-runtime/edits.tsv"), header + rows.join("\n") + "\n");
console.log(rows.join("\n"));
