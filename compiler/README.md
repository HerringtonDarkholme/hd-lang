# The hd Compiler

This directory contains the new Rust compiler. It compiles hd source through
typed IR to Wasm, then runs the module with V8 through Node. The TypeScript
compiler in `src/` is a behavior checklist during the transition, not a
library used by these crates.

Rust commands below run from `compiler/`. The workspace pins its toolchain in
[`rust-toolchain.toml`](rust-toolchain.toml).

## Crate Graph

The table lists direct workspace dependencies. External dependencies are in
the crate manifests. The design links explain each crate's intended boundary.

| Crate | Role and direct workspace dependencies | Design |
| --- | --- | --- |
| `hd_base` | IDs, spans, hashes, fuel, and shared low-level values; leaf crate | [Core data structures](../future-work/compiler/data-structures.md#3-core-data-structures) |
| `hd_intern` | Interned strings and stable paths; `hd_base` | [Interners](../future-work/compiler/data-structures.md#33-interners) |
| `hd_diag` | Diagnostic codes and records; `hd_base` | [Diagnostic records](../future-work/compiler/data-structures.md#38-diagnostic-records), [diagnostics](../future-work/compiler/checking-and-tir.md#414-diagnostics) |
| `hd_syntax` | Lexer, layout, parser, green tree, and skeletons; `hd_base`, `hd_diag`, `hd_intern` | [Front-half stages](../future-work/compiler/syntax.md#4-front-half-stages) |
| `hd_fmt` | Green-tree formatter; `hd_base`, `hd_syntax` | [Formatting command](../future-work/compiler/commands.md#76-hd-fmt-hd-fix) |
| `hd_project` | Manifests, source discovery, modules, folders, and package graph; `hd_base`, `hd_syntax` | [Discovery and folders](../future-work/compiler/resolution-and-interfaces.md#47-discovery-module-identity-and-folders) |
| `hd_types` | Type storage, inference primitives, rows, and solver data; `hd_base`, `hd_intern` | [Types](../future-work/compiler/data-structures.md#34-types), [type checking](../future-work/compiler/type-checking.md), [trait solving](../future-work/compiler/trait-solver.md) |
| `hd_host_abi` | Shared host import and ABI descriptions; `hd_base` | [Host interface](../future-work/compiler/runtime-and-host.md#17-host-interface-and-embedding) |
| `hd_resolve` | Names, folder interfaces, hashes, ownership, and visibility; `hd_base`, `hd_diag`, `hd_host_abi`, `hd_intern`, `hd_project`, `hd_syntax`, `hd_types` | [Resolution and interfaces](../future-work/compiler/resolution-and-interfaces.md) |
| `hd_tir` | Typed IR, builder, verifier, printer, and wire form; `hd_base`, `hd_intern`, `hd_types` | [TIR](../future-work/compiler/checking-and-tir.md#41311-the-typed-ir-tir), [TIR storage](../future-work/compiler/data-structures.md#318-tir) |
| `hd_check` | Body checking, exhaustiveness, facts, coherence, and initialization; `hd_base`, `hd_diag`, `hd_intern`, `hd_resolve`, `hd_syntax`, `hd_tir`, `hd_types` | [Body checking](../future-work/compiler/checking-and-tir.md#413-body-checking), [type checking](../future-work/compiler/type-checking.md), [trait solving](../future-work/compiler/trait-solver.md) |
| `hd_mono` | Reachable-instance collection and monomorphization; `hd_base`, `hd_tir`, `hd_types` | [Monomorphization and merging](../future-work/compiler/codegen.md#13-monomorphization-and-merging) |
| `hd_wasm` | Wasm GC emission and linking; `hd_base`, `hd_host_abi`, `hd_mono`, `hd_tir`, `hd_types` | [Emitting from TIR](../future-work/compiler/codegen.md#12-emitting-from-tir), [Wasm layout](../future-work/compiler/wasm-layout.md) |
| `hd_cache` | Cache keys, entry framing, memory store, and optional disk store; `hd_base` | [Cache](../future-work/compiler/cache.md#5-cache) |
| `hd_run` | Engine-neutral program loading and run outcomes; `hd_base`, `hd_cache`, `hd_wasm` | [Runtime](../future-work/compiler/runtime-and-host.md#16-runtime), [execution engines](../future-work/compiler/engines-and-test-runner.md#18-execution-engines-and-tiers) |
| `hd_run_wasmtime` | Wasmtime-side engine boundary; `hd_cache`, `hd_run` | [Wasmtime configuration](../future-work/compiler/engines-and-test-runner.md#181-wasmtime-configuration) |
| `hd_sched` | Serial, stepping, and threaded task executors; leaf workspace crate | [Scheduler](../future-work/compiler/scheduler.md) |
| `hd_driver` | Owns the task graph and assembles compiler outputs; all core front- and back-half crates | [Pipeline and task graph](../future-work/compiler/design-overview.md#1-overview) |
| `hd_doc` | Documentation extraction and rendering; `hd_base`, `hd_resolve` | [`hd doc`](../future-work/compiler/commands.md#77-hd-doc) |
| `hd_stdpack` | Build-time standard-library packer; `hd_base`, `hd_driver`, `hd_project` | [Build order, slice 4](../future-work/compiler/build-order.md#9-build-order) |
| `hd_web` | Browser adapter and stepping execution; `hd_base`, `hd_cache`, `hd_driver`, `hd_project`, `hd_run`, `hd_sched` | [Browser runner](../future-work/compiler/engines-and-test-runner.md#184-the-browser-runner), [playground](../future-work/compiler/commands.md#79-the-playground) |
| `hd_testkit` | Determinism and incremental-test support; `hd_base`, `hd_driver`, `hd_sched` | [Testing the checker](../future-work/compiler/type-checking.md#14-testing), [systems measurements](../future-work/compiler/build-order.md#92-systems-measurements-per-slice) |
| `hd_cli` | Native `hd` binary, disk sources/cache, threaded execution, and Node/V8 engine; `hd_cache`, `hd_driver`, `hd_project`, `hd_run`, `hd_sched` | [Command flows](../future-work/compiler/commands.md) |

In dependency order, the main path is:

```text
source
  -> hd_syntax -> hd_project -> hd_resolve
  -> hd_check  -> hd_tir     -> hd_mono -> hd_wasm
  -> hd_driver -> hd_run     -> hd_cli/Node
```

`hd_cache` and `hd_sched` are services supplied to `hd_driver`.
`hd_host_abi` is shared by resolution and code generation. `hd_fmt`, `hd_doc`,
`hd_stdpack`, `hd_web`, `hd_run_wasmtime`, and `hd_testkit` sit beside the
native compile-and-run path.

## Pipeline And Tasks

Stages are reported in this order. The middle stages form a dependency graph,
so rows next to each other can run concurrently rather than as one serial
pipeline.

| Stage | Task kind and unit | Result |
| --- | --- | --- |
| Discover | driver setup, one package | source and manifest inventory |
| Skim | `Skim(file)` | imports and header skeleton |
| Parse | `Parse(file)`, created on demand | tokens and green tree |
| FolderGraph | `FolderGraph`, one package | acyclic folder dependency graph |
| FolderIface | `FolderIface(folder)` | resolved, encoded public interface |
| HeaderCheck | `HeaderCheck(folder)` | bound and header diagnostics |
| ModulePrep | `ModulePrep(module)` | scope, signatures, and checker inputs |
| Body | `Body(module)` | checked bodies and TIR |
| ModuleFinish | `ModuleFinish(module)` | stable module result and cache entry |
| TestOverlay | `TestOverlay(module)` | checked `tests:` overlay |
| Coherence | `Coherence`, one task for changed trait keys | overlap diagnostics |
| InitOrder | `InitOrder(folder)` | module initialization order |
| PackageResult | `PackageResult`, one package | sorted diagnostics and package result |
| Collect | `Collect`, one program | reachable monomorphized instances |
| Emit | `Emit(instance)` | one relocatable Wasm code entry |
| Link | `Link`, one program | deterministic Wasm module |
| Precompile | `Precompile`, engine work | engine-specific compiled artifact |
| Run | `RunCase(case)`, runner work | exit, panic, or internal outcome |

The driver currently schedules through `Link`. The CLI hands linked bytes to
the Node/V8 engine directly; `Precompile` and `RunCase` remain task kinds for
the later test-runner pipeline.

## Build And Run

Build the native binary once, then use the forms implemented at this
commit. `hd FILE.hd` runs one file. `hd run [NAME]` and `hd build [FILE.hd]`
work on the package whose `hd.toml` is at or above the working directory:

```sh
cargo build -p hd_cli
./target/debug/hd samples/hello/hello.hd
(cd samples/fib && ../../target/debug/hd run)
(cd samples/fib && ../../target/debug/hd build)
```

`hd FILE.hd` and `hd run` compile and execute with Node/V8. `hd run` runs the
package's one executable (`src/main.hd`, or `main.hd` in the package
directory), and `hd run NAME` runs the executable or the task `tasks/NAME.hd`
that NAME names; `hd run FILE` is an error. `hd build` writes each executable
to `build/debug/NAME.wasm` (`build/release/` with `--release`), and
`hd build FILE.hd` writes `build/debug/files/STEM.wasm`. Outside a package,
`hd run` and `hd build` are errors that suggest `hd new`. Every rejected
command line exits with status 101. `--release` is accepted but selects only
the output directory; there is no separate release pipeline yet.

## Inspection Examples

The driver examples use in-memory sources and caches, so they do not populate
the CLI disk cache.

```sh
# Report every pipeline stage reached by a package.
cargo run -p hd_driver --example stages -- samples/hello app

# Time a generated package: N functions per folder, then thread count.
cargo run --release -p hd_driver --example bench -- 20 1

# Print selected checked TIR paths.
cargo run -p hd_driver --example tir -- samples/hello hello app/hello/main

# Parse a file or directory; add --tree to print the green tree.
cargo run -p hd_syntax --example parse_check -- samples/hello
cargo run -p hd_syntax --example parse_check -- --tree samples/hello/hello.hd
```

The `bench` invocation is wired but is not usable at this commit: its generated
`main` calls `println` without declaring `$ Console`, so checking stops with
`missing-requirement`. This is recorded in `audit/codex_task.md`; do not treat
its partial timing as a benchmark result.

## Tests And Snapshots

- Unit tests live beside each crate's source. Integration tests live under
  `crates/*/tests/`.
- Parser corpus, fuzz, and tree snapshots are under
  `crates/hd_syntax/tests/`. `.tree` files pair with `.hd` inputs.
- Checker, runtime, incremental, and standard-library stage tests are under
  `crates/hd_driver/tests/`. Golden TIR is in
  `crates/hd_driver/tests/tir/`.
- CLI compile/run samples are in `samples/` and exercised by
  `crates/hd_cli/tests/run.rs`.
- The implementation-neutral suite is in `../spec/conformance/`; temporary
  disagreements with this compiler are listed in
  [`KNOWN_FAILURES.tsv`](KNOWN_FAILURES.tsv).

From `compiler/`, run the Rust workspace tests with `cargo test --workspace`.
Only update checked-in output after reviewing the diff:

```sh
HD_BLESS=1 cargo test -p hd_syntax --test snapshots
HD_UPDATE_GOLDEN=1 cargo test -p hd_driver --test checker std_bodies_match_golden_tir
```

`HD_BLESS=1` rewrites parser `.tree` snapshots.
`HD_UPDATE_GOLDEN=1` rewrites the selected `.tir` golden files.

## Pre-Commit Hook

The repository hook is [`.githooks/pre-commit`](../.githooks/pre-commit).
`pnpm install --frozen-lockfile` runs the root `prepare` script, which sets
`core.hooksPath` to `.githooks`. When staged Rust or compiler manifest files
change, the hook runs `cargo fmt --all --check` and workspace Clippy with
warnings denied. It does nothing for a documentation-only commit.

## Disk Cache

The CLI stores compiled objects in an `obj` directory:

| Environment | Location |
| --- | --- |
| `HD_CACHE` set | `$HD_CACHE/obj` |
| macOS default | `$HOME/Library/Caches/hd/obj` |
| Other Unix with `XDG_CACHE_HOME` | `$XDG_CACHE_HOME/hd/obj` |
| Other Unix fallback | `$HOME/.cache/hd/obj` |

There is no cache-cleaning subcommand yet. Stop running `hd`, then delete only
that `obj` directory. For disposable development runs, make the target
unambiguous:

```sh
HD_CACHE=/private/tmp/hd-cache ./target/debug/hd samples/hello/hello.hd
rm -rf /private/tmp/hd-cache/obj
```

The next compile recreates it.

## Status And Planning

- [Implemented footprint](../future-work/compiler/footprint.md)
- [Code/design reconciliation](../future-work/compiler/reconciliation.md)
- [Remaining work estimate](../future-work/compiler/work-estimate.md)
- [Phase-2 job briefs](../future-work/compiler/phase2-jobs.md)
