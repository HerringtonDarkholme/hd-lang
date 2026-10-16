# New Compiler: Detailed Design

Status: design record; may lag the code. The code and the spec win.
Each "Implementation differs" note marks a place where the code is
behind this design.

Part of the [compiler design](ARCHITECTURE.md).

## Terms

| Term | Meaning |
| --- | --- |
| file | one `.hd` source file |
| module | one file's module ([`module.path.name`](../../spec/lang/10-modules.md#r-module.path.name)); the **cache unit** |
| folder | the directory a module belongs to ([Folders](../../spec/lang/10-modules.md#folders)); the **interface unit** |
| package | a manifest's library, executables, tests and tasks |
| item | a declaration: function, method, data, enum, trait, impl, alias |
| body | the executable part of an item, a top-level statement group, or a `tests:` block |
| skeleton | what the header pass extracts from one file: hashes, uses, body ranges |
| interface blob | a folder's serialized interface |
| deep hash | a folder's interface hash folded with the deep hashes of every folder its blob mentions |
| key | a 128-bit content hash naming a cache entry |
| TIR | the one typed IR per body ([`crates/hd_tir/DESIGN.md`](../../crates/hd_tir/DESIGN.md)) |

## 1. Overview

### 1.1 The Pipeline

```text
 SourceSet ──► Skim(f), per file ──► FolderGraph (uses only; cycles)
                                          │
                     FolderIface(F) per folder, in DAG order ═══► [iface entry]
                       (resolve uses + headers, blob, deep hash)
                                          │ frozen interfaces + impl tables
              ┌───────────────────────────┼──────────────────────────────┐
     HeaderCheck(F) ═► [graph: hdr]   ModulePrep(m)              Coherence ═► [graph: coh]
                                          │
                                   Body(m): every body of the module
                                          │
                                   ModuleFinish(m) ═══════════════════► [check entry]
                                          │
                                   PackageResult ──► InitOrder(F) ═► [graph: init]
                                          │
                                   Collect ─► Emit per instance ═► [code] ─► Link ═► [link]
```

`═══►` marks a cache boundary. Every boundary is content-addressed through
the `CacheStore` interface
([crates/hd_cache/DESIGN.md](../../crates/hd_cache/DESIGN.md)). Nothing
crosses a boundary by an in-memory ID. `Body(m)` is one scheduled task per
module; the individual bodies remain the logical units for determinism,
fuel and TIR.

Planned, not built: a stat manifest in `build/` that lets a warm run
skip hashing unchanged files, and a `pkgres` entry that answers a warm
run with no edit from one key.

### 1.2 Stages

| Stage | Input | Output | Unit | Cached | Key |
| --- | --- | --- | --- | --- | --- |
| Skim (header pass) | file bytes | `HeaderSkeleton` | file | dependency packages only, in `depfiles` | |
| Full parse | file bytes | `TokenBuf`, `GreenTree`, item index | file | a `parse` entry records that a source parses cleanly | `parse_key` |
| Folder graph | use lists of every file | folder DAG, `folder-cycle` | package | no | |
| Folder interface | the folder's files, interfaces of used folders | `FolderIface`, blob, deep hash, header diagnostics | folder | `iface` | [§5.3](../../crates/hd_cache/DESIGN.md#53-key-composition) |
| Header check | a folder's interface and its closure's interfaces | header bound, supertrait, newtype-base and delegation diagnostics | folder | one `graph` entry per folder | `hdr_key` |
| Module prep | module tree, interfaces of the folder's closure | module scope, inferred results | module | inside `check` | |
| Body check | body tree, frozen tables | TIR, diagnostics, facts | module (all its bodies) | inside `check` | |
| Module finish | the module's body results | the `check` entry | module | `check` | `check_key` |
| Test overlay | `tests:` block and doc tests of a module | TIR, diagnostics | module | see below | |
| Coherence | impl heads of the traits whose `coh_key` changed | `overlapping-impl` | one task per changed trait | `coh` part of `graph` | trait path plus sorted head hashes |
| Init order | init summaries of a folder's modules | statement order, `top-level-read-before-initialization` | folder | `init` part of `graph` | `init_key` |
| Package result | all of the above | sorted diagnostics, summary | package | no | |
| Collect, Emit, Link | TIR, interfaces | Wasm | program, instance, program | `codepack` per folder group, `link` per program | `code_key`, `prog_key` |

Implementation differs: one `Coherence` task, not one per trait
(`crates/hd_driver/src/lib.rs`, `fn coherence`). It runs when any
interface changes, and checks again only the traits whose `coh_key`
changed, reading the rest from the package's coherence rows
(`crates/hd_cache/DESIGN.md` §5.3).

Implementation differs: under `hd test` and `hd check --tests` the
package's modules are checked with their `tests:` blocks into `check`
entries of role `test`; no `check-test` entry is written; not judged.

### 1.3 The Task Graph

```text
Skim(f) ──► FolderGraph ──► FolderIface(F) ──► ModulePrep(m) ──► Body(m) ──► ModuleFinish(m) ──┐
                │              │    └──► HeaderCheck(F) ─────────────────────────────────────────┤
                │              └──► Coherence ───────────────────────────────────────────────────┤
                └── FolderIface(G) for each folder G that F uses                                 ▼
                                                     Collect ◄── InitOrder(F) ◄── PackageResult
```

- `FolderGraph` creates the rest of the front half. A `FolderIface(F)`
  task waits for the interfaces of the folders `F` uses, never for
  bodies, so a deep chain of folders does not stall wide levels.
- A `HeaderCheck(F)` task waits for the interfaces of F's closure. No
  check waits for it; `PackageResult` does.
- A `ModulePrep(m)` task waits for the interfaces of its folder's
  closure, then creates `Body(m)` and `ModuleFinish(m)`.
- The driver interns one `ImplUniverseId` per solving context: the
  module's bodies, its test overlay, each `HeaderCheck(F)` and each
  derive instance ([crates/hd_sched/DESIGN.md §6.1](../../crates/hd_sched/DESIGN.md)).
  It adds no edge to the graph.
- Bodies never wait on each other, with one exception: a private
  function with an omitted result type, which `ModulePrep` infers first.
- `PackageResult` waits for every header check, `Coherence` and module
  finish. It creates one `InitOrder(F)` per built
  folder, and, for a program with no error, `Collect` after them.
  `Collect` creates one `Emit` per folder group with a code-entry miss
  and a `Link` after them.
- A file is parsed on demand, by the task that first needs its tree,
  and at most once per run. `hd check` and `hd doc` also add a
  `Parse(f)` task per file.
- On a warm run a cache hit at a boundary skips the work below it.

### 1.4 What A Run Touches

| Situation | Files read | Tasks run | Entries written |
| --- | --- | --- | --- |
| private body edit in module `m` | every source file (hashing) | `FolderIface` is a key hit (api text unchanged); prep, bodies and finish of `m` | `check` of `m` |
| private header edit in `m` | as above | as above, plus `FolderIface` of `m`'s folder, which yields the same deep hash | `iface`, `check` of `m` |
| public signature edit in `m` | as above | `FolderIface` of the folder and of folders whose deep hash changes; every module whose key includes a changed deep hash | those entries |

The first row is the `recheck-precision` target: one module. The count
is module `check` entries computed, the summary's `modules_checked`.
Other necessary work is reported separately:

- an edit to a top-level statement in a multi-module init group also
  reruns that folder's `InitOrder`;
- any interface change reruns `Coherence` (§1.2);
- `hd build` and `hd test` also re-emit the changed instances.

The target holds for an edit that changes no exported semantic
information. Work that correctness needs is never skipped to meet it.

## 2. Crate Graph

### 2.1 Crates

The crates, their roles and their layers are in
[ARCHITECTURE.md](ARCHITECTURE.md); each crate's direct dependencies are
in its `Cargo.toml`. Small crates in a strict layer order mean most
compiler edits rebuild one crate. `hd_base` and `hd_sched` are leaf
crates, and the back half (`hd_mono`, `hd_opt`, `hd_wasm`) depends on
`hd_tir` and `hd_types`, never on the front half.

### 2.2 Rules

1. **Leaf crates off the hot path.** `wasmtime` and Cranelift sit only in
   `hd_run_wasmtime`; `rayon` only behind `hd_sched`'s `threads` feature.
   File-system access is in `hd_cache`'s `disk` feature, `hd_cli`,
   `hd_deps`, `hd_run_wasmtime` and `hd_base::path`'s canonicalizing
   helpers. A front-half edit never rebuilds `hd_run_wasmtime`.
2. **Generated code is checked in.** The diagnostic code enum
   (`crates/hd_diag/src/codes.rs`) is generated from the spec's code list
   by `spec/tools/diagnostic-codes.ts`. `hd_driver`'s `build.rs` embeds
   `lib/std`'s sources. Planned, not built: a std pack of prebuilt
   interface blobs (`hd_stdpack`) in place of the embedded sources.
3. **The driver owns no host facilities.** `hd_driver` takes its sources,
   cache, executor and clock from the caller; it has no `std::fs`,
   `std::process` or `Instant`. Sources come through `SourceSet`, the
   cache through `CacheStore`, work through the executor, and time
   through the supplied clock. The browser build then needs no stubs.
4. **No `serde` in the compiler.** Blob and entry formats are
   hand-written over `hd_base::wire`. `toml` parses manifests only;
   `serde_json` is used only by `hd_run_wasmtime`.
5. **The browser build compiles in CI:**
   `cargo build -p hd_syntax -p hd_web --target wasm32-unknown-unknown`.
