# Compiler Architecture

A code map of the Rust compiler, in the style of rust-analyzer's
`architecture.md`: where things live and which rules hold across crates.
The crates are in [`crates/`](../../crates); [`crates/README.md`](../../crates/README.md)
covers building, testing and the disk cache. The [specification](../../spec/README.md)
defines the language; when this map or a design doc disagrees with the code
or the spec, the code and the spec win.

## Pipeline

`hd_driver` runs one task graph per package. Each source file is skimmed
for its imports and header, and parsed on demand into a green tree
(`hd_syntax`). `hd_project` maps files to modules, folders and packages.
`hd_resolve` builds each folder's interface: names, signatures, impl
tables and content hashes. `hd_check` checks each module's bodies against
those interfaces and emits one typed IR (TIR, `hd_tir`) per body; the
driver's `Coherence` and `InitOrder` tasks add the overlap and
initialization-order results. For a program, `hd_mono`
collects the reachable instances from the TIR, `hd_opt` runs the pass
pipeline chosen by engine target and level, and `hd_wasm_linear` emits and links
one RC-managed Wasm module. The CLI runs it on V8 through Node (`host/*.mjs`), or
in process on wasmtime (`hd_run_wasmtime`) when built with the `wasmtime`
feature. Every stage boundary goes through the caller's cache store.

## Crates

Direct workspace dependencies are in each crate's `Cargo.toml`.

Shared base:

- `hd_base`: IDs, spans, stable hashes, fuel, wire helpers, and the build
  profile, engine target and level. Leaf crate.
- `hd_intern`: interned strings and stable paths.
- `hd_diag`: diagnostic codes and records.
- `hd_types`: the type pool, unification, rows, the trait solver and the
  entry-local tables that carry types into cache entries.
- `hd_host_abi`: the one host ABI table: capability traits, methods,
  codecs, wait flags and import names.

Front half:

- `hd_syntax`: lexer, layout, skim, parser and green tree.
  [DESIGN.md](../../crates/hd_syntax/DESIGN.md)
- `hd_project`: manifests, source discovery, modules, folders and the
  package graph.
- `hd_resolve`: name resolution, folder interfaces, interface blobs and
  their hashes, visibility. [DESIGN.md](../../crates/hd_resolve/DESIGN.md)
- `hd_structure`: the compiler-supplied `Structure` generator that writes
  TIR bodies for derivations and tuples.
- `hd_check`: body checking, exhaustiveness, facts, header checks and
  initialization summaries; emits TIR.
  [DESIGN.md](../../crates/hd_check/DESIGN.md)
- `hd_tir`: TIR columns, builder, verifier, printer, wire form and TIR
  hash. [DESIGN.md](../../crates/hd_tir/DESIGN.md)

Back half:

- `hd_mono`: reachable-instance collection and monomorphization; applies
  body plans. [DESIGN.md](../../crates/hd_mono/DESIGN.md)
- `hd_opt`: the pass manager and the pipelines keyed by (engine target,
  level). [DESIGN.md](../../crates/hd_opt/DESIGN.md)
- `hd_wasm_linear`: RC layout, emission, linking, the runtime pieces it
  generates, and each target's lowering profile.
  [DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)

Services, driver and runtime:

- `hd_cache`: the `CacheStore` interface, its memory and disk stores,
  entry framing and key rules. [DESIGN.md](../../crates/hd_cache/DESIGN.md)
- `hd_sched`: the serial, stepping and threaded executors over the task
  graph. Leaf crate. [DESIGN.md](../../crates/hd_sched/DESIGN.md)
- `hd_driver`: the task graph of a run; assembles every output.
- `hd_run`: the engine-neutral embedding API, the poll and wake driver, the
  test runner core and the live-execution journal.
- `hd_run_wasmtime`: the wasmtime engine and its host providers; outside the
  default members. [DESIGN.md](../../crates/hd_run_wasmtime/DESIGN.md)

Tools and front ends:

- `hd_cli`: the `hd` binary: commands, disk sources and cache, the Node
  engine, the REPL. [DESIGN.md](../../crates/hd_cli/DESIGN.md)
- `hd_deps`: version selection, fetching with `git`, tree hashes, `hd.sum`.
- `hd_fmt`: the green-tree formatter.
- `hd_doc`: documentation pages for `hd doc`.
- `hd_stdpack`: the build-time std packer.
  [DESIGN.md](../../crates/hd_stdpack/DESIGN.md)
- `hd_web`: the playground's compile session for `wasm32`, over an
  in-memory store. [DESIGN.md](../../crates/hd_web/DESIGN.md)
- `hd_testkit`: the determinism matrix, test scratch directories and the
  `KNOWN_FAILURES.tsv` reader (dev only).

## Invariants

- **TIR is the contract between the halves.** The back half (`hd_mono`,
  `hd_opt`, `hd_wasm`) depends on `hd_tir` and `hd_types`, never on
  `hd_syntax`, `hd_resolve` or `hd_check`.
- **Deterministic output.** Diagnostics are reported in a sorted order,
  and Wasm bytes do not depend on the executor, the thread count
  or task order. `hd_testkit`'s matrix compares the bytes.
- **Content-addressed cache keys.** Every key is a hash of a tag and
  content fields, never a run ID. A key must cover everything its entry
  depends on, so that a hit is sound.
- **The driver does no I/O of its own.** Sources, the cache store, the
  executor and the clock come from the caller, so `hd_cli`, `hd_web` and
  the tests share one driver.
- **`unsafe_code` is denied** workspace-wide. Two modules allow it:
  `hd_base`'s `AppendVec` and `hd_web`'s exports.

## Cross-Cutting Design Docs

- [design-overview.md](design-overview.md): the pipeline, task graph and
  crate rules (§1 to §2).
- [data-structures.md](data-structures.md): IDs, interners, types, arenas,
  the data-oriented encoding and IR contracts (§3).
- [lowering-catalog.md](lowering-catalog.md): the lowering of each data
  type and syntax form, with its cost.
- [engines-and-test-runner.md](engines-and-test-runner.md): engines, tiers
  and the test runner (§18, §19 and §22).
- [testing-the-compiler.md](testing-the-compiler.md): determinism and
  soundness tests (§8, §21).
- [tiering.md](tiering.md): the dev and optimized pipelines and the pass
  architecture.
- [goals.md](goals.md): the owner's goals, the metrics and feature triage.

Open designs stay in [`docs/future-work/`](../future-work/README.md), such as
[live execution](../future-work/live-execution.md).
