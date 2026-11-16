// check-change.ts judges what a branch adds since it left its base: a base
// that moved on after the branch began blames none of its own edits on it.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./check-change.ts", import.meta.url));

// A repository on `main` whose one Rust file holds `source`.
function repository(source: string): { dir: string; git: (...args: string[]) => void } {
  const dir = mkdtempSync(join(tmpdir(), "check-change-"));
  const git = (...args: string[]) => {
    execFileSync(
      "git",
      ["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t", ...args],
      { cwd: dir, stdio: "ignore" },
    );
  };
  git("init", "-q", "-b", "main");
  mkdirSync(join(dir, "crates/hd_wasm/src"), { recursive: true });
  mkdirSync(join(dir, "spec/conformance"), { recursive: true });
  writeFileSync(join(dir, "crates/hd_wasm/src/a.rs"), source);
  writeFileSync(join(dir, "spec/conformance/cases.tsv"), "");
  git("add", "-A");
  git("commit", "-qm", "base");
  return { dir, git };
}

const unreasoned = 'fn a() {\n    x.expect("y");\n}\n';
const plain = "fn a() {}\n";

function check(dir: string): { status: number | null; out: string } {
  const run = spawnSync(process.execPath, [script, "main", "lane"], {
    cwd: dir,
    encoding: "utf8",
  });
  return { status: run.status, out: run.stdout + run.stderr };
}

test("a line the base removed after the branch began is not the branch's", () => {
  const { dir, git } = repository(unreasoned);
  try {
    git("checkout", "-qb", "lane");
    writeFileSync(join(dir, "notes.txt"), "b\n");
    git("add", "-A");
    git("commit", "-qm", "notes");
    git("checkout", "-q", "main");
    writeFileSync(join(dir, "crates/hd_wasm/src/a.rs"), plain);
    git("commit", "-qam", "main moves");
    const { status, out } = check(dir);
    assert.equal(status, 0, out);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a line the branch adds is still the branch's", () => {
  const { dir, git } = repository(plain);
  try {
    git("checkout", "-qb", "lane");
    writeFileSync(join(dir, "crates/hd_wasm/src/a.rs"), unreasoned);
    git("commit", "-qam", "lane adds");
    const { status, out } = check(dir);
    assert.equal(status, 1, out);
    assert.match(out, /SHORTCUT CODE/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
