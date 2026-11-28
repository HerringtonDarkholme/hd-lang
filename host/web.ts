// The browser layer of the JS host: what the playground binds over the
// platform-neutral core (core.ts). The core already answers `Console`,
// `Clock`, `Random`, `Args` and most of `TestRunner`; this layer adds
// `TestRunner.temp_dir`, `Workers` on a browser `Worker`, and `Http`,
// sent with the browser and granted to the page's own origin alone
// (spec/std/http.md#playground). Every other platform trait (`FsRead`,
// `FsWrite`, `Process`, `Net`, `Env`, `Sys`, `ConsoleInput`) stays unbound,
// so a runner refuses a module that imports one before it instantiates it
// (`unboundTraits`).
//
// The program runs in a dedicated worker, and the ABI answers each host
// call synchronously, so `send` uses a synchronous `XMLHttpRequest`, which a
// worker still allows.

import {
  createCoreHost,
  httpProvider,
  isAsciiUrl,
  unsupportedSetting,
  writtenHost,
} from "./core.ts";
import type { CoreEnv, Host, Session, Sink } from "./core.ts";
import type { TestFiles } from "./cases.ts";
import { createSession } from "./session.ts";
import { Frames, workerSends } from "./worker-link.ts";

export type { Host, Session, Sink } from "./core.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

export interface BrowserRequest {
  method: string;
  url: string;
  headers: readonly (readonly [string, string])[];
  body: Uint8Array;
  redirects: number;
  timeout: number | null;
  settings?: {
    dnsServers: string[] | null;
    tlsRoots: string[];
    tlsIdentity: { certificateChain: string; privateKey: string } | null;
    connectTimeout: number | null;
    tlsHandshakeTimeout: number | null;
    responseHeaderTimeout: number | null;
    bodyReadTimeout: number | null;
  };
}

export type BrowserAnswer =
  | {
      status: number;
      headers: [string, string][];
      body: Uint8Array;
      url: string;
      error?: undefined;
    }
  | { error: string; text: string | null };

// The text of a request the browser refused (std-http.playground.refused).
const REFUSED = "the browser refused the request: a network error or CORS";
const MAX_DECODED_BYTES = 64 * 1024 * 1024;

// Each `name: value` line of `getAllResponseHeaders`, as a pair, in order.
const headerPairs = (text: string): [string, string][] =>
  text
    .split(/\r?\n/)
    .filter((line: string) => line.includes(":"))
    .map((line: string) => {
      const colon = line.indexOf(":");
      return [line.slice(0, colon).trim(), line.slice(colon + 1).trim()];
    });

function browserUrl(url: string): URL | undefined {
  try {
    if (!isAsciiUrl(url)) {
      return undefined;
    }
    const parsed = new URL(url);
    return ["http:", "https:"].includes(parsed.protocol) ? parsed : undefined;
  } catch {
    return undefined;
  }
}

function grantedOrigin(parsed: URL, url: string): boolean {
  const origin = globalThis.location?.origin;
  const own = origin ? new URL(origin) : undefined;
  const ownHost = own?.hostname.replace(/^\[(.*)\]$/, "$1").toLowerCase();
  return (
    writtenHost(url) === ownHost && parsed.protocol === own?.protocol && parsed.port === own?.port
  );
}

function xhrResponse(
  parsed: URL,
  { method, headers, body, timeout }: BrowserRequest,
): BrowserAnswer {
  const xhr = new XMLHttpRequest();
  try {
    xhr.open(method, parsed.href, false);
    xhr.responseType = "arraybuffer";
    if (timeout !== null) {
      xhr.timeout = timeout;
    }
    for (const [name, value] of headers) {
      xhr.setRequestHeader(name, value);
    }
    xhr.send(method === "GET" || method === "HEAD" ? null : (body as XMLHttpRequestBodyInit));
  } catch (error) {
    return (error as Dynamic)?.name === "TimeoutError"
      ? { error: "Timeout", text: "" }
      : { error: "Connect", text: REFUSED };
  }
  if (xhr.status === 0) {
    return { error: "Connect", text: REFUSED };
  }
  const decodedBody = new Uint8Array(xhr.response);
  if (decodedBody.length > MAX_DECODED_BYTES) {
    return { error: "OutputTooLarge", text: null };
  }
  const responseHeaders = headerPairs(xhr.getAllResponseHeaders());
  const staleEncoding = responseHeaders.some(([name]) => name.toLowerCase() === "content-encoding");
  return {
    status: xhr.status,
    headers: staleEncoding
      ? responseHeaders.filter(
          ([name]) => !["content-encoding", "content-length"].includes(name.toLowerCase()),
        )
      : responseHeaders,
    body: decodedBody,
    url: xhr.responseURL || parsed.href,
  };
}

// Sends one request with the browser: `{ method, url, headers, body,
// redirects, timeout, settings }`, with `body` a `Uint8Array` and `timeout` milliseconds or
// `null`. The answer is the Node client's shape (http.ts): `{ status,
// headers, body }`, or `{ error, text }` with an `HttpError` variant name. A
// URL of another origin is `NotGranted(host)`
// (std-http.playground.other-origin), and a request the browser refuses is
// `Connect` (std-http.playground.refused).
export function browserSend({
  method,
  url,
  headers,
  body,
  redirects,
  timeout,
  settings,
}: BrowserRequest): BrowserAnswer {
  // XHR exposes no resolver, TLS identity or phase timer, so every
  // transport setting is refused before any work (std-http.settings.host-support).
  const refused = unsupportedSetting(settings);
  if (refused) {
    return refused;
  }
  const parsed = browserUrl(url);
  if (!parsed) {
    return { error: "InvalidUrl", text: url };
  }
  // The grant covers the page's own origin alone (std-http.playground.origin).
  // The host is the one as written (std-http.url.grant, .no-normalize),
  // not WHATWG's rewrite of it; the scheme and port are the origin's.
  // The page's origin is the worker's; only its `origin` string is relied
  // on, as every location has one.
  if (!grantedOrigin(parsed, url)) {
    return { error: "NotGranted", text: writtenHost(url) };
  }
  // XMLHttpRequest follows redirects automatically; do not silently turn a
  // caller's raw-response request into a followed response.
  if (redirects === 0) {
    return { error: "Other", text: "the browser host cannot stop redirects" };
  }
  return xhrResponse(parsed, { method, url, headers, body, redirects, timeout, settings });
}

// Posts `message` to a worker or its parent, moving `frame`'s buffer.
const post = (to: Dynamic, message: Dynamic, frame?: Uint8Array): void =>
  to.postMessage(message, frame ? [frame.buffer] : []);

// `hd:Workers` in the browser (std-remote.workers.host, worker-link.ts):
// the first send starts a dedicated `Worker` from `env.workerScript`, the
// run worker's own bundle, which runs a second instance of the program's
// module (`serveWorker`) and sends its output here. A send waits as any
// pending operation does: the run awaits the event loop, and the reply's
// message wakes it, so it needs neither `SharedArrayBuffer` nor JSPI.
// Without a script, the core's `Unreachable` answers.
// Cost: one worker and one instance per run; per send, one event-loop
// task per reply; no polling.
function browserWorkers(env: CoreEnv, sink: Sink): Dynamic {
  const { module, workerScript } = env;
  if (typeof workerScript !== "string" || !(module instanceof WebAssembly.Module)) {
    return null;
  }
  const workers = workerSends((deliver) => {
    const worker = new Worker(workerScript, { type: "module" });
    worker.onmessage = ({ data }) =>
      data.text === undefined ? deliver(data) : sink[data.stream as "out" | "err"](data.text);
    worker.onerror = () => {
      worker.terminate();
      deliver({ lost: true });
    };
    worker.postMessage({ hdWorker: module });
    return {
      post: (message: Dynamic, frame?: Uint8Array) => post(worker, message, frame),
      stop: () => worker.terminate(),
    };
  });
  return { send: workers.send, close: () => workers.current()?.stop() };
}

// A worker of `browserWorkers`, in `scope`: it runs `hd.init`, then the
// root `hd.worker` whenever frames are waiting. A worker cannot block
// without `SharedArrayBuffer`, so `WorkerPort.next` gives `.None` once no
// frame is left, `worker_serve` returns, and the next message runs it
// again. Its output, its replies and its end go to the parent.
// Cost: one run of the root per batch of frames that came together.
export function serveWorker(scope: Dynamic, module: WebAssembly.Module): void {
  const frames = new Frames();
  const host = createWebHost(
    {
      out: (text) => post(scope, { stream: "out", text }),
      err: (text) => post(scope, { stream: "err", text }),
    },
    {
      module,
      workerPort: {
        next: () => frames.take(),
        reply: (frame: Uint8Array) => post(scope, { id: frames.taken, frame }, frame),
      },
    },
  );
  const instance = WebAssembly.instantiate(module, host.imports);
  let init: string | null = "hd.init";
  let running = false;
  const end = (): void => {
    host.close();
    post(scope, { lost: true });
    scope.close();
  };
  scope.onmessage = async ({ data }: MessageEvent) => {
    frames.add(data);
    if (running) {
      return;
    }
    running = true;
    try {
      while (frames.ready()) {
        const { status, trapped } = await host.run(await instance, init, "hd.worker");
        init = null;
        if (trapped || status !== 0) {
          return end();
        }
      }
    } catch {
      return end();
    }
    running = false;
  };
}

const webLayer: Dynamic = {
  providers: ({ env, st, sink }: { env: CoreEnv; st: Dynamic; sink: Sink }) => {
    const workers = browserWorkers(env, sink);
    return {
      traits: {
        "hd:Http": httpProvider(browserSend),
        // The temp directory of a case, a path in the run's in-memory files
        // (`memoryFiles`); the core has the other hooks.
        "hd:TestRunner": {
          temp_dir: () => {
            st.tempMade = true;
            return env.tempDir ?? "/tmp";
          },
        },
        ...(workers && { "hd:Workers": { send: workers.send } }),
      },
      close: () => workers?.close(),
    };
  },
};

// One host per instance in the browser; `sink` and `env` are
// `createCoreHost`'s. A test run passes its `memoryFiles()` as `env.files`.
export const createWebHost = (
  sink: Sink,
  env: CoreEnv & { tempDir?: string | null; files?: TestFiles } = {},
): Host => createCoreHost(sink, env, webLayer);

// A REPL session in the browser (core.ts `createSession`), over the same
// providers as a run.
export const createWebSession = (
  sink: Sink,
  env: CoreEnv = {},
  options: { journalCap?: number } = {},
): Session => createSession(sink, env, webLayer, options);

// The files of one test run in the browser (cases.ts): kept in memory and
// gone with the run, since the playground has no disk
// (crates/hd_web/DESIGN.md §5).
export function memoryFiles(): TestFiles {
  const texts = new Map<string, string>();
  return {
    join: (dir: string, name: string) => `${dir}/${name}`,
    read: (path: string) => texts.get(path) ?? null,
    write: (path: string, text: string) => {
      texts.set(path, text);
    },
    remove: (dir: string) => {
      for (const path of texts.keys()) {
        if (path === dir || path.startsWith(`${dir}/`)) {
          texts.delete(path);
        }
      }
    },
  };
}

// The traits a compiled module imports that `host` does not bind, by name
// and in import order: `hd:FsRead` gives `FsRead`. A runner refuses such a
// module before it instantiates it, as `hd_run::check_grants` refuses a
// total deny.
export function unboundTraits(module: WebAssembly.Module, host: Host): string[] {
  const names = WebAssembly.Module.imports(module)
    .map(({ module }) => module)
    .filter((name) => name.startsWith("hd:") && !Object.hasOwn(host.imports, name))
    .map((name) => name.slice(3));
  return [...new Set(names)];
}
