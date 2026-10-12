// The browser layer of the JS host: what the playground binds over the
// platform-neutral core (core.mjs). The core already answers `Console`,
// `Clock`, `Random` and `Args`; this layer adds `Http`, sent with the
// browser and granted to the page's own origin alone
// (spec/std/http.md#playground). Every other platform trait (`FsRead`,
// `FsWrite`, `Process`, `Net`, `Env`, `Sys`, `ConsoleInput`) stays unbound,
// so a runner refuses a module that imports one before it instantiates it
// (`unboundTraits`).
//
// The program runs in a dedicated worker, and the ABI answers each host
// call synchronously, so `send` uses a synchronous `XMLHttpRequest`, which a
// worker still allows.

import { createCoreHost, Enc, HostContract } from "./core.mjs";

// `Method`'s variants (std.http), by index, as the wire names them.
const METHODS = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
// `HttpError`'s variants (std.http).
const HTTP = {
  NotGranted: 0,
  InvalidUrl: 1,
  Dns: 2,
  Connect: 3,
  Tls: 4,
  Timeout: 5,
  TooManyRedirects: 6,
  Other: 7,
};

// The text of a request the browser refused (std-http.playground.refused).
const REFUSED = "the browser refused the request: a network error or CORS";

// The page's origin: the worker's, which is the page's.
const ownOrigin = () => globalThis.location?.origin;

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
// timeout }`, with `body` a `Uint8Array` and `timeout` milliseconds or
// `null`. The answer is the Node client's shape (http.mjs): `{ status,
// headers, body }`, or `{ error, text }` with an `HttpError` variant name. A
// URL of another origin is `NotGranted(host)`
// (std-http.playground.other-origin), and a request the browser refuses is
// `Connect` (std-http.playground.refused).
export function browserSend({ method, url, headers, body, timeout }) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return { error: "InvalidUrl", text: url };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:")
    return { error: "InvalidUrl", text: url };
  // The grant covers the page's own origin alone (std-http.playground.origin).
  if (parsed.origin !== ownOrigin()) return { error: "NotGranted", text: parsed.hostname };
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
  };
}

// `hd:Http`: one `send`, decoded, made with the browser, and encoded
// (std-http.send.*), as the Node layer's `httpImports` does.
function httpImports({ argsOf, ready }) {
  const send = (d) => {
    const m = d.num();
    const method = m < METHODS.length ? METHODS[m] : d.str();
    const url = d.str();
    const headers = d.list((d) => [d.str(), d.str()]);
    const body = Uint8Array.from(d.bytes());
    const timeout = d.num() === 1 ? Number(d.zz()) : null;
    const r = browserSend({ method, url, headers, body, timeout });
    const e = new Enc();
    if (r.error) {
      e.leb(1).leb(HTTP[r.error]);
      return r.error === "Timeout" ? e : e.str(r.text);
    }
    return e
      .leb(0)
      .leb(r.status)
      .list(r.headers, (e, [n, v]) => e.str(n).str(v))
      .bytes(r.body);
  };
  return {
    "send.start": (len) => ready(send(argsOf(len))),
    // `send` never pends, so a finish names a stale handle.
    "send.finish": () => {
      throw new HostContract("a finish of an operation that is not pending");
    },
  };
}

const webLayer = {
  providers: ({ argsOf, ready }) => ({ imports: { "hd:Http": httpImports({ argsOf, ready }) } }),
};

// One host per instance in the browser; `sink` and `env` are
// `createCoreHost`'s.
export const createWebHost = (sink, env = {}) => createCoreHost(sink, env, webLayer);

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
