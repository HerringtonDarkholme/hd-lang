import assert from "node:assert/strict";
import { test } from "node:test";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import { emitHostJavaScript, emitHostSources, readHostSources } from "../scripts/host-emission.mjs";

test("typed host sources emit runnable plain .mjs without changing source", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "hd-host-emission-"));
  try {
    const source = join(temporary, "source");
    const output = join(temporary, "output");
    await mkdir(source);
    const dependency = "export const twice = (value: number): number => value * 2;\n";
    const entry = [
      'import { twice } from "./dependency.mts";',
      "export type Answer = number;",
      "export const answer: Answer = twice(21);",
      'export const load = () => import("./dependency.mts");',
      'export const worker = new URL("./worker.mts?kind=test", import.meta.url);',
      "",
    ].join("\n");
    await writeFile(join(source, "dependency.mts"), dependency);
    await writeFile(join(source, "entry.mts"), entry);
    await writeFile(join(source, "worker.mts"), "export const ready: boolean = true;\n");
    const graph = {
      "worker.mjs": "worker.mts",
      "entry.mjs": "entry.mts",
      "dependency.mjs": "dependency.mts",
    };

    const first = await readHostSources(source, graph);
    const second = await readHostSources(source, graph);
    assert.deepEqual(first, second);
    assert.deepEqual(Object.keys(first), ["dependency.mjs", "entry.mjs", "worker.mjs"]);
    assert.doesNotMatch(first["entry.mjs"], /\.mts/);
    assert.doesNotMatch(first["entry.mjs"], /: Answer|type Answer/);

    assert.deepEqual(await emitHostSources(output, source, graph), first);
    assert.equal(await readFile(join(source, "entry.mts"), "utf8"), entry);
    const module = await import(pathToFileURL(join(output, "entry.mjs")));
    assert.equal(module.answer, 42);
    assert.equal((await module.load()).twice(4), 8);
    assert.equal(module.worker.search, "?kind=test");

    await assert.rejects(emitHostSources(output, source, graph), { code: "EEXIST" });
    assert.equal(await access(join(output, "entry.mjs")), undefined);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test("host emission rejects non-erasable TypeScript", () => {
  assert.throws(
    () => emitHostJavaScript("enum Direction { Left, Right }\n", "direction.mts"),
    /host emission: cannot strip direction\.mts/,
  );
});

test("host emission rejects paths outside the source root", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "hd-host-emission-path-"));
  try {
    await assert.rejects(readHostSources(temporary, { "bad.mjs": "../bad.mts" }), /escapes/);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});
