import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  bumpManifest,
  changes,
  compareVersions,
  cutRelease,
  manifestVersion,
  section,
} from "./release.ts";
import {
  archiveName,
  checksum,
  mainTestNotice,
  publish,
  releaseNotes,
  targets,
  writeReleaseNotes,
} from "./release-workflow.ts";

const manifest = `[workspace]
members = ["probe"]
resolver = "2"
[workspace.package]
version = "0.0.3"
edition = "2024"
[workspace.dependencies]
hd_probe = { path = "probe", version = "=0.0.3" }
[profile.release]
opt-level = 3
`;

test("version ordering is numeric, strict and does not truncate large components", () => {
  assert.equal(compareVersions("0.10.0", "0.9.99"), 1);
  assert.equal(compareVersions("0.0.3", "0.0.3"), 0);
  assert.equal(compareVersions("1.0.0", "2.0.0"), -1);
  assert.equal(compareVersions("9007199254740993.0.0", "9007199254740992.0.0"), 1);
  for (const invalid of ["v1.0.0", "01.0.0", "1.0", "1.2.3-rc.1", "1.2.3\n"]) {
    assert.throws(() => compareVersions(invalid, "1.0.0"));
  }
});

test("manifest changes only the workspace version and exact dependency pins", () => {
  const updated = bumpManifest(manifest, "0.0.4");
  assert.equal(manifestVersion(updated), "0.0.4");
  assert.match(updated, /version = "=0.0.4"/);
  assert.equal(updated.replaceAll("0.0.4", "0.0.3"), manifest);
  assert.throws(() => bumpManifest('[package]\nversion = "0.0.3"\n', "0.0.4"));
});

test("changelog groups actual Fixes trailers, deduplicates issues, and retains ungrouped subjects", () => {
  const commits = changes("abc\0Fix first\0Fixes #7, #9\nFixes #7\0\ndef\0Cleanup\0Refs #8\0\n");
  assert.deepEqual(
    commits.map((entry) => entry.issues),
    [[7, 9], []],
  );
  const text = section({
    version: "0.0.4",
    date: "2026-11-28",
    base: "v0.0.3",
    commits,
    titles: new Map([
      [7, "First issue"],
      [9, "Second issue"],
    ]),
    retired: [{ id: "old.rule", chapter: "lang/01-lexical.md" }],
  });
  assert.match(text, /### Breaking\n\n- Retired `old.rule`/);
  assert.match(text, /issues\/7\) First issue/);
  assert.match(text, /issues\/9\) Second issue/);
  assert.match(text, /### Other changes\n\n- Cleanup/);
  assert.throws(
    () =>
      section({
        version: "0.0.4",
        date: "today",
        base: "v0.0.3",
        commits,
        titles: new Map(),
        retired: [],
      }),
    /missing title/,
  );
});

test("release notes select one exact version and assets cover five distinct targets", async () => {
  assert.equal(
    releaseNotes(
      "# Changelog\n\n## v0.0.4 — today\n\nNew.\n\n## v0.0.3 — before\n\nOld.\n",
      "0.0.4",
    ),
    "## v0.0.4 — today\n\nNew.\n",
  );
  assert.throws(() => releaseNotes("## v0.0.40\nWrong.\n", "0.0.4"));
  assert.equal(new Set(targets.map(archiveName)).size, 5);
  assert.equal(archiveName("x86_64-pc-windows-msvc"), "hd-x86_64-pc-windows-msvc.zip");
  const directory = mkdtempSync(join(tmpdir(), "hd-release-hash-"));
  try {
    const file = join(directory, "archive");
    writeFileSync(file, "abc");
    assert.equal(
      await checksum(file),
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("main's Test state is a notice, never a gate", () => {
  const green = { id: 1, head_sha: "abc", conclusion: "success", status: "completed" };
  assert.match(mainTestNotice([green], "abc"), /run 1 at abc: success/);
  assert.match(mainTestNotice([green], "def"), /no run at def/);
  assert.match(
    mainTestNotice([green, { ...green, id: 2, conclusion: "failure" }], "abc"),
    /run 2 at abc: failure/,
  );
  assert.match(
    mainTestNotice([green, { ...green, id: 2, conclusion: null, status: "in_progress" }], "abc"),
    /run 2 at abc is in_progress/,
  );
});

function fixture(): { root: string; directory: string; git: (...args: string[]) => string } {
  const directory = mkdtempSync(join(tmpdir(), "hd-cut-release-"));
  const root = join(directory, "repo");
  mkdirSync(root);
  const git = (...args: string[]): string =>
    execFileSync("git", args, { cwd: root, encoding: "utf8", timeout: 30_000 }).trim();
  for (const [name, text] of Object.entries({
    "Cargo.toml": manifest,
    ".gitignore": "target/\n",
    "probe/Cargo.toml":
      '[package]\nname = "hd_probe"\nversion.workspace = true\nedition.workspace = true\n',
    "probe/src/lib.rs": "pub fn probe() {}\n",
    "spec/README.md": "# Spec\n",
    "spec/lang/01-lexical.md": "# Lexical\n\n1. r[old.rule] Old rule.\n",
    "spec/lang/06-control-flow.md": "# Control Flow\n",
  })) {
    const path = join(root, name);
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, text);
  }
  git("init", "-q");
  git("config", "user.name", "Release test");
  git("config", "user.email", "release-test@example.invalid");
  execFileSync("cargo", ["generate-lockfile", "--offline"], { cwd: root, timeout: 30_000 });
  git("add", "--", "Cargo.toml", "Cargo.lock", ".gitignore", "probe", "spec");
  git("commit", "-qm", "Base");
  git("tag", "v0.0.3");
  writeFileSync(join(root, "spec/lang/01-lexical.md"), "# Lexical\n\n1. r[new.rule] New rule.\n");
  git("add", "--", "spec/lang/01-lexical.md");
  git("commit", "-qm", "Fix widgets\n\nFixes #7");
  return { root, directory, git };
}

test(
  "cut a real local release, update Cargo.lock, and never push; reject dirty/equal versions",
  { skip: process.platform === "win32" },
  () => {
    // This toy repository must not consume the user's signing identity or
    // global hooks. The actual release command preserves normal git config.
    const previousGlobal = process.env.GIT_CONFIG_GLOBAL;
    const previousSystem = process.env.GIT_CONFIG_NOSYSTEM;
    process.env.GIT_CONFIG_GLOBAL = "/dev/null";
    process.env.GIT_CONFIG_NOSYSTEM = "1";
    const made = fixture();
    const oldPath = process.env.PATH;
    const oldGlobal = previousGlobal;
    const oldSystem = previousSystem;
    try {
      const bin = join(made.directory, "bin");
      mkdirSync(bin);
      const gh = join(bin, "gh");
      writeFileSync(
        gh,
        `#!${process.execPath}\nconsole.log(JSON.stringify({data:{repository:{i7:{title:"Widget correctness"}}}}));\n`,
      );
      chmodSync(gh, 0o755);
      process.env.PATH = `${bin}:${oldPath}`;
      assert.throws(() => cutRelease("0.0.3", made.root), /greater/);
      writeFileSync(join(made.root, "untracked"), "dirty");
      assert.throws(() => cutRelease("0.0.4", made.root), /clean worktree/);
      rmSync(join(made.root, "untracked"));
      const lock = readFileSync(join(made.root, "Cargo.lock"), "utf8");
      const cargo = join(bin, "cargo");
      writeFileSync(cargo, `#!${process.execPath}\nprocess.exit(1);\n`);
      chmodSync(cargo, 0o755);
      assert.throws(() => cutRelease("0.0.4", made.root), /Command failed/);
      assert.equal(readFileSync(join(made.root, "Cargo.toml"), "utf8"), manifest);
      assert.equal(readFileSync(join(made.root, "Cargo.lock"), "utf8"), lock);
      assert.equal(made.git("status", "--porcelain"), "");
      assert.ok(!existsSync(join(made.root, "CHANGELOG.md")));
      rmSync(cargo);
      cutRelease("0.0.4", made.root);
      assert.equal(made.git("log", "-1", "--format=%s"), "Release v0.0.4");
      assert.equal(made.git("rev-parse", "HEAD"), made.git("rev-parse", "v0.0.4^{commit}"));
      assert.equal(made.git("status", "--porcelain"), "");
      assert.match(readFileSync(join(made.root, "Cargo.lock"), "utf8"), /version = "0.0.4"/);
      const notes = readFileSync(join(made.root, "CHANGELOG.md"), "utf8");
      assert.match(notes, /Widget correctness/);
      assert.match(notes, /Retired `old.rule`/);
      assert.equal(made.git("remote"), "");
      assert.ok(existsSync(join(made.root, "CHANGELOG.md")));
    } finally {
      if (oldPath === undefined) {
        delete process.env.PATH;
      } else {
        process.env.PATH = oldPath;
      }
      if (oldGlobal === undefined) {
        delete process.env.GIT_CONFIG_GLOBAL;
      } else {
        process.env.GIT_CONFIG_GLOBAL = oldGlobal;
      }
      if (oldSystem === undefined) {
        delete process.env.GIT_CONFIG_NOSYSTEM;
      } else {
        process.env.GIT_CONFIG_NOSYSTEM = oldSystem;
      }
      rmSync(made.directory, { recursive: true, force: true });
    }
  },
);

test(
  "publication keeps drafts private, marks tags latest and rejects damaged archives",
  { skip: process.platform === "win32" },
  async () => {
    const saved = { ...process.env };
    const cwd = process.cwd();
    process.env.GIT_CONFIG_GLOBAL = "/dev/null";
    process.env.GIT_CONFIG_NOSYSTEM = "1";
    const made = fixture();
    try {
      const bin = join(made.directory, "bin");
      mkdirSync(bin);
      const log = join(made.directory, "calls.jsonl");
      const fake = join(bin, "gh");
      writeFileSync(
        fake,
        `#!${process.execPath}\nimport {appendFileSync} from 'node:fs';\nconst args=process.argv.slice(2);\nappendFileSync(${JSON.stringify(log)},JSON.stringify(args)+'\\n');\nif(args[0]==='api') console.log(JSON.stringify({data:{repository:{release:process.env.TEST_EXISTING_DRAFT==='1'?{isDraft:true}:null}}}));\n`,
      );
      chmodSync(fake, 0o755);
      process.env.PATH = `${bin}:${saved.PATH}`;
      process.env.GITHUB_REPOSITORY = "owner/repository";
      process.env.GITHUB_EVENT_NAME = "workflow_dispatch";
      process.env.RELEASE_TAG = "dry-run-v0.0.3-123";
      process.env.TEST_EXISTING_DRAFT = "0";
      const directory = join(made.root, "target/release-download");
      mkdirSync(directory, { recursive: true });
      for (const target of targets) {
        const file = join(directory, archiveName(target));
        writeFileSync(file, `binary for ${target}`);
        writeFileSync(`${file}.sha256`, `${await checksum(file)}  ${archiveName(target)}\n`);
      }
      process.chdir(made.root);
      await publish();
      process.env.TEST_EXISTING_DRAFT = "1";
      await publish();
      const calls = readFileSync(log, "utf8")
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line) as string[]);
      const create = calls.filter((args) => args[1] === "create");
      assert.equal(create.length, 1);
      assert.ok(create[0]!.includes("--draft"));
      const edits = calls.filter((args) => args[1] === "edit");
      assert.equal(edits.length, 2);
      assert.ok(edits.every((args) => args.includes("--draft=true")));
      const uploads = calls.filter((args) => args[1] === "upload");
      assert.ok(
        uploads.every(
          (args) =>
            args.includes("--clobber") &&
            args.filter((word) => /\.(zip|gz)$/.test(word)).length === 5,
        ),
      );
      const sums = readFileSync(join(directory, "SHA256SUMS"), "utf8");
      assert.equal(sums.trim().split("\n").length, 5);
      assert.ok(edits.every((args) => !args.includes("--latest")));
      process.env.GITHUB_EVENT_NAME = "push";
      process.env.RELEASE_TAG = "v0.0.3";
      writeFileSync(
        join(made.root, "CHANGELOG.md"),
        "# Changelog\n\n## v0.0.3 — today\n\nPublished notes.\n",
      );
      await publish();
      const publishedCalls = readFileSync(log, "utf8")
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line) as string[]);
      const publishedEdit = publishedCalls.findLast((args) => args[1] === "edit");
      assert.ok(publishedEdit?.includes("--latest"));
      assert.ok(publishedEdit?.includes("--draft=false"));
      const beforeDamage = publishedCalls.length;
      writeFileSync(join(directory, archiveName(targets[0])), "tampered");
      await assert.rejects(publish(), /checksum mismatch/);
      assert.equal(readFileSync(log, "utf8").trim().split("\n").length, beforeDamage);
    } finally {
      process.chdir(cwd);
      for (const key of Object.keys(process.env)) {
        if (!(key in saved)) {
          delete process.env[key];
        }
      }
      Object.assign(process.env, saved);
      rmSync(made.directory, { recursive: true, force: true });
    }
  },
);

test("oversized real-world notes remain complete as a checksummed asset", async () => {
  const directory = mkdtempSync(join(tmpdir(), "hd-release-large-notes-"));
  try {
    const completeNotes = "Commit: abc\n\n" + "Full change entry.\n".repeat(50_000);
    writeFileSync(join(directory, "SHA256SUMS"), "");
    const files = await writeReleaseNotes({
      directory,
      version: "0.0.4",
      repo: "owner/repo",
      completeNotes,
    });
    assert.equal(files.length, 1);
    assert.equal(readFileSync(files[0]!, "utf8"), completeNotes);
    const body = readFileSync(join(directory, "notes.md"), "utf8");
    assert.ok(body.length < 125_000);
    assert.match(body, /releases\/download\/v0\.0\.4\/CHANGELOG-v0\.0\.4\.md/);
    assert.equal(
      readFileSync(join(directory, "SHA256SUMS"), "utf8"),
      `${await checksum(files[0]!)}  CHANGELOG-v0.0.4.md\n`,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
