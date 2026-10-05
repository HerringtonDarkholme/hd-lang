// Fetching dependencies (spec/cli/command-line.md#dependencies), end to end
// and with no network: each "remote" is a local git repository, and git's
// own `url.<base>.insteadOf` setting, in a temporary GIT_CONFIG_GLOBAL,
// maps https://github.com/ to it. HOME and HD_CACHE are temporary too, so a
// test never reads the user's git configuration, credentials, or cache.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";

import { removeTree } from "../src/dependencies/cache.ts";
import { maskCredentials } from "../src/dependencies/git.ts";
import {
  addWorkspaceEntry,
  addWorkspaceMember,
  removeDependency,
  setDependency,
} from "../src/dependencies/manifest-edit.ts";
import {
  compareVersions,
  compatibilityLine,
  parseHostPath,
  parseHostRequirement,
  parseVersion,
  pseudoBase,
  pseudoCommit,
} from "../src/dependencies/requirement.ts";
import { formatSum } from "../src/dependencies/sum.ts";
import { runHd, type HdResult } from "./hd-in-process.ts";

const hasGit = spawnSync("git", ["--version"]).status === 0;

let root: string;
let variables: Record<string, string>;

/** Runs git for a test's own repositories, with the test's configuration only. */
function git(cwd: string, ...args: string[]): string {
  const result = spawnSync("git", args, { cwd, env: variables, encoding: "utf8" });
  assert.equal(result.status, 0, `git ${args.join(" ")}: ${result.stderr}`);
  return result.stdout;
}

/** Writes `files` under `directory`. */
async function writeTree(directory: string, files: Record<string, string>): Promise<void> {
  for (const [path, text] of Object.entries(files)) {
    await mkdir(dirname(join(directory, path)), { recursive: true });
    await writeFile(join(directory, path), text);
  }
}

/** A "remote" repository at github.com/acme/NAME, one commit and tag per version. */
async function remote(
  name: string,
  versions: readonly (readonly [string, Record<string, string>])[],
): Promise<string> {
  const directory = join(root, "remotes", "acme", name);
  await mkdir(directory, { recursive: true });
  git(directory, "init", "--quiet");
  for (const [tag, files] of versions) {
    await writeTree(directory, files);
    git(directory, "add", "--all");
    git(directory, "commit", "--quiet", "--allow-empty", "-m", tag);
    git(directory, "tag", tag);
  }
  return directory;
}

/** The tree hash of `files`, by cli.sum.summary and cli.sum.hash, computed here on its own. */
function expectedHash(files: Record<string, string>): string {
  const summary = Object.keys(files)
    .sort()
    .map((path) => `${createHash("sha256").update(files[path]!).digest("hex")}  ${path}\n`)
    .join("");
  return `h1:${createHash("sha256").update(summary).digest("base64")}`;
}

/** A version's manifest line (cli.sum.manifest-line): the hash of a tree of its hd.toml alone. */
function manifestLine(version: string, files: Record<string, string>): string {
  return `${version}/hd.toml ${expectedHash({ "hd.toml": files["hd.toml"]! })}\n`;
}

/** A selected version's two lines: its tree line, then its manifest line (cli.sum.order). */
function selectedLines(version: string, files: Record<string, string>): string {
  return `${version} ${expectedHash(files)}\n${manifestLine(version, files)}`;
}

async function hd(
  cwd: string,
  args: string[],
  extra: Record<string, string> = {},
): Promise<HdResult> {
  return runHd(args, { cwd, variables: { ...variables, ...extra } });
}

/** A new application package that prints through its dependencies. */
async function app(name: string, main: string, manifest = ""): Promise<string> {
  const directory = join(root, "apps", name);
  await writeTree(directory, {
    "hd.toml": `[package]\nname = "${name}"\n${manifest}`,
    "src/main.hd": main,
  });
  return directory;
}

const TEXT_V1 = {
  "hd.toml": '[package]\nname = "text"\n',
  "src/lib.hd":
    'pub fn shout(word: string) -> string:\n    word + "!"\n\nfn secret() -> string: "hidden"\n',
};
const TEXT_V1_1 = {
  ...TEXT_V1,
  "src/lib.hd": `${TEXT_V1["src/lib.hd"]}\npub fn whisper(word: string) -> string:\n    "(" + word + ")"\n`,
};

const SHOUT_MAIN =
  'use dep.text.{shout}\n\npub fn main() -> void $ Console:\n    println(shout("hello"))\n';

const needsGit = { skip: !hasGit && "git is not installed" };

/** Makes the temporary root, its git configuration, and the remote `text`, once. */
async function setUp(): Promise<void> {
  if (root) return;
  root = await mkdtemp(join(tmpdir(), "hd-deps-"));
  await mkdir(join(root, "home"));
  const config = join(root, "gitconfig");
  await writeFile(
    config,
    [
      "[user]",
      "\tname = hd test",
      "\temail = test@example.invalid",
      "[init]",
      "\tdefaultBranch = main",
      "[tag]",
      "\tgpgSign = false",
      "[commit]",
      "\tgpgSign = false",
      `[url "file://${join(root, "remotes")}/"]`,
      "\tinsteadOf = https://github.com/",
      "",
    ].join("\n"),
  );
  variables = {
    HOME: join(root, "home"),
    HD_CACHE: join(root, "cache"),
    GIT_CONFIG_GLOBAL: config,
    GIT_CONFIG_NOSYSTEM: "1",
    PATH: process.env.PATH ?? "",
  };
  await remote("text", [
    ["v1.0.0", TEXT_V1],
    ["v1.1.0", TEXT_V1_1],
    ["v1.2.0-rc.1", { "NOTES.md": "release candidate\n" }],
    ["v2.0.0", { "src/lib.hd": 'pub fn shout(word: string) -> string:\n    word + "!!"\n' }],
  ]);
}

after(async () => {
  if (root) await removeTree(root);
});

describe("dependencies", needsGit, () => {
  before(setUp);

  test("hd add fetches a tag, records its hash, and dep.NAME runs", async () => {
    const directory = await app("greeter", SHOUT_MAIN, "\n# kept\n[dependencies]\n");
    const added = await hd(directory, ["add", "text", "github.com/acme/text@1.0.0"]);
    assert.equal(added.status, 0, added.stderr);
    assert.match(added.stderr, /hd: fetching github\.com\/acme\/text@1\.0\.0/);
    const manifest = await readFile(join(directory, "hd.toml"), "utf8");
    assert.equal(
      manifest,
      '[package]\nname = "greeter"\n\n# kept\n[dependencies]\ntext = "github.com/acme/text@1.0.0"\n',
    );
    assert.equal(
      await readFile(join(directory, "hd.sum"), "utf8"),
      selectedLines("github.com/acme/text@1.0.0", TEXT_V1),
    );
    // The entry is read-only once written (cli.cache.read-only).
    const entry = join(root, "cache", "pkg", "github.com", "acme", "text@1.0.0");
    assert.equal((await stat(join(entry, "src", "lib.hd"))).mode & 0o222, 0);
    const ran = await hd(directory, ["run"]);
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "hello!\n");
  });

  test("a cached version needs no git, and a private declaration stays private", async () => {
    const directory = join(root, "apps", "greeter");
    // With no git on PATH, a command that tried to fetch would fail.
    const empty = join(root, "empty-path");
    await mkdir(empty, { recursive: true });
    const checked = await hd(directory, ["check"], { PATH: empty });
    assert.equal(checked.status, 0, checked.stderr);
    assert.doesNotMatch(checked.stderr, /fetching/);
    await writeFile(
      join(directory, "src", "main.hd"),
      "use dep.text.{secret}\n\npub fn main() -> void $ Console:\n    println(secret())\n",
    );
    const leaked = await hd(directory, ["check", "--format", "json"], { PATH: empty });
    assert.equal(leaked.status, 101);
    assert.match(leaked.stdout, /"code":"private-import"/);
    await writeFile(join(directory, "src", "main.hd"), SHOUT_MAIN);
  });

  test("hd check fetches a missing version implicitly and verifies it", async () => {
    const directory = join(root, "apps", "greeter");
    const fresh = join(root, "cache-fresh");
    const checked = await hd(directory, ["check"], { HD_CACHE: fresh });
    assert.equal(checked.status, 0, checked.stderr);
    assert.match(checked.stderr, /hd: fetching github\.com\/acme\/text@1\.0\.0/);
    assert.ok(existsSync(join(fresh, "pkg", "github.com", "acme", "text@1.0.0", "hd.toml")));
    await removeTree(fresh);
  });

  test("a tampered hd.sum entry or a moved tag is sum-mismatch, never a warning", async () => {
    const directory = join(root, "apps", "greeter");
    const sum = join(directory, "hd.sum");
    const good = await readFile(sum, "utf8");
    const fake = `h1:${Buffer.alloc(32, 7).toString("base64")}`;
    const forged = `github.com/acme/text@1.0.0 ${fake}\n${manifestLine("github.com/acme/text@1.0.0", TEXT_V1)}`;
    await writeFile(sum, forged);
    const mismatched = await hd(directory, ["check", "--format", "json"]);
    assert.equal(mismatched.status, 101);
    assert.match(mismatched.stdout, /"code":"sum-mismatch","severity":"error"/);
    assert.match(mismatched.stdout, /"file":"hd.toml","line":6/);
    // hd fetch never replaces an entry (cli.sum.keep).
    assert.equal((await hd(directory, ["fetch"])).status, 101);
    assert.equal(await readFile(sum, "utf8"), forged);
    // A manifest line is checked too, before selection reads the manifest
    // (cli.dep.verify.manifest).
    await writeFile(sum, `github.com/acme/text@1.0.0 ${expectedHash(TEXT_V1)}\n`);
    const unread = await hd(directory, ["check", "--format", "json"], { PATH: "" });
    assert.match(unread.stdout, /"code":"missing-sum-entry"/);
    assert.match(unread.stdout, /text@1\.0\.0\/hd\.toml/);
    await writeFile(
      sum,
      `github.com/acme/text@1.0.0 ${expectedHash(TEXT_V1)}\ngithub.com/acme/text@1.0.0/hd.toml ${fake}\n`,
    );
    const forgedManifest = await hd(directory, ["check", "--format", "json"]);
    assert.match(forgedManifest.stdout, /"code":"sum-mismatch"/);
    assert.match(forgedManifest.stdout, /fetched manifest's hash/);
    await writeFile(sum, good);
    // The tag moves to other code; a fresh cache fetches it and the hash differs.
    const moved = await remote("moving", [["v1.0.0", TEXT_V1]]);
    const other = await app(
      "mover",
      SHOUT_MAIN,
      '\n[dependencies]\ntext = "github.com/acme/moving@1.0.0"\n',
    );
    assert.equal((await hd(other, ["fetch"])).status, 0);
    await writeFile(
      join(moved, "src", "lib.hd"),
      'pub fn shout(word: string) -> string:\n    "pwned"\n',
    );
    git(moved, "commit", "--quiet", "--all", "-m", "moved");
    git(moved, "tag", "--force", "v1.0.0");
    const fresh = await hd(other, ["check"], { HD_CACHE: join(root, "cache-moved") });
    assert.equal(fresh.status, 101);
    assert.match(fresh.stderr, /sum-mismatch/);
  });

  test("a version without an hd.sum entry is missing-sum-entry until hd fetch", async () => {
    const directory = join(root, "apps", "greeter");
    await rm(join(directory, "hd.sum"));
    const checked = await hd(directory, ["check", "--format", "json"], { PATH: "" });
    assert.equal(checked.status, 101);
    assert.match(checked.stdout, /"code":"missing-sum-entry"/);
    assert.match(checked.stdout, /hd fetch/);
    const fetched = await hd(directory, ["fetch"]);
    assert.equal(fetched.status, 0, fetched.stderr);
    assert.equal(
      await readFile(join(directory, "hd.sum"), "utf8"),
      selectedLines("github.com/acme/text@1.0.0", TEXT_V1),
    );
    assert.equal((await hd(directory, ["check"])).status, 0);
  });

  test("hd update moves to the newest release on the line and tidies hd.sum", async () => {
    const directory = join(root, "apps", "greeter");
    const updated = await hd(directory, ["update", "text"]);
    assert.equal(updated.status, 0, updated.stderr);
    assert.match(updated.stdout, /text: 1\.0\.0 -> 1\.1\.0/);
    // Not 2.0.0, another line, nor 1.2.0-rc.1, a pre-release (cli.dep.update.release).
    assert.match(
      await readFile(join(directory, "hd.toml"), "utf8"),
      /text = "github.com\/acme\/text@1.1.0"/,
    );
    assert.equal(
      await readFile(join(directory, "hd.sum"), "utf8"),
      selectedLines("github.com/acme/text@1.1.0", TEXT_V1_1),
    );
    const again = await hd(directory, ["update"]);
    assert.equal(again.status, 0, again.stderr);
    assert.equal(again.stdout, "");
  });

  test("an unknown tag is unknown-version, and hd add then writes nothing", async () => {
    const directory = join(root, "apps", "greeter");
    const before = await readFile(join(directory, "hd.toml"), "utf8");
    const added = await hd(directory, [
      "add",
      "--format",
      "json",
      "text",
      "github.com/acme/text@1.9.0",
    ]);
    assert.equal(added.status, 101);
    assert.match(added.stdout, /"code":"unknown-version"/);
    assert.match(added.stdout, /1\.0\.0, 1\.1\.0/);
    assert.equal(await readFile(join(directory, "hd.toml"), "utf8"), before);
  });

  test("an unreachable repository is fetch-failed, naming git's credentials", async () => {
    const directory = join(root, "apps", "greeter");
    const added = await hd(directory, ["add", "secret", "github.com/acme/private@1.0.0"]);
    assert.equal(added.status, 101);
    assert.match(added.stderr, /fetch-failed: cannot fetch github\.com\/acme\/private@1\.0\.0/);
    assert.match(added.stderr, /git's own credentials/);
    assert.doesNotMatch(await readFile(join(directory, "hd.toml"), "utf8"), /secret/);
    assert.equal(
      maskCredentials("fatal: could not read https://bot:t0ken@github.com/acme/private"),
      "fatal: could not read https://***@github.com/acme/private",
    );
  });

  test("a subdirectory package uses its tag prefix, and selection takes the largest minimum", async () => {
    await remote("tools", [
      [
        "lint/v0.3.0",
        {
          "README.md": "tools\n",
          "lint/hd.toml":
            '[package]\nname = "lint"\n\n[dependencies]\ntext = "github.com/acme/text@1.1.0"\n',
          "lint/src/lib.hd":
            "use dep.text.{whisper}\n\npub fn hint(word: string) -> string:\n    whisper(word)\n",
        },
      ],
    ]);
    const directory = await app(
      "linter",
      'use dep.lint.{hint}\nuse dep.text.{shout}\n\npub fn main() -> void $ Console:\n    println(hint(shout("x")))\n',
      '\n[dependencies]\ntext = "github.com/acme/text@1.0.0"\n',
    );
    assert.equal((await hd(directory, ["fetch"])).status, 0);
    const added = await hd(directory, ["add", "lint", "github.com/acme/tools/lint@0.3.0"]);
    assert.equal(added.status, 0, added.stderr);
    // lint requires text 1.1.0, so 1.1.0 is selected and 1.0.0's tree line
    // goes. Selection still reads 1.0.0's manifest, so its manifest line stays.
    const lint = {
      "hd.toml":
        '[package]\nname = "lint"\n\n[dependencies]\ntext = "github.com/acme/text@1.1.0"\n',
      "src/lib.hd":
        "use dep.text.{whisper}\n\npub fn hint(word: string) -> string:\n    whisper(word)\n",
    };
    const sum = await readFile(join(directory, "hd.sum"), "utf8");
    assert.equal(
      sum,
      manifestLine("github.com/acme/text@1.0.0", TEXT_V1) +
        selectedLines("github.com/acme/text@1.1.0", TEXT_V1_1) +
        selectedLines("github.com/acme/tools/lint@0.3.0", lint),
    );
    assert.equal(
      formatSum(
        new Map(
          sum
            .trim()
            .split("\n")
            .reverse()
            .map((line) => line.split(" ") as [string, string]),
        ),
      ),
      sum,
    );
    const ran = await hd(directory, ["run"]);
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "(x!)\n");
    const removed = await hd(directory, ["remove", "lint"]);
    assert.equal(removed.status, 0, removed.stderr);
    assert.doesNotMatch(await readFile(join(directory, "hd.toml"), "utf8"), /^lint =/m);
    assert.equal(
      await readFile(join(directory, "hd.sum"), "utf8"),
      selectedLines("github.com/acme/text@1.0.0", TEXT_V1),
    );
  });

  test("a fetched package's dbg prints nothing and warns once; a path package's prints", async () => {
    await remote("noisy", [
      [
        "v1.0.0",
        {
          "hd.toml": '[package]\nname = "noisy"\n',
          "src/lib.hd":
            "pub fn twice(n: i32) -> i32:\n    dbg(n)\n    n * 2\n\npub fn thrice(n: i32) -> i32:\n    dbg(n)\n    n * 3\n",
        },
      ],
    ]);
    const directory = await app(
      "debugger",
      "use dep.noisy.{twice, thrice}\nuse dep.local.{inc}\n\npub fn main() -> void $ Console:\n    result := twice(thrice(inc(1)))\n    dbg(result)\n    println(result)\n",
      '\n[dependencies]\nnoisy = "github.com/acme/noisy@1.0.0"\nlocal = { path = "local" }\n',
    );
    await writeTree(join(directory, "local"), {
      "hd.toml": '[package]\nname = "local"\n',
      "src/lib.hd": "pub fn inc(n: i32) -> i32:\n    dbg(n + 1)\n    n + 1\n",
    });
    const fetched = await hd(directory, ["fetch"]);
    assert.equal(fetched.status, 0, fetched.stderr);
    const ran = await hd(directory, ["run"]);
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "12\n");
    // The package and its path dependency print (module.dbg.own-code); the
    // fetched package prints nothing and warns once (module.dbg.dependency).
    assert.match(ran.stderr, /local\/src\/lib\.hd:2:5: n \+ 1 = 2$/m);
    assert.match(ran.stderr, /src\/main\.hd:6:5: result = 12$/m);
    assert.doesNotMatch(ran.stderr, /: n = /);
    const checked = await hd(directory, ["check"]);
    assert.equal(checked.status, 0, checked.stderr);
    assert.equal(checked.stderr.match(/dbg-in-dependency/g)?.length, 1, checked.stderr);
    assert.match(checked.stderr, /dependency github\.com\/acme\/noisy@1\.0\.0 calls dbg/);
    // A release build rejects the package's and the path dependency's calls only.
    const release = await hd(directory, ["build", "--release"]);
    assert.equal(release.status, 101, release.stderr);
    assert.equal(release.stderr.match(/dbg-in-release/g)?.length, 2, release.stderr);
  });
});

describe("pseudo-versions and workspaces", needsGit, () => {
  before(setUp);

  test("a pseudo-version fetches its commit, and a wrong hash or time is unknown-version", async () => {
    const draft = await remote("draft", [["v0.4.0", TEXT_V1]]);
    await writeFile(
      join(draft, "src", "lib.hd"),
      'pub fn shout(word: string) -> string:\n    word + "?"\n',
    );
    git(draft, "commit", "--quiet", "--all", "-m", "untagged");
    const [hash, seconds] = git(draft, "log", "-1", "--format=%H %ct").trim().split(" ");
    const time = new Date(Number(seconds) * 1000).toISOString().replace(/\.\d{3}Z$|[-T:]/g, "");
    // An untagged commit after v0.4.0 is 0.4.1-0.TIME-HASH (module.version.pseudo).
    const pseudo = `0.4.1-0.${time}-${hash!.slice(0, 12)}`;
    const directory = await app("drafter", SHOUT_MAIN, "\n[dependencies]\n");
    const added = await hd(directory, ["add", "text", `github.com/acme/draft@${pseudo}`]);
    assert.equal(added.status, 0, added.stderr);
    assert.match(
      await readFile(join(directory, "hd.sum"), "utf8"),
      new RegExp(`^github\\.com/acme/draft@${pseudo.replaceAll(".", "\\.")} h1:`),
    );
    const ran = await hd(directory, ["run"]);
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "hello?\n");
    // A pseudo-version whose TIME is not its commit's time names no commit
    // (module.version.pseudo-missing), and neither does an unknown HASH.
    const wrongTime = pseudo.replace(time, "20000101000000");
    const timed = await hd(directory, ["add", "text", `github.com/acme/draft@${wrongTime}`]);
    assert.equal(timed.status, 101);
    assert.match(timed.stderr, /unknown-version/);
    assert.match(timed.stderr, new RegExp(`did you mean ${pseudo.replaceAll(".", "\\.")}`));
    const unknown = await hd(directory, [
      "add",
      "text",
      `github.com/acme/draft@0.4.1-0.${time}-000000000000`,
    ]);
    assert.equal(unknown.status, 101);
    assert.match(unknown.stderr, /unknown-version: github\.com\/acme\/draft has no commit/);
    assert.match(await readFile(join(directory, "hd.toml"), "utf8"), new RegExp(pseudo));
  });

  test("a workspace selects once for every member, and root commands act on the members", async () => {
    const workspace = join(root, "shop");
    await writeTree(workspace, { "hd.toml": "[workspace]\nmembers = [\n]\n" });
    // hd new inside a workspace adds the member (cli.new.workspace-member).
    for (const [kind, path] of [
      ["--lib", "libs/util"],
      ["--app", "apps/web"],
    ] as const) {
      const created = await hd(workspace, ["new", kind, "--vcs", "none", path]);
      assert.equal(created.status, 0, created.stderr);
      assert.match(created.stdout, new RegExp(`Added "${path}" to the members of hd.toml`));
    }
    assert.equal(
      await readFile(join(workspace, "hd.toml"), "utf8"),
      '[workspace]\nmembers = [\n    "libs/util",\n    "apps/web",\n]\n',
    );
    // web requires text 1.0.0 and util requires 1.1.0, in one hd.sum.
    const web = join(workspace, "apps", "web");
    const util = join(workspace, "libs", "util");
    assert.equal((await hd(web, ["add", "text", "github.com/acme/text@1.0.0"])).status, 0);
    assert.equal((await hd(util, ["add", "text", "github.com/acme/text@1.1.0"])).status, 0);
    assert.ok(!existsSync(join(web, "hd.sum")));
    assert.equal(
      await readFile(join(workspace, "hd.sum"), "utf8"),
      manifestLine("github.com/acme/text@1.0.0", TEXT_V1) +
        selectedLines("github.com/acme/text@1.1.0", TEXT_V1_1),
    );
    await writeFile(
      join(web, "hd.toml"),
      '[package]\nname = "web"\n\n[dependencies]\ntext = "github.com/acme/text@1.0.0"\nutil = { path = "../../libs/util" }\n',
    );
    // whisper exists only in text 1.1.0, which the workspace selects for web too.
    await writeFile(
      join(web, "src", "main.hd"),
      'use dep.text.{whisper}\nuse dep.util.{greet}\n\npub fn main() -> void $ Console:\n    println(whisper(greet("world")))\n',
    );
    await writeFile(
      join(web, "tests", "web.hd"),
      'use std.testing.{assert_equal, hd_run}\n\nit("prints a quiet greeting"):\n    let out = hd_run!("web")\n    assert_equal(out.stdout, "(hello, world)\\n", reason="the greeting")\n',
    );
    const offline = { PATH: join(root, "empty-path") };
    const checked = await hd(workspace, ["check"], offline);
    assert.equal(checked.status, 0, checked.stderr);
    assert.equal(checked.stdout, "util: ok\nweb: ok\n");
    const tested = await hd(workspace, ["test"]);
    assert.equal(tested.status, 0, tested.stderr + tested.stdout);
    assert.match(tested.stdout, /libs\/util\/tests\/util\.hd: 1 passed/);
    assert.match(tested.stdout, /apps\/web\/tests\/web\.hd: 1 passed/);
    const ran = await hd(workspace, ["run", "web"], offline);
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "(hello, world)\n");
    // A bare hd run at the root names no program (cli.workspace.run-bare).
    const bare = await hd(workspace, ["run"], offline);
    assert.equal(bare.status, 101);
    assert.match(bare.stderr, /util: none\n {2}web: web/);
    // -p selects members, from the root or inside a member (cli.workspace.select.*).
    const selected = await hd(util, ["check", "-p", "web"], offline);
    assert.equal(selected.stdout, "web: ok\n");
    const unknown = await hd(workspace, ["test", "-p", "shop"], offline);
    assert.equal(unknown.status, 101);
    assert.match(unknown.stderr, /no member named 'shop'; its members are util, web/);
    const runP = await hd(workspace, ["run", "-p", "web"], offline);
    assert.equal(runP.stdout, "(hello, world)\n");
  });

  test("hd doc dep.KEY.ITEM finds a dependency's pub item only", async () => {
    // spec/cli/command-line.md#r-cli.doc.name.dependency
    const directory = await app("reader", SHOUT_MAIN, "\n[dependencies]\n");
    assert.equal((await hd(directory, ["add", "text", "github.com/acme/text@1.1.0"])).status, 0);
    const found = await hd(directory, ["doc", "dep.text.whisper"]);
    assert.equal(found.status, 0, found.stderr);
    assert.match(found.stdout, /pub fn whisper\(word: string\) -> string/);
    const hidden = await hd(directory, ["doc", "dep.text.secret"]);
    assert.equal(hidden.status, 1);
    assert.match(hidden.stderr, /no symbol named dep\.text\.secret/);
    const unknown = await hd(directory, ["doc", "dep.json.parse"]);
    assert.match(unknown.stderr, /package 'reader' has no dependency named 'json'/);
  });

  test("a path requirement links another member of the workspace", async () => {
    const workspace = join(root, "ws");
    await writeTree(workspace, {
      "hd.toml": '[workspace]\nmembers = ["app", "util"]\n',
      "util/hd.toml": '[package]\nname = "util"\n',
      "util/src/lib.hd": "pub fn twice(n: i32) -> i32:\n    n * 2\n",
      "app/hd.toml": '[package]\nname = "app"\n\n[dependencies]\nutil = { path = "../util" }\n',
      "app/src/main.hd":
        "use dep.util.{twice}\n\npub fn main() -> void $ Console:\n    println(twice(21))\n",
      "loose/hd.toml": '[package]\nname = "loose"\n',
      "loose/src/main.hd": "pub fn main() -> void $ Console:\n    println(1)\n",
    });
    const ran = await hd(join(workspace, "app"), ["run"], { PATH: "" });
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "42\n");
    assert.ok(!existsSync(join(workspace, "hd.sum")));
    // loose is under the workspace manifest, which neither lists nor
    // excludes it (cli.mode.member.unlisted), with two fix-its
    // (cli.mode.member.unlisted.fix).
    const loose = await hd(join(workspace, "loose"), ["check", "--format", "json"]);
    assert.equal(loose.status, 101);
    const [diagnostic] = loose.stdout.split("\n").map((line) => JSON.parse(line || "{}"));
    assert.equal(diagnostic.code, null);
    assert.equal(diagnostic.file, "../hd.toml");
    assert.equal(diagnostic.fix, null);
    assert.deepEqual(
      diagnostic.fixes.map((fix: { message: string; edits: { replacement: string }[] }) => [
        fix.message,
        fix.edits.map((edit) => edit.replacement).join(""),
      ]),
      [
        ['add "loose" to members', ', "loose"'],
        ['add "loose" to exclude', 'exclude = ["loose"]\n'],
      ],
    );
    const text = await hd(join(workspace, "loose"), ["run"]);
    assert.equal(text.status, 101);
    assert.match(text.stderr, /lists it neither in members nor in exclude/);
    assert.match(
      text.stderr,
      / {2}fix-it: add "loose" to members\n {2}fix-it: add "loose" to exclude/,
    );
    // The dependency commands refuse it too, and write nothing.
    assert.equal((await hd(join(workspace, "loose"), ["fetch"])).status, 101);
    // Excluded, it is a package of its own (cli.mode.member.excluded).
    await writeFile(
      join(workspace, "hd.toml"),
      '[workspace]\nmembers = ["app", "util"]\nexclude = ["loose"]\n',
    );
    assert.equal((await hd(join(workspace, "loose"), ["run"])).stdout, "1\n");
  });

  test("a path requirement names a package outside any workspace, as Cargo's does", async () => {
    // spec/lang/10-modules.md#path-requirements: money is a sibling, in no
    // workspace; its requirement on text joins the selection of shop, whose
    // hd.sum alone records it. money's own hd.sum and dev dependencies are
    // never read.
    const money = join(root, "local", "money");
    await writeTree(money, {
      "hd.toml":
        '[package]\nname = "money"\n\n[dependencies]\ntext = "github.com/acme/text@1.1.0"\n\n[dev-dependencies]\nfixtures = "github.com/acme/missing@9.9.9"\n',
      "hd.sum": "github.com/acme/text@1.1.0 h1:not-read\n",
      "src/lib.hd":
        'use dep.text.{whisper}\n\npub fn price(cents: i32) -> string:\n    whisper("${cents} cents")\n',
    });
    const shop = join(root, "local", "shop");
    await writeTree(shop, {
      "hd.toml":
        '[package]\nname = "shop"\n\n[dependencies]\nmoney = { path = "../money" }\ntext = "github.com/acme/text@1.0.0"\n',
      "src/main.hd":
        "use dep.money.{price}\nuse dep.text.{shout}\n\npub fn main() -> void $ Console:\n    println(shout(price(250)))\n",
    });
    const checked = await hd(shop, ["check", "--format", "json"], { PATH: "" });
    assert.equal(checked.status, 101);
    assert.match(checked.stdout, /"code":"missing-sum-entry"/);
    const fetched = await hd(shop, ["fetch"]);
    assert.equal(fetched.status, 0, fetched.stderr);
    assert.equal(
      await readFile(join(shop, "hd.sum"), "utf8"),
      manifestLine("github.com/acme/text@1.0.0", TEXT_V1) +
        selectedLines("github.com/acme/text@1.1.0", TEXT_V1_1),
    );
    const ran = await hd(shop, ["run"], { PATH: join(root, "empty-path") });
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "(250 cents)!\n");
    // A path that holds no package is invalid (module.path-dep.no-package).
    await writeFile(
      join(shop, "hd.toml"),
      '[package]\nname = "shop"\n\n[dependencies]\nmoney = { path = "../nowhere" }\n',
    );
    const missing = await hd(shop, ["check"]);
    assert.equal(missing.status, 101);
    assert.match(
      missing.stderr,
      /invalid-requirement: money: the path requirement names \.\.\/nowhere/,
    );
  });

  test("hd fetch at a workspace root fetches every member's selection", async () => {
    // spec/cli/command-line.md#r-cli.dep.workspace-fetch
    const workspace = join(root, "ci");
    await writeTree(workspace, {
      "hd.toml": '[workspace]\nmembers = ["web", "util"]\n',
      "web/hd.toml":
        '[package]\nname = "web"\n\n[dependencies]\ntext = "github.com/acme/text@1.0.0"\n',
      "web/src/main.hd": SHOUT_MAIN,
      "util/hd.toml":
        '[package]\nname = "util"\n\n[dependencies]\ntext = "github.com/acme/text@1.1.0"\n',
      "util/src/lib.hd": "pub fn one() -> i32:\n    1\n",
    });
    const fresh = { HD_CACHE: join(root, "cache-ci") };
    const fetched = await hd(workspace, ["fetch"], fresh);
    assert.equal(fetched.status, 0, fetched.stderr);
    assert.match(fetched.stderr, /hd: fetching github\.com\/acme\/text@1\.1\.0/);
    assert.equal(
      await readFile(join(workspace, "hd.sum"), "utf8"),
      manifestLine("github.com/acme/text@1.0.0", TEXT_V1) +
        selectedLines("github.com/acme/text@1.1.0", TEXT_V1_1),
    );
    const offline = await hd(join(workspace, "web"), ["run"], { ...fresh, PATH: "" });
    assert.equal(offline.stdout, "hello!\n");
    // The commands that edit a requirement stay member-only (cli.dep.workspace-member-only).
    const added = await hd(workspace, ["add", "text", "github.com/acme/text@1.0.0"]);
    assert.equal(added.status, 101);
    assert.match(added.stderr, /run it inside that member's directory/);
    // hd add may lower a requirement, and says so (cli.dep.add.lower); the
    // other member's 1.1.0 still wins the selection (cli.dep.add.lower.selection).
    const util = join(workspace, "util");
    const lowered = await hd(util, ["add", "text", "github.com/acme/text@1.0.0"], fresh);
    assert.equal(lowered.status, 0, lowered.stderr);
    assert.equal(lowered.stdout, "lowered text 1.1.0 -> 1.0.0\n");
    assert.match(await readFile(join(util, "hd.toml"), "utf8"), /text@1\.0\.0"/);
    assert.equal(
      await readFile(join(workspace, "hd.sum"), "utf8"),
      selectedLines("github.com/acme/text@1.0.0", TEXT_V1),
    );
    const raised = await hd(util, ["add", "text", "github.com/acme/text@1.1.0"], fresh);
    assert.equal(raised.stdout, 'text = "github.com/acme/text@1.1.0"\n');
    await removeTree(join(root, "cache-ci"));
  });

  test("a pseudo-version's base tag must precede its commit, as Go checks", async () => {
    // spec/cli/command-line.md#r-cli.dep.pseudo.base
    const repo = await remote("based", [
      ["v0.4.0", TEXT_V1],
      ["v0.5.0", { "NOTES.md": "five\n" }],
    ]);
    const commit = async (file: string): Promise<string> => {
      await writeFile(join(repo, file), `${file}\n`);
      git(repo, "add", "--all");
      git(repo, "commit", "--quiet", "-m", file);
      const [hash, seconds] = git(repo, "log", "-1", "--format=%H %ct").trim().split(" ");
      const time = new Date(Number(seconds) * 1000).toISOString().replace(/\.\d{3}Z$|[-T:]/g, "");
      return `${time}-${hash!.slice(0, 12)}`;
    };
    const tagged = git(repo, "log", "-1", "--format=%H %ct").trim().split(" ");
    const taggedCommit = `${new Date(Number(tagged[1]) * 1000).toISOString().replace(/\.\d{3}Z$|[-T:]/g, "")}-${tagged[0]!.slice(0, 12)}`;
    const after = await commit("after.txt");
    git(repo, "tag", "v0.6.0-rc.1");
    const rc = await commit("rc.txt");
    git(repo, "checkout", "--quiet", "-b", "side", "v0.4.0");
    const side = await commit("side.txt");
    git(repo, "checkout", "--quiet", "main");
    const directory = await app("pseudo-base", SHOUT_MAIN, "\n[dependencies]\n");
    const add = (version: string): Promise<HdResult> =>
      hd(directory, ["add", "text", `github.com/acme/based@${version}`]);
    // Any earlier tag on the commit's history may be the base, not only the
    // closest; 0.0.0-TIME-HASH has none.
    for (const version of [
      `0.5.1-0.${after}`,
      `0.4.1-0.${after}`,
      `0.6.0-rc.1.0.${rc}`,
      `0.0.0-${rc}`,
    ]) {
      const added = await add(version);
      assert.equal(added.status, 0, `${version}: ${added.stderr}`);
    }
    const rejected: [string, RegExp][] = [
      [`0.6.1-0.${after}`, /has no tag v0\.6\.0, which the pseudo-version/],
      [`0.5.1-0.${side}`, /does not descend from the tag v0\.5\.0/],
      [`0.5.1-0.${taggedCommit}`, /is the tag v0\.5\.0 itself, so require 0\.5\.0 instead/],
    ];
    for (const [version, message] of rejected) {
      const added = await add(version);
      assert.equal(added.status, 101, version);
      assert.match(added.stderr, /unknown-version/);
      assert.match(added.stderr, message);
    }
    // X.Y.0-0.TIME-HASH follows no release, so it is no pseudo-version and needs a tag.
    const zero = await add(`0.5.0-0.${after}`);
    assert.match(zero.stderr, /has no tag v0\.5\.0-0\./);
  });
});

test("host paths name a repository and a package directory", () => {
  assert.deepEqual(parseHostPath("github.com/acme/tools/lint"), {
    path: "github.com/acme/tools/lint",
    repository: "github.com/acme/tools",
    subdirectory: "lint",
  });
  assert.deepEqual(parseHostPath("git.example.com/shop/billing.git/core"), {
    path: "git.example.com/shop/billing.git/core",
    repository: "git.example.com/shop/billing.git",
    subdirectory: "core",
  });
  for (const invalid of [
    "github.com/acme",
    "example.com/shop/json",
    "github.com/acme/json/v2",
    "acme/json",
  ])
    assert.equal(typeof parseHostPath(invalid), "string", invalid);
  assert.match(parseHostRequirement("github.com/acme/json@v2.1.0") as string, /leading 'v'/);
});

test("versions order by SemVer and fall into compatibility lines", () => {
  const order = ["0.4.1", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-beta", "1.0.0", "1.2.0", "10.0.0"];
  const versions = order.map((text) => parseVersion(text)!);
  for (let index = 1; index < versions.length; index += 1)
    assert.ok(compareVersions(versions[index - 1]!, versions[index]!) < 0, order[index]);
  assert.equal(compatibilityLine(parseVersion("0.4.1")!), "0.4");
  assert.equal(compatibilityLine(parseVersion("2.3.0")!), "2");
  assert.equal(parseVersion("1.2"), undefined);
});

test("a pseudo-version takes one of Go's three forms", () => {
  const commit = { time: "20260912081500", hash: "3f2c9e1a7b6d" };
  for (const text of [
    "0.0.0-20260912081500-3f2c9e1a7b6d",
    "0.4.1-0.20260912081500-3f2c9e1a7b6d",
    "1.0.0-rc.1.0.20260912081500-3f2c9e1a7b6d",
  ])
    assert.deepEqual(pseudoCommit(parseVersion(text)!), commit, text);
  for (const text of [
    "1.0.0-20260912081500-3f2c9e1a7b6d",
    "0.4.1-1.20260912081500-3f2c9e1a7b6d",
    "0.4.1-0.20260912081500-3F2C9E1A7B6D",
    "0.4.1-rc.1",
    // A release base has patch Z, so the pseudo-version's patch is Z+1 >= 1.
    "0.4.0-0.20260912081500-3f2c9e1a7b6d",
  ])
    assert.equal(pseudoCommit(parseVersion(text)!), undefined, text);
  // Each form names its base tag's version (cli.dep.pseudo.base).
  const base = (text: string): string | undefined => pseudoBase(parseVersion(text)!)?.text;
  assert.equal(base("0.0.0-20260912081500-3f2c9e1a7b6d"), undefined);
  assert.equal(base("0.4.1-0.20260912081500-3f2c9e1a7b6d"), "0.4.0");
  assert.equal(base("1.0.0-rc.1.0.20260912081500-3f2c9e1a7b6d"), "1.0.0-rc.1");
  // A pseudo-version is a pre-release, so it orders below the release it precedes.
  assert.ok(
    compareVersions(parseVersion("0.4.1-0.20260912081500-3f2c9e1a7b6d")!, parseVersion("0.4.1")!) <
      0,
  );
});

test("dependency commands edit hd.toml line by line", () => {
  const text =
    '[package]\nname = "a"  # the app\n\n[dependencies]\njson = "github.com/x/json@1.0.0"  # pinned\n\n[[executable]]\nname = "a"\nmodule = "main"\n';
  assert.equal(
    setDependency(text, "json", "github.com/x/json@1.1.0"),
    text.replace("json@1.0.0", "json@1.1.0"),
  );
  assert.equal(
    setDependency(text, "yaml", "github.com/x/yaml@0.1.0"),
    text.replace("# pinned\n", '# pinned\nyaml = "github.com/x/yaml@0.1.0"\n'),
  );
  assert.equal(
    removeDependency(text, "json"),
    text.replace('json = "github.com/x/json@1.0.0"  # pinned\n', ""),
  );
  assert.equal(removeDependency(text, "yaml"), undefined);
  // hd new adds a member to the array as it is written (cli.new.workspace-member).
  assert.equal(
    addWorkspaceMember('[workspace]\nmembers = ["a"]  # all\n', "b"),
    '[workspace]\nmembers = ["a", "b"]  # all\n',
  );
  assert.equal(
    addWorkspaceMember("[workspace]\nmembers = []\n", "b"),
    '[workspace]\nmembers = ["b"]\n',
  );
  assert.equal(
    addWorkspaceMember('[workspace]\nmembers = [\n  "a"  # first\n]\n', "b"),
    '[workspace]\nmembers = [\n  "a",  # first\n  "b",\n]\n',
  );
  assert.equal(
    addWorkspaceMember('[workspace]\nexclude = ["x"]\n', "b"),
    '[workspace]\nmembers = ["b"]\nexclude = ["x"]\n',
  );
  assert.equal(addWorkspaceMember('[package]\nname = "a"\n', "b"), undefined);
  // The unlisted-member fix-its add to members or to exclude (cli.mode.member.unlisted.fix).
  assert.equal(
    addWorkspaceEntry('[workspace]\nmembers = ["a"]\n\n[other]\n', "exclude", "b"),
    '[workspace]\nmembers = ["a"]\nexclude = ["b"]\n\n[other]\n',
  );
  assert.equal(
    addWorkspaceEntry('[workspace]\nmembers = ["a"]\nexclude = ["x"]\n', "exclude", "b"),
    '[workspace]\nmembers = ["a"]\nexclude = ["x", "b"]\n',
  );
});
