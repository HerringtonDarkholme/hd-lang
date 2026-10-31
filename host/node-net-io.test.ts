import assert from "node:assert/strict";
import { once } from "node:events";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import dgram from "node:dgram";
import net from "node:net";
import { dirname, join } from "node:path";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { pathToFileURL } from "node:url";

import { fixture, netHost, netWorker, tcpHost } from "./node-test-support.ts";
import { Ok } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// #814: a held resolver lives only in the test worker; the provider and
// transport are unmodified production sources copied beside that wrapper.
async function heldLookupHost(testContext: TestContext): Promise<Dynamic> {
  const root = fixture(testContext, {});
  for (const name of [
    "node.ts",
    "node-files.ts",
    "core.ts",
    "core-package.ts",
    "session.ts",
    "output.ts",
    "output-worker.ts",
    "worker-control.ts",
    "checked-report.ts",
    "float.ts",
    "journal.ts",
    "run-support.ts",
    "wire.ts",
    "package-contract.ts",
    "package-providers.ts",
    "gen/abi.ts",
  ]) {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    copyFileSync(new URL(`./${name}`, import.meta.url), join(root, name));
  }
  const entered = join(root, "entered"),
    release = join(root, "release"),
    finished = join(root, "finished");
  writeFileSync(
    join(root, "net.ts"),
    `
    import {promises as dns} from 'node:dns';
    import {existsSync,writeFileSync} from 'node:fs';
    const lookup=dns.lookup;
    dns.lookup=async(host,options)=>{
      if(host!=='slow.example')return lookup(host,options);
      writeFileSync(${JSON.stringify(entered)},'');
      await new Promise(resolve=>{const timer=setInterval(()=>{if(existsSync(${JSON.stringify(release)})){clearInterval(timer);resolve();}},1);});
      writeFileSync(${JSON.stringify(finished)},'');
      const address={address:'127.0.0.1',family:4};
      return options.all?[address]:address;
    };
    await import(${JSON.stringify(new URL("./net.ts", import.meta.url).href)});
  `,
  );
  const module = await import(pathToFileURL(join(root, "node.ts")).href);
  return { ...netHost(testContext, "hd:Net", {}, module.createHost), entered, release, finished };
}

for (const abort of [false, true]) {
  test(`Net slow lookup lets a sibling clock run (${abort ? "abort" : "finish"})`, async (testContext) => {
    const { host, call, take, entered, release, finished } = await heldLookupHost(testContext);
    const lookup = call("lookup", "slow.example");
    assert.equal(typeof lookup.operation, "number");
    assert.ok(existsSync(entered));
    for (let tick = 0; tick < 3; tick++) {
      const [, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
      assert.ok(host.imports["hd:rt"].block() > 0);
      host.imports["hd:Clock"]["sleep.finish"](clock);
      assert.equal(host.core.st.ops.get(lookup.operation).completed, false);
    }
    if (abort) {
      host.imports["hd:rt"].abort(lookup.operation);
    }
    writeFileSync(release, "");
    if (abort) {
      await testContext.waitFor(() => assert.ok(existsSync(finished)), { timeout: 5000 });
      assert.deepEqual(take("lookup", call("lookup", "127.0.0.1")), Ok(["127.0.0.1"]));
    } else {
      assert.deepEqual(take("lookup", lookup), Ok(["127.0.0.1"]));
    }
  });
}

async function finishWrite(
  testContext: TestContext,
  api: Dynamic,
  started: Dynamic,
): Promise<Dynamic> {
  if (started.result) {
    return started.result;
  }
  await testContext.waitFor(
    () => {
      const [, clock] = api.host.imports["hd:Clock"]["sleep.start"](0n);
      api.host.imports["hd:rt"].block();
      api.host.imports["hd:Clock"]["sleep.finish"](clock);
      assert.equal(api.host.core.st.ops.get(started.operation).completed, true);
    },
    { timeout: 5000, interval: 1 },
  );
  return api.take("TcpStream.write", started);
}

for (const abort of [false, true]) {
  test(
    `Net backpressured write keeps siblings running (${abort ? "abort" : "finish"})`,
    { timeout: 10000 },
    async (testContext) => {
      const api = await tcpHost(testContext);
      const { host, call, handle, peer } = api;
      peer.pause();
      host.core.st.memory.grow(256);
      const bytes = Buffer.alloc(16 * 1024 * 1024);
      for (let i = 0; i < bytes.length; i++) {
        bytes[i] = i % 251;
      }
      const started = call("TcpStream.write", handle, bytes);
      assert.equal(typeof started.operation, "number");
      for (let tick = 0; tick < 3; tick++) {
        const [, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
        host.imports["hd:rt"].block();
        host.imports["hd:Clock"]["sleep.finish"](clock);
        assert.equal(host.core.st.ops.get(started.operation).completed, false);
      }
      const chunks: Buffer[] = [];
      let size = 0;
      peer.on("data", (chunk: Buffer) => {
        chunks.push(chunk);
        size += chunk.length;
      });
      if (abort) {
        host.imports["hd:rt"].abort(started.operation);
      }
      peer.resume();
      if (abort) {
        const tail = Buffer.from("after-abort");
        assert.deepEqual(
          await finishWrite(testContext, api, call("TcpStream.write", handle, tail)),
          Ok(undefined),
        );
        await testContext.waitFor(
          () => assert.ok(Buffer.concat(chunks).subarray(-tail.length).equals(tail)),
          { timeout: 5000 },
        );
        assert.ok(
          size < bytes.length + tail.length,
          "abort stops submitting the rest and keeps the stream usable",
        );
      } else {
        assert.deepEqual(await finishWrite(testContext, api, started), Ok(undefined));
        await testContext.waitFor(() => assert.equal(size, bytes.length), { timeout: 5000 });
        assert.ok(Buffer.concat(chunks).equals(bytes), "all bytes arrive in order");
      }
    },
  );
}

for (const kind of ["listen", "bind_udp"]) {
  test(`Net ${kind} abort releases a ready undelivered handle`, async (testContext) => {
    const call = netWorker(testContext);
    const operation = call({
      op: "wait_start",
      kind,
      host: "127.0.0.1",
      port: 0,
      address: "127.0.0.1:0",
    }).ok;
    let result: Dynamic;
    await testContext.waitFor(
      () => {
        result = call({ op: "wait_poll", operation });
        assert.equal(typeof result.ok, "number");
      },
      { timeout: 5000 },
    );
    assert.deepEqual(
      call({ op: "wait_poll", operation }),
      result,
      "poll does not consume the handle",
    );
    call({ op: "wait_abort", operation });
    assert.deepEqual(call({ op: "check", handle: result.ok }), { unknown: true });
    assert.deepEqual(call({ op: "open" }), { ok: { stream: 0, listener: 0, udp: 0 } });
  });
}

for (const kind of ["listen", "bind_udp"]) {
  test(`Net ${kind} abort before its callback leaves no late handle`, async (testContext) => {
    const root = fixture(testContext, {}),
      ready = join(root, "ready"),
      release = join(root, "release"),
      finished = join(root, "finished");
    const url = join(root, "held.ts");
    writeFileSync(
      url,
      `
      import net from 'node:net';import dgram from 'node:dgram';
      import {existsSync,writeFileSync} from 'node:fs';
      const proto=${kind === "listen" ? "net.Server.prototype" : "dgram.Socket.prototype"};
      const method=${JSON.stringify(kind === "listen" ? "listen" : "bind")};
      const original=proto[method];
      proto[method]=function(...args){
        const callback=args.pop();
        return original.call(this,...args,()=>{
          writeFileSync(${JSON.stringify(ready)},String(this.address().port));
          const timer=setInterval(()=>{if(existsSync(${JSON.stringify(release)})){
            clearInterval(timer);callback();writeFileSync(${JSON.stringify(finished)},'');}},1);
        });
      };
      await import(${JSON.stringify(new URL("./net.ts", import.meta.url).href)});
    `,
    );
    const call = netWorker(testContext, { url: pathToFileURL(url) });
    const operation = call({
      op: "wait_start",
      kind,
      host: "127.0.0.1",
      port: 0,
      address: "127.0.0.1:0",
    }).ok;
    await testContext.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
    assert.deepEqual(call({ op: "wait_poll", operation }), { pending: true });
    call({ op: "wait_abort", operation });
    const port = Number(readFileSync(ready, "utf8"));
    if (kind === "listen") {
      const replacement = net.createServer();
      await new Promise<void>((resolve, reject) => {
        replacement.once("error", reject);
        replacement.listen(port, "127.0.0.1", () => resolve());
      });
      await new Promise<void>((resolve) => replacement.close(() => resolve()));
    } else {
      const replacement = dgram.createSocket("udp4");
      await new Promise<void>((resolve, reject) => {
        replacement.once("error", reject);
        replacement.bind(port, "127.0.0.1", () => resolve());
      });
      await new Promise<void>((resolve) => replacement.close(() => resolve()));
    }
    writeFileSync(release, "");
    await testContext.waitFor(() => assert.ok(existsSync(finished)), { timeout: 5000 });
    assert.deepEqual(call({ op: "open" }), { ok: { stream: 0, listener: 0, udp: 0 } });
  });
}

test("Net UDP abort while DNS is held sends no late datagram", async (testContext) => {
  const api = await heldLookupHost(testContext);
  const { host, call, take, entered, release, finished } = api;
  const bound = take("bind_udp", call("bind_udp", "127.0.0.1", 0));
  assert.ok(bound.ok);
  const target = dgram.createSocket("udp4");
  await new Promise<void>((resolve) => target.bind(0, "127.0.0.1", () => resolve()));
  testContext.after(() => target.close());
  let arrivals = 0;
  target.on("message", () => {
    arrivals++;
  });
  const send = call(
    "UdpSocket.send_to",
    bound.value,
    "slow.example",
    target.address().port,
    Buffer.from("cancelled"),
  );
  assert.equal(typeof send.operation, "number");
  assert.ok(existsSync(entered));
  host.imports["hd:rt"].abort(send.operation);
  writeFileSync(release, "");
  await testContext.waitFor(() => assert.ok(existsSync(finished)), { timeout: 5000 });
  // A successor datagram proves the original socket remains usable and
  // all already queued receive events have an observation barrier.
  const received = once(target, "message");
  const next = call(
    "UdpSocket.send_to",
    bound.value,
    "127.0.0.1",
    target.address().port,
    Buffer.from("next"),
  );
  assert.deepEqual(take("UdpSocket.send_to", next), Ok(undefined));
  const [bytes] = await received;
  assert.equal(bytes.toString(), "next");
  assert.equal(arrivals, 1);
});
