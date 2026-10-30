import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "node:http";
import { once } from "node:events";
import { AsyncLocalStorage } from "node:async_hooks";
import { createGrant, isAsciiUrl, writtenHost } from "../../../host/core.mjs";
import { createHttpClient, responseHeaders } from "../adapters/http.mjs";
const request = (url, options = {}) => ({
  url,
  method: { tag: "Get", fields: {} },
  headers: [],
  body: new Uint8Array(),
  redirects: 10,
  ...options,
});
// The transport must hand native work to this lease seam before starting
// it. Dispatcher ownership itself has separate adversarial controls.
function operation(
  input,
  { fetch = globalThis.fetch, Http = true, timeoutMs = 30000, owner = null } = {},
) {
  let start,
    cleanup,
    live = true,
    complete;
  const done = new Promise((resolve) => {
    complete = resolve;
  });
  const enter = (work) => (owner ? owner.run("request-owner", work) : work());
  const lease = {
    operation(_context, work) {
      start = work;
    },
  };
  createHttpClient(lease, {
    fetch,
    grant: createGrant({ Http }),
    isAsciiUrl,
    writtenHost,
    timeoutMs,
  }).send(input, {});
  assert.equal(typeof start, "function", "the provider delegates start to its lease");
  enter(() => {
    cleanup = start((value) => {
      if (!live) {
        return;
      }
      live = false;
      cleanup?.();
      complete(value);
    });
  });
  return {
    done,
    abort() {
      if (!live) {
        return;
      }
      live = false;
      enter(() => cleanup?.());
      complete(undefined);
    },
  };
}
const answer = (status = 200, headers = {}, body = "body") =>
  new Response(body, { status, headers });

test("registering a transport operation performs no fetch before its lease starts it", async () => {
  let start,
    calls = 0,
    complete;
  const done = new Promise((resolve) => {
    complete = resolve;
  });
  const client = createHttpClient(
    {
      operation(_context, work) {
        start = work;
        return "pending";
      },
    },
    {
      grant: createGrant({}),
      isAsciiUrl,
      writtenHost,
      fetch() {
        calls++;
        return answer();
      },
    },
  );
  assert.equal(client.send(request("https://allowed.test/"), {}), "pending");
  assert.equal(calls, 0);
  const dispose = start(complete);
  try {
    assert.ok((await done).ok);
    assert.equal(calls, 1);
  } finally {
    dispose();
  }
});

test("ungranted, invalid, percent-host and URL-parser-smuggling requests perform no fetch", async () => {
  let calls = 0;
  const fetch = () => {
    calls++;
    return answer();
  };
  for (const [url, Http, tag] of [
    ["https://allowed.test/", false, "NotGranted"],
    ["https://other.test/", ["allowed.test"], "NotGranted"],
    ["https://bücher.test/", true, "InvalidUrl"],
    ["https://%61llowed.test/", true, "InvalidUrl"],
    ["http://denied.test\\@allowed.test/", true, "InvalidUrl"],
    ["ftp://allowed.test/", true, "InvalidUrl"],
  ]) {
    assert.equal((await operation(request(url), { Http, fetch }).done).error.tag, tag, url);
  }
  assert.equal(calls, 0);
});

test("every transport setting is refused in its fixed order before any fetch", async () => {
  let calls = 0;
  const fetch = () => {
    calls++;
    return answer();
  };
  const timeout = { millis: 1000n };
  for (const [settings, name] of [
    [{ dns_servers: ["10.0.0.2"], tls_roots: ["root"] }, "resolver"],
    [{ dns_servers: [] }, "resolver"],
    [{ tls_roots: ["root"], connect_timeout: timeout }, "tls.roots"],
    [{ tls_identity: { certificate_chain: "chain", private_key: "key" } }, "tls.identity"],
    [{ connect_timeout: timeout }, "connect_timeout"],
    [{ tls_handshake_timeout: timeout }, "tls_handshake_timeout"],
    [{ response_header_timeout: timeout }, "response_header_timeout"],
    [{ body_read_timeout: { millis: 0n } }, "body_read_timeout"],
  ]) {
    const result = await operation(request("https://denied.test/", settings), {
      Http: false,
      fetch,
    }).done;
    assert.deepEqual(result.error, { tag: "UnsupportedSetting", fields: { name } });
  }
  const defaults = {
    dns_servers: null,
    tls_roots: [],
    tls_identity: null,
    connect_timeout: null,
    tls_handshake_timeout: null,
    response_header_timeout: null,
    body_read_timeout: null,
  };
  assert.ok((await operation(request("https://allowed.test/", defaults), { fetch }).done).ok);
  assert.equal(calls, 1);
});

test("written host and port are checked before every redirect", async () => {
  const calls = [];
  const fetch = (url, init) => {
    calls.push({ url, init });
    return answer(302, { location: "http://denied.test/private" });
  };
  const value = await operation(request("https://allowed.test/"), {
    Http: ["allowed.test:443"],
    fetch,
  }).done;
  assert.equal(value.error.tag, "NotGranted");
  assert.equal(value.error.fields.host, "denied.test");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].init.redirect, "manual");
});

test("redirect authorities are validated as written before URL normalization", async () => {
  for (const location of [
    "//%65vil.test/end",
    "///%65vil.test/end",
    "https:denied.test/secret",
    "https:/denied.test/secret",
    "https:///denied.test/secret",
    "///denied.test/secret",
  ]) {
    for (const Http of [true, ["allowed.test"]]) {
      const calls = [];
      const value = await operation(request("http://allowed.test/"), {
        Http,
        fetch: (url) => {
          calls.push(url);
          return calls.length === 1 ? answer(302, { location }) : answer();
        },
      }).done;
      assert.equal(value.error?.tag, "InvalidUrl", location);
      assert.equal(calls.length, 1, `${location} must not fetch its normalized target`);
    }
  }
});

test("valid RFC redirects retain their destination under scoped grants", async () => {
  for (const [location, target] of [
    ["//allowed.test/end", "http://allowed.test/end"],
    ["/end", "http://allowed.test/end"],
    ["../end", "http://allowed.test/end"],
    ["?page=2", "http://allowed.test/start/item?page=2"],
    ["#part", "http://allowed.test/start/item#part"],
    ["HTTP://allowed.test/end", "http://allowed.test/end"],
  ]) {
    const calls = [];
    const value = await operation(request("http://allowed.test/start/item"), {
      Http: ["allowed.test"],
      fetch: (url) => {
        calls.push(url);
        return calls.length === 1 ? answer(302, { location }) : answer();
      },
    }).done;
    assert.ok(value.ok, location);
    assert.deepEqual(calls, ["http://allowed.test/start/item", target]);
    assert.equal(value.value.url, target);
  }
});

test("zero redirects returns the original response without validating Location", async () => {
  const value = await operation(request("https://allowed.test/", { redirects: 0 }), {
    fetch: () => answer(302, { location: "ftp://bad.test/next" }, "redirect body"),
  }).done;
  assert.equal(value.value.status, 302);
  assert.equal(new TextDecoder().decode(value.value.body), "redirect body");
});

test("redirect exhaustion names the next target and never sends it", async () => {
  const calls = [];
  const fetch = (url) => {
    calls.push(url);
    return answer(302, { location: "/next" });
  };
  const value = await operation(request("https://allowed.test/start", { redirects: 1 }), { fetch })
    .done;
  assert.equal(value.error.tag, "TooManyRedirects");
  assert.equal(value.error.fields.url, "https://allowed.test/next");
  assert.equal(calls.length, 2);
});

test("POST redirects use GET without body framing; 307 replays owned bytes", async () => {
  for (const status of [301, 302, 303, 307, 308]) {
    const calls = [];
    const input = request("https://allowed.test/start", {
      method: { tag: "Post", fields: {} },
      body: new Uint8Array([0, 255]),
      headers: [
        ["Content-Type", "binary"],
        ["Content-Length", "2"],
      ],
    });
    const fetch = (url, init) => {
      calls.push({ url, init });
      return calls.length === 1 ? answer(status, { location: "/end" }) : answer(200);
    };
    const value = await operation(input, { fetch }).done;
    assert.ok(value.ok);
    assert.equal(calls[1].init.method, status === 307 || status === 308 ? "POST" : "GET");
    if (status === 307 || status === 308) {
      assert.deepEqual(calls[1].init.body, new Uint8Array([0, 255]));
    } else {
      assert.equal(calls[1].init.body, undefined);
      assert.deepEqual(calls[1].init.headers, []);
    }
    assert.deepEqual(input.headers, [
      ["Content-Type", "binary"],
      ["Content-Length", "2"],
    ]);
  }
});

test("cross-origin redirects remove credentials before the next granted fetch", async () => {
  const calls = [];
  const fetch = (url, init) => {
    calls.push({ url, init });
    return calls.length === 1 ? answer(302, { location: "https://other.test/end" }) : answer(200);
  };
  const value = await operation(
    request("https://allowed.test/", {
      headers: [
        ["Authorization", "private"],
        ["Cookie", "private"],
        ["Proxy-Authorization", "private"],
        ["Host", "allowed.test"],
        ["X-Public", "ok"],
      ],
    }),
    { Http: ["allowed.test", "other.test"], fetch },
  ).done;
  assert.ok(value.ok);
  assert.deepEqual(calls[1].init.headers, [["X-Public", "ok"]]);
});

test("statuses and raw body bytes survive; repeated cookies use the platform multi-value API", async () => {
  const value = await operation(request("https://allowed.test/"), {
    fetch: () =>
      new Response(new Uint8Array([0, 255, 128]), {
        status: 503,
        headers: [
          ["Set-Cookie", "a=1; Expires=Wed, 21 Oct 2030 07:28:00 GMT"],
          ["Set-Cookie", "b=2"],
        ],
      }),
  }).done;
  assert.equal(value.value.status, 503);
  assert.deepEqual(value.value.body, new Uint8Array([0, 255, 128]));
  assert.equal(value.value.headers.filter(([name]) => name === "set-cookie").length, 2);
  const cloudflare = {
    *[Symbol.iterator]() {
      yield ["set-cookie", "combined"];
      yield ["x-test", "ok"];
    },
    getAll(name) {
      assert.equal(name, "Set-Cookie");
      return ["a=1", "b=2"];
    },
  };
  assert.deepEqual(responseHeaders(cloudflare), [
    ["set-cookie", "a=1"],
    ["set-cookie", "b=2"],
    ["x-test", "ok"],
  ]);
});

test("Fetch-decoded responses drop stale coding headers and enforce the 64 MiB cap", async () => {
  const decoded = await operation(request("https://allowed.test/"), {
    fetch: () =>
      new Response("decoded", {
        headers: { "Content-Encoding": "gzip", "Content-Length": "99", "X-Test": "kept" },
      }),
  }).done;
  assert.ok(decoded.ok);
  assert.equal(new TextDecoder().decode(decoded.value.body), "decoded");
  assert.equal(
    decoded.value.headers.some(([name]) =>
      ["content-encoding", "content-length"].includes(name.toLowerCase()),
    ),
    false,
  );
  assert.deepEqual(
    decoded.value.headers.find(([name]) => name === "x-test"),
    ["x-test", "kept"],
  );

  const atLimit = await operation(request("https://allowed.test/"), {
    fetch: () => ({
      status: 200,
      headers: new Headers(),
      arrayBuffer: async () => new ArrayBuffer(64 * 1024 * 1024),
    }),
  }).done;
  assert.equal(atLimit.value.body.length, 64 * 1024 * 1024);

  const oversized = await operation(request("https://allowed.test/"), {
    fetch: () => ({
      status: 200,
      headers: new Headers(),
      arrayBuffer: async () => new ArrayBuffer(64 * 1024 * 1024 + 1),
    }),
  }).done;
  assert.equal(oversized.error.tag, "OutputTooLarge");

  let redirects = 0;
  const oversizedRedirect = await operation(request("https://allowed.test/start"), {
    fetch: () => {
      redirects++;
      return {
        status: 302,
        headers: new Headers({ Location: "/next" }),
        arrayBuffer: async () => new ArrayBuffer(64 * 1024 * 1024 + 1),
      };
    },
  }).done;
  assert.equal(oversizedRedirect.error.tag, "OutputTooLarge");
  assert.equal(redirects, 1);
});

test("native fetch and abort run only through the lease callbacks", async () => {
  const owner = new AsyncLocalStorage(),
    events = [];
  let started;
  const began = new Promise((resolve) => {
    started = resolve;
  });
  const fetch = (_url, init) =>
    new Promise((_resolve, reject) => {
      events.push(["fetch", owner.getStore()]);
      started();
      init.signal.addEventListener(
        "abort",
        () => {
          events.push(["abort", owner.getStore()]);
          reject(new Error("cancelled"));
        },
        { once: true },
      );
    });
  const running = operation(request("https://allowed.test/"), { fetch, owner });
  await began;
  owner.run("other-request", () => running.abort());
  await running.done;
  assert.deepEqual(events, [
    ["fetch", "request-owner"],
    ["abort", "request-owner"],
  ]);
});

test("one timeout includes response body reading and aborts its controller", async () => {
  let aborted = 0;
  const fetch = (_url, init) => {
    const stream = new ReadableStream({
      start(controller) {
        init.signal.addEventListener(
          "abort",
          () => {
            aborted++;
            controller.error(new Error("cancelled"));
          },
          { once: true },
        );
      },
    });
    return new Response(stream);
  };
  const value = await operation(request("https://allowed.test/"), { fetch, timeoutMs: 15 }).done;
  assert.equal(value.error.tag, "Timeout");
  assert.equal(aborted, 1);
});

test(
  "native fetch handles an actual redirect, binary body and repeated cookies",
  { timeout: 5000 },
  async () => {
    const seen = [];
    const server = createServer((req, res) => {
      seen.push(req.url);
      if (req.url === "/start") {
        res.writeHead(307, { location: "/end" });
        res.end();
      } else {
        res.writeHead(404, { "set-cookie": ["a=1", "b=2"], "x-repeat": ["one", "two"] });
        res.end(Buffer.from([0, 255, 128]));
      }
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    try {
      const url = "http://127.0.0.1:" + server.address().port;
      const value = await operation(request(url + "/start"), {
        Http: ["127.0.0.1:" + server.address().port],
      }).done;
      assert.equal(value.value.status, 404);
      assert.equal(value.value.url, url + "/end");
      assert.deepEqual(value.value.body, new Uint8Array([0, 255, 128]));
      assert.equal(value.value.headers.filter(([name]) => name === "set-cookie").length, 2);
      assert.deepEqual(seen, ["/start", "/end"]);
    } finally {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

test("native DNS, TLS and connection causes retain their HttpError categories", async () => {
  for (const [code, message, tag] of [
    ["ENOTFOUND", "not found", "Dns"],
    ["CERT_HAS_EXPIRED", "certificate expired", "Tls"],
    ["EPROTO", "TLS alert unknown ca", "Tls"],
    ["ECONNREFUSED", "refused", "Connect"],
    ["", "platform error", "Other"],
  ]) {
    const value = await operation(request("https://allowed.test/"), {
      fetch() {
        throw new TypeError("fetch failed", { cause: { code, message } });
      },
    }).done;
    assert.equal(value.error.tag, tag);
  }
});
