import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { invoke } from "./fetch-control.mjs";
const evidence = process.argv[2];
if (!evidence) throw new Error("pass a scratch evidence directory");
const bytes = readFileSync(new URL("./app/build/debug/muse-http-prototype.wasm", import.meta.url));
const module = await WebAssembly.compile(bytes);
let active = 0,
  peak = 0,
  completed = 0;
const upstream = createServer((req, res) => {
  active++;
  peak = Math.max(peak, active);
  let ended = false;
  const end = () => {
    if (!ended) {
      ended = true;
      active--;
    }
  };
  const t = setTimeout(() => {
    completed++;
    res.end("upstream");
    end();
  }, 30);
  res.on("close", () => {
    clearTimeout(t);
    end();
  });
});
await new Promise((r) => upstream.listen(0, "127.0.0.1", r));
try {
  const url = `http://127.0.0.1:${upstream.address().port}`;
  const rows = [];
  for (const [name, status, body] of [
    ["json", 200, '{"greeting":"Hello Ada"}'],
    ["timeout", 504, "deadline"],
    ["fetch", 200, "upstream"],
  ]) {
    const got = await invoke(
      module,
      new Request(`https://handler.test/${name}`, {
        method: name === "timeout" ? "GET" : "POST",
        body: name === "fetch" ? url : name === "json" ? '{"name":"Ada"}' : undefined,
      }),
    );
    const response = got.response
      ? { status: got.response.status, body: await got.response.text() }
      : null;
    rows.push({ name, ...got, response });
    assert.equal(response.status, status);
    assert.equal(response.body, body);
  }
  for (const [name, url, body, status] of [
    ["bad-json", "https://handler.test/json", "{", 400],
    ["bad-utf8", "https://handler.test/json", new Uint8Array([255]), 400],
    ["query", "https://handler.test/?x=1&x=2", undefined, 200],
  ]) {
    const got = await invoke(
      module,
      new Request(url, { method: body === undefined ? "GET" : "POST", body }),
    );
    const response = { status: got.response.status, body: await got.response.text() };
    assert.equal(response.status, status);
    if (name === "query") assert.equal(response.body, '"https://handler.test/?x=1&x=2"');
    rows.push({ name, ...got, response });
  }
  await assert.rejects(
    invoke(module, new Request("https://handler.test/blocking", { method: "POST", body: url })),
    /pending synchronous block_on requires JSPI or another driver/,
  );
  rows.push({
    name: "blocking",
    expectedError: "pending synchronous block_on requires JSPI or another driver",
  });
  const start = performance.now();
  const together = await Promise.all(
    Array.from({ length: 8 }, () =>
      invoke(module, new Request("https://handler.test/fetch", { method: "POST", body: url })),
    ),
  );
  const concurrentMs = performance.now() - start;
  const items = await Promise.all(
    together.map(async (x) => ({
      ...x,
      response: { status: x.response?.status, body: await x.response?.text() },
    })),
  );
  assert.equal(peak, 8);
  for (const x of items) {
    assert.equal(x.response.status, 200);
    assert.equal(x.response.body, "upstream");
  }
  rows.push({ name: "concurrent", concurrentMs, peak, completed, items });
  const controller = new AbortController();
  const pending = invoke(
    module,
    new Request("https://handler.test/fetch", {
      method: "POST",
      body: url,
      signal: controller.signal,
    }),
  );
  setTimeout(() => controller.abort(), 5);
  const cancelled = await pending;
  assert.equal(cancelled.result.status, 130);
  assert.equal(cancelled.aborted, 1);
  rows.push({ name: "cancel", ...cancelled });
  writeFileSync(
    `${evidence}/node-control.json`,
    JSON.stringify(
      {
        node: process.version,
        bytes: bytes.length,
        exports: WebAssembly.Module.exports(module),
        imports: WebAssembly.Module.imports(module),
        rows,
      },
      null,
      2,
    ),
  );
  console.log(
    JSON.stringify(
      rows.map((x) => ({
        name: x.name,
        response: x.response,
        peak: x.peak,
        ms: x.concurrentMs,
        status: x.result?.status,
        blockCalls: x.blockCalls,
        started: x.started,
        aborted: x.aborted,
        memories: x.memories,
      })),
      null,
      2,
    ),
  );
} finally {
  upstream.closeAllConnections();
  await new Promise((r) => upstream.close(r));
}
