import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { overviewHelp } from "../src/cli-args.ts";
import {
  readRuntimeSection,
  withRuntimeSection,
  type RuntimeInterface,
} from "../src/runtime-interface.ts";
import { assembleWat } from "../src/wasm.ts";
import { runHd } from "./hd-in-process.ts";

// `hd FILE.wasm` runs a module that `hd build` wrote, without its source
// (spec/cli/command-line.md#prebuilt-modules). The CLI cases wasm-run-built,
// wasm-cap-flags-only, and wasm-invalid cover the command line; these tests
// cover the module's `hd.runtime` section and the errors no case can build.

async function withPackage(
  files: Readonly<Record<string, string>>,
  body: (directory: string) => Promise<void>,
): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "hd-prebuilt-"));
  try {
    for (const [path, text] of Object.entries(files)) {
      await mkdir(join(directory, path, ".."), { recursive: true });
      await writeFile(join(directory, path), text);
    }
    await body(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

/** A runtime section with an entry `main` and no host import. */
const BARE_RUNTIME: RuntimeInterface = {
  boundary: { data: [], enums: [], consentIn: [] },
  hostTraits: [],
  hostFunctions: [],
  functions: [],
  initializerRequirements: [],
  entry: { name: "main", requirements: [] },
};

const READER = [
  "use std.console.{read_line, ConsoleInput}",
  "",
  "pub fn main!() -> void $ Console + ConsoleInput:",
  "    match read_line!():",
  '        .Ok(.Some(line)) => println("got ${line}")',
  '        _ => panic("no input")',
  "",
].join("\n");

test("hd build writes the hd.runtime section, and the module runs as its source does", async () => {
  await withPackage(
    { "hd.toml": '[package]\nname = "reader"\n', "src/main.hd": READER },
    async (directory) => {
      const built = await runHd(["build"], { cwd: directory });
      assert.equal(built.status, 0, built.stderr);
      const bytes = await readFile(join(directory, "build/debug/reader.wasm"));
      const runtime = readRuntimeSection(await WebAssembly.compile(bytes));
      assert.ok(runtime);
      assert.deepEqual(runtime.entry, { name: "main", requirements: ["Console", "ConsoleInput"] });
      assert.deepEqual(runtime.hostTraits.map(({ name }) => name).sort(), [
        "Console",
        "ConsoleInput",
      ]);
      // Standard input reaches the module (cli.wasm.host).
      const read = await runHd(["build/debug/reader.wasm"], {
        cwd: directory,
        readInput: async () => "hi\n",
      });
      assert.deepEqual([read.status, read.stdout], [0, "got hi\n"]);
      // A panic ends a built module as it ends its source, with no location
      // since the module has none (cli.wasm.status).
      const panicked = await runHd(["build/debug/reader.wasm"], { cwd: directory });
      const source = await runHd(["src/main.hd"], { cwd: directory });
      assert.equal(panicked.status, source.status);
      assert.equal(panicked.stderr, "explicit-panic: no input\n");
    },
  );
});

test("a valid module that hd build did not write names what it lacks", async () => {
  await withPackage({}, async (directory) => {
    const noExport = await assembleWat("(module)");
    await writeFile(
      join(directory, "no-export.wasm"),
      withRuntimeSection(noExport.bytes, BARE_RUNTIME),
    );
    const missing = await runHd(["no-export.wasm"], { cwd: directory });
    assert.equal(missing.status, 101);
    assert.equal(
      missing.stderr,
      "hd: no-export.wasm was not built by this hd: it lacks the entry export 'main'; rebuild it with hd build\n",
    );
    const foreign = await assembleWat(
      '(module (import "hd" "nope" (func)) (func (export "main")))',
    );
    await writeFile(
      join(directory, "foreign.wasm"),
      withRuntimeSection(foreign.bytes, BARE_RUNTIME),
    );
    const unknown = await runHd(["foreign.wasm"], { cwd: directory });
    assert.equal(unknown.status, 101);
    assert.equal(
      unknown.stderr,
      "hd: foreign.wasm was not built by this hd: it imports hd.nope, which this hd does not provide; rebuild it with hd build\n",
    );
    await writeFile(join(directory, "plain.wasm"), foreign.bytes);
    const plain = await runHd(["plain.wasm"], { cwd: directory });
    assert.equal(plain.status, 101);
    assert.match(plain.stderr, /plain\.wasm was not built by this hd: .*hd\.runtime/);
  });
});

test("a FILE that is no Wasm module, or is missing, is an error that names it", async () => {
  await withPackage({ "notes.wasm": "text\n" }, async (directory) => {
    const text = await runHd(["notes.wasm"], { cwd: directory });
    assert.equal(text.status, 101);
    assert.match(text.stderr, /^hd: notes\.wasm is not a valid WebAssembly module: /);
    const absent = await runHd(["absent.wasm"], { cwd: directory });
    assert.deepEqual(
      [absent.status, absent.stderr],
      [101, "hd: cannot read absent.wasm: no such file\n"],
    );
  });
});

test("the command list names hd FILE.wasm", () => {
  assert.match(overviewHelp(), /FILE\.wasm +run a module that hd build wrote/);
});
