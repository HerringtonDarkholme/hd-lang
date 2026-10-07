# New Compiler: Detailed Design

Status: Design, not decided. Both parts are done, 2026-10-07: Part D1,
the pipeline and front half, and Part D2, the back half. D2 also revised
D1 after the owner's review (see [Changes To D1](#changes-to-d1)).

## How To Read This

This is the design that follows the research. It does not reopen what the
owner decided. Its inputs:

- [NEW_COMPILER_ARCHITECTURE.md](NEW_COMPILER_ARCHITECTURE.md): goals,
  Arena pillars, goal metrics, feature triage, and the Decided list.
- [COMPILER_ARCHITECTURE_RESEARCH.md](COMPILER_ARCHITECTURE_RESEARCH.md):
  the research, and the owner's answers in
  [Open Questions For The Owner](COMPILER_ARCHITECTURE_RESEARCH.md#open-questions-for-the-owner).
- [COMPILER_PRIOR_ART_ISSUES.md](COMPILER_PRIOR_ART_ISSUES.md): known
  issues of prior front ends. Its [Lessons For hd](COMPILER_PRIOR_ART_ISSUES.md#lessons-for-hd)
  and its "Changes Suggested" list are design requirements here.
- The language specification, chapters 1 to 4, 9 to 14, and the
  [CLI chapter](../spec/cli/command-line.md). The spec stays authoritative.

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
| module | one file's module ([`module.path.name`](../spec/lang/10-modules.md#r-module.path.name)); the **cache unit** |
| folder | the directory a module belongs to ([Folders](../spec/lang/10-modules.md#folders)); the **interface unit** |
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
                              Body tasks per item (parallel, frozen tables)
                                              │
                              ModuleFinish per module (rows, sort) ═══════► [check entry]
                                              │
                    Coherence per trait ═► [coh]     InitOrder per folder ═► [init]
                                              │
                              Output assembly in content order ═══════════► [pkgres entry]
                     └────────────────────────┼────────────────────────────────────────┘
                                              ▼
                       D2: TIR ─► collect ─► emit Wasm per instance ─► link ─► run / test
```

`═══►` marks a cache boundary. Every boundary is content-addressed through
the `CacheStore` interface (section 5). Nothing crosses a boundary by an
in-memory ID.

### 1.2 Stages

| Stage | Input | Output | Unit | Parallel | Cached | Key |
| --- | --- | --- | --- | --- | --- | --- |
| Discover and stat | package root, manifest, `SourceSet` | file list with path, size, mtime, content hash | package | hashing in parallel | stat manifest in `build/` (local, not shared) | none |
| Skim (header pass) | file bytes | `Skeleton` | file | yes | no; the manifest keeps its use list and api hash | none |
| Full parse | file bytes | `TokenBuf`, `GreenTree` | file | yes | no | none |
| Folder graph | use lists of every file | folder DAG, `folder-cycle` | package | serial (tiny) | inside `pkgres` | none |
| Folder interface | skeletons of the folder, deep interfaces of used folders | `FolderIface`, blob, deep hash, header diagnostics | folder | yes, in DAG order | `iface` | [§5.3](#53-key-composition) |
| Module prep | module CST, own folder interface, used interfaces | module scope, private signatures, inferred results | module | yes, across modules | inside `check` | none |
| Body check | body CST, frozen tables | TIR (§4.13.11), diagnostics, facts | body | yes | inside `check`; TIR in `tir` | none |
| Module finish | the module's body results | `ModuleResult` | module | yes, across modules | `check` | [§5.3](#53-key-composition) |
| Test overlay | `tests:` block and doc tests of a module | TIR, diagnostics | module | yes | `check-test` | check key plus test uses |
| Coherence | impl heads of one trait over the graph | `overlapping-impl` | trait | yes | `coh` | trait path plus sorted head hashes |
| Init order | init summaries of a folder's modules | statement order, `top-level-read-before-initialization` | folder (only when a group spans modules) | yes | `init` | sorted summary hashes |
| Package result | all of the above | sorted diagnostics, summary | package | serial | `pkgres` | sorted keys of the parts |
| D2 stages | TIR, interfaces | Wasm | instance, program | see §11.2 | see §11.2 | see §11.2 |

### 1.3 The Task Graph

```text
Skim(f) ──► FolderGraph ──► FolderIface(F) ──► ModulePrep(m) ──► Body(m, i) ──► ModuleFinish(m) ──► PackageResult
               │              ▲    │                ▲                                │
Parse(f) ──────┼──────────────┼────┼────────────────┘                                ├──► InitOrder(F)
               │              │    └──► Coherence(trait) ◄── FolderIface(every F) ───┘
               └── FolderIface(G) for each folder G that F uses
```

- A `FolderIface(F)` task waits for the interfaces of the folders `F` uses,
  never for bodies. A deep chain of folders does not stall wide levels.
- A `Body` task waits only for its `ModulePrep`. Bodies never wait on each
  other, with one exception: a private function with an omitted result
  type, which `ModulePrep` infers first (§4.13.1).
- On a warm run most tasks never exist: a cache hit at a boundary removes
  the tasks below it.

### 1.4 What A Run Touches

| Situation | Files read | Tasks run | Entries written |
| --- | --- | --- | --- |
| warm, no edit | the manifest; no source file | none: the fast-path key hits `pkgres` | none |
| private body edit in module `m` | `m` | parse `m`; `FolderIface` is a key hit (api text unchanged); prep, bodies and finish of `m` | `check` of `m`, `pkgres` |
| private header edit in `m` | `m` | as above, plus `FolderIface` of `m`'s folder, which yields the same deep hash | `iface`, `check` of `m`, `pkgres` |
| public signature edit in `m` | `m`, plus files of folders whose interface changes | `FolderIface` of the folder and of folders whose deep hash changes; every module whose key includes a changed deep hash | those entries |
| first run in a new worktree of a known commit | every source file once (hashing) | none if another worktree checked the same content | the local manifest |

The second row is the `recheck-precision` target: one module.

## 2. Crate Graph

### 2.1 Crates

Small crates in a strict layer order, so most compiler edits rebuild one
crate ([Q1 risks](COMPILER_ARCHITECTURE_RESEARCH.md#risks)). This refines
[Q7](COMPILER_ARCHITECTURE_RESEARCH.md#proposed-crates).

| Crate | Responsibility | Depends on | Browser |
| --- | --- | --- | --- |
| `hd_base` | ID newtype macro, `Hash128` and `StableHasher`, `Span`, `FileId`, `Fuel`, small collections, the counting allocator | `xxhash-rust`, `smallvec`, `hashbrown` | yes |
| `hd_intern` | sharded string interner, stable-path interner, `DefId` table | `hd_base` | yes |
| `hd_diag` | `Diagnostic`, the `Code` enum generated from the spec's code list, fix-its, compact and JSON renderers | `hd_base` | yes |
| `hd_syntax` | lexer with skim mode, layout cursor, parser, green tree, typed views, skeletons | `hd_base`, `hd_intern`, `hd_diag` | yes |
| `hd_fmt` | the formatter on the green tree | `hd_syntax` | yes |
| `hd_project` | manifests, module discovery and identity, folders, folder and package graphs, the `SourceSet` trait | `hd_syntax`, `toml` | yes |
| `hd_iface` | blob writer, zero-copy reader, validator, per-item and deep hashes, the std pack format | `hd_base`, `hd_intern` | yes |
| `hd_types` | type interner, local type arenas, rows, unification, poison, the trait solver core and its memo | `hd_iface` | yes |
| `hd_resolve` | use resolution, folder interface builder, derived heads, orphan and visibility checks, interface validation | `hd_types`, `hd_project` | yes |
| `hd_tir` | TIR: the generated tags, views and builder, the verifier, serialization (§4.13.11) | `hd_types` | yes |
| `hd_check` | body checker emitting TIR, exhaustiveness, GADT refinement, templates, coherence, init order, facts | `hd_resolve`, `hd_tir` | yes |
| `hd_cache` | `CacheStore` trait, keys, entry framing, memory store, disk store (feature `disk`), stat manifest, trim | `hd_iface` | memory store only |
| `hd_sched` | `Scheduler` trait, task graph, serial executor, thread pool executor (feature `threads`), budgets, memory cap | `hd_base`, `rayon` (feature) | serial only |
| `hd_driver` | `Session`, command pipelines (check, test plan, doc, fix), output assembly | all of the above | yes |
| `hd_doc` | `hd doc` rendering ([HD_DOC.md](HD_DOC.md)) | `hd_driver` | yes |
| `hd_stdpack` | build-time tool: checks `lib/std` and writes the std pack that `hd_driver` embeds | `hd_driver` | the pack, not the tool |
| D2 crates | `hd_mono`, `hd_wasm`, `hd_host_abi`, `hd_run` (§11.4) | `hd_tir`, `hd_driver` | yes |
| `hd_run_wasmtime` | wasmtime embedding (D2) | `hd_run`, `wasmtime` | no |
| `hd_cli` | the native `hd` binary: argument parsing, the disk cache, threads, terminal output | everything native | no |
| `hd_web` | wasm-bindgen glue, the JS `SourceSet` and `CacheStore`, the stepping scheduler | `hd_driver`, D2 crates, `wasm-bindgen` | only there |
| `hd_testkit` | dev only: determinism matrix, edit-script fuzzer, differential harness | `hd_driver` | no |

```text
hd_base ─► hd_intern ─► hd_diag ─► hd_syntax ─► hd_fmt
                │                     │
                │                     └──► hd_project ─┐
                └──► hd_iface ─► hd_types ─────────────┴─► hd_resolve ─► hd_check ─┐
                         └──► hd_cache                                              │
hd_base ─► hd_sched ────────────────────────────────────────────────────────────────┤
                                                                                     ▼
                                          hd_driver ─► hd_doc, D2 crates ─► hd_cli ◄─ hd_run_wasmtime
                                                                          └► hd_web
```

### 2.2 Rules

1. **Leaf crates off the hot path.** `wasmtime` and Cranelift sit only in
   `hd_run_wasmtime`; `wasm-bindgen` only in `hd_web`; `rayon` only behind
   `hd_sched`'s `threads` feature; file-system access only in `hd_cache`'s
   `disk` feature and `hd_cli`. A front-half edit never rebuilds them.
2. **No build scripts in hot crates.** Generated code (syntax kinds, typed
   views, the diagnostic code enum) is produced by `cargo xtask codegen`
   and checked in. CI fails if regenerating changes it.
3. **No `std::fs`, threads or clocks below `hd_driver`.** Sources come
   through `SourceSet`, the cache through `CacheStore`, work through
   `Scheduler`, and time only through budgets measured in steps. The
   browser build then needs no stubs.
4. **No `serde` in the front half.** Blob and entry formats are
   hand-written and checked (§4.11). `toml` parses manifests only.
5. **The browser build compiles in CI from slice 1:**
   `cargo build -p hd_web --target wasm32-unknown-unknown`.

## 3. Core Data Structures

### 3.1 IDs And Their Scopes

Every ID is a `u32` newtype. IDs are cheap to compare and never reach
output or a hash. Each has a stable form used at boundaries.

| ID | Scope | Indexes | Stable form at boundaries |
| --- | --- | --- | --- |
| `Symbol` | run | interned string | the string |
| `PackageId` | run | resolved package | `root`, or `HOST_PATH@VERSION` plus tree hash, or a workspace-relative path for a path dependency |
| `FileId` | run | source file | package plus package-relative path |
| `ModuleId` | run | module | package plus module path, as `shop.cart` |
| `FolderId` | run | folder | package plus folder path, as `src/shop` |
| `DefId` | run | any declaration, member, variant or impl | `StablePath` (§3.2) |
| `Ty` | run (global) or body (local) | type | structural encoding over stable paths |
| `RowId` | run | interned row | sorted stable keys |
| `TokenIdx`, `NodeIdx` | file | token, green node | byte offsets |
| `ItemIdx` | module | item in source order | item path |
| `InferVar`, `LocalId`, `Inst` | body | inference variable, local binding, TIR instruction | none: never leaves the body |
| `TaskId` | run | scheduler task | none |

```rust
// hd_base: IDs implement Eq and Hash, not Ord and not StableHash.
// Sorting anything for output needs an explicit content key (§6.5).
macro_rules! id { ($name:ident) => {
    #[derive(Copy, Clone, PartialEq, Eq, Hash)]
    pub struct $name(NonZeroU32);
} }
id!(Symbol); id!(FileId); id!(ModuleId); id!(FolderId); id!(DefId);
```

Making IDs unsortable and unhashable for stable hashes is a compile-time
guard for prior-art lesson 2 (mine): code that tries to sort by an ID or
hash one into a key does not compile.

### 3.2 Stable Paths And Stable Hashing

```rust
pub struct StablePath {
    pub package: PackageKey,       // "root", "github.com/acme/json@2.1.0#<tree>", "std"
    pub module: Box<[Symbol]>,     // ["shop", "cart"]
    pub item: Box<[ItemSeg]>,      // ["Cart", "total"] or [Impl(head_hash, ordinal)]
}
pub enum ItemSeg { Name(Symbol), Impl { head: Hash128, ordinal: u16 } }

pub trait StableHash {
    fn stable_hash(&self, h: &mut StableHasher, cx: &StableCx);
}
// StableCx maps run IDs to stable paths. DefId's hash goes through cx.path(def).
```

- `Hash128` is xxh3-128. The algorithm is named in the cache layout file,
  so a later remote cache can move to BLAKE3 without ambiguity.
- An impl has no name. Its stable segment is the hash of its head's
  normalized text, plus an ordinal among identical heads in the module
  (identical heads are `overlapping-impl` anyway).
- Program-database symbols are the printed stable path, as
  `github.com/acme/json@2.1.0/json.parse/Parser.next`
  ([Q6](COMPILER_ARCHITECTURE_RESEARCH.md#q6-program-database-hook)).

### 3.3 Interners

| Interner | Holds | Concurrency | Lifetime |
| --- | --- | --- | --- |
| strings | identifiers, paths, literal text that types name | 64 shards by hash; each a `Mutex` around a hash table of indices, over an append-only chunked vector read without locks | process |
| stable paths | `StablePath` to `DefId`, plus `DefData` (kind, owner module, span) | same scheme | process |
| InternPool | types with no inference variables, rows, and constant values | per-thread append-only columns plus a sharded index (§3.9.2) | process |
| body-local pool | types holding `InferVar`s, in the InternPool's encoding | none: owned by one body | body |

- Keywords, prelude names and primitive types are pre-seeded at fixed
  indices, so the common case never locks.
- An ID's value depends on which thread interned first. That is harmless
  because no ID is printed, sorted or hashed (§6.5). The determinism
  matrix shifts IDs on purpose to prove it (§8.1).
- In the browser the shards' mutexes are uncontended single-thread locks.

### 3.4 Types

A type is an index into the InternPool (§3.9.2). `TyKind` below is the
**decoded view** that the accessor layer returns (§3.9.6). It is not how
types are stored: storage is a one-byte tag, a `u32` data word and, for
variable parts, words in the pool's `extra` column.

```rust
/// A type: an InternPool index. Bit 31 set: an index into the current body's local pool.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Ty(u32);

pub enum TyKind {                  // decoded on demand; `TyList` and `RowId` are slices of `extra`
    Prim(Prim),                    // i8..u64, usize, f32, f64, bool, char, string, void
    Never,
    Poison,                        // see §3.6
    Adt { def: DefId, args: TyList },
    Tuple { elems: TyList, rest: Option<Ty> },   // rest element of tuple rest types
    Optional(Ty),
    Fn { params: TyList, result: Ty, row: Row, suspends: bool },
    TraitValue { def: DefId, args: TyList, bindings: AssocList },
    Param(ParamRef),               // (owner DefId, index); declared, never inferred
    Assoc { base: Ty, trait_: DefId, name: Symbol },  // an unnormalized projection
    Readonly(Ty),                  // a readonly view
    Infer(InferVar),               // local arena only
    IntLit(InferVar),              // open literal width, local arena only
}

pub struct Row { pub keys: RowId, pub param: Option<RowParamRef>, pub var: Option<RowVar> }
```

- `Row.var` is a row variable for a private callee whose row is inferred.
  It lives only until the module's row solve (§4.13.4).
- `TyList`, `AssocList` and `RowId` are interned slices, so type equality is
  one integer comparison for global types.
- A global type never contains a local one. Interning into the global
  pool rejects an item whose `has_local` flag is set.
- **How body-created types are interned without copies.** A body builds
  types with variables in its local pool. When the body finishes (or a
  type becomes variable-free earlier), `resolve` replaces variables by
  their bindings and interns the result in the global pool. Equal content
  gives the equal `Ty` whichever thread interns it. There is one pool, not
  a copy per thread (lesson 5), and no ID escapes (lesson 2).

### 3.5 Arenas And Lifetimes

| Arena | Holds | Created | Freed |
| --- | --- | --- | --- |
| session | interners, `DefData`, global types, frozen interfaces, impl tables, memo tables | process start | process end (REPL: never; it is bounded, §7.9) |
| file | `TokenBuf`, `GreenTree`, line starts | parse | after its module's `ModuleFinish`, unless `hd fmt`, `hd fix` or D2 needs the tree |
| folder build | scratch for resolution | `FolderIface` start | its end; the result is the frozen `FolderIface` |
| body | the local pool, inference tables, trail, the scratch buffer, TIR columns under construction | `Body` start; one set of columns per worker, truncated to empty per body | body end; TIR's columns move into the module result |
| module result | TIR of every body, diagnostics, facts | the end of each body | `ModuleFinish`, after writing the `tir` entry (§4.13.11) |

**Frozen versus per-task.** A `FolderIface` is written once, by its task,
into a `OnceLock` slot, and read only by tasks that the graph orders after
it. A `ModuleScope` is the same. Body tasks therefore read shared data with
no locks. The only shared mutable structures are the append-only interners
and the memo tables, whose answers are pure functions of frozen inputs
(§4.12.2).

### 3.6 The Poison Type

1. `Ty::POISON` is a pre-seeded global type.
2. It unifies with every type, satisfies every bound, has every member, and
   entails every row.
3. Any check whose operand or expected type contains poison stays silent.
   Only the error that created the poison reports.
4. Sources of poison: an unknown name (reported once per name per body), a
   broken header (reported once in its module), a syntax error node, a
   failed limit, and an item whose interface is poison.
5. A poisoned item still exports: its interface record carries `Poison`
   where its type is unknown, so dependents check the rest without noise.

### 3.7 Spans And Files

```rust
pub struct Span { pub file: FileId, pub lo: u32, pub hi: u32 }   // byte offsets
```

- Line and column are computed only when output is rendered, from a
  per-file line-start table.
- In a cache entry a span is stored as a byte range in the entry's own
  module file, or as (file stable path, range) for another file.
- Doc tests map their spans back to the `##` line that holds them
  ([`cli.test.doc.location`](../spec/cli/command-line.md#r-cli.test.doc.location)).
- Offsets are `u32`, so a file is at most 4 GiB. The practical limit is far
  lower (§4.15).

### 3.8 Diagnostic Records

```rust
pub struct Diagnostic {
    pub code: Code,                    // generated from the spec's code list
    pub severity: Severity,            // Error | Warning
    pub primary: Span,
    pub message: Message,              // template id plus typed arguments, rendered late
    pub labels: SmallVec<[Label; 2]>,  // secondary spans
    pub notes: SmallVec<[Note; 1]>,    // shown in detail mode only
    pub fixes: SmallVec<[Fix; 1]>,
    pub root: RootKey,                 // one diagnostic per root cause (§4.14)
}
pub struct Fix { pub title: Message, pub edits: SmallVec<[Edit; 1]>, pub safety: FixSafety }
pub struct Edit { pub file: FileId, pub lo: u32, pub hi: u32, pub text: Box<str> }
pub enum FixSafety { Exact, Suggestion }   // Exact passed the re-parse check; `hd fix` applies it
```

- Message arguments are types, names and paths, not strings. They render
  through stable paths with the shortest unambiguous name, so a message is
  the same on every run and every machine.
- A diagnostic stored in a cache entry is in this form with stable spans.
  Rendering happens at output.

### 3.9 Data-Oriented Encoding

Added with D2, after the owner asked whether this design is truly
data-oriented. Every IR is stored as columns of fixed-size items with
`u32` indices between them. There are no `Box`es, no pointers and no
variable-size enums in storage. Each IR's encoding is chosen by who reads
it.

Sources, credited where each idea is used:

- **Zig**: Sema emits AIR directly, with no tree lowered later. ZIR and
  AIR store each instruction as a one-byte tag plus an 8-byte data word
  whose meaning depends on the tag, with variable operands in an
  `extra: []u32` array. The InternPool stores types and values the same
  way, with per-thread shards since Zig 0.14.
- **Carbon** SemIR: the checker emits the semantic IR directly; typed
  instruction kinds over value stores, read through typed accessors
  (`inst.As<T>()`), and IR blocks as ranges.
- **Yuku**, a Zig JS/TS parser
  ([write-up](https://www.arshad.fyi/writings/data-oriented-design-in-yukus-parser)):
  named fields at a public boundary, a scratch buffer with checkpoint,
  flush and truncate, backtracking by truncation, children at lower
  indices than parents so a loop from 0 to n is a post-order walk,
  position-independent bytes serialized with one copy, a generated
  `Int32Array` decoder for JavaScript, compile-time size asserts, and one
  definition that generates the layout, the encoder and both decoders. It
  parses an 8 MB file into 853k nodes in about 19 ms.
- **Vx**: flat type and HIR streams. Its 256-bit IDs and per-worker
  arenas with an epoch merge were compared and not adopted in the
  research ([Comparison With Vx](COMPILER_ARCHITECTURE_RESEARCH.md#comparison-with-vx)).
- **rust-analyzer**: typed views generated from an ungrammar (§4.4).

#### 3.9.1 Four IRs, And The Stopping Point Of Each

The owner asked whether all these IRs are needed and what they cost in
memory (2026-10-07). There are four: tokens with the syntax tree, the
interface, TIR, and Wasm.

```text
tokens (transient) ─► syntax tree ─► interface (cached, memory-mapped) ─► TIR, one per body ─► Wasm
```

- **TIR** is one typed IR per body, emitted directly by the checker. There
  is no checked tree that is later lowered to a mid-level IR. hd has no
  borrow checker, so it has no reason for rustc's THIR and MIR split.
  Zig's Sema emits AIR and Carbon's checker emits SemIR the same way.
- **No monomorphized IR.** Emission walks the generic TIR under a type
  substitution and writes each instance's Wasm at once (§13.8). TIR is
  cached per module; Wasm bytes are cached per instance.
- **The item tree** is an index over the syntax tree and the interface,
  not a copy of either (§4.6).

| IR | Consumers | Encoding | Why |
| --- | --- | --- | --- |
| tokens | the parser, skim mode, the formatter | columns (§4.1) | read in order, one or two columns at a time |
| syntax tree | `hd fmt`, `hd fix`, `hd doc`, the playground's JS, the program database (Later) | compact columns, plus named-field typed views generated from `hd.ungram` in Rust and in JS (§4.4) | outside readers want Yuku's ergonomics; storage stays at about 14 bytes per node |
| folder interface | resolution, the checker, `hd doc`, D2 | its blob, read in place through views (§4.10, §4.11) | memory-mapped from the cache; never decoded into a second copy |
| TIR | D2: collection and emission | Zig-style tag + 8-byte data + `extra` (§4.13.11) | internal; dense |
| Wasm code entries | the linker | bytes plus relocation columns (§13.8) | patched in place |

The InternPool (§3.9.2) is storage that all of them share, not an IR.
The syntax tree is the only IR that leaves the compiler, so it is the
only one with named-field views at its boundary. TIR and the pool are
internal, so they take the dense encoding, and the accessor layer
(§3.9.6) gives the compiler's own code named fields anyway.

#### 3.9.2 The InternPool

One pool holds every global type, row and constant value: the literals,
fact values and constant data that TIR and the linker need.

```rust
pub struct InternPool {
    locals: Box<[PoolLocal]>,           // one per worker thread; appended only by that thread
    index: [Mutex<RawTable<Index>>; 64],// content hash -> Index; an index, never the storage
}
struct PoolLocal {
    tags: AppendVec<PoolTag>,           // u8
    data: AppendVec<u32>,               // inline payload, or an offset into `extra`
    extra: AppendVec<u32>,              // variable parts
    bytes: AppendVec<u8>,               // string and byte constants
}
pub struct Index(u32);                  // bit 31: body-local; bits 25..30: thread; bits 0..24: item
```

| Tag | `data` | `extra` |
| --- | --- | --- |
| `Prim`, `Never`, `Poison` | the primitive | none |
| `Optional`, `Readonly` | the inner `Index` | none |
| `Param` | offset | owner `DefId`, index |
| `Adt` | offset | `DefId`, count, arguments |
| `Tuple` | offset | count, elements, rest or `NONE` |
| `Fn` | offset | flags (suspends), row, result, count, parameters |
| `TraitValue` | offset | `DefId`, argument count, arguments, binding count, bindings |
| `Assoc` | offset | base, trait, name |
| `Row` | offset | key count, keys in content order, row parameter or `NONE` |
| `IntSmall` | the value, if it fits in 32 bits | the type |
| `Int64`, `Float` | offset | the type, low word, high word |
| `Str` | offset | byte offset, length |
| `Aggregate` | offset | the type, field count, field values |

- **Items are fixed size**: 5 bytes in the tag and data columns, plus
  `extra` words for compound items. A type averages about 13 bytes.
- **Lookup** hashes the content, takes the shard's lock, probes, and
  appends to the calling thread's columns on a miss. Equal content always
  gets the same `Index`, so equality stays one comparison. Reads of an
  existing item take no lock: the columns are append-only, and an index
  is published only after its item is written.
- **No per-thread copies.** Threads share one pool. Only appending is per
  thread, which is Zig 0.14's scheme, not Vx's merge.
- Primitives, `Poison` and common types are pre-seeded at fixed indices.
- The body-local pool has the same columns, in the body's arena.

#### 3.9.3 The Dense Encoding

TIR uses the pool's shape with two more columns (its full definition is
§4.13.11):

```rust
pub struct Ir<Tag> {
    tags: Vec<Tag>,              // 1 byte
    data: Vec<[u32; 2]>,         // 8 bytes; meaning set by the tag
    ty: Vec<Ty>,                 // 4 bytes: the instruction's result type
    syn: Vec<NodeIdx>,           // 4 bytes: the syntax node, so the span is exact
    extra: Vec<u32>,             // operand lists and rare-size payloads
    side: SideTables,            // rare data: origins of generated code, hole candidates, await facts
}
pub const NONE: u32 = u32::MAX;  // "no child", as in Yuku
```

- **Operands by index.** An operand is a `u32` reference. A list of
  operands is `{start, len}` in `extra`.
- **Rare data in side tables**, keyed by instruction index, so common
  instructions stay small. A typed hole's candidate names, the origin
  chain of generated code and the facts at each suspension point live
  there.
- **Compile-time asserts** fix each item's size: `const _: () =
  assert!(size_of::<[u32; 2]>() == 8);` and one per `extra` record, so an
  instruction can never grow by accident (Yuku).
- **Order.** Every operand has a lower index than its user, so a loop
  from 0 to n visits operands first (Yuku, Zig's AIR). Instructions are
  appended in evaluation order, and each block lists its instructions in
  `extra`, so emission walks block lists front to back.

#### 3.9.4 Tables As Struct-Of-Arrays

The item index, `DefId` data, impl tables, TIR locals and interface
sections are columns indexed by an ID, never `Vec<struct>` with `HashMap`
storage:

- A pass that reads one property (all item kinds, all local types) reads
  one dense column.
- **Hash maps are only indexes.** A `HashMap<Symbol, ItemIdx>` or
  `HashMap<DefId, Range>` may point into the columns. No data lives only
  in a map, so iterating a map never decides output order (§6.5).
- Sorted sections with binary search replace maps where the table is
  frozen, as the interface blob's export index does.

#### 3.9.5 Building: Scratch Buffer, Checkpoints, Truncation

- **One scratch buffer per worker** (Yuku). A builder that collects a
  variable list (call arguments, a block's instructions) pushes onto the
  scratch buffer, then flushes the range into `extra` when the list is
  done and truncates the scratch back to its checkpoint. Nested lists nest
  checkpoints.
- **Rollback is truncation.** Every column is append-only while a body is
  checked, so a checkpoint is a tuple of lengths: instructions, `extra`,
  scratch, locals, the local pool, the inference trail, buffered
  diagnostics. Undoing a trial truncates them all. The parser's
  speculative paths and the checker's candidate trials use this one
  protocol, which replaces the prototype's two rollback mechanisms with
  different guarantees (CA-05, §3.10.4).
- The parser's event vector is the same pattern: recovery truncates the
  events of a failed attempt.
- **In-place patches** are allowed in two places only: the final type
  sweep rewrites `ty`, and M3 fills the provider lists of calls to private
  callees whose rows were omitted (§4.13.1). Both overwrite fixed-size
  words; neither moves an instruction.

#### 3.9.6 One Schema, Generated Accessors

Each IR has one schema file (`hd_syntax/hd.ungram`, `hd_tir/tir.ir`,
`hd_types/pool.ir`). For every tag it lists the operand names, their
kinds, the `extra` layout, the type rule and the verifier's checks.
`cargo xtask codegen` generates from it, and the output is checked in
(§2.2):

- the `#[repr(u8)]` tag enum and the size asserts;
- the builder's encoding functions (§4.13.11);
- **typed views**: `tir.view(i)` decodes one instruction into a small
  `Copy` enum with named fields, such as `TirView::Call { callee, args,
  providers }`, where `args` is a slice view over `extra`. Code reads like
  code over a Rust enum, and agents never index raw words;
- the verifier's structural part (§3.10.3);
- a printer for `hd debug tir` and test snapshots;
- for the syntax tree only, the wire encoder and the JavaScript decoder,
  which reads the columns through `Int32Array` and `Uint16Array` views
  (Yuku).

**What the accessor layer costs.** Decoding a view is a tag load, one
8-byte load and a slice computation. It is inlined, so it costs about a
nanosecond and no allocation. The costs that are real: a generated
module per IR to keep in sync (CI regenerates and diffs it), longer
compile times of the generated `match`es, and debugging that goes through
the printer instead of a derived `Debug` on a tree. The printer is
therefore part of the first slice of each IR.

#### 3.9.7 Byte Budgets And Linear Passes

| Item | Size | Fields |
| --- | --- | --- |
| token | 13 B | kind, start, end, indent, depth, line flag (§4.1) |
| syntax node | 14 B | kind, first token, last token, subtree length (§4.4) |
| item index entry | 8 B | header node, parent (§4.6) |
| pool item | 5 B + `extra` | about 13 B per type on average |
| TIR instruction | 17 B + `extra` | tag 1, data 8, type 4, syntax node 4; budget 26 B with `extra` |
| TIR local | 13 B | type 4, name 4, syntax node 4, flags 1 |
| relocation | 9 B | offset 4, kind 1, target 4 |

CI measures the averages on std and the generated packages and fails on
a regression past the budget, as the size asserts do for a single item.

Passes that are one linear scan over columns: lexing; tree building from
events; skeleton extraction; TIR's final type sweep (resolve every `ty`,
one column); collecting calls for init summaries and fact records (a scan
of `tags`); TIR hashing and serialization (column copies); collection's
call scan per instance (§13.2); liveness at suspension points (one
backward scan per block); relocation patching; interface blob writing.
Emission walks block lists in order: a forward scan with a stack of open
blocks.

#### 3.9.8 What It Buys: Locality And Memory

- **Locality.** A pass touches only the columns it reads. The final type
  sweep reads `ty` and the local pool; the call scan reads `tags`. A
  column of 4-byte items puts 16 to a cache line, where a tree of boxed
  enums would put one node per line or less.
- **Memory.** §3.10.2 gives each IR's bytes per source line and lifetime,
  and the peaks for a check and a build. A warm check after a
  one-function edit holds one file's tree and one module's TIR, well under
  a megabyte beside the `hd` binary's own pages and the pool.
- **No per-thread copies.** Interfaces are frozen and shared; the pool is
  shared with per-thread appends; each worker has one set of body
  columns, reused body after body. Parallel checking therefore adds
  little memory, which the `parallel-speedup` exit test bounds at 1.5x
  the memory of one thread (§9).
- **Serialization is copying.** Interface blobs, `tir` entries and code
  entries are columns written as position-independent little-endian
  bytes, so writing is a few copies and reading maps the file (Yuku's
  memcpy property).

### 3.10 IR Abstraction Contracts

Added with D2, after the owner asked how abstract the checked IR is.
Each IR has a contract: what is already decided in it, what is
deliberately not in it, its lifetime and size, and the invariants its
verifier checks. A pass may rely on the contract of its input and must
establish the contract of its output.

#### 3.10.1 What Each IR Holds

| IR | Resolved in it | Deliberately not in it |
| --- | --- | --- |
| tokens | token kinds and spans, line facts, string frames | decoded literal values (decoded on demand into a small pool, as Yuku does), names, layout decisions |
| syntax tree | full syntax, every layout token, every comment by position | names, types, desugaring |
| item index | which syntax node or interface record each item is, its parent, its name lookup | anything the node or record already holds |
| folder interface | every public signature as pool types; re-exports followed to the real declaration; impl heads; templates with resolution tables; per-item hashes | bodies, private items (except hidden template items), fact values, doc text, layouts |
| TIR | every name as a `DefId` or a local; every type; every callee (static, trait method with its bound or impl, trait value, function value); every coercion as an instruction; operators on primitives as primitive instructions and all others as trait calls; indexing as calls; pipe, interpolation, string prefixes, literal suffixes, compound assignment, comprehensions and `?` desugared; evaluation order as instruction order; default arguments; named arguments mapped to parameter slots; `for` over std ranges, lists and maps as dedicated loops, and every other `for` desugared to the protocol; match decision trees as switch instructions; GADT refinements; cleanup scopes with their `defer` suites; suspension points; hook points; providers per call; closure captures with their sharing mode; structured control flow | layouts; boxing; capture cells; vtables and dictionaries; frame layouts and state-machine dispatch; overflow and bounds check sequences (a profile choice); instantiation |
| Wasm code entry | instructions, locals, Wasm types as canonical descriptors | indices (symbolic relocations), folding, section layout |

Everything in TIR's right column is decided during emission, per instance
and per tier (§12, §14, §15). Nothing in it needs a second IR.

#### 3.10.2 Lifetimes, Sizes And Peak Memory

Estimates for typical hd code: about 35 bytes, 8 tokens, 10 syntax nodes
and 7 TIR instructions per source line.

| IR | Lifetime | Bytes per source line |
| --- | --- | --- |
| tokens | with their file's tree | about 100 |
| syntax tree | from parse to its module's `ModuleFinish`, or to the end of `hd fmt` or `hd fix` | about 140 |
| item index | with its module scope | under 10 |
| interface | memory-mapped for the run; shared by every process that maps it | about 40 per public line; 0 for private code |
| TIR | from a body's check to its module's `ModuleFinish`, which writes the `tir` entry and frees it; emission maps the entry | about 200 in memory; the same on disk |
| Wasm code entries | from emission to the link that reads them; mapped from the cache | about 40 |
| linked module | the link and the precompile | about 30 per reachable line |

**A cold check of a 10k-line package** (8 workers): each worker holds the
tree and TIR of the modules in flight, about 0.35 MB per 1,000-line
module, so a few MB in all. Every finished module's tree and TIR are
freed at `ModuleFinish`. The pool holds a few MB, interfaces are mapped
(about 0.4 MB). **A check never keeps TIR**: it writes the `tir` entry
and drops it, so a later build reuses it. Peak: the binary's pages plus
about 10 MB, inside the `resources` target of 50 MB.

**A build** adds collection (a set of instance keys), emission (one
instance at a time per worker, reading mapped `tir` entries), the linked
module (a few hundred KB for 10k lines) and Cranelift's compile memory,
which dominates: several MB per worker. No monomorphized IR exists at any
point. The `resources` metric records build peaks; it sets no target
for them.

#### 3.10.3 Invariants And Verifiers

Each verifier runs in the compiler's debug builds, in CI on std and the
conformance suite, and in verify mode (§5.6). A failure is an internal
error naming the item, never a user diagnostic.

| IR | Verifier checks |
| --- | --- |
| tokens and tree | text round-trips byte for byte; token ranges increase; subtree sizes nest; every layout token sits between real tokens |
| item index | every entry names a header node or an interface record; parents precede children; every synthetic impl names its opt-in |
| folder interface | validate on write and decode-compare (§4.10, step 8) |
| InternPool | an item's `extra` offsets lie in bounds; a global item mentions no local index; hash-consing holds (re-interning an item returns its index) |
| TIR | §4.13.11: operands precede users; types are global after the sweep; every operand's type equals what its position expects; control-flow targets enclose their exits; cleanup and suspension rules hold |
| Wasm | `wasmparser` validation of the linked module; every index immediate in a code entry has a relocation (debug builds emit a sentinel index and check that none survives the patch) |

#### 3.10.4 Prototype Failures And The Rules That Prevent Them

The audit's findings ([findings](../audit/compiler/findings-2026-10-04.md),
[directions](../audit/compiler/architecture-directions-2026-10-05.md)):

| Prototype failure | Rule here |
| --- | --- |
| CA-01: packages flattened into joined source; identity reconstructed later | modules and folders are real units from discovery (§4.7); every declaration is a `DefId` with a stable path (§3.2); no source is ever joined |
| CA-02: types are canonical strings | types are InternPool items (§3.9.2); text exists only when a diagnostic renders (§3.8) |
| CA-03: one broad mutable checker object; bodies checked several times for private row inference | a body task owns its columns and reads frozen tables (§3.5); omitted results are inferred once, in M1; omitted rows are solved from constraints in M3 and patched into call provider lists, never by rechecking (§4.13.1); a debug counter asserts each body is checked exactly once per run |
| CA-04: HIR already holds capture cells, dictionary plans, provider packs and suspension indices; suspension CFG built in the emitter; ABI spread over several consumers | TIR's right column (§3.10.1): it holds sharing modes, not cells; dispatch choices, not dictionaries; suspension points and their cleanup facts, not frames. The state machine is built by emission from those explicit facts, one rule for every target (§14.2). One ABI description generates every side (§17.1) |
| CA-05: two rollback mechanisms with different guarantees | one checkpoint protocol: truncation of append-only columns plus the inference trail (§3.9.5) |
| CA-06: reachability at HIR and at WAT level select different dependencies | one collection over TIR includes runtime helpers, codecs, imports and types (§13.2); the linker adds nothing outside the instance set, and fails if a relocation names a symbol outside it |
| CA-07: no HIR verifier; internal failures rethrown; panics without source attribution; timeouts that cannot interrupt a body | a verifier per IR (§3.10.3); task-boundary internal errors (§6.4); site tables (§15.5); epoch interruption (§17.8) |
| separate generation paths for ordinary cleanup and suspension cleanup | one cleanup scope per TIR `Scope` instruction; ordinary exits and cancellation both run that scope's suites, and the TIR verifier checks that every suspension point names the scopes it would clean up (§4.13.11, §14.6) |
| F-506: frames keep every local | per-suspension-point liveness, cleanup reads included (§14.2) |
| F-552: suspension code grows about N³ | one dispatch case per basic block of a flattened region (§14.2) |
| CS-01: a process-global set of spans grows across compilations | no process-global mutable state except the interners and memo tables, whose contents are pure functions of their keys; per-run facts live in run arenas; `long-session` tests the REPL (§7.8) |
| CS-02: diagnostic registries keyed by file path and offsets, without compilation identity | diagnostics carry a `RootKey` over stable paths (§3.8); origins of generated code are a TIR side table per body, so no registry outlives its body |
| F-626: speculation clones checker state | the trail and truncation (§3.9.5) |

## 4. Front-Half Stages

### 4.1 Lexer

**Job.** Turn bytes into significant tokens, comments and line facts, in
one pass, pulled on demand by the parser.

```rust
pub struct TokenBuf {
    pub kinds: Vec<TokenKind>,     // u8; significant tokens only
    pub starts: Vec<u32>,          // byte offsets
    pub ends: Vec<u32>,
    pub line_first: BitVec,        // the token is the first on its physical line
    pub indent: Vec<u16>,          // that line's column (meaningful for first tokens)
    pub depth: Vec<u16>,           // bracket depth before the token
    pub comments: Vec<(u32, u32, CommentKind)>,  // Plain | Doc | ModuleDoc
    pub line_starts: Vec<u32>,
}
```

- **Lossless without trivia tokens.** Whitespace is the gap between
  tokens. Comments are a side list. Tokens, gaps and the original text
  reproduce the file byte for byte, which slice 1 tests on every file.
- **Modes.** The lexer keeps a frame stack: `Code { bracket_depth }` or
  `Str { delimiter, raw, prefix }`. Interpolation `${...}` pushes a code
  frame inside a string frame. Multiline `"""`, raw strings and prefixed
  strings are string frames. The same stack serves skim mode (§4.3), so
  the two cannot disagree (the TypeScript `preProcessFile` lesson).
- **Fast path.** ASCII bytes go through a byte-class table. Non-ASCII
  identifiers are checked for NFC with a quick-check table only when they
  occur ([`lex.encoding.scalars`](../spec/lang/01-lexical-structure.md#r-lex.encoding.scalars)).
- **Errors never stop it.** Invalid UTF-8, a bare CR, a tab in
  indentation, a stray BOM, and an unterminated single-line string (which
  ends at its line end) each produce one diagnostic and an `Error` token,
  then lexing continues.
- **Precomputed classes (Yuku).** Token kinds are numbered so that
  "operator", "keyword" and "starts an expression" are range or bit
  tests, and a 256-entry table gives each kind's precedence and flags in
  one load. Keywords are found by a perfect hash on the first byte, the
  second byte, the last byte and the length: one probe, one length
  compare, one short compare.
- **No early decoding.** Identifier and literal tokens are spans. Escapes,
  numeric values and NFC checks run only when the parser or the checker
  asks for the value, into a small pool of decoded exceptions.
- **Complexity.** O(n) time, about 13 bytes per significant token.
  Lexing and parsing throughput are measured separately (the Carbon
  lesson), so a regression points at its phase.

### 4.2 Layout

**An input inconsistency, resolved.** The research says no layout decision
needs the parser ([Q4](COMPILER_ARCHITECTURE_RESEARCH.md#q4-parser-and-cst)).
The spec says layout and parsing cooperate at a suite-introducing colon
([`lex.colon.cooperate`](../spec/lang/01-lexical-structure.md#r-lex.colon.cooperate)),
and `SUITE_END` boundaries need parser-aware processing
([`lex.suite-end.no-spelling`](../spec/lang/01-lexical-structure.md#r-lex.suite-end.no-spelling)).
Both are partly true. At bracket depth zero, Python's indentation stack
decides everything. Inside brackets, and for same-line suites, only the
parser knows that a colon opens a suite.

**Design: a layout cursor that the parser drives** (mine in this form).

```rust
pub struct LayoutCursor<'t> {
    toks: &'t TokenBuf,
    pos: TokenIdx,
    indents: SmallVec<[u16; 16]>,      // depth-zero indentation stack
    suites: SmallVec<[OpenSuite; 8]>,  // suites the parser opened
    pending: SmallVec<[Layout; 4]>,    // virtual tokens due before `pos`
}
pub struct OpenSuite {
    kind: SuiteKind,           // Indented | SameLine
    delim_depth: u16,          // bracket depth at the colon
    reference_indent: u16,     // lex.nested.reference
    statement_indent: u16,     // lex.nested.body-depth
    closure: bool,             // the lex.closure.* end rules apply
}
pub enum Layout { Newline, Indent, Dedent, SuiteEnd }

impl LayoutCursor<'_> {
    pub fn peek(&mut self) -> TokenOrLayout;   // computes pending layout lazily
    pub fn bump(&mut self);
    /// Called by the parser exactly when it consumes a suite-introducing colon.
    pub fn open_suite(&mut self, closure: bool);
}
```

1. At depth zero with no suite opened inside brackets, `peek` runs the
   algorithm of [Indentation Levels](../spec/lang/01-lexical-structure.md#indentation-levels)
   from `line_first` and `indent`. No parser input is needed.
2. `open_suite` decides indented or same-line by whether the next token
   starts a physical line. It records the references that the nested rules
   need ([Suites Inside Delimiters](../spec/lang/01-lexical-structure.md#suites-inside-delimiters)).
3. A same-line suite closes at a logical line end, at a comma or closing
   delimiter of its depth, or before `else`, innermost first
   ([Same-Line Suites](../spec/lang/01-lexical-structure.md#same-line-suites)).
   The cursor emits the `SuiteEnd`s; the parser never counts them.
4. Layout tokens enter the green tree as zero-width tokens, so the tree
   records every layout decision and the formatter sees them.
5. **Recovery.** `invalid-dedent`: report once, dedent to the nearest
   lower active level, go on. `unexpected-indentation`: report, then treat
   the line as part of the current suite. A closure end-rule violation:
   `syntax-error`, then close the closure's suite at that line.

### 4.3 Skim Mode And The Header Pass

The header pass ([Q4b](COMPILER_ARCHITECTURE_RESEARCH.md#design)) is the
parser's item-level code run with a body policy of `Skip`.

```rust
pub enum BodyPolicy { Parse, Skip }

// In the item parser, where the grammar expects suite_body after a header:
fn suite_body(p: &mut Parser) {
    match p.policy {
        BodyPolicy::Parse => parse_suite(p),
        BodyPolicy::Skip => {
            let range = p.lexer.skip_body(p.header_indent());  // skim mode
            p.skipped_body(range);                             // one SKIPPED_BODY node
        }
    }
}
```

- `skip_body` runs the lexer's frame machine without storing tokens. It
  stops at the first logical line, at bracket depth zero and outside any
  string, whose indentation is at most the header's and which is not blank
  or comment-only. A same-line body ends at its logical line end.
- Kept: template bodies (`impl ... by Structure`), the token ranges of
  parameter, field and shared-data defaults, and fact and decorator
  expressions, parsed as expressions but not resolved.
- Skipped: function and method bodies, `tests:` blocks (their `use` lines
  are kept and marked test-only), top-level statements.
- With answer 13 (the direct-only `block_on` ban), nothing a dependent
  needs comes from a body. There is no drive summary.

```rust
pub struct Skeleton {
    pub source_hash: Hash128,
    pub api_text_hash: Hash128,        // §4.5
    pub uses: Vec<UseDecl>,            // path, group, alias, pub, test_only, span
    pub tree: GreenTree,               // headers, with SKIPPED_BODY nodes
    pub items: Vec<SkelItem>,          // node, kind, name, visibility, body range, parent
    pub doc_comments: Vec<(u32, u32)>, // ranges only; the text stays in the source
    pub broken: bool,                  // some header failed to parse
}
```

- **Exactness.** `skeleton(skim(bytes)) == skeleton(full_tree(bytes))` on
  every fixture and std file is a slice 1 exit test. The research's script
  found the same body ends as the prototype on 2,888 files.
- **Errors.** The header pass reports nothing. A broken header marks the
  item broken (poison) and resynchronizes at the next column-0 line. The
  file is fully parsed when its module is checked, and that parse reports
  the error once.
- **When each runs** follows the table in
  [Q4b](COMPILER_ARCHITECTURE_RESEARCH.md#design): skim for files whose
  module needs no check this run; full parse for changed files, missed
  modules, `hd fmt` and `hd fix`. A fully parsed file derives its skeleton
  from its tree, so no file is read twice.

### 4.4 Parser And Green Tree

**Parser.** Hand-written recursive descent emitting matklad-style events
(`Start(kind)`, `Token`, `Finish`, `Error`) into one vector, then built into
the flat tree in one pass.

- **Recovery sets.** A `Newline` or `Dedent` ends a statement. A token at
  column 0 always starts a new top-level item, so one broken item never
  swallows the next (`errors-per-run`). Inside brackets, the closing
  delimiter and `,` are the recovery set. Junk is wrapped in an `ERROR`
  node.
- **Fuel.** Every loop that can fail to consume a token decrements a
  counter that each consumed token resets. Running out is a compiler bug:
  a debug build panics; a release build emits an `ERROR` node and skips a
  token.
- **Operators.** Precedence climbing with an explicit operand and operator
  stack, so a long chain of binary operators uses no native stack.
- **Depth.** One counter for nested brackets, blocks, closures and
  interpolations. Past the limit (§4.15) the subtree becomes an `ERROR`
  node with the limit diagnostic, and parsing resumes after it.

**Green tree.**

```rust
pub struct GreenTree {
    kinds: Vec<SyntaxKind>,     // u16
    first_token: Vec<u32>,
    last_token: Vec<u32>,       // inclusive
    subtree_len: Vec<u32>,      // preorder: node i's subtree is i .. i + subtree_len[i]
    layout: Vec<(TokenIdx, Layout)>,  // zero-width layout tokens, in order
}
#[derive(Copy, Clone)]
pub struct NodeRef<'t> { tree: &'t GreenTree, idx: NodeIdx }
```

- Preorder with subtree sizes: a child walk is a loop, and skipping a
  subtree is one addition. Parent links are built on demand for fix-its
  and the formatter.
- **Typed views** (`FnDecl`, `MatchExpr`, ...) are generated from one
  grammar file, `hd_syntax/hd.ungram`, by the codegen task. Each view is a
  `NodeRef` with accessors that find children by kind.
- **No red tree and no incremental reparse.** A whole file reparses in
  well under a millisecond at the target speed. An editor tier can add
  them later.
- About 14 bytes per node. With tokens, a 10k-line package's trees take a
  few MB, freed module by module (§3.5).
- **Named fields at the boundary (after Yuku).** The tree is the one IR
  that tools outside the checker read: the formatter, `hd doc`, fix-its,
  the playground's JavaScript and later the program database. They read
  it through the generated typed views, never through raw columns
  (§3.9.6). Storage stays compact; Yuku's 52-byte nodes with named fields
  buy the same ergonomics at four times the memory.
- **Wire format (Yuku).** Tokens, tree columns, comments and line starts
  are position-independent little-endian columns behind a small header.
  Sending a tree from the compiler worker to the page is one copy into an
  `ArrayBuffer`, and the generated JavaScript decoder reads it through
  typed-array views without building objects.
- **Recovery by truncation.** The parser's event vector is append-only.
  A speculative or failed parse truncates it to its checkpoint (§3.9.5).

### 4.5 Header Extraction And The API Text Hash

Each skeleton carries an **api text hash** (mine, as a cache key input):
the hash of the token kinds and texts of everything that can change its
folder's interface:

- every `use` and `pub use` line, except test-only ones, since names in
  signatures resolve through them;
- every declaration header, private ones included, without bodies;
- every field and variant of every `data` and `enum`, private ones
  included, since derived bounds read member types
  ([`annot.bound.params`](../spec/lang/14-annotations.md#r-annot.bound.params));
- decorators, `@derive`, `@error` and their message strings;
- template bodies, and the default and fact token ranges.

Left out: function bodies, `tests:` blocks, top-level statements, comments,
doc comments and whitespace.

The folder interface key uses these hashes instead of source hashes
(§5.3). So a body edit never rebuilds its folder's interface. A private
header edit rebuilds it, but the rebuilt blob's deep hash changes only if
the public part changed. This two-level cutoff avoids Swift's
over-invalidation, where adding a private function recompiles every user
([swift #92617](https://github.com/swiftlang/swift/issues/92617)).

### 4.6 Item Index

The item tree is an **index** over the syntax tree (for parsed modules)
and the interface (for modules known only through their folder's blob).
It copies nothing that the node or the record already holds (§3.9.1).

```rust
pub struct ItemIndex {               // per module; ItemIdx = source order
    pub at: Box<[ItemAt]>,           // u32: a header NodeIdx, or an interface record index (high bit set)
    pub parent: Box<[ItemIdx]>,      // NONE at top level; methods, variants, fields
    pub by_name: HashMap<Symbol, ItemIdx>,   // an index into the columns, not storage (§3.9.4)
}
```

Kind, name, visibility, header and body range are read through the
syntax tree's typed views or the interface reader. Flags that neither
holds (result omitted, row omitted, broken) are one bit column in the
module scope.

Derived impls (`@derive`) and error impls (`@error`, `@from`) appear as
synthetic `Impl` items in the module of their declaration
([`trait.own.module.generated`](../spec/lang/09-traits.md#r-trait.own.module.generated)).

### 4.7 Discovery, Module Identity And Folders

`hd_project` maps the file list to modules and folders from paths only:

1. Walk the source root, the test root and `tasks` through `SourceSet`.
2. Apply the module rules: `mod.hd`, `lib.hd`, `main.hd`, `_test.hd`,
   `tests/` programs and shared modules
   ([Path-Inferred Modules](../spec/lang/10-modules.md#path-inferred-modules),
   [Test Modules](../spec/lang/10-modules.md#test-modules)).
3. Validate identities: NFC identifiers and case-folding collisions
   (`duplicate-module-name`, `invalid-module-path`, `reserved-module-name`).
4. Assign folders, including the rule that `x.hd` beside a directory `x/`
   of source files lives in folder `x/`
   ([`module.folder.parent-file`](../spec/lang/10-modules.md#r-module.folder.parent-file)).

The module set always comes from this scan, never from cache entries, so a
deleted file is never served from the cache (Gleam #4320).

### 4.8 Folder Graph

1. For each non-test `use` in a file of folder `A`, find the module its
   path reaches: the longest prefix of the path that names a module.
   `use pkg.shop.Item` and `use pkg.shop.{Item}` both reach `shop`
   ([`module.cycle.folder-edge`](../spec/lang/10-modules.md#r-module.cycle.folder-edge)).
2. Add the edge `A -> folder(module)` when the folders differ and the
   module is in the same package.
3. Run Tarjan's algorithm with folders visited in path order, so the SCCs
   and their order are deterministic.
4. For each SCC with more than one folder, report `folder-cycle` once: the
   shortest loop found by breadth-first search from the SCC's least
   folder, the `use` that makes each edge, the tangle size (the SCC's
   size), and the fix-it that moves the first file `x.hd` on the loop to
   `x/mod.hd` ([Cycle Diagnostic](../spec/lang/10-modules.md#cycle-diagnostic)).
5. **Recovery (mine).** The folders of a cyclic SCC are resolved together
   as if they were one folder. Bodies still check, and the cycle is the
   only error.

Test code makes no edges. `tests:` blocks and doc tests are checked in a
**test overlay** after the folders they use (§4.13.9). Test modules form
one test unit per package, which may loop internally, after every folder.
Integration test programs are their own roots. The package graph comes
from manifests, and `package-cycle` is found the same way.

### 4.9 Name Resolution

Resolution runs inside the `FolderIface` task for headers, and inside
`ModulePrep` for the module's private signatures and bodies.

**Module scope.** The prelude, the module's uses, and its own top-level
declarations, with the shadowing and duplicate rules of
[Module Scope](../spec/lang/03-names-and-scopes.md#module-scope).

**Uses into other folders.** The target folder's interface is complete and
frozen. Its export index maps a name to the **real declaration's stable
path**: a `pub use` re-export is followed to its end when the blob is
written. A dependent never follows chains. The folder that declares the
item is a mention of the re-exporting blob, so the deep hash covers it
(TypeScript #64386).

**Uses inside the folder** may loop, and `pub use` chains may run through
several sibling modules. One serial worklist per folder resolves them:

```text
exports[m] = own pub declarations of m  ∪  { name -> Pending(use) : each pub use in m }
for each pending entry, in (module path, source order):
    follow its path through sibling exports or external interfaces,
    marking each pub use passed; reaching a marked one is re-export-loop
```

- Every `pub use` on a loop gets `re-export-loop`, and so does a plain use
  whose name leads into one
  ([`module.pub-use.chain.loop-use`](../spec/lang/10-modules.md#r-module.pub-use.chain.loop-use)).
- The work is bounded by the folder's uses times the chain length. It is a
  serial, monotone pass over one SCC (lesson 7).
- Other use errors: `unknown-module`, `unknown-import`, `private-import`,
  `direct-variant-use`, `ambiguous-import`, `test-only-use`.

**Paths in types and expressions** resolve through the module scope:
module aliases, qualified names, `Self`, type parameters, and the names
behind literal suffixes and string prefixes
([Forms Resolved By Name](../spec/lang/02-grammar.md#forms-resolved-by-name)).
An unknown name is reported once per name per body; later uses are poison.

**Orphan and coherence inputs.** For each impl head, resolution records the
owning package and module of the trait, of the target's outer constructor
and of each trait argument's outer constructor. The orphan rule
(`orphan-impl`) and the module rule (`nonlocal-impl`) are local checks on
these facts ([Implementation Ownership](../spec/lang/09-traits.md#implementation-ownership)).
The same facts make trait lookup local (§4.12.1).

### 4.10 Folder Interface Construction

```rust
pub struct FolderIface {
    pub folder: FolderId,
    pub blob: Arc<[u8]>,                     // the canonical bytes; also the cache payload; mapped when read from disk
    pub reader: IfaceReader,                 // typed views over the blob's sections (§4.11.1): items, exports,
                                             // impls, templates, hidden items, item hashes, diagnostics
    pub api_hash: Hash128,                   // shallow: the blob's api sections
    pub deep_hash: Hash128,                  // §4.11.3
    pub heads_hash: Hash128,                 // every impl head, private targets included
    pub impl_tables: Box<[(ModuleId, ImplTable)]>,  // §4.12.1, built from the impls section
}
```

The in-memory interface is its blob: the builder writes the sections as
struct-of-arrays records, and every reader, the building run included,
reads them in place (§3.9.4). There is no second, decoded copy.

**Algorithm** for folder `F`, once the interfaces of the folders it uses
are frozen:

1. Collect the skeleton items of `F`'s modules.
2. Resolve uses (§4.9).
3. Lower every header to interface types: generics with bounds, variance
   and defaults; parameters with default presence; results; rows,
   normalized after alias expansion; `!`; fields; variants with payloads
   and GADT result types; trait members, associated types, supertraits and
   default presence; impl heads with `by` delegation; aliases and
   newtypes.
4. Build derived heads. For `@derive(X)`, the impl of `X` with `T < X` for
   each type parameter in a walked member
   ([Derived Bounds](../spec/lang/14-annotations.md#derived-bounds)). For
   `@error`, `@from` and `@source`, the `Display`, `Error` and `From`
   heads.
5. Run the header checks that need nothing else: `private-type-leak`,
   `orphan-impl`, `nonlocal-impl`, `unconstrained-impl-parameter`,
   `ambiguous-row-pattern`, `misplaced-derivation`, a second template
   (`overlapping-impl`), a missing public result type or row
   ([`module.package.annotated`](../spec/lang/10-modules.md#r-module.package.annotated)),
   and duplicate declarations.
6. Build each module's `ImplTable` (§4.12.1).
7. Write the blob (§4.11), then compute item hashes, `api_hash`,
   `deep_hash` and `heads_hash`.
8. **Validate on write** (lesson 12). Every `DefId` that the api sections
   mention is exported from its folder, or is a hidden template item of
   this folder. A failure is an internal compiler error naming the item,
   not a user diagnostic, since user-facing leaks are already
   `private-type-leak` in step 5. Debug builds and verify mode also decode
   the written bytes and compare them with the in-memory interface.

**What the interface holds.** Public items, all impl heads, templates with
their bodies, hidden template items, and default and fact expressions as
token ranges with their types. It holds no private item, body, doc comment
or fact value. A check needs only a fact's type; D2 computes values at
build time from TIR (the research's Q4b contradiction 2).

**Two hashes, two readers (mine).** `api_hash` and `deep_hash` cover what
a dependent's check can read: public items, and impl heads whose target
and trait are both nameable outside their module. Impl heads for private
types go only into `heads_hash`, which coherence reads (§4.12.3). A
dependent cannot name a private type, and a private type cannot appear in
a public signature
([`module.vis.signature`](../spec/lang/10-modules.md#r-module.vis.signature)).
So adding `impl Display for PrivateThing` rechecks no dependent.

**Complexity.** Linear in the folder's header tokens plus the use worklist.
A folder task cannot be split, so one very large folder bounds the wall
time of a cold run. That is a measured risk, not designed away.

### 4.11 The Interface Blob

#### 4.11.1 Layout

```text
header      magic "HDIF", format version, toolchain key, folder stable path,
            flags, section count
sections    (kind, offset, length, Hash128) per section
strings     length-prefixed UTF-8, sorted, deduplicated
paths       StablePath records over string offsets
types       TypeRecord { tag: u8, flags: u8, a: u32, b: u32, c: u32 }
items       ItemRecord, in (module path, source order)
exports     (module, name) -> item, sorted by bytes, for binary search
impls       ImplRecord: head, bounds, owner module, by-clause, api or heads-only
templates   template headers; bodies as token text plus a resolution table
            (path node -> stable path in the trait's module)
hidden      hidden template items, in the item record shape
item_hash   (item index, shallow Hash128, deep Hash128)
mentions    (folder stable path, deep hash) of every other folder named
diags       header diagnostics with stable spans (outside every api hash)
```

- Records are fixed-width, little-endian and 4-byte aligned, with `u32`
  offsets into the blob. A reader casts `&[u8]` to record slices after
  checking bounds and alignment once per section. Natively the blob is
  memory-mapped from the cache. In the browser it is a byte array. Std's
  blobs are embedded in the binary as one pack.
- **Lazy decode.** `IfaceReader::item(name)` binary-searches the export
  index and decodes one record. `reader.ty(TypeRef)` interns a type on
  first use and memoizes it in a per-reader table. A run that names 5
  items of a 200-item std folder decodes 5 items.
- **Canonical bytes.** Records are ordered by stable content: module path,
  source order, sorted names. Never by run IDs or hash-map order. Two runs
  on the same inputs write the same bytes; the determinism matrix compares
  them (§8.1).
- **Templates.** A template body is checked again at each opt-in, in the
  deriving module ([`annot.template.checked`](../spec/lang/14-annotations.md#r-annot.template.checked)),
  but its names resolve in the trait's module. So the blob stores the body
  as token text plus the resolution table.

#### 4.11.2 Per-Item Hashes

Each item has a shallow hash (its record and the type records it reaches)
and a deep hash: the shallow hash folded with the deep hashes of the items
it mentions. Items of one folder can mention each other in loops (`User`
holds `Order`, `Order` holds `User`). So deep item hashes are computed per
SCC of the folder's item-mention graph: one hash over the SCC's members in
canonical order plus every external mention's deep item hash (mine).

The first release keys modules by folder (§5.3). Per-item hashes are
stored from day 1, so a later switch to keys per used item (Pyrefly, gopls
typerefs) changes keys, not the format (lesson 6).

#### 4.11.3 The Deep Hash

```text
deep_hash(F) = H("deep", api_hash(F), sorted [(path(G), deep_hash(G)) for G in mentions(F)])
```

- `mentions(F)` is every folder, in any package or in std, whose items
  `F`'s api sections name: in signatures, field types, bounds, re-export
  targets, template bodies and hidden items.
- The folder and package graphs are acyclic, so this is a Merkle hash
  computed bottom-up in the order the tasks already run. A cyclic SCC, an
  error, is hashed as one unit.
- **What it fixes.** A module that calls `service.load_user()` and reads
  `.name` never names `user/types`. Its key holds the deep hash of
  `service`'s folder, which mentions `user/types`. An edit to `User`
  changes the key ([Go verdict](COMPILER_PRIOR_ART_ISSUES.md#go-gotypes-the-build-cache-and-gopls)).

### 4.12 Traits, Impls And Coherence

#### 4.12.1 Owner-Module Impl Tables (mine)

hd's ownership rules say exactly where an impl may live: the module that
declares the trait, the target's outer constructor, or a trait argument's
outer constructor ([Implementation Modules](../spec/lang/09-traits.md#implementation-modules)).
So no global impl index is needed:

```rust
pub struct ImplTable {                    // per module, frozen with its folder; columns sorted by (trait, head key)
    pub trait_: Box<[DefId]>,             // DefId::NONE for inherent impls
    pub def: Box<[DefId]>,
    pub head_key: Box<[HeadKey]>,         // packed u32: kind in the high bits
    pub generic: BitBox,
    pub by_trait: HashMap<DefId, (u32, u32)>,  // an index: trait -> range of rows
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), Fn, Param }   // fast reject
```

To solve `Target: Trait[Args]`, look in the tables of at most
`2 + len(Args)` modules: the trait's, the target constructor's, and each
argument constructor's. Blanket impls (`impl[T] Tr for T`) can only be in
the trait's module. Built-in owners (tuples, functions, primitives) map to
fixed std modules. Local impls, which must involve a local type or trait,
are in a body-local table.

Every module consulted declares something the goal mentions, so it lies in
the asking module's deep dependency closure. Trait lookup therefore reads
nothing that the module's cache key does not cover. And the table a goal
needs is frozen as soon as its one folder's interface is, so bodies can
run while unrelated folders are still being resolved.

#### 4.12.2 Solving

- **Goal.** `(trait, self type, args)`, after normalizing projections whose
  base is concrete.
- **Candidates,** in order: bounds in the parameter environment; impls from
  owner tables, fast-rejected by `HeadKey`; compiler-derived impls for
  tuples at every arity and for numeric families.
- **Match** the impl head by unification, then solve its bounds as
  subgoals, depth first.
- **Cycles.** A goal already on the stack succeeds only inside a derived
  impl's member check, which is coinductive
  ([`annot.bound.recursive`](../spec/lang/14-annotations.md#r-annot.bound.recursive)).
  Elsewhere it fails with `trait-resolution-depth`.
- **Memo.** A fully concrete goal is memoized in a global sharded table as
  `(answer, steps)`. A goal with inference variables is memoized per body.
  "Type T satisfies bound B" is the most common goal and gets both (Go
  #66699). First writer wins. The answer is a pure function of frozen
  inputs, so every writer writes the same answer.
- **Budget.** Each candidate tried and each subgoal expanded costs one
  step of the body's fuel. A memo hit charges the steps stored with it. So
  whether a goal runs out depends neither on which body computed it first,
  nor on the thread count, nor on cache warmth
  ([Budgets And Schedule Independence](COMPILER_PRIOR_ART_ISSUES.md#budgets-and-schedule-independence)).
- **Method lookup.** Inherent methods of the receiver's nominal type come
  first, from the target module's inherent table. Then the methods of
  traits available in the module
  ([Trait Availability](../spec/lang/09-traits.md#trait-availability)):
  its uses, its declarations, the prelude and the receiver's bounds. Each
  such trait with the method name asks one goal. Several matches are an
  ambiguity error. None is an unknown-method error, with a `use`
  suggestion when an unavailable trait would match.

#### 4.12.3 Coherence

- Overlap is decided per trait from heads alone
  ([Overlap](../spec/lang/09-traits.md#overlap)).
- One `Coherence(trait)` task runs per trait with more than one impl in
  the program's graph (root package, dependencies, std). Its input is that
  trait's heads from the `heads` sections of every folder interface. Heads
  are bucketed by `HeadKey`; a `Param` head joins every bucket. Each pair
  in a bucket is unified after renaming apart. Numeric-family heads expand
  to their members first.
- An overlap is reported once, at the later impl in content order
  (package, module path, offset), naming the earlier one.
- The task's key is the trait's stable path plus the sorted head hashes,
  so it is cached and reruns only when a head of that trait changes.
- `hd check` runs coherence over the whole graph, not only at link time
  ([`module.interface.coherence`](../spec/lang/10-modules.md#r-module.interface.coherence)).
  It is cached and cheap, and agents see the error one command earlier.

### 4.13 Body Checking

#### 4.13.1 Task Structure Per Module

| Phase | Task | Serial or parallel | Work |
| --- | --- | --- | --- |
| M1 | `ModulePrep(m)` | serial within the module | full parse if needed; module scope; lower private headers; the module's local impl tables; infer the results of private callables with omitted result types |
| M2 | `Body(m, i)` | parallel across all bodies of all modules | check one body against frozen tables |
| M3 | `ModuleFinish(m)` | serial within the module | solve inferred rows, run deferred row checks, write the init summary, sort diagnostics, build `ModuleResult` |

**Omitted result types (M1).** A private function without a result type
must be checked before its callers. M1 checks these functions in source
order. When one calls another that is not done, M1 checks the callee
first, depth first. A callee already in progress closes a cycle:
`recursive-function-needs-result-type`, reported at the cycle's first
function in source order, whichever function was entered first. The walk
is serial and bounded by the module's item count.

**Omitted rows (M3; mine).** Rows do not order checking. A call to a
private callable with an omitted row gives the call a row variable
`RowVar(callee)`. Each body records constraints: `RowVar(f) ⊇ keys` for the
keys `f`'s body uses outside `$.with` blocks, `RowVar(f) ⊇ RowVar(g)` for a
call from `f` to `g`, and a deferred check `available(h) ⊇ RowVar(g)` for a
call from an annotated `h`. M3 solves the variables as a least fixpoint
over the module's constraint graph, one SCC at a time, with monotone union
and at most one pass per key per SCC
([`req.row.omitted.cycle`](../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.cycle)).
Then it runs the deferred checks and fills the pending provider lists in
TIR (§3.9.5).

#### 4.13.2 The Inference Engine

- **Bidirectional and local to the body.** `check(expr, expected)` and
  `infer(expr)`. An expected type flows into literals, closures, `match`
  arms, and data and collection literals.
- **Declarations are never inferred.** Generic parameters, signatures,
  public rows and bounds are what the source writes. Inference solves only
  use-site type arguments, local bindings, closure parameters from an
  expected function type, literal widths, and private results and rows as
  above.
- **Use-site type arguments.** Instantiate the callee's generics with
  fresh variables, unify parameters with arguments in source order, apply
  defaults, and solve bounds through the trait solver
  ([Inference From Several Arguments](../spec/lang/04-type-system.md#inference-from-several-arguments),
  [Inference Through A Bound](../spec/lang/04-type-system.md#inference-through-a-bound)).
  A variable still unsolved at the end is an ambiguity error.
- **Tables.** Union-find over `InferVar` with path halving and rank, a
  binding per root, and the first span that decided it, for blame. Literal
  widths follow [Literal Inference](NEW_COMPILER_ARCHITECTURE.md#literal-inference).
- **Speculation without copies.** Trying a candidate pushes bindings on a
  trail, and failure pops back to a mark. No checker state is cloned (the
  prototype's F-626).
- **No overloading, no disjunctions.** Every call resolves to one callee by
  name and receiver type, so there is no search over alternatives (the
  Swift lesson).

#### 4.13.3 Expressions And Statements

The checker follows the spec chapters in order. Points that matter for the
design:

- **Coercions** (implicit `Option` wrapping, readonly views, function row
  subsumption) become explicit TIR instructions, so D2 never re-derives
  them.
- **Mutability and access** checks run on TIR places after inference
  ([Mutable Paths](../spec/lang/04-type-system.md#mutable-paths)).
- **Typed holes.** `_` and `todo()` record the expected type and the
  in-scope names whose types fit, for the hole diagnostic.
- **Definite initialization** of locals and the `let-else` rules are a
  forward dataflow pass over the body's TIR.

#### 4.13.4 Rows

- A row is `(RowId, Option<RowParamRef>, Option<RowVar>)`. Keys are kept
  sorted by `Ty` value for in-run set operations, so union and membership
  are linear merges ([Row Sets](../spec/lang/11-requirements-and-suspension.md#row-sets)).
- Printing and hashing re-sort keys by stable content, never by `Ty` value
  (the tsgo lesson).
- Entailment is membership after alias expansion
  ([Entailment](../spec/lang/11-requirements-and-suspension.md#entailment)).
- A pattern with one unknown row parameter takes the least solution
  ([Least Row Solutions](../spec/lang/11-requirements-and-suspension.md#least-row-solutions)).
- `$.with` blocks push lexical keys onto the body's `available` set.

#### 4.13.5 Suspension

- `!` is part of the name, so suspension needs no inference.
- A bang call outside a driver context is `bang-call-outside-suspension`
  ([`req.bang.driver-contexts`](../spec/lang/11-requirements-and-suspension.md#r-req.bang.driver-contexts)).
- **The `block_on` ban is direct-only** (answer 13). In `defer` suites,
  default expressions, fact expressions and module initialization, a call
  whose resolved callee is `block_on` or `println` is an error. The check
  reads resolved callees in those contexts and no other body. An indirect
  call panics at run time; D2 emits that check.
- `Suspend[T]` values, `all!` and `race!` are typed as the spec says;
  their lowering is D2's.

#### 4.13.6 GADT Refinement

- Per `match` arm, unify the variant's result type with the scrutinee's
  type, first-order and nominal
  ([Refinement Algorithm](../spec/lang/13-gadts.md#refinement-algorithm)).
- The equalities this yields on the scrutinee's type parameters are pushed
  on the trail and popped at the arm's end, so they never escape the arm.
- A variant whose result cannot unify is impossible. Exhaustiveness skips
  it.
- Existential parameters get fresh rigid variables per arm.

#### 4.13.7 Tuples, Varargs And Arity

There are no variadic generics ([chapter 12](../spec/lang/12-variadic-generics.md)).
`Args < Tuple` bounds an ordinary type parameter, a vararg's type is a
tuple, and a spread `f(args...)` unifies a tuple with a parameter list.
Tuple rest elements are the `rest` field of the tuple type. Tuple `Eq`,
`Ord` and `Hash` come from compiler-derived candidates at every arity.

#### 4.13.8 Exhaustiveness

Maranget's usefulness algorithm over the pattern matrix, with GADT
impossibility (§4.13.6), literal ranges, `Option` and tuples.

- Each matrix cell visited costs one fuel step, so a pathological match
  stops with the match limit diagnostic instead of hanging (§4.15).
- The missing-case witness becomes the diagnostic's example, and later a
  code-generating fix-it.

#### 4.13.9 Derive Instances, Templates, Facts And Test Overlays

- **Derive instances.** Each `@derive(X)` on a type whose trait has a
  template is a body task in the type's module. It instantiates `X`'s
  template from the trait's blob for the target, then checks it with the
  template's names taken from its resolution table and the target's
  members known ([Templates](../spec/lang/14-annotations.md#templates)).
  The result is part of the target module's `check` entry. A template edit
  changes the trait folder's deep hash, which rechecks exactly the modules
  that derive from it.
- **Facts and defaults.** A fact or default expression is checked once, in
  its declaring module, as a requirement-free expression. Its value is
  D2's.
- **Test overlay.** The `tests:` block and the doc tests of module `m` are
  checked by a `TestOverlay(m)` task under `hd check --tests` and
  `hd test`. Their uses make no folder edge, so the task waits for the
  interfaces they use, and is cached as `check-test`. Doc tests are
  extracted from `##` blocks into synthetic files whose spans map back to
  their lines.

#### 4.13.10 Module Initialization

Definite initialization of top-level bindings follows transitive read sets
through function bodies
([`module.init.definite`](../spec/lang/10-modules.md#r-module.init.definite)).
Inside one module, M3 computes it from that module's TIR. But an
initialization group that spans several modules of a folder, through a use
loop, needs the bodies of all of them. That is a body-derived fact across
modules, which the research did not list.

**Design (mine).** M3 writes an **init summary** per module: for each
function and top-level statement, the top-level bindings it reads
directly, the same-folder functions it calls, and the trait methods it
dispatches ([`module.init.definite.dispatch`](../spec/lang/10-modules.md#r-module.init.definite.dispatch)).
An `InitOrder(F)` task runs only for a folder whose use graph has a
multi-module loop with top-level statements. It reads the summaries,
orders statements by [Order Inside A Group](../spec/lang/10-modules.md#order-inside-a-group),
and reports `top-level-read-before-initialization`. Its key is the sorted
summary hashes, so a body edit that does not change what a function reads
reruns nothing. Groups never span folders, so the fact stays inside one
folder.

#### 4.13.11 The Typed IR (TIR)

TIR is the one typed IR of a body. The checker emits it directly while it
checks, as Zig's Sema emits AIR and Carbon's checker emits SemIR. D2
consumes it: collection reads it, and emission walks it under a type
substitution to write Wasm (§13). There is no other tree between the
syntax tree and Wasm (§3.9.1). This section is the contract between the
checker's design and D2: the checker builds TIR only through the builder
API below, and D2 relies only on the invariants below. Its contract is
§3.10.1's TIR row.

A **body** is a function or method, an init group's statements, a test
case, a fact or a default expression. A closure is a **sub-body** of the
body that contains it, in the same columns.

##### Encoding

```rust
pub struct Tir {
    pub item: DefId,
    pub kind: BodyKind,                 // Fn | Init(group) | TestCase(n) | Fact | Default
    // instructions (§3.9.3)
    pub tags: Vec<TirTag>,              // u8
    pub data: Vec<[u32; 2]>,            // operands; meaning set by the tag
    pub ty: Vec<Ty>,                    // result type; `void` or `never` for statements
    pub syn: Vec<NodeIdx>,              // the syntax node, for spans and sites
    pub extra: Vec<u32>,
    // locals: user bindings, pattern bindings, parameters (columns)
    pub local_ty: Vec<Ty>, pub local_name: Vec<Symbol>, pub local_syn: Vec<NodeIdx>, pub local_flags: Vec<u8>,
    // sub-bodies: index 0 is the body itself, then one per closure
    pub sub_root: Vec<Inst>,            // the root Block
    pub sub_params: Vec<(u32, u32)>,    // parameter locals
    pub consts: Vec<Index>,             // constants that a Ref names
    pub side: TirSide,                  // await facts, origins of generated code, hole candidates
}
pub struct Inst(u32);
pub struct Ref(u32);                    // below 2^31: an instruction's value; else consts[ref - 2^31]
pub struct LocalId(u32);
```

- **Values are instructions.** An instruction's result is its value, used
  by later instructions through a `Ref`. Mutable user bindings are locals
  read with `LocalGet` and written with `LocalSet`.
- **Blocks list instructions.** A `Block` holds a `{start, len}` range in
  `extra` of the instructions it runs, in order. Every instruction is in
  exactly one block. The order of a block's list is evaluation order.
- **Types** are pool indices (§3.9.2). While a body is checked they may be
  body-local; the final sweep makes them global.

##### Instruction Catalog

Notation: `a` and `b` are the two data words; `[...]` is a record in
`extra`. "Join" is the arms' common type by the spec's join rules.

**Values, locals and globals**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Const` | a: pool `Index` | the constant's type |
| `LocalGet` | a: local | the local's type, as a readonly view when the binding is readonly |
| `LocalSet` | a: local, b: value | `void`; the value's type equals the local's |
| `GlobalGet` | a: `DefId` of a top-level binding | the binding's type |
| `GlobalSet` | a: `DefId`, b: value | `void`; only in init bodies and for mutable top-level bindings |
| `ItemRef` | a: `DefId`, b: `[type arguments]` | the item's function type, instantiated (functions, variant constructors, method references) |
| `ProviderGet` | a: key type | the key's provider type (`$.use(K)`) |
| `Hole` | a: expected type | the expected type; only in a module with errors |
| `Poison` | none | poison; only in a module with errors |

**Operators and calls**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Prim` | a: `PrimOp`, b: `[operands]` (one or two) | arithmetic, bitwise and shifts: the operand type; comparisons: `bool`; conversions: the target. Only on primitive types |
| `And`, `Or` | a: left, b: right `Block` | `bool`; the right block runs only when needed |
| `Call` | a: `[callee]`, b: `[arguments, providers]` | the callee's result, substituted; `mut Suspend[T]` when the callee suspends and the call is plain (a cold call) |
| `CallValue` | a: function value, b: `[arguments, context]` | the function type's result |
| `CallDyn` | a: trait value, b: `[method, arguments]` | the method's result |
| `CallHost` | a: host method, b: `[arguments]` | the method's result; only in std's provider bodies (§17.1) |
| `Intrinsic` | a: intrinsic, b: `[type arguments, arguments]` | the intrinsic's declared result |
| `Default` | a: `DefId` of the parameter or field, b: `[type arguments]` | the parameter's or field's type |
| `Interp` | b: `[parts]`, each a literal constant or a value with its `Display` callee | `string` |
| `Coerce` | a: `[kind, trait or impl]`, b: value | the target type in `ty`. Kinds: option wrap, readonly view, row subsumption, to trait value, to `Any`, suspending function to constructor, `never` to any |

A **callee record** is one of `Item(DefId, type arguments)`,
`TraitMethod(trait, method, self type, type arguments, choice)` where the
choice is the impl's `DefId` or the index of the bound in scope, and
`Evidence(value, bound, method)` for a call through a GADT existential's
stored evidence. **Arguments** are listed in parameter order; their
instructions were emitted earlier in source order, which is how named
arguments keep their evaluation order. **Providers** are one `Ref` per
key of the callee's row in key order, one context `Ref` for a
row-polymorphic callee, or `Pending(row variable)` until M3 fills it
(§3.9.5).

**Suspension and hooks**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Await` | a: `[callee]`, b: `[arguments, providers]` | the callee's result `T`. A bang call: a suspension point |
| `AwaitValue` | a: a `mut Suspend[T]` value | `T`. `s!()` on a stored suspension: a suspension point |
| `AwaitAll` | b: `[children]`, each `mut Suspend[X_i]` | `(X_1, ..., X_n)`. `all!`: one suspension point |
| `AwaitRace` | a: a `List[mut Suspend[T]]` | `T`. `race!`: one suspension point |
| `Hook` | a: `HookKind`, b: the instruction it observes | `void`; normal emission drops it (§14.7) |

Every suspension point has a side record: the `Scope`s enclosing it from
the innermost out, which are the scopes whose suites cancellation must
run there, and its hook site. Liveness is not recorded; emission computes
it (§14.2).

**Data and closures**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `NewData` | b: `[field values]` in declaration order | the data type in `ty` |
| `CopyData` | a: source, b: `[(field, value)]` replacements | the source's type. Copy-update literals and part copies; every part not replaced is copied too ([`data.part.copy-update`](../spec/lang/08-data-and-enums.md#r-data.part.copy-update)) |
| `NewVariant` | a: variant `DefId`, b: `[payload values, evidence choices]` | the enum type in `ty`; one evidence choice per bound of each existential parameter |
| `NewTuple` | b: `[elements]` | the tuple type |
| `NewList` | b: `[elements, spread bits]` | `List[T]` |
| `NewMap` | b: `[key, value pairs]` | `Map[K, V]` |
| `Field` | a: base, b: field index (parts included) | the field's type, with the base's access |
| `FieldSet` | a: base, b: `[field, value]` | `void`; the base is mutable |
| `TupleGet` | a: base, b: index | the element's type |
| `Closure` | a: sub-body, b: `[captures]`: (outer local, mode) | the closure's function type. Mode is `Copy` (no side writes it after the capture), `Move` (only the closure uses it afterward) or `Shared` (both may) |

**Providers**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `With` | a: `[(key, provider value)]`, b: body `Block` | the block's type (`$.with`) |
| `ContextNew` | a: `[(key, provider value)]` | the reusable context's type (`$.context`) |
| `ContextFor` | a: a row | the context passed to a row-polymorphic callee, built from the providers in scope |

**Control flow and cleanup**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Block` | a: `[instructions]`, b: tail value or `NONE` | the tail's type, or `void` |
| `Scope` | a: body `Block`, b: `[defer suites]`, each a `Block` | the body's type. A cleanup scope ([`flow.defer.scopes`](../spec/lang/06-control-flow.md#r-flow.defer.scopes)) |
| `Defer` | a: suite number in the enclosing `Scope` | `void`; registers that suite here |
| `If` | a: condition, b: `[then Block, else Block]` | the join of both |
| `Loop` | a: body `Block`, b: `[else Block or NONE]` | the join of its `Break` values, or `void` |
| `ForRange` | a: `[start, end, kind, loop local]`, b: `[body, else]` | as `Loop`; `kind` is half-open, inclusive or from |
| `ForList`, `ForMap` | a: `[collection, item locals]`, b: `[body, else]` | as `Loop` |
| `Break` | a: target `Loop` or `Block`, b: value or `NONE` | `never` |
| `Continue` | a: target `Loop` | `never` |
| `Return` | a: value | `never` |
| `Unreachable` | none | `never`; the default of an exhaustive switch |

An exit (`Break`, `Continue`, `Return`, or falling off a scope) runs the
registered suites of every `Scope` it leaves, innermost first. Which
scopes those are is explicit in the nesting, and cancellation at a
suspension point runs the same suites of the same scopes (its side
record). Ordinary cleanup and cancellation therefore share one source.

**Matching**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Match` | a: scrutinee, b: `[decision Block, arm Blocks]` | the join of the arms |
| `SwitchTag` | a: an enum or `Option` value, b: `[(variant, Block) cases, default Block or NONE]` | `never`: inside a decision tree, every path ends in `ToArm` or `Unreachable` |
| `SwitchInt`, `SwitchChar` | a: value, b: `[(constant or range, Block) cases, default]` | as `SwitchTag` |
| `SwitchStr` | a: value, b: `[(constant, Block) cases, default]` | as `SwitchTag` |
| `Payload` | a: a value switched to a variant, b: `[variant, field]` | the payload field's type, refined by GADT matching |
| `Unwrap` | a: an `Option` value switched to `.Some` | the inner type |
| `Guard` | a: condition `Block`, b: `[arm, fail Block]` | `never`; the fail block continues with the remaining rows |
| `ToArm` | a: arm number | `never`; the leaf has already bound the arm's locals with `LocalSet` |

The checker emits each arm's `Block` while it checks the arm, then builds
the decision tree after exhaustiveness and emits it as these switch
instructions. Shared arms appear once.

##### What The Checker Desugars

| Source | TIR |
| --- | --- |
| `a \|> f(_, b)` | `a`'s instructions, then `Call` with `a` in the placeholder's slot |
| `"x $y"` | `Interp` |
| string prefixes, literal suffixes (`10ms`) | `Call` of the resolved prefix or suffix function |
| `x[i]`, `x[i] = v` | `Call` of the resolved index or set method |
| `x += y`, and the other compound assignments | the place's operands evaluated once (their `Ref`s used twice), `Prim` or a trait `Call`, then `LocalSet`, `FieldSet` or a set `Call` |
| `x.E ...= e` | `CopyData`, then `FieldSet` |
| `[for x in xs if c => e]`, map comprehensions | a `Block`: an empty `NewList` or `NewMap`, a loop, a push `Call` |
| `e?` | `SwitchTag` on the `Result` or `Option`; the failing leaf converts the error with the resolved `From` call and `Return`s |
| `while c: body` | `Loop` whose body is `If c then body else Break` |
| `for` over a std range, list or map | `ForRange`, `ForList`, `ForMap`, chosen when the iterable's impl resolves to std's |
| any other `for` | a `Block`: the `iter` `Call`, then a `Loop` around the `next` `Call` and a `Match` on its `Option` |
| `let p = e else: ...`, `x is p` | `Match` with two arms |
| `x := e`, `let p = e` | a new local and `LocalSet`, or a `Match` for a pattern |
| `a ** b` | `Prim` on primitives, else the trait `Call` |
| `$.with`, `$.use`, `$.context` | `With`, `ProviderGet`, `ContextNew` |
| `all!(a(x), b(y))` | two cold `Call`s, then `AwaitAll` |
| `race!(...)` | a `NewList` of cold suspensions, then `AwaitRace` |
| test registrations | `Call`s in a `TestCase` body |

##### The Builder API

```rust
impl<'w> TirBuilder<'w> {
    pub fn new(item: DefId, kind: BodyKind, cols: &'w mut TirColumns) -> Self;  // the worker's reused columns

    // values (each returns the new instruction's value)
    pub fn konst(&mut self, c: Index, syn: NodeIdx) -> Ref;
    pub fn local(&mut self, ty: Ty, name: Symbol, flags: LocalFlags, syn: NodeIdx) -> LocalId;
    pub fn get(&mut self, l: LocalId, syn: NodeIdx) -> Ref;
    pub fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx);
    pub fn prim(&mut self, op: PrimOp, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref;
    pub fn call(&mut self, callee: Callee, args: &[Ref], prov: Providers, ty: Ty, syn: NodeIdx) -> Ref;
    pub fn await_(&mut self, target: AwaitTarget, ty: Ty, syn: NodeIdx) -> Ref;  // records the scope chain
    pub fn coerce(&mut self, kind: Coercion, v: Ref, to: Ty, syn: NodeIdx) -> Ref;
    pub fn emit(&mut self, tag: TirTag, a: u32, b: &[u32], ty: Ty, syn: NodeIdx) -> Ref;  // generated per tag

    // structure: each open_* takes a scratch checkpoint, each close_* flushes it (§3.9.5)
    pub fn open_block(&mut self) -> BlockMark;
    pub fn close_block(&mut self, m: BlockMark, tail: Option<Ref>, ty: Ty, syn: NodeIdx) -> Ref;
    pub fn open_scope(&mut self) -> ScopeMark;
    pub fn defer(&mut self, suite: Ref /* a closed Block */, syn: NodeIdx);
    pub fn close_scope(&mut self, m: ScopeMark, body: Ref, syn: NodeIdx) -> Ref;
    pub fn open_loop(&mut self) -> LoopMark;   // Break and Continue name its LoopMark
    pub fn sub_body(&mut self, params: &[LocalId]) -> SubMark;  // a closure's body

    // speculation (§3.9.5)
    pub fn checkpoint(&self) -> Checkpoint;
    pub fn rollback(&mut self, c: Checkpoint);

    // end of the body: the final type sweep, then the verifier in debug builds
    pub fn finish(self, solution: &Solution) -> TirBody;
}
```

- The builder is the only writer of TIR. `emit` and the per-tag helpers
  are generated from `tir.ir` (§3.9.6), so every encoding goes through the
  schema.
- Types may be inference variables while a body is built. `coerce` is
  called where bidirectional checking finds a coercion, so coercions are
  never re-derived later.
- `finish` resolves every `ty` to a global type, rejects leftovers, and
  in debug builds runs the verifier.

##### Invariants

The verifier checks each of these:

1. Every operand precedes its user, and is visible where it is used: in
   the same block earlier, or in an enclosing block.
2. Every instruction is in exactly one block list.
3. After `finish`, every type is global. `Hole` and `Poison` occur only
   in a module with errors, which D2 does not lower.
4. **Every operand's type equals the type its position expects**: call
   arguments equal the substituted parameter types, a `LocalSet` value
   equals the local's type, a `Return` value equals the body's result, a
   `Break` value equals its target's type. Coercions are explicit, so any
   mismatch is a checker bug.
5. `Break` and `Continue` targets enclose them. `ToArm` is inside its
   `Match`'s decision block. `Defer` is inside its `Scope`.
6. `Await`, `AwaitValue`, `AwaitAll` and `AwaitRace` occur only in a
   suspending body or closure. No suspension point, `Return`, `Break`,
   `Continue` or failing `?` leaf is inside a `defer` suite.
7. Every suspension point's side record lists exactly the `Scope`s that
   enclose it.
8. Every call to a callee with a row has providers for exactly its keys,
   and none is `Pending` after M3.
9. `CallHost` occurs only in std's provider bodies; an `Intrinsic` only
   where it is declared.
10. A `Closure`'s captures name locals of its enclosing sub-bodies.
11. Every switch in a decision tree has a case for each value of its
    type, or a default.

##### Lifetime And The `tir` Entry

- A body's TIR lives in its worker's columns until the body finishes, then
  moves into the module result. `ModuleFinish` fills pending providers,
  serializes every body of the module into the `tir` entry (column
  copies, §3.9.8), and frees them. A check never keeps TIR, and a later
  build reads the entry instead of rechecking (§3.10.2).
- The entry's key is `H("tir", check_key(m))`. It also stores, per item,
  a **TIR hash** (the hash of that item's serialized columns, closures
  included) and a **dependency list**: the stable paths of the items its
  TIR names, each with its per-item interface hash. Instance keys use both
  (§13.8).
- Types stay generic (`Param`). D2 never materializes an instance as TIR.

### 4.14 Diagnostics

**One per root cause.**

1. Poison silences follow-ups (§3.6).
2. Each diagnostic carries a `RootKey`: the item, plus the name or span
   that caused it. `ModuleFinish` keeps the first diagnostic per root key
   in content order and drops the rest.
3. An unknown name reports once per body, a broken header once per item, a
   failed import once per use.
4. A header error in module `m` is reported in `m`, never again in its
   dependents, which see poison.

**Order.** Every diagnostic is sorted by package order, file stable path,
start offset, end offset, code and rendered message. The key is total, so
no tie falls back to an ID (tsgo #64589). Exact duplicates are dropped.

**Fix-its.**

- A fix-it is a list of exact edits on byte ranges.
- **Re-parse check at emission.** Before a fix-it is marked `Exact`, its
  edits are applied to a copy of the file text, which is re-lexed and
  re-parsed. If the parse gains an error, the fix-it is demoted to
  `Suggestion`: shown, but never applied. Only the first 20 fix-its per
  file are verified this way; the rest stay `Suggestion`.
- **`hd fix`** adds a type check of the result (§7.6).
- First-release kinds: did-you-mean names, missing `use` lines, `let mut`,
  the folder-cycle move, and the fix-its the spec names. Code-generating
  fix-its come later.

**Rendering.**

- **Compact text** (default): one line `file:line:col: code: message`, and
  at most one `hint:` line naming the fix. Notes and secondary labels
  appear with `--verbose`. The target is at most 60 tokens per diagnostic
  (the `mistakes` metric).
- **JSON lines** with the fields of
  [Machine Output](../spec/cli/command-line.md#machine-output), plus
  `fixes` (edits with file, byte range and text) and `notes`. Adding these
  fields is CLI spec work for the spec pass.
- **Capping.** `--max-errors N` stops printing after N errors and still
  prints the summary with full counts. A summary mode prints counts per
  code and file.
- **`modules_checked`** in the JSON summary is the number of module
  `check` entries computed in this run, not read from the cache (answer 6).

### 4.15 Limits

Each limit counts language-level units, so a program passes or fails the
same way on every run, thread count and cache state (lesson 3). Code names
marked "proposed" are for the spec pass (answer 5).

| Limit | Counts | Default | Diagnostic |
| --- | --- | --- | --- |
| file size | bytes | 16 MiB | `file-too-large` (proposed) |
| nesting depth | brackets, blocks, closures and interpolations open at once | 256 | `nesting-too-deep` (proposed) |
| trait resolution depth | nested subgoals | 64 | `trait-resolution-depth` (spec) |
| body fuel | candidates, subgoals, unification steps, matrix cells; memo hits at their stored cost | 2,000,000 steps per body | `item-too-complex` (proposed), naming the item |
| type size | nodes in one type | 10,000 | `type-too-large` (proposed) |
| exhaustiveness | matrix cells, within the body's fuel | shares body fuel | `match-too-complex` (proposed) |
| embedding depth | nested `data` embedding | the spec's | `embedding-too-deep` (spec) |
| instantiation depth | nesting depth of an instance's type arguments; length of the request chain from a root | 32; 256 | `instantiation-too-deep` (proposed; §13.4) |
| memory | process bytes | **none by default** (owner, 2026-10-07); opt in with `--max-memory` or `HD_MAX_MEMORY` | `memory-limit` (proposed), naming the stage |

**The opt-in memory cap.** By the owner's decision there is no default
cap. When one is set, a counting wrapper around the global allocator keeps
the process total in one atomic counter. The scheduler checks it before it
starts each task, and fuel checks it every 4,096 steps. Over the cap, no
new task starts, running bodies stop at their next check, and `hd` reports
one hard-limit diagnostic. It names the stage, such as "folder interfaces"
or "body checking", and the items in flight in content order. Then `hd`
exits with status 101. When a cap is hit depends on scheduling, so this is
the one limit that is not deterministic, and it is off unless asked for.

## 5. Cache

### 5.1 Where Things Live

| Data | Location | Shared | Why |
| --- | --- | --- | --- |
| interface blobs, check results, TIR, coherence and init results, package results; D2's code | `$HD_CACHE/obj/` | per user, across worktrees and packages | keys are content hashes, so worktrees on one commit share everything |
| fetched dependency trees | `$HD_CACHE/pkg/` (spec today) | per user | [Cache](../spec/cli/command-line.md#cache) |
| staging for atomic writes | `$HD_CACHE/tmp/` | per user | same file system as `obj/`, so rename is atomic |
| stat manifest, last test record | `build/.hd/` of each package | per worktree | holds paths and mtimes, which must not enter shared keys |
| std interfaces | embedded in the `hd` binary | per binary | no cache read for std ([Q2](COMPILER_ARCHITECTURE_RESEARCH.md#recommendation-1)) |

`$HD_CACHE` resolves as [`cli.cache.directory`](../spec/cli/command-line.md#r-cli.cache.directory)
says. The spec's `hd clean --cache` today refuses a cache directory that
holds anything but `pkg`, `hash` and `tmp`
([`cli.clean.cache.layout`](../spec/cli/command-line.md#r-cli.clean.cache.layout)),
so `obj/` needs a spec change. That is open question 1.

### 5.2 Entry Kinds

| Kind | Content | Written by | Read by |
| --- | --- | --- | --- |
| `iface` | a folder interface blob (§4.11); its header carries `api_hash`, `deep_hash`, `heads_hash` | `FolderIface` | dependents' key computation, resolution, coherence, `hd doc` |
| `check` | a module's diagnostics, init summary, row results, and fact records | `ModuleFinish` | output, `InitOrder`, D2 |
| `tir` | a module's TIR, per-item TIR hashes and dependency lists (§4.13.11) | `ModuleFinish` | D2 |
| `check-test` | the test overlay's diagnostics and test registrations | `TestOverlay` | `hd check --tests`, `hd test` |
| `coh` | one trait's overlap diagnostics | `Coherence` | output |
| `init` | one folder's statement order and its diagnostics | `InitOrder` | output, D2 |
| `pkgres` | the package's sorted diagnostics and summary counts | `PackageResult` | the warm fast path |
| `depfiles` | a fetched dependency's file list with content and api text hashes | first use of the dependency | every later run (§5.5) |
| `code`, `link`, `cwasm` | Wasm per instance, per program, and precompiled per engine (§11.2) | D2 | D2 |

**Fact records** (the program-database hook,
[Q6](COMPILER_ARCHITECTURE_RESEARCH.md#q6-program-database-hook)) are a
section of each `check` entry: declarations with spans and signature text,
resolved references with spans, call edges, impls and derives, inferred
rows, and the diagnostics. Symbols are printed stable paths. No schema is
public yet; a merged index comes before `hd callers` ships (prior-art
change 13).

### 5.3 Key Composition

Every key is `H(kind tag, fields...)` over canonical bytes, with xxh3-128.

```text
toolchain_key = H("tc", compiler build id, std pack hash, target ("wasm32-gc"),
                  host profile table hash, cache layout version, hash algorithm id)

iface_key(F)  = H("iface", toolchain_key, package key, folder path,
                  sorted [(module path, role, api_text_hash) for each file of F],
                  sorted [(folder path, deep_hash) for each folder that F's uses reach])

check_key(m)  = H("check", toolchain_key, package key, module path, role, source_hash(m),
                  deep_hash(own folder),
                  sorted [(folder path, deep_hash) for each folder that m's uses reach])

test_key(m)   = H("check-test", check_key(m), source_hash(m),
                  sorted [(folder path, deep_hash) for each folder the test code uses],
                  sorted dev-dependency keys)

coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, folder path, sorted [(module path, init summary hash)])
pkgres_key    = H("pkgres", sorted check, test, coh and init keys, folder graph hash,
                  manifest diagnostics hash, command mode)
fast_key      = H("fast", toolchain_key, package key, sorted [(path, source_hash)] of every
                  file, sorted dependency keys, command mode)
```

- **Package key.** `root` for the package being built, never its path.
  A fetched dependency is `HOST_PATH@VERSION` plus its tree hash. A path
  dependency is its workspace-relative path plus its own file hashes.
- **Command mode** is the set of options that change what is checked:
  `--tests`, `--all`, a FILE scope. Output options (`--format`,
  `--max-errors`) are not in keys, since entries hold structured
  diagnostics and rendering happens at output.
- **What a key never contains** (lesson 9): absolute paths, the working
  directory, the `hd` binary's location, mtimes, user names, environment
  variables, thread counts, run IDs. The path-independence test checks one
  package from two checkout paths and compares every key byte for byte.
- **What changing an input reaches** (lesson 1 and TypeScript #64552): the
  std version, the target and the host profile table are in
  `toolchain_key`, so changing any of them misses every entry.
- **The fast path (mine).** `fast_key` names a `pkgres` entry directly, so
  a warm run with no edit reads the manifest and one entry. When it
  misses, the run computes the real keys bottom-up and still reuses every
  entry that did not change.
- **Hermetic by API.** A module's check receives its source and the
  interface handles its key names. It cannot reach the file system or
  another module's source, so it cannot read an input its key misses
  ([Q2](COMPILER_ARCHITECTURE_RESEARCH.md#recommendation-1)).

### 5.4 Entry Format And Atomic Publish

```text
obj/
  LAYOUT                      "hd-obj 1 xxh3-128"
  <kind>/<2 hex>/<32 hex>     one file per key
  size                        approximate total bytes, for eviction (§5.7)
  gc                          time of the last full scan
```

- **Entry header:** magic, kind, layout version, the key itself, payload
  length, and an xxh3-64 checksum of the payload. A reader checks all of
  them. A mismatch, such as a truncated or corrupt file, counts as a miss,
  and the file is deleted.
- **Publish:** write to `$HD_CACHE/tmp/<random>`, flush, rename to the
  final path. On a platform where rename does not replace, an existing
  final file wins. Entries are immutable, and two writers of one key write
  the same bytes, so a lost race costs only the duplicate work.
- **No locks on reads.** Readers open and map files. Writers never modify
  a file in place.
- **Dedup across processes** is not coordinated: two processes that miss
  the same key both compute it. Module entries take milliseconds. This
  conflicts with the proposed `cache-contention` target "each entry
  computed once", which is open question 3.
- **Write failures** (read-only or full disk) produce one warning and the
  run continues uncached. A cache never makes a check fail.
- **Large entries** (blobs over 64 KiB, D2's code) are memory-mapped.
  Small ones are read whole.

### 5.5 The Stat Manifest And Change Detection

```rust
pub struct Manifest {            // build/.hd/manifest, one per package per worktree
    pub written_at: Timestamp,
    pub files: Vec<ManifestFile>, // sorted by path
}
pub struct ManifestFile {
    pub path: RelPath, pub size: u64, pub mtime_ns: i128, pub inode: u64,
    pub source_hash: Hash128, pub api_text_hash: Hash128,
    pub uses: Vec<ModulePath>,   // for the folder graph and --affected, without reading the file
    pub role: FileRole,
}
```

1. **List.** Walk the roots' directories every run. A file that the
   manifest lists but the walk does not find is deleted; a file that the
   walk finds but the manifest lacks is new (Gleam #4320).
2. **Stat** every file. The research's `io-per-check` target expects
   stats proportional to the file count.
3. **Trust** a record when size, mtime and inode are equal, and the mtime
   is earlier than `written_at`. A file whose mtime is at or after
   `written_at` is "racily clean" and is hashed anyway (git's rule). This
   covers same-second writes on file systems with coarse timestamps.
4. **Hash** every other file. A restored file with an old mtime has a
   different inode or size, or its content hash decides.
5. **Write** the manifest atomically at the end of the run if anything
   changed. A concurrent run in the same worktree may overwrite it; that
   is safe, because every record is verified by stat, and a mismatch means
   re-hashing.
6. **Dependencies are not stat'ed.** A fetched version is read-only and
   verified by its tree hash ([`cli.cache.hash`](../spec/cli/command-line.md#r-cli.cache.hash)).
   Its file hashes and use lists are a `depfiles` entry keyed by the tree
   hash, computed once per machine.

### 5.6 Verify Mode

`HD_CACHE_VERIFY=1` turns every cache hit into a recompute and a byte
comparison. A mismatch is an internal error that names the entry kind,
the key, and a structural diff of the two decoded entries, and exits with
status 101. CI runs the conformance suite and the incremental-soundness
fuzzer in verify mode, as rustc's incremental verification does.

### 5.7 Eviction And The Size Cap

Owner, 2026-10-07: the shared cache defaults to **10 GB**, evicted
least-recently-used. Eviction runs automatically when the cache is over
the cap, `hd cache gc` runs it on demand, and the cap is configurable.

- **Use marks.** A cache hit touches the entry's mtime if it is more than
  an hour old (Go's rule), so the mtime is the last-use time to within an
  hour, at most one write per entry per hour.
- **Size accounting.** Each run adds the bytes it wrote to `obj/size`,
  under a short advisory file lock, once at the end of the run.
- **Automatic eviction.** When the total passes the cap, the run that
  noticed it scans `obj/` after its output is flushed, deletes entries in
  order of oldest use until the total is at most 90% of the cap, and
  writes the exact total back. A full scan also runs at least once a day
  (`obj/gc`), which corrects drift in `size`.
- **`hd cache gc`** runs the same scan and eviction now, and prints the
  bytes before and after.
- **The cap** comes from `HD_CACHE_MAX_SIZE` (bytes, with `K`, `M`, `G`
  suffixes). Whether a manifest key or a user config file can also set it
  is a CLI spec detail.
- **Safety.** Eviction deletes only `obj/<kind>/` files whose names parse
  as keys, never follows a symbolic link, and never touches `pkg/`.
- A process holding a mapped entry keeps working if another process
  deletes it, because unlinking a mapped file is safe on Unix. On Windows,
  eviction skips files it cannot delete.

### 5.8 The Browser `CacheStore`

```rust
pub trait CacheStore: Sync {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<EntryBytes>;  // synchronous
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]);
    fn touch(&self, kind: EntryKind, key: &Hash128);
}
pub struct MemoryStore { entries: Mutex<HashMap<(EntryKind, Hash128), Arc<[u8]>>>, new: Mutex<Vec<..>> }
```

- The core never awaits storage. Before a run the JS host loads the
  playground's entries from IndexedDB into the `MemoryStore`. After the
  run it drains the new entries and writes them back.
- Keys are the same as native. `toolchain_key` keeps a new compiler build
  from reading old entries.
- The IndexedDB store keeps a last-use time per entry and evicts the
  oldest past a small browser cap, such as 50 MB, when it loads.
- Std is embedded, and playground programs are small, so a cold check is
  the normal browser case. The store mainly speeds up "Run" after "Check".

## 6. Scheduler And Task Graph

### 6.1 Tasks

```rust
pub enum TaskKind {
    Skim(FileId), Parse(FileId), FolderGraph(PackageId),
    FolderIface(FolderId), ModulePrep(ModuleId), Body(ModuleId, ItemIdx), ModuleFinish(ModuleId),
    TestOverlay(ModuleId), Coherence(DefId), InitOrder(FolderId), PackageResult(PackageId),
    Ext(ExtTask),                       // D2's tasks, behind a trait object
}
struct TaskNode {
    kind: TaskKind,
    waiting_on: AtomicU32,              // unfinished dependencies
    successors: Mutex<SmallVec<[TaskId; 4]>>,
    priority: u32,                      // §6.3
    state: AtomicU8,                    // Waiting | Ready | Running | Done | Cancelled
}
pub struct TaskGraph { nodes: AppendVec<TaskNode> }
```

- **Creation is dynamic.** The driver creates the discovery tasks. A
  finished task may add tasks and edges, for example the folder graph task
  creates one `FolderIface` per folder whose key missed. An edge to a task
  that is already done counts as satisfied at once.
- **Results** go to per-kind slot vectors (`OnceLock<T>` indexed by the
  file, folder or module ID). A task reads only slots of tasks it depends
  on, so reads never race with writes.
- **Cache checks are tasks too.** A key can be computed only when the
  deep hashes it names are known, so "compute key, look up, skip or run"
  is the first step of each `FolderIface` and `ModuleFinish` path. A hit
  marks the subtree below it done without creating it.

### 6.2 Executors

```rust
pub trait Scheduler: Sync {
    fn run(&self, graph: &TaskGraph, exec: &(dyn Fn(TaskId, &Spawner) + Sync));
    fn threads(&self) -> usize;
}
pub struct SerialScheduler { order: SerialOrder }       // Fifo | Shuffled(seed): tests only
pub struct PoolScheduler { pool: rayon::ThreadPool }    // feature "threads"
pub struct SteppingScheduler { .. }                     // browser: run_for(max_steps) -> Progress
```

- **Pool.** Ready tasks are spawned into rayon's work-stealing pool. Each
  worker keeps its own bump arena for body tasks. The default thread
  count is open question 2.
- **Serial.** One ready queue ordered by priority, then creation order.
  It is the `--threads 1` mode and the browser's base. Its `Shuffled` mode
  picks among ready tasks by a seeded random choice, which finds order
  dependence without threads (§8.1).
- **Stepping (mine).** The browser runs the serial scheduler in slices:
  `run_for(steps)` returns to JavaScript between slices, so the worker can
  receive a "source changed" message and cancel the run.

### 6.3 Priority

Priority only affects speed, never output.

- `FolderIface` tasks are ordered by their height in the folder DAG,
  longest path to a sink first, so the critical path starts early.
- `Body` tasks are ordered by body size, largest first (longest
  processing time first), so one huge body does not start last.
- Tasks that unblock many others (`ModulePrep`, `FolderGraph`) come first.

### 6.4 Budgets, Cancellation And The Memory Cap

- **Fuel** is per body (§4.15). A memo hit charges its stored steps.
- **Cancellation** is an atomic flag. A task checks it when it starts,
  and fuel checks it every 4,096 steps. It is set by the stepping
  scheduler on a newer edit, by the opt-in memory cap, and by Ctrl-C.
  `--max-errors` does not cancel checking: output is in content order,
  and counts in the summary stay exact.
- **A panic in a task** is caught at the task boundary, reported as an
  internal error naming the task's item, and the rest of the run goes on.
  The command still exits with status 101.
- **The memory cap** is opt-in (§4.15).

### 6.5 Deterministic Output Assembly

1. **Content order everywhere.** Anything printed, hashed or used to
   break a tie is ordered by stable paths, source positions and codes,
   never by IDs, pointers or hash-map order (lesson 2). IDs have no `Ord`
   (§3.1), and output maps are `BTreeMap`s keyed by content.
2. **Local counters.** Inference variables, closure indices and generated
   names are numbered per body or per item, never per session (rustc's
   leaked allocation IDs).
3. **Streaming in order (mine).** Diagnostics are grouped by module.
   A release cursor walks modules in content order and prints a module's
   diagnostics once it and every module before it are finished. An agent
   sees the first errors early, and the bytes are the same as a run that
   prints at the end. Folder, coherence and init diagnostics belong to the
   module of their primary span.
4. **Cycle diagnostics** name the cycle's first member in source or path
   order, whichever thread found it.
5. **The summary** comes last, with counts over the whole run.

## 7. Command Flows

### 7.1 `hd check` (Package Mode)

1. Find the package or workspace ([Package Mode](../spec/cli/command-line.md#package-mode)).
   Parse `hd.toml`, select dependency versions, verify `hd.sum`, and
   fetch what is missing ([Fetching](../spec/cli/command-line.md#fetching)).
2. Open the session: `toolchain_key`, the embedded std pack (mapped, not
   decoded), the cache store, the scheduler.
3. Walk and stat the files; diff against the manifest (§5.5).
4. Compute `fast_key`. On a hit, print the stored `pkgres` and stop.
5. Build the folder graph from manifest use lists, plus skims of the
   changed files.
6. Bottom-up over folders: compute `iface_key`; on a hit, read the
   entry's header for its hashes; on a miss, skim the folder's files and
   run `FolderIface`.
7. For each module, compute `check_key`; on a hit, take its result; on a
   miss, parse it and run M1 to M3.
8. Run `Coherence` for traits whose `coh_key` missed, and `InitOrder` for
   folders that need it.
9. Stream diagnostics in content order; print the summary; write the
   `pkgres` entry, the manifest, and possibly run eviction.

Touched: `iface`, `check`, `coh`, `init`, `pkgres`. Dependency bodies are
never parsed: with answer 13, interfaces come from syntax alone, so the
research's "dependency bodies skipped" now holds cold as well as warm.

### 7.2 `hd check FILE`

1. As steps 1 to 6 above, but only for the folders that FILE's module
   deeply depends on, plus its own folder.
2. Check only FILE's module (its `check` entry, and its test overlay when
   FILE is test code).
3. Coherence runs only for traits with an impl in FILE.
4. Output: FILE's diagnostics, plus one note per used folder whose
   interface has errors ("src/shop has 2 errors; run hd check").
5. A FILE under no root is a single-file program
   ([`cli.package.no-root`](../spec/cli/command-line.md#r-cli.package.no-root)):
   one module, std only, no manifest.

### 7.3 `hd test` (Up To D2)

1. `hd check --tests`: also test overlays, test modules (one test unit),
   integration test programs (each its own root over the package's public
   interfaces, [`module.test.integration.view`](../spec/lang/10-modules.md#r-module.test.integration.view)),
   and doc tests.
2. On errors, print them and stop with status 101.
3. Build the **test plan**: the list of test programs and their cases.
   Unit tests per module with test code; one program per integration test
   file; doc tests grouped per module. Registration names are string
   literals ([Registration Functions](../spec/lang/10-modules.md#registration-functions)),
   so the checker lists each program's cases without running anything.
   Only the row count of an `it_each` case is learned at run time.
   `--filter` is applied to this list.
4. Hand the plan to D2, which builds, runs and reports. The package's
   executables are built first ([`cli.test.builds-executables`](../spec/cli/command-line.md#r-cli.test.builds-executables)).
   §20.1 continues this flow.

### 7.4 `hd test --affected`

1. Read `build/.hd/last-test`: the source hash of every module at the last
   test run, and that run's failing programs.
2. A module is changed when its source hash differs. Body edits count,
   because tests run code.
3. Take the reverse closure of changed modules over the module use graph,
   built from manifest use lists, test uses included. No file is parsed
   for this.
4. Select the test programs whose roots are in the closure, plus the
   programs that failed last time.
5. Continue as `hd test` with that plan, and write the new record.

### 7.5 `hd run`, `hd build`

- **`hd run [NAME]`:** check the package without test code, then hand the
  chosen executable's entry module to D2. D2 builds from the reachable
  items and runs under the default profile.
- **`hd build`:** check the whole package. On errors, stop. Otherwise D2
  writes `build/debug/NAME.wasm` per executable, or `build/release/` with
  `--release`.
- **A library-only package** (owner, 2026-10-07): `hd build` checks it and
  writes only its interface and cache entries. It writes no `.wasm` file.

§20.2 to §20.4 continue these flows.

### 7.6 `hd fmt`, `hd fix`

**`hd fmt`.** Per file, in parallel:

1. Skip the file when its manifest record says its current source hash is
   already formatted (a per-file bit, mine).
2. Full parse. A file with a syntax error is not formatted; its first
   syntax error is printed. Whether that is right is open question 6.
3. Lower the green tree to a Wadler-style document and print it within the
   line width. Comments are trivia in gaps and are kept.
4. Write changed files atomically. `--check` lists files that would
   change.

No type checking and no cache entries. Idempotence is tested on every
fixture (`fmt(fmt(x)) == fmt(x)`).

**`hd fix`.**

1. Run `hd check` with fix-its.
2. Take the `Exact` fix-its. Where two overlap, the first in content order
   wins.
3. Apply them per file, re-parse each edited file, and drop any fix whose
   file gains a syntax error.
4. Re-check incrementally: only edited modules and their dependents miss
   the cache.
5. If an edited module has a diagnostic with a new root key, revert that
   file's fixes one at a time until it has none.
6. Repeat up to 3 rounds, write the files, and print what was applied and
   what remains.

### 7.7 `hd doc`

- **Whole package:** build interfaces (no bodies), then render
  [HD_DOC.md](HD_DOC.md) pages from `iface` entries plus doc comment
  text, read from source through the skeleton's ranges.
- **`hd doc NAME`:** resolve NAME through the package's exports or a
  dependency's, skim the one declaring file for its doc comment, and print
  the item from its interface record.
- Doc comments are never in an interface hash, so a doc edit rechecks
  nothing.

### 7.8 `hd repl`

1. In package mode the session acts as code inside `src/lib.hd`
   ([`cli.repl.package.lib`](../spec/cli/command-line.md#r-cli.repl.package.lib)):
   run `ModulePrep` for `lib.hd` to get its private scope.
2. Each input is a synthetic file `repl#n` in folder `src`. Its scope is
   `lib.hd`'s scope, the uses of every earlier input, and the earlier
   inputs' top-level bindings with their fixed types
   ([`cli.repl.input-types`](../spec/cli/command-line.md#r-cli.repl.input-types)).
3. Check it as top-level statements of a one-input module, then hand it to
   D2, which runs it in the session's persistent program instance.
4. Each input's arenas are freed after it runs. Only interned names and
   types persist, and they grow with new names only, which keeps
   `long-session` flat.

### 7.9 The Playground

```text
 page (main thread)          compiler worker                  program worker
 ──────────────────          ───────────────                  ──────────────
 editor edits ─────────────► SourceSet (JS map, versioned)
 "check" ──────────────────► SteppingScheduler.run_for(...) loop
                             ◄── JSON lines (same as the CLI)
 "run" ────────────────────► check + D2 build ──► Wasm bytes + import list ──► instantiate on V8
                                                                              with generated JS host
 console, http ◄──────────────────────────────────────────────────────────── host calls
 "stop" / timeout ──────────────────────────────────────────────────────────► terminate worker
```

1. The compiler worker loads `hd_web`'s Wasm once; std is embedded.
2. Before a run the page's IndexedDB entries fill the `MemoryStore`.
3. Sources cross as UTF-8 bytes per path with a version counter; unchanged
   paths are not re-sent.
4. Checking uses the stepping scheduler. A newer edit cancels the run
   between slices; the session keeps its interners and frozen interfaces
   for the next run.
5. "Run" hands the built Wasm bytes to a separate program worker, which
   instantiates them with D2's generated JS host. Stopping or timing out a
   program terminates that worker only, so the compiler's warm state
   survives (mine).
6. After each run the worker drains new cache entries to IndexedDB.

## 8. Determinism And Soundness Tests

### 8.1 The Determinism Matrix

Each case runs over the product of these dimensions. All stdout, stderr,
JSON lines, cache keys and entry bytes must be identical.

| Dimension | Values | Catches |
| --- | --- | --- |
| threads | 1, 2, 4, 16 | schedule-dependent output |
| serial order | FIFO, shuffled with 3 seeds | order dependence without threads, in the browser build too |
| hasher seed | 3 seeds for every in-memory map (`HD_DEBUG_HASH_SEED`) | hash-map order leaking into output (Gleam #6383) |
| ID shift | the interners pre-filled with 0, 1,000 or 50,000 junk entries (mine) | any interned ID that is printed, sorted or hashed |
| checkout path | two paths of different lengths | paths in keys (Swift's module variants) |
| cache state | cold; warm; warm after an unrelated edit; verify mode | hits that differ from recomputation |
| platform | Linux, macOS, the browser build under Node | platform-dependent ordering or hashing |

The cases are the conformance suite, std, and three generated packages of
1k, 10k and 50k lines. CI runs a sample of the product on each change and
the full product nightly.

### 8.2 Incremental Soundness

An edit-script fuzzer applies random edits to a generated or fixture
package. After each edit it runs an incremental check and a clean check
with an empty `HD_CACHE`, and compares their outputs and entries byte for
byte. The edit vocabulary:

| Edit | Expected recheck (for `recheck-precision`) |
| --- | --- |
| private function body | that module only |
| private header (signature, new private item) | that module; its folder's interface is rebuilt with the same deep hash |
| public signature | the folder's modules and every module whose key includes a changed deep hash |
| field type of a type reached only through a used signature (`service.load_user().name`) | the reader of `.name`, through the deep hash |
| add, remove or redirect a `pub use` in a chain | every module that uses the re-exported name |
| add or remove an impl for a public type; a blanket impl | dependents, and that trait's coherence |
| add or remove an impl for a private type | that module and the trait's coherence; no dependent |
| add or remove `@derive`, edit a template body in another package | the modules that derive the trait |
| edit a top-level statement read in a multi-module init group | that module and the folder's init order |
| edit a doc comment or a comment | nothing |
| delete a file, then restore it with its old mtime | the right modules both times |
| move `x.hd` to `x/mod.hd` | the folder's and its dependents' modules |
| touch without change; rewrite within the same second | nothing; the change is found |
| change a dependency version in `hd.toml`; change the std version | dependents of that package; everything |
| switch `--tests` on and off | test overlays only |
| check from a second checkout path | nothing (all keys equal) |

Sorbet's lesson is tracked too: `recheck-precision` reports the share of
edits in a scripted session that recheck more than one module.

### 8.3 Differential And Conformance Testing

- **Portable conformance suite.** The new compiler answers the command
  contract (`hd parse`, `hd check`, `hd test`, exit codes, `code:` in
  diagnostics; [portable README](../test/portable/README.md)). Its own
  `KNOWN_FAILURES.tsv` is the spec sync, as the prototype's is.
- **Against the frozen prototype.** Run both on the conformance suite and
  on generated programs; compare accept or reject and the set of
  diagnostic codes per file, not messages. Each disagreement is triaged as
  a prototype bug or a new-compiler bug.
- **Header pass against full parse** on every fixture and std file (§4.3).
- **Pathological suite** with an ill-typed variant of each case (Swift's
  lesson), each within its time budget and with its limit diagnostic.
- **Fuzzing.** `cargo fuzz` on the lexer, layout and parser for panics and
  round-trip failures. Program generators live under `test/` as portable
  tools that take any `hd` binary.

## 9. Build Order

Consistent with [Q7](COMPILER_ARCHITECTURE_RESEARCH.md#build-order),
updated for answer 13: no drive summary in any slice.

| Slice | Delivers | Exit test |
| --- | --- | --- |
| 1. Syntax | `hd_base`, `hd_intern`, `hd_diag`, `hd_syntax`: lexer, layout cursor, parser, green tree, typed views, skim mode and skeletons; `hd parse FILE` | parse-phase portable cases pass; every fixture and std file round-trips byte for byte; skim and full-parse skeletons agree on every file; lexer and parser fuzzed with no panic; `hd_web` builds for `wasm32-unknown-unknown` in CI; lexing and parsing throughput recorded separately |
| 1b. Formatter (beside slice 2) | `hd_fmt`, `hd fmt` | idempotent on every fixture and std file; `fmt` timing recorded |
| 2. Std interfaces | `hd_project`, `hd_iface`, `hd_types` (types and rows), `hd_resolve`, `hd_sched` serial, `hd_cache` memory store, `hd_driver`; keys computed from the start | std's folder interfaces build with no diagnostic; name-phase cases pass (`unknown-import`, `private-import`, `re-export-loop`, `folder-cycle`, `private-type-leak`, `orphan-impl`, `nonlocal-impl`); blobs decode to equal interfaces; blob bytes equal across shuffled serial orders; a deep-hash test changes a type reached only through a signature and sees the dependent's key change |
| 2b. `hd doc` (beside slice 3) | `hd_doc` on interfaces | HD_DOC cases pass; `answer-size` budget met |
| 3. Bodies | `hd_check` chapter by chapter in spec order: inference, rows, suspension, GADTs, exhaustiveness, templates, coherence, init order, TIR with its verifier and printer | type-phase cases pass with a known-failures list; std's bodies check clean; `errors-per-run`, `mistakes` and `diag-location` measured; `pathological` runs within budgets |
| 3b. Fix-its and `hd fix` | re-parse safety, `hd fix` rounds | `fixit-safety` at 100%; fix-it share of `mistakes` measured |
| 4. Cache and threads | disk store, manifest, eviction and `hd cache gc`, the pool scheduler, the opt-in memory cap, the embedded std pack, verify mode | `recheck-precision`, `edit-latency`, `cold-check`, `resources`, `startup`, `determinism` (full matrix), `incremental-soundness`, `cache-contention`, `cache-growth`, `parallel-speedup` with peak memory at 8 threads at most 1.5x that at 1 thread |
| 5. Browser front end | `hd_web` with the stepping scheduler and the JS stores | the playground checks programs in a worker; size measured so the owner can set a budget (answer 4); `long-session` flat in the browser |

D2's slices 6 to 10 follow in §22, refining [Q16](COMPILER_ARCHITECTURE_RESEARCH.md#build-order-1).

## 10. Open Questions For The Owner

Settled by the owner on 2026-10-07 and used above, not asked again: no
default memory cap, with an opt-in cap and a hard-limit diagnostic naming
the stage (§4.15); a 10 GB LRU shared cache with automatic eviction and
`hd cache gc` (§5.7); `hd build` on a library-only package writes only its
interface and cache entries (§7.5).

1. **Where compiled cache entries live.** `hd clean --cache` refuses a
   cache directory holding anything but `pkg`, `hash` and `tmp`
   ([`cli.clean.cache.layout`](../spec/cli/command-line.md#r-cli.clean.cache.layout)).
   **Recommendation:** put entries in `$HD_CACHE/obj/`, add `obj` to the
   rule's list, and let `hd clean --cache` remove it too, since it holds
   only derived data. The alternative, a per-package `build/cache`,
   loses sharing across worktrees, which pillar 2 depends on.
2. **Default thread count.** Many agents run `hd` at once on one machine,
   and too many threads slowed the prototype's suite about 9x.
   **Recommendation:** `min(cores, 8)` by default, `--jobs N` and
   `HD_JOBS` to change it, and `--jobs 1` as the serial mode. The
   `parallel-speedup` metric already stops at 8 cores.
3. **The `cache-contention` target.** The proposed metric says "each entry
   computed once" across N processes. The no-daemon cache gives "no
   corruption" for free, but "computed once" needs lock files.
   **Recommendation:** change the target to "no corruption, and N
   concurrent identical checks cost at most 1.5x the CPU of one". Add lock
   files later only for D2's expensive entries if a measurement asks.
4. **Templates that call private helpers (mine).** The spec does not say
   whether a template body may name private items of its trait's module.
   If it may, a dependent's check of the instantiated template needs their
   signatures. **Recommendation:** allow it. The interface carries those
   items as hidden items, which user code cannot name and which the
   interface validator accepts only when a template names them.
5. **Diagnostic codes for limits.** Answer 5 left the names to the
   orchestrator. This design proposes `file-too-large`, `nesting-too-deep`,
   `item-too-complex`, `type-too-large`, `match-too-complex` and
   `memory-limit` (§4.15). No owner question unless the owner wants fewer
   codes, for example one `limit-exceeded` with the limit named in the
   message. **Recommendation:** separate codes, as the Day 1 list asks.
6. **`hd fmt` on a file with a syntax error.** **Recommendation:** leave
   the file untouched and report its first syntax error, as gofmt does.
   Formatting around error nodes, as Biome does, risks rewriting code the
   parser misunderstood.

### 10.1 Inconsistencies Found In The Inputs

1. **Layout.** The research says layout needs no parser feedback; the spec
   says layout and parsing cooperate at suite colons and for `SUITE_END`.
   Resolved by the parser-driven layout cursor (§4.2).
2. **Pack bodies.** [`module.interface.contents`](../spec/lang/10-modules.md#r-module.interface.contents)
   lists "the bodies of pack code", but packs were removed
   ([chapter 12](../spec/lang/12-variadic-generics.md)).
3. **Dictionaries.** [`module.interface.dictionaries`](../spec/lang/10-modules.md#r-module.interface.dictionaries)
   still says generic functions compile once with dictionaries, against
   answer 8 (code per concrete type).
4. **Local impl heads.** The same table lists local impl heads "needed for
   coherence". A local impl must involve a local type or trait, so no
   other module can overlap it. The research's Q4b noted this too.
5. **Fact values.** [`module.interface.fact-values`](../spec/lang/10-modules.md#r-module.interface.fact-values)
   puts fact values in the package interface; the check interface here
   keeps expressions and their types, and D2 computes values (the
   research's Q4b contradiction 2).
6. **Cache directory.** The CLI spec's clean rule allows only `pkg`,
   `hash` and `tmp` under `HD_CACHE` (question 1).
7. **Drive summaries.** The research's Q3 step 5 and Q7 slices 3 and 4
   still mention drive summaries and their hash, which answer 13 removed.
8. **Initialization order** needs bodies across the modules of an
   initialization group, a cross-module body fact not listed in the
   research. Handled inside the folder by init summaries (§4.13.10).
9. **"Query engine."** The Day 1 list says "incremental checking on a
   query engine", while the decision is a per-module cache with no salsa.
   The wording should follow the decision.
10. **CLI surface.** `hd fmt`, `hd fix`, `hd test --affected`,
    `--max-errors`, `--jobs`, `modules_checked` and the JSON `fixes` field
    are triaged or answered but not yet in the CLI spec. The spec pass
    adds them.

## Part D2: The Back Half

D2 designs monomorphization, suspension lowering, Wasm GC emission, link,
the runtime, the host interface and the test runner. It consumes these
interfaces from D1 and does not reach around them:

| Interface | From | What D2 gets |
| --- | --- | --- |
| TIR per body (§4.13.11) | `hd_check` through `hd_tir` | the one typed IR: desugared, generic bodies with explicit coercions, dispatch, decision trees, cleanup scopes, suspension and hook points; read from the `tir` entry |
| `ModuleResult` | `hd_check` | init summary, fact records, diagnostics; whether the module has errors |
| folder interfaces (§4.10, §4.11) | `hd_resolve`, `hd_iface` | signatures, impl tables, templates and hidden items, default and fact expressions, per-item hashes for instance keys |
| types and rows | `hd_types` | the InternPool (§3.9.2), stable paths for every `DefId`, and `StableHash` |
| init order | `InitOrder` | statement order per initialization group |
| test plan (§7.3) | `hd_driver` | test programs and their statically registered cases |
| cache | `hd_cache` | `CacheStore`, the key rules of §5.3, entry framing, verify mode, eviction. D2 adds the kinds `code`, `link` and `cwasm` (§11.2) |
| scheduler | `hd_sched` | `TaskKind::Ext` tasks with dependencies on D1 tasks, fuel, cancellation, the opt-in memory cap, content-ordered output |
| diagnostics | `hd_diag` | the `Diagnostic` record for D2's own errors, such as instantiation depth |
| determinism rules | §6.5 | content order, local counters, no IDs in output or keys; D2's entries join the determinism matrix |

### Changes To D1

The owner reviewed D1 while D2 was written and asked three questions:
how abstract the checked IR is, whether the design is truly
data-oriented, and whether all these IRs are needed. D2 answers them by
editing D1 in place:

1. **§3.9 Data-Oriented Encoding (new).** The InternPool for types and
   constants; one dense tag + data + `extra` encoding; struct-of-arrays
   tables with hash maps only as indexes; the scratch buffer and rollback
   by truncation; one schema per IR generating typed views, builders,
   verifiers and printers; byte budgets; what it buys. Credits Zig,
   Carbon, Yuku and Vx.
2. **§3.10 IR Abstraction Contracts (new).** For each IR: what is
   resolved in it and what is deliberately not, its lifetime and bytes per
   source line, peak memory for a check and a build, its verifier, and a
   table mapping each prototype failure in the audit to the rule that
   prevents it.
3. **One typed IR.** THIR and the planned MIR are merged into **TIR**,
   one typed IR per body that the checker emits directly (§4.13.11, which
   now holds its full instruction catalog, desugaring table, builder API
   and invariants). No instance is ever materialized as IR: emission walks
   the generic TIR under a substitution (§13.8). The chain is tokens,
   syntax tree, interface, TIR, Wasm.
4. **§3.3, §3.4, §3.5.** Types, rows and constants live in the InternPool;
   `TyKind` is a decoded view; body arenas are reused column sets.
5. **§4.1, §4.4.** Token classes and keyword hashing after Yuku; the
   syntax tree's named-field views at its boundary, its wire format, and
   recovery by truncation.
6. **§4.6** is now the item index: an index over the syntax tree and the
   interface, not a copy.
7. **§4.10, §4.12.1.** The interface is its blob read in place; impl
   tables are sorted columns with a hash index.
8. **§1.1, §1.2, §2.1, §3.1, §4.13, §5.1, §5.2, §9.** Renamed to TIR; the
   crate `hd_tir` added; the `tir` cache entry added; D2's rows filled in.
9. **§4.15.** The instantiation depth row is filled in (§13.4).
10. **§7.3.** The test plan lists each program's statically registered
    test cases, since registration names are string literals
    ([`module.testing.reg.name`](../spec/lang/10-modules.md#r-module.testing.reg.name)).
    Only `it_each` row counts are learned at run time (§19.2).
11. **§7.3 and §7.5** end with a pointer to §20, which finishes them.

## 11. Back-Half Overview

### 11.1 From TIR To A Running Program

```text
 ModuleFinish(m) ═══════════════════════════════════► [tir entry]    per module (D1)
                                  │ generic TIR, per-item TIR hashes, dependency lists
 program root(s) ─────────────────┤  (entry, test registrations, REPL input)
                                  ▼
                       Collect(P): reachability over TIR + impl tables
                                  │ instance set {(item, type args)}, content-ordered
                                  ▼
                       Emit(inst): walk the generic TIR under the substitution,
                                   choose layouts, write Wasm ═══════════► [code entry]   per instance, parallel
                                  │ body bytes + symbolic relocations
                                  ▼
                       Link(P): fold identical bodies, assign indices,
                                build types/globals/data, patch relocations ═► [link entry] = Wasm bytes
                                  │
            ┌─────────────────────┴──────────────────────────┐
            ▼ native (hd_run_wasmtime)                        ▼ browser (hd_web + generated JS glue)
   Precompile(P, tier) ═► [cwasm entry]             program worker: WebAssembly.compile
   InstancePre + pooling allocator                  imports from the JS host
   Store per run / per test case                    poll/wake loop on the worker's event loop
   poll/wake driver + host reactor                  JSPI or sync XHR for block_on
```

`═══►` marks a cache boundary, as in D1. Every boundary goes through
`CacheStore`, and every key follows §5.3's hygiene: no paths, no IDs, no
thread counts.

### 11.2 Stages

This continues D1's table (§1.2).

| Stage | Input | Output | Unit | Parallel | Cached | Key |
| --- | --- | --- | --- | --- | --- | --- |
| Collect | program roots, TIR of reachable modules, impl tables | the instance set, the import set, the type set | program | serial per program; programs in parallel | inside `link` | none |
| Emit | one instance: generic TIR, its substitution, the TIR of callees it inlines | Wasm body bytes, relocations, site and line records | instance | yes | `code` | §13.8 |
| Link | code entries of the instance set | one Wasm module with its custom sections | program | serial per program; fold hashing in parallel | `link` | §13.10 |
| Precompile | Wasm bytes, engine config | wasmtime's serialized module | program and tier | Cranelift compiles functions in parallel | `cwasm` | `H("cwasm", hash of the Wasm, wasmtime version, config hash, target)` |
| Instantiate and run | precompiled module, host | outcome, output | run or test case | test cases in parallel | no | none |

There is no MIR stage: the checker's TIR is the input of `Collect` and
`Emit` (§3.9.1).

### 11.3 Tasks

D2's tasks are `TaskKind::Ext` tasks (§6.1):

```rust
pub enum ExtTask {
    Collect(ProgramId),          // after the tir entry of every module the program can reach
    Emit(InstanceSlot),          // created by Collect, one per code-entry miss
    Link(ProgramId),             // after every Emit of its program
    Precompile(ProgramId, Tier), // native only
    RunCase(ProgramId, CaseIdx), // test runner (§19); the browser runs cases in its worker instead
}
```

- `Collect` starts only after the `tir` entries of every module that the
  program's roots reach in the module use graph exist. That set comes
  from the manifest's use lists, so it is known before any check runs.
- **The program fast key (mine).** Before `Collect`, D2 computes
  `prog_key = H("prog", toolchain_key, tier, profile, root description,
  sorted [(module path, tir key)] of the modules reachable in the use
  graph)`. A hit names the `link` entry directly. Collection, emission and
  linking are skipped, and a warm `hd run` or `hd test` reads one entry
  per program. Module-level reachability is coarser than item-level, so a
  hit is always sound. An edit to a reachable module that changes no
  instance still misses, and then rebuilds from code-entry hits.
- `Emit` tasks are ordered by TIR size, largest first, as body tasks are
  (§6.3).

### 11.4 Crates

This refines the research's back-half crates
([Q16](COMPILER_ARCHITECTURE_RESEARCH.md#crates)) and D1's §2.1. The
research's `hd_mir` and `hd_opt` are gone: TIR lives in `hd_tir` (§2.1),
and emission-time analyses live in `hd_mono`.

| Crate | Holds | Depends on | Browser |
| --- | --- | --- | --- |
| `hd_mono` | collection, substitution, instance keys, layouts, the depth limit, emission-time analyses (liveness, capture sharing, inlining decisions) | `hd_tir` | yes |
| `hd_host_abi` | the host ABI description: traits, methods, codecs, wait flags; generators for hd-side stubs, wasmtime stubs and the JS glue | `hd_base` | yes |
| `hd_wasm` | emission with `wasm-encoder`, the code entry format, folding, the linker, name and site sections | `hd_mono`, `hd_host_abi` | yes |
| `hd_run` | the embedding API: `Engine`, `Host`, `Provider`, grants, the poll/wake driver logic, panic decoding, the test runner core | `hd_wasm` | yes, without engines |
| `hd_run_wasmtime` | the wasmtime engine, providers of the default profile, the reactor, epochs, limits, the `cwasm` cache | `hd_run`, `wasmtime`, `tokio` (current-thread), `cap-std` | no |
| `hd_web` (extended) | runs D2 in the compiler worker; hands bytes and metadata to the program worker | `hd_run` | only there |

The JS glue is a generated file, not a crate. `cargo xtask codegen`
writes it from `hd_host_abi`, and CI fails when regenerating changes it,
as D1's generated code does (§2.2).

## 12. Emitting From TIR

### 12.1 One Walk Per Instance

`Emit(inst)` reads the instance's TIR from its module's mapped `tir`
entry and walks the root block's list in order, with a stack of open
blocks. For each instruction it:

1. substitutes the instance's type arguments into the instruction's type
   (memoized per instance: each distinct generic type is substituted and
   laid out once);
2. picks the layout of the result (§15.1) and assigns Wasm locals;
3. writes Wasm with `wasm-encoder`, recording relocations (§13.8);
4. for a call, records the callee instance for collection's check
   (§13.2) and, when the callee is inlined, walks the callee's TIR in
   place with a remapping of its operands (§12.5).

Nothing is written back to TIR, and no instance IR exists. Per-instance
state is the substitution memo, the value-to-local map, the open-block
stack and the output buffer, all in the worker's arena, reset per
instance.

### 12.2 Lowering Rules

| TIR | Wasm |
| --- | --- |
| a value instruction | a Wasm local (or several, for a `multi` layout, §15.1) when used more than once or across a block; otherwise left on the operand stack |
| `LocalGet`, `LocalSet` | `local.get`, `local.set`; a `Shared` captured local is a cell, a one-field struct |
| `Block`, `If`, `Loop` | `block`, `if`, `loop`; `Break` and `Continue` become `br` |
| `Scope` with `Defer` | the exit ladder below |
| `Match` and its switches | `br_table` on a tag or a dense range; a binary search of `if`s for a sparse range of more than 8 cases; length then bytes for strings; each arm once, in nested blocks that leaves branch to |
| `Call` with an `Item` callee | `call`, relocated to the callee instance |
| `Call` with a `TraitMethod` callee | the impl is selected per instance by the solver (§13.2); a direct `call` |
| `Call` with an `Evidence` callee | a `call_ref` through the vtable stored in the variant (§13.5) |
| `CallDyn` | `struct.get` of the vtable slot, then `call_ref` |
| `CallValue` | `struct.get` of the closure's code, then `call_ref` with the closure as the first argument |
| `CallHost` | an import call with the exchange-buffer codecs (§17.2) |
| `Closure` | a struct of the closure's environment: `Copy` and `Move` captures as fields, `Shared` ones as their cells; a closure with no capture is a constant global |
| `Coerce` | option wrap: a tag set or nothing (§15.2); to a trait value: a pair with a constant vtable; readonly view and row subsumption: nothing |
| `Interp` | lengths summed first, one string allocated, parts copied; each `Display` part writes into the builder through its resolved callee |
| `Default` | a call of the default's getter (§12.3) |
| `ForRange`, `ForList`, `ForMap` | counted loops (§12.4) |
| `Await*` | the state machine (§14) |
| `Hook` | nothing, except in hook builds (§14.7) |

**The exit ladder.** Each `Scope` with a `Defer` gets one:

1. Every suite gets a "registered" flag local, set at its `Defer`.
   Suites registered unconditionally at the scope's top need no flag.
2. Every exit that leaves the scope (fall-through, `Break`, `Continue`,
   `Return`, a failing `?` leaf) stores its value and an exit code in
   locals, then branches to one cleanup block at the scope's end
   ([`flow.defer.result-saved`](../spec/lang/06-control-flow.md#r-flow.defer.result-saved)).
3. The cleanup block runs the flagged suites, last in, first out, then
   dispatches on the exit code with a `br_table` to the real target.
4. A scope with one exit runs its suites there directly (the common case).

Panics skip the ladder: a panic is a trap and runs no suite
([`flow.defer.panic`](../spec/lang/06-control-flow.md#r-flow.defer.panic)).
Cancellation runs the same suites of the same scopes through the frame's
cancel function (§14.6).

### 12.3 Facts, Defaults, Derives And Tests

- **Facts and defaults (mine).** A fact or default body whose TIR is a
  tree of constants and constructors is evaluated at emission into a
  constant global. Any other becomes a getter that computes the value on
  first use and stores it in a global. Facts are requirement-free, so the
  getter needs no providers.
- **Derives.** Derive instances, including the generated `walk`,
  `describe` and `build`, are ordinary bodies (§4.13.9). With the
  walker's type known at each instance, every `w.member(h, value)` call is
  direct, so a derived `encode` is straight-line code
  ([`annot.walk.members`](../spec/lang/14-annotations.md#r-annot.walk.members)).
  Member handles are constant globals (§15.4).
- **Test registrations.** A `TestCase` body evaluates its registration
  call's run-time arguments (`rows`, `timeout`, examples) and calls the
  std registration function, which drives the test body (§19.3).

### 12.4 Rows And Providers

Requirement rows never become type arguments
([`req.poly.one-body`](../spec/lang/11-requirements-and-suspension.md#r-req.poly.one-body)).

- **A concrete row** (`$ Console + Clock`) adds one parameter per key,
  in the order of the keys' stable paths. Each is a trait value of the
  key's trait (§15.2). A call passes the providers TIR names for it, so a
  provider costs one Wasm argument per key and no allocation.
- **A row parameter** (`$R`) adds one **context** parameter (mine). A
  context is an immutable linked list of `(key id, provider)` nodes. Key
  ids are numbered at link time in stable-path order. Looking up a key
  walks the list and takes the first match, which is how an inner
  `$.with` shadows an outer one.
- **Extension** (`$.with(Logger=...)` around a call that needs `R +
  Logger`) pushes one node: one allocation per call into row-polymorphic
  code.
- **A function value with a row** takes a context, because its caller may
  not know its row's keys. TIR's `ContextFor` builds it once per calling
  body.
- **Cold suspensions** capture their providers in the frame at
  construction ([`req.bind.construction`](../spec/lang/11-requirements-and-suspension.md#r-req.bind.construction)).

### 12.5 Counted Loops And Checks

`ForRange`, `ForList` and `ForMap` are emitted as counters in both tiers
(mine). A loop that allocates per step fails the `allocations` target, so
this is a lowering rule, not an optimization:

```text
ForRange a..b       i = a; end = b
                    loop: if i >= end: break; body; i = i + 1     # cannot overflow here

ForRange a..=b      i = a; end = b
                    if i <= end: loop: body; if i == end: break; i = i + 1

ForRange a..        i = a
                    loop: body; i = i + 1   # checked in debug and test, wraps in release
```

The `a..` step is a checked add with the loop's site
([`flow.for.range.from-overflow`](../spec/lang/06-control-flow.md#r-flow.for.range.from-overflow)).
`continue` jumps to the increment. No range value, no iterator and no
`Option` exist at run time. `ForList` and `ForMap` loop over the backing
arrays' indices, capture the length at the start and compare it before
each step, panicking with `iterator-invalidated`
([`flow.for.invalidate.panic`](../spec/lang/06-control-flow.md#r-flow.for.invalidate.panic)).

**Checks.** TIR holds no check sequences; emission adds them by profile.

| Check | Debug and test | Release |
| --- | --- | --- |
| integer overflow (`+`, `-`, `*`, unary `-`, narrowing conversions) | panics `integer-overflow` | wraps |
| `INT_MIN / -1`, `INT_MIN % -1` | panics `integer-overflow` | wraps; Wasm's `div_s` would trap, so release needs a test too |
| division by zero | Wasm traps; the trap maps to `integer-division-by-zero` | same |
| shift by the width or more | panics `invalid-shift` | same; Wasm masks the count |
| list index | std's index method compares with the length, `index-out-of-bounds` | same; the backing array is longer than the list |
| string byte index | the engine's array bounds check; the trap maps by code offset | same |
| use of a closed handle | panics with the use site | not emitted; the host provider still refuses a closed handle |

The overflow sequences are those of the research
([Debug-Tier Checks](COMPILER_ARCHITECTURE_RESEARCH.md#debug-tier-checks)).
`i64` multiplication has no wide multiply in core Wasm, so its check
divides back, which costs about 20 cycles. The `release-check-cost` metric
watches it.

### 12.6 Tiers And Optimizations

The triage put the optimizing tier after the first release
([Pillar 3 features](NEW_COMPILER_ARCHITECTURE.md#pillar-3-features-artifact-quality)).
D2 keeps one emission for both tiers (mine): `release-check-cost` asks
that the debug build cost at most 1.3x the release build, and any
optimization only one tier has counts against that ratio. The tiers
differ only where the spec or a first-release feature says so.

| Rule | Debug | Release | Status |
| --- | --- | --- | --- |
| counted loops (§12.5) | yes | yes | first release |
| `multi` layouts: `Option`, `Result`, tuples and trait values as several Wasm values (§15.1) | yes | yes | first release |
| capture-free closures as constants | yes | yes | first release |
| constant folding and dead branches during the walk | yes | yes | first release |
| trivial inlining: the walk descends into a callee of at most 8 instructions with no loop, no suspension point and no closure | yes | yes | first release (mine) |
| bounded inlining: callees up to a size budget, and closures passed to a known callee, such as iterator adapters | yes | yes | open question 1 |
| scalar replacement: a non-escaping closure, cell or small data value after inlining becomes locals; an escape analysis over the instance's walk, recorded in the emission state | yes | yes | open question 1 |
| overflow checks | checked | wrap | spec |
| hook points (§14.7) | dropped | dropped | emitted only by hook builds (Later) |
| debug-only checks: closed handles, deadlock reports with frame lists (§14.8) | yes | no | first release |

Inlining walks the callee's generic TIR, mapped from its module's entry,
under the composed substitution. The inlined items' TIR hashes join the
instance's code key (§13.8). Escape facts and inlining decisions are
emission state, never written into TIR.

### 12.7 Emission-Time Checks

D1's TIR verifier (§4.13.11) holds before emission starts. During the
walk, emission asserts in the compiler's debug builds and in CI:

1. no `Param` type remains after substitution;
2. every `TraitMethod` callee resolves to exactly one impl at the
   instance's types;
3. every suspension point gets a resume case and a cancel case (§14.2);
4. every relocation names a symbol in the program's instance, type, import
   or global sets (§13.10).

A failure is an internal error naming the instance, with exit status 101,
as a task panic is (§6.4). The linked module is then validated by
`wasmparser` (§15.7).

## 13. Monomorphization And Merging

Owner's answer 8: code per concrete type, then merge byte-identical
functions, the same in debug and release. Dictionaries exist only for
trait values and GADT evidence.

### 13.1 Roots

| Program | Roots |
| --- | --- |
| executable or task (`hd run`, `hd build`) | the entry wrapper of `main` or `main!` (§16.2), and the init function of every group the entry module reaches |
| `hd build FILE` | the same, for FILE's module |
| unit test program (one module) | one `TestCase` body per registration, and the module's reachable init groups |
| integration test program (one file) | its `TestCase` bodies, and its reachable init groups |
| doc tests of a module | one `TestCase` body per doc test |
| REPL input | the input's top-level statements as an init body (§20.5) |

Only reachable items are compiled, so dead std code never reaches the
binary, and the import list holds only reachable host methods
([`cli.cap.total.needs`](../spec/cli/command-line.md#r-cli.cap.total.needs)).

### 13.2 Collection

```rust
pub struct Instance { pub item: StablePathRef, pub body: BodyRef, pub ty_args: CanonTys }
pub struct InstanceSet {
    pub instances: Vec<(InstanceKey, Instance)>,  // sorted by key bytes at the end
    pub vtables: Vec<(CanonTy, TraitRef)>,        // (concrete type, trait) pairs
    pub imports: BTreeSet<HostMethodId>,
    pub types: BTreeSet<CanonTy>,                 // every type whose layout the program uses
}
```

A worklist walk, as rustc's collector does:

1. Push the roots with no type arguments.
2. Pop an instance. Map its module's `tir` entry. Scan the body's `tags`
   column for calls, closures, coercions and host calls, substituting the
   instance's type arguments into each one's types.
3. For each `Item` callee, push the callee with the substituted
   arguments.
4. For each `TraitMethod` callee, solve the bound at the concrete self
   type with D1's solver (§4.12.2), pick the impl, and push the impl
   method with the impl's type arguments. Coherence guarantees one answer.
   The solver's memo is shared across programs of a run.
5. For each coercion to a trait value, and each evidence choice of a
   `NewVariant`, record the vtable `(type, trait)` and push every method
   of the trait at that type, supertraits included.
6. For each `Closure`, push the closure body with the parent's
   arguments.
7. Record every `CallHost` as an import, and every type whose layout
   an operation needs.
8. Repeat until the worklist is empty.

The order of the walk never reaches output: the result is sorted by
instance key before anything is emitted (§6.5). Collection is serial per
program and takes milliseconds; many programs (a test plan) collect in
parallel.

### 13.3 Instance Keys

```text
canon(T)      = structural encoding of a type over stable paths, with no IDs
                (Prim | Adt(path, args) | Tuple(elems) | Optional(T) | Fn(params, result, row keys, suspends) | ...)

instance_key  = H("inst", item stable path, sub-body index (closures), tir_hash(item),
                  [canon(arg) for each type argument])
```

The instance key names the instance. The code entry key (§13.8) adds what
the emitted bytes depend on.

### 13.4 The Instantiation Depth Limit

Polymorphic recursion (a generic call that instantiates itself at a
growing type) has no finite instance set.

- **Depth** is measured on type arguments: the nesting depth of the
  deepest type argument of an instance. Its default limit is **32**.
  A type nests that deep only through a recursive instantiation chain;
  hand-written types stay far below it.
- **Chain length** is the shortest chain of instance requests from a root.
  Its default limit is **256**.
- The walk is breadth-first in content order, so the first instance over
  either limit is the same on every run.
- The diagnostic `instantiation-too-deep` (proposed, in D1's §4.15 style)
  points at the generic call that grows the type, and shows the first
  three and the last instance of the chain, with types printed in full.
- This is a build error, reported by `hd build`, `hd run` and `hd test`.
  `hd check` does not report it, since it does not collect. Open
  question 2 asks whether that split is acceptable.

### 13.5 Dictionaries: Trait Values And GADT Evidence

- **Vtables.** A vtable is an immutable struct of typed function
  references, one per trait method, at one concrete type, plus references
  to supertrait vtables and a type id for `Any` and `Debug` of erased
  values. Each is a global with a constant initializer, built once per
  `(type, trait)` pair, so a coercion to a trait value never allocates a
  vtable.
- **Trait values** are pairs: the value as `anyref` and its vtable
  (§15.2). A call through a trait value is one `struct.get` and one
  `call_ref`.
- **GADT evidence.** A variant whose existential parameter has bounds
  stores one vtable per bound as hidden fields of the variant struct,
  filled at construction, where the concrete type is known
  ([`gadt.runtime.evidence`](../spec/lang/13-gadts.md#r-gadt.runtime.evidence)).
  The existential payload is erased to `anyref`. The matching arm's code
  calls through the stored vtables, so it is compiled once, not once per
  hidden type.
- **Everything else is static.** A call through a bound on a type
  parameter is a direct call in every instance.

### 13.6 Tuples, Arity And `all!`

There are no variadic generics (§4.13.7). Tuples are ordinary types:

- `Args < Tuple` instantiates like any type parameter, so `f(args...)`
  is one instance per tuple type.
- Compiler-derived tuple `Eq`, `Ord`, `Hash` and `Debug` are generated
  bodies per arity, instantiated on demand.
- `all!` is one intrinsic frame instance per tuple of child result types
  (§14.5). `race!` is an ordinary generic intrinsic over `T`.

### 13.7 Merging Byte-Identical Functions

**Where merging comes from.** Each instance is emitted with exact Wasm
types (§15.2). Two instances fold when their emitted bytes are equal after
relocation. Equal bytes arise from:

- type arguments with the same Wasm layout: `List[u32].push` and
  `List[i32].push`, `Option[char]` and `Option[u32]` helpers;
- data types with the same field layout. Wasm GC canonicalizes types
  structurally, and hd needs no nominal Wasm types (§15.3), so `data A:
  x: i32` and `data B: x: i32` are one Wasm type, and their instances
  fold;
- type parameters that the body does not use in any operation;
- identical small std helpers at different types.

`List[Point].push` and `List[User].push` fold only when `Point` and
`User` have the same field layout. The research expected them to fold
always; that needs erased element storage, which costs a cast per read
(§23.2, inconsistency 1).

**The algorithm (mine, after safe ICF in linkers).**

1. Start with classes keyed by `H(body bytes with type, global and data
   relocations resolved to canonical targets, and function relocations
   left symbolic)`.
2. Refine: two members stay in a class only if their function relocations
   point to the same classes. Repeat until no class splits. This handles
   mutual recursion and needs at most a few rounds.
3. The representative of a class is the member with the smallest instance
   key. The others become aliases.
4. The fold list (representative, then aliases in key order) goes into
   the `hd.folds` custom section, so a backtrace can say "also
   `List[Point].push`" (§15.5).

Every step works on content keys, so the result does not depend on
threads or order. Merging runs in `Link`, in both tiers.

### 13.8 Code Entries

`Emit(inst)` lowers one instance to Wasm with `wasm-encoder` and stores a
`code` entry:

```rust
pub struct CodeEntry {
    pub body: Box<[u8]>,                  // locals and instructions; index immediates as 5-byte padded LEBs
    pub relocs: Box<[Reloc]>,             // sorted by offset
    pub sites: Box<[SiteRecord]>,         // (offset, site kind, stable span) for traps and explicit panics
    pub lines: Box<[(u32, StableSpan)]>,  // offset -> statement span, for backtraces
    pub sig: CanonSig,                    // Wasm signature, as canonical types
}
pub enum Reloc {
    Func { at: u32, target: FuncTarget },     // InstanceKey | Import(HostMethodId) | Intrinsic helper
    Type { at: u32, ty: CanonWasmTy },        // a canonical Wasm type descriptor
    Global { at: u32, global: GlobalSym },    // module storage, constants, vtables, string literals
    Data { at: u32, bytes: Hash128 },         // a passive data segment, by content
    Site { at: u32, local: u32 },             // a site number, renumbered globally at link
}
```

```text
code_key = H("code", toolchain_key, tier, instance_key,
             sorted [(stable path, per-item interface hash)] of every item its TIR names,
             sorted [(stable path, tir_hash)] of every item inlined into it)
```

- The interface hashes carry layouts: a field added to a `data` type in
  another module changes that type's item hash, so every instance that
  touches the type re-emits. Function bodies of callees do not affect a
  caller's bytes, because calls are relocations.
- An edit to one private function re-emits its own instances, plus the
  instances that inlined it. Every other code entry is shared by `main`,
  the tests and other worktrees.

### 13.9 What Instances Cost

| Cost | Control |
| --- | --- |
| number of instances | reachability; folding; rows are not type arguments |
| emission time | one walk of the generic TIR per instance; cached by `code_key` |
| type section size | structural canonicalization and dedup at link (§15.3) |
| Cranelift time | wasmtime's per-function cache (§18.2) |

A per-item instance budget stays a fallback, as the research says. Add it
only if the `dead-code` metric fails after folding.

### 13.10 The Link Step

`Link(P)`:

1. **Drop and fold.** An instance that no relocation names, because every
   caller inlined it, is dropped. Then the rest fold (§13.7).
2. **Order functions.** Imports first, sorted by module and name. Then
   the generated runtime helpers, sorted by name. Then the representatives
   in instance-key order. Content order keeps indices stable across
   unrelated edits, which helps wasmtime's per-function cache.
3. **Types.** The canonical types the functions, globals and exports name
   (§15.3).
4. **Globals.** Constants (vtables, closures without captures, payloadless
   variant singletons, facts, member handles, short string literals),
   module storage, then the runtime's globals (§15.4).
5. **Data.** One passive segment per distinct string literal, by first
   reference in function order.
6. **Elements.** One declarative segment for every function used with
   `ref.func`. The reserved hot-reload table stays empty (§18.5).
7. **Exports**: `hd.init`, `hd.poll`, `hd.wake`, the exchange buffer, and test entries (§14.4, §15.4, §19.2).
8. **Code.** Copy each representative's body and patch its relocations in
   place. In a release build, re-encode the padded LEBs at minimal width
   (mine); the debug build keeps them, since size does not matter there.
9. **Custom sections**: `name`, `hd.runtime`, `hd.sites`, `hd.lines`,
   `hd.folds` (§15.5, §16.4).

```text
link_key = prog_key (§11.3), and on a prog_key miss
           H("link", toolchain_key, tier, profile, root description, sorted code keys)
```

A program is written under both keys, so the next warm run hits the
cheap one. Link is serial per program and linear in output size.

## 14. Suspension Lowering

The spec fixes the protocol: cold, one-shot, poll-based suspensions,
synchronous cancellation, and a waker-driven entry driver
([Compilation Strategy](../spec/lang/11-requirements-and-suspension.md#compilation-strategy)).
D2 uses state machines with lazily materialized frames, as the research
recommends, which
[`req.lowering.representation`](../spec/lang/11-requirements-and-suspension.md#r-req.lowering.representation)
allows.

### 14.1 Functions Per Suspending Body

For each instance of a suspending function `f!`:

| Function | Signature | Use |
| --- | --- | --- |
| `f$body` | `(frame: (ref null $F_f), args..., providers...) -> (T, (ref null $F_f))` | the body. A null frame starts at state 0 with arguments in locals. A non-null frame resumes from its saved state. A null second result means Ready with the first; a frame means Pending, and the first result is a zero value |
| `f$cold` | `(args..., providers...) -> (ref $F_f)` | the plain call `f(args)`: allocates a frame holding the arguments and providers in state 0 |
| `f$poll` | `(frame: (ref $Suspend_L), cx) -> (i32, T)` | the vtable's `poll`; casts the frame and calls `f$body` with it |
| `f$cancel` | `(frame: (ref $Suspend_L)) -> ()` | the vtable's `cancel` (§14.6) |

`$Suspend_L` is one base struct per result layout `L` (§15.2):

```text
$Suspend_L = (sub (struct (field $state (mut i32))      ;; resume point; 0 = not started
                          (field $flags (mut i32))      ;; ACTIVE, STARTED, DONE, CANCELLED bits
                          (field $driver (mut anyref))  ;; the driver that owns it, for competing-driver checks
                          (field $vt (ref $SuspendVT_L))))
$F_f       = (sub final $Suspend_L (struct ... saved locals ... (field $child (mut (ref null $F_g))) ...))
```

### 14.2 The State Machine

Emission builds the state machine from TIR's explicit suspension points;
TIR itself is not changed (§3.10.1):

1. **Liveness.** For each suspension point, a backward scan of the body
   finds the locals and values live across it, plus the locals that the
   `defer` suites of the enclosing `Scope`s read (the point's side record
   names those scopes). These become frame fields, one field per such
   value. The scan is layout-independent, so it runs once per body per
   run and is memoized.
2. **Blocks containing a suspension point** are flattened into a
   dispatch loop (mine): `loop $dispatch { block_n ... block_1 {
   br_table pc } ... case code ... }`. Each basic block of a flattened
   block becomes one case, and a transfer sets `pc` and branches to
   `$dispatch`.
3. **Blocks without a suspension point** stay structured inside their
   case. Most control flow (a non-suspending loop, an `if`, a match)
   keeps its plain Wasm shape.
4. **Resume points** are the cases that follow each suspension point. The
   prologue reads `frame.state` and sets `pc` to its case, after
   reloading the saved locals.

Code size is linear: one case per basic block of a flattened block, one
save sequence and one reload sequence per suspension point. The prototype
grew about N³ (F-552).

Wasm's structured control flow cannot jump into a nested block, which is
why blocks with a suspension point are flattened. The alternative,
re-entering the nesting with skip flags as Binaryen's Asyncify does,
costs code size on every path. The cost of flattening is a branch through `$dispatch` at
each control transfer inside a suspending loop, a few cycles.

### 14.3 Lazy Frame Materialization

The ready path allocates nothing (C#'s design, as the research says):

```text
;; g!(x) inside f!
call $g$body (ref.null $F_g) x ...       ;; -> (result, child frame)
if child frame is null:                   ;; Ready: continue with the result
    ...
else:                                     ;; Pending
    if f's frame is null: frame = struct.new $F_f (default fields)
    save the live locals into frame; frame.child = child frame; frame.state = k
    return (zero, frame)                  ;; f is Pending too
```

- A deep chain allocates one frame per level, on the first real wait
  only. Later waits reuse the frames.
- **A resumed `f$body` resumes its child directly**: the saved child's
  type is known statically, so `f$body` calls `g$body(child, zeros)`
  with no vtable call.
- **A stored suspension** (`s!()` on a `mut Suspend[T]`) is polled
  through the vtable: one `call_ref`.
- **A cold call** (`f(x)` without `!`) allocates its frame at once. That
  is the spec's semantics, not overhead
  ([`req.suspend.cold.captures`](../spec/lang/11-requirements-and-suspension.md#r-req.suspend.cold.captures)).
- **Runtime checks** sit in `f$poll`, not in `f$body`: polling an ACTIVE
  frame panics `suspension-reentrant-poll`, a second driver panics
  `suspension-competing-driver`, and polling a DONE or CANCELLED frame
  panics `suspension-invalid-state`
  ([Runtime Checks](../spec/lang/11-requirements-and-suspension.md#runtime-checks)).
  A bang call of a direct callee never needs them: its frame is fresh and
  has one driver by construction.

The ready-path cost of `g!(x)` is one direct call and one null test,
which fits the 100 ns `suspension-overhead` budget with room to spare.

### 14.4 Wakers And The Entry Driver

**Wakers (mine).** A waker is a small struct `(task, slot)`: the frame of
an `all!` or the root, and a child index. `wake` sets the slot's bit in
the task's wake mask, propagates the bit up to the root, and sets the
instance's "woken" global. Wakes coalesce in the mask
([`req.waker.coalesced`](../spec/lang/11-requirements-and-suspension.md#r-req.waker.coalesced)).

Host operations cannot hold GC references across both engines cheaply, so
the instance keeps a **wake table**: a growable array of wakers, indexed
by host handle. The host only ever names handles.

**The entry driver** is a pair of exports:

| Export | Does |
| --- | --- |
| `hd.poll() -> i32` | polls the root (the entry or the running test case); returns `-1` for Pending, or the exit status after `report()` |
| `hd.wake(n: i32)` | reads `n` completed handles from the exchange buffer and invokes their wakers |

```text
host loop:
    status = hd.poll()
    while status == -1:
        if no host operation is pending and no timer is set: deadlock (§14.8)
        wait in the host reactor for at least one completion
        write the completed handles to the exchange buffer; hd.wake(n)
        status = hd.poll()
```

One `hd.wake` call carries every completion of one reactor turn, so a
burst of completions costs one crossing. The driver returns to the host
whenever the root is Pending
([`req.entry.busy-poll`](../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll)).

### 14.5 `all!` And `race!`

`all!(a, b)` is an intrinsic with one frame type per tuple of child
result types:

- **Fields**: the child frames, one result field per child, a done mask
  and a wake mask.
- **First poll**: poll each child in argument order
  ([`req.schedule.all-unfinished`](../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished)).
  When every child completes on the first poll, `all!` returns the tuple
  from locals and allocates nothing of its own. Only the children's cold
  frames were allocated.
- **Later polls** visit the unfinished children in argument order. A
  child whose wake bit is clear is skipped (mine). Polling it would only
  return Pending with no effect, so skipping is not observable, and it
  makes a wake O(woken children) instead of O(children).
- **Cancellation** cancels the unfinished children in argument order.

`race!(tasks...)` takes a `List[mut Suspend[T]]`:

- It polls the children in list order. The first Ready wins.
- It then cancels every other unfinished child synchronously, in list
  order, before returning
  ([`req.combinator.race-losers`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers)).
- An empty list at run time panics `explicit-panic`.

`all_list!` is std hd over nested `all!` (std's task chapter), so it
needs nothing here.

### 14.6 Cancellation And `defer`

`f$cancel(frame)`:

1. If the frame is ACTIVE on the current poll stack, panic
   `suspension-reentrant-poll` with no state change.
2. If it is DONE or CANCELLED, return
   ([`req.check.cancel-idempotent`](../spec/lang/11-requirements-and-suspension.md#r-req.check.cancel-idempotent)).
3. Mark it CANCELLED.
4. Cancel the child, if any, through the child's vtable. A child that
   waits on a host operation calls the `hd:rt` abort import for its handle
   ([`req.cancel.external-abort`](../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
5. Run the `defer` suites registered at the frame's state, last in, first
   out ([`req.cancel.defer`](../spec/lang/11-requirements-and-suspension.md#r-req.cancel.defer)).
   `f$cancel` is one `br_table` on the state; each case runs that state's
   suites over the saved locals. Suites cannot suspend, so this is plain
   synchronous code.

A frame that was never polled has no registered suite, so cancelling it
only marks it. Raw abandonment of a started frame runs nothing, as the
spec says.

### 14.7 Hook Points

A `Hook` instruction sits in TIR at each suspension point, resume, frame
creation, completion, cancellation, and host call start and finish. Normal
emission drops it, so it costs nothing in debug or release code.

A trace, replay or simulation build (all Later) sets a flag in the tier
part of `code_key` and emits each hook as a call to an `hd:hook` import
with the site and the frame. Hooks live in the cached TIR, so turning them
on re-emits code entries and never re-checks.

Every nondeterministic input already crosses a host import (time, random,
I/O) or a wake. A simulation host can therefore replay an instance by
feeding recorded host results and wake orders
([`req.determinism.replay`](../spec/lang/11-requirements-and-suspension.md#r-req.determinism.replay)).

### 14.8 Deadlock Detection

A deadlock is a root that is Pending while no host operation is pending,
no timer is set and no wake is queued. Nothing can ever wake it.

- **Both tiers** detect it in the host loop for free (§14.4), and fail
  the run instead of hanging. The panic category is open question 3.
- **The debug tier** adds a report. Each frame records the site of its
  current `Await` in its state, so the host walks the frame tree from the
  root and prints each waiting frame's function and source line, as the
  first-release feature list asks.

A `block_on` whose argument waits on a suspension of the outer driver
hangs by the spec's note. That is a deadlock of the same shape inside
`block_on`, and the same check reports it.

### 14.9 `block_on`

`block_on(s)` is std hd over one runtime import:

```text
loop:
    poll s with a waker that sets a local flag
    if Ready: return the value
    if the flag is set: continue
    hd:rt/block()   ;; the host waits for >= 1 completion, writes the handles, returns n
    process the n wakes
```

- **wasmtime:** the import runs the reactor on the same thread until a
  completion arrives. The Wasm stack waits below the host call.
- **Browser:** §17.6.
- **The indirect ban (answer 13).** A global counter counts entered
  forbidden contexts: `defer` suites, default and fact getters, and
  module initialization other than the entry's. `block_on` and `println`
  read it and panic when it is not zero. D1 rejects the direct calls at
  check time (§4.13.5). The run-time category is open question 3.

## 15. Wasm GC Layout And Emission

### 15.1 Layout Classes

Each instance's types map to a layout. A layout is one or more Wasm value
types in locals, parameters and results, and one or more fields in a
struct or an array element.

| Class | Wasm values | hd types |
| --- | --- | --- |
| `i32` | `i32` | `bool`, `char`, integers of 32 bits or less, `usize`, payloadless enums |
| `i64` | `i64` | `i64`, `u64` |
| `f32`, `f64` | `f32`, `f64` | floats |
| `ref` | `(ref $T)` or `(ref null $T)` | data, enums with payloads, `string`, lists, maps, closures, frames |
| `multi` | 2 to 4 Wasm values | `Option` of a scalar, `Result`, tuples, trait values (§15.2) |
| `erased` | `anyref` | the payload of a trait value, `Any`, an existential payload |
| `void` | none | `void`, `()`, and `never` |

In locals, parameters and results, a `multi` layout over 4 Wasm values
is passed as one immutable struct instead (mine); Wasm's multi-value
results make 4 a cheap bound on both engines. In fields and arrays a
value layout stays unboxed, as the spec's shape rules ask
([Shapes and Generic Code](../spec/lang/04-type-system.md#shapes-and-generic-code)):
it becomes several fields, or several arrays.

### 15.2 Values

| hd value | In locals and results | In a field or array element |
| --- | --- | --- |
| integers, `bool`, `char`, floats | the scalar | packed: `i8` for `bool`, `u8`, `i8`; `i16` for 16-bit integers; else the scalar |
| `data T` | `(ref $T)`: a struct with one mutable field per declared field, in declaration order | same |
| embedded part | a separate struct, referenced by an immutable field of the outer struct ([`data.part.unobservable`](../spec/lang/08-data-and-enums.md#r-data.part.unobservable)) | same |
| payloadless enum | `i32` tag | `i8` or `i16` when the variant count fits |
| enum with payloads, **flat** | one struct: tag plus the union of payload fields, unused fields null or zero | same |
| enum with payloads, **subtypes** | a non-final base struct (tag, shared fields) and one final subtype per payload variant; payloadless variants are constant singletons | same |
| `T?`, `T` a non-nullable reference | `(ref null $T)`; null is `.None` | same |
| `T?`, `T` a scalar | `(i32 tag, T)` | two fields |
| `T??` and `Option` of a `multi` | a tag plus the inner layout | the same fields |
| `Result[T, E]` | `(i32 tag, T', E')` | the same fields, flattened |
| tuple | its elements' values | its elements' fields, flattened |
| trait value, `Any` | `(anyref, (ref $VT))` | two fields |
| closure | `(ref $Fn_sig)`: a base struct holding the code as a typed function reference; one subtype per capture shape | same |
| capture-free closure | a constant global of the base type, with no environment | same |
| `string` | `(ref $str)`, `$str = (array i8)`, immutable, valid UTF-8 | same |
| `List[T]` | `(ref $List_T)` = struct `{len: mut i32, data: mut (ref $Arr_T)}` | same |
| `Map[K, V]` | std hd over arrays (§16.1) | same |
| `mut Suspend[T]` | `(ref $Suspend_L)` (§14.1) | same |

**Enum layout per enum (mine).** The triage asks for a layout chosen per
enum. The rule is deterministic, with no annotation: an enum is **flat**
when its payload fields number at most 4 in total and no variant has an
existential parameter. Otherwise it uses **subtypes**. Flat enums need no
cast on a match, and their payloadless variants need no allocation.
Subtype enums cast once per matching arm, which the engine checks with
one load and compare.

**Erased scalars.** In an erased position, `bool`, `char` and integers of
16 bits or less become `i31ref`. Wider scalars are boxed in
`$Box_i32`, `$Box_i64`, `$Box_f32` or `$Box_f64`. This is where the owner's
"at least `i31ref`" lands: in monomorphized code no scalar is boxed at
all.

**Arrays.** `Array[T]` is the one compiler-known collection: `(array (mut
T'))` with `T'` the element layout of the table. An array of a `multi`
layout, such as `Array[Option[i32]]` or `Array[(i32, f64)]`, is a struct
of one array per Wasm value (mine): a structure of arrays, which keeps
every element unboxed, as the spec's packed `List[i32]` and list of
tuples ask, and allocates nothing per element. `Array[T]` is intrinsic,
so `List[T]` and `Map[K, V]` in std see one type either way.

### 15.3 The Type Section

1. **Canonical descriptors.** The linker maps each hd type to a canonical
   Wasm type descriptor: kind, fields with mutability and storage type,
   supertype, finality. Structurally equal descriptors are one type.
2. **Structural, not nominal (mine).** hd never tests an object's Wasm
   type to learn its hd type. Enum variants use the tag, and `Any` uses
   the vtable's type id. So two hd types with the same layout may share a
   Wasm type, which shrinks the type section and lets instances fold
   (§13.7).
3. **Rec groups.** The type graph's strongly connected components become
   rec groups; a non-recursive type is a group of one. Members of a group
   are ordered by their canonical descriptor with back references
   numbered, so equal recursive shapes give equal groups.
4. **Subtyping.** Declared only where hd needs it: enum variants under
   their base, frames under `$Suspend_L`, closure environments under
   `$Fn_sig`. Every leaf is `final`, which lets engines skip subtype
   checks.
5. **Order.** Groups in order of their canonical descriptor bytes. The
   type section is then a pure function of the type set.

### 15.4 Globals And Module Initialization

| Global | Wasm | Initialized by |
| --- | --- | --- |
| module storage (top-level bindings) | mutable, nullable or zero | the group's init function |
| vtables, capture-free closures, payloadless variant singletons, member handles | immutable | a constant expression (`struct.new` and `ref.func` are constant in Wasm GC) |
| constant facts, short string literals (16 bytes or less) | immutable | a constant expression (`array.new_fixed`) |
| other string literals, non-constant facts | mutable, nullable | lazily: the first use runs `array.new_data` or the getter |
| runtime state: panic category and site, the wake table, the forbidden-context counter | mutable | constants |

- **Init order.** The `hd.init` export calls each reachable group's init
  function once, in D1's order (`InitOrder` and M3), after every group it
  uses ([`module.init.group.once`](../spec/lang/10-modules.md#r-module.init.group.once)).
- **No Wasm `start` function** (mine). Init can panic and can call
  imports. Calling it as an export after instantiation lets the host
  refuse a start, attribute a trap to init, and run a test case's init
  inside the case's time limit.
- Initialization is synchronous: module top level is not a driver
  context.

### 15.5 Panic Sites And Backtraces

**Explicit panics.** A panic stub writes the category id and the global
site number into globals, copies the message into the exchange buffer,
sets its length, and executes `unreachable`. The host reads the globals
and the buffer after the trap, so it never calls into a poisoned instance
([`flow.panic.poison`](../spec/lang/06-control-flow.md#r-flow.panic.poison)).

**Engine traps.** A trap without a stored category is mapped by its trap
code and its code offset:

| Trap | Category |
| --- | --- |
| integer division by zero | `integer-division-by-zero` |
| array out of bounds (string byte access) | `index-out-of-bounds` |
| call stack exhausted (wasmtime's trap code; V8's `RangeError`) | `stack-exhausted` |
| epoch deadline (§17.8) | `time-limit` (proposed by answer 11) |
| GC heap growth refused (§17.8) | `heap-exhausted` (proposed by answer 11) |
| null dereference, failed cast, `unreachable` without a category | an internal error: a compiler bug, reported with the site and exit status 101 |

**Sections.**

| Section | Holds | Release |
| --- | --- | --- |
| `name` | function names: printed stable path plus type arguments, as `shop.cart/Cart.total` or `std.list/List.push[i32]` | yes, always |
| `hd.sites` | per site: category or kind, file index, line, column; plus a file table of package-relative paths | yes |
| `hd.lines` | per function: sorted code offsets with a delta-encoded line, for every statement that can call or trap | yes |
| `hd.folds` | folded aliases per representative (§13.7) | yes |
| `hd.runtime` | §16.4 | yes |

Spans become line and column at link time, from the source's line table.
The link key includes the source hashes through the code keys, so a
stale line cannot survive.

**Backtraces.** wasmtime's `WasmBacktrace` gives each frame's function
index and module offset. V8's `Error.stack` gives
`wasm-function[i]:0xOFF` frames. `hd_run` maps both through `hd.lines` and
`name` to `file:line function`, and prints the panic's own site first.
Release builds keep these sections, so release backtraces are symbolized,
as the first-release feature list asks.

**Browser devtools (mine).** The program worker builds a source map from
`hd.lines` when the page asks for it, and serves it through a
`sourceMappingURL` that names a blob URL. The binary carries no source
map, so it costs nothing in `hd build` output.

### 15.6 The 2 KB Tiny Program

There is no runtime blob. Everything in a module is reachable code
generated for that program, std included (§16.1). The tiny program
(`println("hello")`) needs:

| Part | Estimate |
| --- | --- |
| header, type section (`$str`, the import and export signatures) | 70 B |
| imports: `hd:Console` `write_line`, `hd:rt` stderr writer | 50 B |
| function, memory (the exchange buffer), global, export sections | 80 B |
| code: the entry wrapper, the copy loop to the exchange buffer, std's `println` and its panic on a closed pipe | 300 B |
| data: `hello` | 10 B |
| `name`, `hd.sites`, `hd.lines`, `hd.runtime` | 300 B |
| total | about 800 B |

What keeps it small:

- reachability and folding; no unused type; no lazy-literal code for short
  literals;
- compact LEBs in release (§13.10);
- the string copy loop is one shared helper per program;
- short export names (`hd.init`, `hd.poll`, `hd.wake`, `hd.x`).

**Start to first output (≤ 5 ms).** A warm `hd FILE.wasm` deserializes
the cached precompiled module (§18.2), instantiates it from an
`InstancePre`, and runs `hd.init` and `hd.poll`. Process start of `hd`
dominates: the `startup` target is 20 ms for `hd --version`, so a cold
process may miss 5 ms. How the metric counts it is inconsistency 7
(§23.2).

### 15.7 Emission

- **`wasm-encoder`** builds each body. A wrapper around its instruction
  sink records a relocation wherever it writes a function, type, global,
  data or site index, and writes that index as a 5-byte padded LEB.
- **Link** assembles the module with `wasm_encoder::Module`, writes code
  bodies with `CodeSection::raw` after patching, and appends the custom
  sections.
- **Validation.** `wasmparser` validates every linked module in the
  compiler's debug builds, in CI, and in verify mode. A release `hd`
  trusts its own output; the engine validates again anyway.
- **WAT** for the playground's view and `hd build --wat` (Later) comes
  from `wasmprinter`.

### 15.8 Deterministic Bytes

Reproducible builds are parked as a metric, but the cache needs
deterministic bytes anyway: equal keys must mean equal entries (verify
mode, §5.6). Every order in §13.10 and §15.3 is content-based: instance
keys, canonical descriptors, first reference in function order. No hash
map's iteration order reaches the output. Locals in a body are numbered
in TIR order; sites, closures and temporaries per body (§6.5). Wasm bytes
join the determinism matrix (§21.1).

## 16. Runtime

### 16.1 Where Each Piece Lives

The rule is the repository's: library code is hd in `lib/std`; the
compiler emits only what the spec names as intrinsic or what only it can
build.

| Piece | Where | Notes |
| --- | --- | --- |
| `List`, `Map`, `Set`, `StringBuilder`, string operations, formatting, `Debug`, `Display`, JSON, iterators, `retry!`, `all_list!` | hd in `lib/std` | over `Array[T]` and string intrinsics |
| `Choices`, shrink-free generators, structured assert diffs | hd in `lib/std` (`std.testing`) | the runner shrinks on the host side |
| `block_on`, the waker type, the wake table | hd in `lib/std` (`std.task`) | over one `hd:rt` import |
| default-profile provider types (`HostConsole`, `HostFs`, ...) | hd declarations in `lib/std` whose method bodies are intrinsic | an intrinsic body maps to a host import by the ABI table (§17.1); the signature is plain hd |
| frames, `all!` and `race!` frames, vtables, closure environments, entry wrappers, init sequencing, panic stubs, check sequences | generated | §12 to §15 |
| boundary encoders and decoders | generated per boundary type | like a derive, from the type's structure (§17.4) |
| `Array[T]` operations, string byte access, float bits, bit counts | intrinsics | a few Wasm instructions each |
| capability methods, `hd:rt` (block, abort, stderr), `TestRunner`, `PropertyRunner` | host | §17 |

### 16.2 Panics And Exit Codes

- A panic traps (§15.5). The host reports the category, the message, the
  location and the backtrace on standard error.
- `hd run` and `hd FILE` exit with the profile's panic status, which is
  never 101
  ([`module.profile.panic-status`](../spec/lang/10-modules.md#r-module.profile.panic-status)).
- An entry that returns `.Err(e)` exits with status 1. The entry wrapper
  renders `e` and its cause chain in Wasm and writes them through the
  `hd:rt` stderr import. That import is not a capability, so a program
  whose row lacks `Console` can still report its error.
- An internal error (a trap with no category) exits with status 101, as
  `hd`'s own failures do.
- A test case's panic fails the case, or passes it under a matching
  `expect_panic` (§19.4).

### 16.3 Allocation

- Every hd object is a Wasm GC struct or array. There is no allocator in
  linear memory.
- The exchange buffer (§17.3) is the only linear memory, and only modules
  that cross structured values have one.
- **Allocation counts (mine).** Each `struct.new` and `array.new` site
  carries a `Hook(Alloc)`. A stats build (`hd run --stats`, Later)
  emits it as a counter increment. That gives the `allocations` metric a
  count on wasmtime, where V8's heap statistics do not exist (§23.2,
  inconsistency 6).

### 16.4 Metadata And The Import List

**The import list** is the only source of a module's capability needs
([`cli.cap.total.needs`](../spec/cli/command-line.md#r-cli.cap.total.needs)).
Each capability trait's methods are imported from module `hd:<Key>`, with
`Key` as in the grant table (`hd:Console`, `hd:FsRead`, `hd:Http`).
Non-capability imports use `hd:rt`, `hd:TestRunner`, `hd:PropertyRunner`
and, in hook builds, `hd:hook`.

**`hd.runtime`** is a custom section, as the prototype's is:

```rust
pub struct RuntimeMeta {
    pub format: u16,
    pub compiler: Hash128,              // build id; not part of the module's semantics
    pub entry: EntryKind,               // Main | MainBang | Tests | ReplInput
    pub tests: Vec<TestMeta>,           // test programs only (§19.2)
    pub exchange: Option<ExportName>,   // the exchange buffer export, when present
}
pub struct TestMeta { pub export: u32, pub name: Box<str>, pub kind: TestKind, pub file: u32, pub line: u32 }
```

`hd FILE.wasm` treats a module without `hd.runtime`, without the entry
exports, or with an import outside the ABI table as not built by `hd`
([`cli.wasm.not-hd.detect`](../spec/cli/command-line.md#r-cli.wasm.not-hd.detect)).

## 17. Host Interface And Embedding

### 17.1 One ABI Description

`hd_host_abi` holds one table, written in Rust:

```rust
pub struct HostTrait { pub key: &'static str, pub std_path: &'static str, pub methods: &'static [HostMethod] }
pub struct HostMethod {
    pub name: &'static str,             // "read_text"
    pub params: &'static [Codec],       // Scalar(I32 | I64 | F64 ...) | Buffer(BoundaryTy)
    pub result: Codec,
    pub wait: Wait,                     // Never | May: start-and-poll
    pub resource: Option<ResourceArg>,  // which argument the grant checks: Path | Host | Addr | EnvName | Program | SysName
}
```

Four things are generated from it:

1. the hd-side bodies of std's provider methods (each intrinsic body
   becomes an import call with the encoders it needs);
2. wasmtime host stubs that decode arguments, check the grant, call the
   provider and encode results;
3. the JS glue's import object, codecs and grant checks (§17.10);
4. the list of known import names for `hd FILE.wasm`'s checks.

Std's capability traits are still declared in `lib/std` in hd. The table
mirrors them, and a test fails when the two disagree.

### 17.2 Import Shapes

| Method kind | Import | Example |
| --- | --- | --- |
| scalars only, never waits | one function with Wasm parameters and results | `hd:Clock` `now_ms() -> i64` |
| structured values, never waits | `(len: i32) -> i32`: arguments in the exchange buffer, result length back | `hd:Env` `get` |
| may wait | `name.start(len) -> i32`, then `name.finish(h) -> i32` | `hd:FsRead` `read_text.start`, `read_text.finish` |
| runtime | `hd:rt` `block() -> i32`, `abort(h)`, `stderr(len)` | |

A `.start` call returns `-1` when the operation finished at once, with the
result in the buffer, or a handle `h >= 0` when it is pending. The leaf
frame registers its waker in the wake table at `h` and returns Pending.
After `hd.wake(h)`, the next poll calls `.finish(h)`, which writes the
result into the buffer.

The prototype names imports `hd:<trait>/<method>`. D2 splits that into
Wasm's module and name fields, so the startup check reads the module field
alone.

### 17.3 The Exchange Buffer

- One exported linear memory, `hd.x`, of one page at first, grown on
  demand. A module that crosses only scalars has none.
- The caller writes encoded arguments at offset 0 and passes their length.
  The host writes the encoded result at offset 0 and returns its length.
- Calls do not nest: an instance runs on one thread, and no host call
  calls back into Wasm. `block` returns before any wake is processed, so
  one buffer is enough.
- **Strings and byte lists** cross as raw bytes, copied by a generated
  Wasm loop (Wasm GC has no bulk copy between arrays and memory). A
  string crosses with no conversion
  ([`types.string.host-bytes`](../spec/lang/04-type-system.md#r-types.string.host-bytes)).
  The host reads the bytes in one slice.

This replaces the prototype's call per byte (F-558). A `list_dir!` with a
few names is one start call, one copy loop each way, and one finish call.

### 17.4 Encoding Structured Values

A compact binary format (mine), with both sides generated from the same
type description, so it needs no field names:

| Value | Encoding |
| --- | --- |
| integers | LEB128; zigzag for signed |
| floats | raw IEEE 754 little-endian bytes |
| `bool` | one byte |
| `string`, `List[u8]` | length, then bytes |
| list | count, then elements |
| map | count, then key and value pairs in iteration order |
| data | fields in declaration order |
| enum | variant index, then payload fields |
| `Option` | 0, or 1 and the value |
| tuple | elements |

- Only boundary-safe types cross, as the spec defines them.
- **Cycles** ([`module.boundary.cycle`](../spec/lang/10-modules.md#r-module.boundary.cycle)):
  the encoder counts depth, and past depth 256 it keeps a stack of the
  references above it and fails with `boundary-cycle` on a repeat. Acyclic
  values below the depth pay nothing.
- Decoding a map calls the key type's `Eq` and `Hash` in Wasm, as the
  spec requires.

### 17.5 Host Calls On wasmtime

- Host methods are plain synchronous Rust functions (`Linker::func_wrap`).
  D2 does not turn on wasmtime's async support (mine): with start-and-poll,
  no Wasm call ever waits inside the engine, so no fiber stacks are
  needed, and each call stays a plain native call.
- **The reactor** is a tokio current-thread runtime per worker thread.
  A `.start` that cannot finish at once spawns a local task and returns
  its handle. The task's completion lands in the reactor's completion
  queue.
- **The driver loop** (§14.4) runs in `hd_run_wasmtime`: poll, then
  `block_on` the reactor for the next completion, then wake and poll
  again.
- **`hd:rt/block`** runs the same reactor loop nested inside a host call
  (§14.9).
- **Abort**: `hd:rt/abort(h)` aborts the task; HTTP requests and timers
  stop for real
  ([`req.cancel.external-abort`](../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
- **Path sandbox.** Granted directories are opened once as `cap-std`
  directory handles, and every file operation resolves relative to them.
  This removes the prototype's check-then-use race on symbolic links, as
  HOST_CAPABILITIES asks.

### 17.6 Host Calls In The Browser

- **Start and poll.** The JS host starts a Promise (fetch, a timer), keeps
  it in a handle table, and returns the handle. On settle it queues the
  handle. A queued handle schedules one task that calls `hd.wake(n)` and
  then `hd.poll()`, so wakes coalesce per task turn.
- **Cancellation** calls `AbortController.abort()` for fetches and
  `clearTimeout` for timers.
- **`block_on`** (answer 9):
  1. **JSPI present** (`WebAssembly.Suspending` exists): the glue wraps
     `hd:rt/block` as a suspending import, and wraps `hd.init`, `hd.poll`
     and the test exports with `WebAssembly.promising`. `block` then
     waits on a real Promise.
  2. **No JSPI, and the module imports `hd:rt/block`** (mine): the glue
     runs the whole program in synchronous mode. A same-origin HTTP
     request is a synchronous `XMLHttpRequest` that finishes inside
     `.start`, so it never pends. Every other operation that would pend
     inside `block` panics `host-contract` with a message that names
     JSPI. The import list decides the mode before the program starts,
     because reachability makes it exact.
  3. **No `hd:rt/block` import**: the normal asynchronous mode, on every
     browser with Wasm GC.

### 17.7 Capability Grants

- **Startup refusal.** Before instantiation, the host reads the import
  list, maps each `hd:<Key>` module to its trait, and refuses with status
  101 when a needed trait is totally denied, naming the setting
  ([Total Deny](../spec/cli/command-line.md#total-deny)). Nothing runs
  before the check, not even init.
- **Partial deny.** The generated host stub extracts the method's
  resource argument, checks it against the grant, and on a refusal
  encodes the method's `NotGranted` variant as the result. The program
  sees an ordinary `.Err`
  ([`cli.cap.partial.refuse`](../spec/cli/command-line.md#r-cli.cap.partial.refuse)).
  `Env.get` returns `.None` and the host prints the one-line notice once
  per name.
- Checks live in host providers only, never in hd code, as
  HOST_CAPABILITIES requires.

### 17.8 Resource Limits

| Limit | Mechanism | Failure |
| --- | --- | --- |
| time (`--time-limit`, a test's `timeout`) | epoch interruption: one ticker thread per process increments the engine epoch every millisecond; each store sets its deadline in ticks | the trap maps to `time-limit`; a pending host wait is cut by a reactor timer at the same deadline |
| GC heap and the exchange buffer (`--max-heap`) | a `ResourceLimiter` on the store; wasmtime grows its GC heap through the same limiter as linear memory (confirm in slice 6) | refused growth traps, mapped to `heap-exhausted` |
| stack | `Config::max_wasm_stack`, part of the runtime profile | `stack-exhausted` |
| instances, tables, memories | the pooling allocator's totals (§18.3) | an internal error naming the limit |

The flag names are open question 4; answer 11 settled the categories but
not the flags.

### 17.9 The Embedding API

The CLI is a host on this API (a Day 1 decision); the test runner and
later embedders are others.

```rust
pub trait Engine: Send + Sync {                       // wasmtime natively; the JS host in the browser
    type Module: Send + Sync;
    fn load(&self, wasm: &[u8], cache: &dyn CacheStore) -> Result<Self::Module, LoadError>;
    fn instantiate(&self, m: &Self::Module, host: &HostSetup) -> Result<Box<dyn Instance>, StartError>;
}
pub trait Instance {
    fn init(&mut self) -> Outcome;                    // runs hd.init
    fn poll(&mut self) -> Poll<Outcome>;              // runs hd.poll
    fn wake(&mut self, handles: &[u32]);
    fn call_test(&mut self, export: u32) -> Poll<Outcome>;
}
pub trait Provider: Send + Sync {
    fn key(&self) -> HostKey;
    fn call(&self, m: MethodId, args: &[u8], out: &mut Vec<u8>, cx: &mut CallCx) -> CallResult;  // Ready | Pending(OpId)
}
pub struct HostSetup {
    pub providers: ProviderSet,    // one per bound trait
    pub grants: Grants,            // from hd.toml and flags, or the test grant
    pub limits: Limits,
    pub reactor: ReactorHandle,
}
pub enum Outcome { Exit(u8), Panic(PanicReport), Internal(String) }
```

- `hd run` builds a `HostSetup` from the default profile's providers and
  the package's grant. `hd test` builds one per test kind (§19.3).
- A later embedder supplies its own providers for its own capability
  traits through the same `Provider` trait. Their ABI descriptions then
  come at run time instead of from the static table. That is the Later
  embedding feature, and the API leaves room for it.

### 17.10 The Generated JS Glue

One ES module, `hd-host.js`, generated from `hd_host_abi` and checked in:

- `makeImports(module, providers, grants)` returns the import object for
  exactly the module's imports;
- the codecs of §17.4, reading and writing the exchange buffer through a
  `DataView`, and strings through `TextDecoder` and `TextEncoder`;
- the poll and wake loop of §17.6, the JSPI detection, and synchronous
  mode;
- panic decoding: reads the panic globals and the buffer, maps
  `wasm-function[i]:0xOFF` frames through `hd.lines`;
- a provider interface; the playground supplies browser providers (§20.6).

It uses only web platform APIs, so it also runs under Node, which the
metric scripts can use to run a release `.wasm` on V8.

## 18. Execution Engines And Tiers

### 18.1 wasmtime Configuration

| Setting | Debug and test | Release |
| --- | --- | --- |
| `wasm_gc`, `wasm_function_references` | on | on |
| exceptions, threads, stack switching | off | off |
| compiler | Cranelift, `OptLevel::None` to start (§18.6) | Cranelift, `OptLevel::Speed` |
| collector | the default copying collector | same |
| `epoch_interruption` | on | on |
| `consume_fuel` | off (fuel is for simulation, Later) | off |
| `max_wasm_stack` | the profile's stack limit | same |
| incremental compilation cache | on, through `CacheStore` | on |
| allocation strategy | pooling (§18.3) | pooling for `hd test`; on-demand for one `hd run` |
| parallel compilation | on, capped at the `--jobs` budget | same |

Winch cannot be the debug tier: it supports no GC. Both tiers are
Cranelift, as the research found.

### 18.2 Compile Caches

- **Precompiled modules.** `Engine::precompile_module` output is stored
  as a `cwasm` entry. A disk entry is loaded with
  `Module::deserialize_file`, which maps the file instead of copying it.
  The entry's checksum (§5.4) and wasmtime's own compatibility check both
  guard it. The key holds the Wasm hash, the wasmtime version, the engine
  configuration and the target, so a config change misses.
- **Per-function cache.** `Config::enable_incremental_compilation` takes
  a cache store; an adapter maps it onto `CacheStore` as entry kind
  `cranelift`. A program whose other functions did not change recompiles
  only the changed ones. Function order in the link is content-based
  (§13.10), which keeps unchanged functions' indices stable; Cranelift's
  cache keys abstract callee names, so a shifted callee index does not
  miss either (to confirm in slice 6).

### 18.3 Instantiation

- **One `Linker` per host kind** (default profile, unit test, integration
  test), built once per process.
- **One `InstancePre` per module**: imports resolved once, so each
  instantiation skips name lookup.
- **The pooling allocator** reserves slots for instances, exchange
  buffers, tables and GC heaps up front. Its totals scale with the worker
  count. An instantiation then takes a slot instead of mapping memory, a
  few microseconds.
- **A `Store` per run or per test case**, with its own `ResourceLimiter`
  and epoch deadline. Dropping the store returns the slot.

### 18.4 The Browser Runner

- The compiler worker builds the module (§20.6) and sends the bytes, the
  `hd.runtime` section and the source map request to the program worker.
- The program worker compiles with `WebAssembly.compile`, instantiates
  with the generated glue's imports, and runs the poll and wake loop on
  its event loop. V8 starts every function in Liftoff, which suits short
  playground programs.
- **Stop** or a timeout terminates the program worker only.
- **Conformance in a headless browser** (§21.2): a page hosts the program
  worker, and a Node driver feeds it the portable runtime cases, as the
  playground's e2e test drives a page today.

### 18.5 The Reserved Hot-Reload Table

Hot reload is Later. The emitter reserves its hook now
([Q14](COMPILER_ARCHITECTURE_RESEARCH.md#q14-hot-reload-later-brief)):

- an emitter switch routes every user-to-user call through one `funcref`
  table, one slot per user function, in content order;
- the type section is already canonical and deterministic (§15.3), so
  the next build gives the same layouts the same Wasm types;
- with the switch on, module storage globals are exported.

The switch is off in the first release. Turning it on changes the tier
part of `code_key` and nothing else.

### 18.6 Measured Choices

Two engine settings are decided by slice 6's measurements, not here:

- the debug tier's Cranelift level: `None` compiles faster, `Speed` keeps
  `release-check-cost` safer. If debug code at `None` costs more than 1.3x
  release, debug moves to `Speed` and the compile time is recorded;
- a null collector for short test instances (the research's experiment):
  adopted only if it measurably cuts `unit-test-perf`.

## 19. Test Runner

### 19.1 Building

1. `hd check --tests` (§7.3), then the test plan: one program per module
   with unit tests, one per integration test file, one per module's doc
   tests.
2. The package's executables are built first
   ([`cli.test.builds-executables`](../spec/cli/command-line.md#r-cli.test.builds-executables)),
   in the test profile ([`cli.profile.test`](../spec/cli/command-line.md#r-cli.profile.test)).
3. Each program goes through `Collect`, `Emit`, `Link` and `Precompile`
   as tasks, in parallel across programs. Warm, each program is one
   `prog_key` hit and one `cwasm` hit.

### 19.2 Listing Cases

- Registration names are string literals, so the linker writes each
  program's cases into `hd.runtime` (§16.4): export index, name, kind
  (`it`, `it_each`, `it_prop`, doc test), file and line.
- `--filter` selects cases from that list before anything runs.
- **`it_each`** rows are evaluated at run time
  ([`std-testing.it-each.rows-at-run`](../spec/std/testing.md#r-std-testing.it-each.rows-at-run)).
  The first instance of an `it_each` case runs row 0; its `row(count)`
  call tells the runner the count. The runner then schedules rows 1 to
  `count - 1`, each in a fresh instance. A filter that names `name[i]`
  still runs row 0 first to learn the count, and reports only the rows it
  names.

### 19.3 Running

```text
work queue: (program, case) in content order: program path, then registration order, then row
workers (--jobs): pull a case, then
    store = Store::new(engine, limits)              ;; pooling slot
    inst  = instance_pre.instantiate(store)         ;; imports pre-resolved
    inst.init()                                     ;; hd.init: this program's init groups
    outcome = drive(inst.call_test(case))           ;; poll/wake loop; reactor only for integration tests
    drop(store)                                     ;; slot returned
results: slot per case; printed by the release cursor in content order (§6.5)
```

- **A fresh instance per case**
  ([`module.testing.instance`](../spec/lang/10-modules.md#r-module.testing.instance)),
  made cheap by `InstancePre` and the pooling allocator. Starting from a
  snapshot was dropped by the owner, so module initialization runs per
  case.
- **Host setups.** A unit test case gets `TestRunner` alone. An
  integration test or doc test gets the default profile, `TestRunner`, the
  test runner's `Process`, the test grant, the package directory as its
  working directory, no arguments and closed input
  ([Test Environments](../spec/cli/command-line.md#test-environments)).
- **`temp_dir()`** makes the case's directory on first call and the
  runner removes it when the case ends
  ([`cli.test.env.temp-dir`](../spec/cli/command-line.md#r-cli.test.env.temp-dir)).
  A unit test that never calls it costs nothing.

**Per-case overhead budget** (target: at most 1 ms per test, and 1,000
unit tests in at most 1 s warm):

| Step | Estimate |
| --- | --- |
| take a pooling slot and instantiate from `InstancePre` | 5 to 20 µs |
| `hd.init` of a small test program | 1 to 50 µs |
| call, poll, `TestRunner` calls | 1 to 5 µs |
| report into the result slot | under 1 µs |
| total per case | about 0.1 ms at most, before the test's own work |

Warm `hd test` on 1,000 unit tests in 100 modules: process start and the
warm check fast path (about 30 ms), 100 mapped `cwasm` loads (about 1 ms
each, in parallel), and 1,000 cases over 8 workers. That is well under a
second. `test-latency` (edit, then `hd test --filter one` in at most
300 ms) pays one module's check, the instances whose code keys changed,
one link, Cranelift for the changed functions, and one case.

### 19.4 Property Tests, Panics And Timeouts

**`it_prop`.** One property test is one test case, so all its generated
cases run in one instance (a first-release feature):

- The runner implements `PropertyRunner` on the host. `start` returns
  the case's `PropertyCase` through the exchange buffer; `record` is a
  scalar import, one per draw; `show` sends the input's `Debug` text.
- **A discard** is a panic before `show`
  ([`std-testing.runner.discard-read`](../spec/std/testing.md#r-std-testing.runner.discard-read)).
  The instance is poisoned, so the runner takes a fresh instance and
  continues with the next seed. A discard costs one instantiation.
- **A failure** poisons the instance too. Shrinking runs on the host,
  over the recorded draws. Each shrink attempt replays in an instance;
  passing attempts reuse it, and a failing attempt costs a fresh one.
- **Parallel cases (mine).** When workers are idle, one property's
  cases may be split across them, each worker with its own instance and
  its own range of case numbers. The reported failure is the failing case
  with the smallest number, and shrinking is serial, so the outcome does
  not depend on the split.
- **Regression file.** Saved streams replay first, and a new failure's
  shrunk stream is written to `__regressions__/`
  ([`std-testing.prop.regression-file`](../spec/std/testing.md#r-std-testing.prop.regression-file)).
- **Throughput.** A simple generator's case is a few draws (each a
  scalar import of tens of nanoseconds), one `Debug` render and one short
  string crossing, and the property body. That is a few microseconds per
  case, inside the 10 µs that 100k cases per second allow, on one worker.

**`expect_panic`.** The panic's category is read from the instance's
globals (§15.5). A matching category passes the case; completion or
another category fails it.

**`timeout`.** `report_timeout(millis)` sets the store's epoch deadline
and a reactor timer, so a busy loop traps and a pending host wait is cut
at the same moment. Either fails the case with `time-limit`. A case
without a `timeout` has no deadline unless `--time-limit` sets one.

### 19.5 Reports

- **Quiet passes.** Passing cases print nothing; a summary line ends the
  run. JSON lines list every case.
- **Each failure** prints its name, the panic category and message, the
  symbolized backtrace (§15.5), and a **repro command**:
  `hd test src/billing.hd --filter "sums prices"`. A property failure adds
  its seed and the regression file path; rerunning the repro replays the
  saved stream.
- **Structured assert diffs.** `assert_equal` in `std.testing` compares
  the two values through their derived structure and builds a message
  that lists only the differing paths (`.items[2].price: 3 != 4`). It is
  std hd, so the runner only prints the message.
- **Order.** Results are printed in content order by the release cursor,
  whatever order workers finish in.

### 19.6 Tests In The Browser

The playground runs a program's tests in its program worker with the same
Wasm. The JS glue implements `TestRunner` and `PropertyRunner`, and each
case gets a fresh `WebAssembly.Instance` of the compiled module.
Instantiation in a worker is synchronous for any module size.

## 20. Command Flows Completed

### 20.1 `hd test`

Continuing D1's §7.3 at step 4:

1. Build the executables in the test profile, then the test programs
   (§19.1).
2. Refuse a program whose import list needs a totally denied trait, with
   status 101 ([`cli.cap.total.test`](../spec/cli/command-line.md#r-cli.cap.total.test)).
3. List cases, apply `--filter` (§19.2). `hd test FILE` with no case, or
   a filter that matches nothing in FILE, is an error.
4. Run the cases (§19.3), stream failures in content order, print the
   summary, write `build/.hd/last-test` for `--affected`.
5. Exit 0 when every case passed, 1 when one failed, 101 when `hd` itself
   failed.

### 20.2 `hd run` And `hd FILE`

1. Check (D1 §7.5), build the program in the debug or release tier, and
   load its `cwasm` entry.
2. Read the import list; refuse a totally denied need with status 101.
3. Instantiate with the default profile's providers and the grant.
4. Call `hd.init`, then drive `hd.poll` and the reactor (§14.4).
5. Exit with the program's status: the entry's `ExitCode`, 1 for an
   `.Err`, or the profile's panic status.

### 20.3 `hd build`

- Writes the link entry's bytes to `build/debug/NAME.wasm` or
  `build/release/NAME.wasm`, per executable, and
  `build/{debug,release}/files/STEM.wasm` for `hd build FILE`. The file
  is a copy of a cache entry, so an unchanged program writes identical
  bytes.
- A library-only package writes no `.wasm` (D1 §7.5).
- No grant is written into the module
  ([`cli.cap.build`](../spec/cli/command-line.md#r-cli.cap.build)).

### 20.4 `hd app.wasm`

1. Validate the bytes; check `hd.runtime`, the entry exports, and that
   every import is in the ABI table. Otherwise the module was not built
   by `hd`: status 101 with the missing export or unknown import named.
2. The grant comes from `--cap` flags alone; refuse totally denied needs.
3. Precompile through the `cwasm` cache, keyed by the module's hash, then
   run as in §20.2.

### 20.5 The REPL

- The session is one `Store`. Each input is checked (D1 §7.8) and built
  as its own small Wasm module, which imports the earlier inputs' exported
  bindings and functions and exports its own. Wasm GC types canonicalize
  across modules, so values flow between inputs unchanged.
- Each input runs once: its init function evaluates its statements and
  prints an expression input's value with its type
  ([`cli.repl.value`](../spec/cli/command-line.md#r-cli.repl.value)).
  Host calls are never repeated
  ([`cli.repl.host.once`](../spec/cli/command-line.md#r-cli.repl.host.once)).
- A panic in an input is open question 6.
- Each input's compiler arenas are freed after it runs; only the pool and
  the session's instances grow, which `long-session` measures.

### 20.6 The Playground

Continuing D1's §7.9:

1. "Run" in the compiler worker: check, then `Collect`, `Emit` and `Link`
   in `hd_web` with the stepping scheduler. The `MemoryStore` holds code
   entries, so a second run after a small edit re-emits few instances.
2. The bytes and `hd.runtime` go to the program worker (§18.4).
3. Browser providers: `Console` to the page, `Clock` and `Random` from the
   platform, same-origin `Http` with `fetch` (or synchronous XHR in
   synchronous mode, §17.6), an in-memory file system, and the
   playground's grants (HOST_CAPABILITIES S7).
4. A panic shows the category, message and a backtrace mapped through
   `hd.lines`; the source map is built on demand (§15.5).

## 21. Back-Half Determinism And Soundness Tests

### 21.1 Byte-Identical Wasm

D1's determinism matrix (§8.1) adds every `code` and `link` entry and
every written `.wasm` to what must be byte-identical across thread counts,
serial orders, hasher seeds, ID shifts, checkout paths, cache states and
platforms. Reproducible builds are parked as a metric, but the cache
needs them internally: equal keys must give equal bytes.

### 21.2 Cross-Engine Conformance

The portable runtime cases run on wasmtime (in `hd`) and in a headless
browser (Chromium with and without JSPI, and Firefox), with **one**
`KNOWN_FAILURES.tsv`. A case that passes on one engine and fails on the
other is a bug, never a per-engine known failure; this is the Day 1
cross-backend rule.

### 21.3 Differential Tests Against The Prototype

The frozen prototype and the new compiler run the runtime cases and
generated programs; stdout, exit status and panic category are compared.
Each disagreement is triaged against the spec, as for checking (§8.3).
Program generators are portable tools under `test/`.

### 21.4 Codegen-Cache Soundness

- The edit-script fuzzer (§8.2) also builds: after each edit, an
  incremental `hd build` and a clean one must write identical bytes.
- **Key completeness tests**, one per dependency kind of `code_key`: a
  field added to a type in another module, a callee's signature changed,
  an inlined callee's body changed, an impl added that changes nothing
  (coherence forbids changing a selection), a tier switch. Each must miss
  exactly the instances that touch it.
- Verify mode (§5.6) recomputes `tir`, `code` and `link` entries on hits.
- `wasmparser` validates every linked module in CI.

## 22. Back-Half Build Order

D1's slices 1 to 5 come first. Slice 3's exit already includes TIR with
its verifier and printer.

| Slice | Delivers | Exit test | Pillar 3 metrics it should start meeting |
| --- | --- | --- | --- |
| 6. Scalars end to end | `hd_mono`, `hd_wasm` for scalars, functions, `println`; `hd_run` and `hd_run_wasmtime` with `hd run`, `hd build`, `hd FILE.wasm`; the `code`, `link`, `cwasm` entries | the scalar cases of `runtime/valid` pass; Wasm bytes deterministic across the matrix | `size-startup-heap` (tiny ≤ 2 KB); `hd` binary size and Cranelift time recorded; `release-check-cost` on scalar code |
| 7. Data and std | data, enums, closures, strings, lists, maps, `Option` and `Result` layouts, the exchange buffer for structured values, panics with sites and backtraces, folding | `runtime/valid` and the `runtime/panic` cases chapter by chapter, with a known-failures list | `runtime`, `allocations` (counted loops: 0), `dead-code`, `text-throughput` |
| 8. Suspension, host and tests | state machines, `all!`, `race!`, cancellation, the reactor, grants and startup refusal, resource limits, the test runner with property tests | the suspension and capability cases; the CLI cases of [`cli-cases.tsv`](../spec/conformance/cli-cases.tsv) | `suspension-overhead`, `host-call-overhead`, `unit-test-perf`, `proptest-perf`, `integration-test-perf`, `test-latency` |
| 9. The browser | the JS glue, the program worker, synchronous mode, the headless-browser adapter | the runtime cases pass on wasmtime and in the browser with one known-failures list | browser size recorded; `allocations` on V8 through the glue |
| 10. Optimization within the shared tier | bounded inlining and scalar replacement if question 1 says so; the measured engine choices (§18.6) | no conformance regression; Wasm still deterministic | `runtime` (≤ 1.5x Node), `allocations` (chains ≤ 1), `serde-throughput`, `long-run-memory` |

### 22.1 How The Pillar 3 Targets Are Met

| Metric | Target | How | Status |
| --- | --- | --- | --- |
| `runtime` | geomean ≤ 1.5x Node; no case > 3x | per-type code with direct calls; counted loops; unboxed layouts; std written for it (streaming JSON) | at risk: wasmtime's copying GC is slower than V8's on allocation-heavy cases, and general inlining is open question 1 |
| `allocations` | 0 per counted loop; ≤ 1 per chain | counted loops are a lowering rule (§12.5); chains need inlining and scalar replacement | loops met by design; chains at risk until question 1 |
| `size-startup-heap` | tiny ≤ 2 KB; ≤ 5 ms; heap ≤ 2x Node | reachability, folding, no runtime blob (§15.6); `InstancePre` and `cwasm` | size met by estimate (about 800 B); 5 ms at risk if it counts process start (§23.2) |
| `host-call-overhead` | scalar ≤ 1 µs; Fs ≤ 10 µs; serde boundary ≤ 5 µs | scalar imports; one exchange buffer and one copy loop; no async machinery in wasmtime calls | met by design; measured in slice 8 |
| `suspension-overhead` | ≤ 100 ns per await; ≥ 1M tasks/s | lazy frames: a ready await is a call and a null test; `all!` allocates nothing of its own when children finish at once | met by design |
| `serde-throughput` | ≥ 0.5x Node | byte arrays, direct monomorphized calls, a streaming `JsonWriter` in std | plausible; std work |
| `text-throughput` | ≥ 0.5x Node | `(array i8)` strings, a growable builder, `Interp` sized once | plausible |
| `dead-code` | ≤ 10 KB per 1,000 lines; no unused std | reachability over TIR; folding; structural types | met by design; measured in slice 7 |
| `long-run-memory` | flat after warm-up | no global caches in std's runtime; wake table entries freed on completion | depends on wasmtime's collector; measured in slice 10 |
| `unit-test-perf` | ≤ 1 ms per test; 1,000 in ≤ 1 s | one module per program, `InstancePre`, pooling, parallel workers (§19.3) | met by estimate |
| `proptest-perf` | ≥ 100k cases/s; shrink ≤ 1 s | cases in one instance; scalar `record`; host-side shrinking | met by estimate for simple generators |
| `integration-test-perf` | ≤ 20 ms setup per program | warm `prog_key` and `cwasm` hits; lazy temp directories | met by design |
| `release-check-cost` | debug ≤ 1.3x release | one emission for both tiers; only checks differ | at risk on `i64` multiplication heavy code and on the Cranelift level (§18.6) |
| `test-latency` | ≤ 300 ms | one module rechecked, code keys reused, Cranelift's per-function cache | met by estimate |

## 23. Open Questions And Inconsistencies

### 23.1 Open Questions For The Owner

1. **Bounded inlining and scalar replacement in the first release.** The
   triage put inlining and escape analysis in Later, but `allocations`
   (at most 1 per iterator chain) and `runtime` (1.5x Node) need them for
   iterator chains and closures. **Recommendation:** pull a bounded
   version into slice 10: inline callees under a size budget and closures
   passed to known callees, and replace non-escaping closures and cells by
   locals. Both run in the one shared emission (§12.6).
2. **Polymorphic recursion: error or boxed fallback.** Answer 8's
   research proposed an error at an instantiation depth limit; the spec's
   [Shapes and Generic Code](../spec/lang/04-type-system.md#shapes-and-generic-code)
   says such code falls back to shared boxed bodies. A fallback needs a
   second, erased code path. **Recommendation:** the error,
   `instantiation-too-deep`, reported by `hd build`, `hd run` and
   `hd test` (§13.4); it is contrived code with a simple fix. The spec
   pass rewrites that section to answer 8.
3. **Two panic categories.** A deadlocked entry (§14.8) and an indirect
   `block_on` or `println` in a forbidden context (§14.9) have no stable
   category. **Recommendation:** add `suspension-deadlock` and
   `suspension-forbidden-context`, the latter matching the check-time
   code, as answer 11 added two.
4. **Resource limit flags.** `--max-heap` is named in the triage; the
   time limit has no flag. **Recommendation:** `--max-heap SIZE` and
   `--time-limit DURATION` on `hd run`, `hd FILE`, `hd FILE.wasm` and
   `hd test` (where it caps each case), with no default limit.
5. **The property-test base seed.** The `determinism` metric wants equal
   output on every run, but a fixed seed explores the same cases each
   time. **Recommendation:** derive each property's first seed from its
   test's stable name, so runs are reproducible, and add `--seed N` to
   vary it.
6. **A panic in the REPL.** The REPL chapter does not say what a panic
   does to the session. **Recommendation:** report it and keep the
   session; the panicking input adds no binding, and mutations it made
   before the panic remain. The spec pass adds a rule.

### 23.2 Inconsistencies Found In The Inputs

1. **Folding reference instances.** The research says `List[Point].push`
   and `List[User].push` fold. With exact Wasm types they fold only when
   `Point` and `User` have the same layout (§13.7); folding them always
   needs erased element storage and a cast per read.
2. **Shapes in the spec.** The type-system chapter's
   [Shapes and Generic Code](../spec/lang/04-type-system.md#shapes-and-generic-code)
   describes one shared body for reference types, dictionaries for trait
   bounds, a boxed fallback for polymorphic recursion, and one reference
   shape for every enum. Answer 8 replaced that strategy. D1 listed only
   `module.interface.dictionaries` (§10.1, item 3).
3. **The transitive `block_on` ban** remains in
   [`req.drive.block-on.transitive`](../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.transitive)
   and [`flow.defer.block-on`](../spec/lang/06-control-flow.md#r-flow.defer.block-on),
   though answer 13 made it direct-only.
4. **The Component Model** is still named by
   [`req.host-wait.leaf`](../spec/lang/11-requirements-and-suspension.md#r-req.host-wait.leaf)
   and by HOST_CAPABILITIES' boundary table, against answer 7.
5. **Resource limits.** The stable panic categories lack
   `heap-exhausted` and `time-limit`
   ([`flow.panic.stable-categories`](../spec/lang/06-control-flow.md#r-flow.panic.stable-categories)),
   though answer 11 added them, and the CLI chapter has no limit flags.
6. **The `allocations` metric** reads V8's heap statistics, while the
   first release runs programs on wasmtime. It can run the release
   `.wasm` under Node through the generated glue (§17.10), or count
   through a stats build's allocation hooks (§16.3).
7. **Start to first output.** `size-startup-heap` asks for at most 5 ms,
   measured through a process, while `startup` allows 20 ms for
   `hd --version`. The 5 ms can only hold for instantiation to output,
   not for a cold process.
8. **One instance per property test** (a first-release feature) meets
   [`flow.panic.poison`](../spec/lang/06-control-flow.md#r-flow.panic.poison):
   a discard is a panic, so each discard costs a fresh instance (§19.4).
   Not a contradiction, but a cost the feature text does not mention.
9. **D1's test plan** said cases are registered only at run time; the
   spec lists them statically. Fixed in §7.3.
10. **The research's crates** `hd_mir` and `hd_opt` and its MIR stage are
    superseded by the one-IR decision (§3.9.1, §11.4).
11. **The triage** keeps inlining and escape analysis in Later while two
    goal metrics depend on them (question 1).

### 23.3 What Was Kept Brief

Sections 21 to 23 are brief by plan. Two designs need a measurement
before more text: the debug tier's Cranelift level and the null collector
for tests (§18.6).
