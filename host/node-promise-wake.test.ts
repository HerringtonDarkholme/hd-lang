import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";

test(
  "an idle Node host stays alive without a polling timer and releases its port",
  {
    skip: process.platform === "win32",
    timeout: 5_000,
  },
  async () => {
    const source = `
    import { nodeLayer } from ${JSON.stringify(new URL("./node.ts", import.meta.url).href)};
    globalThis.setTimeout = () => { throw new Error('no polling timer'); };
    globalThis.setImmediate = () => { throw new Error('no polling loop'); };
    const platform = nodeLayer.providers({env:{},st:{},grant:{},flush(){},left:()=>Infinity});
    const release = platform.keepAlive();
    const stopped = new Promise(resolve => process.once('SIGUSR2', resolve));
    process.stdout.write('waiting\\n');
    await stopped;
    release();
    platform.close();
  `;
    // Cost: one child proves the process-lifetime behavior, without other I/O handles.
    const child = spawn(process.execPath, ["--input-type=module", "-e", source], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let errors = "";
    child.stderr.on("data", (data) => {
      errors += data;
    });
    const exited = once(child, "exit");
    try {
      const [ready] = await once(child.stdout, "data");
      assert.equal(String(ready), "waiting\n");
      await delay(20);
      assert.equal(child.exitCode, null, errors);
      child.kill("SIGUSR2");
      const [code, signal] = await exited;
      assert.equal(code, 0, errors);
      assert.equal(signal, null);
    } finally {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill("SIGKILL");
      }
    }
  },
);
