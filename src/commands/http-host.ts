// The default profile's `Http` provider on Node
// (spec/cli/command-line.md#host-capabilities, spec/std/http.md#sending).
// The prototype answers every host call synchronously, so `send!` runs one
// request at a time on a worker thread, and the main thread waits for it
// with `Atomics.wait` (future-work/HOST_CAPABILITIES.md, S6). The provider
// follows redirects itself, so it checks the grant of each hop
// (spec/cli/command-line.md#r-cli.cap.scope.redirect).

import { MessageChannel, receiveMessageOnPort, Worker } from "node:worker_threads";

import type { HostBoundaryValue } from "../compiler.ts";
import { coversHost, type Grant } from "./capabilities.ts";

/** The provider's default timeout (spec/std/http.md#r-std-http.send.timeout). */
const DEFAULT_TIMEOUT_MS = 30_000;

/** The most redirects a request follows (spec/std/http.md#r-std-http.send.redirect). */
const MAX_REDIRECTS = 10;

/** One request on the wire, as the worker sends it. */
interface Hop {
  readonly method: string;
  readonly url: string;
  readonly headers: readonly (readonly [string, string])[];
  readonly body: readonly number[];
  readonly timeout: number;
}

/** What the worker answers for one hop. */
type HopResult =
  | {
      readonly ok: true;
      readonly status: number;
      readonly headers: [string, string][];
      readonly body: number[];
    }
  | { readonly ok: false; readonly error: HostBoundaryValue };

// The worker sends one hop with `node:http` or `node:https`, which keep a
// repeated header and the order of the headers, and follow no redirect.
const WORKER = `
const { parentPort } = require("node:worker_threads");
const http = require("node:http");
const https = require("node:https");
const TLS = /^(ERR_TLS_|ERR_SSL_|CERT_|UNABLE_TO_|DEPTH_ZERO_|SELF_SIGNED_|HOSTNAME_MISMATCH)/;
parentPort.on("message", ({ hop, port, signal }) => {
  let settled = false;
  const finish = (result) => {
    if (settled) return;
    settled = true;
    port.postMessage(result);
    port.close();
    Atomics.store(signal, 0, 1);
    Atomics.notify(signal, 0);
  };
  const url = new URL(hop.url);
  // Headers given as a list get none of Node's defaults, so Host is added here.
  const named = (name) => hop.headers.some(([key]) => key.toLowerCase() === name);
  const flat = [...(named("host") ? [] : ["Host", url.host]), ...hop.headers.flat()];
  let request;
  try {
    request = (url.protocol === "https:" ? https : http).request(url, { method: hop.method, headers: flat });
  } catch (error) {
    finish({ ok: false, error: { tag: "Other", message: String(error && error.message) } });
    return;
  }
  const timer = setTimeout(() => {
    finish({ ok: false, error: { tag: "Timeout" } });
    request.destroy();
  }, hop.timeout);
  request.on("response", (response) => {
    const chunks = [];
    response.on("data", (chunk) => chunks.push(chunk));
    response.on("end", () => {
      clearTimeout(timer);
      const headers = [];
      for (let index = 0; index < response.rawHeaders.length; index += 2)
        headers.push([response.rawHeaders[index], response.rawHeaders[index + 1]]);
      finish({ ok: true, status: response.statusCode, headers, body: [...Buffer.concat(chunks)] });
    });
    response.on("error", (error) => {
      clearTimeout(timer);
      finish({ ok: false, error: { tag: "Connect", message: String(error.message) } });
    });
  });
  request.on("error", (error) => {
    clearTimeout(timer);
    const code = String(error.code ?? "");
    finish({
      ok: false,
      error:
        code === "ENOTFOUND" || code === "EAI_AGAIN"
          ? { tag: "Dns", host: url.hostname }
          : TLS.test(code)
            ? { tag: "Tls", message: String(error.message) }
            : { tag: "Connect", message: String(error.message) },
    });
  });
  request.end(Buffer.from(hop.body));
});
`;

let worker: Worker | undefined;

/** Sends one hop on the worker, and waits for its answer. */
function sendHop(hop: Hop): HopResult {
  worker ??= new Worker(WORKER, { eval: true });
  // The worker never keeps the program running once its entry returns.
  worker.unref();
  const { port1, port2 } = new MessageChannel();
  const signal = new Int32Array(new SharedArrayBuffer(4));
  worker.postMessage({ hop, port: port2, signal }, [port2]);
  // The worker's own timer ends the hop; the extra second covers its start.
  const waited = Atomics.wait(signal, 0, 0, hop.timeout + 1000);
  const message = receiveMessageOnPort(port1);
  port1.close();
  if (waited === "timed-out" || !message) return { ok: false, error: { tag: "Timeout" } };
  return message.message as HopResult;
}

const failed = (error: HostBoundaryValue): HostBoundaryValue =>
  ({ tag: "err", value: error }) as HostBoundaryValue;

/** The wire name of a `Method` value as it crosses the boundary. */
function methodName(method: HostBoundaryValue): string {
  const tagged = method as { readonly tag: string; readonly name?: string };
  return tagged.tag === "Other" ? String(tagged.name) : tagged.tag.toUpperCase();
}

/** An absolute `http` or `https` URL, or undefined (spec/std/http.md#r-std-http.error.invalid-url). */
function httpUrl(text: string, base?: URL): URL | undefined {
  try {
    const url = new URL(text, base);
    return url.protocol === "http:" || url.protocol === "https:" ? url : undefined;
  } catch {
    return undefined;
  }
}

/**
 * The host's answer to `Http.send!(request)`: a `Result[Response,
 * HttpError]`. A host outside `grant` is `NotGranted`, for the request and
 * for each redirect target (spec/cli/command-line.md#partial-deny).
 */
export function sendRequest(request: HostBoundaryValue, grant: Grant): HostBoundaryValue {
  const fields = request as {
    readonly method: HostBoundaryValue;
    readonly url: string;
    readonly headers: readonly (readonly [string, string])[];
    readonly body: readonly number[];
    readonly timeout: { readonly tag: "none" } | { readonly tag: "some"; readonly value: unknown };
  };
  const timeout =
    fields.timeout.tag === "some"
      ? Math.max(0, Number((fields.timeout.value as { millis: bigint | number }).millis))
      : DEFAULT_TIMEOUT_MS;
  let url = httpUrl(fields.url);
  if (!url) return failed({ tag: "InvalidUrl", url: fields.url });
  let method = methodName(fields.method);
  let body = fields.body;
  for (let redirects = 0; ; redirects++) {
    if (!coversHost(grant, url)) return failed({ tag: "NotGranted", host: url.hostname });
    const result = sendHop({
      method,
      url: url.href,
      headers: fields.headers.map(([name, value]) => [name, value] as const),
      body,
      timeout,
    });
    if (!result.ok) return failed(result.error);
    const location = result.headers.find(([name]) => name.toLowerCase() === "location")?.[1];
    if (location === undefined || ![301, 302, 303, 307, 308].includes(result.status))
      return {
        tag: "ok",
        value: { status: result.status, headers: result.headers, body: result.body },
      } as HostBoundaryValue;
    const target = httpUrl(location, url);
    if (!target) return failed({ tag: "InvalidUrl", url: location });
    if (redirects === MAX_REDIRECTS) return failed({ tag: "TooManyRedirects", url: target.href });
    // A 303, or a 301 or 302 after a POST, continues as a GET with no body,
    // as `fetch` does.
    if (result.status === 303 || (method === "POST" && result.status <= 302)) {
      if (method !== "HEAD") method = "GET";
      body = [];
    }
    url = target;
  }
}
