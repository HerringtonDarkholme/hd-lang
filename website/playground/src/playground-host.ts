// The playground's host providers (spec/std/http.md#playground): the web
// host's `Clock` and `Random`, as `hd run` answers them, and an `Http` that
// sends a request with the browser, granted to the page's own origin alone.
// The program runs in a dedicated worker, which answers each host call
// synchronously, so `send!` uses a synchronous `XMLHttpRequest`, which a
// worker still allows. `Process`, `Sys`, and `Net` stay unbound
// (std-http.playground.unbound).

import type {
  HostBoundaryValue,
  HostSuspensionCall,
  HostSuspensionOutcome,
} from "../../../src/compiler.ts";
import { WEB_HOST_TRAITS, webHostAnswer } from "../../../src/web-host.ts";

/** The traits the playground binds, by module and name. */
export const PLAYGROUND_TRAITS: readonly { readonly module: string; readonly name: string }[] = [
  ...WEB_HOST_TRAITS,
  { module: "std.http", name: "Http" },
];

/** The text of a request the browser refused (std-http.playground.refused). */
const REFUSED = "the browser refused the request: a network error or CORS";

const failed = (error: HostBoundaryValue): HostBoundaryValue =>
  ({ tag: "err", value: error }) as HostBoundaryValue;

/** The page's origin: the worker's, which is the page's. */
function ownOrigin(): string | undefined {
  return (globalThis as { location?: { origin?: string } }).location?.origin;
}

/** Each `name: value` line of `getAllResponseHeaders`, as a pair, in order. */
function headerPairs(text: string): [string, string][] {
  return text
    .split(/\r?\n/)
    .filter((line) => line.includes(":"))
    .map((line) => {
      const colon = line.indexOf(":");
      return [line.slice(0, colon).trim(), line.slice(colon + 1).trim()];
    });
}

/**
 * The playground's answer to `Http.send!(request)`. A URL of another origin
 * is `NotGranted(host)` (std-http.playground.other-origin), and a request
 * the browser refuses is `Connect` (std-http.playground.refused).
 */
export function sendFromBrowser(request: HostBoundaryValue): HostBoundaryValue {
  const fields = request as {
    readonly method: { readonly tag: string; readonly name?: string };
    readonly url: string;
    readonly headers: readonly (readonly [string, string])[];
    readonly body: readonly number[];
    readonly timeout: { readonly tag: "none" } | { readonly tag: "some"; readonly value: unknown };
  };
  let url: URL;
  try {
    url = new URL(fields.url);
  } catch {
    return failed({ tag: "InvalidUrl", url: fields.url });
  }
  if (url.protocol !== "http:" && url.protocol !== "https:")
    return failed({ tag: "InvalidUrl", url: fields.url });
  // The grant covers the page's own origin alone (std-http.playground.origin).
  if (url.origin !== ownOrigin()) return failed({ tag: "NotGranted", host: url.hostname });
  const method =
    fields.method.tag === "Other" ? String(fields.method.name) : fields.method.tag.toUpperCase();
  const xhr = new XMLHttpRequest();
  try {
    xhr.open(method, url.href, false);
    xhr.responseType = "arraybuffer";
    if (fields.timeout.tag === "some")
      xhr.timeout = Number((fields.timeout.value as { millis: bigint | number }).millis);
    // The browser drops a forbidden header, such as Cookie or Host.
    for (const [name, value] of fields.headers) xhr.setRequestHeader(name, value);
    xhr.send(method === "GET" || method === "HEAD" ? null : Uint8Array.from(fields.body));
  } catch (error) {
    if ((error as { name?: string }).name === "TimeoutError") return failed({ tag: "Timeout" });
    return failed({ tag: "Connect", message: REFUSED });
  }
  if (xhr.status === 0) return failed({ tag: "Connect", message: REFUSED });
  return {
    tag: "ok",
    value: {
      status: xhr.status,
      headers: headerPairs(xhr.getAllResponseHeaders()),
      body: [...new Uint8Array(xhr.response as ArrayBuffer)],
    },
  } as HostBoundaryValue;
}

/** The playground's answer to `call`, or undefined when no provider of it answers it. */
export function playgroundAnswer(call: HostSuspensionCall): HostSuspensionOutcome | undefined {
  if (call.standardName === "std.http.Http" && call.methodName === "send")
    return { pending: false, value: sendFromBrowser(call.arguments[0]!) };
  return webHostAnswer(call);
}
