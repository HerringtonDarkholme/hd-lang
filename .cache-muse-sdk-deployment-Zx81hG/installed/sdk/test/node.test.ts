import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { once } from "node:events";
import { resolve, join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const hd = process.env.HD_SDK_TEST_HD ?? resolve(root, "target/debug/hd");

function killProcess(pid) {
  try {
    process.kill(pid, "SIGKILL");
  } catch (error) {
    if (error.code !== "ESRCH") {
      throw error;
    }
  }
}

test(
  "one ordinary main answers real JSON requests and concurrent slow handlers",
  { timeout: 30000 },
  async () => {
    const cache = await mkdtemp(join(root, ".cache-muse-266-sdk-"));
    const child = spawn(hd, ["run", "--engine", "node"], {
      cwd: join(root, "examples/server-api"),
      env: { ...process.env, HD_CACHE: cache, HD_SERVER_PORT: "0" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    const closed = once(child, "close");
    const lines = [];
    let errors = "";
    child.stderr.setEncoding("utf8").on("data", (chunk) => {
      errors += chunk;
    });
    const output = createInterface({ input: child.stdout });
    let listening;
    const ready = new Promise((accept, reject) => {
      output.on("line", (line) => {
        lines.push(line);
        if (line.startsWith('{"event":"listening"')) {
          listening = JSON.parse(line);
          accept(listening);
        }
      });
      child.once("error", reject);
      child.once("close", (code) =>
        reject(new Error(`server ended before admission: ${code}: ${errors}`)),
      );
    });
    const deadline = AbortSignal.timeout(15000);
    try {
      const address = await Promise.race([
        ready,
        new Promise((_, reject) =>
          deadline.addEventListener("abort", () => reject(new Error("server did not listen")), {
            once: true,
          }),
        ),
      ]);
      assert.equal(address.host, "127.0.0.1");
      const url = `http://127.0.0.1:${address.port}`;
      const request = async (path, body) => {
        const response = await fetch(url + path, {
          method: "POST",
          headers: { "content-type": "application/json", connection: "close" },
          body,
          signal: deadline,
        });
        return { status: response.status, body: await response.text() };
      };
      for (const [index, message] of ["one", "two", "🦉 three"].entries()) {
        const response = await request("/", JSON.stringify({ message }));
        assert.equal(response.status, 200);
        assert.deepEqual(JSON.parse(response.body), { count: index + 1, message });
      }
      assert.equal((await request("/", "{bad JSON")).status, 400);
      const responses = await Promise.all(
        [0, 1, 2].map((item) => request("/slow", JSON.stringify({ message: `slow ${item}` }))),
      );
      assert.ok(responses.every((response) => response.status === 200));
      assert.deepEqual(
        responses.map((response) => JSON.parse(response.body).count).sort(),
        [4, 5, 6],
      );
      process.kill(listening.pid, "SIGTERM");
      const [code, signal] = await closed;
      assert.equal(code, 0, errors);
      assert.equal(signal, null);
      assert.equal(errors, "");
      const stopped = lines.filter((line) => line.startsWith('{"event":"stopped"')).map(JSON.parse);
      assert.equal(stopped.length, 1, "one retained instance stops once");
      assert.ok(stopped[0].peak >= 3, "the slow handlers actually overlap");
      assert.equal(lines.at(-1), "total 6");
    } finally {
      if (child.exitCode === null && child.signalCode === null) {
        if (listening) {
          killProcess(listening.pid);
        }
        child.kill("SIGKILL");
        await closed;
      }
      output.close();
      await rm(cache, { recursive: true, force: true });
    }
  },
);

test(
  "disconnect releases one lease and the retained workers keep serving",
  { timeout: 30000 },
  async () => {
    const cache = await mkdtemp(join(root, ".cache-muse-266-disconnect-"));
    const child = spawn(hd, ["run", "--engine", "node"], {
      cwd: join(root, "examples/server-api"),
      env: { ...process.env, HD_CACHE: cache, HD_SERVER_PORT: "0" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    const closed = once(child, "close");
    let errors = "",
      listening,
      acceptReady,
      acceptFinished;
    const ready = new Promise((resolve) => {
      acceptReady = resolve;
    });
    const finished = new Promise((resolve) => {
      acceptFinished = resolve;
    });
    const output = createInterface({ input: child.stdout });
    child.stderr.setEncoding("utf8").on("data", (text) => {
      errors += text;
    });
    output.on("line", (line) => {
      if (line.startsWith('{"event":"listening"')) {
        listening = JSON.parse(line);
        acceptReady(listening);
      }
      if (line.startsWith('{"event":"request-finished"')) {
        acceptFinished(JSON.parse(line));
      }
    });
    const deadline = AbortSignal.timeout(15000);
    const aborted = new Promise((_, reject) =>
      deadline.addEventListener(
        "abort",
        () => reject(new Error("disconnect control timed out: " + errors)),
        { once: true },
      ),
    );
    let slow;
    try {
      const address = await Promise.race([ready, aborted]);
      const url = `http://127.0.0.1:${address.port}`;
      const controller = new AbortController();
      slow = fetch(url + "/slow", {
        method: "POST",
        body: '{"message":"cancel me"}',
        signal: AbortSignal.any([controller.signal, deadline]),
      }).then(
        (response) => ({ response }),
        (error) => ({ error }),
      );
      let fast = 0,
        started = false;
      // Only these fast requests and the slow request can change the count.
      // A count above `fast` proves the slow HD handler entered before abort.
      while (fast < 20 && !started) {
        const response = await fetch(url, {
          method: "POST",
          body: '{"message":"other"}',
          signal: deadline,
        });
        assert.equal(response.status, 200);
        fast++;
        started = (await response.json()).count > fast;
      }
      assert.ok(started, "the disconnected handler must have started");
      controller.abort();
      assert.ok((await slow).error, "the client observes its own abort");
      const event = await Promise.race([finished, aborted]);
      assert.equal(event.cancelled, "Disconnected");
      assert.equal(event.abandoned, false);
      const after = await fetch(url, {
        method: "POST",
        body: '{"message":"after"}',
        signal: deadline,
      });
      assert.equal(after.status, 200);
      assert.deepEqual(await after.json(), { count: fast + 2, message: "after" });
      process.kill(listening.pid, "SIGTERM");
      const [code, signal] = await closed;
      assert.equal(code, 0, errors);
      assert.equal(signal, null);
      assert.equal(errors, "");
    } finally {
      if (child.exitCode === null && child.signalCode === null) {
        if (listening) {
          killProcess(listening.pid);
        }
        child.kill("SIGKILL");
        await closed;
      }
      await slow;
      output.close();
      await rm(cache, { recursive: true, force: true });
    }
  },
);
