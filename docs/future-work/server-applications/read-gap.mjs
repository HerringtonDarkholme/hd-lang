// A peer replies only after both clients have announced themselves.
// Node's current blocking read holds the guest before its sibling connects.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import net from "node:net";
import { fileURLToPath } from "node:url";

const [hd, engine, output] = process.argv.slice(2);
assert(hd && output && ["node", "wasmtime"].includes(engine));
const sockets = [];
let ready = 0;
const server = net.createServer((socket) => {
  sockets.push(socket);
  let text = "";
  socket.on("data", (bytes) => {
    text += bytes.toString();
    if (text === "ready") {
      ready++;
      if (ready === 2) for (const peer of sockets) peer.write("x");
    }
  });
  socket.on("error", () => {});
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const child = spawn(
  hd,
  [
    "run",
    "--jobs",
    "2",
    "--engine",
    engine,
    "--time-limit",
    "1s",
    "--cap",
    "Net=127.0.0.1",
    "--",
    String(server.address().port),
  ],
  {
    cwd: fileURLToPath(new URL("./prototypes/read-concurrency/", import.meta.url)),
    detached: process.platform !== "win32",
    stdio: ["ignore", "pipe", "pipe"],
  },
);
let stdout = "",
  stderr = "";
child.stdout.on("data", (b) => {
  stdout += b;
});
child.stderr.on("data", (b) => {
  stderr += b;
});
let killed = false;
const guard = setTimeout(() => {
  killed = true;
  try {
    if (process.platform === "win32") child.kill("SIGKILL");
    else process.kill(-child.pid, "SIGKILL");
  } catch {}
}, 10000);
try {
  const result = await new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code, signal) => resolve({ code, signal }));
  });
  assert.equal(killed, false, "CLI time limit must bound the wait");
  if (engine === "wasmtime") {
    assert.equal(result.code, 0, stderr);
    assert.equal(stdout, "1 1\n");
    assert.equal(stderr, "");
    assert.equal(ready, 2);
  } else {
    assert.equal(result.code, 3, stderr);
    assert.equal(stdout, "");
    assert.match(stderr, /panic: time-limit/);
    assert.equal(ready, 1);
  }
  const record = {
    engine,
    result,
    stdout,
    stderr,
    connections: sockets.length,
    ready,
    verdict: engine === "node" ? "current host gap reproduced" : "both children progress",
  };
  writeFileSync(output, JSON.stringify(record, null, 2) + "\n");
  console.log(JSON.stringify(record));
} finally {
  clearTimeout(guard);
  for (const socket of sockets) socket.destroy();
  server.close();
}
