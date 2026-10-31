import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";
import {
  emitTypeScriptJavaScript,
  readTypeScriptJavaScript,
} from "../packaging/typescript-emission.ts";
import { fixedBindingsModule } from "../packaging/companion.ts";

test("published SDK modules are runnable JavaScript with JavaScript specifiers", async () => {
  const names = [
    "packaging/contract-bindings.ts",
    "adapters/application.ts",
    "adapters/dispatcher.ts",
    "adapters/console.ts",
    "adapters/http.ts",
    "adapters/fetch.ts",
    "adapters/worker.ts",
    "adapters/supervisor.ts",
    "adapters/vercel-node.ts",
  ];
  const output = await mkdtemp(join(tmpdir(), "hd-sdk-typescript-emission-"));
  try {
    await writeFile(join(output, "package.json"), '{"type":"module"}\n');
    for (const name of names) {
      const source = await readTypeScriptJavaScript(new URL(`../${name}`, import.meta.url), name);
      const javascript = source.toString("utf8");
      assert.doesNotMatch(javascript, /(?:from |import\()["'][^"']+\.ts["']/, name);
      if (name === "adapters/application.ts") {
        assert.match(javascript, /from "\.\/dispatcher\.js"/);
      }
      await writeFile(join(output, basename(name, ".ts") + ".js"), source);
    }
    const application = await import(pathToFileURL(join(output, "application.js")).href);
    assert.equal(typeof application.createApplication, "function");
    const supervisor = await import(pathToFileURL(join(output, "supervisor.js")).href);
    assert.equal(typeof supervisor.createSupervisor, "function");
  } finally {
    await rm(output, { recursive: true, force: true });
  }
});

test("SDK TypeScript emission rejects non-erasable syntax", () => {
  assert.throws(
    () => emitTypeScriptJavaScript("enum Direction { Left, Right }\n", "direction.ts"),
    /cannot strip direction\.ts/,
  );
});

test("published bindings import the emitted contract module", () => {
  const source = fixedBindingsModule(new Uint8Array(), "runtime");
  assert.match(source, /from "\.\/sdk\/contract-bindings\.js"/);
  assert.doesNotMatch(source, /\.ts["']/);
});
