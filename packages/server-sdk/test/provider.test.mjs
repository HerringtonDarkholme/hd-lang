import assert from "node:assert/strict";
import { test } from "node:test";
import provider from "../providers/node.mjs";

test(
  "a released Node lease clears timers and rejects retained clock handles",
  { timeout: 10000 },
  async () => {
    const instance = new AbortController();
    const context = {
      instanceSignal: instance.signal,
      signal: new AbortController().signal,
      pending: (poll, done, abort, finish) => ({ poll, done, abort, finish }),
    };
    let listening;
    const ready = new Promise((resolve) => {
      listening = resolve;
    });
    const log = console.log;
    const port = process.env.HD_SERVER_PORT;
    process.env.HD_SERVER_PORT = "0";
    console.log = (text) => {
      if (text.startsWith('{"event":"listening"')) {
        listening(JSON.parse(text));
      }
    };
    let response;
    try {
      const incoming = provider.accept(context);
      const address = await ready;
      response = fetch(`http://127.0.0.1:${address.port}`, { signal: AbortSignal.timeout(5000) });
      const { value } = await incoming;
      const clock = provider.clock(value.id, context);
      assert.equal(typeof clock.now(), "string");
      assert.equal(typeof clock.monotonic(), "bigint");
      const long = clock.sleep(2147483648n, context);
      await new Promise((resolve) => setTimeout(resolve, 10));
      assert.equal(long.poll(), false, "a long duration must not overflow Node's timer limit");
      provider.finish(value.id, false, context);
      provider.finish(value.id, false, context);
      for (const use of [
        () => clock.now(),
        () => clock.monotonic(),
        () => clock.sleep(0n, context),
      ]) {
        assert.throws(use, /expired request lease/);
      }
      assert.equal(long.poll(), false, "finishing the lease cancels its still-pending timer");
      const ended = await response;
      assert.equal(ended.status, 503);
      await ended.text();
    } finally {
      instance.abort();
      await response?.catch(() => {});
      console.log = log;
      if (port === undefined) {
        delete process.env.HD_SERVER_PORT;
      } else {
        process.env.HD_SERVER_PORT = port;
      }
    }
  },
);
