import assert from "node:assert/strict";
import { test } from "node:test";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import { emitHostJavaScript, emitHostSources, readHostSources } from "../scripts/host-emission.ts";

test("typed host sources emit runnable plain .js without changing source", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "hd-host-emission-"));
  try {
    const source = join(temporary, "source");
    const output = join(temporary, "output");
    await mkdir(source);
    const dependency = "export const twice = (value: number): number => value * 2;\n";
    const entry = [
      'import { twice } from "./dependency.ts";',
      "export type Answer = number;",
      "export const answer: Answer = twice(21);",
      'export const load = () => import("./dependency.ts");',
      'export const worker = new URL("./worker.ts?kind=test", import.meta.url);',
      "",
    ].join("\n");
    await writeFile(join(source, "dependency.ts"), dependency);
    await writeFile(join(source, "entry.ts"), entry);
    await writeFile(join(source, "worker.ts"), "export const ready: boolean = true;\n");
    await writeFile(join(source, "plain.ts"), 'export { answer } from "./entry.ts";\n');
    const graph = {
      "worker.js": "worker.ts",
      "entry.js": "entry.ts",
      "dependency.js": "dependency.ts",
      "plain.js": "plain.ts",
    };

    const first = await readHostSources(source, graph);
    const second = await readHostSources(source, graph);
    assert.deepEqual(first, second);
    assert.deepEqual(Object.keys(first), ["dependency.js", "entry.js", "plain.js", "worker.js"]);
    assert.doesNotMatch(first["entry.js"], /\.ts/);
    assert.doesNotMatch(first["entry.js"], /: Answer|type Answer/);

    assert.deepEqual(await emitHostSources(output, source, graph), first);
    assert.equal(await readFile(join(source, "entry.ts"), "utf8"), entry);
    const module = await import(pathToFileURL(join(output, "entry.js")).href);
    assert.equal(module.answer, 42);
    assert.equal((await module.load()).twice(4), 8);
    assert.equal(module.worker.search, "?kind=test");
    assert.equal((await import(pathToFileURL(join(output, "plain.js")).href)).answer, 42);

    await assert.rejects(emitHostSources(output, source, graph), { code: "EEXIST" });
    assert.equal(await access(join(output, "entry.js")), undefined);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test("host emission rejects non-erasable TypeScript", () => {
  assert.throws(
    () => emitHostJavaScript("enum Direction { Left, Right }\n", "direction.ts"),
    /host emission: cannot strip direction\.ts/,
  );
});

test("host emission rejects paths outside the source root", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "hd-host-emission-path-"));
  try {
    await assert.rejects(readHostSources(temporary, { "bad.js": "../bad.ts" }), /escapes/);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});
