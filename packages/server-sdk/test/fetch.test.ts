import assert from "node:assert/strict";
import { test } from "node:test";
import { AsyncLocalStorage } from "node:async_hooks";
import { createFetchHandler } from "../adapters/fetch.ts";

const tick = async () => {
  for (let count = 0; count < 12; count++) {
    await Promise.resolve();
  }
};
const reply = (patch = {}) => ({
  response: {
    status: 200,
    headers: [],
    body: new Uint8Array([0, 255, 128]),
    url: "https://reply.test/",
    ...patch,
  },
  cancelled: null,
  abandoned: false,
});
function fake(handle = () => reply()) {
  const calls = [],
    failures = [];
  let stopped = 0;
  return {
    calls,
    failures,
    get stopped() {
      return stopped;
    },
    dispatch(request, signal) {
      calls.push({ request, signal });
      return handle(request, signal);
    },
    stop() {
      stopped++;
    },
    fail(error) {
      failures.push(error);
    },
  };
}
function clock() {
  let time = 0,
    serial = 0;
  const pending = new Map();
  return {
    timers: {
      set(fn, delay) {
        const id = ++serial;
        pending.set(id, { fn, at: time + delay });
        return id;
      },
      clear(id) {
        pending.delete(id);
      },
    },
    async advance(amount) {
      const end = time + amount;
      for (;;) {
        const next = [...pending]
          .filter(([, value]) => value.at <= end)
          .sort((left, right) => left[1].at - right[1].at)[0];
        if (!next) {
          break;
        }
        time = next[1].at;
        pending.delete(next[0]);
        next[1].fn();
        await tick();
      }
      time = end;
      await tick();
    },
    active: () => pending.size,
  };
}
function acknowledgeAbortAfter(signal, time, delay) {
  return new Promise((resolve) => {
    signal.addEventListener(
      "abort",
      () => {
        time.timers.set(
          () => resolve({ response: null, cancelled: signal.reason, abandoned: true }),
          delay,
        );
      },
      { once: true },
    );
  });
}
const request = (body, headers = {}, method = "POST") =>
  new Request("https://in.test/p?q=a%2Cb&q=two", {
    method,
    headers,
    body,
    ...(body instanceof ReadableStream ? { duplex: "half" } : {}),
  });

test("Fetch mapping preserves absolute URL, raw bytes, combined headers and method tokens", async () => {
  const app = fake();
  const adapter = createFetchHandler(app);
  const response = await adapter.fetch(
    request(
      new Uint8Array([0, 255, 128]),
      [
        ["x-repeat", "one, two"],
        ["connection", "x-remove"],
        ["x-remove", "private"],
        ["content-length", "3"],
        ["transfer-encoding", "chunked"],
      ],
      "CUSTOM",
    ),
  );
  const value = app.calls[0].request;
  assert.equal(value.url, "https://in.test/p?q=a%2Cb&q=two");
  assert.equal(Object.hasOwn(value, "query"), false);
  assert.deepEqual(value.method, { tag: "Other", fields: { name: "CUSTOM" } });
  assert.deepEqual([...value.body], [0, 255, 128]);
  assert.deepEqual(value.headers, [["x-repeat", "one, two"]]);
  assert.equal(response.status, 200);
  assert.deepEqual([...new Uint8Array(await response.arrayBuffer())], [0, 255, 128]);
  assert.equal(adapter.active(), 0);
});

test("Fetch response strips all host framing and Connection-listed fields while keeping cookies separate", async () => {
  const app = fake(() =>
    reply({
      status: 503,
      headers: [
        ["connection", "x-hidden"],
        ["x-hidden", "secret"],
        ["content-length", "999"],
        ["transfer-encoding", "chunked"],
        ["keep-alive", "timeout=5"],
        ["te", "trailers"],
        ["trailer", "x-check"],
        ["upgrade", "websocket"],
        ["proxy-authenticate", "Basic"],
        ["proxy-authorization", "Basic xxx"],
        ["x-repeat", "one, two"],
        ["set-cookie", "a=1; Expires=Wed, 21 Oct 2030 07:28:00 GMT"],
        ["set-cookie", "b=2"],
      ],
    }),
  );
  const result = await createFetchHandler(app).fetch(request(""));
  assert.equal(result.status, 503);
  for (const name of [
    "connection",
    "x-hidden",
    "content-length",
    "transfer-encoding",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
    "proxy-authenticate",
    "proxy-authorization",
  ]) {
    assert.equal(result.headers.has(name), false, name);
  }
  assert.equal(result.headers.get("x-repeat"), "one, two");
  assert.deepEqual(result.headers.getSetCookie(), [
    "a=1; Expires=Wed, 21 Oct 2030 07:28:00 GMT",
    "b=2",
  ]);
});

test("HEAD and bodyless statuses suppress bytes without splitting other fields", async () => {
  for (const [method, status] of [
    ["HEAD", 200],
    ["GET", 204],
    ["GET", 205],
    ["GET", 304],
  ]) {
    const app = fake(() => reply({ status, headers: [["x-value", "a,b"]] }));
    const response = await createFetchHandler(app).fetch(request(undefined, {}, method));
    assert.equal(response.status, status);
    assert.equal(response.body, null);
    assert.equal(response.headers.get("x-value"), "a,b");
  }
});

test("declared and streamed bodies are bounded before guest dispatch and rejected streams are cancelled", async () => {
  for (const declared of [false, true]) {
    let cancelled = 0,
      reads = 0;
    const body = new ReadableStream(
      {
        pull(controller) {
          reads++;
          controller.enqueue(new Uint8Array(1024 * 1024 + 1));
        },
        cancel() {
          cancelled++;
        },
      },
      { highWaterMark: 0 },
    );
    const app = fake();
    const response = await createFetchHandler(app).fetch(
      request(body, declared ? { "content-length": "1048577" } : {}),
    );
    assert.equal(response.status, 413);
    assert.equal(app.calls.length, 0);
    assert.equal(cancelled, 1);
    assert.equal(reads, declared ? 0 : 1, "the declared size is checked before any read");
  }
  const app = fake();
  const accepted = await createFetchHandler(app).fetch(
    request(new Uint8Array(1024 * 1024), { "content-length": "1" }),
  );
  assert.equal(accepted.status, 200);
  assert.equal(app.calls[0].request.body.length, 1024 * 1024, "actual bytes are still counted");
});

test("64KiB input headers reject before reading; malformed declared lengths return 400", async () => {
  let pulls = 0,
    cancelled = 0;
  const body = new ReadableStream(
    {
      pull() {
        pulls++;
      },
      cancel() {
        cancelled++;
      },
    },
    { highWaterMark: 0 },
  );
  const app = fake();
  const response = await createFetchHandler(app).fetch(
    request(body, { "x-large": "a".repeat(65536) }),
  );
  assert.equal(response.status, 431);
  assert.equal(pulls, 0);
  assert.equal(cancelled, 1);
  assert.equal(app.calls.length, 0);
  for (const length of ["-1", "1, 2", "nonsense"]) {
    assert.equal(
      (await createFetchHandler(fake()).fetch(request("x", { "content-length": length }))).status,
      400,
    );
  }
});

test("oversized/invalid responses become generic private-log 500 without committing header or body data", async () => {
  const bad = [
    { body: new Uint8Array(1024 * 1024 + 1) },
    { headers: [["x-large", "a".repeat(65536)]] },
    { status: 199 },
    { status: 600 },
    { headers: [["x-bad", "private\r\nvalue"]] },
    { body: [999] },
  ];
  for (const value of bad) {
    const logs = [];
    const app = fake(() => reply(value));
    const result = await createFetchHandler(app, {
      sink: { err: (text) => logs.push(text) },
    }).fetch(request(""));
    assert.equal(result.status, 500);
    assert.equal(await result.text(), "internal server error");
    assert.equal(result.headers.has("x-large"), false);
    assert.deepEqual(
      logs.map((line) => JSON.parse(line).event),
      ["http.response-invalid"],
    );
    assert.equal(
      app.failures.length,
      0,
      "a bounded response rejection need not poison shared state",
    );
  }
});

test("eight admission reservations include body reads; ninth gets immediate 503 with no queue", async () => {
  const streams = [],
    app = fake();
  const adapter = createFetchHandler(app);
  const pending = Array.from({ length: 8 }, () =>
    adapter.fetch(
      request(
        new ReadableStream(
          {
            start(controller) {
              streams.push(controller);
            },
          },
          { highWaterMark: 0 },
        ),
      ),
    ),
  );
  assert.equal(adapter.active(), 8);
  assert.equal(app.calls.length, 0);
  const overload = await adapter.fetch(request("ninth"));
  assert.equal(overload.status, 503);
  assert.equal(overload.headers.get("retry-after"), "1");
  assert.equal(adapter.active(), 8);
  assert.equal(app.calls.length, 0);
  streams.forEach((controller) => controller.close());
  assert.ok((await Promise.all(pending)).every((response) => response.status === 200));
  assert.equal(adapter.active(), 0);
});

test("the fixed 20s deadline starts at admission and includes a stalled body read", async () => {
  const time = clock();
  let cancelled = 0;
  const app = fake();
  const adapter = createFetchHandler(app, { timers: time.timers });
  const result = adapter.fetch(
    request(
      new ReadableStream(
        {
          cancel() {
            cancelled++;
          },
        },
        { highWaterMark: 0 },
      ),
    ),
  );
  await tick();
  await time.advance(19999);
  assert.equal(adapter.active(), 1);
  assert.equal(app.calls.length, 0);
  await time.advance(1);
  const response = await result;
  assert.equal(response.status, 504);
  assert.equal(cancelled, 1);
  assert.equal(adapter.active(), 0);
  assert.equal(time.active(), 0);
  assert.equal(app.failures.length, 0);
});

test("deadline and disconnect wait for cooperative lease release without changing sibling authority", async () => {
  const time = clock(),
    releases = [];
  const app = fake(
    (_request, signal) =>
      new Promise((resolve) => {
        releases.push(() => resolve(reply()));
        signal.addEventListener(
          "abort",
          () => resolve({ response: null, cancelled: signal.reason }),
          { once: true },
        );
      }),
  );
  const adapter = createFetchHandler(app, { timers: time.timers });
  const controller = new AbortController();
  const disconnected = adapter.fetch(
    new Request("https://in.test/", { signal: controller.signal }),
  );
  const deadline = adapter.fetch(new Request("https://in.test/"));
  await tick();
  controller.abort();
  await assert.rejects(disconnected, { name: "AbortError" });
  assert.equal(app.calls[0].signal.reason, "Disconnected");
  assert.equal(app.calls[1].signal.aborted, false);
  await time.advance(20000);
  assert.equal((await deadline).status, 504);
  assert.equal(app.calls[1].signal.reason, "Deadline");
  assert.equal(app.failures.length, 0);
  assert.equal(time.active(), 0);
});

test("noncooperative cancellation escalates at exactly 200ms and stale completion cannot reopen admission", async () => {
  const time = clock();
  let late;
  const app = fake(
    () =>
      new Promise((resolve) => {
        late = resolve;
      }),
  );
  const adapter = createFetchHandler(app, { timers: time.timers });
  const result = adapter.fetch(new Request("https://in.test/"));
  await tick();
  await time.advance(20000);
  await time.advance(199);
  assert.equal(adapter.active(), 1);
  assert.equal(app.failures.length, 0);
  await time.advance(1);
  assert.equal((await result).status, 504);
  assert.equal(app.failures.length, 1);
  late(reply());
  await tick();
  assert.equal(adapter.active(), 0);
  assert.equal((await adapter.fetch(request("late"))).status, 503);
  assert.equal(time.active(), 0);
});

test("first stop drains admitted body reads; second stop cancels admitted work", async () => {
  let stream;
  const app = fake();
  const adapter = createFetchHandler(app);
  const result = adapter.fetch(
    request(
      new ReadableStream({
        start(controller) {
          stream = controller;
        },
      }),
    ),
  );
  adapter.stop();
  assert.equal(app.stopped, 0, "body reads were admitted before draining began");
  assert.equal((await adapter.fetch(request("new"))).status, 503);
  stream.close();
  assert.equal((await result).status, 200);
  assert.equal(app.stopped, 1);

  const held = fake(
    (_request, signal) =>
      new Promise((resolve) => {
        signal.addEventListener(
          "abort",
          () => resolve({ response: null, cancelled: signal.reason }),
          { once: true },
        );
      }),
  );
  const draining = createFetchHandler(held);
  const pending = draining.fetch(new Request("https://in.test/"));
  await tick();
  draining.stop();
  assert.equal(held.calls[0].signal.aborted, false);
  draining.stop();
  assert.equal((await pending).status, 503);
  assert.equal(held.calls[0].signal.reason, "Stopping");
});

test("guest failure produces only generic 500, cancels other readers and closes admission", async () => {
  let release;
  const app = fake(
    () =>
      new Promise((_resolve, reject) => {
        release = reject;
      }),
  );
  const adapter = createFetchHandler(app);
  const active = adapter.fetch(request(""));
  let cancelled = 0;
  const reader = adapter.fetch(
    request(
      new ReadableStream(
        {
          cancel() {
            cancelled++;
          },
        },
        { highWaterMark: 0 },
      ),
    ),
  );
  await tick();
  release(new Error("private details"));
  assert.equal(await (await active).text(), "internal server error");
  assert.equal((await reader).status, 500);
  assert.equal(cancelled, 1);
  assert.equal(app.failures.length, 1);
  assert.equal((await adapter.fetch(request("late"))).status, 503);
});

test("cross-request failure cancels a native reader only in the reader's owning coroutine", async () => {
  const owner = new AsyncLocalStorage();
  let cancelledIn;
  const app = fake(() => {
    throw new Error("private trap");
  });
  const adapter = createFetchHandler(app);
  const reading = owner.run("reader", () =>
    adapter.fetch(
      request(
        new ReadableStream(
          {
            cancel() {
              cancelledIn = owner.getStore();
            },
          },
          { highWaterMark: 0 },
        ),
      ),
    ),
  );
  await tick();
  const failed = owner.run("failed request", () => adapter.fetch(request("")));
  assert.equal((await failed).status, 500);
  assert.equal((await reading).status, 500);
  assert.equal(cancelledIn, "reader");
});

test("an incoming stream which ignores cancellation is abandoned within the 200ms host backstop", async () => {
  const time = clock(),
    app = fake();
  const adapter = createFetchHandler(app, { timers: time.timers });
  const result = adapter.fetch(
    request(
      new ReadableStream(
        {
          cancel() {
            return new Promise(() => {});
          },
        },
        { highWaterMark: 0 },
      ),
    ),
  );
  await tick();
  await time.advance(20000);
  await time.advance(199);
  assert.equal(adapter.active(), 1);
  assert.equal(app.failures.length, 0);
  await time.advance(1);
  assert.equal((await result).status, 504);
  assert.equal(adapter.active(), 0);
  assert.equal(app.failures.length, 1);
});

// A cooperative guest ACK is allowed after the SDK work budget has elapsed.
test("a guest acknowledgement at105ms keeps admission healthy until the200ms backstop", async () => {
  const time = clock();
  let calls = 0;
  const app = fake((_request, signal) => {
    calls++;
    if (calls > 1) {
      return reply();
    }
    return acknowledgeAbortAfter(signal, time, 105);
  });
  const adapter = createFetchHandler(app, { timers: time.timers });
  const first = adapter.fetch(new Request("https://in.test/"));
  await tick();
  await time.advance(20000);
  await time.advance(104);
  assert.equal(adapter.active(), 1);
  assert.equal(app.failures.length, 0);
  await time.advance(1);
  assert.equal((await first).status, 504);
  assert.equal(app.failures.length, 0);
  assert.equal(adapter.active(), 0);
  assert.equal((await adapter.fetch(new Request("https://in.test/next"))).status, 200);
  assert.equal(time.active(), 0);
});
