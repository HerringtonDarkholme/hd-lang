# Embedded V8 Study Probes

Status: experimental controls for [#826](https://github.com/HerringtonDarkholme/hd-lang/issues/826), not a production HD host or accepted engine decision.
Read the [report](../EMBEDDED_V8.md) for limitations and results.

`component/` contains the two minimal engine executables used for the cold-build/size experiment.
`hd/` adds UTF-8, output, clock and system-random callbacks to the V8 probe.
Both retain their measured Cargo manifests and lockfiles. Rust sources are formatted for publication; the original source hashes accompany the measurements.
The copied Wasmtime executable in `hd/` is unused; the HD runtime comparison uses the production native CLI.

`bundle.mjs` bundles the repository's unchanged `host/core.mjs` closure and surrounds it with `shim.js` and `run.js`.
Only CPU work and ready completions are supported. The Rust harness rejects genuinely pending I/O; it does not implement a platform event loop.
UTF-8 `λ😀` is checked on startup; random requests use system entropy, with typed-view byte offsets respected.
This is not full TextEncoder/TextDecoder or crypto Web API validation.

## Reproduce

Run from the repository root on macOS, with existing repository Node dependencies, Rust and a native-feature `hd` binary.
Use the report's base for comparable compiler output, and save its hash before checking out other work.
A native CLI is built with `cargo build -p hd_cli --features wasmtime` in the isolated checkout.
`measure.py` refuses an existing output directory; use a new scratch path for each run.
Every HD command gets scratch `HD_CACHE`. Cargo uses dev opt-level=1, separate new targets, and locked dependencies.

Download the public [aarch64-apple-darwin V8 Release archive](https://github.com/denoland/rusty_v8/releases/download/v152.2.0/librusty_v8_release_aarch64-apple-darwin.a.gz), decompress it, and supply its path.
The measured decompressed archive SHA-256 is `680617c57021c1ad7490749e4c10c4504978e84c3e3a288b8bb39a31a9e0d1f8`.
Archive download and registry population are outside the cold-target timing.

```sh
python3 docs/future-work/embedded-v8/measure.py components \
  .cache-muse-826/reproduce-components --archive /absolute/path/librusty_v8.a
python3 docs/future-work/embedded-v8/measure.py runtime \
  .cache-muse-826/reproduce-runtime --archive /absolute/path/librusty_v8.a \
  --hd /absolute/path/frozen-native-hd
```

The component command times one successful fresh-target build per engine, strips copies, then runs fifteen alternating empty-module samples plus preflight.
The runtime command builds both engine variants of the unchanged seven benchmark sources plus an empty control.
It checks every execution against the existing JavaScript reference, rotates engines for five timed fresh processes plus preflight, then measures native with fresh caches.
It saves every timing, module hash, output/status and available resource record in the scratch results.
No benchmark algorithm, tiering flag, production engine or root Cargo manifest changes.

## Recorded evidence

[results.json](results.json) contains the original machine/compiler provenance, component builds, all startup trials, all 144 runtime observations and 48 additional native fresh-cache observations.
It includes original source hashes and absolute scratch paths as provenance; the reproduction commands generate their own paths.
`publishedReproduction` records a second complete execution through the published runner: another 192 HD runs and 32 component startup trials.
Its negative controls reject pending I/O and JavaScript exceptions; native primitive controls check UTF-8 and random typed-view boundaries.
The original timing scripts and failed experiments remain in `/private/tmp/muse-826-1792924496864963000` on the measuring machine.
The initial runtime attempt lacked a random callback and failed at map-count; it is excluded from the complete corrected cohort.
The initial failed Rust API build is likewise excluded from successful cold-build timing.
No warm native-code cache, persistent V8 serializer, hosted deployment, or general I/O parity is demonstrated for embedded V8.
