import assert from "node:assert/strict";
import { test } from "node:test";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { createVercelNodeHandler } from "../adapters/vercel-node.mjs";

class NativeResponse extends EventEmitter {
  headers = {};
  headersSent = false;
  writableFinished = false;
  destroyed = false;
  setHeader(name, value) {
    this.headers[name.toLowerCase()] = value;
  }
  writeHead(status, headers = {}) {
    this.status = status;
    for (const [name, value] of Object.entries(headers)) this.setHeader(name, value);
    this.headersSent = true;
  }
  end(bytes) {
    this.body = Buffer.from(bytes ?? "");
    this.writableFinished = true;
    this.emit("finish");
  }
  destroy() {
    this.destroyed = true;
    this.emit("close");
  }
}
function incoming(patch = {}) {
  const stream = new PassThrough();
  Object.assign(stream, {
    method: "POST",
    url: "/",
    headers: { host: "trusted.test" },
    rawHeaders: ["Host", "trusted.test"],
    ...patch,
  });
  stream.end("native body");
  return stream;
}
async function invoke(handler, patch) {
  const request = incoming(patch),
    response = new NativeResponse();
  await handler(request, response);
  assert.equal(request.listenerCount("aborted"), 0);
  assert.equal(response.listenerCount("close"), 0);
  return { request, response };
}
async function view(request) {
  const body = await new Response(request.body).text();
  return new Response(JSON.stringify({ method: request.method, url: request.url, body }));
}

test("malformed native authority, target and Headers return generic 400 before dispatch", async () => {
  let calls = 0;
  const records = [],
    handler = createVercelNodeHandler(
      {
        async fetch(request) {
          calls++;
          return view(request);
        },
      },
      { log: (record) => records.push(record) },
    );
  for (const patch of [
    { headers: { host: undefined } },
    { headers: { host: "" } },
    { headers: { host: "private@authority.test" } },
    { headers: { host: "private/path" } },
    { headers: { host: "private\\authority" } },
    { headers: { host: "private authority" } },
    { headers: { host: "[invalid-ipv6" } },
    { url: undefined },
    { url: "/path\\other" },
    { url: "/path\u0001other" },
    { url: "https://private-target.test/" },
    { rawHeaders: ["bad name", "private-value"] },
    { rawHeaders: ["x-header", "private\nvalue"] },
  ]) {
    const { request, response } = await invoke(handler, patch);
    assert.equal(response.status, 400);
    assert.equal(response.body.toString(), "bad request");
    assert.equal(response.headers.connection, "close");
    assert.equal(request.listenerCount("data"), 0);
    assert.equal(calls, 0);
  }
  assert.ok(records.length >= 10);
  assert.ok(!JSON.stringify(records).includes("private"));
  const { response } = await invoke(handler);
  assert.equal(response.status, 200);
  assert.equal(calls, 1, "the same handler remains healthy after ingress rejection");
  assert.equal(JSON.parse(response.body).body, "native body");
});

test("native GET and HEAD bodies and double slash paths keep their original authority", async () => {
  const handler = createVercelNodeHandler({ fetch: view });
  for (const method of ["GET", "HEAD"]) {
    const { response } = await invoke(handler, { method, url: "//foreign.test/path" });
    assert.equal(response.status, 200);
    const value = JSON.parse(response.body);
    assert.equal(value.method, method);
    assert.equal(value.body, "native body");
    assert.equal(value.url, "https://trusted.test//foreign.test/path");
  }
});

test("cold native requests return immediate 503 without dispatch or queued ownership", async () => {
  let calls = 0;
  const handler = createVercelNodeHandler(
    {
      fetch() {
        calls++;
        throw Error("must not dispatch");
      },
    },
    { isReady: () => false, retryAfter: "2" },
  );
  for (let n = 0; n < 100; n++) {
    const { request, response } = await invoke(handler);
    assert.equal(response.status, 503);
    assert.equal(response.headers["retry-after"], "2");
    assert.equal(response.headers.connection, "close");
    assert.equal(request.listenerCount("data"), 0);
  }
  assert.equal(calls, 0);
});

test("failure after validated ingress remains a generic internal 500", async () => {
  const handler = createVercelNodeHandler({
    async fetch(request) {
      await new Response(request.body).text();
      throw Error("private provider detail");
    },
  });
  const { response } = await invoke(handler);
  assert.equal(response.status, 500);
  assert.equal(response.body.toString(), "internal server error");
  assert.equal(response.headers.connection, "close");
});

test("native encoded paths and repeated query parameters reach the mapper intact", async () => {
  const handler = createVercelNodeHandler({ fetch: view });
  for (const target of ["/a%2Fb?q=x%20y", "/path?x=1&x=2", "//other.test/a%2Fb?x=%23"]) {
    const { response } = await invoke(handler, { url: target });
    assert.equal(response.status, 200);
    assert.equal(JSON.parse(response.body).url, "https://trusted.test" + target);
  }
});
