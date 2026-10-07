# New Compiler: Detailed Design

Part of the [compiler design](README.md).

Status: Design, not decided. Both parts are done, 2026-10-07: Part D1,
the pipeline and front half, and Part D2, the back half. D2 also revised
D1 after the owner's review (see [Changes To D1](open-questions.md#changes-to-d1)).

## How To Read This

This is the design that follows the research. It does not reopen what the
owner decided. Its inputs:

- [goals.md](goals.md): goals,
  Arena pillars, goal metrics, feature triage, and the Decided list.
- [research.md](research.md):
  the research, and the owner's answers in
  [Open Questions For The Owner](research.md#open-questions-for-the-owner).
- [prior-art-issues.md](prior-art-issues.md): known
  issues of prior front ends. Its [Lessons For hd](prior-art-issues.md#lessons-for-hd)
  and its "Changes Suggested" list are design requirements here.
- The language specification, chapters 1 to 4, 9 to 14, and the
  [CLI chapter](../../spec/cli/command-line.md). The spec stays authoritative.

Conventions:

- **Mine** marks a design idea of this document, not shipped practice and
  not in the research.
- **D1** is sections 1 to 10: the pipeline, core data structures, the
  front half up to TIR, the cache, the scheduler and command flows.
  **D2** is sections 11 to 23: monomorphization, suspension lowering,
  Wasm GC emission, link, runtime, host interface, test runner.
- **TIR** is the one typed IR per body (§4.13.11). D1 originally planned
  a checked tree (THIR) and a separate MIR; D2 merged them (§3.9.1).
- Rust code is a sketch of shape and ownership, not final code.
- "First release" is the first shipped compiler. "Later" is after it.

Terms used throughout:

| Term | Meaning |
| --- | --- |
| file | one `.hd` source file |
| module | one file's module ([`module.path.name`](../../spec/lang/10-modules.md#r-module.path.name)); the **cache unit** |
| folder | the directory a module belongs to ([Folders](../../spec/lang/10-modules.md#folders)); the **interface unit** |
| package | a manifest's library, executables, tests and tasks |
| item | a declaration: function, method, data, enum, trait, impl, alias |
| body | the executable part of an item, a top-level statement group, or a `tests:` block; the **parallel task unit** |
| skeleton | what the header pass extracts from one file: uses, headers, body ranges |
| interface blob | a folder's serialized, indexed interface |
| deep hash | a folder's interface hash folded with the deep hashes of every folder its blob mentions |
| key | a 128-bit content hash naming a cache entry |

## 1. Overview

### 1.1 The Pipeline

```text
                     ┌─────────────────── front half (this part) ───────────────────┐
 SourceSet ──► discover + stat ──► manifest diff
                     │                  │ unchanged files: hash, use list, api hash from manifest
                     │                  ▼ changed files
                     │        ┌── skim (header pass) ──► Skeleton ─┐   per file, parallel
                     │        └── full parse ─► tokens + green tree ┘   (changed or needed files)
                     ▼                                │
              folder graph (uses only; cycles)        │
                     │                                ▼
                     └──────────► FolderIface task per folder, in DAG order ═══► [iface entry]
                                    (resolve uses + headers, derive heads,         deep hash
                                     validate, blob, per-item hashes)                 │
                                              │ frozen interfaces + impl tables       ▼
                                              ▼                            module check keys
                              ModulePrep per module (scope, private sigs,        │ miss
                                inferred results)                                 ▼
                                              │
                              Body(m), one task per module
                                (bodies are the logical unit; frozen tables)
                                              │
                              ModuleFinish per module (rows, sort) ═══════► [check entry]
                                              │
              Coherence for changed coh_key traits ═► [graph: coh parts]
                       (one task per trait)          InitOrder per folder ═► [graph: init parts]
                                              │
                              Output assembly in content order ═══════════► [pkgres entry]
                     └────────────────────────┼────────────────────────────────────────┘
                                              ▼
                       D2: TIR ─► collect ─► emit Wasm per instance ─► link ─► run / test
```

`═══►` marks a cache boundary. Every boundary is content-addressed through
the `CacheStore` interface (section 5). Nothing crosses a boundary by an
in-memory ID. `Body(m)` is one scheduled task per module; the individual
bodies remain the logical units for determinism, fuel, cache keys and TIR
([reconciliation, item 1](reconciliation.md#design-changes-proposed)).

### 1.2 Stages

| Stage | Input | Output | Unit | Parallel | Cached | Key |
| --- | --- | --- | --- | --- | --- | --- |
| Discover and stat | package root, manifest, `SourceSet` | file list with path, size, mtime, content hash | package | hashing in parallel | stat manifest in `build/` (local, not shared) | none |
| Skim (header pass) | file bytes | `Skeleton` | file | yes | no; the manifest keeps its use list and api hash | none |
| Full parse | file bytes | `TokenBuf`, `GreenTree` | file | yes | no | none |
| Folder graph | use lists of every file | folder DAG, `folder-cycle` | package | serial (tiny) | inside `pkgres` | none |
| Folder interface | skeletons of the folder, deep interfaces of used folders | `FolderIface`, blob, deep hash, header diagnostics | folder | yes, in DAG order | `iface` | [§5.3](cache.md#53-key-composition) |
| Header check (stage B) | a folder's interface, its dependencies' interfaces and impl tables | header bound, supertrait, newtype-base and delegation diagnostics | folder | yes | `hdr` part of `graph` | [§5.3](cache.md#53-key-composition) |
| Module prep | module CST, own folder interface, used interfaces | module scope, private signatures, inferred results, the closure bit set and `ImplUniverseId` | module | yes, across modules | inside `check` | none |
| Body check | body CST, frozen tables | TIR (§4.13.11), diagnostics, facts | body | yes | inside `check`; TIR in `tir` | none |
| Module finish | the module's body results | `ModuleResult` | module | yes, across modules | `check` | [§5.3](cache.md#53-key-composition) |
| Test overlay | `tests:` block and doc tests of a module | TIR, diagnostics | module | yes | `check-test` | check key plus test uses |
| Coherence | impl heads of the traits whose `coh_key` changed | `overlapping-impl` | one task per changed trait | yes | `coh` part of `graph` | trait path plus sorted head hashes |
| Init order | init summaries of a folder's modules | statement order, `top-level-read-before-initialization` | folder (only when a group spans modules) | yes | `init` part of `graph` | sorted summary hashes |
| Package result | all of the above | sorted diagnostics, summary | package | serial | `pkgres` | sorted keys of the parts |
| D2 stages | TIR, interfaces | Wasm | instance, program | see §11.2 | see §11.2 | see §11.2 |

### 1.3 The Task Graph

```text
Skim(f) ──► FolderGraph ──► FolderIface(F) ──► ModulePrep(m) ──► Body(m) ──► ModuleFinish(m) ──► PackageResult
               │              │ miss              │ miss                              │
               │              └── creates Parse(f)└── creates Parse(f)                ├──► InitOrder(F)
               │                 for files read       for files read                  │
               │              │    └──► Coherence(trait with changed coh_key) ◄───────┘
               └── FolderIface(G) for each folder G that F uses
```

- A `FolderIface(F)` task waits for the interfaces of the folders `F` uses,
  never for bodies. A deep chain of folders does not stall wide levels.
- A `HeaderCheck(F)` task (stage B) waits for the same interfaces as
  `FolderIface(F)` plus F's own. No check waits for it; `PackageResult`
  and codegen's `Collect` do (scheduler.md §6.1).
- After M1 builds a module's closure bit set, the driver interns one
  `ImplUniverseId` per solving context: the module's bodies, its test
  overlay, each `HeaderCheck(F)` and each derive instance
  (scheduler.md §6.1). It adds no edge to the graph.
- A `Body` task waits only for its `ModulePrep`. Bodies never wait on each
  other, with one exception: a private function with an omitted result
  type, which `ModulePrep` infers first (§4.13.1).
- On a warm run most tasks never exist: a cache hit at a boundary removes
  the tasks below it.
- `Parse(f)` is created by the `FolderIface(F)` or `ModulePrep(m)` task
  that misses and needs the file; it is not a static predecessor. A file
  is parsed at most once per run
  ([reconciliation, item 1](reconciliation.md#design-changes-proposed)).

### 1.4 What A Run Touches

| Situation | Files read | Tasks run | Entries written |
| --- | --- | --- | --- |
| warm, no edit | the manifest; no source file | none: the fast-path key hits `pkgres` | none |
| private body edit in module `m` | `m` | parse `m`; `FolderIface` is a key hit (api text unchanged); prep, bodies and finish of `m` | `check` of `m`, `pkgres` |
| private header edit in `m` | `m` | as above, plus `FolderIface` of `m`'s folder, which yields the same deep hash | `iface`, `check` of `m`, `pkgres` |
| public signature edit in `m` | `m`, plus files of folders whose interface changes | `FolderIface` of the folder and of folders whose deep hash changes; every module whose key includes a changed deep hash | those entries |
| first run in a new worktree of a known commit | every source file once (hashing) | none if another worktree checked the same content | the local manifest |

The second row is the `recheck-precision` target: one module.

**What is counted, and the exceptions** (Codex review, P6). The count is
module `check` entries computed, the JSON summary's `modules_checked`.
Other necessary work is not hidden by it, and is reported separately:

- an edit to a top-level statement in a multi-module init group also
  reruns that folder's `InitOrder`;
- a private helper named by an exported template, and a body that a fact
  depends on, can change what dependents check. Their contracts are open
  in the frontend lane (Codex findings 4 and 6). Until they close, such
  an edit is outside the one-module target;
- `hd build` and `hd test` also re-emit the changed instances (§13.8).

The target holds for an edit that changes no exported semantic
information. Work that correctness needs is never skipped to meet it.

## 2. Crate Graph

### 2.1 Crates

Small crates in a strict layer order, so most compiler edits rebuild one
crate ([Q1 risks](research.md#risks)). This refines
[Q7](research.md#proposed-crates).

| Crate | Responsibility | Depends on | Browser |
| --- | --- | --- | --- |
| `hd_base` | ID newtype macro, `Hash128` and `StableHasher`, `Span`, `FileId`, `Fuel`, small collections, the counting allocator | `xxhash-rust`, `smallvec`, `hashbrown` | yes |
| `hd_intern` | sharded string interner, stable-path interner, `DefId` table | `hd_base` | yes |
| `hd_diag` | `Diagnostic`, the `Code` enum generated from the spec's code list, fix-its, compact and JSON renderers | `hd_base` | yes |
| `hd_syntax` | lexer with skim mode, layout cursor, parser, green tree, typed views, skeletons | `hd_base`, `hd_intern`, `hd_diag` | yes |
| `hd_fmt` | the formatter on the green tree | `hd_syntax` | yes |
| `hd_project` | manifests, module discovery and identity, folders, folder and package graphs, the `SourceSet` trait | `hd_syntax`, `toml` | yes |
| `hd_types` | type interner, local type arenas, rows, unification, poison, the trait solver core and its memo | `hd_base`, `hd_intern` | yes |
| `hd_resolve` | use resolution, folder interface builder and codec, per-item and deep hashes, derived heads, orphan and visibility checks | `hd_types`, `hd_project` | yes |
| `hd_tir` | TIR: the generated tags, views and builder, the verifier, serialization (§4.13.11) | `hd_types` | yes |
| `hd_check` | body checker emitting TIR, exhaustiveness, templates, coherence, init order, facts | `hd_resolve`, `hd_tir` | yes |
| `hd_cache` | `CacheStore` trait, keys, entry framing, memory store, disk store (feature `disk`), stat manifest, trim | `hd_base` | memory store only |
| `hd_sched` | `Scheduler` trait, task graph, serial executor, thread pool executor (feature `threads`), budgets, memory cap | `hd_base`, `rayon` (feature) | serial only |
| `hd_driver` | `Session`, command pipelines (check, test plan, doc, fix), output assembly | all of the above | yes |
| `hd_doc` | `hd doc` rendering ([HD_DOC.md](../HD_DOC.md)) | `hd_driver` | yes |
| `hd_stdpack` | build-time tool: checks `lib/std` and writes the std pack that `hd_driver` embeds | `hd_driver` | the pack, not the tool |
| D2 crates | `hd_mono`, `hd_wasm`, `hd_host_abi`, `hd_run` (§11.4) | `hd_tir`, `hd_types`, `hd_host_abi`; `hd_run` also depends on `hd_cache` | yes |
| `hd_run_wasmtime` | wasmtime embedding (D2) | `hd_run`, `wasmtime` | no |
| `hd_cli` | the native `hd` binary: argument parsing, the disk cache, threads, terminal output | everything native | no |
| `hd_web` | wasm-bindgen glue, the JS `SourceSet` and `CacheStore`, the stepping scheduler | `hd_driver`, D2 crates, `wasm-bindgen` | only there |
| `hd_testkit` | dev only: determinism matrix, edit-script fuzzer, differential harness | `hd_driver` | no |

```text
hd_base ─► hd_intern ─► hd_diag ─► hd_syntax ─► hd_fmt
                │                     │
                │                     └──► hd_project ─┐
                └────────────► hd_types ─────────────┴─► hd_resolve ─► hd_check ─┐
                └────────────► hd_cache                                            │
hd_base ─► hd_sched ────────────────────────────────────────────────────────────────┤
                                                                                     ▼
                                          hd_driver ─► hd_doc ────────────────► hd_cli ◄─ hd_run_wasmtime
                                          hd_tir, hd_types, hd_host_abi ─► D2 crates ─┤
                                                                          └─────────► hd_web
```

D2 consumes `hd_tir`, `hd_types` and `hd_host_abi`, not `hd_driver`;
`hd_run` also consumes `hd_cache`
([reconciliation, item 2](reconciliation.md#design-changes-proposed)).

### 2.2 Rules

1. **Leaf crates off the hot path.** `wasmtime` and Cranelift sit only in
   `hd_run_wasmtime`; `wasm-bindgen` only in `hd_web`; `rayon` only behind
   `hd_sched`'s `threads` feature; file-system access only in `hd_cache`'s
   `disk` feature and `hd_cli`. A front-half edit never rebuilds them.
2. **No build scripts in hot crates.** Generated code (syntax kinds, typed
   views, the diagnostic code enum) is produced by `cargo xtask codegen`
   and checked in. CI fails if regenerating changes it.
3. **The driver owns no host facilities.** `hd_driver` takes its sources,
   cache, executor and clock from the caller; it has no `std::fs`,
   `std::process` or `Instant`
   ([reconciliation, item 3](reconciliation.md#design-changes-proposed)).
   Sources come through `SourceSet`, the cache through `CacheStore`, work
   through `Scheduler`, and time through the supplied clock or budgets
   measured in steps. The browser build then needs no stubs.
4. **No `serde` in the front half.** Blob and entry formats are
   hand-written and checked (§4.11). `toml` parses manifests only.
5. **The browser build compiles in CI from slice 1:**
   `cargo build -p hd_web --target wasm32-unknown-unknown`.
