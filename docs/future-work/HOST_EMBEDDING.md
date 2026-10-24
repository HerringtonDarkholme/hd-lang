# Host Embedding

Status: package host capabilities implement the [owner's #51 decision](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6388965405).
General exported-function embedding and native library loading remain deferred.
The production HTTP SDK and deployment adapters follow in [#266](https://github.com/HerringtonDarkholme/hd-lang/issues/266), under [Server Applications](SERVER_APPLICATIONS.md).

## Contents

- [Package-Declared Host Capabilities](#package-declared-host-capabilities)
- [Purpose and existing pieces](#purpose-and-existing-pieces)
- [Other embedders](#other-embedders)
- [Choices and recommendation](#choices-and-recommendation)
- [Registration and providers](#registration-and-providers)
- [Calls and wire values](#calls-and-wire-values)
- [Panics and process status](#panics-and-process-status)
- [Delivery and checks](#delivery-and-checks)
- [Evidence](#evidence)

## Package-Declared Host Capabilities

An SDK owns an ordinary `@host` trait and maps it to a JavaScript provider in its package.
The app's root manifest grants consent to load that provider.
The provider default-exports plain methods; generated app glue handles the Wasm ABI.

### A Complete SDK And App

Create this `embedding-demo/` directory beside the checkout.
This deliberately small request source demonstrates the binding API; it is not the production HTTP API being built in #266.
Its Node provider delivers one body and ends the stream.

**`embedding-demo/sdk/hd.toml`**

```toml
[package]
name = "server_sdk"

[host]
"server.HttpServer" = "providers/http.mjs"
```

**`embedding-demo/sdk/src/lib.hd`**

```text
pub use pkg.server.{HttpServer}
```

**`embedding-demo/sdk/src/server.hd`**

```text
@host
pub trait HttpServer:
    fn next!(&mut self) -> string?
    fn respond!(&mut self, body: string) -> void
```

**`embedding-demo/sdk/providers/http.mjs`**

```js
const requests = ["hello"];
export default {
  async next() { return requests.shift() ?? null; },
  async respond(body) { console.log(body); },
};
```

**`embedding-demo/app/hd.toml`**

```toml
[package]
name = "embedding_app"

[dependencies]
server_sdk = { path = "../sdk" }

[capabilities]
"dep.server_sdk.server.HttpServer" = true
```

**`embedding-demo/app/src/main.hd`**

```text
use dep.server_sdk.server.HttpServer

pub fn main!() -> void $ HttpServer:
    server := $.use(HttpServer)
    while true:
        match server.next!():
            .Some(body) => server.respond!(body + "!")
            .None => return
```

Build the dev compiler in the isolated checkout, then run from the app directory:

```sh
cargo build -p hd_cli
embedding_hd="$(pwd)/target/debug/hd"
cd ../embedding-demo/app
"$embedding_hd" check
"$embedding_hd" run --engine node
"$embedding_hd" build --engine node
"$embedding_hd" build/debug/embedding_app.wasm --engine node
```

Both runs print `hello!` and exit successfully.
The build writes `build/debug/embedding_app.wasm` and `embedding_app.host.mjs`.
The SDK provider imports neither file and survives changes to the app's output name or build directory.
A prebuilt run discovers the nearest package manifest above the Wasm file, independently of the caller's working directory.

### The Platform Adapter Call

An edge adapter supplies the same plain methods through the generated binding module.
For this example, copy the platform-neutral runtime from the checkout into `embedding-demo/runtime/`:

```sh
mkdir -p ../embedding-demo/runtime/gen
cp host/core.mjs host/checked-report.mjs host/float.mjs host/journal.mjs \
   host/wire.mjs host/package-providers.mjs ../embedding-demo/runtime/
cp host/gen/abi.mjs ../embedding-demo/runtime/gen/
```

The following bridge keeps one ordinary `main!` alive across requests.
Its `appCapabilities` argument is the app's root table, copied by the trusted adapter packaging step.
It checks consent before constructing the provider and binds by the generated nominal module key.

**`embedding-demo/edge.mjs`**

```js
import { createCoreHost } from "./runtime/core.mjs";

// This small example has one queue and one ordinary HD main per instance.
// #266 supplies production request lifetimes, limits and deployment tooling.
export async function createBridge(module, bindings, appCapabilities) {
  const root = bindings.interfaces.find(i => !i.handle && i.identity.trait_path === "server.HttpServer");
  if (!root) throw new Error("missing server.HttpServer interface");
  const granted = root.selectors.find(path => appCapabilities[path] === true);
  if (!granted || root.selectors.some(path => appCapabilities[path] === false))
    throw new Error("server.HttpServer needs root manifest consent");
  const queue = [];
  let waiting = null, current = null, closed = false, failure = null;
  const take = () => {
    if (queue.length) { current = queue.shift(); return current.body; }
    return null;
  };
  const provider = {
    next(context) {
      if (queue.length || closed) return take();
      return new Promise((resolve, reject) => {
        const abort = () => { waiting = null; reject(new Error("server stopped")); };
        context.signal.addEventListener("abort", abort, { once: true });
        waiting = () => {
          waiting = null;
          context.signal.removeEventListener("abort", abort);
          resolve(take());
        };
      });
    },
    respond(body) {
      if (!current) throw new Error("response without a request");
      current.resolve(new Response(body));
      current = null;
    },
  };
  const host = createCoreHost({ out() {}, err(text) { throw new Error(text); } }, {
    module,
    grants: { [granted]: true },
    packageBindings: runtime => bindings.bind(runtime, new Map([[root.module, provider]])),
  });
  const instance = await WebAssembly.instantiate(module, host.imports);
  const done = host.run(instance, "hd.init", "hd.poll").then(result => {
    if (result.status !== 0) throw new Error(`HD status ${result.status}`);
  }).catch(error => {
    failure = error;
    current?.reject(error);
    for (const item of queue.splice(0)) item.reject(error);
    throw error;
  });
  // Retain a rejection handler even before the invoker calls close().
  done.catch(() => {});
  return {
    async fetch(request) {
      const body = await request.text();
      if (failure) throw failure;
      if (closed) throw new Error("server closed");
      return new Promise((resolve, reject) => {
        queue.push({ body, resolve, reject });
        waiting?.();
      });
    },
    async close() {
      closed = true;
      waiting?.();
      try { await done; } finally { host.close(); }
    },
  };
}
```

Vercel's [Web Handler](https://vercel.com/docs/functions/functions-api-reference) receives a web `Request` and returns a `Response`.
A Node function loads the built module once and calls the bridge:

**`embedding-demo/api/index.mjs`**

```js
import { readFile } from "node:fs/promises";
import * as bindings from "../app/build/debug/embedding_app.host.mjs";
import { createBridge } from "../edge.mjs";
const appCapabilities = { "dep.server_sdk.server.HttpServer": true };
const ready = WebAssembly.compile(await readFile(
  new URL("../app/build/debug/embedding_app.wasm", import.meta.url),
)).then(module => createBridge(module, bindings, appCapabilities));
export default { async fetch(request) { return (await ready).fetch(request); } };
```

Cloudflare/workerd supplies a compiled Wasm module through its [Wasm module import](https://developers.cloudflare.com/workers/runtime-apis/webassembly/javascript/).
Its adapter makes the same checked binding call inside the request-scoped poll driver:

**`embedding-demo/workerd-binding.mjs`**

```js
import module from "./app/build/debug/embedding_app.wasm";
import * as bindings from "./app/build/debug/embedding_app.host.mjs";
import { createCoreHost } from "./runtime/core.mjs";
const appCapabilities = { "dep.server_sdk.server.HttpServer": true };
export function bindServer(provider, sink) {
  const root = bindings.interfaces.find(i => !i.handle &&
    i.identity.trait_path === "server.HttpServer");
  const path = root.selectors.find(path => appCapabilities[path] === true);
  if (!path || root.selectors.some(path => appCapabilities[path] === false))
    throw new Error("server.HttpServer needs root manifest consent");
  return createCoreHost(sink, {
    module, grants: { [path]: true },
    packageBindings: runtime => bindings.bind(runtime,
      new Map([[root.module, provider]])),
  });
}
```

This is the workerd binding module, not a complete fetch driver.
Do not retain the Node bridge's global `host.run` Promise across workerd requests: the earlier real workerd control rejects cross-request I/O lifetimes.
#266's [request-scoped dispatcher](SERVER_APPLICATIONS.md#architecture) drives poll turns and owns outgoing I/O in each platform request's coroutine.
That dispatcher consumes `bindServer(provider, sink)` and preserves one ordinary HD main and instance.

The handwritten capability constant mirrors the complete app table above; production packaging must derive it from that manifest.
The bridge is a tested API example, not a deployed server adapter.
#266 adds request identity, disconnection, deadlines, body limits, streaming, platform lifetime constraints, packaging and deployment checks.
Those cannot be inferred from a successful local Request/Response call.

### Common Mistakes

| Mistake | Result and correction |
| --- | --- |
| Omit the root `true` grant | Startup names the package/version/trait and asks for its `[capabilities]` entry; no provider module loads. |
| Set the grant to `false` | Capability denial before import; remove the denial only if the app should load this provider. |
| Pass `--cap PATH=true` without manifest consent | Startup still refuses; add the root manifest entry. |
| Misspell or omit the mapped provider file | Startup reports the provider path and declaring manifest line; fix the SDK map. |
| Map a file outside the SDK, including a symlink escape | Startup refuses canonical containment; put the provider inside the declaring package. |
| Add `@host` without a map entry, or map an unmarked trait | `invalid-manifest` identifies the disagreement; make source and map match. |
| Export a class instance or inherit required root methods | Binding refuses; default-export a plain object with own methods. |
| Import app-generated glue from SDK provider code | Remove that dependency; the invoker wraps the plain object with the app's generated glue. |
| Run a required package capability on Wasmtime | Refusal before guest initialization names the capability and JavaScript-host requirement. |
| Run prebuilt Wasm without an app manifest above it | Precise startup refusal; retain the artifact inside its app package. |

### Goal, Existing Mechanisms And Options

The decided surface is `[host] "server.HttpServer" = "providers/http.mjs"`, paired exactly with bare `@host` on an owned public trait.
`traits = [...]`, `javascript = ...`, `--host` and `defineProvider` are superseded.
Library manifests never grant root consent.
An app continues using ordinary rows, `$.use` and `$.with` fakes.

### Identity And Contract

A fetched capability's identity contains its canonical package host path, exact selected version and package-relative trait path.
A local package uses its canonical-directory hash; moving the directory changes that identity.
`hd:pkg:<hash>` namespaces imports, while the custom section retains the complete identity and method schemas for validation.
A final trait name alone never selects a provider.

Only a reachable package host requirement emits an `hd.capabilities` section and generated companion.
Built-in-only programs retain their import names and emit no package companion.
The generated glue and Wasm contract must agree byte for byte.

### Binding, Startup And Authority

Node selects provider files from the declaring SDK's map only after validating every required root manifest grant.
It resolves and checks all files before importing any provider.
Provider JavaScript has its realm's ambient authority; HD grants do not sandbox its imports.
An adapter is trusted code responsible for honoring the same root consent and its own resource restrictions.

An unused declaration needs no provider load.
A host trait replaced by an ordinary HD fake needs no host provider for those replaced calls.
Registering HttpServer grants no implicit Net, Http, Process or filesystem access to the guest.

### Pending Calls And Request Context

A suspending method returns either a ready value or a Promise.
Its final argument supplies `signal` for the operation and `instanceSignal` for the instance.
Aborting discards undelivered results and signals cancellation; a later Promise completion cannot revive a consumed operation ID.
The provider remains responsible for stopping its external work.

### Generated Glue For JS Hosts

`crates/hd_host_abi/src/javascript.rs` generates `bind(runtime, implementations)` from checked signatures.
`implementations` is a `Map` from generated interface module keys to plain method objects.
Generated wrappers pin nominal identity and the full checked schema; provider files import no application artifact.
Returned handles may have prototype methods, while root provider methods must be own functions.

| HD value | Provider value |
| --- | --- |
| Structural integers through 32 bits | `Number` |
| Structural 64-bit integers | `BigInt` |
| Float | `Number` |
| String / bytes | string / `Uint8Array` |
| List / tuple / public record | array / array / named object |
| Enum | `{ tag: "Variant", fields: { ... } }` |
| Result | `{ ok: true, value }` or `{ ok: false, error }` |
| Optional | `null` or its value |
| Private-field type | The JS value chosen by its directional Serde implementation; Serde integers are `BigInt` and floats are `Number`. |

Private values use an internal flat typed transport, preserving integer range, float kind and finite float bits without exposing transport text.
A custom serializer that writes a string delivers that string; it does not expose the private record's layout.
The builtin hosts retain their existing boundary codecs.

A handle's final HD release removes its instance-local registry binding through `$release`.
Resource cleanup remains an explicit operation, such as `dispose!`; release does not call arbitrary resource methods.

### Wasmtime Refusal And Deferred Native Loading

The native host refuses required package capabilities before entering the guest.
There is no engine fallback.
Dylib loading, native provider registration and general exported functions remain deferred.

### A GUI-Shaped Current-Compiler Control

The focused event-handle fixture supplies a Surface with rendering and an explicitly disposable event handle.
It exercises returned handles and suspension without introducing GUI or DOM behavior into std.
GUI application design remains parked.

### Data Flow, Budget And Tests

```text
SDK source + provider map -> checked nominal contract + generated bindings
root app consent + selected manifests -> checked provider paths -> plain JS methods
HD values <-> generated codecs <-> JS values
```

The manifest parser records each mapping's line; paired source checks survive cached builds.
Monomorphization collects private package codecs only for reachable package boundaries, including inherited methods and returned handles.
The existing builtin/private codecs and package codecs occupy separate entries, so either may use the same private HD type.
Emission uses the existing profile and exchange-buffer machinery.

Tests cover declarations, aliases, exact versions, plain providers, missing/denied consent, ready/Pending calls, abort, late completion and handle release.
Private round trips include integer extrema, float `1.0`, negative zero, nested data and custom serialization.
A no-package-capability control must remain byte-identical to the same source built on the integration main baseline.

### One-Hour Slices And Exact Scope

The #51 revision covers manifest maps and source checks, root consent and selection, generated wrapping, private values, and worked examples.
Only the explicitly approved linear backend files are edited; no emitter, checker, TIR or std implementation changes belong here.
HTTP implementation follows [Server Applications](SERVER_APPLICATIONS.md), slices 1–8, with #827 when required grants first become necessary.

### Recommendation, Approvals And Evidence

The [confirmed review](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6388965405) selects the map, root consent, plain objects and unified JS values.
[Prebuilt discovery](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6389198731) follows the artifact's nearest app manifest.
The worked SDK/app checks, source run, build and prebuilt run succeed locally.
The bridge serves `one!` and `two!` through one live instance; hosted deployment is not claimed.

## Purpose and existing pieces

The broader embedding proposal remains deferred.
The current implementation reuses ordinary main, requirement profiles and the existing [Wasm boundary](../../spec/lang/10-modules.md#wasm-boundary).
It does not expose arbitrary HD functions as host-callable exports.

## Other embedders

The [Component Model](https://component-model.bytecodealliance.org/) remains a separate integration option.
The owner's first-release decision uses HD's own core-Wasm imports.
No comparison here changes that decision.

## Choices and recommendation

Package-declared capabilities are the current implementation target.
General registration, durable execution and native library loading are deferred rather than alternate ways to bind this SDK.

## Registration and providers

The package path, exact version and trait path form nominal identity.
Generated glue validates the Wasm contract; trusted adapters choose implementations and enforce root consent.
A general export registration API needs its own approved design.

## Calls and wire values

Package JS values are described above.
The language boundary still defines tree semantics, directional private-field serialization and poisoning failures.
No arbitrary HD address or closure crosses as a JavaScript handle.

## Panics and process status

A malformed provider result is a host-contract failure.
A poisoned instance must be discarded; adapters cannot resume it as a healthy request processor.
The existing CLI panic and cancellation policies remain in effect.

## Delivery and checks

#51 supplies the package binding mechanism.
#266 owns the production HTTP SDK, platform adapters and actual deploy checks.
Local Node Request/Response evidence is not proof of Vercel or workerd request lifetime behavior.

## Evidence

The implementation and review evidence are linked from [#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51).
Worked files and probe outputs are retained under `.cache-muse-51-review/worked/` in the isolated worktree.
The first proposed selector API is superseded by the confirmed review, not a second supported interface.
