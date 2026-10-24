# Host Embedding

Design proposal, 2026-10-23; nothing proposed here is accepted behavior or
implemented by this note. It addresses [#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51)
under the [current work order](https://github.com/HerringtonDarkholme/hd-lang/issues/58#issuecomment-6367238331).
The owner reviews the proposal before implementation or specification edits.

The narrowed package-capability section below is active under the 2026-10-24 owner order.
The broader registered-function proposal remains deferred.

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

Status: focused design, 2026-10-24; domains: CLI, compiler, runtime and host. Paired declarations, identity, invoker trust, JS-only providers and signature-generated JS glue are owner decisions; interface details below remain proposals.
This section answers [#51's narrowed order](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6385909694).
The registered-function API in the rest of this document stays deferred.
The two consumers are #266's SDK-owned HttpServer and a future GUI package's rendering/event capability; GUI implementation remains parked.

### Goal, Existing Mechanisms And Options

A package declares an ordinary trait that a selected host may supply.
An application uses ordinary requirement rows, `$.use` and `$.with`; no HTTP-specific entry or registered callback is required.
The minimum covers one ordinary main, value arguments/results, Pending operations, startup binding checks and explicit authority.
Generated glue covers strings, structural values, supported host handles and suspension. Generic host methods and general exported functions remain separate work; a signature outside the existing boundary codec receives a precise diagnostic.

Today's compiler already accepts a toolchain-supplied profile containing an application trait.
`profile_vtable` in the linear backend emits its exchange-buffer stubs and suspending method protocol.
It currently names that import with the trait's final path segment, which is insufficient for package/version identity.
The ordinary CLI profile instead comes from the built-in ABI table, and native runtime profiles are a fixed enum.
These are reuse points and gaps, not proof that arbitrary package host providers work today.

| Mechanism | Effect and cost |
| --- | --- |
| Package manifest declaration | Owner-selected `[host] traits = ["server.HttpServer"]`, paired with `@host`; validate the same ordinary trait in both places. No grant or provider-code execution follows from this list. |
| Source marker | Owner-selected bare `@host` on an owned public trait, with a matching manifest entry. It adds a decorator and its target/form checks; no new trait/type/row mechanism is introduced. |
| Host-selected build profile only | Smallest existing mechanism: the embedder supplies a profile listing an ordinary trait. It cannot satisfy package declaration or ordinary package tooling by itself. |

The [owner chose both declarations](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6386209584): bare `@host` on a package-owned `pub trait`, and a matching `[host] traits` list.
They must match exactly; either missing side is an error naming the trait and both source/manifest locations.
This is an explicit new decorator with a paired metadata check, rather than the earlier manifest-only recommendation.
The manifest allows tools to discover eligibility without scanning source and reminds users of provider responsibility.
Declaring eligibility for a host binding is separate from installing a provider or granting its authority.
A dependency's manifest cannot execute JavaScript or expand the invoker's policy.

### Identity And Contract

Use a nominal capability key comprising package identity, selected version and package-relative trait path.
For fetched packages, use canonical host path and the exact selected version, not just its compatibility line.
The existing dependency identity deliberately coalesces compatible versions; changing that language identity is outside this job.
Carry the additional selected-version fact separately for the host contract.
For local packages, the proposed initial key hashes their existing canonical-directory identity; moving a checkout requires rebinding that local key.
This avoids guessing a globally unique publisher identity from a local manifest name.
Portable local identity needs a separate explicit decision, rather than silently equating two packages with the same name.

A deterministic import namespace is `hd:pkg:<capability-key-hash>`, with method names underneath it.
Encode component strings with lengths before hashing; concatenating ambiguous separators is insufficient.
Keep the complete unhashed identity in the module contract for validation and diagnostics.
Reject duplicate or conflicting identity/schema pairs; a hash alone is not the authority decision.
Built-in imports retain their existing names and bytes.

Add a versioned `hd.capabilities` custom section only when a reachable entry needs a package host provider.
It contains capability identity, wire version, canonical method names, suspension flags and complete canonical argument/result schemas.
Validation compares these complete schemas, rather than trusting a schema hash alone.
No absolute source path is embedded for local identity; the identity carries a directory hash and the package-relative trait path.
Method ordinals are local dispatch indexes, never global identity or authority.
Unreachable methods retain the existing uncalled-slot behavior.

Accept only methods with concrete boundary-safe argument/result types and `&self` or `&mut self` receivers initially.
Reuse directional private-field serialization and the existing value codec; do not expose RC pointers or live exchange-memory views.
Describe unsupported maps or recursive codecs accurately until their existing backend gaps are resolved.
An unsupported signature gets a declaration-site diagnostic before an artifact is emitted.
A descriptor cannot authorize a method whose checked signature did not produce it.

### Binding, Startup And Authority

The trusted adapter explicitly registers a provider against a descriptor extracted from the built module.
Registration checks the complete nominal identity, wire version, schema and methods, including the suspension flag.
Never register by final trait name or by a string suffix.
Two packages' Server traits and two selected versions remain distinct, even if their methods happen to match.

Validate every required binding and grant before `hd.init` and before guest module initializers execute.
A missing provider is a startup refusal naming package, version, trait and missing methods.
Do not substitute a panic-only function or defer missing-binding failure until the first call.
Unused eligible declarations need neither a provider nor a grant.
An ordinary HD fake selected with `$.with` does not need a host provider for the replaced calls.

**Security boundary:** provider JavaScript is trusted host code, with the authority of its embedding realm.
HD's capability grants cannot sandbox arbitrary JavaScript imports or prevent a malicious provider from using ambient Node APIs.
Therefore a package declaration must never automatically load a dependency's JavaScript module.
Only the invoker or trusted platform adapter selects and executes provider code; describe that trust explicitly in SDK tooling.
Loading a provider and granting guest calls to it are separate actions.
A provider registered for HttpServer does not implicitly bind or grant Net, Http, Process or filesystem capabilities to the guest.
Resource-specific checks inside a trusted provider remain part of its host contract.

The [owner requires invoker-only loading](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6386194376), through a platform adapter or explicit run-command registration.
A manifest grants no authority; an unregistered package capability is unavailable.
A dependency cannot override the invoker's decision or select provider code automatically.
Do not accept arbitrary provider-defined scope strings as if the generic runtime understood their security meaning.
For the first version, the adapter configures a concrete resource scope, such as one HTTP listener or one GUI surface.
Root-manifest metadata is not consent to load provider code; explicit invoker registration is required.
No new `hd build --platform` flag or platform-specific core command follows from this capability mechanism.

The [orchestrator selected source-path spelling](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6386908223) for the root manifest's custom capability keys: quoted `dep.server_sdk.server.HttpServer`, or `pkg.ui.Ui` for a root-owned trait.
Built-in keys keep their bare names.
Resolve these paths through the selected graph to the full nominal identity; never match only the final name.
This avoids rewriting manifest grants on every version bump without weakening the exact-version runtime binding.
A false grant refuses the binding, and no table entry causes JavaScript loading.


### Pending Calls And Request Context

The existing start/poll/finish/abort protocol remains the method contract.
A bang call may return ready; work that would wait returns Pending without blocking the guest's polling thread.
Copy encoded arguments before returning Pending, and retain results until finish or abort decides ownership.
Aborting an operation releases its resources; a write discards its unwritten suffix while preserving bytes already passed to the stream.
Late completions cannot wake another operation or a later instance generation.

A host provider may own context beyond the generic runtime, such as a platform request lease or GUI surface.
The SDK must carry that context explicitly in its values or provider objects; ambient current-request state is insufficient.
The #266 control demonstrates the failure: another request's Wasm poll can resume an older task under the wrong workerd I/O lifetime.
Generic package capabilities must not prevent a lease-scoped provider from routing that operation back to its owning platform request.
The runtime still forbids reentrant calls into one instance.

### Generated Glue For JS Hosts

The [owner narrowed execution to JS hosts](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6386503811) and then [required signature-generated glue](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6386656263).
A provider author implements plain JavaScript methods; the toolchain generates their Wasm conversion and operation plumbing from the checked `@host` trait signatures.
Node, workerd and browser adapters consume the same generated interface, with platform-specific implementations supplied by the invoker.
The Worker wrapper owns platform request scopes independently of the guest's ordinary main.
The GUI host supplies rendering and events without introducing a DOM dependency into std.

**Approved generator placement:** `crates/hd_host_abi/src/javascript.rs`, alongside the versioned schema representation in `schema.rs`.
It consumes the compiler-checked contract, rather than reparsing HD source or guessing types from JavaScript values.
The driver exposes the resulting glue as a build output; package builds write `NAME.host.mjs` beside `NAME.wasm` only when a package capability is reachable.
Direct Node execution passes that generated module to the runner, and an SDK packaging task copies it into a workerd/browser wrapper.
No platform-specific `hd build` flag, runtime `eval`, arbitrary package script or provider implementation is part of generation.
A trusted adapter selects the provider implementation separately and binds it through the generated module.

The generator writes conversions for each concrete method's arguments and result, string encodings, supported handle references, and the existing start/poll/finish/abort entry points.
Small shared host helpers own checked buffer access and operation lifetimes; generated method-specific functions call them with the exact schema.
Generate synchronous methods as direct calls and suspending methods as ready values or Promise completions, retaining owned inputs/results across Pending.
The proposed provider call context carries cancellation and instance-local handle ownership without requiring provider authors to encode bytes or invent operation IDs.
A returned handle is an opaque provider object registered in the instance's handle table; guest integers never become ambient JavaScript objects without a generation/type check.
Handle release and cancellation use the existing ownership protocol; arbitrary HD references, trait objects and closures do not become wire handles.
Malformed results fail at the checked host boundary before guest memory receives them.
The output includes the complete nominal contract and canonical schemas, so stale glue or a wrong-version provider is rejected before initialization.
The generated module and the actual Wasm custom section must match byte for byte.

The generated module exports `defineProvider(selector, implementation)` and `bind(runtime, providers)`.
A provider pins the generated interface it was implemented against; the runtime checks the required method and returned-handle schemas against that pinned contract.
Source selectors are display and registration paths, never nominal identity or a final-name fallback.
A generated interface resolves a root source path, exact identity or module key; unqualified trait-name shorthand is rejected.

The explicit Node command is `hd run --engine node --host dep.server_sdk.server.HttpServer=./provider.mjs`.
The flag is repeatable, is parsed before `--`, and never comes from a dependency manifest.
The provider module's default export is the result of `defineProvider` with plain JS methods.
The CLI validates required selectors and denials before loading any provider module, whose initialization already has host authority.
Direct JS adapters supply `packageBindings: runtime => generated.bind(runtime, registrations)` to `createCoreHost` alongside the compiled Wasm module.
These helpers use web APIs only; the CLI's module loader is Node-specific.

Provider values use numbers for integers through 32 bits, BigInt for 64-bit integers, strings, Uint8Array for bytes, arrays and named record objects.
An enum is `{ tag: "Variant", fields: { ...payload } }`; keeping payload fields separate preserves a field literally named `tag`.
A Result is `{ ok: true, value }` or `{ ok: false, error }`, and an optional is null or its value.
Private-field types retain the existing Serialize/Deserialize JSON **text**, avoiding a lossy JavaScript parse of 64-bit integers.
Methods receive a final context containing the call's `signal` and the instance's `instanceSignal`.
A suspending method returns a ready value or Promise; a non-suspending method returning a Promise is a host-contract failure.

Each delivered handle creates one instance-local registry record; its HD box owns that record and ordinary HD aliases share its count.
The box's last release invokes a hidden `$release` import that only removes that registry record.
It performs no I/O and calls no provider resource method: explicit disposal remains explicit.
Encoding a malformed enclosing result rolls back any handle records allocated earlier in that result.
Pending completion retains owned values without touching Wasm; only validated finish encodes and transfers them.
Abort removes the operation, signals its AbortController and ignores late Promise completion; closing the instance aborts outstanding operations and clears handle records.

This is code generation, not a promise that provider authors manually follow a wire-format document.
The generator has golden output and execution tests: two same-named traits, strings, ready and Pending results, malformed values, handle release, cancellation and stale operation generations.
The built-in ABI generator and built-in imports stay unchanged; a program with no package host use emits no glue and keeps its Wasm bytes.

### Wasmtime Refusal And Deferred Native Loading

The CLI stays on Wasmtime. A program needing a package `@host` trait is refused before guest initialization, naming the complete trait and saying it requires a JS host.
There is no engine fallback, Node bridge or native provider loading.
An ordinary HD fake that removes the host requirement still runs on either engine.
`hd install` copies no provider artifacts for now.

Native dylibs, a C ABI, `[host.native]`, per-target provider artifacts and an FFI loader are deferred until users need them.
They are not active delivery slices, workspace dependencies or package metadata in this change.
Any later revival needs a separate ABI/lifetime design and explicit approval for unsafe loading.

### A GUI-Shaped Current-Compiler Control

The complete following program parses, checks and runs today with an ordinary HD fake.
It prints `selected=7 commands=1`; its Ui name is not compiler-known.
This establishes the row and fake-binding part only, not a package-declared default host provider.

```text
# A GUI-shaped capability without a compiler-known trait name.
pub enum Event:
    Click(node: u64)
    Closed

pub enum Draw:
    Text(node: u64, value: string)

pub trait Ui:
    fn render!(&mut self, commands: List[Draw]) -> void
    fn next!(&mut self) -> Event?

struct FakeUi:
    calls: usize

impl Ui for FakeUi:
    fn render!(&mut self, commands: List[Draw]) -> void:
        self.calls = self.calls + commands.len()

    fn next!(&mut self) -> Event?:
        .Some(.Click(7))

fn panel!() -> u64 $ Ui:
    $.use(Ui).render!([.Text(7, "Continue")])
    match $.use(Ui).next!():
        .Some(.Click(node)) => node
        _ => 0

pub fn main!() -> void $ Console:
    fake := &mut FakeUi { calls: 0 }
    let selected: u64 = 0
    $.with(Ui=fake):
        selected = panel!()
    println("selected=$selected commands=${fake.calls}")
```

Changing main to require Ui directly produces this intentional current gap:

```console
nonhost-entry-requirement: `Ui` is not a host capability trait of the runtime profile, so the entry point `main!` cannot require it
```

This is an expected negative probe, not an agent writing mistake.
HTTP stress cases add whole Request/Response values, two same-named traits in different dependencies, and cancellation after readiness.
GUI stress cases add a pending event source, a ready render, surface-local IDs, denied input access and a fake renderer.
Neither consumer requires source callbacks, a new entry form or registered host-callable HD functions.

### Data Flow, Budget And Tests

The proposed records are `DeclaredCapability` (owning package and trait path), `CapabilityKey` (nominal identity and selected version), and `MethodSchema` (name, wait flag, codecs).
`Binding` holds the exact key/schema, trusted implementation and explicit grant; `PendingCall` holds instance generation, operation ID, owned bytes and abort ownership.
Store declarations in package metadata, and derive descriptors only after normal resolution and signature checking.
Include declaration and schema fingerprints in build/cache keys; a provider's JavaScript implementation is a runtime input, not a reason to rebuild unrelated HD code.
Sort emitted descriptors by canonical identity and methods deterministically.

The core algorithm is host pseudocode; this is not a new public API.

```rust
fn prepare(module, registry, invoker_policy) -> BoundInstance {
    let contract = read_and_validate_contract(module)?;
    check_wire_version(contract.version)?;
    let mut bindings = Vec::new();
    for needed in contract.reachable_capabilities() {
        let supplied = registry.lookup_exact(&needed.identity)?;
        check_schema(needed, supplied)?;
        check_explicit_activation(invoker_policy, needed)?;
        bindings.push(bind_owned(needed, supplied));
    }
    let imports = make_imports(bindings);
    let instance = instantiate_without_initializing(module, imports)?;
    instance.initialize()?;
    instance
}

fn start(binding, method, encoded_args) -> Reply {
    let args = copy_and_validate(encoded_args, method.schema)?;
    match binding.start(method, args)? {
        Ready(bytes) => encode_checked_result(bytes),
        Pending(operation) => {
            let id = register_owned_operation(operation);
            Reply::Pending(id)
        }
    }
}

fn finish_or_abort(id, finish) {
    let operation = remove_exact_generation(id)?;
    if finish { transfer_checked_result(operation); }
    else { operation.abort_and_release(); }
}
```

Current file sizes below are physical lines measured on the source-matched main; none is proposed for wholesale replacement.
New-line figures are planning budgets derived from the listed components, not measurements of code that already exists.

| Component | Current file lines / deleted | Proposed added code and proving test |
| --- | --- | --- |
| Declaration and package inputs | `manifest.rs` 867 / 0 | 55 parsing/validation, 75 propagation and selected-version metadata; invalid path and dependency-rename tests. |
| Existing profile vtable and descriptor | `rt.rs` 3524, `boundary.rs` 1549 / 0 | 35 import-identity changes, 100 descriptor/schema emission; two same-named traits and byte-stable imports. |
| Host ABI schema | Existing generated ABI stays | 100 owned/versioned schema and validation; deterministic round-trip and invalid-contract tests. |
| JS dynamic provider adapter | `core.mjs` 1319 / 0 | 140 codecs/registration, 80 operation ownership; malformed result, ready/Pending and late-abort tests. |
| Signature-generated JS glue | No package-glue generator exists | 90 method/codec projections, 65 operation wrappers, 65 generated-interface tests; plain JS providers on Node and workerd. |
| Wasmtime startup refusal | Existing engine import validation retained / 0 | 20 contract-aware refusal lines and 20 startup-order test lines; name the trait before any guest initialization. |
| Conformance and tooling | Existing harnesses retained | 180 focused fixture/control lines; HTTP and GUI-shaped cases, fake replacement, denied grants and startup ordering. |

Stop for review if a slice exceeds 1.5 times its code budget or needs a new runtime mechanism.
A program using no custom host binding must keep its Wasm bytes unchanged, including hello; prove this against a saved before-build hash.
No custom codec, bridge or descriptor is linked into such a program.
Measure compile time, descriptor bytes and ready/Pending dispatch before claiming overhead targets; generated-glue bytes and helper bytes must be reported separately.
The web compiler's 2 MB gzip cap stays a required gate for compiler/monomorphizer changes.
Provider code is not automatically bundled into that compiler or a GUI guest module.

### One-Hour Slices And Exact Scope

Each slice needs owner approval of its named production files, plus its corresponding spec and conformance fixtures.
No changes to hd_check or hd_tir are planned; report any actual need before entering those protected lanes.

| Slice | Proposed production files and proof |
| --- | --- |
| P0 | This focused design and GUI/negative controls; owner selected paired declarations, nominal identity, invoker-only trust, JS-only execution and generated glue. Native loading is deferred. |
| P1 | `crates/hd_project/src/manifest.rs`, `crates/hd_project/src/lib.rs`, `crates/hd_cli/src/disk.rs`, `crates/hd_driver/src/lib.rs`, approved `crates/hd_resolve/src/facts.rs` and `lower/decorators.rs`; propagate declarations and already-selected exact versions, reject both mismatch directions and invalid marker targets. No dependency-selection change is needed. |
| P2 | `crates/hd_host_abi/src/lib.rs`, new `schema.rs` and approved `javascript.rs`, existing driver build-output callers, `crates/hd_mono/src/lib.rs` if its existing profile plumbing needs adjustment, and approved `crates/hd_wasm_linear/src/{lib.rs,rt.rs,boundary.rs,module.rs}`; emit nominal imports, checked descriptors and generated glue; prove hello unchanged. Stay out of `hd_tir` and backend `emit/`, which belong to C4. |
| P3 | `host/core.mjs`, new `host/package-providers.mjs`, `host/run.mjs`, `host/node.test.mjs`, `crates/hd_cli/src/node.rs` and direct generated-output callers; trusted explicit JS binding, generated conversions, startup validation, ready/Pending/abort, handles and ordinary fake replacement. `crates/hd_run_wasmtime/src/engine.rs` adds only the JS-host-required startup refusal and its focused tests. |
| Final acceptance | Applicable language/CLI rules and conformance fixtures accompany their implementing code; HTTP/GUI-shaped JS calls, native refusal, fake replacement on both engines, ABI/cache determinism and compiler/web size checks. No native loader or provider-artifact installation. |

The new declaration and custom grant policy belong in CLI/package rules; host binding and boundary semantics belong in the language-tier module chapter.
This changes the CLI's closed built-in-profile rule; it needs a new rule ID, with the retired ID recorded in the implementing commit.
No std-owned HttpServer or GUI capability is added.

### Recommendation, Approvals And Evidence

Apply the owner's ordinary `pub trait` plus `@host` and matching manifest declaration, complete nominal descriptors, explicit trusted provider registration and signature-generated JS glue.
Keep the mechanism neutral between an incoming HTTP source and a rendering/event source.
The owner accepted local directory hashing and explicit invoker trust; no Node bridge or automatic package provider loading is planned.
C3 has landed and the four named backend files are approved; C4 owns `hd_tir` and `emit/`. The [generator file is approved](https://github.com/HerringtonDarkholme/hd-lang/issues/51#issuecomment-6386715935), alongside the schema and direct generated-output callers; native loading is deferred.
P1 declaration metadata may proceed meanwhile, with source/manifest mismatch checks required before claiming the paired-declaration rule implemented.

The GUI fake passes parse, check and execution on a dev compiler from `02d4381ff6d7f6ad52fb659329360531c9200b3a`.
Its expected missing-default-provider variant reports `nonhost-entry-requirement`, before any host module is loaded.
Both controls are retained under `.cache-muse-51-minimum/` with scratch cache and temporary directories inside the isolated worktree.
The deliberate negative control does not establish the proposed startup diagnostic, which still needs implementation.

Relevant semantics: [runtime profiles](../../spec/lang/10-modules.md#runtime-profiles), [host boundary](../../spec/lang/10-modules.md#host-boundary), [capability grants](../../spec/cli/command-line.md#capability-grants), and [host operations](../../spec/std/host.md#host-operations).
[WebAssembly's JS interface](https://webassembly.github.io/spec/js-api/) supplies imports but no HD trait schema or grant policy.
[Wasmtime Linker](https://docs.wasmtime.dev/api/wasmtime/struct.Linker.html) supplies explicit named native bindings, not execution of JavaScript providers.
[Extism host functions](https://extism.org/docs/concepts/host-functions/) likewise require host-supplied functions and signatures; importing a function does not itself constrain its host authority.
These reuse the earlier survey's linking lesson without reopening the general embedding API.

## Purpose and existing pieces

An application should call named hd functions, bind its own capability
providers, and receive typed results or structured failures. Native and
browser hosts should share the registration contract and value encoding.
The first release uses core Wasm, as the owner already decided.

The language distinguishes [registration](../../spec/lang/10-modules.md#registration)
from `pub`. Each registered function is an entry for provider checking.
Its row must fit its selected profile, and the host binds every key
before invocation. A manifest cannot invent a runtime profile.

The [boundary rules](../../spec/lang/10-modules.md#boundary-safe-values) already
define structural values, private-field serialization, and poisoning panics.
They exclude dynamic trait values, closures, and live handles from registered
parameters and results. This proposal preserves those restrictions.

| Existing piece | Present limit |
| --- | --- |
| `hd_run::Engine`, `Instance`, `Provider`, `HostSetup` | `Instance` exposes polling and test invocation, without general typed registered calls. |
| `HostSetup::providers` | The native design records that its engine binds built-in providers directly instead. |
| `hd_host_abi` and generated JS/Rust codecs | Built-in provider descriptions exist; application-trait registration is not a public general API. |
| Exchange memory and poll/wake driver | One active call per instance, without reentrant callbacks into Wasm. |

At `5cdef5f66`, the generated model contains 12 capability traits with 34
methods, two runner traits with seven methods, three handle traits with
18 methods, and 22 structured types. These are coverage counts, not an
embedding-readiness or implementation-cost measurement.

## Other embedders

| Tool | Relevant approach | Lesson for hd |
| --- | --- | --- |
| [Wasmtime](https://docs.wasmtime.dev/api/wasmtime/struct.Linker.html) | Its linker binds named host functions, including typed wrappers. | Explicit provider registration can hide engine internals. |
| [Wasmer](https://wasmerio.github.io/wasmer/crates/doc/wasmer/struct.Imports.html) | Imports pair module/name keys with host functions. | Validate complete binding identities, not method names alone. |
| [wazero](https://pkg.go.dev/github.com/tetratelabs/wazero#HostModuleBuilder) | A host module builder exports functions into an instance. | A library API can hide raw linkage. |
| [wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/reference/attributes/on-rust-exports/index.html) | Generates bindings for selected exports. | Generate adapters from the compiled module's checked contract. |
| [Extism](https://extism.org/docs/concepts/host-functions/) | Host functions declare names, input/output types, and application data. | Custom providers need signatures and instance-lifetime state. |
| [AssemblyScript](https://www.assemblyscript.org/runtime.html#interface) | Raw managed references need host pinning across allocations. | Prefer owned boundary copies over exposing hd's RC pointers. |

Explicit bindings and generated adapters work with core Wasm. These tools'
memory policies differ; hd should preserve its own boundary semantics.

## Choices and recommendation

Every option selects this ordinary hd function:

```text
pub struct Request:
    pub name: string

pub fn greet(request: Request) -> string:
    "hello, ${request.name}"

pub fn main() -> void:
    pass
```

`pub` alone registers nothing. Registration selects `greet`; the existing
source needs no export decorator or foreign-function declaration.

| Criterion | Explicit library contract | One hand-written dispatcher | Automatic public exports |
| --- | --- | --- | --- |
| Mechanism | A library build API selects concrete source items; compiler-generated adapters consume checked signatures. | One request/response enum and dispatcher combine all calls. | Every public function becomes a host entry. |
| Named typed calls | Generated per entry | Repeated manual dispatch and codec boilerplate | Includes unintended public helpers |
| Provider checking | Every registered entry | Dispatcher union row | Every public function's row |
| Rule cost | Existing registration semantics; new build/runtime APIs | Existing semantics and ordinary hd dispatcher | Changes the visibility/registration distinction |
| Safety | Checked signatures and generated codecs | Checked envelope | Still needs signature checks |
| Agent writing / human reading | Ordinary hd plus explicit registrations | More boilerplate, one envelope to inspect | Shorter, with an unrelated visibility effect |
| Evolution | Independent entry contracts | One growing envelope | Public API changes alter host exposure implicitly |

The dispatcher is the radical simplification and a useful prototype.
Automatic public exports contradict an existing decision and are not eligible
without reopening it. None needs a new syntax form to express the example.

**Recommendation.** Use explicit library registration and generated adapters,
with Rust and JavaScript SDKs sharing one catalogue and wire description.
Keep registration an explicit compiler build input supplied by the library.
This favors runtime/library APIs over a new keyword or semantic exception.

This sacrifices implicit discovery and direct heap references. The next-best
option is a bounded dispatcher prototype with the same provider and wire checks.
No general callback, durable-resume, or Component Model mechanism is proposed.

## Registration and providers

Proposed API concepts, not current method names:

| Operation | Contract |
| --- | --- |
| `register(external_name, source_item)` | Select an accessible concrete function and a unique external name; resolve its actual signature. |
| `profile(toolchain_profile)` | Select a toolchain-defined profile and its nominal bindable trait identities. |
| `build(registrations)` | Root and provider-check every selected entry; generate its adapter and catalogue. |
| `bind(trait_descriptor, provider)` | Match the complete method signatures and wait behavior. |
| `call(entry, arguments)` | Encode checked inputs, invoke the entry, and yield a typed value or structured failure. |

The embedding library owns this build API. Applications may wrap it for
tools or workflows. Registration is neither an `@export` decorator nor an
implicit effect of visibility.

The catalogue records external name, concrete parameter/result schema,
requirement row, profile, and wire version. Open rows and unspecialized
generic functions require concrete registrations; ordinary generic hd functions remain valid.

Provider descriptors use nominal trait identities and actual import mappings.
Two traits called `Database` are not interchangeable. Reject colliding mappings
and incompatible descriptors before initialization; retain existing built-in import names.

Before module initializers run, validate the catalogue, wire version, provider
shapes, grants, and required import bindings. Missing providers produce a
startup error before user code. Bindings remain fixed for that instance.

Application traits work when the selected toolchain profile explicitly supports
their descriptors and adapters. A string-keyed provider object does not add
authority for a trait absent from that profile.

## Calls and wire values

Reuse the exchange-buffer encoding and codec model from
[Host Interface and Embedding](../../crates/hd_run_wasmtime/DESIGN.md#17-host-interface-and-embedding).
Generate the adapter and host codecs from one checked type description.
The catalogue and wire contract are versioned together; incompatible adapters fail at load.

| Value | Handling |
| --- | --- |
| Scalars | Existing width/range checks; live floats retain IEEE 754 values. |
| Strings, byte lists, lists, tuples, public structs, enums, optionals, results | Structural encoding, recursively checked against the signature. |
| Private-field structs | Honor directional Serialize/Deserialize exactly; layout metadata cannot expose omitted fields. |
| Maps | Count and key/value pairs; decoding invokes ordinary Eq and Hash, with the specified poisoning behavior for decoder panics. |
| References, dynamic trait values, closures, live handles | Reject in registered parameters/results under the existing boundary rules. |

The low-level implementation does not cover every specified structural case.
Its design records unsupported map and recursive-type paths. Completing those
adapters is delivery work; this proposal claims no existing full coverage.

The host owns argument and result copies. No exchange-memory view survives
an allocation, await, wake, or invocation boundary. Copy encoded arguments
before reusing the buffer. A provider retaining arguments must own its
copies before returning Pending.

Allow one active invocation per instance. A second call or a provider
callback into that instance returns busy before entering Wasm. Parallel
calls use distinct instances with disjoint heaps and drivers.

Suspending entries use the existing poll/wake protocol behind SDK futures
or promises. Host cancellation retains the current cleanup bounds and requires
its exported entry hook to remain a compiler reachability root.

Provider-returned host handles are separate from registered value parameters.
They use profile descriptors, instance-local host identifiers, and existing close
behavior. They never expose hd pointers. The broader handle design in [#9](https://github.com/HerringtonDarkholme/hd-lang/issues/9)
is not reopened by this proposal.

## Panics and process status

Keep typed invocation outcomes separate from process policy:

| Event | SDK outcome | CLI adaptation |
| --- | --- | --- |
| A function returns `Result.Err` | Ordinary typed result | Exit mapping applies only when its entry contract requests that mapping. |
| hd panic, including host-contract or decoder panic | Panic report; poison and discard the instance | Selected profile's panic status |
| Module/profile/provider mismatch | Load/start error before user code | 101 when `hd` itself owns the failed tool operation |
| Runtime implementation failure | Internal failure, distinct from a language panic | Existing internal-failure reporting; no fabricated panic category |

Recommend panic status **3** for official native and Node profiles, matching
their current runtime behavior. Other toolchain profiles declare a status
explicitly, never 101. Browsers return the panic report and invalidate the
instance; they have no process status to set.

SDKs never terminate the embedding process. The application owns that policy.
Histories record the stable diagnostic name, message, and logical source locations.
Engine trap spellings remain diagnostic detail, not replay identity.
Never invoke a poisoned instance again.

## Delivery and checks

The owner approves this runtime API direction before code. Runtime API
and ABI details belong with the runtime contract; existing language registration
and boundary rules remain authoritative. No spec rule or ID changes here.

Start with one pure structural entry, explicit registration, a generated
descriptor, and native/Node invocation. Then wire application providers and
missing-binding failures, suspending entries and cancellation, and the remaining structural types.

The native slice must actually dispatch supplied providers instead of leaving
`HostSetup::providers` unused. Validate application provider results with the
same rules as built-in providers. Do not advertise a complete SDK before
the supported boundary types have corresponding adapters and tests.

Fixtures cover private-field redaction, domain errors, narrow integers, floats,
maps, invalid UTF-8, wrong tags, stale handles, profile mismatch, reentrancy,
and use after panic. Compare semantic outcomes across hosts and retain
generated-schema consistency checks and golden wire vectors.

No size or speed promise follows from this note. Measure the first vertical
slice before proposing catalogue caching, zero-copy values, or cross-instance sharing.

## Evidence

Source inspection and model counts use `5cdef5f66e054c4df32a04da56a500726020bc28`.
The count command reads `host/gen/abi.json`, counts each table's entries and
methods, and counts its type declarations. Exact counts and source hashes
are retained in this note's validation evidence.

The complete hd example parses and checks with zero errors and warnings.
That validates its source only: it does not register `greet`, implement
an SDK, or demonstrate a cross-boundary invocation.

Validation used frozen compiler SHA-256 `9b1f42c4c0c62247243594f55a61319e28f09dea3580f89353051b9ff86a9a7c`
and parser SHA-256 `ca6a7ddc0432b75003dab442c5da45356473dd40c531a861054dc6fcd2d448a5`.
The compiler is `bc3ffb05a3` plus #526 cancellation pruning, which changes
no syntax or type rule. Exact commands, outputs, ABI counts, and source
hashes are retained in `/private/tmp/muse-51-evidence-1792772655965311000`.
