# New Compiler: Detailed Design

Status: Design, not decided. Part D1 of 2: pipeline and front half, 2026-10-07.

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
- **D2** is Part 2 of this design: MIR, monomorphization, suspension
  lowering, Wasm GC emission, link, runtime, host interface, test runner.
  This part ends with the interfaces D2 consumes.
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
                       D2: MIR ─► monomorphize ─► Wasm GC codegen ─► link ─► run / test
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
| Body check | body CST, frozen tables | THIR, diagnostics, facts | body | yes | inside `check` | none |
| Module finish | the module's body results | `ModuleResult` | module | yes, across modules | `check` | [§5.3](#53-key-composition) |
| Test overlay | `tests:` block and doc tests of a module | THIR, diagnostics | module | yes | `check-test` | check key plus test uses |
| Coherence | impl heads of one trait over the graph | `overlapping-impl` | trait | yes | `coh` | trait path plus sorted head hashes |
| Init order | init summaries of a folder's modules | statement order, `top-level-read-before-initialization` | folder (only when a group spans modules) | yes | `init` | sorted summary hashes |
| Package result | all of the above | sorted diagnostics, summary | package | serial | `pkgres` | sorted keys of the parts |
| D2 stages | THIR, interfaces | Wasm | module, instance, program | D2 | D2 | D2 |

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
| `hd_check` | body checker, THIR, exhaustiveness, GADT refinement, templates, coherence, init order, facts | `hd_resolve` | yes |
| `hd_cache` | `CacheStore` trait, keys, entry framing, memory store, disk store (feature `disk`), stat manifest, trim | `hd_iface` | memory store only |
| `hd_sched` | `Scheduler` trait, task graph, serial executor, thread pool executor (feature `threads`), budgets, memory cap | `hd_base`, `rayon` (feature) | serial only |
| `hd_driver` | `Session`, command pipelines (check, test plan, doc, fix), output assembly | all of the above | yes |
| `hd_doc` | `hd doc` rendering ([HD_DOC.md](HD_DOC.md)) | `hd_driver` | yes |
| `hd_stdpack` | build-time tool: checks `lib/std` and writes the std pack that `hd_driver` embeds | `hd_driver` | the pack, not the tool |
| D2 crates | `hd_mir`, `hd_mono`, `hd_wasm`, `hd_host_abi`, `hd_run` ([Q16](COMPILER_ARCHITECTURE_RESEARCH.md#crates)) | `hd_check`, `hd_driver` | yes |
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
| `InferVar`, `LocalId`, `ThirIdx` | body | inference variable, local binding, THIR node | none: never leaves the body |
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
| global types | `TyKind` with no inference variables | same scheme, hash-consed | process |
| rows | sorted key lists | same scheme | process |
| body-local types | types holding `InferVar`s | none: owned by one body | body |

- Keywords, prelude names and primitive types are pre-seeded at fixed
  indices, so the common case never locks.
- An ID's value depends on which thread interned first. That is harmless
  because no ID is printed, sorted or hashed (§6.5). The determinism
  matrix shifts IDs on purpose to prove it (§8.1).
- In the browser the shards' mutexes are uncontended single-thread locks.

### 3.4 Types

```rust
/// A type. Bit 31 set: an index into the current body's local arena.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Ty(u32);

pub enum TyKind {
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
- A global type never contains a local one. `intern_global` rejects a
  `TyKind` whose `has_local` flag is set.
- **How body-created types are interned without copies.** A body builds
  types with variables in its local arena. When the body finishes (or a
  type becomes variable-free earlier), `resolve` replaces variables by
  their bindings and interns the result in the global sharded table. Equal
  content gives the equal `Ty` whichever thread interns it. There is one
  table, not a copy per thread (lesson 5), and no ID escapes (lesson 2).

### 3.5 Arenas And Lifetimes

| Arena | Holds | Created | Freed |
| --- | --- | --- | --- |
| session | interners, `DefData`, global types, frozen interfaces, impl tables, memo tables | process start | process end (REPL: never; it is bounded, §7.9) |
| file | `TokenBuf`, `GreenTree`, line starts | parse | after its module's `ModuleFinish`, unless `hd fmt`, `hd fix` or D2 needs the tree |
| folder build | scratch for resolution | `FolderIface` start | its end; the result is the frozen `FolderIface` |
| body | local types, inference tables, trail, THIR under construction | `Body` start; one bump arena per worker, reset per body | body end; THIR moves into the module result |
| module result | THIR of every body, diagnostics, facts | `ModuleFinish` | after D2 has lowered it, or at exit |

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

### 4.6 Item Tree

```rust
pub struct ItemTree {                // per module, from the skeleton
    pub items: Vec<Item>,            // ItemIdx = source order
    pub by_name: HashMap<Symbol, ItemIdx>,
}
pub struct Item {
    pub kind: ItemKind,              // Fn, Data, Enum, Trait, Impl, Template, Alias, Newtype,
                                     // Method, Variant, Field, AssocType, TopLevelGroup, TestsBlock
    pub name: Option<Symbol>,        // None for impls
    pub vis: Vis,                    // Pub | Private
    pub header: NodeIdx,
    pub body: Option<BodyRange>,
    pub parent: Option<ItemIdx>,     // methods, variants, fields
    pub flags: ItemFlags,            // result_omitted, row_omitted, suspends, broken, derived
}
```

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
    pub items: Vec<IfaceItem>,               // pub items, in (module path, source order)
    pub exports: SortedMap<(ModuleId, Symbol), DefId>,
    pub impl_tables: Vec<(ModuleId, ImplTable)>,
    pub templates: Vec<TemplateRef>,
    pub hidden: Vec<IfaceItem>,              // items a template body names (§10, question 4)
    pub header_diags: Vec<(ModuleId, Diagnostic)>,
    pub api_hash: Hash128,                   // shallow: the blob's api sections
    pub deep_hash: Hash128,                  // §4.11.3
    pub heads_hash: Hash128,                 // every impl head, private targets included
    pub item_hashes: SortedMap<StablePath, (Hash128, Hash128)>,  // shallow, deep
    pub blob: Arc<[u8]>,                     // the canonical bytes; also the cache payload
}
```

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
build time from MIR (the research's Q4b contradiction 2).

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
pub struct ImplTable {                                   // per module, frozen with its folder
    pub by_trait: HashMap<DefId, SmallVec<[ImplEntry; 2]>>,
    pub inherent: HashMap<DefId, SmallVec<[ImplEntry; 1]>>,
}
pub struct ImplEntry { pub def: DefId, pub head_key: HeadKey, pub generic: bool }
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
Then it runs the deferred checks and substitutes the rows into THIR.

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
  subsumption) become explicit THIR nodes, so D2 never re-derives them.
- **Mutability and access** checks run on THIR paths after inference
  ([Mutable Paths](../spec/lang/04-type-system.md#mutable-paths)).
- **Typed holes.** `_` and `todo()` record the expected type and the
  in-scope names whose types fit, for the hole diagnostic.
- **Definite initialization** of locals and the `let-else` rules are a
  forward dataflow pass over the body's THIR.

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
Inside one module, M3 computes it from that module's THIR. But an
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

#### 4.13.11 The Checked Body: THIR

The checker's output is a typed high-level tree (THIR) per body, built
while checking. It is D2's input.

```rust
pub struct Thir {
    pub item: DefId,
    pub nodes: Vec<ThirNode>,           // ThirIdx; children before parents
    pub locals: Vec<LocalDecl>,         // name, type, mutability, span
    pub root: ThirIdx,
}
pub struct ThirNode { pub kind: ThirKind, pub ty: Ty, pub span: Span }
pub enum ThirKind {
    Local(LocalId), Item(DefId, TyList), Literal(Lit),
    Call { callee: Callee, args: IdxRange, row: Row },  // Static(DefId, TyList) | Trait(..) | Value
    Bang { call: ThirIdx },                             // a suspension point
    Field { base: ThirIdx, field: DefId }, Index { base: ThirIdx, index: ThirIdx },
    Coerce { kind: Coercion, inner: ThirIdx },
    Data { ty: DefId, fields: IdxRange }, Tuple(IdxRange),
    Closure { body: Box<Thir>, captures: IdxRange },
    If { .. }, Loop { .. }, Break { .. }, Return(ThirIdx), Try(ThirIdx),  // `?` made explicit
    Match { scrutinee: ThirIdx, arms: IdxRange, decision: DecisionRef },
    Let { pat: PatIdx, init: ThirIdx, else_: Option<ThirIdx> }, Defer(ThirIdx),
    With { keys: RowId, providers: IdxRange, body: ThirIdx },
    Hole { expected: Ty }, Poison,
}
```

- Every operator, interpolation, iteration, implicit wrap, default
  argument and trait dispatch is explicit. D2 never consults names or
  scopes.
- Types stay generic (`Param`). D2 monomorphizes.
- A body with an error still has THIR, with `Poison` nodes. In the first
  release D2 does not lower a module with errors. The Later "deferred type
  errors" feature lowers `Poison` to a panic.
- THIR is never serialized. On a cache hit, D2 reads its own cached MIR,
  so THIR exists only in the run that checked the module.

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
| instantiation depth | D2 | D2 | D2 |
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
| interface blobs, check results, coherence and init results, package results; D2's MIR and code | `$HD_CACHE/obj/` | per user, across worktrees and packages | keys are content hashes, so worktrees on one commit share everything |
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
| `check` | a module's diagnostics, init summary, row results, and fact records | `ModuleFinish` | output, `InitOrder`, D2 (beside its own MIR entry) |
| `check-test` | the test overlay's diagnostics and test registrations | `TestOverlay` | `hd check --tests`, `hd test` |
| `coh` | one trait's overlap diagnostics | `Coherence` | output |
| `init` | one folder's statement order and its diagnostics | `InitOrder` | output, D2 |
| `pkgres` | the package's sorted diagnostics and summary counts | `PackageResult` | the warm fast path |
| `depfiles` | a fetched dependency's file list with content and api text hashes | first use of the dependency | every later run (§5.5) |
| D2 kinds | MIR per module, code per instance, precompiled engine modules | D2 | D2 |

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
3. Build the **test plan**: the list of test programs. Unit tests per
   module with test code; one program per integration test file; doc tests
   grouped per module. Test names are registered at run time by
   [Registration Functions](../spec/lang/10-modules.md#registration-functions),
   so the plan lists programs, not cases. `--filter` is applied by the
   runner.
4. Hand the plan to D2, which builds, runs and reports. The package's
   executables are built first ([`cli.test.builds-executables`](../spec/cli/command-line.md#r-cli.test.builds-executables)).

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
| 3. Bodies | `hd_check` chapter by chapter in spec order: inference, rows, suspension, GADTs, exhaustiveness, templates, coherence, init order, THIR | type-phase cases pass with a known-failures list; std's bodies check clean; `errors-per-run`, `mistakes` and `diag-location` measured; `pathological` runs within budgets |
| 3b. Fix-its and `hd fix` | re-parse safety, `hd fix` rounds | `fixit-safety` at 100%; fix-it share of `mistakes` measured |
| 4. Cache and threads | disk store, manifest, eviction and `hd cache gc`, the pool scheduler, the opt-in memory cap, the embedded std pack, verify mode | `recheck-precision`, `edit-latency`, `cold-check`, `resources`, `startup`, `determinism` (full matrix), `incremental-soundness`, `cache-contention`, `cache-growth`, `parallel-speedup` with peak memory at 8 threads at most 1.5x that at 1 thread |
| 5. Browser front end | `hd_web` with the stepping scheduler and the JS stores | the playground checks programs in a worker; size measured so the owner can set a budget (answer 4); `long-session` flat in the browser |

D2's slices 6 to 10 follow, as in [Q16](COMPILER_ARCHITECTURE_RESEARCH.md#build-order-1).

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

## Part D2: The Back Half (pending)

D2 designs MIR, monomorphization, suspension lowering, Wasm GC emission,
link, the runtime, the host interface and the test runner. It consumes
these interfaces from D1 and may not reach around them:

| Interface | From | What D2 gets |
| --- | --- | --- |
| `Thir` per body (§4.13.11) | `hd_check` | typed, desugared, generic bodies with explicit coercions, dispatch, `?` and suspension points; only in the run that checked the module |
| `ModuleResult` | `hd_check` | THIR of every body, item tree, init summary, fact records, diagnostics; whether the module has errors |
| folder interfaces (§4.10, §4.11) | `hd_resolve`, `hd_iface` | signatures, impl tables, templates and hidden items, default and fact expressions as checked token ranges, per-item deep hashes for instance keys |
| types and rows | `hd_types` | the global type interner (`Ty`, `TyKind`, rows), stable paths for every `DefId`, and `StableHash` |
| init order | `InitOrder` | statement order per initialization group |
| test plan (§7.3) | `hd_driver` | test programs: unit per module, integration per file, doc tests per module |
| cache | `hd_cache` | `CacheStore`, the key rules of §5.3 (toolchain key, hygiene), entry framing, verify mode, eviction. D2 may add kinds `mir` (key: `check_key` plus the D2 version), `code` (per instance), `link` (per program), and precompiled engine modules |
| scheduler | `hd_sched` | `TaskKind::Ext` tasks with dependencies on D1 tasks, fuel, cancellation, the opt-in memory cap, content-ordered output |
| diagnostics | `hd_diag` | the `Diagnostic` record for D2's own errors, such as instantiation depth |
| determinism rules | §6.5 | content order, local counters, no IDs in output or keys; D2's entries join the determinism matrix |
