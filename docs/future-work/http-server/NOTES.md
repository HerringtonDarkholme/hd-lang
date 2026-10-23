# HTTP Request Handler: Research Notes (WIP)

Status: work in progress, 2026-10-23; research notes for
[#266](https://github.com/HerringtonDarkholme/hd-lang/issues/266), not a
design and not accepted. The lane stopped here when #266 moved to another
lane. Nothing here has been parsed or checked yet.

## What hd has today

- `std.http` ([spec/std/http.md](../../../spec/std/http.md),
  `lib/std/http.hd`): `Request` and `Response` are plain structs with all
  fields `pub`: method, url, query, headers as `List[(string, string)]`,
  body as `List[u8]`, and redirects. Both are already boundary types: their
  wire encoding is in crates/hd_run_wasmtime/DESIGN.md §17.4. Bodies are
  whole (`std-http.bodies`), so a handler can reuse both types as they are.
- Entry exports (crates/hd_wasm_linear/src/rt.rs `entry`): `hd.init`,
  `hd.poll(ctx)`, `hd.wake(n)`, `hd.cancel`, `hd.abandon`, `hd.x`. The
  runtime has **one root per instance** (`Global::Root`, `Global::Owner`
  in `task::root_poll`). Running several requests at once in one instance
  needs a root table. That is the main compiler gap.
- JS host (`host/core.mjs`): an async driver (`run`) that polls, waits
  with `await` and calls `hd.wake`. Its pending operations (`Later`,
  `pendingOperation`) already have poll and abort hooks. `hd:rt block` is
  synchronous: it uses `Atomics.wait` or a busy loop.
- `Http` on Node (`host/http.mjs`): a worker thread with start, poll and
  abort messages. Workers and the Edge runtime have no worker threads, so
  they need a provider over the platform `fetch` with `AbortController`.
  That is the start-and-poll design of DESIGN.md §17.6, which `host/web.mjs`
  does not implement today.
- Language rules that apply: registration
  ([spec/lang/10 Registration](../../../spec/lang/10-modules.md#registration)),
  the boundary-safe values in the same chapter, `module.entry.row.host`,
  and the entry driver and its cancellation, `req.entry.*` in
  [spec/lang/11](../../../spec/lang/11-requirements-and-suspension.md#entry-driver).
  Grants are in spec/cli Host Capabilities; `cli.cap.build` says a built
  module carries no grant.
- [HOST_EMBEDDING.md](../HOST_EMBEDDING.md) (#51, deferred) proposes one
  active call per instance. That conflicts with a server, which runs
  requests at once in one isolate.

## Platform facts gathered (fetched 2026-10-23)

| Platform | Facts |
| --- | --- |
| Cloudflare Workers ([limits](https://developers.cloudflare.com/workers/platform/limits/)) | 64 MiB uncompressed Worker size (compressed: no limit stated); 1 s startup; 128 MB memory per isolate, Wasm included; CPU per request 10 ms on Free, 30 s by default on Paid (up to 5 min, `limits.cpu_ms`); no wall-clock limit while the client stays connected; `waitUntil` +30 s; 6 connections at once waiting on headers; 50 subrequests on Free, 10,000 on Paid. |
| Workers Wasm ([webassembly](https://developers.cloudflare.com/workers/runtime-apis/webassembly/), [javascript](https://developers.cloudflare.com/workers/runtime-apis/webassembly/javascript/)) | `import mod from "./x.wasm"` then `WebAssembly.instantiate(mod, imports)`; only precompiled modules, no compile from bytes; no threads; instantiate at global scope, not once per request. |
| Workers fetch handler ([fetch](https://developers.cloudflare.com/workers/runtime-apis/handlers/fetch/)) | `export default { async fetch(request, env, ctx) }`; `ctx.waitUntil`, `ctx.passThroughOnException`; `request.signal` aborts on client disconnect behind the `enable_request_signal` flag ([changelog](https://developers.cloudflare.com/changelog/2025-05-22-handle-request-cancellation/)). |
| Workers errors ([errors](https://developers.cloudflare.com/workers/observability/errors/)) | 1101 "Worker threw a JavaScript exception"; 1102 "exceeded CPU time limit". |
| Vercel Functions ([limits](https://vercel.com/docs/functions/limitations)) | Fluid compute; 250 MB uncompressed bundle; 2 GB memory (Pro up to 4 GB); max duration 300 s by default (Pro up to 800 s), then 504 `FUNCTION_INVOCATION_TIMEOUT`; 4.5 MB request or response body, else 413. |
| Vercel Node runtime ([node-js](https://vercel.com/docs/functions/runtimes/node-js)) | `export default { fetch(request) }` with Web `Request` and `Response`, or `export function GET`; cancellation is enabled per route. |
| Vercel Edge ([edge](https://vercel.com/docs/functions/runtimes/edge)) | Vercel recommends migrating to Node.js, and Next.js 16.3 drops `runtime = 'edge'`; code size after gzip 1 MB (Hobby), 2 MB (Pro), 4 MB (Enterprise); a response must start within 25 s; Wasm only through `import`, and `WebAssembly.compile` or `instantiate` from bytes is disabled; `SharedArrayBuffer` and `Atomics` are listed. |

Still to research: Spin, wasmCloud, Fastly Compute (one instance per
request), and the Rust `worker` crate (panics leave its shared instance
unusable). Also still to check: whether workerd and Vercel Edge support
JSPI, which `block_on` needs.

## Current leaning (not reviewed)

1. **Entry.** A `pub fn handle!(request: Request) -> Response $ R` in an
   executable's entry module, selected by a `kind = "handler"` key on its
   `[[executable]]` table, so the manifest names the module and never the
   function (`cli.exe.table`). This is registration of one entry kind
   under `module.register.*`, with no new syntax. Alternatives to weigh: a
   std function that registers the handler from `main`, or a
   `@handler` annotation. Each needs owner approval.
2. **Exports.** `hd.serve.start(len) -> root`, with the encoded `Request`
   in `hd.x`; `hd.serve.poll(root) -> status`; `hd.serve.cancel(root)` and
   `abandon(root)`; and the encoded `Response` at completion. The runtime
   needs a root table in place of the one `Global::Root`. The host keeps
   a map from handle to root, filled while it polls each root, so a wake
   polls only the root that owns the handle.
3. **Concurrency.** One instance per isolate, with many request roots
   driven cooperatively (`req.schedule.one-thread`). Top-level bindings
   are the state that lives across requests. A panic poisons the instance:
   the glue answers 500 to every in-flight request, logs the report, and
   instantiates again. The other option, one instance per request as
   Fastly does, is simpler and isolates panics, but costs memory under the
   128 MB isolate limit and an instantiation per request.
4. **Hosts.** One `host/serve.mjs` core over `core.mjs` with a `fetch`
   provider for `Http` (start, then a pending handle, with abort through
   `AbortController`). Three thin glues over it: a `node:http` server for
   `hd run`, a Workers `export default { fetch }`, and a Vercel
   `export default { fetch }` (Node or Edge). `hd build --target
   cloudflare|vercel` would write `build/<target>/` with
   `worker.mjs`/`api/index.mjs`, the `.wasm` file, and a
   `wrangler.toml` or `vercel.json` stub. The target flag needs approval.
5. **Bodies.** Whole bodies in v1. Streaming is not required for a JSON
   API, and Vercel caps a whole body at 4.5 MB anyway. It waits for
   STREAMING_DESIGN.md.
6. **Grants.** A grant is compiled into the glue from `[capabilities]`,
   since `cli.cap.build` writes none into the `.wasm`. On a platform,
   `Console` maps to `console.log`, `Clock` to timers, `Http` to `fetch`
   checked against the grant, and `Env` to `env` or `process.env`.
   `FsRead`, `FsWrite`, `Process`, `Net` and `Sys` are refused at build
   time.
7. **Timeouts.** `std.task.with_timeout!` inside the handler gives a 504
   chosen by the handler. A client disconnect, when the platform's signal
   reports it, cancels the root under a cleanup bound.

## What remains

The design document itself, under design-doc/SKILL.md; prior-art
research; prototypes (`hd check` of a handler, and a Node glue sketch
run against a built module); a bottom-up cost; a stress test; phasing; a
recommendation and its approvals; the self-review table.
