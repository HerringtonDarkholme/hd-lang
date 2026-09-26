// Phase 4.1: round-trip every suspension fixture through `hd record` and `hd replay`.
// Usage: node --experimental-strip-types audit/scripts/runtime/roundtrip.ts LIST_FILE
// LIST_FILE holds one repository-relative fixture path per line.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

const root = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const probeDir = join(root, "audit/probes/runtime/roundtrip");
const evidenceDir = join(root, "audit/evidence/04-runtime");

interface RunResult {
  readonly code: number | null;
  readonly stdout: string;
  readonly stderr: string;
}

function hd(args: readonly string[]): RunResult {
  const result = spawnSync("node", ["--experimental-strip-types", "bin/hd.js", ...args], {
    cwd: root,
    encoding: "utf8",
    timeout: 30_000,
  });
  if (result.error) return { code: -1, stdout: "", stderr: `TIMEOUT(30s): ${result.error.message}` };
  return { code: result.status, stdout: result.stdout.trim(), stderr: result.stderr.trim() };
}

function directive(source: string, name: string): string | undefined {
  const match = new RegExp(`^# ${name}: (.+)$`, "m").exec(source);
  return match?.[1]?.trim();
}

function firstLine(text: string): string {
  return (text.split("\n")[0] ?? "").slice(0, 110).replaceAll("\t", " ");
}

function outcome(result: RunResult): string {
  if (result.code === 0) return "ok";
  const stderrLines = result.stderr.split("\n").filter((line) => !line.startsWith("    at "));
  const errorLine =
    stderrLines.find((line) => /^(Error|RuntimePanicError|TypeError)/.test(line)) ??
    stderrLines.find((line) => line.includes("runtime panic")) ??
    stderrLines[0] ??
    "";
  return `exit${result.code}:${firstLine(errorLine)}`;
}

const listFile = process.argv[2];
if (!listFile) throw new Error("usage: roundtrip.ts LIST_FILE");
const fixtures = readFileSync(listFile, "utf8").split("\n").filter(Boolean);
mkdirSync(probeDir, { recursive: true });
const rows: string[] = [
  "fixture\tprofile\tscenario\tcheck\trun\trecord\treplay\tevents\tstdout_match\tverdict",
];
for (const fixture of fixtures) {
  const source = readFileSync(join(root, fixture), "utf8");
  const profile = directive(source, "fixture-runtime-profile");
  const scenario = directive(source, "fixture-runtime-scenario");
  const copyName = fixture.replaceAll("/", "__");
  const copy = join(probeDir, copyName);
  copyFileSync(join(root, fixture), copy);
  const sidecar = `${copy}.replay.json`;
  rmSync(sidecar, { force: true });
  const relCopy = `audit/probes/runtime/roundtrip/${copyName}`;
  const profileArgs = profile ? ["--profile", profile] : [];
  const checked = hd(["check", ...profileArgs, relCopy]);
  if (checked.code !== 0) {
    rows.push(
      [fixture, profile ?? "-", scenario ?? "-", outcome(checked), "-", "-", "-", "-", "-", "static-reject"].join("\t"),
    );
    continue;
  }
  const ran = hd(["run", ...profileArgs, relCopy]);
  const recorded = hd(["record", ...profileArgs, relCopy]);
  const hasSidecar = existsSync(sidecar);
  const replayed = hasSidecar ? hd(["replay", ...profileArgs, relCopy]) : undefined;
  let events = "-";
  if (hasSidecar) {
    const parsed = JSON.parse(readFileSync(sidecar, "utf8")) as { operation: string }[];
    const counts = new Map<string, number>();
    for (const event of parsed) counts.set(event.operation, (counts.get(event.operation) ?? 0) + 1);
    events = [...counts].map(([key, value]) => `${key}=${value}`).join(",") || "0";
  }
  const recordedStdout = recorded.stdout
    .split("\n")
    .filter((line) => !line.endsWith(".replay.json"))
    .join("\n");
  const stdoutMatch =
    replayed === undefined
      ? "-"
      : replayed.stdout === recordedStdout && recordedStdout === ran.stdout
        ? "yes"
        : `no(run=${JSON.stringify(ran.stdout)} rec=${JSON.stringify(recordedStdout)} rep=${JSON.stringify(replayed.stdout)})`;
  let verdict: string;
  if (!hasSidecar) verdict = recorded.code === 0 ? "no-sidecar" : "not-recordable";
  else if (replayed?.code === 0 && stdoutMatch === "yes") verdict = "roundtrip-ok";
  else if (replayed && outcome(replayed) === outcome(recorded) && stdoutMatch === "yes")
    verdict = "roundtrip-same-failure";
  else verdict = "roundtrip-MISMATCH";
  rows.push(
    [
      fixture,
      profile ?? "-",
      scenario ?? "-",
      "ok",
      outcome(ran),
      outcome(recorded),
      replayed ? outcome(replayed) : "-",
      events,
      stdoutMatch,
      verdict,
    ].join("\t"),
  );
  process.stderr.write(`${basename(fixture)}: ${verdict}\n`);
}
mkdirSync(evidenceDir, { recursive: true });
const header = `# commit bd985d7, ${new Date().toISOString()}, node ${process.version}\n# command: node --experimental-strip-types audit/scripts/runtime/roundtrip.ts ${listFile}\n`;
writeFileSync(join(evidenceDir, "roundtrip.tsv"), header + rows.join("\n") + "\n");
