// The default profile's HTTP client (std-http.send.*): a worker thread
// that `core.mjs` starts on a program's first `Http.send!`. The program's
// thread posts each request on `port` and waits on `flag`; this thread
// makes the request with `node:http` or `node:https`, posts the outcome
// back, and wakes it. So a request finishes inside its `.start` call, as a
// `Process.run!` does, and `block_on` never waits on the event loop.
//
// - The grant covers a request by its URL's host and port
//   (cli.cap.scope.host, .host.forms), and each redirect target as a new
//   request (cli.cap.scope.redirect).
// - Up to 10 redirects are followed; the 11th is `TooManyRedirects` with
//   its target (std-http.send.redirect). A 303, or a 301 or 302 of a
//   `POST`, continues as a `GET` without a body, as `fetch` does.
// - Every status is a response (std-http.send.status). Headers keep their
//   order and repeats (std-http.headers.pairs); the body is whole
//   (std-http.bodies).
// - A request that takes longer than its timeout, or 30 s by default, is
//   `Timeout` (std-http.send.timeout).
import http from "node:http";
import https from "node:https";
import { workerData } from "node:worker_threads";
import { createGrant } from "./core.mjs";

// The provider's default timeout (std-http.request.defaults).
const DEFAULT_TIMEOUT_MS = 30_000;
const REDIRECTS = 10;

const err = (variant, text = null) => ({ error: variant, text });

const TLS_CODES = new Set([
  "CERT_HAS_EXPIRED",
  "CERT_NOT_YET_VALID",
  "DEPTH_ZERO_SELF_SIGNED_CERT",
  "SELF_SIGNED_CERT_IN_CHAIN",
  "UNABLE_TO_GET_ISSUER_CERT",
  "UNABLE_TO_GET_ISSUER_CERT_LOCALLY",
  "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
]);

// The `HttpError` of a failed connection to `host`.
const failure = (e, host) => {
  const code = e?.code ?? "";
  if (code === "ENOTFOUND" || code === "EAI_AGAIN" || code === "EAI_NODATA" || code === "EAI_FAIL") {
    return err("Dns", host);
  }
  if (TLS_CODES.has(code) || code.startsWith("ERR_TLS") || code.startsWith("ERR_SSL")) {
    return err("Tls", String(e.message));
  }
  return err("Connect", String(e?.message ?? e));
};

// One request without redirects, within `ms` milliseconds.
const once = (url, method, headers, body, ms) =>
  new Promise((done) => {
    if (ms <= 0) {
      done(err("Timeout"));
      return;
    }
    const lib = url.protocol === "https:" ? https : http;
    let settled = false;
    const settle = (r) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        done(r);
      }
    };
    // A body is sent with its length, as `fetch` sends a byte body,
    // unless the request gives its own framing.
    const framed = headers.some(([n]) => /^(content-length|transfer-encoding)$/i.test(n));
    const sized = body.length && !framed ? [...headers, ["Content-Length", String(body.length)]] : headers;
    // Headers go to Node as a raw list, which keeps repeated names in
    // order but makes Node skip its own `Host`; an HTTP/1.1 request
    // needs one (RFC 9112 3.2), so the URL's authority supplies it unless
    // the request names its own.
    const all = sized.some(([n]) => /^host$/i.test(n)) ? sized : [["Host", url.host], ...sized];
    const req = lib.request(url, { method, headers: all.flat(), agent: false });
    const timer = setTimeout(() => {
      settle(err("Timeout"));
      req.destroy();
    }, ms);
    req.on("error", (e) => settle(failure(e, url.hostname)));
    req.on("response", (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("error", (e) => settle(failure(e, url.hostname)));
      res.on("end", () => {
        const pairs = [];
        for (let i = 0; i + 1 < res.rawHeaders.length; i += 2) {
          pairs.push([res.rawHeaders[i], res.rawHeaders[i + 1]]);
        }
        settle({ status: res.statusCode, headers: pairs, body: Buffer.concat(chunks) });
      });
    });
    req.end(body.length ? Buffer.from(body) : undefined);
  });

// A request and its redirects, as the program's grant allows them.
async function send(r) {
  const grant = createGrant(r.grants);
  let { method, url, body } = r;
  const deadline = Date.now() + (r.timeout ?? DEFAULT_TIMEOUT_MS);
  for (let redirects = 0; ; redirects++) {
    let u;
    try {
      u = new URL(url);
    } catch {
      return err("InvalidUrl", url);
    }
    if (u.protocol !== "http:" && u.protocol !== "https:") return err("InvalidUrl", url);
    const host = u.hostname.replace(/^\[(.*)\]$/, "$1");
    const port = u.port ? Number(u.port) : u.protocol === "https:" ? 443 : 80;
    if (!grant.coversHost("Http", host, port)) return err("NotGranted", host);
    const got = await once(u, method, r.headers, body, deadline - Date.now());
    const location = got.headers?.find(([n]) => n.toLowerCase() === "location")?.[1];
    if (got.error || ![301, 302, 303, 307, 308].includes(got.status) || location === undefined) {
      return got;
    }
    url = new URL(location, u).href;
    if (redirects === REDIRECTS) return err("TooManyRedirects", url);
    if (got.status === 303 || ((got.status === 301 || got.status === 302) && method === "POST")) {
      method = "GET";
      body = new Uint8Array(0);
    }
  }
}

const { port, flag } = workerData;
port.on("message", async (r) => {
  let out;
  try {
    out = await send(r);
  } catch (e) {
    out = err("Other", String(e?.message ?? e));
  }
  port.postMessage(out);
  Atomics.store(flag, 0, 1);
  Atomics.notify(flag, 0);
});
