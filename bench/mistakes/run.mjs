// Q25: mistake corpus vs `hd check --format json`.
//
// Each file in cases/ carries `# code:` (expected diagnostic code) and
// `# line:` (the source line holding the mistake) headers. The script runs
// `hd check --format json` on each, takes the first diagnostic, and reports
// the share with the expected code and the share whose reported line is the
// mistake's line. Files with a `tests:` block (the test-code mistake kinds)
// are checked with `hd check --tests --format json` instead.
//
// Usage: node compiler/bench/mistakes/run.mjs
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const NEW_HD = join(REPO, "compiler", "target", "release", "hd");
const TIMEOUT_MS = 120_000;

function gitHash() {
  try {
    return execFileSync("git", ["rev-parse", "--short", "HEAD"], {
      encoding: "utf8",
      cwd: REPO,
    }).trim();
  } catch {
    return "unknown";
  }
}

function check(file, tests) {
  const argv = tests
    ? ["check", "--tests", "--format", "json", file]
    : ["check", "--format", "json", file];
  let out = "";
  try {
    out = execFileSync(NEW_HD, argv, {
      encoding: "utf8",
      timeout: TIMEOUT_MS,
    });
  } catch (err) {
    out = (err.stdout ?? "").toString();
  }
  const diags = [];
  for (const line of out.split("\n")) {
    const t = line.trim();
    if (!t.startsWith("{")) continue;
    try {
      const o = JSON.parse(t);
      if (o.kind === "diagnostic") diags.push(o);
    } catch {
      /* ignore */
    }
  }
  return diags;
}

function main() {
  const names = readdirSync(join(HERE, "cases"))
    .filter((f) => f.endsWith(".hd"))
    .sort();
  const rows = [];
  for (const name of names) {
    const text = readFileSync(join(HERE, "cases", name), "utf8");
    const code = text.match(/^# code: (\S+)/m)[1];
    const line = Number(text.match(/^# line: (\d+)/m)[1]);
    const tests = /^tests:/m.test(text);
    const diags = check(join(HERE, "cases", name), tests);
    const first = diags[0] ?? null;
    rows.push({
      name: name.slice(0, -3),
      code,
      line,
      mode: tests ? "--tests" : "check",
      got: first ? first.code : "(no diagnostic)",
      gotLine: first ? first.line : "-",
      gotSev: first ? first.severity : "-",
      n: diags.length,
    });
  }
  const codeHit = rows.filter((r) => r.got === r.code).length;
  const lineHit = rows.filter((r) => r.gotLine === r.line).length;
  const bothHit = rows.filter((r) => r.got === r.code && r.gotLine === r.line).length;

  console.log(`# Mistake corpus vs hd check`);
  console.log(``);
  console.log(`- hd commit: \`${gitHash()}\`; \`hd check [--tests] --format json FILE\` per case (--tests for cases with a tests: block)`);
  console.log(`- cases: ${rows.length}; first diagnostic per file decides`);
  console.log(`- code match: ${codeHit}/${rows.length} (${((100 * codeHit) / rows.length).toFixed(1)}%)`);
  console.log(
    `- line match: ${lineHit}/${rows.length} (${((100 * lineHit) / rows.length).toFixed(1)}%; goal >= 95%)`,
  );
  console.log(`- both: ${bothHit}/${rows.length}`);
  console.log(``);
  console.log(`| Case | Mode | Expected | Line | Got | Got line | Diags |`);
  console.log(`| --- | --- | --- | ---: | --- | ---: | ---: |`);
  for (const r of rows) {
    const flag = r.got === r.code && r.gotLine === r.line ? "" : " **MISS**";
    console.log(
      `| ${r.name} | ${r.mode} | ${r.code} | ${r.line} | ${r.got} | ${r.gotLine} | ${r.n} |${flag}`,
    );
  }
}

main();
