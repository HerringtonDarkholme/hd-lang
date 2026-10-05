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
import { removeDependency, setDependency } from "../src/dependencies/manifest-edit.ts";
import {
  compareVersions,
  compatibilityLine,
  parseHostPath,
  parseHostRequirement,
  parseVersion,
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

describe("dependencies", { skip: !hasGit && "git is not installed" }, () => {
  before(async () => {
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
  });

  after(async () => {
    if (root) await removeTree(root);
  });

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
      `github.com/acme/text@1.0.0 ${expectedHash(TEXT_V1)}\n`,
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
    const forged = `github.com/acme/text@1.0.0 h1:${Buffer.alloc(32, 7).toString("base64")}\n`;
    await writeFile(sum, forged);
    const mismatched = await hd(directory, ["check", "--format", "json"]);
    assert.equal(mismatched.status, 101);
    assert.match(mismatched.stdout, /"code":"sum-mismatch","severity":"error"/);
    assert.match(mismatched.stdout, /"file":"hd.toml","line":6/);
    // hd fetch never replaces an entry (cli.sum.keep).
    assert.equal((await hd(directory, ["fetch"])).status, 101);
    assert.equal(await readFile(sum, "utf8"), forged);
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
      `github.com/acme/text@1.0.0 ${expectedHash(TEXT_V1)}\n`,
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
      `github.com/acme/text@1.1.0 ${expectedHash(TEXT_V1_1)}\n`,
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
    // lint requires text 1.1.0, so 1.1.0 is selected and 1.0.0's entry goes.
    const sum = await readFile(join(directory, "hd.sum"), "utf8");
    assert.equal(
      sum,
      formatSum(
        new Map([
          ["github.com/acme/text@1.1.0", expectedHash(TEXT_V1_1)],
          [
            "github.com/acme/tools/lint@0.3.0",
            expectedHash({
              "hd.toml":
                '[package]\nname = "lint"\n\n[dependencies]\ntext = "github.com/acme/text@1.1.0"\n',
              "src/lib.hd":
                "use dep.text.{whisper}\n\npub fn hint(word: string) -> string:\n    whisper(word)\n",
            }),
          ],
        ]),
      ),
    );
    const ran = await hd(directory, ["run"]);
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "(x!)\n");
    const removed = await hd(directory, ["remove", "lint"]);
    assert.equal(removed.status, 0, removed.stderr);
    assert.doesNotMatch(await readFile(join(directory, "hd.toml"), "utf8"), /^lint =/m);
    assert.equal(
      await readFile(join(directory, "hd.sum"), "utf8"),
      `github.com/acme/text@1.0.0 ${expectedHash(TEXT_V1)}\n`,
    );
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
      "loose/hd.toml": '[package]\nname = "loose"\n\n[dependencies]\nutil = { path = "../util" }\n',
      "loose/src/main.hd": "pub fn main() -> void $ Console:\n    println(1)\n",
    });
    const ran = await hd(join(workspace, "app"), ["run"], { PATH: "" });
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "42\n");
    assert.ok(!existsSync(join(workspace, "hd.sum")));
    // loose is no member, so its path requirement is invalid.
    const loose = await hd(join(workspace, "loose"), ["check", "--format", "json"]);
    assert.equal(loose.status, 101);
    assert.match(loose.stdout, /"code":"invalid-requirement"/);
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
});
