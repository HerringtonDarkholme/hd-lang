import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import type { Terminal } from "../src/commands/index.ts";
import { runHd } from "./hd-in-process.ts";

// `hd new` (spec/cli/command-line.md#creating-a-package).

async function withDirectory(run: (directory: string) => Promise<void>): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "hd-new-"));
  try {
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

/** A terminal that answers each question with the next of `answers`, and records the questions. */
function scripted(answers: readonly string[]): Terminal & { readonly questions: string[] } {
  const questions: string[] = [];
  const queue = [...answers];
  return { questions, ask: async (question) => (questions.push(question), queue.shift() ?? "") };
}

test("hd new --lib writes a library whose test passes", async () => {
  await withDirectory(async (directory) => {
    const made = await runHd(["new", "--lib", "--vcs", "none", "text-tools"], { cwd: directory });
    assert.equal(made.status, 0, made.stderr);
    const root = join(directory, "text-tools");
    assert.equal(await readFile(join(root, "hd.toml"), "utf8"), '[package]\nname = "text-tools"\n');
    // The test file is named after the package with each - as _ (cli.new.test-name).
    assert.match(
      await readFile(join(root, "tests/text_tools.hd"), "utf8"),
      /^use pkg\.\{greet\}$/m,
    );
    assert.ok(existsSync(join(root, "src/lib.hd")));
    assert.ok(!existsSync(join(root, ".gitignore")));
    const tested = await runHd(["test"], { cwd: root });
    assert.equal(tested.status, 0, tested.stderr);
    assert.equal(tested.stdout, "tests/text_tools.hd: 1 passed\n");
    assert.equal((await runHd(["check"], { cwd: root })).stdout, "text-tools: ok\n");
  });
});

test("hd new --app writes an application that hd run runs", async () => {
  await withDirectory(async (directory) => {
    assert.equal((await runHd(["new", "--app", "--vcs", "none"], { cwd: directory })).status, 0);
    // No [[executable]] table, so src/main.hd is the default executable.
    assert.doesNotMatch(await readFile(join(directory, "hd.toml"), "utf8"), /executable/);
    const ran = await runHd(["run"], { cwd: directory });
    assert.equal(ran.stdout, "hello, world\n");
    assert.match(await readFile(join(directory, "src/main.hd"), "utf8"), /hello, world/);
  });
});

test("hd new asks for the kind on a terminal, and fails without one", async () => {
  await withDirectory(async (directory) => {
    const closed = await runHd(["new", "--vcs", "none", "a"], { cwd: directory });
    assert.equal(closed.status, 101);
    assert.match(closed.stderr, /--app.*--lib/);
    assert.ok(!existsSync(join(directory, "a")));

    const terminal = scripted(["lib", "y"]);
    const asked = await runHd(["new", "--vcs", "none", "b"], { cwd: directory, terminal });
    assert.equal(asked.status, 0, asked.stderr);
    assert.equal(terminal.questions.length, 2);
    assert.match(terminal.questions[1]!, /GitHub Pages/);
    assert.ok(existsSync(join(directory, "b/src/lib.hd")));
    // A yes is --pages (cli.new.pages.ask).
    assert.match(
      await readFile(join(directory, "b/.github/workflows/docs.yml"), "utf8"),
      /hd doc --out _site/,
    );
  });
});

test("hd new writes nothing when a file it would write exists", async () => {
  await withDirectory(async (directory) => {
    await mkdir(join(directory, "src"));
    await writeFile(join(directory, "src/main.hd"), "pass\n");
    const failed = await runHd(["new", "--app", "--vcs", "none"], { cwd: directory });
    assert.equal(failed.status, 101);
    assert.match(failed.stderr, /src\/main\.hd already exists/);
    assert.ok(!existsSync(join(directory, "hd.toml")));
    // A name that is no identifier after - becomes _ is rejected.
    assert.equal(
      (await runHd(["new", "--lib", "--vcs", "none", "2fast"], { cwd: directory })).status,
      101,
    );
    assert.equal((await runHd(["new", "--lib", "--app", "x"], { cwd: directory })).status, 101);
  });
});

test("hd new runs git init and writes a .gitignore outside a repository", async () => {
  await withDirectory(async (directory) => {
    assert.equal((await runHd(["new", "--app", "tracked"], { cwd: directory })).status, 0);
    assert.ok(existsSync(join(directory, "tracked/.git")));
    // The .gitignore lists only the build directory (cli.new.vcs.ignore).
    assert.equal(await readFile(join(directory, "tracked/.gitignore"), "utf8"), "/build/\n");
    // Inside a repository, hd new runs no git init.
    assert.equal(
      (await runHd(["new", "--lib", "inner"], { cwd: join(directory, "tracked") })).status,
      0,
    );
    assert.ok(!existsSync(join(directory, "tracked/inner/.git")));
    assert.ok(!existsSync(join(directory, "tracked/inner/.gitignore")));
  });
});
