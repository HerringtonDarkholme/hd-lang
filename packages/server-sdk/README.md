# Server SDK

An application owns its ordinary `main!`, one state object and a joined
group of workers. `server.HttpServer` is an SDK-owned package capability;
requests and responses use `std.http` values.

The implementation supplies the typed source, joined workers, bounded
request cleanup and a loopback Node listener. A request races its source
cancellation signal against a handler wrapped in `timeout!(20s, cleanup=100ms)`.
The source supplies its cleanup clock, so `serve!` adds no ambient Clock row.
Every accepted lease is released after response delivery or cleanup.
The public JavaScript adapters supply retained synchronous turns,
lease-owned outgoing I/O, admission deadlines, request logs and Node worker
supervision. The SDK also prepares Cloudflare Worker output through an
ordinary deployment task. A second task prepares supervised Vercel Node
functions. Hosted deployment acceptance remains unproven.

From the repository root, build `cargo build -p hd_cli --features wasmtime`.
Then run `hd run` from `examples/server-api`, granting the SDK trait in the
application's manifest. The sample accepts a JSON object with a `message`
string; malformed JSON returns 400. `POST /slow` suspends for 150 ms and
demonstrates overlapping workers. `HD_SERVER_PORT` selects the loopback
port (8080 by default, 0 for an ephemeral test port). SIGTERM closes
admission and joins workers after their admitted requests finish.

Focused checks, with scratch `HD_CACHE`:

```sh
cd packages/server-sdk
hd check --all
hd test --engine node
hd test --engine wasmtime
```

The library tests bind an ordinary HD provider. The live Node check uses
the actual sample and package-provider boundary:

```sh
node --test packages/server-sdk/test/*.test.mjs
```

`HD_SDK_TEST_HD` can name a different compiled CLI. No test needs a public
listener, external network service or deployment credentials.

## Adapter Boundary

`adapters/application.mjs` exports `createApplication({ module, bindings,
createCoreHost, grants, sink, log })`. The caller supplies the built Wasm module,
its signature-generated `.host.mjs` exports, the public core host factory,
an explicit capability map and an output sink or private log observer. No compiler-cache
path is an SDK interface.

`dispatch(request, signal)` takes the plain `std.http.Request` JavaScript
value and resolves to `{ response, cancelled, abandoned }` after guest
cleanup. An aborted signal preserves the owned `Deadline` or `Stopping`
reason; other abort reasons become `Disconnected`. The platform caller
constructs its native Response afterward.
`stop()` closes admission and joins the retained main after admitted jobs;
`fail(error)` closes the poisoned host and rejects all active dispatches.

`adapters/dispatcher.mjs` provides the lower-level lease scheduler. A native
operation's start receives resolve/reject callbacks and returns optional
disposal. Both run in its lease's `run()` coroutine, even when another
request caused the guest poll. Late callbacks are ignored after disposal.
Shared state stores readiness and owner wake callbacks, without retaining a
global request Promise. This boundary is usable by subsequent fetch and
platform adapters; it does not yet supply their complete policies.

## Outgoing Fetch Transport

`adapters/http.mjs` exports `createHttpClient(lease, options)`. It takes a
resolved core grant plus the public `isAsciiUrl` and `writtenHost` helpers;
its returned `send(request, context)` method uses `lease.operation` to
schedule native I/O. The optional fetch implementation supports platform
adapters and deterministic controls. The provider timeout defaults to
thirty seconds, covering redirects and body reading.

The transport validates grants before each manual redirect hop and strips
credentials on cross-origin redirects. Responses preserve raw body bytes,
non-success statuses and separate Set-Cookie values. Non-cookie headers
may be combined by the platform according to RFC 9110 §5.3. The serving loop
binds each request's source client and source clock around its handler and
cleanup. Returned handles expire when the lease finishes;
they cannot start a later request. The Node provider and Application adapter
use the core runtime's narrow `context.http` policy view, so explicit denial
and host scopes are checked before native I/O and on every redirect.
Admission limits and deployment packaging remain separate adapter layers.

## Fetch Mapping And Cleanup Backstop

`adapters/fetch.mjs` exports `createFetchAdapter(options)` for one retained
Application, or `createFetchHandler(application, options)` for an existing
Application or Node supervisor. Both consume standard Fetch requests and
return standard Fetch responses. Runtime and generated bindings remain
explicit inputs.

The mapper reserves at most eight requests, including body reading, with a
one-MiB body limit and 64-KiB header limit on either side. A ninth request
gets 503 and Retry-After: 1. Its twenty-second deadline includes body reading.
Native stream cancellation runs in the request's own continuation.

The SDK's cooperative cleanup budget remains **100 ms from cleanup start**.
The adapter's separate watchdog fires at **200 ms from cancellation**, only
if no cleanup acknowledgement has arrived. A normal abandoned-cleanup
acknowledgement around 105 ms keeps the instance healthy. Platform CPU
limits remain independent; a blocked isolate needs an external supervisor.

## Node Worker Supervision

`adapters/supervisor.mjs` exports `createSupervisor({ program, policy, event,
log, exhausted })`. `program` supplies a compiled WebAssembly.Module, explicit
runtime/bindings/Application module URLs, and resolved grants. Each worker
retains its own application state and accepts one active request at a time.
`createFetchHandler(supervisor)` keeps physical response ownership outside
the interruptible worker.

The cleanup watchdog is fixed at 200 ms and runs on the supervisor's
thread. Startup, request, drain, outer-stop, restart-count/window, and
Retry-After values are required caller inputs. An outer-stop budget below
200 ms is rejected, so it cannot truncate the cleanup contract. First stop
drains, second stop cancels, and stop/close completion joins termination.
Forced termination reports cleanup as unconfirmed.

`Application.start()` runs startup eagerly. Optional `onReady` and
`onStopped` observers report a completed turn parked on request acceptance
and stopped main completion, respectively. A main that never yields cannot
announce readiness.

These are explicit embedding components. The development `hd run` provider
does not automatically select the worker supervisor. Aggregate memory
acceptance remains part of each platform's release checks.

## Application Logs

The serving loop binds a request-owned `Console` beside its Http and Clock
handles. Handler and cleanup writes keep that lease's identity even when
another request causes the guest turn. Console denial returns `NotGranted`
before capture; a retained console rejects writes after its lease ends.

`log(record)` receives `{ version: 1, lease, stream, text }`. The stream is
`out` or `err`; lease generation and sequence are decimal strings. Startup
and unassociated runtime diagnostics in Application use `lease: null`. Without this
observer, Application forwards text through its ordinary sink.

Supervision adds `worker: { slot, generation }` and `request`, the physical
request ID, or null for unassociated output. Its log callback is separate
from lifecycle `event` callbacks. Both are synchronous observers; return
values are not awaited, and throwing does not own response or cleanup.
The SDK adds no headers, bodies or environment values to metadata, and
never copies private log text into generic HTTP errors.

The development Node provider writes request-console JSON records to their
corresponding standard streams; ambient CLI output keeps its ordinary Console
format. This does not add an aggregate memory
guarantee or select deployment budgets.

## Deploy-Time Production Grants

`packaging/grants.mjs` is the shared manifest reader for the SDK deployment tasks. `readProductionGrants(rootDirectory)` reads exactly that root's `hd.toml` on each call. `parseProductionGrants(text)` provides the same validation for an already-read manifest. Both preserve explicit true, false and scope lists, including package-capability consent; neither imports development grants or supplies missing grants.

The reader accepts TOML quoted keys, multiline arrays and ordinary escaping through a pinned, vendored parser. It rejects malformed TOML, invalid grant shapes and non-UTF-8 input. Error text omits source excerpts and grant values. Scope paths keep their manifest spelling for the consuming platform adapter to resolve; this helper does not invent a platform filesystem.

The policy conformance inputs are in `test/fixtures/deployment-grants.json`, executed by `node --test packages/server-sdk/test/grants.test.mjs` from the repository root and by the CLI's maintained SDK test wrapper. `hd build` remains policy-free: there is no `NAME.host.json`, and deployment grants are not read from compiler caches or build artifacts. The Cloudflare task uses this reader before preparing its output.

## Portable Runtime Snapshot

The SDK carries the platform-neutral host graph and its ABI model in `runtime/runtime-sources.json`. Deployment tools use `packaging/portable-runtime.mjs` to validate this snapshot and emit it into a fresh task-owned staging directory. `readPortableRuntime()` returns the file texts and identity metadata; `emitPortableRuntime(directory)` also returns public module URLs for the emitted core and package-contract reader. Existing output is refused.

Snapshot source text uses LF independently of checkout line endings. Emitted files match those archived bytes.

The fingerprint covers emitted names and bytes, the package-host ABI version and the source base revision. The content hashes identify the actual source even when maintenance runs on an uncommitted host edit. These are integrity and provenance records, not signatures. The package-host ABI version is distinct from a claim that every compiler/runtime ABI is versioned. The Cloudflare task validates the built module and companion data before publishing its local output.

The installed SDK needs no compiler checkout, build companion or compiler cache to emit its runtime. The relocation control builds the sample, removes its HD source and SDK development provider, then uses the installed snapshot and production grants to serve counters 1, 2 and 3 and stop normally in a fresh Node process. Platform build validation remains part of each deploy task's acceptance.

Repository maintainers refresh the generated snapshot with `node packages/server-sdk/packaging/sync-runtime.mjs` when the portable host graph changes. The maintained SDK tests reject a snapshot that differs from the current host sources. Application deploy tasks consume the snapshot and never run this repository-maintenance command.

## Cloudflare Deployment Task

The sample includes `tasks/deploy-cloudflare.hd`, an ordinary task using
`dep.server_sdk.deployment.cloudflare!`. Grant its SDK capability for local
task execution in the application's manifest:

```toml
[dev-capabilities]
"dep.server_sdk.deployment.Deployment" = true
```

The installed SDK carries the JavaScript provider declared by its own
`[host]` table, its runtime snapshot and its expected server ABI data. The
task needs no compiler checkout or compiler-cache path.

From `examples/server-api`, prepare and validate the Worker locally:

```sh
hd build
hd run deploy-cloudflare --engine node
wrangler deploy --dry-run --config cloudflare/wrangler.json
```

Node runs the SDK's package-declared JavaScript provider. The task reads
`build/debug/server_api.wasm` and its `.host.mjs` companion
and writes `cloudflare/wrangler.json` with its complete payload. Artifact
paths in the task resolve from the application root, including when the
task is invoked from a nested directory. The task prepares files locally;
Wrangler performs any later upload separately.

Application permissions come from that root's production `[capabilities]`,
including explicit consent to `dep.server_sdk.server.HttpServer`. Development
consent to the Deployment task does not grant the deployed application's
capabilities. Explicit denial through any alias of the request source
rejects preparation. An imported primitive Clock or Random capability also
rejects preparation when production policy explicitly denies it.

For Env, set `CloudflareOptions.env_bindings` to the names the Worker should
receive and grant those names in production policy. Only selected string
bindings are exposed; configure their values through Cloudflare separately.
The task records required names without reading or embedding values.

Preparation checks the captured Wasm against the installed SDK's nominal
request-source identity and expected boundary ABI. It checks the companion's
contract header without executing its JavaScript body, then emits SDK-owned
bindings. These checks establish ABI compatibility and provenance; they do
not authenticate arbitrary compiler output.

The SDK owns the output marker, generation store and regular `wrangler.json`
selector. Repeated preparation selects a complete immutable generation and
retains previous generations for readers already using them. Unrelated root
files and Wrangler caches are preserved. Preparation refuses an unowned
output directory or modified SDK payload; other writers must leave the
reserved paths unchanged while preparation runs.

The maintained deployment, output, schema and binding tests run with the
other SDK JavaScript tests. Local workerd probes exercised startup Env
decoding, retained string bindings, outgoing Http grants, Clock and Random.
They also exercised short and fixed twenty-second timeouts with cleanup
and recovery.
The synchronous driver rejected a pending `block_on` before native fetch;
this profile does not use a JSPI adapter, even when the engine exposes JSPI.

Five fresh local workerd processes reached their first response in a median
40.183 ms from spawn, measured externally. A private instrumented copy used
10.9375 MiB across both guest linear memories after eight overlapping
one-MiB requests. The [design record](../../docs/future-work/SERVER_APPLICATIONS.md#cost-and-measurements)
records the probe versions, measurements and limits. These probes establish
local behavior, not hosted acceptance, CPU startup or total isolate memory.

## Vercel Node Deployment Task

The sample's `tasks/deploy-vercel.hd` uses
`dep.server_sdk.deployment.vercel!` through the same Deployment capability
as the Cloudflare task. Keep its development capability consent and the
application's separate production grants in the root manifest.

From `examples/server-api`, prepare the Node function locally:

```sh
hd build
hd run deploy-vercel --engine node
```

The task reads the built Wasm and binding companion and writes
`.vercel/output/config.json` with an immutable function generation. Vercel
uploads are a separate step using its ordinary prebuilt-output workflow.
The task performs no upload and requires no credentials. Its generated
function declares `nodejs24.x`, a [supported Vercel Node version](https://vercel.com/docs/functions/runtimes/node-js/node-js-versions).

`VercelOptions` selects the function name, `Artifacts`, explicit
`SupervisorPolicy`, and named `env_bindings`. The policy supplies worker
count, startup, drain, outer-stop, restart count/window and Retry-After.
Retry-After must be nonempty printable ASCII.
Choose one to eight workers; each retains its own application state and
accepts one active request. The request budget stays twenty seconds, with
the existing hundred-millisecond SDK cleanup and two-hundred-millisecond
host backstop.

Unavailable worker capacity returns 503 with the selected Retry-After
value, including during startup. Native Node streams feed the SDK's
bounded Fetch mapper, preserving request cancellation and separate
Set-Cookie values. Whole-body limits remain one MiB in either direction.

Production source consent, SDK identity, ABI and companion validation use
the same preparation checks as Cloudflare. Only explicitly selected
string environment bindings reach the Env provider, subject to production
grants. Their values are captured at Node entry initialization rather than
embedded in generated output.

Repeated preparation atomically changes `config.json` to select a complete
immutable function under `functions/`. Earlier generations remain available
to existing readers; unrelated root files and caches are preserved. An
unowned output directory, changed SDK payload, or unknown function directory
is refused. Other writers must leave the SDK's reserved paths unchanged
during publication.

Local controls use the official `@vercel/node` launcher and build-utils ZIP
builder, exercising retained requests, overlapping work, body limits and
production policy. The generated route uses a documented `request.path`
transform to pass the matched pathname to the Node runtime. Local launcher
controls bypass deployment-wide routing and do not establish hosted
build/deployment acceptance. Malformed native authority,
targets and headers receive 400 before guest dispatch.

Vercel Edge is outside this SDK's scope because Vercel deprecated standalone
[Edge Functions](https://vercel.com/docs/functions/runtimes/edge/edge-functions.rsc);
a later request requires its own issue.
