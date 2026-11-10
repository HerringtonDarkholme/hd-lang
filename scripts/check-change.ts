// Output-quality checks on a change, enforced in CI (owner, 2026-11-08: "do the
// quality gating script in CI, make it only check the changed case, do not run
// full check"). Every check reads only the commits in BASE..HEAD: their added
// lines, the files they touch, and their messages. Nothing scans the tree.
//
// Usage: node scripts/check-change.ts BASE [HEAD]
//
// Checks, each with its owner rule:
// - unsupported: no added `unsupported(` or `Code::Unsupported` in crates/*/src.
//   No override (2026-11-08: "ban fucking unsupported shit").
// - shortcut: an added non-test line in a core crate that refuses (`todo!(`,
//   `unimplemented!(`) or asserts a case away (`unreachable!(`, `.expect("`)
//   needs `ruled out:` or `gap: #N` within the 3 lines above it or on it.
// - cost: an added line that walks a directory, spawns a process or rewrites an
//   index needs a `cost:` comment within 3 lines (shared-rules "Cost Model").
// - fix-test: a commit that says it fixes, reverts, leaks or miscompiles, and
//   adds compiler or std source, adds a conformance fixture or a test.
// - price: a change to code the compiler emits that adds programs needs a .wat
//   baseline showing what they emit.
//
// An override is a trailer line in a commit message of the range, read after
// the code: `Shortcut-OK: reason`, `Cost-OK: reason`, `Price-OK: reason`, and
// `No-fixture-OK: reason` naming the existing case. The merge script writes the
// overrides it was given into an empty "Review overrides" commit of the batch.
//
// Cost per run: one `git diff -U3` per check and one `git log` of the range, plus
// one `git show` per commit in the range and per changed Rust file a check reads.
// Linear in the change, not the repository.

import { execFileSync } from "node:child_process";

const [base, head = "HEAD"] = process.argv.slice(2);
if (!base) {
  console.error("usage: node scripts/check-change.ts BASE [HEAD]");
  process.exit(2);
}

// cost: every git call below reads the range BASE..HEAD only.
function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 28 });
}

const CORE = [
  "hd_syntax",
  "hd_resolve",
  "hd_types",
  "hd_check",
  "hd_tir",
  "hd_mono",
  "hd_opt",
  "hd_wasm",
  "hd_driver",
  "hd_structure",
];

interface Added {
  file: string;
  body: string;
  // Up to 3 lines before it in the new file.
  before: string[];
  inTest: boolean;
}

// The first line of a Rust file's top-level `#[cfg(test)]` item, 1-based, or
// Infinity: everything from it on is test code, where a panic on a broken
// setup is the intended failure. Reads the file at HEAD once.
function testStart(file: string): number {
  if (!file.endsWith(".rs")) {
    return Infinity;
  }
  const text = git("show", `${head}:${file}`);
  const index = text.split("\n").findIndex((line) => line.startsWith("#[cfg(test)]"));
  return index < 0 ? Infinity : index + 1;
}

// Added lines of `git diff -U3`, each with the 3 lines above it in the new file.
function addedLines(paths: string[], exclude: string[] = []): Added[] {
  const excluded = exclude.map((path) => `:(exclude)${path}`);
  const diff = git("diff", "-U3", `${base}..${head}`, "--", ...paths, ...excluded);
  const out: Added[] = [];
  let file = "";
  let before: string[] = [];
  let tests = Infinity;
  // The new file's line number of the next context or added line.
  let lineNo = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ ")) {
      file = line.slice(6).trim();
      tests = line.startsWith("+++ /dev/null") ? Infinity : testStart(file);
      before = [];
      continue;
    }
    const hunk = /^@@ -\d+(?:,\d+)? \+(\d+)/.exec(line);
    if (hunk) {
      lineNo = Number(hunk[1]);
      before = [];
      continue;
    }
    const header = ["--- ", "diff ", "index "].some((prefix) => line.startsWith(prefix));
    if (header) {
      before = [];
      continue;
    }
    if (line.startsWith("-")) {
      continue;
    }
    const body = line.slice(1);
    if (line.startsWith("+")) {
      out.push({ file, body, before: [...before], inTest: lineNo >= tests });
    }
    before = [...before, body].slice(-3);
    lineNo += 1;
  }
  return out;
}

function isComment(body: string): boolean {
  return /^\s*(\/\/|#|\*)/.test(body);
}

function show(added: Added): string {
  return `${added.file}: ${added.body.trim().slice(0, 120)}`;
}

const messages = git("log", "--format=%B%x00", `${base}..${head}`);

function override(key: string): string | undefined {
  const found = messages.match(new RegExp(`^${key}:\\s*(.+)$`, "mi"));
  return found?.[1];
}

const failures: string[] = [];

function report(title: string, lines: string[], key?: string): void {
  if (lines.length === 0) {
    return;
  }
  const reason = key ? override(key) : undefined;
  const listed = lines.map((line) => `  ${line}`).join("\n");
  if (reason) {
    console.log(`${title} (overridden, ${key}: ${reason})\n${listed}`);
    return;
  }
  console.error(`${title}\n${listed}`);
  failures.push(title);
}

// unsupported: banned outright.
report(
  "UNSUPPORTED IS BANNED: implement the case, or reject it under a real spec rule and ticket the gap",
  addedLines(["crates/*/src/**"])
    .filter(
      (added) => /\bunsupported\(|Code::Unsupported/.test(added.body) && !isComment(added.body),
    )
    .map(show),
);

// shortcut: a refusal or asserted-away case with no reason next to it.
const risky = /\btodo!\(|\bunimplemented!\(|\bunreachable!\(|\.expect\("/;
function reasoned(added: Added): boolean {
  return [...added.before, added.body].some(
    (text) => text.includes("ruled out:") || /gap: #\d+/.test(text),
  );
}
report(
  "SHORTCUT CODE: state what rules the case out (`ruled out:`) or ticket it (`gap: #N`)",
  addedLines(CORE.map((crate) => `crates/${crate}/src/**`))
    .filter(
      (added) =>
        !added.inTest && risky.test(added.body) && !isComment(added.body) && !reasoned(added),
    )
    .map(show),
  "Shortcut-OK",
);

// cost: whole-directory walks, spawns and index rewrites state their bound.
const costly =
  /\bread_dir\(|readdirSync\(|readdir\(|spawnSync\(|execFileSync\(|execSync\(|\bspawn\(|Command::new\(|write_index\(|index_bytes\(/;
function costStated(added: Added): boolean {
  return [...added.before, added.body].some((text) => text.includes("cost:"));
}
report(
  "PER-RUN COST UNSTATED: a walk, spawn or index rewrite needs a `cost:` comment bounding it by the change",
  addedLines(
    [
      "crates/*/src/**",
      "scripts/**",
      "spec/*.ts",
      "spec/tools/**",
      "website/*.ts",
      "website/src/**",
      "packages/**/*.ts",
    ],
    ["**/tests/**", "**/test/**", "**/*test*.ts", "**/*.md", "**/*.tsv", "**/*.json", "**/*.yml"],
  )
    .filter((added) => costly.test(added.body) && !isComment(added.body) && !costStated(added))
    .map(show),
  "Cost-OK",
);

// fix-test: a fix carries the test that fails without it.
const fixWords =
  /\b(fix(es)?|revert|leak|double free|miscompil\w*|wrong answer|use after free)\b|^fixed\b/im;
const untested: string[] = [];
for (const commit of git("rev-list", "--reverse", `${base}..${head}`).split("\n").filter(Boolean)) {
  const numstat = git(
    "show",
    "--numstat",
    "--format=",
    commit,
    "--",
    "crates/*/src/**",
    "lib/std/**",
  );
  const addedCount = numstat
    .split("\n")
    .reduce((sum, row) => sum + (Number(row.split("\t")[0]) || 0), 0);
  if (addedCount === 0) {
    continue;
  }
  const message = git("log", "-1", "--format=%s%n%b", commit).replace(
    /fixed point|fixed-[a-z]+/g,
    "",
  );
  if (!fixWords.test(message)) {
    continue;
  }
  if (/^No-fixture-OK:/m.test(message) || override("No-fixture-OK")) {
    continue;
  }
  const status = git("show", "--name-status", "--format=", commit);
  if (
    /^A\s+(spec\/conformance\/(runtime|typing|parse|cli)\/|crates\/[a-z_]+\/tests\/)/m.test(status)
  ) {
    continue;
  }
  untested.push(`${commit.slice(0, 9)} ${message.split("\n")[0].slice(0, 100)}`);
}
report(
  "NO TEST FOR A FIX: add a runtime fixture or a test, or name the existing one in a `No-fixture-OK:` trailer",
  untested,
);

// price: changed emitted code that adds programs shows their .wat.
const changes = git("diff", "--name-status", `${base}..${head}`)
  .split("\n")
  .filter(Boolean)
  .map((row) => {
    const parts = row.split("\t");
    return { status: parts[0], path: parts.at(-1) ?? "" };
  });
const emitter =
  /^crates\/(hd_wasm\/src|hd_tir\/src\/own|hd_tir\/src\/inline|hd_opt\/src|hd_mono\/src)|^lib\/std\//;
const emits = changes.some(
  (change) => change.status !== "D" && emitter.test(change.path) && !change.path.endsWith(".md"),
);
const priced = changes.some((change) => /\/baselines\/.*\.wat$/.test(change.path));
const program =
  /^spec\/conformance\/(runtime|cli)\/|^crates\/[^/]+\/tests\/cases\/|^crates\/hd_driver\/tests\/std_rt\//;
const programs = changes
  .filter((change) => change.status === "A" && program.test(change.path))
  .map((change) => change.path);
report(
  "PRICE MISSING: compiler code changed and new programs were added, but no .wat baseline shows what they emit",
  emits && !priced ? programs.slice(0, 5) : [],
  "Price-OK",
);

if (failures.length > 0) {
  console.error(
    `\ncheck-change: ${failures.length} check(s) failed on ${base.slice(0, 9)}..${head}`,
  );
  process.exitCode = 1;
} else {
  console.log(`check-change: ok on ${base.slice(0, 9)}..${head}`);
}
