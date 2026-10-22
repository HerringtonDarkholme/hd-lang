// The browser layer of the JS host: what the playground binds over the
// platform-neutral core (core.mjs). The core already answers `Console`,
// `Clock`, `Random`, `Args` and most of `TestRunner`; this layer adds
// `TestRunner.temp_dir` and `Http`, sent with the browser and granted to
// the page's own origin alone (spec/std/http.md#playground). Every other platform trait (`FsRead`,
// `FsWrite`, `Process`, `Net`, `Env`, `Sys`, `ConsoleInput`) stays unbound,
// so a runner refuses a module that imports one before it instantiates it
// (`unboundTraits`).
//
// The program runs in a dedicated worker, and the ABI answers each host
// call synchronously, so `send` uses a synchronous `XMLHttpRequest`, which a
// worker still allows.

import { createCoreHost, createSession, httpProvider, isAsciiUrl, writtenHost } from "./core.mjs";

// The text of a request the browser refused (std-http.playground.refused).
const REFUSED = "the browser refused the request: a network error or CORS";

// Each `name: value` line of `getAllResponseHeaders`, as a pair, in order.
const headerPairs = (text) =>
  text
    .split(/\r?\n/)
    .filter((line) => line.includes(":"))
    .map((line) => {
      const colon = line.indexOf(":");
      return [line.slice(0, colon).trim(), line.slice(colon + 1).trim()];
    });

// Sends one request with the browser: `{ method, url, headers, body,
// redirects, timeout }`, with `body` a `Uint8Array` and `timeout` milliseconds or
// `null`. The answer is the Node client's shape (http.mjs): `{ status,
// headers, body }`, or `{ error, text }` with an `HttpError` variant name. A
// URL of another origin is `NotGranted(host)`
// (std-http.playground.other-origin), and a request the browser refuses is
// `Connect` (std-http.playground.refused).
export function browserSend({ method, url, headers, body, redirects, timeout }) {
  let parsed;
  try {
    if (!isAsciiUrl(url)) return { error: "InvalidUrl", text: url };
    parsed = new URL(url);
  } catch {
    return { error: "InvalidUrl", text: url };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:")
    return { error: "InvalidUrl", text: url };
  // The grant covers the page's own origin alone (std-http.playground.origin).
  // The host is the one as written (std-http.url.grant, .no-normalize),
  // not WHATWG's rewrite of it; the scheme and port are the origin's.
  // The page's origin is the worker's; only its `origin` string is relied
  // on, as every location has one.
  const host = writtenHost(url);
  const origin = globalThis.location?.origin;
  const own = origin ? new URL(origin) : undefined;
  const ownHost = own?.hostname.replace(/^\[(.*)\]$/, "$1").toLowerCase();
  if (host !== ownHost || parsed.protocol !== own?.protocol || parsed.port !== own?.port) {
    return { error: "NotGranted", text: host };
  }
  // XMLHttpRequest follows redirects automatically; do not silently turn a
  // caller's raw-response request into a followed response.
  if (redirects === 0) {
    return { error: "Other", text: "the browser host cannot stop redirects" };
  }
  const xhr = new XMLHttpRequest();
  try {
    xhr.open(method, parsed.href, false);
    xhr.responseType = "arraybuffer";
    if (timeout !== null) xhr.timeout = timeout;
    // The browser drops a forbidden header, such as Cookie or Host.
    for (const [name, value] of headers) xhr.setRequestHeader(name, value);
    xhr.send(method === "GET" || method === "HEAD" ? null : body);
  } catch (error) {
    if (error?.name === "TimeoutError") return { error: "Timeout", text: "" };
    return { error: "Connect", text: REFUSED };
  }
  if (xhr.status === 0) return { error: "Connect", text: REFUSED };
  return {
    status: xhr.status,
    headers: headerPairs(xhr.getAllResponseHeaders()),
    body: new Uint8Array(xhr.response),
    url: xhr.responseURL || parsed.href,
  };
}

const webLayer = {
  providers: ({ env }) => ({
    traits: {
      "hd:Http": httpProvider(browserSend),
      // The temp directory of a case, a path in the run's in-memory files
      // (`memoryFiles`); the core has the other hooks.
      "hd:TestRunner": { temp_dir: () => env.tempDir ?? "/tmp" },
    },
  }),
};

// One host per instance in the browser; `sink` and `env` are
// `createCoreHost`'s. A test run passes its `memoryFiles()` as `env.files`.
export const createWebHost = (sink, env = {}) => createCoreHost(sink, env, webLayer);

// A REPL session in the browser (core.mjs `createSession`), over the same
// providers as a run.
export const createWebSession = (sink, env = {}, options = {}) =>
  createSession(sink, env, webLayer, options);

// The files of one test run in the browser (cases.mjs): kept in memory and
// gone with the run, since the playground has no disk
// (crates/hd_web/DESIGN.md §5).
export function memoryFiles() {
  const texts = new Map();
  return {
    join: (dir, name) => `${dir}/${name}`,
    read: (path) => texts.get(path) ?? null,
    write: (path, text) => {
      texts.set(path, text);
    },
    remove: (dir) => {
      for (const path of [...texts.keys()]) {
        if (path === dir || path.startsWith(`${dir}/`)) texts.delete(path);
      }
    },
  };
}

// The traits a compiled module imports that `host` does not bind, by name
// and in import order: `hd:FsRead` gives `FsRead`. A runner refuses such a
// module before it instantiates it, as `hd_run::check_grants` refuses a
// total deny.
export function unboundTraits(module, host) {
  const names = WebAssembly.Module.imports(module)
    .map(({ module }) => module)
    .filter((name) => name.startsWith("hd:") && !Object.hasOwn(host.imports, name))
    .map((name) => name.slice(3));
  return [...new Set(names)];
}
