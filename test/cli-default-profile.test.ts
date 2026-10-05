import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";
import test from "node:test";

import { replCommand } from "../src/commands/help.ts";
import { ReplSession } from "../src/repl.ts";
import { hd } from "./hd-in-process.ts";

// The default profile (spec/cli/command-line.md#host-capabilities): `hd
// FILE`, `hd run`, and a task bind `ConsoleInput`, `Args`, `Env`, `Clock`,
// `Random`, `FsRead`, and `FsWrite` for the traits the entry row names.
// Every test runs in its own temporary directory, and sets the only
// environment variables the program sees.

const PROGRAM = `use std.host.{Args, Env, args, env}
use std.console.{ConsoleInput, read_line}
use std.time.{Clock, Duration, now}
use std.random.Random
use std.fs.{FsRead, FsWrite, read_text, write_text}
use std.path.Path

pub fn main!() -> void $ Console + ConsoleInput + Args + Env + Clock + Random + FsRead + FsWrite:
    println("program \${$.use(Args).program()}")
    for argument in args():
        println("arg \${argument}")
    match env("HD_DEMO_NAME"):
        .Some(name) => println("env \${name}")
        .None => println("env unset")
    println("env missing \${env("HD_DEMO_MISSING").is_none()}")
    match write_text!(Path("out/note.txt"), "hello file"):
        .Ok(()) => println("wrote")
        .Err(error) => println("write failed: \${error}")
    let mut files = $.use(FsWrite)
    match files.create_dir_all!(Path("out")):
        .Ok(()) => pass
        .Err(error) => println("mkdir failed: \${error}")
    match write_text!(Path("out/note.txt"), "hello file"):
        .Ok(()) => println("wrote")
        .Err(error) => println("write failed: \${error}")
    match read_text!(Path("out/note.txt")):
        .Ok(text) => println("read \${text}")
        .Err(error) => println("read failed: \${error}")
    match read_text!(Path("missing.txt")):
        .Ok(_) => println("missing exists")
        .Err(.NotFound(path)) => println("not found \${path}")
        .Err(error) => println("other \${error}")
    match $.use(FsRead).list_dir!(Path("out")):
        .Ok(entries) =>
            for entry in entries:
                println("entry \${entry.path} \${entry.size}")
        .Err(error) => println("list failed: \${error}")
    println("after 2020 \${now().unix_milliseconds() > 1577836800000}")
    let mut clock = $.use(Clock)
    start := clock.monotonic()
    clock.sleep!(Duration::milliseconds(20))
    println("slept \${clock.monotonic().since(start).as_milliseconds() >= 20}")
    let mut random = $.use(Random)
    println("random \${random.fill(4).len()}")
    while true:
        match read_line!():
            .Ok(.Some(line)) => println("line \${line}")
            .Ok(.None) => break
            .Err(_) => break
    match read_line!():
        .Ok(.None) => println("still at the end")
        _ => println("read past the end")
`;

test("hd FILE binds each trait of the default profile that main's row names", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-default-profile-"));
  try {
    await writeFile(join(directory, "demo.hd"), PROGRAM);
    const { stdout } = await hd(["demo.hd", "--", "a", "b c"], {
      cwd: directory,
      variables: { HD_DEMO_NAME: "demo" },
      readInput: async () => "one\r\ntwo\nthree",
    });
    assert.equal(
      stdout,
      [
        "program demo.hd",
        "arg a",
        "arg b c",
        "env demo",
        "env missing true",
        "write failed: not found: out/note.txt",
        "wrote",
        "read hello file",
        "not found missing.txt",
        "entry out/note.txt 10",
        "after 2020 true",
        "slept true",
        "random 4",
        "line one",
        "line two",
        "line three",
        "still at the end",
        "",
      ].join("\n"),
    );
    assert.equal(await readFile(join(directory, "out", "note.txt"), "utf8"), "hello file");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("hd run passes the arguments after --, and a task runs in its package directory", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-default-profile-"));
  try {
    await hd(["new", "--app", "--vcs", "none", "shop"], { cwd: directory });
    const pkg = join(directory, "shop");
    await writeFile(
      join(pkg, "src", "main.hd"),
      [
        "use std.host.{Args, args}",
        "use std.fs.{FsRead, read_text}",
        "use std.path.Path",
        "",
        "pub fn main!() -> void $ Console + Args + FsRead:",
        '    println("${$.use(Args).program()} ${args().len()}")',
        "    for argument in args():",
        "        println(argument)",
        '    match read_text!(Path("where.txt")):',
        '        .Ok(text) => println("cwd ${text}")',
        '        .Err(error) => println("cwd ${error}")',
        "",
      ].join("\n"),
    );
    await mkdir(join(pkg, "tasks"));
    await writeFile(join(pkg, "tasks", "where.hd"), await readFile(join(pkg, "src", "main.hd")));
    await writeFile(join(pkg, "where.txt"), "package");
    await mkdir(join(pkg, "sub"));
    await writeFile(join(pkg, "sub", "where.txt"), "sub");
    const run = await hd(["run", "--", "a", "-x"], { cwd: join(pkg, "sub") });
    assert.equal(run.stdout, "shop 2\na\n-x\ncwd sub\n");
    const task = await hd(["run", "where", "--", "z"], { cwd: join(pkg, "sub") });
    assert.equal(task.stdout, "where 1\nz\ncwd package\n");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("an entry row key outside the default profile is nonhost-entry-requirement", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-default-profile-"));
  try {
    await writeFile(
      join(directory, "other.hd"),
      "pub trait Database\n\npub fn main() -> void $ Database:\n    pass\n",
    );
    await assert.rejects(hd(["other.hd"], { cwd: directory }), (error: { stderr: string }) =>
      /nonhost-entry-requirement/.test(error.stderr),
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("a REPL session binds the default profile, and each host call runs once", async () => {
  // spec/cli/command-line.md#r-cli.repl.host.default-profile, cli.repl.host.cwd,
  // cli.repl.host.args, and cli.repl.host.once.
  const directory = await mkdtemp(join(tmpdir(), "hd-default-profile-"));
  try {
    await writeFile(join(directory, "notes.txt"), "remember the milk");
    const input = new PassThrough();
    const output = new PassThrough();
    let text = "";
    output.on("data", (chunk: Buffer) => (text += chunk.toString()));
    const done = replCommand({ input, output, terminal: false }, { cwd: directory });
    input.end(
      [
        "use std.time.{now}",
        "use std.fs.{FsWrite, read_text}",
        "use std.path.{Path}",
        "use std.host.{args}",
        "started := now()",
        "started",
        'read_text!(Path("notes.txt"))',
        "let mut files = $.use(FsWrite)",
        'files.append_text!(Path("log.txt"), "once")',
        "args()",
        "started",
        "",
      ].join("\n"),
    );
    assert.equal(await done, 0);
    const lines = text.trimEnd().split("\n");
    const [first, second] = lines.filter((line) => line.endsWith(" : Timestamp"));
    // The clock is read once; the second display replays that reading.
    assert.match(first!, /^Timestamp \{ millis: \d+ \}/);
    assert.equal(second, first);
    assert.ok(lines.includes('Ok("remember the milk") : Result[string, FsError]'), text);
    assert.ok(lines.includes("[] : List[string]"), text);
    // Three inputs ran after the append; the file still holds it once.
    assert.equal(await readFile(join(directory, "log.txt"), "utf8"), "once");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("a REPL session answers an accepted input's host calls from its record", async () => {
  let readings = 0;
  const session = new ReplSession({}, undefined, {
    traits: [{ module: "std.time", name: "Clock" }],
    invoke: (call) => {
      if (call.methodName !== "now") return undefined;
      readings += 1;
      return { pending: false, value: { millis: BigInt(readings * 1000) } as never };
    },
  });
  await session.evaluate("use std.time.{now}");
  assert.equal((await session.evaluate("now().to_rfc3339()")).value, '"1970-01-01T00:00:01Z"');
  // An input that panics after its reading is rejected, and its reading is not kept.
  assert.equal((await session.evaluate("[now().to_rfc3339()][3]")).accepted, false);
  assert.equal((await session.evaluate("now().to_rfc3339()")).value, '"1970-01-01T00:00:03Z"');
  assert.equal(readings, 3);
});
