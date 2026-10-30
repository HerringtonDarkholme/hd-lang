import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { parseProductionGrants, readProductionGrants } from "../packaging/grants.mjs";

const cases = JSON.parse(
  await readFile(new URL("./fixtures/deployment-grants.json", import.meta.url), "utf8"),
);
for (const fixture of cases) {
  test(`deployment grant conformance: ${fixture.name}`, () => {
    if (fixture.error) {
      assert.throws(
        () => parseProductionGrants(fixture.toml),
        (error) => {
          assert.ok(error.message.includes(fixture.error), error.message);
          assert.ok(!error.message.includes("DONT_ECHO_ME"));
          assert.ok(!error.message.includes("private-value"));
          return true;
        },
      );
    } else {
      assert.deepEqual(parseProductionGrants(fixture.toml), fixture.grants);
    }
  });
}

test("production grants and their scope arrays cannot be changed after parsing", () => {
  const grants = parseProductionGrants('[capabilities]\nHttp=["one.test"]\nConsole=true\n');
  assert.throws(() => (grants.Console = false), TypeError);
  assert.throws(() => grants.Http.push("another.test"), TypeError);
  assert.deepEqual(Object.keys(grants), ["Console", "Http"]);
  assert.equal(Object.hasOwn(grants, "Process"), false);
});

test("the selected root is read afresh and parent grants never fill missing entries", async () => {
  const dir = await mkdtemp(join(tmpdir(), "hd-deploy-grants-"));
  try {
    await writeFile(join(dir, "hd.toml"), "[capabilities]\nProcess=true\n");
    const root = join(dir, "app");
    await mkdir(root);
    await assert.rejects(readProductionGrants(root), { code: "ENOENT" });
    const manifest = join(root, "hd.toml");
    await writeFile(manifest, "[capabilities]\nConsole=false\n[dev-capabilities]\nProcess=true\n");
    assert.deepEqual(await readProductionGrants(root), { Console: false });
    await writeFile(manifest, "[capabilities]\nConsole=true\n");
    assert.deepEqual(await readProductionGrants(root), { Console: true });
    await writeFile(manifest, Uint8Array.of(0xff));
    await assert.rejects(readProductionGrants(root), /invalid UTF-8/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("the SDK task bridge writes a sorted production map without development grants", async () => {
  const dir = await mkdtemp(join(tmpdir(), "hd-deploy-grants-cli-"));
  try {
    await writeFile(
      join(dir, "hd.toml"),
      '[capabilities]\nHttp=["root.test"]\nConsole=false\n[dev-capabilities]\nHttp=true\nProcess=true\n',
    );
    const script = new URL("../packaging/grants.mjs", import.meta.url);
    const output = execFileSync(process.execPath, [fileURLToPath(script), dir], {
      encoding: "utf8",
    });
    assert.equal(output, '{"Console":false,"Http":["root.test"]}\n');
    assert.deepEqual(JSON.parse(output), await readProductionGrants(dir));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
