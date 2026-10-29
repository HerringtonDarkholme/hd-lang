// The default profile's HTTP client (std-http.send.*): a worker thread
// that `node.mjs` starts on a program's first `Http.send!`. The program's
// thread exchanges short start/poll/abort messages on `port` and `flag`.
// This thread owns each request and its response body until completion;
// the guest can poll its clock while it waits, and abort destroys the
// request across every redirect or while its body is still arriving.
//
// - The grant covers a request by its URL's host and port
//   (cli.cap.scope.host, .host.forms), and each redirect target as a new
//   request (cli.cap.scope.redirect).
// - Up to the request's redirect limit are followed; the next is `TooManyRedirects` with
//   its target (std-http.send.redirect-limit). A 303, or a 301 or 302 of a
//   `POST`, continues as a `GET` without a body, as `fetch` does.
// - Every status is a response (std-http.send.status). Headers keep their
//   repeats (std-http.headers.fields); a name arrives in lowercase, and the
//   values of one name keep their order, grouped at the name's first
//   appearance (std-http.headers.case, .order). The body is whole
//   (std-http.bodies). The response's `url` is the last request's URL
//   (std-http.response.url).
// - A request that takes longer than the provider's 30 s bound is
//   `Timeout` (std-http.send.provider-timeout).
import { postReply } from "./worker-control.mjs";
import http from "node:http";
import https from "node:https";
import { isAsciiUrl, writtenHost } from "./core.mjs";
import { MAX_DECODED_BYTES, decodedResponse, hasNoBody } from "./http-decode.mjs";
import { workerData } from "node:worker_threads";
import { createGrant } from "./node.mjs";

// The provider's default timeout (std-http.request.default-timeout).
const DEFAULT_TIMEOUT_MS = 30_000;

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
  if (
    code === "ENOTFOUND" ||
    code === "EAI_AGAIN" ||
    code === "EAI_NODATA" ||
    code === "EAI_FAIL"
  ) {
    return err("Dns", host);
  }
  // A fatal TLS alert from the peer, such as `unknown_ca`, surfaces as
  // `EPROTO` with OpenSSL's text (`tlsv1 alert unknown ca`).
  const alert = code === "EPROTO" && /\b(tls|ssl)/i.test(String(e.message));
  if (TLS_CODES.has(code) || code.startsWith("ERR_TLS") || code.startsWith("ERR_SSL") || alert) {
    return err("Tls", String(e.message));
  }
  return err("Connect", String(e?.message ?? e));
};

// One request without redirects, within `ms` milliseconds.
const once = (url, host, method, headers, body, ms, signal) =>
  new Promise((done) => {
    if (ms <= 0) {
      done(err("Timeout"));
      return;
    }
    if (signal.aborted) {
      done(err("Other", "request cancelled"));
      return;
    }
    const lib = url.protocol === "https:" ? https : http;
    let settled = false;
    let timer = null;
    const aborted = () => settle(err("Other", "request cancelled"));
    const settle = (r) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        signal.removeEventListener("abort", aborted);
        done(r);
      }
    };
    // A body is sent with its length, as `fetch` sends a byte body,
    // unless the request gives its own framing.
    // Compression negotiation is raw-transport policy. Providers and
    // caller-visible request values keep exactly the headers the caller
    // wrote; only the bytes sent on this transport gain the default.
    const encoded = headers.some(([n]) => /^accept-encoding$/i.test(n))
      ? headers
      : [
          ...headers,
          [
            "Accept-Encoding",
            headers.some(([n]) => /^range$/i.test(n)) ? "identity" : "gzip, deflate",
          ],
        ];
    const framed = encoded.some(([n]) => /^(content-length|transfer-encoding)$/i.test(n));
    const sized =
      body.length && !framed ? [...encoded, ["Content-Length", String(body.length)]] : encoded;
    // Headers go to Node as a raw list, which keeps repeated names in
    // order but makes Node skip its own `Host`; an HTTP/1.1 request
    // needs one (RFC 9112 3.2), so the URL's authority supplies it unless
    // the request names its own. The host is the one as written, not
    // WHATWG's rewrite of it (std-http.url.no-normalize); a name such as
    // `127.1` is the resolver's to resolve.
    const shown = host.includes(":") ? `[${host}]` : host;
    const authority = url.port ? `${shown}:${url.port}` : shown;
    const all = sized.some(([n]) => /^host$/i.test(n)) ? sized : [["Host", authority], ...sized];
    const req = lib.request({
      protocol: url.protocol,
      host,
      port: url.port || undefined,
      path: url.pathname + url.search,
      method,
      headers: all.flat(),
      agent: false,
      signal,
    });
    // Node 24 silently drops response headers past 1000 pairs; a request
    // option does not lift that, the property does (0 is no limit).
    // This transport preserves raw pairs; std-http.headers.combined also
    // permits platforms to combine repeated non-cookie fields.
    req.maxHeadersCount = 0;
    timer = setTimeout(() => {
      settle(err("Timeout"));
      req.destroy();
    }, ms);
    signal.addEventListener("abort", aborted, { once: true });
    req.on("error", (e) => settle(failure(e, host)));
    req.on("response", (res) => {
      const groups = new Map();
      for (let i = 0; i + 1 < res.rawHeaders.length; i += 2) {
        const name = res.rawHeaders[i].toLowerCase();
        if (!groups.has(name)) groups.set(name, []);
        groups.get(name).push(res.rawHeaders[i + 1]);
      }
      const pairs = [...groups].flatMap(([name, values]) => values.map((v) => [name, v]));
      const codings = pairs
        .filter(([name]) => name === "content-encoding")
        .flatMap(([, value]) => value.split(",").map((part) => part.trim().toLowerCase()));
      const decodedIsWireBody = codings.every((coding) => coding === "identity");
      const chunks = [];
      let received = 0;
      res.on("data", (c) => {
        received += c.length;
        if (
          !hasNoBody(method, res.statusCode) &&
          decodedIsWireBody &&
          received > MAX_DECODED_BYTES
        ) {
          settle(err("OutputTooLarge"));
          res.destroy();
          return;
        }
        chunks.push(c);
      });
      res.on("error", (e) => settle(failure(e, host)));
      res.on("end", () => {
        settle(decodedResponse(method, res.statusCode, pairs, Buffer.concat(chunks)));
      });
    });
    req.end(body.length ? Buffer.from(body) : undefined);
  });

// A request and its redirects, as the program's grant allows them.
async function send(r, signal) {
  const grant = createGrant(r.grants);
  let { method, url, body } = r;
  let written = url;
  const deadline = Date.now() + DEFAULT_TIMEOUT_MS;
  for (let redirects = 0; ; redirects++) {
    if (signal.aborted) return err("Other", "request cancelled");
    let u;
    try {
      if (!isAsciiUrl(url)) return err("InvalidUrl", url);
      u = new URL(url);
    } catch {
      return err("InvalidUrl", url);
    }
    if (u.protocol !== "http:" && u.protocol !== "https:") return err("InvalidUrl", url);
    const host = writtenHost(written);
    const port = u.port ? Number(u.port) : u.protocol === "https:" ? 443 : 80;
    if (!grant.coversHost("Http", host, port)) return err("NotGranted", host);
    const got = await once(u, host, method, r.headers, body, deadline - Date.now(), signal);
    const location = got.headers?.find(([n]) => n.toLowerCase() === "location")?.[1];
    if (
      got.error ||
      r.redirects === 0 ||
      ![301, 302, 303, 307, 308].includes(got.status) ||
      location === undefined
    ) {
      return got.error ? got : { ...got, url };
    }
    // A Location outside printable ASCII is no RFC 3986 URL.
    if (!isAsciiUrl(location)) return err("InvalidUrl", location);
    url = new URL(location, u).href;
    // A relative Location keeps the host as written; an absolute one
    // brings its own.
    if (/^([a-z][a-z0-9+.-]*:)?\/\//i.test(location)) {
      written = /^\/\//.test(location) ? `${u.protocol}${location}` : location;
    }
    if (redirects === r.redirects) return err("TooManyRedirects", url);
    if (got.status === 303 || ((got.status === 301 || got.status === 302) && method === "POST")) {
      method = "GET";
      body = new Uint8Array(0);
    }
  }
}

// Replies go only to control calls; asynchronous completion never posts
// an unsolicited message that could become another operation's reply.
const operations = new Map();
let nextOperation = 1;
const run = (r) => {
  switch (r.op) {
    case "start": {
      const operation = nextOperation++;
      const controller = new AbortController();
      const state = { controller, result: null };
      operations.set(operation, state);
      const complete = (result) => {
        if (operations.get(operation) === state) state.result = result;
      };
      send(r, controller.signal).then(complete, (error) =>
        complete(err("Other", String(error?.message ?? error))),
      );
      return { ok: operation };
    }
    case "poll": {
      const state = operations.get(r.operation);
      if (state === undefined) throw new Error("unknown HTTP operation");
      if (state.result === null) return { pending: true };
      operations.delete(r.operation);
      return state.result;
    }
    case "abort":
      operations.get(r.operation)?.controller.abort();
      operations.delete(r.operation);
      return { ok: null };
    case "open":
      return { ok: operations.size };
    default:
      throw new Error("unknown HTTP control call");
  }
};

const { port, flag } = workerData;
port.on("message", ({ id, request: r }) => {
  let out;
  try {
    out = run(r);
  } catch (e) {
    out = err("Other", String(e?.message ?? e));
  }
  postReply(port, flag, id, out);
});
