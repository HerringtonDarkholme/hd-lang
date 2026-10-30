import assert from "node:assert/strict";
import http from "node:http";
import net from "node:net";
import { once } from "node:events";
import { pathToFileURL } from "node:url";
import { join } from "node:path";

// Observe the actual parent-side 200ms backstops without replacing their clocks.
const nativeSetTimeout = globalThis.setTimeout,
  nativeClearTimeout = globalThis.clearTimeout;
const activeCleanupTimers = new Set();
let cleanupBackstops = 0;
globalThis.setTimeout = (callback, delay, ...args) => {
  let timer;
  timer = nativeSetTimeout(function () {
    if (delay === 200) {
      activeCleanupTimers.delete(timer);
      cleanupBackstops++;
    }
    callback.apply(this, args);
  }, delay);
  if (delay === 200) {
    activeCleanupTimers.add(timer);
  }
  return timer;
};
globalThis.clearTimeout = (timer) => {
  activeCleanupTimers.delete(timer);
  return nativeClearTimeout(timer);
};
const entry = await import(pathToFileURL(join(process.argv[2], "index.mjs")).href);
await entry.startup;
// Larger native header limit lets the SDK's own 64KiB ingress limit run.
const server = http.createServer({ maxHeaderSize: 128 * 1024 }, entry.default);
server.listen(0, "127.0.0.1");
await once(server, "listening");
const port = server.address().port,
  url = "http://127.0.0.1:" + port;
const requestTimeout = 5000;
function post(body) {
  return fetch(url + "/", {
    method: "POST",
    body,
    signal: AbortSignal.timeout(requestTimeout),
  });
}
async function stalled(status, extraHeaders, bodyPrefix = "x") {
  const socket = net.createConnection({ host: "127.0.0.1", port });
  const chunks = [];
  let received = false;
  const complete = new Promise((resolve, reject) => {
    const timeout = nativeSetTimeout(() => {
      socket.destroy();
      reject(Error("stalled ingress never received its response"));
    }, requestTimeout);
    socket.on("data", (chunk) => {
      received = true;
      chunks.push(chunk);
    });
    socket.once("error", (error) => {
      nativeClearTimeout(timeout);
      reject(error);
    });
    socket.once("end", () => {
      nativeClearTimeout(timeout);
      resolve(Buffer.concat(chunks).toString());
    });
  });
  await once(socket, "connect");
  const length = status === 413 ? 1024 * 1024 + 1 : 1024 * 1024;
  // Deliberately keep the upload open. The server must produce its early error
  // and close the connection without waiting for the client to finish or abort.
  socket.write(
    "POST / HTTP/1.1\r\nHost: 127.0.0.1:" +
      port +
      "\r\nContent-Length: " +
      length +
      "\r\n" +
      extraHeaders +
      "\r\n" +
      bodyPrefix,
  );
  const response = await complete;
  assert.equal(received, true);
  assert.match(response, new RegExp("^HTTP/1\\.1 " + status + " "));
  assert.match(response, /connection: close/i);
  assert.ok(
    response.includes(status === 431 ? "request headers too large" : "request body too large"),
  );
}
try {
  const counts = [];
  for (const message of ["one", "two", "three"]) {
    const response = await post(JSON.stringify({ message }));
    assert.equal(response.status, 200);
    const value = await response.json();
    assert.equal(value.message, message);
    counts.push(value.count);
  }
  assert.deepEqual(counts, [1, 2, 3]);
  const invalid = await post("{broken");
  assert.equal(invalid.status, 400);
  await invalid.arrayBuffer();
  // The declared size must be rejected before any body arrives. An eager
  // fetch upload can race the intentional early connection close with EPIPE.
  // The paused probe checks the response body and close as well as the status.
  await stalled(413, "", "");
  const malformed = await new Promise((resolve, reject) => {
    const req = http.request(
      url + "/",
      {
        method: "POST",
        headers: { host: "private@authority.test" },
        signal: AbortSignal.timeout(requestTimeout),
      },
      (response) => {
        const chunks = [];
        response.on("error", reject);
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () =>
          resolve({ status: response.statusCode, body: Buffer.concat(chunks).toString() }),
        );
      },
    );
    req.on("error", reject);
    req.end(JSON.stringify({ message: "must not admit" }));
  });
  assert.deepEqual(malformed, { status: 400, body: "bad request" });
  await stalled(431, "X-Padding: " + "x".repeat(64 * 1024) + "\r\n");
  await stalled(413, "");
  const response = await post(JSON.stringify({ message: "healthy" }));
  assert.equal(response.status, 200);
  const healthy = await response.json();
  assert.equal(healthy.message, "healthy");
  assert.equal(
    healthy.count,
    4,
    "rejected native uploads must not poison or replace the retained worker",
  );
  assert.equal(cleanupBackstops, 0);
  assert.equal(activeCleanupTimers.size, 0);
  console.log(
    "VERCEL-RETAINED-CONTROL:" +
      JSON.stringify({ counts, healthy: healthy.count, stalled: [431, 413], cleanupBackstops }),
  );
} finally {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
  await entry.close();
  globalThis.setTimeout = nativeSetTimeout;
  globalThis.clearTimeout = nativeClearTimeout;
}
