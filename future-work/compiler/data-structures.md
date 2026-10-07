# New Compiler Design: Core Data Structures

Part of the [compiler design](../README.md).

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
  ([Q6](research.md#q6-program-database-hook)).

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
  ([`cli.test.doc.location`](../../spec/cli/command-line.md#r-cli.test.doc.location)).
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
  research ([Comparison With Vx](research.md#comparison-with-vx)).
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

The audit's findings ([findings](../../audit/compiler/findings-2026-10-04.md),
[directions](../../audit/compiler/architecture-directions-2026-10-05.md)):

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
