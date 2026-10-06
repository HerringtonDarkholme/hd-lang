import assert from "node:assert/strict";
import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  capabilityFlags,
  coversName,
  coversPath,
  grantsOf,
  resolvedPath,
  testGrantsOf,
  totalDenial,
  type Grant,
} from "../src/commands/capabilities.ts";
import { readManifest } from "../src/manifest.ts";
import { runHd } from "./hd-in-process.ts";

// Capability grants (spec/cli/command-line.md#capability-grants): the
// precedence of the table and the flags, path and name scopes, the test
// grant, and the startup refusal of a totally denied need.

const flags = (...values: string[]) => capabilityFlags(values, "hd run");

test("grant precedence: deny wins, then flags, then the table, then no limit", () => {
  const table = { FsRead: ["data/"], Process: false, Env: true } as const;
  const grants = grantsOf({
    table,
    tableName: "hd.toml",
    tableBase: "/pkg",
    flags: flags("FsRead=out/", "Process=true", "Http=a.test", "Http=b.test", "Console=false"),
    flagBase: "/cwd",
  });
  // A flag replaces the table's value, relative to the working directory (cli.cap.order.flag).
  assert.deepEqual(grants.get("FsRead"), { kind: "list", entries: [resolvedPath("/cwd", "out")] });
  // A table false wins over a flag true (cli.cap.order.deny).
  assert.deepEqual(grants.get("Process"), { kind: "deny", setting: "Process = false in hd.toml" });
  assert.deepEqual(grants.get("Console"), { kind: "deny", setting: "--cap Console=false" });
  // Flags for one trait add up; the table's true stays (cli.cap.order.table).
  assert.deepEqual(grants.get("Http"), { kind: "list", entries: ["a.test", "b.test"] });
  assert.deepEqual(grants.get("Env"), { kind: "all" });
  // Nothing names Clock, so it has no limit (cli.cap.order.default).
  assert.equal(grants.get("Clock"), undefined);
  assert.equal(
    grantsOf({ flags: flags("Env=A", "Env=true"), flagBase: "/" }).get("Env")?.kind,
    "all",
  );
  // An empty list covers nothing (cli.cap.value.empty).
  assert.equal(
    coversName(grantsOf({ flags: flags("Env="), flagBase: "/" }).get("Env")!, "A"),
    false,
  );
});

test("the manifest reads [capabilities] and [test.capabilities], and rejects a bad key", () => {
  const read = readManifest(
    '[package]\nname = "p"\n\n[capabilities]\nFsRead = ["data/"]\nConsole = false\n\n[test.capabilities]\nHttp = ["localhost"]\n',
  );
  assert.ok("manifest" in read);
  assert.deepEqual(read.manifest.capabilities, { FsRead: ["data/"], Console: false });
  assert.deepEqual(read.manifest.testCapabilities, { Http: ["localhost"] });
  assert.deepEqual(read.manifest.unknownKeys, []);
  const bad = readManifest(
    '[package]\nname = "p"\n\n[capabilities]\nNetwork = true\nClock = ["x"]\n',
  );
  assert.ok("errors" in bad);
  assert.match(bad.errors[0]!.message, /'Network' is not a capability/);
  assert.match(bad.errors[1]!.message, /Clock takes only true or false/);
});

test("a path entry covers its file or directory, after .. and symbolic links resolve", async () => {
  const directory = await realpath(await mkdtemp(join(tmpdir(), "hd-capabilities-")));
  try {
    await mkdir(join(directory, "data", "inner"), { recursive: true });
    await mkdir(join(directory, "secret"));
    await symlink(join(directory, "secret"), join(directory, "data", "link"));
    const grant: Grant = { kind: "list", entries: [resolvedPath(directory, "data/")] };
    const covers = (path: string): boolean => coversPath(grant, resolvedPath(directory, path));
    assert.equal(covers("data"), true);
    assert.equal(covers("data/inner/new.txt"), true);
    assert.equal(covers("data/../secret/key"), false);
    assert.equal(covers("data/link/key"), false); // the link leads out of data/
    assert.equal(covers("database.txt"), false); // a prefix of the name is no parent
    const file: Grant = { kind: "list", entries: [resolvedPath(directory, "config.toml")] };
    assert.equal(coversPath(file, resolvedPath(directory, "./config.toml")), true);
    assert.equal(coversPath(file, resolvedPath(directory, "config.toml.bak")), false);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("an Env entry names a variable, or a prefix before *", () => {
  const grant: Grant = { kind: "list", entries: ["HOME", "APP_*"] };
  assert.equal(coversName(grant, "HOME"), true);
  assert.equal(coversName(grant, "HOMEDIR"), false);
  assert.equal(coversName(grant, "APP_TOKEN"), true);
  assert.equal(coversName(grant, "APP"), false);
});

test("the test grant reads the package and writes only the temporary directory", () => {
  let temporary: string | undefined;
  const plain = testGrantsOf({ flags: [], flagBase: "/" }, "/pkg", () => temporary);
  const read = plain.get("FsRead")!;
  const write = plain.get("FsWrite")!;
  assert.equal(coversPath(read, resolvedPath("/", "/pkg/fixtures/a.csv")), true);
  assert.equal(coversPath(write, resolvedPath("/", "/pkg/out.txt")), false);
  temporary = "/tmp-case";
  assert.equal(coversPath(write, resolvedPath("/", "/tmp-case/out.txt")), true);
  // A listed FsWrite gets the temporary directory added (cli.test.env.grant.fs-added).
  const listed = testGrantsOf(
    { table: { FsWrite: ["out/"] }, tableName: "t", tableBase: "/pkg", flags: [], flagBase: "/" },
    "/pkg",
    () => temporary,
  ).get("FsWrite")!;
  assert.equal(coversPath(listed, resolvedPath("/", "/pkg/out/x")), true);
  assert.equal(coversPath(listed, resolvedPath("/", "/tmp-case/x")), true);
  // `true` stays no limit.
  const open = testGrantsOf(
    { flags: flags("FsWrite=true"), flagBase: "/" },
    "/pkg",
    () => undefined,
  );
  assert.equal(open.get("FsWrite")?.kind, "all");
});

test("a totally denied need refuses the start, naming the setting", () => {
  const grants = grantsOf({ flags: flags("Console=false"), flagBase: "/" });
  assert.equal(
    totalDenial(["Console"], grants),
    "hd: the program needs Console, which --cap Console=false denies",
  );
  assert.equal(totalDenial(["Clock"], grants), undefined);
});

test("hd FILE refuses to start a program whose import list needs a denied trait", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-capabilities-"));
  try {
    await writeFile(
      join(directory, "hello.hd"),
      'pub fn main() -> void $ Console:\n    println("hello")\n',
    );
    await writeFile(
      join(directory, "quiet.hd"),
      "use std.time.{Clock, now}\n\npub fn main() -> void $ Clock:\n    _ := now()\n",
    );
    const refused = await runHd(["--cap", "Console=false", "hello.hd"], { cwd: directory });
    assert.equal(refused.status, 101);
    assert.equal(refused.stdout, "");
    assert.equal(
      refused.stderr,
      "hd: the program needs Console, which --cap Console=false denies\n",
    );
    // A module that imports no Console method starts.
    const started = await runHd(["quiet.hd", "--cap", "Console=false"], { cwd: directory });
    assert.equal(started.status, 0, started.stderr);
    const unknown = await runHd(["hello.hd", "--cap", "Consol=true"], { cwd: directory });
    assert.equal(unknown.status, 101);
    assert.match(unknown.stderr, /--cap Consol: unknown capability/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("hd test refuses an integration test module that needs a denied trait", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-capabilities-"));
  try {
    await mkdir(join(directory, "src"));
    await mkdir(join(directory, "tests"));
    await writeFile(
      join(directory, "hd.toml"),
      '[package]\nname = "p"\n\n[test.capabilities]\nFsRead = false\n',
    );
    await writeFile(join(directory, "src", "lib.hd"), "pub fn one() -> i32:\n    1\n");
    await writeFile(
      join(directory, "tests", "read.hd"),
      'use std.fs.read_text\nuse std.path.Path\n\nit("reads"):\n    _ := read_text!(Path("hd.toml"))\n',
    );
    const refused = await runHd(["test"], { cwd: directory });
    assert.equal(refused.status, 101);
    assert.match(
      refused.stderr,
      /hd: the program needs FsRead, which FsRead = false in \[test\.capabilities\] of hd\.toml denies/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
