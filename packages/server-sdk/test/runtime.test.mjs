import assert from "node:assert/strict";
import { test } from "node:test";
import { readFile, writeFile, mkdtemp, rm, access } from "node:fs/promises";
import { cpSync, readFileSync, readdirSync, writeFileSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";
import { readPortableRuntime, emitPortableRuntime } from "../packaging/portable-runtime.mjs";
import { runtimeSources } from "../packaging/runtime-model.mjs";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const archive = new URL("../runtime/runtime-sources.json", import.meta.url);

test("the SDK snapshot matches the canonical portable host and ABI sources", async () => {
  const snapshot = await readPortableRuntime();
  for (const [name, source] of Object.entries(runtimeSources))
    assert.equal(
      snapshot.files[name],
      (await readFile(join(root, source), "utf8")).replaceAll("\r\n", "\n"),
      name,
    );
  const schema = await readFile(join(root, "crates/hd_host_abi/src/schema.rs"), "utf8");
  assert.equal(
    snapshot.packageHostABI,
    Number(schema.match(/^pub const VERSION: u32 = (\d+);$/m)[1]),
  );
  assert.ok(Object.isFrozen(snapshot.files));
});

for (const [name, mutate] of [
  ["unknown format", (value) => (value.format = 2)],
  ["missing file", (value) => delete value.files["core.mjs"]],
  ["path escape", (value) => (value.files["../outside.mjs"] = value.files["core.mjs"])],
  ["changed code", (value) => (value.files["core.mjs"].source += "\n// changed\n")],
  ["changed hash", (value) => (value.files["core.mjs"].sha256 = "0".repeat(64))],
  ["changed ABI", (value) => (value.packageHostABI += 1)],
  ["changed provenance", (value) => (value.baseRevision = "0".repeat(40))],
  ["changed fingerprint", (value) => (value.fingerprint = "0".repeat(64))],
]) {
  test(`an invalid snapshot is rejected before output: ${name}`, async () => {
    const directory = await mkdtemp(join(tmpdir(), "hd-runtime-reject-"));
    try {
      const value = JSON.parse(await readFile(archive, "utf8"));
      mutate(value);
      const file = join(directory, "snapshot.json");
      const output = join(directory, "output");
      await writeFile(file, JSON.stringify(value));
      await assert.rejects(emitPortableRuntime(output, file), /SDK portable runtime/);
      await assert.rejects(access(output), { code: "ENOENT" });
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
}

test("runtime emission copies exact bytes and refuses existing output", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-runtime-output-"));
  try {
    const output = join(directory, "runtime");
    const info = await emitPortableRuntime(output);
    const snapshot = await readPortableRuntime();
    assert.equal(info.fingerprint, snapshot.fingerprint);
    for (const [name, text] of Object.entries(snapshot.files))
      assert.equal(await readFile(join(output, name), "utf8"), text);
    const core = await import(info.coreURL);
    assert.equal(typeof core.createCoreHost, "function");
    const sentinel = join(output, "keep.txt");
    await writeFile(sentinel, "keep");
    await assert.rejects(emitPortableRuntime(output), { code: "EEXIST" });
    assert.equal(await readFile(sentinel, "utf8"), "keep");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test(
  "an installed SDK runs a built application after relocation without source or dev providers",
  { timeout: 40000 },
  async () => {
    const directory = await mkdtemp(join(tmpdir(), "hd-runtime-installed-"));
    try {
      const sdk = join(directory, "sdk");
      cpSync(join(root, "packages/server-sdk"), sdk, { recursive: true });
      const app = join(directory, "app");
      cpSync(join(root, "examples/server-api"), app, { recursive: true });
      const manifest = join(app, "hd.toml");
      writeFileSync(
        manifest,
        readFileSync(manifest, "utf8").replace('"../../packages/server-sdk"', JSON.stringify(sdk)),
      );
      const hd = process.env.HD_SDK_TEST_HD ?? resolve(root, "target/debug/hd");
      execFileSync(hd, ["build", "--engine", "node"], {
        cwd: app,
        env: { ...process.env, HD_CACHE: join(directory, "cache") },
        stdio: "pipe",
        timeout: 30000,
      });
      const build = join(app, "build/debug");
      const binding = readdirSync(build).find((name) => name.endsWith(".host.mjs"));
      assert.ok(binding);
      rmSync(join(app, "src"), { recursive: true });
      rmSync(join(sdk, "src"), { recursive: true });
      rmSync(join(sdk, "providers"), { recursive: true });
      const script = join(directory, "application.mjs");
      const url = (file) => JSON.stringify(pathToFileURL(file).href);
      await writeFile(
        script,
        `
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { emitPortableRuntime } from ${url(join(sdk, "packaging/portable-runtime.mjs"))};
import { readProductionGrants } from ${url(join(sdk, "packaging/grants.mjs"))};
import { createApplication } from ${url(join(sdk, "adapters/application.mjs"))};
import * as bindings from ${url(join(build, binding))};
const runtime = await emitPortableRuntime(${JSON.stringify(join(directory, "emitted-runtime"))});
const { createCoreHost } = await import(runtime.coreURL);
const module = new WebAssembly.Module(await readFile(${JSON.stringify(join(build, "server_api.wasm"))}));
const sections = WebAssembly.Module.customSections(module, "hd.capabilities");
assert.equal(sections.length, 1);
assert.equal(new DataView(sections[0]).getUint32(0, true), runtime.packageHostABI);
assert.deepEqual(new Uint8Array(sections[0]), bindings.contract);
const output = [], errors = [], counts = [];
let stopped;
const completion = new Promise((resolve) => { stopped = resolve; });
const application = createApplication({ module, bindings, createCoreHost,
  grants: await readProductionGrants(${JSON.stringify(app)}),
  sink: { out: (text) => output.push(text), err: (text) => errors.push(text) }, onStopped: stopped });
for (const message of ["one", "two", "three"]) {
  const result = await application.dispatch({ method: { tag: "Post", fields: {} },
    url: "https://installed.test/", query: [], headers: [], redirects: 10,
    body: new TextEncoder().encode(JSON.stringify({ message })) });
  assert.equal(result.response.status, 200);
  const reply = JSON.parse(new TextDecoder().decode(result.response.body));
  assert.equal(reply.message, message);
  counts.push(reply.count);
}
application.stop();
await completion;
assert.deepEqual(errors, []);
console.log(JSON.stringify({ counts, output: output.join("") }));
`,
      );
      const stdout = execFileSync(process.execPath, [script], {
        cwd: directory,
        encoding: "utf8",
        timeout: 10000,
      });
      assert.deepEqual(JSON.parse(stdout), { counts: [1, 2, 3], output: "total 3\n" });
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  },
);
