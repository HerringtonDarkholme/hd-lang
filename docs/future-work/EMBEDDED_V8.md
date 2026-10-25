# Embedded V8 For The CLI

Status: measured study for [#826](https://github.com/HerringtonDarkholme/hd-lang/issues/826), 2026-10-25. Nothing here accepts new behavior or changes the production engine.
It reviews [host embedding](HOST_EMBEDDING.md), [installed programs](../../spec/cli/command-line.md), and the existing native/JavaScript host boundary.

**Recommendation:** keep Wasmtime for now. Embedded V8 builds faster with a local prebuilt archive, but increases component size and shows no consistent runtime advantage.
Sharing the portable host is feasible; asynchronous native I/O and a persistent code-cache bridge remain substantial work.

## Measurement boundary

Base: `f66af14419527ef3f812fce2524f68663658be25`. Machine: Apple M3 Max, 36 GiB, macOS 26.6.2; Rust 1.97.1.
Wasmtime is 49.0.2 with the repository's feature set; rusty_v8 is pinned to 152.2.0.
Node is 24.19.0, V8 `13.6.233.17-node.51`. These are different V8 versions; embedding alone does not explain their differences.
Sources, locks, commands, hashes and individual observations ship in [embedded-v8/](embedded-v8/README.md), including [raw results](embedded-v8/results.json).

Rust uses **dev, opt-level=1, default debug** throughout. V8's downloaded C++ static library is its prebuilt Release artifact.
Cold builds use fresh, separate targets, warm registry sources and a local archive: download time and building V8 from source are excluded.
The seven unchanged `bench/runtime/progs` programs use release **Wasm**, as the benchmark does; no Rust release build was run.

## Binary, build and startup cost

| Minimal engine executable | V8 | Wasmtime |
|---|---:|---:|
| Fresh-target build, seconds | 12.235 | 33.012 |
| Executable, bytes | 64,279,704 | 17,602,480 |
| After `strip -S -x`, bytes | 43,584,752 | 14,322,184 |
| Empty core-Wasm startup median, ms | 6.115 | 3.907 |

The stripped V8 component is **3.04×** the Wasmtime component.
These executables initialize an engine, compile and instantiate an eight-byte module, then exit; neither implements the HD platform host.
Startup uses fifteen alternating fresh processes after one discarded preflight per engine; every result is retained.
Ranges were V8 5.20–276.63 ms and Wasmtime 3.50–9.88 ms; binary paging and first-use effects remain visible.

These component sizes do not predict the full release CLI size: shared dependencies, linking and profiles differ.
The measured original native **dev** CLI is 42,780,960 bytes. The issue's 13 MB release figure was not remeasured under this dev-only policy.
An initial failed V8 API build is excluded from the successful cold-build duration and preserved in local evidence.

## Actual HD runtime

Each cell is the median of five fresh processes, after one preflight. The three engines rotate order per trial.
All 144 processes match each program's JavaScript reference output and exit successfully.
The additional fresh-cache native cohort has 48 successful processes; each receives a new `HD_CACHE`.

| Program | Embedded V8, ms | Node host, ms | Native, normal cache, ms | Native, fresh cache, ms |
|---|---:|---:|---:|---:|
| empty | 15.58 | 308.40 | 13.01 | 25.83 |
| closures | 259.03 | 293.47 | 265.58 | 285.12 |
| int-loop | 793.42 | 834.53 | 806.57 | 818.11 |
| list-sort | 652.33 | 618.14 | 252.74 | 273.87 |
| map-count | 20.87 | 64.90 | 22.57 | 24.84 |
| string-build | 129.98 | 160.71 | 130.60 | 134.26 |
| string-builder | 17.26 | 61.18 | 18.95 | 24.27 |
| trait-dispatch | 761.79 | 734.70 | 574.56 | 588.30 |

The embedded CPU host executes the unmodified portable core, with Rust output, UTF-8, monotonic-clock and system-random callbacks.
Node and native use their actual production hosts and engine-specific builds of the same HD sources.
Only native has a persistent code cache here; the normal-cache cohort permits its existing per-function cache after preflight.
Fresh process does not mean cold OS pages or cold code cache.

The embedded probe has no asynchronous filesystem, process, socket, timer or HTTP implementation; genuinely pending I/O fails explicitly.
Its TextEncoder/TextDecoder shims cover these controls, not complete Web API conformance.
Node's initial 308 ms empty result includes full platform startup/shutdown; it is not a pure engine comparison or a diagnosed engine defect.
The published runner independently repeats all 192 HD executions successfully; empty medians become embedded 21.15, Node 42.88 and native 13.86 ms.
First-use effects make the initial startup gap unstable. List-sort remains about 2.54× slower on embedded V8 in the repeat.
Typical peak RSS is embedded 18–23 MiB, native 14–26 MiB and Node 56–70 MiB; closures reaches about 227/222/274 MiB respectively.

List-sort is 2.58× slower than normal-cache native; trait dispatch is 1.33× slower. Other CPU cases are close or favor embedded startup.
No tiering flags or algorithms changed. V8's lack of Wasm OSR makes long, rarely called loops a plausible shared concern with [#165](https://github.com/HerringtonDarkholme/hd-lang/issues/165).
That is an inference from [V8's pipeline documentation](https://v8.dev/docs/wasm-compilation-pipeline), not a profile of these embedded program counters.

## Shared host code and moonrun

The actual bundled `host/core.mjs` dependency graph has **4,102 physical lines**, including 531 generated ABI lines: **3,571 handwritten lines**.
Its unminified IIFE is 116,595 bytes. All seven files execute unchanged in the CPU probe.

| Reusable JavaScript candidates | Still required in an embedded host |
|---|---|
| Core poll/driver protocol; journal; wire/float codecs; checked reports; package-provider protocol; generated ABI | Native files, children, stdin, sockets, timers, HTTP; V8 memory/reference rooting; callback adapters; asynchronous event pumping and cancellation |

The native crate has 24,092 physical `src` lines, including inline tests, plus 6,891 separate test lines.
This audit does **not** establish that 30,983 Rust lines can disappear. Platform implementations and parity tests retain their obligations.

[moonrun at `3778465`](https://github.com/moonbitlang/moon/tree/3778465a252d2838357dddf25dc0ebf3ff3ba40f/crates/moonrun) defaults to V8, with optional Wasmtime/wee8.
Its [engine layer](https://github.com/moonbitlang/moon/blob/3778465a252d2838357dddf25dc0ebf3ff3ba40f/crates/moonrun/src/engine.rs) uses shared **Rust** host domains/import registration with engine-specific adapters.
Its workspace pins v8 0.106.0 and Wasmtime 48.0.1; it is architectural prior art, not this benchmark's dependency set.
Using the same engine family across CLI, Node, workerd and browsers still leaves version, flag, feature and platform differences.

## Installed programs and caching

The issue's precompiled-module premise needs correction: today's [installer](../../crates/hd_cli/src/install_cmd.rs) stores immutable release **Wasm**, its hash, ABI revision and grants.
The native [code cache](../../crates/hd_run_wasmtime/src/code_cache.rs) uses safe per-function Cranelift `clpack` entries; it deliberately avoids unsafe `Module::deserialize`.
Replacing the engine need not change the installed Wasm/launcher layout, but requires a different optional cache backend.

In [rusty_v8 152.2.0](https://github.com/denoland/rusty_v8/blob/v152.2.0/src/wasm.rs), `CompiledWasmModule` supports in-process sharing and wire-byte access, without a public serialization method.
The bundled [C++ header](https://docs.rs/crate/v8/152.2.0/source/v8/include/v8-wasm.h) exposes `CompiledWasmModule::Serialize`; its result excludes the original wire bytes.
Rust already exposes `ModuleCachingInterface::set_cached_compiled_module_bytes`, which can reject incompatible data.
Writing the missing serialization binding or shim remains work; **persistent V8 caching was not implemented or timed**.

A future cache must keep original Wasm, identify engine version/CPU/flags/ABI, validate identity and permissions, and compile normally on rejection.
Existing `clpack` payloads are not reusable.
[V8 documents](https://v8.dev/docs/wasm-compilation-pipeline#code-caching) caching optimized TurboFan code, not Liftoff code; small modules may never reach the browser caching threshold.
Producing optimized install-time cache entries without executing guest effects needs an explicit compilation policy and a measured proof.

The next useful proof would cover real asynchronous I/O, cleanup/cancellation, package providers and cache rejection before considering an engine migration.
This study changes no compiler, std API, production host, specification or default tiering policy.
