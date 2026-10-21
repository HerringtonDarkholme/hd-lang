# The hd Compiler

The Rust compiler is the Cargo workspace at the repository root. It
compiles hd source through typed IR to Wasm, then runs the module on V8
through Node, or in process on wasmtime. Rust commands below run from the
repository root. The toolchain is pinned in
[`rust-toolchain.toml`](../rust-toolchain.toml).

The [architecture map](../docs/compiler/ARCHITECTURE.md) explains the
pipeline, the crate boundaries, and the design docs. Direct dependencies are
in each crate's `Cargo.toml`.

## Crates

| Crate | Role | Design |
| --- | --- | --- |
| `hd_base` | IDs, spans, hashes, fuel, build profile, engine target and level; leaf crate | [Core data structures](../docs/compiler/data-structures.md#3-core-data-structures) |
| `hd_intern` | Interned strings and stable paths | [Interners](../docs/compiler/data-structures.md#33-interners) |
| `hd_diag` | Diagnostic codes and records | [Diagnostic records](../docs/compiler/data-structures.md#38-diagnostic-records), [diagnostics](hd_tir/DESIGN.md#414-diagnostics) |
| `hd_syntax` | Lexer, layout, parser, green tree, and skeletons | [Front-half stages](hd_syntax/DESIGN.md#4-front-half-stages) |
| `hd_fmt` | Green-tree formatter | [`hd fmt`, `hd fix`](hd_cli/DESIGN.md#76-hd-fmt-hd-fix) |
| `hd_project` | Manifests, source discovery, modules, folders, package graph | [Discovery and folders](hd_resolve/DESIGN.md#47-discovery-module-identity-and-folders) |
| `hd_types` | Type storage, inference primitives, rows, solver data | [Types](../docs/compiler/data-structures.md#34-types), [type checking](hd_check/DESIGN.md) |
| `hd_host_abi` | Host import and ABI descriptions | [Host interface](hd_run_wasmtime/DESIGN.md#17-host-interface-and-embedding) |
| `hd_resolve` | Names, folder interfaces, hashes, ownership, visibility | [Resolution and interfaces](hd_resolve/DESIGN.md) |
| `hd_tir` | Typed IR, builder, verifier, printer, wire form | [TIR](hd_tir/DESIGN.md#41311-the-typed-ir-tir), [TIR storage](../docs/compiler/data-structures.md#318-tir) |
| `hd_structure` | The compiler-supplied `Structure` generator for derivations and tuples | [Type checking](hd_check/DESIGN.md) |
| `hd_check` | Body checking, trait solving, exhaustiveness, facts, coherence, initialization | [Body checking](hd_tir/DESIGN.md#413-body-checking), [trait solving](hd_check/DESIGN.md#new-compiler-the-trait-solver) |
| `hd_mono` | Reachable-instance collection and monomorphization | [Monomorphization and merging](hd_mono/DESIGN.md#13-monomorphization-and-merging) |
| `hd_opt` | Pass manager and pipelines keyed by (engine target, level) | [Optimization pipelines](hd_opt/DESIGN.md) |
| `hd_wasm_linear` | The linear-memory backend, the default: reference-counted values in linear memory, one module per program | [Linear-memory layout](hd_wasm_linear/DESIGN.md#23-linear-memory-layout) |
| `hd_cache` | Cache keys, entry framing, memory and disk stores | [Cache](hd_cache/DESIGN.md#5-cache) |
| `hd_deps` | Remote dependencies: version selection, `git` fetching, tree hashes, `hd.sum` | [Dependencies](../spec/cli/command-line.md#dependencies) |
| `hd_run` | Engine-neutral program loading and run outcomes | [Runtime](hd_run_wasmtime/DESIGN.md#16-runtime), [execution engines](../docs/compiler/engines-and-test-runner.md#18-execution-engines-and-tiers) |
| `hd_run_wasmtime` | The wasmtime engine and its host providers | [Runtime and host interface](hd_run_wasmtime/DESIGN.md) |
| `hd_sched` | Serial, stepping, and threaded task executors; leaf crate | [Scheduler](hd_sched/DESIGN.md) |
| `hd_driver` | The task graph of a run; assembles compiler outputs | [Pipeline and task graph](../docs/compiler/design-overview.md#1-overview) |
| `hd_doc` | Documentation extraction and rendering | [`hd doc`](hd_cli/DESIGN.md#77-hd-doc) |
| `hd_stdpack` | Build-time standard-library packer | [Standard-library bootstrap](hd_stdpack/DESIGN.md) |
| `hd_web` | Playground compile session over an in-memory store | [Playground on Wasm](hd_web/DESIGN.md) |
| `hd_testkit` | Determinism and incremental-test support, and the `KNOWN_FAILURES.tsv` reader | [Testing the compiler](../docs/compiler/testing-the-compiler.md) |
| `hd_cli` | The `hd` binary: commands, disk sources and cache, Node engine, REPL | [Command flows](hd_cli/DESIGN.md) |

## Build And Run

```sh
cargo build -p hd_cli
./target/debug/hd examples/hello/hello.hd
(cd examples/fib && ../../target/debug/hd run)
(cd examples/fib && ../../target/debug/hd build)
```

`hd help` lists the commands and `hd help COMMAND` a command's flags. The
rules for each command are in [spec/cli/](../spec/cli/README.md), and the
day-to-day use is in [The `hd` Command](../website/guide/COMMANDS.md).
`--release` selects the release profile, whose integer arithmetic wraps and
whose shift counts are masked; `hd test --release` stays checked.

## Wasmtime Engine

Node is the default engine. The in-process wasmtime engine
(`--engine wasmtime`, `HD_ENGINE=wasmtime`) is the `wasmtime` cargo feature
of `hd_cli`, off by default, because wasmtime and cranelift are most of a
cold build (a dev `hd` build is about 8 s without them and 27 s with them).
`hd_run_wasmtime` is also outside the workspace's `default-members`, so plain
`cargo build`, `cargo test` and `cargo clippy` never compile it.

```sh
cargo build --bin hd                       # Node only
cargo build --bin hd --features wasmtime   # Node and wasmtime
```

An `hd` built without the feature refuses the engine when it reads the name,
with `this build of hd has no wasmtime engine; rebuild with --features
wasmtime`. It never falls back to Node.

Every build that runs is tuned for the engine that runs it
([optimization pipelines](hd_opt/DESIGN.md), §5): `hd run`,
`hd FILE`, `hd test` and `hd build` take the engine from `--engine`, else
`HD_ENGINE`, else Node, so `hd build --engine wasmtime` writes a module
tuned for wasmtime, which `hd --engine wasmtime FILE.wasm` runs. Every
target's module runs on every engine; a mismatch costs only speed. The
REPL builds for wasmtime and `hd_web` for the browser.

The REPL (`hd` with no arguments on a terminal, `hd_cli/src/repl.rs`) runs on
wasmtime only, so its line editor (`rustyline`) and Ctrl-C handler (`ctrlc`)
come with the feature too. An `hd` without it says the REPL needs wasmtime
and exits with status 101; piped standard input still runs as a program.

The tests that need wasmtime run only with the feature. `hd_driver` has a
`wasmtime` feature of its own for them: the wasmtime modes of its
conformance test (`HD_CONFORMANCE_ENGINE=wasmtime` and `wasmtime-only`) and
the float text comparison. In those modes each engine runs a build for its
own target, so the compare mode builds each program for `V8` and for
`Wasmtime`; CI runs it, and a local run without the variable tests the `V8`
target on Node only. The wasmtime modes of `cli_conformance` need an
`hd` built with the feature. Without it those modes are refused and the
other tests run unchanged.

```sh
cargo test --workspace --all-features      # everything, as CI runs it
cargo test --features wasmtime             # default members plus wasmtime
cargo test -p hd_run_wasmtime              # the wasmtime crate alone
HD_CONFORMANCE_ENGINE=wasmtime cargo test --features wasmtime -p hd_driver --test conformance
```

## Inspection Examples

The driver examples use in-memory sources and caches, so they do not populate
the CLI disk cache.

```sh
# Report every pipeline stage reached by a package.
cargo run -p hd_driver --example stages -- examples/hello app

# Time a generated package: N functions per folder, then thread count.
cargo run --release -p hd_driver --example bench -- 20 1

# Print selected checked TIR paths.
cargo run -p hd_driver --example tir -- examples/hello hello app/hello/main

# Parse a file or directory; add --tree to print the green tree. It prints
# each diagnostic as hd check does and exits 101 when any file has one.
cargo run -p hd_syntax --example parse_check -- examples/hello
cargo run -p hd_syntax --example parse_check -- --tree examples/hello/hello.hd
```

## Tests And Stage Cases

- Unit tests live beside each crate's source. Integration tests live under
  `crates/*/tests/`.
- Compiler output is tested with stage cases: a plain `.hd` file under
  `crates/<crate>/tests/cases/<topic>/` with its generated output under
  `tests/baselines/<topic>/`. `hd_syntax` (tokens, tree), `hd_check` (tir),
  `hd_wasm_linear` (wat) and `hd_driver` (counts, layout) each run theirs
  with `cargo test -p <crate> --test stages` (`hd_syntax`:
  `cargo test -p hd_syntax --test all stages::`). The harness is
  [`hd_testkit/src/stages.rs`](hd_testkit/src/stages.rs); the format, views
  and how to write a case are in the
  [compiler-tests skill](../.agents/skills/compiler-tests/SKILL.md).
- Parser corpus, fuzz, and tree snapshots are under
  `crates/hd_syntax/tests/`. `.tree` files pair with `.hd` inputs.
  Checker, runtime, incremental, and standard-library tests are under
  `crates/hd_driver/tests/`. Golden TIR of a few std bodies is in
  `crates/hd_driver/tests/tir/`.
- CLI compile/run samples are in `examples/` and exercised by
  `crates/hd_cli/tests/all/run.rs`.
- The implementation-neutral suite is in `spec/conformance/`. Every
  indexed case must pass except a row of
  [`KNOWN_FAILURES.tsv`](../KNOWN_FAILURES.tsv) (`path`, `issue`,
  `engine`, `reason`), and a listed case that passes fails too. Both
  tiers (`hd_driver/tests/conformance.rs`, `hd_cli/tests/cli_conformance.rs`)
  print their counts.
  Both build with the linear backend, the default.
  `HD_CONFORMANCE_CHECKED=1 cargo test -p hd_driver --test
  conformance` builds every case in checked mode instead, so a leak or a
  count change on a freed block fails it.

From the repository root, `cargo test` runs the default members, which
leave out `hd_run_wasmtime` and the tests that need it. `cargo test --workspace
--all-features` runs everything (see [Wasmtime Engine](#wasmtime-engine)).
Only update checked-in output after reviewing the diff. `HD_BLESS=1` is the
one variable that accepts new output: stage baselines, `.tree` snapshots,
the golden std TIR
(`checker::std_bodies_match_golden_tir`) and the generated host ABI files
(`cargo test -p hd_host_abi --test abi_model`).

```sh
HD_BLESS=1 cargo test -p hd_check --test stages
HD_BLESS=1 cargo test -p hd_driver --test all checker::std_bodies_match_golden_tir
```

## Pre-Commit Hook

The repository hook is [`.githooks/pre-commit`](../.githooks/pre-commit).
`pnpm install --frozen-lockfile` runs the root `prepare` script, which sets
`core.hooksPath` to `.githooks`. When staged Rust or compiler manifest files
change, the hook runs `cargo fmt --all --check` and
`cargo clippy --all-targets -- -D warnings` on the default members, so a commit
never builds wasmtime. CI lints with `--workspace --all-features`. The hook
does nothing for a documentation-only commit.

## Disk Cache

The CLI stores compiled objects in an `obj` directory:

| Environment | Location |
| --- | --- |
| `HD_CACHE` set | `$HD_CACHE/obj` |
| macOS default | `$HOME/Library/Caches/hd/obj` |
| Other Unix with `XDG_CACHE_HOME` | `$XDG_CACHE_HOME/hd/obj` |
| Other Unix fallback | `$HOME/.cache/hd/obj` |

`hd cache gc` evicts the least recently used entries down to the size cap
(10 GiB, or `HD_CACHE_MAX_SIZE`). `hd clean --cache` removes fetched
dependency versions, not compiled objects. To drop compiled objects, stop
running `hd` and delete only the `obj` directory. For disposable development
runs, make the target unambiguous:

```sh
HD_CACHE=/private/tmp/hd-cache ./target/debug/hd examples/hello/hello.hd
rm -rf /private/tmp/hd-cache/obj
```
