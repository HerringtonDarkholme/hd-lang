# Host Embedding

Design proposal, 2026-10-23; nothing proposed here is accepted behavior or
implemented by this note. It addresses [#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51)
under the [current work order](https://github.com/HerringtonDarkholme/hd-lang/issues/58#issuecomment-6367238331).
The owner reviews the proposal before implementation or specification edits.

## Contents

- [Purpose and existing pieces](#purpose-and-existing-pieces)
- [Other embedders](#other-embedders)
- [Choices and recommendation](#choices-and-recommendation)
- [Registration and providers](#registration-and-providers)
- [Calls and wire values](#calls-and-wire-values)
- [Panics and process status](#panics-and-process-status)
- [Delivery and checks](#delivery-and-checks)
- [Evidence](#evidence)

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
