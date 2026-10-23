if (!process.argv[3]) throw new Error("pass the Edge Runtime tool directory");
if (!process.argv[2]) throw new Error("pass a scratch evidence directory");
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { SourceTextModule, isContext } from "node:vm";
const require = createRequire(`${process.argv[3]}/probe.cjs`);
const { EdgeVM } = require("@edge-runtime/vm");
const edge = new EdgeVM();
assert.equal(isContext(edge.context), true);
const modules = new Map();
const root = new URL("./fetch-control.mjs", import.meta.url);
function load(url) {
  const key = url.href;
  if (modules.has(key)) return modules.get(key);
  const m = new SourceTextModule(readFileSync(url, "utf8"), {
    context: edge.context,
    identifier: key,
  });
  modules.set(key, m);
  return m;
}
const linked = load(root);
await linked.link((specifier, source) => load(new URL(specifier, source.identifier)));
await linked.evaluate();
const invoke = linked.namespace.invoke;
const bytes = readFileSync(new URL("./app/build/debug/muse-http-prototype.wasm", import.meta.url));
const wasm = await WebAssembly.compile(bytes);
let active = 0,
  peak = 0;
const upstream = createServer((req, res) => {
  active++;
  peak = Math.max(peak, active);
  setTimeout(() => {
    active--;
    res.end("upstream");
  }, 30);
});
await new Promise((r) => upstream.listen(0, "127.0.0.1", r));
try {
  const rows = [];
  const url = `http://127.0.0.1:${upstream.address().port}`;
  for (const [name, status, expected] of [
    ["json", 200, '{"greeting":"Hello Ada"}'],
    ["timeout", 504, "deadline"],
    ["fetch", 200, "upstream"],
  ]) {
    const got = await invoke(
      wasm,
      new edge.context.Request(`https://handler.test/${name}`, {
        method: name === "timeout" ? "GET" : "POST",
        body: name === "fetch" ? url : name === "json" ? '{"name":"Ada"}' : undefined,
      }),
    );
    const response = { status: got.response.status, body: await got.response.text() };
    assert.equal(response.status, status);
    assert.equal(response.body, expected);
    rows.push({ name, ...got, response });
  }
  const all = await Promise.all(
    Array.from({ length: 8 }, () =>
      invoke(
        wasm,
        new edge.context.Request("https://handler.test/fetch", { method: "POST", body: url }),
      ),
    ),
  );
  assert.equal(peak, 8);
  for (const got of all) {
    assert.equal(got.response.status, 200);
    assert.equal(await got.response.text(), "upstream");
  }
  const proof = {
    node: process.version,
    vm: "@edge-runtime/vm@5.0.0",
    primitives: "6.0.0",
    productionVercel: false,
    features: edge.evaluate(
      "({Suspending:typeof WebAssembly.Suspending,promising:typeof WebAssembly.promising})",
    ),
    peak,
    rows,
  };
  writeFileSync(`${process.argv[2]}/edge-control.json`, JSON.stringify(proof, null, 2));
  console.log(
    JSON.stringify({
      features: proof.features,
      peak,
      rows: rows.map((x) => ({ name: x.name, response: x.response, blockCalls: x.blockCalls })),
    }),
  );
} finally {
  upstream.closeAllConnections();
  await new Promise((r) => upstream.close(r));
}
