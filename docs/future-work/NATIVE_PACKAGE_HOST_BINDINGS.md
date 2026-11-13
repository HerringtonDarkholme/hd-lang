# Native Package Host Bindings

Status: accepted design, 2026-11-02; domain: runtime and host; not built.
Tracked by [#1822](https://github.com/HerringtonDarkholme/hd-lang/issues/1822) (owner, 2026-11-13: a package already needs it).
Nothing here is in the specification until a spec pass applies it.
This reviews [Package Host Capabilities](../compiler/package-host-capabilities.md),
[`module.host.package.*`](../../spec/lang/10-modules.md#package-host-traits), and
[`cli.host.package.*`](../../spec/cli/command-line.md#package-host-bindings).

## Context

Package host traits already give JavaScript providers checked identities,
schemas, codecs, consent, cancellation, and handle release. Wasmtime instead
refuses every module that needs one before guest initialization.

The native engine already owns the linker, instance-local host state, reactor,
and poll driver. It lacks a typed way for an embedder to add package imports.

## Hdru Problem

Hdru keeps its UI protocol independent from each renderer. Its DOM and terminal
packages each own this renderer boundary:

```text
use dep.ui.{Command, UiEvent}

@host
pub trait Surface:
    fn apply!(&mut self, commands: List[Command]) -> void
    fn next_event!(&mut self) -> UiEvent
```

The same protocol can describe GPUI, but a Rust application cannot bind
`Surface`. Running a JavaScript provider under Node would separate the native
window state from the application that owns it.

## Proposal

`hd build --engine wasmtime` writes `NAME.host.rs` beside `NAME.wasm` when
package host traits are reachable, as it writes `NAME.host.mjs` for JavaScript;
a build with none emits no companion. The file holds one Rust trait per host
interface, its codecs and an app registry, all generated from the same checked
package-host contract. A native embedder implements the traits and passes the
registry by value when it instantiates the module through `hd_run_wasmtime`; hd
never discovers or loads a dynamic library. The Rust values mirror the
JavaScript mapping (Native Value Mapping, below), so both providers observe the
same hd values.

A GPUI renderer would implement generated Rust, then pass one checked registry
to the engine:

```rust
impl counter_host::Surface for GpuiSurface {
    fn apply(&mut self, commands: Vec<Command>) -> HostResult<()> { /* ... */ }
    fn next_event(&mut self, cx: CallContext<'_>) -> NativeCall<UiEvent> { /* ... */ }
}

let bindings = counter_host::Bindings::builder()
    .surface(GpuiSurface::new(window))
    .build()?;
let instance = engine.instantiate_with_bindings(&module, &setup, io, bindings)?;
```

The generated registry validates nominal identity and the complete method
schema before guest initialization. Registration selects native code; it does
not grant the guest authority. A provider runs only when both keys are present:
the app's `true` capability and the embedder's registration. Either one missing
refuses before any provider method runs.

## Problems It Solves

| Program | Today | With the proposal |
| --- | --- | --- |
| hdru with GPUI | Wasmtime refuses `Surface` before initialization. | GPUI implements generated `Surface` and owns the instance. |
| Native terminal renderer | The provider must be JavaScript under Node. | Crossterm or another Rust renderer binds directly. |
| Custom Wasmtime embedder | It must reproduce private codecs and import names by hand. | Generated codecs and registration use the embedded checked contract. |
| Stale renderer build | Raw imports can disagree after an hd package changes. | Startup compares identity and every reachable method before calling code. |

## How It Works

`hd build` already embeds `hd.capabilities`, including exact package identity,
trait path, import module, methods, suspension, and value schemas. Rust
generation consumes that contract rather than introducing WIT or another
interface description.

The companion defines owned Rust data and provider traits. Root providers live
in an instance-local registry. Returned handle traits live in the same registry;
the last hd release drops the Rust value without invoking a resource method.

`NativeCall<T>` has ready and pending forms. A pending call owns its completion
and abort hooks, reports readiness to the existing reactor, and ignores a late
completion after cancellation or instance close. Provider objects need not be
`Send`; only a completion sender crossing threads must be `Send`.

Each handle registry entry records its owner instance and generated handle
trait. Every handle argument checks both before dispatch. An unknown integer,
another instance's handle, or a handle of another trait is `host-contract`; the
provider method is not called.

The Rust generator lives in `hd_host_abi` behind a `native-rust` feature. Native
CLI builds enable it. `hd_web` leaves it disabled, so generator code cannot enter
the web compiler.

Instantiation performs one preflight:

1. Decode and validate the module contract.
2. Resolve only its reachable root interfaces against the generated registry.
3. Check root-manifest consent for every selected capability.
4. Compare nominal identity, method schemas, suspension, and codecs.
5. Add the package imports to the Wasmtime linker, then initialize the guest.

The native provider is linked into the trusted embedder and retains the
process's ambient authority. Hd grants constrain guest access and provider
selection; they do not sandbox Rust code.

### Native Value Mapping

| hd boundary value | Generated Rust value |
| --- | --- |
| `void`, `bool`, `char` | `()`, `bool`, `char` |
| Structural integers and floats | The same-width Rust scalar |
| `string`, bytes | `String`, `Vec<u8>` |
| List, tuple, public record | `Vec<T>`, tuple, named struct |
| Enum with named payload fields | Rust enum with the same variant and field names |
| Optional, result | `Option<T>`, `Result<T, E>` |
| Private-field type | Typed Serde tree preserving integer width and finite float bits |
| Returned host handle | Generated handle trait object in the instance registry |

Directional private serialization still decides the value. A private record
serialized as a string reaches Rust as a string, not as its hidden fields.

## Cost

| Component | Expected change and implementation gate |
| --- | --- |
| Code removed | Replace Wasmtime's blanket package-host refusal; keep refusal when bindings are absent. |
| Code added | Rust generator, native value codecs, registry, pending calls, and handle release. Count during implementation. |
| Compile time | Not measured; compare an hdru-shaped build with and without companion generation. |
| Runtime | One schema preflight; codec cost per call. Benchmark ready and pending renderer calls. |
| Wasm size | No intended change: the same imports and `hd.capabilities` section remain. Require byte identity. |
| Web compiler gzip | The `native-rust` generator feature is off in `hd_web`, so it adds no web bytes; the existing 2 MB gate verifies this. |

The design adds runtime and CLI mechanisms, not language syntax, semantics,
intrinsics, or std APIs.

## Appendix A: Stress Test

### Hdru's retained renderer

The current [rendering design](https://github.com/Eric-Song-Nop/hdru/blob/main/docs/rendering.md)
passes `List[Command]` into `apply!` and waits in `next_event!`. Commands include
enums with named payload fields, strings, numbers, optionals, and nested public
records. The proposed mappings preserve those names and shapes without JSON.

`apply!` returns ready. `next_event!` returns pending until GPUI reports an
event, then its completion sender wakes the existing driver. The provider stays
on the foreground thread, so a non-`Send` window handle is not rejected.

**Finding:** a Rust `Future + Send` requirement would exclude ordinary GUI
state. The binding therefore needs instance-local providers and a separate
thread-safe completion sender, rather than requiring the provider itself to be
`Send`.

### Two renderers in one app

Hdru applications may consent to both `ui-dom` and `ui-tui`, while each entry
reaches one renderer. Native preflight uses the built module's reachable
contract, not every manifest declaration. An unused consent entry neither
requires registration nor initializes a provider.

**Finding:** registration must be checked after reachability selection. Checking
all dependency maps would make a native GPUI entry require unrelated DOM code.

### Cancellation and late events

Close an instance while `next_event!` is pending, then deliver a queued mouse
event. Close invokes the pending abort hook once, releases its captured values,
and makes the late completion a no-op. It never wakes a new instance that reused
an integer handle.

**Finding:** a bare future result is insufficient. The native call form needs
instance identity, one-shot delivery, and an explicit abort path.

### Handles and private values

A provider returns two handles to the same native object, and hd releases them
in either order. Each binding owns its registry entry; the final release drops
the Rust object and calls no arbitrary method. A private Serde value round-trips
`i64` extrema, `-0.0`, bytes, and a custom string serializer.

A guest then supplies an integer it never received, a closed handle, and a
handle issued for another trait. The registry rejects each before dispatch by
checking the owner instance and generated trait type. Each mismatch is
`host-contract`, and integer reuse cannot reach a new entry.

**Finding:** `serde_json::Value` is not the boundary type. The existing typed
transport's integer and float distinctions must remain observable to codecs.
Raw integer handles are likewise insufficient without owner and trait checks.

## Appendix B: Prior Art

### Wasmtime component bindings

Wasmtime's [`bindgen!`](https://docs.wasmtime.dev/api/wasmtime/component/macro.bindgen.html)
generates host traits and an `add_to_linker` function from a WIT world:

```rust
impl HelloWorldImports for MyState {
    fn name(&mut self) -> String { self.name.clone() }
}

let mut linker = Linker::new(&engine);
HelloWorld::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
let bindings = HelloWorld::instantiate(&mut store, &component, &linker)?;
```

WASI Preview 2 similarly stores context and resources in the Wasmtime store,
then uses generated host traits and
[`add_to_linker_sync` or `add_to_linker_async`](https://docs.wasmtime.dev/api/wasmtime_wasi/p2/index.html).
Hd takes typed generated traits and explicit linker registration. It keeps its
existing core-Wasm ABI and checked contract instead of adding WIT.

### Tauri native providers

Tauri plugins register native Rust commands explicitly and expose them only
through declared permissions:

```rust
Builder::new("renderer")
    .invoke_handler(tauri::generate_handler![commands::apply])
```

```toml
[[permission]]
identifier = "allow-apply"
commands.allow = ["apply"]
```

The official [plugin guide](https://v2.tauri.app/develop/plugins/#adding-commands)
separates handler registration from
[command permission](https://v2.tauri.app/develop/plugins/#command-permissions).
Hd takes the same two-key shape: the embedder registers code, while the app
manifest grants guest access. Hd differs by generating from a Wasm contract,
not exposing commands to a webview IPC caller.

## Appendix C: Alternatives Considered

**Dynamic libraries in `[host]`.** This permits provider discovery, but adds
library search, signatures, platform ABIs, unloading, and a second consent
surface. A Rust application already owns its renderer and can pass it directly.

**Hand-written `wasmtime::Linker` imports.** This avoids generation but makes
every embedder reproduce unstable module names, exchange-buffer codecs,
suspension, and handle release. A schema mismatch would be found late.

**WIT and the component model.** This has excellent generated host tooling, but
would duplicate the checked hd trait schema or change the chosen core-Wasm
backend. It remains a separate embedding option.

**A JavaScript sidecar.** This is available today and proves the renderer
protocol, but it is not a native binding for GPUI, platform UI objects, or Rust
application state.

## Appendix D: Today In Hd

[`cli.host.package.javascript`](../../spec/cli/command-line.md#r-cli.host.package.javascript)
allows package providers only on JavaScript hosts.
[`cli.host.package.native-refusal`](../../spec/cli/command-line.md#r-cli.host.package.native-refusal)
requires Wasmtime to refuse before guest initialization.

`hd_run_wasmtime::WasmtimeEngine::load` performs that refusal whenever it sees
`hd.capabilities`. The linker, store host state, import dispatcher, and reactor
registration are internal. `hd_run::Instance` already exposes `init`, `poll`,
`wake`, and `completed`, so a GUI can own the driving loop after instantiation.

JavaScript generation in `hd_host_abi` already uses the checked contract and
the value mappings summarized in [Package Host Capabilities](../compiler/package-host-capabilities.md#generated-glue-for-js-hosts).
The native design extends that source of truth; it does not change hd source.

Parse log: the one `text` block parses with
`scripts/hd-conformance.sh parse`; parsing checks syntax only.

## Appendix E: Delivery Details

- **Object layout and counting:** generated Rust values are owned; guest memory never escapes a call; handles record owner instance and trait type.
- **Host ABI parity:** Node and Wasmtime decode the same schema and implement the same logical value table.
- **Suspension:** ready and pending native calls share the reactor's handles, delivery, abort, and instance-close rules.
- **Failure and cleanup:** codec, forged handle, wrong handle type, or provider failure is `host-contract`; startup mismatch is refusal; close aborts pending calls and drops registry values.
- **Footprint:** Wasm should remain byte-identical; measure Rust source and native binary size; `hd_web` excludes the generator feature.
- **Sandbox and grants:** Rust has ambient process authority; root manifest consent and explicit registration are both required.
- **CLI workflow:** native builds emit a deterministic Rust companion; missing output paths, stale schemas, and absent bindings need text and JSON diagnostics.
- **Platforms:** first delivery targets Rust embedders using `hd_run_wasmtime`; dylibs, C ABI, mobile language bridges, and general exports remain deferred.

Implementation slices after approval:

1. Add native owned values and codec round-trips behind `hd_host_abi/native-rust`.
2. Generate deterministic Rust traits and snapshot the hdru-shaped contract.
3. Add registry preflight and linker registration to `hd_run_wasmtime`.
4. Add pending-call cancellation and handle-release tests.
5. Emit the companion in `hd build`, then run both-engine conformance, size, and renderer-shaped end-to-end tests.
