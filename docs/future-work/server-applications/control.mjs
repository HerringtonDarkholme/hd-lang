// Live protocol-neutral broker control; not a production deployment SDK.
// Usage: node control.mjs HD node|wasmtime library|entry MODE OUTPUT.json
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import { fileURLToPath } from "node:url";

const [hd, engine, shape, mode, output] = process.argv.slice(2);
assert(hd && output);
assert(["node", "wasmtime"].includes(engine));
assert(["library", "entry", "typed"].includes(shape));
assert(["http", "worker", "queue", "cron", "drain", "signal"].includes(mode));
const clients = new Map();
const replies = [];
const queued = [];
let draining = false;
let next = 0;
let active = 0;
let peak = 0;
let child;
let childEnded = false;
let stdout = "";
let stderr = "";
let httpServer;
let signalCount = 0;
let failure;
const accepted = [];

function killChild() {
  if (!child || childEnded) return;
  try {
    if (process.platform === "win32") child.kill("SIGKILL");
    else process.kill(-child.pid, "SIGKILL");
  } catch {
    /* A child that already exited needs no kill. */
  }
}
function fail(error) {
  failure ??= error;
  writeFileSync(
    output + ".failed",
    JSON.stringify(
      {
        message: String(error),
        stdout,
        stderr,
        active,
        peak,
        queued: queued.length,
        accepted,
        clients: [...clients].map(([id, c]) => ({ id, busy: c.busy?.index, stopped: c.stopped })),
        replies,
      },
      null,
      2,
    ),
  );
  httpServer?.closeAllConnections();
  killChild();
  for (const { socket } of clients.values()) socket.destroy();
}
function send(socket, text) {
  const body = Buffer.from(text);
  assert(body.length <= 65536);
  const frame = Buffer.alloc(4 + body.length);
  frame.writeUInt32LE(body.length);
  body.copy(frame, 4);
  socket.write(frame);
}
// Allocates once per incoming frame; fragmented input is copied linearly.
function frames(socket, receive) {
  let header = Buffer.alloc(4),
    head = 0,
    body = null,
    used = 0;
  socket.on("data", (chunk) => {
    try {
      let at = 0;
      while (at < chunk.length) {
        if (body === null) {
          const n = Math.min(4 - head, chunk.length - at);
          chunk.copy(header, head, at, at + n);
          at += n;
          head += n;
          if (head !== 4) continue;
          const size = header.readUInt32LE();
          assert(size <= 65536, "frame too large");
          body = Buffer.alloc(size);
          used = 0;
        }
        const n = Math.min(body.length - used, chunk.length - at);
        chunk.copy(body, used, at, at + n);
        used += n;
        at += n;
        if (used === body.length) {
          const text = body.toString("utf8");
          body = null;
          head = 0;
          receive(text);
        }
      }
    } catch (error) {
      fail(error);
    }
  });
  socket.on("error", fail);
}
function stop() {
  draining = true;
  for (const c of clients.values())
    if (!c.busy && !c.stopped) {
      c.stopped = true;
      send(c.socket, "stop");
    }
}
function dispatch() {
  if (clients.size !== 2) return;
  for (const c of clients.values())
    if (!c.busy && !c.stopped) {
      if (draining) {
        c.stopped = true;
        send(c.socket, "stop");
        continue;
      }
      const item = queued.shift();
      if (!item) continue;
      c.busy = item;
      active++;
      peak = Math.max(active, peak);
      send(c.socket, JSON.stringify(item.event));
    }
  if (mode === "signal" && active === 2) {
    console.log(JSON.stringify({ event: "active", count: active }));
  }
}
function enqueue(event, respond) {
  queued.push({ index: next++, event, respond });
  dispatch();
}
const broker = net.createServer((socket) => {
  const observed = { bytes: 0, frames: 0 };
  accepted.push(observed);
  socket.on("data", (b) => {
    observed.bytes += b.length;
  });
  socket.setNoDelay(true);
  let c;
  frames(socket, (text) => {
    observed.frames++;
    if (!c) {
      assert(["0", "1"].includes(text), "bad worker hello");
      const slot = Number(text);
      assert(Number.isInteger(slot) && slot >= 0 && slot < 2 && !clients.has(slot));
      c = { socket, busy: null, stopped: false };
      clients.set(slot, c);
      dispatch();
      return;
    }
    assert(c.busy, "unsolicited response");
    const item = c.busy;
    c.busy = null;
    active--;
    const reply = JSON.parse(text);
    replies.push({ index: item.index, reply });
    item.respond?.(reply);
    if (mode === "drain" || (mode !== "http" && mode !== "signal" && replies.length === next))
      stop();
    if (draining) stop();
    else dispatch();
  });
});
await new Promise((resolve, reject) => {
  broker.once("error", reject);
  broker.listen(0, "127.0.0.1", resolve);
});
const port = broker.address().port;
if (mode === "http") {
  httpServer = http.createServer((request, response) => {
    const chunks = [];
    let bytes = 0;
    request.on("data", (part) => {
      bytes += part.length;
      if (bytes > 65536) request.destroy();
      else chunks.push(part);
    });
    request.on("end", () =>
      enqueue({ kind: "http", body: Buffer.concat(chunks).toString("utf8") }, (reply) => {
        response.writeHead(reply.status, { "content-type": "application/json" });
        response.end(reply.text);
      }),
    );
  });
  await new Promise((resolve) => httpServer.listen(0, "127.0.0.1", resolve));
} else {
  const kind = mode === "drain" || mode === "signal" ? "hold" : mode;
  for (const n of mode === "queue" || mode === "cron"
    ? [7, 7, 8]
    : mode === "drain" || mode === "signal"
      ? [3, 4, 99]
      : [3, 4])
    enqueue({ kind, number: n });
}
const finished = new Promise((resolve, reject) => {
  child = spawn(
    hd,
    ["run", "--jobs", "2", "--engine", engine, "--cap", "Net=127.0.0.1", "--", String(port)],
    {
      cwd: fileURLToPath(new URL(`./prototypes/live-${shape}/`, import.meta.url)),
      detached: process.platform !== "win32",
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.on("error", reject);
  child.stdout.on("data", (b) => {
    stdout += b;
  });
  child.stderr.on("data", (b) => {
    stderr += b;
  });
  child.on("close", (code, signal) => {
    childEnded = true;
    resolve({ code, signal });
  });
});
process.on("SIGTERM", () => {
  signalCount++;
  if (signalCount === 1) stop();
  else killChild();
});
const watchdog = setTimeout(() => fail(Error("control exceeded 20 seconds")), 20000);
try {
  let httpResults;
  if (mode === "http") {
    const url = `http://127.0.0.1:${httpServer.address().port}/greet`;
    httpResults = await Promise.all(
      ['{"name":"Ada"}', "bad json"].map(async (body) => {
        const response = await fetch(url, { method: "POST", body });
        return { status: response.status, body: await response.text() };
      }),
    );
    assert.deepEqual(httpResults, [
      { status: 200, body: '{"greeting":"Hello Ada"}' },
      { status: 400, body: "" },
    ]);
    stop();
  }
  const result = await finished;
  if (failure) throw failure;
  assert.equal(result.code, 0, stderr);
  assert.equal(result.signal, null);
  assert.equal(stderr, "");
  assert(peak <= 2);
  const count = mode === "http" ? 0 : mode === "queue" || mode === "cron" ? 2 : 7;
  assert.equal(stdout, `count=${count} stopped=true\n`);
  assert.equal(queued.length, mode === "drain" || mode === "signal" ? 1 : 0);
  assert.equal(replies.length, mode === "queue" || mode === "cron" ? 3 : 2);
  if (mode === "signal") assert.equal(signalCount, 1);
  writeFileSync(
    output,
    JSON.stringify(
      {
        engine,
        shape,
        mode,
        result,
        stdout,
        stderr,
        peak,
        queued: queued.length,
        replies: replies.toSorted((a, b) => a.index - b.index),
        httpResults,
        signalCount,
      },
      null,
      2,
    ) + "\n",
  );
  console.log(
    JSON.stringify({
      mode,
      engine,
      shape,
      completed: replies.length,
      queued: queued.length,
      peak,
      signalCount,
    }),
  );
} finally {
  clearTimeout(watchdog);
  killChild();
  for (const { socket } of clients.values()) socket.destroy();
  broker.close();
  httpServer?.closeAllConnections();
  httpServer?.close();
}
