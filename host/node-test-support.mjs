import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { MessageChannel, Worker } from "node:worker_threads";

import { METHODS } from "./gen/abi.mts";
import { createHost } from "./node.mjs";
import { Dec, Enc, str } from "./wire.mts";
import { receiveReply } from "./worker-control.mts";

export function fixture(t, files) {
  // The resolved path: grant errors report a path with its links resolved,
  // and the temporary directory may sit behind one (macOS /var).
  const root = realpathSync(mkdtempSync(join(tmpdir(), "hd-cgroup-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [name, content] of Object.entries(files)) {
    const path = join(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
}

export function netWorker(t, options = {}) {
  const { port1, port2 } = new MessageChannel();
  const flag = new Int32Array(new SharedArrayBuffer(4));
  const worker = new Worker(options.url ?? new URL("./net.mjs", import.meta.url), {
    workerData: { port: port2, flag, ...options.data },
    transferList: [port2],
  });
  t.after(async () => {
    if (options.close) {
      call({ op: options.close });
    }
    port1.close();
    await worker.terminate();
  });
  let nextCall = 1;
  const call = (request, transfer = []) => {
    const id = nextCall++;
    const deadline = performance.now() + 5000;
    Atomics.store(flag, 0, 0);
    port1.postMessage({ id, request }, transfer);
    const reply = receiveReply(port1, flag, id, () => deadline - performance.now());
    assert.ok(reply, "worker control reply");
    return reply.result;
  };
  call.worker = worker;
  return call;
}

// Exercise the real provider through the ABI, including complete-before-finish.
export function netHost(t, trait = "hd:Net", env = {}, make = createHost) {
  const host = make({ out() {}, err() {} }, env);
  host.core.st.deadline = performance.now() + 5000;
  t.after(() => host.close());
  const memory = new WebAssembly.Memory({ initial: trait === "hd:Process" ? 32 : 1 });
  host.prime({ exports: { "hd.x": memory } });
  const imports = host.imports[trait];
  const decode = (name, len) =>
    METHODS[trait][name].result.dec(new Dec(new Uint8Array(memory.buffer, 0, len)));
  const call = (name, ...args) => {
    const method = METHODS[trait][name];
    const encoded = new Enc(),
      raw = [];
    if (method.params.length === 1 && method.params[0] === str) {
      const bytes = new TextEncoder().encode(args[0]);
      new Uint8Array(memory.buffer).set(bytes);
      raw.push(bytes.length);
    } else {
      method.params.forEach((param, index) => {
        if (typeof param === "string") {
          raw.push(args[index]);
        } else {
          param.enc(encoded, args[index]);
        }
      });
      if (raw.length !== method.params.length) {
        new Uint8Array(memory.buffer).set(encoded.done());
        raw.push(encoded.len);
      }
    }
    if (method.wait === "never") {
      return { result: decode(name, imports[name](...raw)) };
    }
    const [status, value] = imports[`${name}.start`](...raw);
    return status === 0 ? { result: decode(name, value) } : { operation: value };
  };
  const take = (name, started) => {
    if (started.result) {
      return started.result;
    }
    if (!host.core.st.ops.get(started.operation).completed) {
      host.imports["hd:rt"].block();
    }
    return decode(name, imports[`${name}.finish`](started.operation));
  };
  return { host, call, take };
}

export async function tcpHost(t) {
  let connected;
  const arrived = new Promise((resolve) => {
    connected = resolve;
  });
  const server = net.createServer((socket) => connected(socket));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  const api = netHost(t);
  const stream = api.take("connect", api.call("connect", "127.0.0.1", server.address().port));
  assert(stream.ok);
  const peer = await arrived;
  t.after(() => peer.destroy());
  return { ...api, handle: stream.value, peer };
}
