import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { packageMode, unselectedMains } from "../src/commands/package-mode.ts";
import { readManifest } from "../src/manifest.ts";
import { ReplSession } from "../src/repl.ts";
import { runHd } from "./hd-in-process.ts";

// hd.toml (src/manifest.ts) and package mode (src/commands/package-mode.ts):
// spec/cli/command-line.md#package-mode and #executables.

test("readManifest reads the package name and the executable tables", () => {
  const read = readManifest(
    [
      "# the shop",
      "[package]",
      'name = "shop-app"  # a name may hold -',
      "",
      "[dependencies]",
      'json = { path = "../json", version = "2.1.0" }',
      "tags = [",
      '  "a",',
      "  'b',",
      "]",
      "",
      "[[executable]]",
      'name = "migrate"',
      'module = "tools.migrate"',
      "",
      "[[executable]]",
      'name = "shop"',
      'module = "main"',
      "",
    ].join("\n"),
  );
  assert.ok("manifest" in read, JSON.stringify(read));
  assert.equal(read.manifest.name, "shop-app");
  assert.equal(read.manifest.workspace, false);
  assert.deepEqual(read.manifest.executables, [
    { name: "migrate", module: "tools.migrate", line: 12 },
    { name: "shop", module: "main", line: 16 },
  ]);
});

test("readManifest rejects what hd cannot read", () => {
  const errors = (text: string): string[] => {
    const read = readManifest(text);
    return "errors" in read ? read.errors.map(({ line, message }) => `${line}: ${message}`) : [];
  };
  assert.match(
    errors('[package]\nname = "a"\nname = "b"\n')[0]!,
    /^3: the key 'name' is defined twice/,
  );
  assert.match(errors("[package]\nname = \n")[0]!, /^2: expected a value/);
  assert.match(errors('[package]\nname = "a\n')[0]!, /^2: a string must end on its line/);
  assert.match(errors("[package]\n")[0]!, /^1: the \[package\] table needs a name/);
  assert.match(errors('name = "x"\n')[0]!, /declares no package/);
  // Executables have unique names, and each names a module (cli.exe.several, cli.exe.table).
  assert.match(
    errors(
      '[package]\nname = "a"\n[[executable]]\nname = "x"\nmodule = "x"\n[[executable]]\nname = "x"\nmodule = "y"\n',
    )[0]!,
    /^6: two executables are named 'x'/,
  );
  assert.match(
    errors('[package]\nname = "a"\n[[executable]]\nname = "x"\n')[0]!,
    /^3: executable 'x' needs a module/,
  );
  assert.match(
    errors('[package]\nname = "a"\n[source]\nroot = "lib"\n')[0]!,
    /only the default source root/,
  );
  // A workspace manifest declares no package (cli.mode.workspace).
  const workspace = readManifest('[workspace]\nmembers = ["a", "b"]\n');
  assert.ok("manifest" in workspace && workspace.manifest.workspace);
});

test("package mode comes from the nearest hd.toml above the start directory", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-package-mode-"));
  try {
    const write = async (path: string, text: string): Promise<void> => {
      await mkdir(join(directory, path, ".."), { recursive: true });
      await writeFile(join(directory, path), text);
    };
    await write("notes/src/a.hd", "pass\n");
    // A src directory without hd.toml makes no package.
    assert.equal((await packageMode(join(directory, "notes/src"))).kind, "outside");

    await write("shop/hd.toml", '[package]\nname = "shop"\n');
    await write("shop/src/main.hd", "pub fn main() -> void:\n    pass\n");
    await write("shop/src/cart/mod.hd", "pass\n");
    await write("shop/tests/cart.hd", "pass\n");
    const mode = await packageMode(join(directory, "shop/src/cart"));
    assert.equal(mode.kind, "package");
    if (mode.kind !== "package") return;
    assert.equal(mode.package.root, join(directory, "shop"));
    assert.deepEqual(Object.keys(mode.package.files).sort(), [
      "src/cart/mod.hd",
      "src/main.hd",
      "tests/cart.hd",
    ]);
    // src/main.hd is the default executable, named after the package.
    assert.deepEqual(mode.package.executables, [{ name: "shop", path: "src/main.hd" }]);
    assert.deepEqual(mode.package.problems, []);

    // With [[executable]] tables, an unlisted src/main.hd is an error, a
    // module that does not exist is missing-entry-point, and a public main
    // that no table names warns unselected-main.
    await write(
      "shop/hd.toml",
      '[package]\nname = "shop"\n\n[[executable]]\nname = "migrate"\nmodule = "tools.migrate"\n\n[[executable]]\nname = "seed"\nmodule = "cart"\n',
    );
    await write("shop/src/helper.hd", "pub fn main() -> void:\n    pass\n");
    const declared = await packageMode(join(directory, "shop"));
    assert.equal(declared.kind, "package");
    if (declared.kind !== "package") return;
    assert.deepEqual(declared.package.executables, [{ name: "seed", path: "src/cart/mod.hd" }]);
    assert.deepEqual(
      declared.package.problems.map(({ line, code }) => `${line}:${code}`),
      ["4:missing-entry-point", "1:unlisted-entry"],
    );
    assert.deepEqual(
      unselectedMains(declared.package).map(({ path, line, code }) => `${path}:${line}:${code}`),
      ["src/helper.hd:1:unselected-main"],
    );

    // A workspace manifest that declares no package is workspace mode.
    await write("ws/hd.toml", '[workspace]\nmembers = ["libs/ui"]\n');
    assert.equal((await packageMode(join(directory, "ws"))).kind, "workspace");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("tasks run by name, check --all checks them, and a task may not share an executable's name", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-tasks-"));
  try {
    const write = async (path: string, text: string): Promise<void> => {
      await mkdir(join(directory, path, ".."), { recursive: true });
      await writeFile(join(directory, path), text);
    };
    await write("hd.toml", '[package]\nname = "shop"\n');
    await write("src/main.hd", 'pub fn main() -> void $ Console:\n    println("app")\n');
    await write("src/util.hd", 'pub fn word() -> string:\n    "seeded"\n');
    await write("tasks/shared/wrap.hd", 'pub fn wrap(text: string) -> string:\n    "[${text}]"\n');
    await write(
      "tasks/seed.hd",
      "use pkg.util.{word}\nuse self.shared.wrap.{wrap}\n\npub fn main() -> void $ Console:\n    println(wrap(word()))\n",
    );
    const mode = await packageMode(directory);
    assert.ok(mode.kind === "package");
    assert.deepEqual(mode.package.tasks, [{ name: "seed", path: "tasks/seed.hd" }]);
    // hd run NAME runs a task (cli.run.name); hd run alone, the executable.
    assert.equal((await runHd(["run", "seed"], { cwd: directory })).stdout, "[seeded]\n");
    assert.equal((await runHd(["run"], { cwd: directory })).stdout, "app\n");
    // Only hd check --all checks the tasks (cli.check.all).
    await write("tasks/broken.hd", "pub fn main() -> i32:\n    true\n");
    assert.equal((await runHd(["check"], { cwd: directory })).status, 0);
    const all = await runHd(["check", "--all"], { cwd: directory });
    assert.equal(all.status, 101);
    assert.match(all.stderr, /^tasks\/broken\.hd:2:\d+: type-mismatch/m);
    // A task named like an executable is an error (cli.task.name-clash).
    await write("tasks/shop.hd", "pub fn main() -> void:\n    pass\n");
    const clash = await runHd(["run", "seed"], { cwd: directory });
    assert.equal(clash.status, 101);
    assert.match(clash.stderr, /tasks\/shop\.hd and the executable 'shop'/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("a REPL session in a package acts as code inside src/lib.hd", async () => {
  const files = {
    "src/lib.hd": "fn secret() -> i32:\n    41\n",
    "src/util.hd": "pub fn double(value: i32) -> i32:\n    value * 2\n",
    "src/main.hd": 'pub fn main() -> void $ Console:\n    println("app")\n',
  };
  const session = new ReplSession({}, { files, programs: ["src/main.hd"] });
  // It sees the private declarations of src/lib.hd (cli.repl.package.lib).
  assert.equal((await session.evaluate("secret() + 1")).value, "42");
  // `use self.util` names src/util.hd.
  assert.deepEqual((await session.evaluate("use self.util.{double}")).errors, []);
  assert.equal((await session.evaluate("double(21)")).value, "42");
  // src/main.hd stays unusable (cli.repl.package.no-lib.main).
  const main = await session.evaluate("use self.main.{main}");
  assert.equal(main.accepted, false);
  assert.match(main.errors[0]!, /unknown-module/);
  // An error in a package file names that file.
  const broken = new ReplSession(
    {},
    {
      files: { ...files, "src/util.hd": "pub fn double(value: i32) -> i32:\n    true\n" },
      programs: [],
    },
  );
  const failed = await broken.evaluate("use self.util.{double}");
  assert.match(failed.errors[0]!, /^src\/util\.hd:2:\d+: type-mismatch/);
  // Outside any package, the session may use only std (cli.repl.outside).
  assert.match(
    (await new ReplSession().evaluate("use self.util.{double}")).errors[0]!,
    /unknown-module/,
  );
});

test("a manifest table or key with no meaning is listed for a warning", () => {
  // spec/cli/command-line.md#r-cli.manifest.unknown-key
  const read = readManifest(
    '[package]\nname = "shop"\nversion = "1.0.0"\n\n[package.metadata]\nteam = "a"\n\n[[executable]]\nname = "shop"\nmodule = "main"\nopt = 3\n\n[dependencies]\nanything = "github.com/acme/x@1.0.0"\n\n[profile]\nrelease = true\n',
  );
  assert.ok("manifest" in read);
  assert.deepEqual(
    read.manifest.unknownKeys.map(({ line }) => line),
    [3, 5, 11, 16],
  );
});
