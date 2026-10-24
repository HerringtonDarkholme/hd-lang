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
    if (error.code !== "ESRCH") throw error;
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
        [0, 1, 2].map((n) => request("/slow", JSON.stringify({ message: `slow ${n}` }))),
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
        if (listening) killProcess(listening.pid);
        child.kill("SIGKILL");
        await closed;
      }
      output.close();
      await rm(cache, { recursive: true, force: true });
    }
  },
);
