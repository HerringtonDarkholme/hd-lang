import assert from "node:assert/strict";
import http from "node:http";
import { AsyncLocalStorage } from "node:async_hooks";
import { test } from "node:test";
import provider from "../providers/node.ts";
import { createGrant, isAsciiUrl, writtenHost } from "../../../host/core.ts";

const nativeFetch = globalThis.fetch;
const tick = () => new Promise((resolve) => setImmediate(resolve));
const request = (url) => ({
  method: { tag: "Get", fields: {} },
  url,
  headers: [],
  body: new Uint8Array(),
  redirects: 10,
});
const reply = (text = "ok") => ({
  status: 200,
  url: "",
  headers: [],
  body: new TextEncoder().encode(text),
});
const pending = (poll, done, abort = () => {}, finish = () => {}) => ({
  poll,
  done,
  abort,
  finish,
});
async function ready(operation) {
  const until = performance.now() + 3000;
  while (!operation.poll()) {
    if (performance.now() > until) {
      throw new Error("operation did not become ready");
    }
    await tick();
  }
  const value = operation.done();
  operation.finish();
  return value;
}
async function fixture(Http = true, Console = true) {
  const instance = new AbortController();
  const grant = createGrant({ Http, Console });
  const context = {
    instanceSignal: instance.signal,
    signal: new AbortController().signal,
    pending,
    console: Object.freeze({ allows: () => grant.allows("Console") }),
    http: Object.freeze({
      coversHost: (host, port) => grant.coversHost("Http", host, port),
      isAsciiUrl,
      writtenHost,
    }),
  };
  let address, acceptAddress;
  const listening = new Promise((resolve) => {
    acceptAddress = resolve;
  });
  const log = console.log,
    port = process.env.HD_SERVER_PORT,
    downstreams = [],
    events = [];
  process.env.HD_SERVER_PORT = "0";
  console.log = (text) => {
    if (text.startsWith("{")) {
      events.push(JSON.parse(text));
    }
    if (text.startsWith('{"event":"listening"')) {
      address = JSON.parse(text);
      acceptAddress();
    }
  };
  return {
    context,
    instance,
    events,
    async accept(owner = "A", signal = AbortSignal.timeout(5000)) {
      const incoming = provider.accept(context);
      if (!address) {
        await listening;
      }
      const response = nativeFetch(`http://127.0.0.1:${address.port}`, {
        headers: { "x-owner": owner },
        signal,
      });
      // Attach rejection handling immediately, including deliberately disconnected controls.
      response.catch(() => {});
      downstreams.push(response);
      const value = await incoming;
      assert.equal(value.ok, true);
      return { id: value.value.id, response };
    },
    async close() {
      instance.abort();
      await Promise.allSettled(downstreams.map(async (p) => (await p).arrayBuffer()));
      console.log = log;
      if (port === undefined) {
        delete process.env.HD_SERVER_PORT;
      } else {
        process.env.HD_SERVER_PORT = port;
      }
    },
  };
}

test(
  "Node Console logs keep source IDs, respect denial and expire with the lease",
  {
    timeout: 10000,
  },
  async () => {
    const records = [];
    const streams = [process.stdout, process.stderr],
      writes = streams.map((stream) => stream.write);
    for (const [index, stream] of streams.entries()) {
      stream.write = function (text, ...args) {
        if (typeof text === "string" && text.startsWith('{"version":1,')) {
          records.push({ channel: index, record: JSON.parse(text) });
          return true;
        }
        return writes[index].call(this, text, ...args);
      };
    }
    try {
      for (const allowed of [true, false]) {
        const f = await fixture(true, allowed);
        try {
          const { id, response } = await f.accept();
          const console = provider.console(id, f.context);
          assert.throws(
            () => provider.console(id, { ...f.context, console: undefined }),
            /Console policy service/,
          );
          const start = records.length;
          const out = console.write_sync("native stdout\n"),
            err = console.write_error_sync("native stderr\n");
          assert.equal(out.ok, allowed);
          assert.equal(err.ok, allowed);
          if (!allowed) {
            assert.equal(out.error.fields.stream.tag, "Stdout");
            assert.equal(err.error.fields.stream.tag, "Stderr");
          }
          provider.finish(id, false, f.context);
          assert.equal((await response).status, 503);
          await tick();
          const own = records.slice(start);
          assert.equal(own.length, allowed ? 2 : 0);
          if (allowed) {
            assert.deepEqual(
              own.map(({ channel }) => channel),
              [0, 1],
            );
            assert.ok(own.every(({ record }) => record.lease.generation === String(id.generation)));
            assert.ok(own.every(({ record }) => record.lease.sequence === String(id.sequence)));
          }
          assert.throws(() => console.write_sync("late"), /expired request lease/);
          assert.throws(() => provider.console(id, f.context), /expired request lease/);
        } finally {
          await f.close();
        }
      }
    } finally {
      streams.forEach((stream, index) => {
        stream.write = writes[index];
      });
    }
  },
);

test(
  "Node clients use authoritative false/scoped policy before any fetch",
  { timeout: 10000 },
  async () => {
    const original = globalThis.fetch;
    let effects = 0;
    globalThis.fetch = async () => {
      effects++;
      throw new Error("must not fetch");
    };
    try {
      for (const policy of [false, ["allowed.test"]]) {
        const f = await fixture(policy);
        try {
          const { id, response } = await f.accept();
          const client = provider.client(id, f.context);
          const result = await ready(client.send(request("https://denied.test/path"), f.context));
          assert.equal(result.ok, false);
          assert.equal(result.error.tag, "NotGranted");
          assert.equal(effects, 0);
          assert.throws(
            () => provider.client(id, { ...f.context, http: undefined }),
            /compiler HTTP policy service/,
          );
          provider.finish(id, false, f.context);
          assert.equal((await response).status, 503);
        } finally {
          await f.close();
        }
      }
    } finally {
      globalThis.fetch = original;
    }
  },
);

test(
  "A's native starts and disposal stay in A when B polls its next call",
  { timeout: 10000 },
  async () => {
    const owners = new AsyncLocalStorage(),
      events = [],
      resolvers = [];
    const createServer = http.createServer,
      original = globalThis.fetch;
    http.createServer = (options, callback) =>
      createServer(options, (req, res) =>
        owners.run(req.headers["x-owner"], () => callback(req, res)),
      );
    globalThis.fetch = (_url, options) => {
      events.push(["start", owners.getStore()]);
      options.signal.addEventListener("abort", () => events.push(["dispose", owners.getStore()]), {
        once: true,
      });
      return new Promise((resolve) => resolvers.push(resolve));
    };
    const f = await fixture(true);
    try {
      const a = await f.accept("A"),
        b = await f.accept("B");
      const client = provider.client(a.id, f.context);
      const first = owners.run("B", () =>
        client.send(request("https://upstream.test/first"), f.context),
      );
      assert.equal(resolvers.length, 0, "the foreign poll only registers the operation");
      await tick();
      assert.equal(resolvers.length, 1);
      owners.run("B", () => resolvers[0](new Response("one")));
      assert.equal(new TextDecoder().decode((await ready(first)).value.body), "one");
      const second = owners.run("B", () =>
        client.send(request("https://upstream.test/second"), f.context),
      );
      const acknowledged = provider.respond(b.id, reply("B done"), f.context);
      assert.equal((await ready(acknowledged)).ok, true);
      provider.finish(b.id, false, f.context);
      assert.equal(await (await b.response).text(), "B done");
      await tick();
      assert.equal(resolvers.length, 2);
      owners.run("B", () => resolvers[1](new Response("two")));
      assert.equal(new TextDecoder().decode((await ready(second)).value.body), "two");
      provider.finish(a.id, false, f.context);
      await a.response;
      await tick();
      assert.deepEqual(events, [
        ["start", "A"],
        ["dispose", "A"],
        ["start", "A"],
        ["dispose", "A"],
      ]);
    } finally {
      await f.close();
      http.createServer = createServer;
      globalThis.fetch = original;
    }
  },
);

test(
  "abort before native start has no effect and released clients cannot be retained",
  { timeout: 10000 },
  async () => {
    let effects = 0;
    const original = globalThis.fetch;
    globalThis.fetch = async () => {
      effects++;
      return new Response("unexpected");
    };
    const f = await fixture(true);
    try {
      const { id, response } = await f.accept();
      const client = provider.client(id, f.context),
        clock = provider.clock(id, f.context);
      const operation = client.send(request("https://upstream.test/"), f.context);
      operation.abort();
      const sleeping = clock.sleep(2147483648n, f.context);
      provider.finish(id, false, f.context);
      provider.finish(id, false, f.context);
      await tick();
      assert.equal(effects, 0);
      assert.equal(sleeping.poll(), false);
      assert.throws(
        () => client.send(request("https://upstream.test/"), f.context),
        /expired request lease/,
      );
      assert.throws(() => clock.now(), /expired request lease/);
      assert.throws(() => clock.sleep(0n, f.context), /expired request lease/);
      assert.throws(
        () => provider.client({ ...id, generation: id.generation + 1n }, f.context),
        /expired request lease/,
      );
      assert.equal((await response).status, 503);
    } finally {
      await f.close();
      globalThis.fetch = original;
    }
  },
);

test(
  "response delivery is acknowledged before finish and duplicate responses cause no second write",
  { timeout: 10000 },
  async () => {
    const f = await fixture();
    try {
      const { id, response } = await f.accept();
      const operation = provider.respond(id, reply("delivered"), f.context);
      assert.equal(provider.respond(id, reply("duplicate"), f.context).ok, false);
      assert.equal((await ready(operation)).ok, true);
      assert.equal(await (await response).text(), "delivered");
      provider.finish(id, false, f.context);
      provider.finish(id, false, f.context);
      assert.equal(provider.respond(id, reply("late"), f.context).error.tag, "Expired");
    } finally {
      await f.close();
    }
  },
);

test(
  "instance close aborts native HTTP and ignores late completion",
  { timeout: 10000 },
  async () => {
    let resolveUpstream,
      aborted = 0;
    const original = globalThis.fetch;
    globalThis.fetch = (_url, options) =>
      new Promise((resolve) => {
        resolveUpstream = resolve;
        options.signal.addEventListener("abort", () => aborted++, { once: true });
      });
    const f = await fixture();
    try {
      const a = await f.accept("A"),
        b = await f.accept("B");
      const client = provider.client(a.id, f.context);
      const operation = client.send(request("https://upstream.test/"), f.context);
      await tick();
      assert.equal(typeof resolveUpstream, "function");
      f.instance.abort();
      await tick();
      assert.equal(aborted, 1);
      resolveUpstream(new Response("late"));
      await tick();
      assert.equal(operation.poll(), false);
      assert.throws(
        () => client.send(request("https://upstream.test/"), f.context),
        /expired request lease/,
      );
      await Promise.allSettled([a.response, b.response]);
    } finally {
      await f.close();
      globalThis.fetch = original;
    }
  },
);

test(
  "disconnect cancels one lease and the sibling keeps fetching and replying",
  { timeout: 10000 },
  async () => {
    const original = globalThis.fetch;
    let resolveA,
      abortedA = 0;
    globalThis.fetch = (url, options) => {
      if (url.endsWith("/A")) {
        return new Promise((resolve) => {
          resolveA = resolve;
          options.signal.addEventListener("abort", () => abortedA++, { once: true });
        });
      }
      return Promise.resolve(new Response("B upstream"));
    };
    const f = await fixture();
    try {
      const disconnected = new AbortController();
      const a = await f.accept("A", disconnected.signal),
        b = await f.accept("B");
      const ca = provider.client(a.id, f.context),
        cb = provider.client(b.id, f.context);
      const activeA = ca.send(request("https://upstream.test/A"), f.context);
      const cancelledA = provider.cancelled(a.id, f.context),
        cancelledB = provider.cancelled(b.id, f.context);
      await tick();
      disconnected.abort();
      assert.equal((await ready(cancelledA)).tag, "Disconnected");
      assert.equal(cancelledB.poll(), false);
      provider.finish(a.id, false, f.context);
      await tick();
      assert.equal(abortedA, 1);
      const resultB = await ready(cb.send(request("https://upstream.test/B"), f.context));
      assert.equal(new TextDecoder().decode(resultB.value.body), "B upstream");
      assert.equal(
        (await ready(provider.respond(b.id, reply("B stayed alive"), f.context))).ok,
        true,
      );
      provider.finish(b.id, false, f.context);
      assert.equal(await (await b.response).text(), "B stayed alive");
      resolveA(new Response("late A"));
      await tick();
      assert.equal(activeA.poll(), false);
    } finally {
      await f.close();
      globalThis.fetch = original;
    }
  },
);

test(
  "soft stop closes admission while a retained handler fetches and drains normally",
  { timeout: 10000 },
  async () => {
    const original = globalThis.fetch;
    globalThis.fetch = async () => new Response("drained upstream");
    const f = await fixture();
    try {
      const a = await f.accept();
      const cancelled = provider.cancelled(a.id, f.context);
      process.emit("SIGTERM");
      assert.equal(provider.accept(f.context).value, null);
      assert.equal(cancelled.poll(), false, "soft stop must not cancel active handlers");
      const result = await ready(
        provider.client(a.id, f.context).send(request("https://upstream.test/drain"), f.context),
      );
      assert.equal(new TextDecoder().decode(result.value.body), "drained upstream");
      assert.equal((await ready(provider.respond(a.id, reply("drained"), f.context))).ok, true);
      provider.finish(a.id, false, f.context);
      assert.equal(await (await a.response).text(), "drained");
      assert.equal(f.events.filter((event) => event.event === "stopped").length, 1);
    } finally {
      await f.close();
      globalThis.fetch = original;
    }
  },
);

test(
  "a cancelled claimed accept cannot deliver an orphan after body reading",
  { timeout: 10000 },
  async () => {
    const createServer = http.createServer;
    let entered;
    const claimed = new Promise((resolve) => {
      entered = resolve;
    });
    http.createServer = (options, callback) =>
      createServer(options, (req, res) => {
        const result = callback(req, res);
        entered();
        return result;
      });
    const f = await fixture();
    let native;
    try {
      const abort = new AbortController();
      const context = { ...f.context, signal: abort.signal };
      const incoming = provider.accept(context);
      while (!f.events.some((event) => event.event === "listening")) {
        await tick();
      }
      const address = f.events.find((event) => event.event === "listening");
      const response = new Promise((resolve, reject) => {
        native = http.request(`http://127.0.0.1:${address.port}`, { method: "POST" }, (res) => {
          res.resume();
          res.once("end", () => resolve(res.statusCode));
        });
        native.once("error", reject);
        native.write("first chunk");
      });
      await claimed;
      abort.abort();
      assert.deepEqual(await incoming, { ok: true, value: null });
      native.end("last chunk");
      let watchdog;
      try {
        const status = await Promise.race([
          response,
          new Promise((_, reject) => {
            watchdog = setTimeout(() => reject(new Error("orphan response never finished")), 250);
          }),
        ]);
        assert.equal(status, 503);
      } finally {
        clearTimeout(watchdog);
      }
      // The orphan must not consume the next worker's request or poison its lease.
      const next = await f.accept("next");
      assert.equal(
        (await ready(provider.respond(next.id, reply("next request"), f.context))).ok,
        true,
      );
      provider.finish(next.id, false, f.context);
      assert.equal(await (await next.response).text(), "next request");
    } finally {
      native?.destroy();
      await f.close();
      http.createServer = createServer;
    }
  },
);
