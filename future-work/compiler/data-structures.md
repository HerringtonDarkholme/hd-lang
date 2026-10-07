# New Compiler Design: Core Data Structures

Part of the [compiler design](README.md).

## Changes In This Pass

Backend lane, 2026-10-07. The owner asked to "design each data structure
better". §3.1 to §3.10 keep their numbers and anchors. §3.11 to §3.25
are new: one full design per structure, a memory budget and the schema
language. Every structure design answers the same nine points, in this
order: purpose and consumers, layout, identity, lifetime, growth,
determinism, wire form, memory, accessors.

Edits in this file:

1. **IDs (§3.1).** One sentinel rule: every index is a plain `u32`
   newtype, and `u32::MAX` (`NONE`) means "absent" in every column, in
   memory and on disk. `NonZeroU32` is dropped, so a column can be cast
   from bytes without a second rule (mine).
2. **Stable paths (§3.2)** are a hash-consed trie of `(parent, segment)`
   nodes with an incremental 128-bit path hash per node (mine).
3. **Interners (§3.3)** share one generic, `ShardedInterner`, built on the
   chunked `AppendVec` of §3.9.4.
4. **Types (§3.4, §3.9.2)**, from type-checking.md §17 item 1:
   `Readonly(Ty)` becomes `Mut(Ty)`; `IntLit` is gone, and one `Infer`
   form keeps its kind in the inference table; a body-local `Rigid` tag
   holds GADT existentials. The decoded view is `TyView`, the accessor
   API of [type-checking.md §1.4](type-checking.md#14-the-type-accessor-api).
5. **InternPool (§3.9.2)**: a `meta` column (flags and node count), a
   corrected constant encoding, interned lists and rows, and no
   hash-consing in the body-local pool (mine).
6. **Diagnostics (§3.8)** are columns in a `DiagBuf`, so rollback
   truncates them like every other body column.
7. **Wire rule (§3.20.2, mine).** No run ID is ever written to disk or
   hashed. Every cache entry carries its own string, path and type tables.
   The schema knows which operand words are IDs and generates the remap.
   This corrects D2's "serialization is a column copy" for TIR, whose
   words hold pool indices and `DefId`s.
8. **Tokens (§3.11)** drop the `indent` and `depth` columns: indentation
   moves to a per-line table, and the layout cursor tracks depth itself.
   A token is 9 bytes, not 13.
9. **Memory budget (§3.24)** and **schema language (§3.25)** are new.

Edits in other files:

| File | Edit |
| --- | --- |
| [checking-and-tir.md](checking-and-tir.md) | TIR's encoding rewritten (columns, `Ref` without a `consts` column, the capture table, sub-bodies, side tables, wire form, size asserts); §17 items 1 to 10 of type-checking.md that touch TIR or §4.13 applied; see its header note |
| [syntax.md](syntax.md) | §4.1 `TokenBuf` and §4.4 `GreenTree` point to §3.11 and §3.13 for their final layouts |
| [resolution-and-interfaces.md](resolution-and-interfaces.md) | §4.11.1 blob `types` section uses the pool's tag, data and `extra` encoding with blob-local operands; §4.12.1 `ImplTable` columns point to §3.17 |
| [cache.md](cache.md) | §5.4 entry header and sections point to §3.20; §5.5 `mtime_ns` is `i64` |
| [codegen.md](codegen.md) | §13.2 `InstanceSet` and §13.8 `CodeEntry` point to §3.22 for their columns |

## 3. Core Data Structures

This chapter defines every data structure the compiler keeps. §3.1 to
§3.10 give the shared rules: IDs, paths, interners, types, arenas,
poison, spans, diagnostics, the data-oriented encoding and each IR's
contract. §3.11 to §3.23 design each structure in full. §3.24 sums their
memory, and §3.25 defines the schema that generates their code.

**Where each structure is designed.**

| Structure | Section | Built by | Freed |
| --- | --- | --- | --- |
| IDs and sentinels | §3.1 | | |
| stable paths, path hashes | §3.2 | resolution, interface readers | process |
| strings and symbols | §3.3 | lexer, interface readers | process |
| InternPool: types, rows, lists, constants | §3.9.2 | resolution, checker, readers | process |
| diagnostics and fix-its | §3.8 | every pass | after output |
| tokens and line tables | §3.11 | lexer | with the file's tree |
| layout cursor, skim state | §3.12 | parser | per parse |
| green tree, wire form, JS decoder | §3.13 | parser | after `ModuleFinish` |
| header skeleton, item index | §3.14 | header pass, parser | with the module scope |
| name-resolution tables | §3.15 | `FolderIface`, `ModulePrep` | process (frozen) |
| folder interface and blob | §3.16 | `FolderIface` | process (mapped) |
| impl tables | §3.17 | `FolderIface` | process (frozen) |
| TIR | §3.18, [§4.13.11](checking-and-tir.md#41311-the-typed-ir-tir) | checker | after `ModuleFinish` writes it |
| per-body checker scratch | §3.19 | `Body` | per body (reused) |
| cache entries, manifest | §3.20 | `ModuleFinish`, driver | on disk |
| task graph | §3.21 | driver, tasks | end of run |
| instance table, code entries | §3.22 | collection, `Emit` | end of build; on disk |
| Wasm emission buffers | §3.23 | `Emit`, `Link` | per instance (reused) |

### 3.1 IDs And Their Scopes

Every ID is a `u32` newtype. IDs are cheap to compare and never reach
output or a hash. Each has a stable form used at boundaries.

| ID | Scope | Indexes | Stable form at boundaries |
| --- | --- | --- | --- |
| `Symbol` | run | interned string (§3.3) | the string |
| `PackageId` | run | resolved package | `root`, or `HOST_PATH@VERSION` plus tree hash, or a workspace-relative path for a path dependency |
| `FileId` | run | source file | package plus package-relative path |
| `ModuleId` | run | module, dense | package plus module path, as `shop.cart` |
| `FolderId` | run | folder, dense | package plus folder path, as `src/shop` |
| `PathId` | run | a node of the stable-path trie (§3.2) | the printed path |
| `DefId` | run | any declaration, member, variant or impl; a `PathId` whose node is an item | `StablePath` (§3.2) |
| `Index` | run (global) or body (local) | an InternPool item (§3.9.2) | the entry's own type or constant table (§3.20.2) |
| `Ty`, `TyList`, `RowId`, `AssocList` | as `Index` | an `Index` whose tag is a type, list or row | as `Index` |
| `TokenIdx`, `NodeIdx` | file | token, green node | byte offsets |
| `ItemIdx` | module | item in source order | item path |
| `InferVar`, `LocalId`, `Inst`, `CaptureId`, `SubId` | body | inference variable, local, TIR instruction, capture, sub-body | none: never leaves the body |
| `TaskId` | run | scheduler task | none |
| `InstId` | build | collected instance | `InstanceKey` (§3.22) |

```rust
// hd_base: IDs implement Eq and Hash, not Ord and not StableHash.
// Sorting anything for output needs an explicit content key (§6.5).
macro_rules! id { ($name:ident) => {
    #[derive(Copy, Clone, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
    #[repr(transparent)]
    pub struct $name(u32);
    impl $name {
        pub const NONE: Self = Self(u32::MAX);
        #[inline] pub fn get(self) -> Option<Self> { (self.0 != u32::MAX).then_some(self) }
        #[inline] pub fn idx(self) -> usize { debug_assert!(self.0 != u32::MAX); self.0 as usize }
    }
    const _: () = assert!(core::mem::size_of::<$name>() == 4);
} }
id!(Symbol); id!(FileId); id!(ModuleId); id!(FolderId); id!(PathId); id!(DefId);
```

**Sentinel rule (mine).** `NONE = u32::MAX` means "absent" in every
column, in memory and on disk. Index 0 is a real item, so a column is
indexed without an offset. An API that returns an optional ID returns
`Option<Id>` through `get()`; storage never holds `Option<Id>`, which
would be 8 bytes. D1 used `NonZeroU32`; one rule for memory and wire is
simpler than two, and `bytemuck` can cast any column whose items are
`u32` newtypes.

**Narrow IDs.** Where a count is bounded by the language, a column may
use `u16` (a generic parameter's index, a tuple arity, a field index):
the limits of §4.15 keep them in range, and the builder asserts it.

Making IDs unsortable and unhashable for stable hashes is a compile-time
guard for prior-art lesson 2 (mine): code that tries to sort by an ID or
hash one into a key does not compile.

### 3.2 Stable Paths And Stable Hashing

**Purpose and consumers.** A stable path names a declaration across
runs, machines and checkouts. Interface blobs, cache keys, instance keys,
diagnostics and the program database print or hash it. In a run, code
compares `DefId`s and never builds a path.

**Layout: a hash-consed trie (mine).** Every package, module segment and
item segment is one trie node. A `DefId` is a `PathId` whose node is an
item. Equal paths are the same node, so path equality is one compare.

```rust
pub struct PathTable {                    // a ShardedInterner (§3.3) over PathNode
    parent: AppendVec<PathId>,            // 4 B; NONE for a package root
    seg:    AppendVec<u32>,               // 4 B; Symbol, or an index into impl_segs
    kind:   AppendVec<PathKind>,          // 1 B; Package | Module | Item | Member | Variant | Impl | Hidden
    hash:   AppendVec<Hash128>,           // 16 B; the stable path hash, computed at intern
    // per-item data, filled once by the task that declares the item (set-once slots)
    def_module: AppendVec<ModuleId>,      // 4 B; NONE for non-items
    def_loc:    AppendVec<u32>,           // 4 B; header NodeIdx, or an interface record (high bit)
}
pub struct ImplSeg { pub head: Hash128, pub ordinal: u16 }   // in a side vector `impl_segs`
#[repr(u8)] pub enum PathKind { Package, Module, Item, Member, Variant, Impl, Hidden }

pub struct StablePath<'a> { pub table: &'a PathTable, pub id: PathId }  // a view; Display prints it
```

- **The path hash is incremental (mine).**
  `hash(node) = xxh3_128(hash(parent) ‖ kind ‖ segment bytes)`. One hash
  per new node, at intern time, so every `DefId` has its 128-bit stable
  hash in one load. Interface blobs, cache keys and instance keys use it
  directly. A package root hashes its `PackageKey` text.
- An impl segment is the hash of its head's normalized text plus an
  ordinal among identical heads in the module (identical heads are
  `overlapping-impl` anyway).
- **Collisions.** Within one interface blob, the writer checks that no two
  paths share a hash, as rustc checks `DefPathHash` per crate. A collision
  is an internal error naming both paths. With 128 bits it is not
  expected.
- `Hash128` is xxh3-128. The algorithm is named in the cache layout file,
  so a later remote cache can move to BLAKE3 without ambiguity.
- Program-database symbols are the printed stable path, as
  `github.com/acme/json@2.1.0/json.parse/Parser.next`
  ([Q6](research.md#q6-program-database-hook)).

**Stable hashing.**

```rust
pub struct StableHasher(xxh3::Xxh3);             // 128-bit, streaming, fixed seed 0
pub trait StableHash {
    fn stable_hash(&self, h: &mut StableHasher, cx: &StableCx);
}
// StableCx maps run IDs to stable forms: a DefId writes its path hash,
// a Ty writes its canonical encoding (§3.20.2), a Symbol writes its bytes.
```

- `StableHash` is derived only for types listed in the schema (§3.25),
  whose ID fields the schema knows. A run ID has no `StableHash` impl, so
  it cannot enter a key by accident.
- Integers hash little-endian at their declared width. Lists hash their
  length first. Strings hash length then bytes. No `usize` is hashed.

**Hash tables in memory.** Every in-run hash table is a
`hashbrown::HashTable<u32>` whose values are row numbers into columns,
hashed by `foldhash` with a fixed seed. It is an index, never storage
(§3.9.4). Iteration over a table is forbidden outside `hd_base` (a
clippy `disallowed_methods` entry), so no table order reaches output. The
determinism matrix runs with a varied seed (§8.1) to prove it.

**Identity, lifetime, growth.** `PathId`s are run-scoped, the table lives
for the process, and it only grows. A REPL session adds the input's
items, bounded by the session's code (§7.9).

**Determinism.** Node IDs depend on interning order; nothing prints or
sorts them. Content order for output uses the printed path bytes.

**Wire form.** None: blobs and entries carry their own path tables of
`(parent, segment)` records, written from the trie and re-interned on
read (§3.16, §3.20.2).

**Memory.** 33 bytes per node in the columns plus about 8 bytes of index
at a load factor of 7/8 near 41 bytes. A 10k-line package with std
touches about 15,000 nodes: about 0.6 MB.

**Accessors.** `paths.intern(parent, kind, seg) -> PathId`,
`paths.parent(id)`, `paths.hash(id)`, `paths.print(id, &mut String)`,
`paths.def_module(id)`. Code outside `hd_base` sees `DefId` and these
methods only.

### 3.3 Interners

| Interner | Holds | Concurrency | Lifetime |
| --- | --- | --- | --- |
| strings (`Symbol`) | identifiers, module and package names, literal text that types name (string prefixes, keys) | `ShardedInterner` | process |
| stable paths (`PathId`) | the trie of §3.2 | `ShardedInterner` | process |
| InternPool (`Index`) | types with no inference variables, rows, lists and constant values | `ShardedInterner` with four columns (§3.9.2) | process |
| body-local pool | types holding `InferVar`s or rigid placeholders, in the InternPool's encoding | none: owned by one body; not hash-consed | body |

**One generic (mine).** All three global interners are one structure:

```rust
pub struct ShardedInterner<C: Columns> {
    locals: Box<[CachePadded<Local<C>>]>,      // one per worker thread, plus one for the driver
    index:  [CachePadded<Mutex<HashTable<(u32 /*hash*/, u32 /*id*/)>>>; 64],
}
struct Local<C> { cols: C /* AppendVecs */, len: AtomicU32 }
// An id packs the owner and the row: bits 25..30 owner thread, 0..24 row.
// Bit 31 is reserved: the pool uses it for body-local types, TIR for constants.
```

1. **Lookup** hashes the content, locks the shard `hash % 64`, and probes
   with the stored 32-bit hash before comparing content.
2. **Miss**: append to the calling thread's columns, then publish
   `len` with `Release`, then insert into the shard and unlock.
3. **Read** of an existing ID takes no lock. An ID is obtained only from
   the index (after its mutex's acquire) or from data published after it,
   so the item is visible. `AppendVec` never moves an item (§3.9.4).
4. **Capacity.** 64 owner threads and 2^25 rows per owner. The thread
   count is `min(cores, 8)` by default
   ([open-questions.md](open-questions.md)); over-capacity is an internal
   error, not a user limit.
5. **Pre-seeding.** Keywords, prelude names, primitive types, `Poison`,
   the empty list, the empty row and common `Option[prim]` types are
   built at compile time into `static` columns of owner 0, so a fresh
   process allocates nothing for them (the `startup` target) and their
   IDs are constants, such as `Ty::I32`.

**Strings.** The string interner's columns are `bytes: AppendVec<u8>`
and `span: AppendVec<(u32 /*offset*/, u32 /*len*/)>`. A `Symbol` resolves
to `&str` with two loads. Strings are UTF-8 and NFC-checked by the lexer
before interning. Decoded literal values that only TIR needs (escapes
processed) go to the pool's `bytes` column as `Str` constants, not here.

- An ID's value depends on which thread interned first. That is harmless
  because no ID is printed, sorted or hashed (§6.5). The determinism
  matrix shifts IDs on purpose to prove it (§8.1).
- In the browser there is one owner and the shards' mutexes are
  uncontended.

**Memory.** A symbol costs its bytes plus 8 bytes of span and about 9
bytes of index: about 25 bytes for a 10-byte name. A 10k-line check
interns about 20,000 symbols with std: about 0.5 MB.

### 3.4 Types

A type is an index into the InternPool (§3.9.2). `TyView` below is the
**decoded view** that the accessor layer returns (§3.25). It is not how
types are stored: storage is a one-byte tag, a `u32` data word, a `u32`
meta word and, for variable parts, words in the pool's `extra` column.
It is the API of [type-checking.md §1.4](type-checking.md#14-the-type-accessor-api);
the generated pool views implement `TyRead` and `TyBuild`.

```rust
/// A type: an InternPool index. Bit 31 set: an index into the current body's local pool.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Ty(u32);

pub enum TyView<'a> {                // decoded on demand; slices borrow `extra`
    Prim(Prim),                      // i8..u64, usize, f32, f64, bool, char, string, void
    Never,
    Poison,                          // see §3.6
    Adt { def: DefId, args: &'a [Ty] },
    Tuple { elems: &'a [Ty], rest: Option<Ty> },   // rest element of tuple rest types
    Option(Ty),                      // T? is Option[T]; its own tag for speed only
    Fn { params: &'a [Ty], result: Ty, row: RowId, suspends: bool },
    TraitValue { def: DefId, args: &'a [Ty], bindings: &'a [(Symbol, Ty)] },
    Param(ParamRef),                 // (owner DefId, index); declared, never inferred
    Assoc { base: Ty, trait_: DefId, name: Symbol },  // an unnormalized projection
    Mut(Ty),                         // the mutable view `mut T`; `T` alone is readonly
    Infer(InferVar),                 // body-local only; the variable's kind is in the inference table
    Rigid(RigidVar),                 // body-local only; a GADT existential of one arm
}
```

- **`Mut`, not `Readonly`** (type-checking.md §17 item 1). In the spec
  `T` is the readonly view and `mut T` the marked form
  ([Views](../../spec/lang/04-type-system.md#views)).
- **One inference form.** A literal is an `Infer` variable whose kind
  (general, integer literal, float literal) lives in the inference table
  (§3.19), because unification merges kinds. So there is no `IntLit`
  and no float-literal tag.
- **`Rigid` (mine, for type-checking.md §6.1).** An existential gets a
  fresh rigid placeholder per arm. It never unifies with anything but
  itself and cannot escape the arm, so it lives only in the local pool.
- `TyList`, `AssocList` and `RowId` are interned slices, so type equality
  is one integer comparison for global types.
- A global type never contains a local one. Interning into the global
  pool rejects an item whose `HAS_LOCAL` flag is set.
- **How body-created types are interned without copies.** A body builds
  types with variables in its local pool. When the body finishes (or a
  type becomes variable-free earlier), `resolve` replaces variables by
  their bindings and interns the result in the global pool. Equal content
  gives the equal `Ty` whichever thread interns it. There is one pool, not
  a copy per thread (lesson 5), and no ID escapes (lesson 2).

**Rows.** A row in the pool is one item: its keys, its declared row
parameters, and, in the local pool only, pending private rows:

```rust
pub struct RowView<'a> {
    pub keys: &'a [Ty],                  // sorted by Ty value for in-run merges
    pub params: &'a [RowParamRef],       // declared row parameters listed in the row
    pub pending: &'a [(RowVar, RowId)],  // local only: RowVar(f) minus these keys (type-checking.md §5.1)
}
```

Keys are sorted by `Ty` value for linear merges inside a run. Printing
and hashing re-sort them by canonical content (§3.20.2), never by `Ty`
value (the tsgo lesson).

### 3.5 Arenas And Lifetimes

| Arena | Holds | Created | Freed |
| --- | --- | --- | --- |
| session | interners, path table, global pool, frozen interfaces and their memo tables, impl tables, solver memo, task result slots | process start | process end (REPL: never; it is bounded, §7.9) |
| file | source bytes, `TokenBuf`, `GreenTree`, line tables | parse | after its module's `ModuleFinish`, unless `hd fmt`, `hd fix` or the playground keeps the tree |
| folder build | the use worklist, header lowering scratch, the blob writer's buffers | `FolderIface` start | its end; the result is the frozen blob |
| worker | one `BodyCx` and one set of TIR columns (§3.19), the emission buffers (§3.23) | worker start | worker end; truncated to empty per body or instance, never shrunk |
| module result | each body's TIR moved out of the worker columns, `DiagBuf`s, row and init facts | the end of each body | `ModuleFinish`, after writing the `check` and `tir` entries |
| build | the instance table, vtable and import sets, mapped `tir` and `code` entries | collection start | end of the build |

**Frozen versus per-task.** A `FolderIface` is written once, by its task,
into a `OnceLock` slot, and read only by tasks that the graph orders after
it. A `ModuleScope` is the same. Body tasks therefore read shared data with
no locks. The only shared mutable structures are the append-only
interners, the interface memo tables (§3.16) and the solver memo, whose
answers are pure functions of frozen inputs (§4.12.2).

**Moving a body out of the worker columns.** At `finish`, the builder
copies the body's ranges of each column into one exact-size allocation
in the module result (one `memcpy` per column, §3.18) and truncates the
worker columns to empty. The worker keeps its capacity for the next
body, so steady-state checking allocates once per body for the output.

### 3.6 The Poison Type

1. `Ty::POISON` is a pre-seeded global type.
2. It unifies with every type, satisfies every bound, has every member, and
   entails every row.
3. Any check whose operand or expected type contains poison stays silent.
   Only the error that created the poison reports. The `HAS_POISON` meta
   flag (§3.9.2) makes "contains poison" one load.
4. Sources of poison: an unknown name (reported once per name per body), a
   broken header (reported once in its module), a syntax error node, a
   failed limit, and an item whose interface is poison.
5. A poisoned item still exports: its interface record carries `Poison`
   where its type is unknown, so dependents check the rest without noise.

### 3.7 Spans And Files

```rust
pub struct Span { pub file: FileId, pub lo: u32, pub hi: u32 }   // byte offsets; 12 B
pub struct SpanIdx(u32);   // body-local: an index into the body's span column, for blame and diagnostics
```

- Line and column are computed only when output is rendered, from the
  file's line table (§3.11).
- A body stores spans as `NodeIdx` (TIR's `syn` column) or as `SpanIdx`
  into its own `(lo, hi)` columns. The file is implied by the body.
- In a cache entry a span is a byte range in the entry's own module file,
  or `(file path index, lo, hi)` into the entry's path table for another
  file (§3.20.2).
- Doc tests map their spans back to the `##` line that holds them
  ([`cli.test.doc.location`](../../spec/cli/command-line.md#r-cli.test.doc.location)).
- Offsets are `u32`, so a file is at most 4 GiB. The practical limit is
  16 MiB (§4.15).

### 3.8 Diagnostic Records

**Purpose and consumers.** Every pass reports through a `DiagBuf`.
`ModuleFinish` sorts, deduplicates by root key and writes them to the
`check` entry. The renderer reads them at output; `hd fix` reads fix-its.

**Layout.** Diagnostics are rare: zero in clean code. So the per-record
cost matters less than two other things: rollback (a trial's diagnostics
must truncate, TC-5) and the wire form. A `DiagBuf` is columns:

```rust
pub struct DiagBuf {
    // one row per diagnostic
    code:     Vec<Code>,          // u16, generated from the spec's code list
    severity: Vec<Severity>,      // u8: Error | Warning
    primary:  Vec<Span>,          // 12 B
    message:  Vec<MsgId>,         // u16 template id
    args:     Vec<Range32>,       // into `arg_kind`/`arg_val`
    labels:   Vec<Range32>,       // into the label columns
    notes:    Vec<Range32>,
    fixes:    Vec<Range32>,
    root:     Vec<RootKey>,       // 12 B: one diagnostic per root cause (§4.14)
    // pooled parts
    arg_kind: Vec<ArgKind>,       // u8: Ty | Def | Sym | Int | Text | Span | Code
    arg_val:  Vec<[u32; 2]>,      // the value: a Ty, a DefId, a Symbol, an i64, a text range
    label_span: Vec<Span>, label_msg: Vec<MsgRef>,
    note_msg: Vec<MsgRef>,
    fix_title: Vec<MsgRef>, fix_safety: Vec<FixSafety>, fix_edits: Vec<Range32>,
    edit_file: Vec<FileId>, edit_lo: Vec<u32>, edit_hi: Vec<u32>, edit_text: Vec<Range32>,
    text: Vec<u8>,                // edit text and literal message text
}
pub struct Range32 { pub start: u32, pub len: u32 }
pub struct RootKey { pub item: DefId, pub cause: CauseKind /* u8 */, pub value: u32 }
pub struct MsgRef { pub id: MsgId, pub args: Range32 }
pub enum FixSafety { Exact, Suggestion }   // Exact passed the re-parse check; `hd fix` applies it
```

- A diagnostic row costs about 60 bytes plus its parts. A typical one
  with two arguments, one label and one fix-it costs about 150 bytes.
- **Message arguments are typed, not strings.** They render through
  stable paths with the shortest unambiguous name, so a message is the
  same on every run and machine.
- **Checkpoint.** A `DiagBuf` checkpoint is the lengths of its row and
  part columns; rollback truncates them.

**Identity.** A diagnostic has no ID. `ModuleFinish` orders by the total
key of §4.14 and drops exact duplicates and repeated root keys.

**Lifetime.** One `DiagBuf` per body, moved into the module result, then
sorted into the module's buffer and written. Header diagnostics live in
the folder's blob (`diags` section).

**Wire form.** The `diags` section of a `check` entry or blob is the same
columns, with `Ty` and `DefId` arguments remapped through the entry's
type and path tables and spans in stable form (§3.20.2). Rendering
happens at output, so `--format` never enters a key.

**Memory.** Tens of KB even for a broken 10k-line package; capped in
output by `--max-errors`, not in storage, so summary counts stay exact.

**Accessors.** `diags.report(code, span).arg_ty(t).label(span, msg)
.fix(title, edits).emit()`, a generated builder per message template that
takes exactly the template's typed arguments, so a missing or
mistyped argument does not compile.

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
  `extra: []u32` array. `MultiArrayList` stores a struct's fields as
  separate arrays. The InternPool stores types and values the same way,
  with per-thread shards and a thread ID in the index's high bits since
  Zig 0.14. The tokenizer stores only a tag and a start offset.
- **Carbon** SemIR: the checker emits the semantic IR directly; typed
  instruction kinds over value stores, read through typed accessors
  (`inst.As<T>()`), and IR blocks as ranges.
- **Yuku**, a Zig JS/TS parser
  ([write-up](https://www.arshad.fyi/writings/data-oriented-design-in-yukus-parser)):
  52-byte nodes (a 44-byte tagged union plus an 8-byte span) asserted at
  compile time; children in an `extras` array as `{start, len}`; five
  scratch buffers flushed with one bulk copy; rewinding by
  `shrinkRetainingCapacity`; strings as source spans unless decoded; a
  40-byte header then position-independent columns; a generated
  `Int32Array` decoder in JavaScript; and the layout derived from one
  definition at compile time. It parses an 8 MB file into 852,919 nodes
  in about 19 ms.
- **Vx**: flat type and HIR streams per worker, 256-bit IDs, an epoch
  merge, and `.vxm` metadata cast with `bytemuck`. The streams and the
  cast are used here; the IDs and the merge were not adopted
  ([Comparison With Vx](research.md#comparison-with-vx)).
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
- **The item index** is an index over the syntax tree and the interface,
  not a copy of either (§3.14).

| IR | Consumers | Encoding | Why |
| --- | --- | --- | --- |
| tokens | the parser, skim mode, the formatter | columns (§3.11) | read in order, one or two columns at a time |
| syntax tree | `hd fmt`, `hd fix`, `hd doc`, the playground's JS, the program database (Later) | compact columns, plus named-field typed views generated from `hd.ungram` in Rust and in JS (§3.13) | outside readers want Yuku's ergonomics; storage stays at 14 bytes per node |
| folder interface | resolution, the checker, `hd doc`, D2 | its blob, read in place through views (§3.16) | memory-mapped from the cache; never decoded into a second copy |
| TIR | D2: collection and emission | Zig-style tag + 8-byte data + `extra` (§3.18) | internal; dense |
| Wasm code entries | the linker | bytes plus relocation columns (§3.22) | patched in place |

The InternPool (§3.9.2) is storage that all of them share, not an IR.
The syntax tree is the only IR that leaves the compiler, so it is the
only one with named-field views at its boundary. TIR and the pool are
internal, so they take the dense encoding, and the accessor layer
(§3.25) gives the compiler's own code named fields anyway.

#### 3.9.2 The InternPool

**Purpose and consumers.** One pool holds every global type, interned
list, row and constant value. Resolution writes header types; interface
readers write types decoded from blobs; the checker writes body types and
literal constants; the solver and the checker read types on every step;
collection and emission read types and constants under substitution.
Reads are random access by `Index`, one item at a time; the type sweep
and the TIR codec read many items in column order.

**Layout.** Four columns per owner thread, from §3.3's `ShardedInterner`:

```rust
pub struct PoolCols {
    tag:   AppendVec<PoolTag>,   // 1 B
    data:  AppendVec<u32>,       // 4 B: inline payload, or an offset into `extra`
    meta:  AppendVec<Meta>,      // 4 B: flags (8 bits) and node count (24 bits)
    extra: AppendVec<u32>,       // variable parts
    bytes: AppendVec<u8>,        // string and byte constants
}
#[repr(transparent)] pub struct Meta(u32);   // HAS_INFER | HAS_RIGID | HAS_POISON | HAS_PARAM | HAS_ROWVAR | HAS_ASSOC | HAS_LOCAL | IS_CONST
pub struct Index(u32);   // bit 31: body-local; bits 25..30: owner; bits 0..24: row
const _: () = assert!(core::mem::size_of::<PoolTag>() == 1);
```

`Meta` holds what type-checking.md §1.4 needs in one load: the flags,
and the node count that the `type-too-large` limit reads (§4.15),
saturated at 2^24 − 1. It is computed from the children's `meta` at
intern time.

| Tag | `data` | `extra` record | Local only |
| --- | --- | --- | --- |
| `Prim` | the `Prim` | none | |
| `Never`, `Poison` | 0 | none | |
| `Option`, `Mut` | the inner `Ty` | none | |
| `Infer` | the `InferVar` | none | yes |
| `Rigid` | the `RigidVar` | none | yes |
| `Param` | offset | `[owner DefId, index]` | |
| `Adt` | offset | `[DefId, args TyList]` | |
| `Tuple` | offset | `[elems TyList, rest Ty or NONE]` | |
| `Fn` | offset | `[params TyList, result Ty, row RowId, flags]`; flags bit 0: suspends | |
| `TraitValue` | offset | `[DefId, args TyList, bindings AssocList]` | |
| `Assoc` | offset | `[base Ty, trait DefId, name Symbol]` | |
| `TyList` | offset | `[len, Ty × len]` | |
| `AssocList` | offset | `[len, (Symbol, Ty) × len]`, sorted by name bytes at intern | |
| `Row` | offset | `[nkeys, key Ty × nkeys, nparams, RowParamRef × nparams, npending, (RowVar, RowId) × npending]` | pending part only |
| `Int` | offset | `[ty, low word, high word]` | |
| `Float` | offset | `[ty, low word, high word]` (the bits) | |
| `Bool`, `Unit` | the value | none; pre-seeded | |
| `Char` | the scalar | none | |
| `Str` | offset | `[byte offset, length]` into `bytes` | |
| `Aggregate` | offset | `[ty, count, value Index × count]` | |
| `ItemConst` | offset | `[DefId, type args TyList]`: a function or a payloadless variant as a value | |

- **Constant encoding fixed.** D1's `IntSmall` put the value in `data` and
  the type in `extra`, which left no word for the offset. Every typed
  constant now keeps `[ty, ...]` in `extra`; only untyped `Bool`, `Unit`
  and `Char` are inline.
- **Lists are items.** A `TyList` is interned like a type, so `Adt`
  equality is a compare of two words, and every `extra` record has a
  fixed width (2 to 4 words) except the list and row items themselves.
- **Items are fixed size**: 9 bytes in the tag, data and meta columns,
  plus `extra` words for compound items, plus an 8-byte index entry.
  Measured shapes give about 20 bytes of columns per type and 28 with its
  index entry; `TyList`s are shared, which is why the average is low.
- **No per-thread copies.** Threads share one pool. Only appending is per
  thread, which is Zig 0.14's scheme, not Vx's merge.
- Primitives, `Poison`, `Never`, `void`, the empty `TyList`, the empty
  row, `true`, `false` and `()` are pre-seeded at fixed indices (§3.3).

**The body-local pool (mine: not hash-consed).** It has the same five
columns in the worker's arena, but no index: `mk` of a type with a
variable always appends. So rollback is truncation of five columns, with
no hash table to repair, and equality of local types is structural, which
unification does anyway. A local type that becomes variable-free is
interned globally by `resolve`, which memoizes per body in a
`resolved: Vec<Ty>` column parallel to the local rows (`NONE` until
resolved).

**Identity and indexing.** `Index` bit 31 marks a local item. Global
items carry an owner thread in bits 25..30. A `Ty`, `TyList`, `RowId`
or constant is an `Index` whose tag the typed accessor asserts in debug
builds.

**Lifetime and growth.** The global pool lives for the process. Each
owner's `AppendVec` grows by doubling chunks (§3.9.4) and never moves an
item. The local pool is truncated to empty at each body's start.

**Determinism.** `Index` values depend on thread order. They never reach
output: printing goes through stable paths, hashing through the canonical
encoding of §3.20.2. Row keys are sorted by `Ty` value only for in-run
merges.

**Wire form.** None for the pool itself. Each cache entry and blob holds
a **type table**: the pool's tag, data and `extra` encoding over the
entry's own path, string and type indices, written in first-use order.
A reader interns each record into the pool on first use and memoizes
the resulting `Index` (§3.20.2).

**Memory.** A cold check of a 10k-line package with std interns about
60,000 items: about 1.7 MB. A warm check touches about 10,000: about
0.3 MB.

**Accessors.** Generated from `pool.ir` (§3.25): `pool.view(t) ->
TyView`, `pool.meta(t)`, `pool.mk(TyNew::Adt { def, args })`,
`pool.list(&[Ty]) -> TyList`, `pool.row(...)`, and `pool.konst(...)`.
Only `hd_types` sees the raw columns.

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

The item index, the path table, impl tables, TIR locals and interface
sections are columns indexed by an ID, never `Vec<struct>` with `HashMap`
storage:

- A pass that reads one property (all item kinds, all local types) reads
  one dense column.
- **Hash maps are only indexes.** A `HashTable<u32>` may point into the
  columns. No data lives only in a map, so iterating a map never decides
  output order (§6.5).
- Sorted sections with binary search replace maps where the table is
  frozen and its order is content, as the interface blob's export index
  does.
- **When not to split (mine).** A record whose every access reads all of
  its fields stays one struct: a task node (§3.21), a relocation
  (§3.22), a manifest record (§3.20.3). Splitting them would add a load
  per field and buy nothing.

**Building blocks** in `hd_base`:

```rust
/// Growable column whose items never move: chunk k holds 2^(k+10) items.
/// Readers index without a lock; only the owner appends.
pub struct AppendVec<T> { chunks: [AtomicPtr<T>; 22], len: AtomicU32 }

/// Exact-size frozen column: the result of a finished task.
pub type Col<T> = Box<[T]>;

/// A range of `extra` words or of another column.
#[repr(C)] pub struct Range32 { pub start: u32, pub len: u32 }

/// Column view over borrowed bytes (a mapped blob or entry): checked once, then cast.
pub struct LeCol<'a, T: Pod> { words: &'a [T] }
```

`AppendVec` index math is two instructions: the chunk is
`31 - lzcnt((i >> 10) + 1)`, the offset follows. Its 22 chunks hold
2^32 items in all, and the first chunk is 1,024 items, so an empty
interner costs nothing but its header.

#### 3.9.5 Building: Scratch Buffer, Checkpoints, Truncation

- **One scratch buffer per worker** (Yuku). A builder that collects a
  variable list (call arguments, a block's instructions) pushes onto the
  scratch buffer, then flushes the range into `extra` when the list is
  done and truncates the scratch back to its checkpoint. Nested lists nest
  checkpoints.
- **Rollback is truncation.** Every column is append-only while a body is
  checked, so a checkpoint is a tuple of lengths: instructions, `extra`,
  scratch, locals, captures, side tables, the local pool, the inference
  trail, buffered diagnostics, obligations and facts. Undoing a trial
  truncates them all and pops the trail. The parser's speculative paths
  and the checker's candidate trials use this one protocol, which
  replaces the prototype's two rollback mechanisms with different
  guarantees (CA-05, §3.10.4).
- The parser's event vector is the same pattern: recovery truncates the
  events of a failed attempt.
- **Writes after the fact** are allowed in three places only, each a
  fixed-size word or a column written once: the final type sweep
  rewrites `ty`; `finish` fills the capture-mode column from the
  checker's solution (§3.18, type-checking.md §17 item 8); M3 fills the
  provider lists of calls to private callees whose rows were omitted
  (§4.13.1). None moves an instruction.

```rust
#[derive(Copy, Clone)]
pub struct TirCheckpoint {          // the builder's part; the checker adds its own (type-checking.md §3.5)
    insts: u32, extra: u32, scratch: u32, locals: u32, captures: u32,
    subs: u32, side_susp: u32, side_origin: u32, side_hole: u32, local_pool: u32,
}
const _: () = assert!(core::mem::size_of::<TirCheckpoint>() == 40);
```

#### 3.9.6 One Schema, Generated Accessors

Each IR and each wire form has one schema file, and `cargo xtask
codegen` generates the tag enums, size asserts, builders, typed views,
verifiers, printers and wire codecs from it. The schema language, the
generated output and what it costs are in §3.25.

#### 3.9.7 Byte Budgets And Linear Passes

| Item | Size | Fields |
| --- | --- | --- |
| token | 9 B + 1 bit | kind 1, start 4, end 4; line-first bit (§3.11) |
| physical line | 10 B | start 4, first token 4, indent 2 (§3.11) |
| syntax node | 14 B | kind 2, first token 4, last token 4, subtree length 4 (§3.13) |
| item index entry | 13 B | node or record 4, parent 4, name 4, kind 1 (§3.14) |
| path node | 33 B | parent 4, segment 4, kind 1, hash 16, module 4, location 4 (§3.2) |
| pool item | 9 B + `extra` | about 20 B per type on average (§3.9.2) |
| TIR instruction | 17 B + `extra` | tag 1, data 8, type 4, syntax node 4; budget 24 B with `extra` (§3.18) |
| TIR local | 13 B | type 4, name 4, syntax node 4, flags 1 |
| task node | 48 B | §3.21 |
| relocation | 9 B | offset 4, kind 1, target 4 (§3.22) |

CI measures the averages on std and the generated packages and fails on
a regression past the budget, as the size asserts do for a single item.

Passes that are one linear scan over columns: lexing; tree building from
events; skeleton extraction; TIR's final type sweep (resolve every `ty`,
one column); collecting calls for init summaries and fact records (a scan
of `tags`); TIR hashing and serialization (column copies plus a remap of
ID words, §3.20.2); collection's call scan per instance (§13.2); liveness
at suspension points (one backward scan per block); relocation patching;
interface blob writing. Emission walks block lists in order: a forward
scan with a stack of open blocks.

#### 3.9.8 What It Buys: Locality And Memory

- **Locality.** A pass touches only the columns it reads. The final type
  sweep reads `ty` and the local pool; the call scan reads `tags`. A
  column of 4-byte items puts 16 to a cache line, where a tree of boxed
  enums would put one node per line or less.
- **Memory.** §3.24 gives each structure's bytes and the peaks for a
  check and a build. A warm check after a one-function edit holds one
  module's tree and TIR, under a megabyte, beside the `hd` binary's own
  pages and the pool.
- **No per-thread copies.** Interfaces are frozen and shared; the pool is
  shared with per-thread appends; each worker has one set of body
  columns, reused body after body. Parallel checking therefore adds
  little memory, which the `parallel-speedup` exit test bounds at 1.5x
  the memory of one thread (§9).
- **Serialization is copying plus a remap.** Interface blobs, `tir`
  entries and code entries are columns written as position-independent
  little-endian bytes. Columns of plain numbers are copied; columns of
  run IDs go through the entry's tables (§3.20.2). Reading maps the file
  and casts (Yuku's memcpy property, and Vx's `bytemuck` cast).

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
| folder interface | every public signature as pool-encoded types; re-exports followed to the real declaration; impl heads; templates with resolution tables; per-item hashes | bodies, private items (except hidden template items), fact values, doc text, layouts |
| TIR | every name as a `DefId` or a local; every type; every callee (static, trait method with its bound, impl or builtin choice, trait value, function value); every coercion as an instruction; operators on primitives as primitive instructions and all others as trait calls; indexing as calls; pipe, interpolation, string prefixes, literal suffixes, compound assignment, comprehensions and `?` desugared; evaluation order as instruction order; default arguments; named arguments mapped to parameter slots; `for` over std ranges, lists and maps as dedicated loops, and every other `for` desugared to the protocol; match decision trees as switch instructions; GADT refinements; cleanup scopes with their `defer` suites; suspension points; hook points; providers per call; closure captures with their sharing mode; structured control flow | layouts; boxing; capture cells; vtables and dictionaries; frame layouts and state-machine dispatch; overflow and bounds check sequences (a profile choice); instantiation |
| Wasm code entry | instructions, locals, Wasm types as canonical descriptors | indices (symbolic relocations), folding, section layout |

Everything in TIR's right column is decided during emission, per instance
and per tier (§12, §14, §15). Nothing in it needs a second IR.

#### 3.10.2 Lifetimes, Sizes And Peak Memory

Estimates for typical hd code: about 35 bytes, 8 tokens, 10 syntax nodes
and 7 TIR instructions per source line. §3.24 sums them per command.

| IR | Lifetime | Bytes per source line |
| --- | --- | --- |
| tokens and line tables | with their file's tree | about 85 |
| syntax tree | from parse to its module's `ModuleFinish`, or to the end of `hd fmt` or `hd fix` | about 145 |
| item index | with its module scope | under 5 |
| interface | memory-mapped for the run; shared by every process that maps it | about 40 per public line; 0 for private code |
| TIR | from a body's check to its module's `ModuleFinish`, which writes the `tir` entry and frees it; emission maps the entry | about 180 in memory; about 210 on disk |
| Wasm code entries | from emission to the link that reads them; mapped from the cache | about 40 |
| linked module | the link and the precompile | about 30 per reachable line |

**A check never keeps TIR**: it writes the `tir` entry and drops it, so
a later build reuses it. Every finished module's tree and TIR are freed
at `ModuleFinish`. The peaks are in §3.24: about 13 MB for a warm check
and about 22 MB for a cold one, inside the `resources` target of 50 MB.

**A build** adds collection, emission (one instance at a time per worker,
reading mapped `tir` entries), the linked module and Cranelift's compile
memory, which dominates. No monomorphized IR exists at any point. The
`resources` metric records build peaks; it sets no target for them.

#### 3.10.3 Invariants And Verifiers

Each verifier runs in the compiler's debug builds, in CI on std and the
conformance suite, and in verify mode (§5.6). A failure is an internal
error naming the item, never a user diagnostic. The structural part of
each verifier is generated from the schema (§3.25).

| IR | Verifier checks |
| --- | --- |
| tokens and tree | text round-trips byte for byte; token ranges increase; subtree sizes nest; every layout token sits between real tokens |
| item index | every entry names a header node or an interface record; parents precede children; every synthetic impl names its opt-in |
| folder interface | validate on write and decode-compare (§4.10, step 8); on load, the bounds checks of §3.16 |
| InternPool | an item's `extra` offsets lie in bounds; a global item mentions no local index; `meta` equals the value recomputed from the children; hash-consing holds (re-interning an item returns its index) |
| TIR | §4.13.11: operands precede users; types are global after the sweep; every operand's type equals what its position expects; control-flow targets enclose their exits; cleanup and suspension rules hold; every capture has a mode |
| cache entries | header, checksum and section bounds on every read; in verify mode, decode and compare with a recompute (§5.6) |
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
| CS-02: diagnostic registries keyed by file path and offsets, without compilation identity | diagnostics carry a `RootKey` over `DefId`s (§3.8); origins of generated code are a TIR side table per body, so no registry outlives its body |
| F-626: speculation clones checker state | the trail and truncation (§3.9.5) |

### 3.11 Tokens And Line Tables

**Purpose and consumers.** The lexer writes tokens as the parser pulls
them. The parser reads `kind` one token at a time with one token of
lookahead; the layout cursor reads `kind`, the line-first bit and the
line table; the tree, the formatter and diagnostics read `start` and
`end`. Skim mode stores no tokens (§3.12).

**Layout.**

```rust
pub struct TokenBuf {
    pub kind:  Vec<TokenKind>,        // 1 B; significant tokens only
    pub start: Vec<u32>,              // 4 B; byte offset
    pub end:   Vec<u32>,              // 4 B; exclusive
    pub line_first: BitVec,           // 1 bit: the first token on its physical line
    // physical lines
    pub line_start: Vec<u32>,         // 4 B; byte offset of each line
    pub line_tok:   Vec<TokenIdx>,    // 4 B; first token on the line, or NONE for a blank or comment-only line
    pub line_indent: Vec<u16>,        // 2 B; column of the first non-blank byte
    pub line_flags: Vec<u8>,          // 1 B: NON_ASCII | HAS_TAB | CONTINUES_STRING
    // comments, in order
    pub com_start: Vec<u32>, pub com_end: Vec<u32>, pub com_kind: Vec<CommentKind>, // Plain | Doc | ModuleDoc
    // decoded values, only for tokens whose text needs work (escapes, NFC, numeric parse)
    pub decoded: Vec<(TokenIdx, Index)>,   // sorted by token; a pool constant
}
#[repr(u8)] pub enum TokenKind { /* numbered by class: ranges for operators, keywords, expression starts (§4.1) */ }
const _: () = assert!(core::mem::size_of::<TokenKind>() == 1);
```

- **Why `start` and `end` (not Zig's start only).** Zig recomputes a
  token's end by re-lexing it. hd's string frames and interpolation make
  re-lexing a single token depend on the frame stack, and diagnostics,
  the formatter and node spans read `end` constantly. 4 bytes per token
  is the price of a span in two loads.
- **D1's `indent` and `depth` columns are gone (mine).** Indentation is
  a fact of a line, not of a token, so it is a line column. Bracket depth
  is tracked by the layout cursor as it walks, the only reader.
- **Token kinds** number the classes so that "operator", "keyword" and
  "starts an expression" are range tests, and a 256-entry static table
  gives each kind's precedence and flags in one load (Yuku's packed tag,
  kept in a side table here because the kind is one byte).

**Identity.** `TokenIdx(u32)` per file; `NONE` for "no token". Layout
tokens are not in `TokenBuf`; the tree holds them (§3.13).

**Lifetime and owner.** The file arena; built by the `Parse(file)` task
and freed with the tree.

**Growth.** `reserve(source_len / 4)` tokens and `source_len / 30`
lines before lexing (Yuku's estimate, adjusted for hd's longer tokens);
the vectors grow by doubling past that.

**Determinism.** Pure function of the bytes.

**Wire form.** Part of the tree's wire form (§3.13).

**Memory.** 9 bytes per token plus 11 bytes per line plus 9 bytes per
comment: about 85 bytes per source line.

**Accessors.** `toks.kind(i)`, `toks.span(i) -> (u32, u32)`,
`toks.text(i, src) -> &str`, `toks.line_of(offset)` by binary search over
`line_start`, `toks.value(i) -> Index` that decodes on first call.

### 3.12 Layout Cursor And Skim State

**Purpose and consumers.** The layout cursor turns tokens into tokens
plus virtual layout tokens for the parser (§4.2). Skim mode (§4.3) runs
the same lexer frame machine without storing tokens to find a body's
end.

**Layout.** All three are small stacks, owned by one parse:

```rust
pub struct LayoutCursor<'t> {
    toks: &'t TokenBuf,
    pos: TokenIdx,
    depth: u16,                          // bracket depth, tracked here (§3.11)
    indents: SmallVec<[u16; 16]>,        // depth-zero indentation stack
    suites: SmallVec<[OpenSuite; 8]>,    // 8 B each
    pending: SmallVec<[Layout; 4]>,      // virtual tokens due before `pos`
}
#[repr(C)] pub struct OpenSuite {
    kind: SuiteKind,           // u8: Indented | SameLine
    closure: bool,             // the lex.closure.* end rules apply
    delim_depth: u16,          // bracket depth at the colon
    reference_indent: u16,     // lex.nested.reference
    statement_indent: u16,     // lex.nested.body-depth
}
const _: () = assert!(core::mem::size_of::<OpenSuite>() == 8);

pub struct LexFrames { stack: SmallVec<[Frame; 8]> }   // shared by full lexing and skim mode
pub enum Frame { Code { bracket_depth: u16 }, Str { delimiter: u8, raw: bool, prefix: bool } }
```

**Identity, lifetime, growth.** No IDs. The stacks live on the parsing
thread's stack; past 16 indentation levels or 8 suites they spill to the
heap, bounded by the nesting limit (§4.15).

**Determinism.** Pure function of the tokens.

**Wire form.** None. Its decisions are recorded as layout tokens in the
tree.

**Memory.** Under 1 KB per parse.

**Accessors.** `peek`, `bump`, `open_suite` (§4.2).

### 3.13 The Green Tree, Its Wire Format And The JS Decoder

**Purpose and consumers.** The tree is the full syntax of one file. The
header pass and the checker read it through typed views (`FnDecl`,
`MatchExpr`); the formatter walks it in preorder with tokens and layout
tokens interleaved; fix-its and `hd doc` find nodes by offset; the
playground's JavaScript reads it from the wire form.

**Layout.**

```rust
pub struct GreenTree {
    kind:        Vec<SyntaxKind>,   // 2 B
    first_token: Vec<TokenIdx>,     // 4 B
    last_token:  Vec<TokenIdx>,     // 4 B; inclusive; first > last for an empty node
    subtree_len: Vec<u32>,          // 4 B; preorder: node i's subtree is i .. i + subtree_len[i]
    // zero-width layout tokens, in order
    layout_at:   Vec<TokenIdx>,     // 4 B; the token before which it sits
    layout_kind: Vec<Layout>,       // 1 B: Newline | Indent | Dedent | SuiteEnd
    // parse errors, in order
    err_node:    Vec<NodeIdx>, err_code: Vec<Code>,
}
#[repr(transparent)] pub struct SyntaxKind(u16);
const _: () = assert!(core::mem::size_of::<SyntaxKind>() == 2);
```

- **Preorder with subtree sizes.** A child walk is a loop: the first
  child is `i + 1`, the next sibling `j + subtree_len[j]`. Skipping a
  subtree is one addition. A node's tokens not covered by its children
  are its own tokens, so tokens need no node column.
- **Parent links** are built on demand into a `Vec<NodeIdx>` for the
  formatter and fix-its, in one preorder pass.
- **Recovery by truncation.** The parser's event vector is append-only
  and truncated to a checkpoint after a failed attempt (§3.9.5). The tree
  is built from the events in one pass after parsing.

**Identity.** `NodeIdx(u32)` per file, `NONE` for absent. A node's
identity across parses is its `(kind, first token start)`, which is what
fix-its and the program database use.

**Lifetime and owner.** The file arena (§3.5). A skimmed file's header
tree is kept in its skeleton (§3.14).

**Growth.** Events and nodes `reserve(tokens * 5 / 4)`; layout tokens
`reserve(lines)`.

**Determinism.** Pure function of the bytes.

**Wire format.** One buffer, little-endian, every section 8-byte
aligned, no pointers, sent from the compiler worker to the page with one
copy into an `ArrayBuffer`:

```text
offset  size  field
0       4     magic "HDST"
4       2     format number (bumped on any change), 2 flags (HAS_SOURCE)
8       4×12  section table: byte offset of each section below, in order
56      ...   sections
  tok_kind    u8  × ntok        (padded to 8)
  tok_start   u32 × ntok
  tok_end     u32 × ntok
  line_first  u32 × ceil(ntok/32)
  line_start  u32 × nline
  line_utf16  u32 × nline       UTF-16 offset of each line start, for JS columns
  line_flags  u8  × nline       (padded)
  com         (u32, u32, u8 → padded u32) × ncom
  node_kind   u16 × nnode       (padded)
  node_first  u32 × nnode
  node_last   u32 × nnode
  node_len    u32 × nnode
  layout      (u32 at, u32 kind) × nlayout
  errors      (u32 node, u32 code) × nerr
  source      u8 × nbytes       only with HAS_SOURCE
```

- Counts are in the section table's derived lengths; a decoder checks
  that every section lies inside the buffer and is aligned, once.
- **UTF-16 columns (after Yuku).** JavaScript indexes strings in UTF-16.
  `line_utf16` and the `NON_ASCII` line flag let the decoder convert a
  byte offset to a UTF-16 offset with one subtraction on ASCII lines and
  a short scan on other lines.
- **Versioning.** The format number is generated from the schema's hash
  (§3.25); the JS decoder is generated in the same commit, so they never
  disagree. A mismatch throws.

**The generated JavaScript decoder.**

```js
// generated by `cargo xtask codegen` from hd.ungram and tree.ir; do not edit
export class Tree {
  constructor(buf) {
    const u32 = new Uint32Array(buf, 0, 14);
    if (u32[0] !== 0x54534448) throw new Error("not an hd tree");    // "HDST"
    if ((u32[1] & 0xffff) !== FORMAT) throw new Error("tree format mismatch");
    const off = (k) => u32[2 + k];
    this.kind  = new Uint16Array(buf, off(8), this.nnode = (off(9) - off(8)) >> 1);
    this.first = new Uint32Array(buf, off(9), this.nnode);
    this.last  = new Uint32Array(buf, off(10), this.nnode);
    this.len   = new Uint32Array(buf, off(11), this.nnode);
    // ... tokens, lines, layout, errors the same way
  }
  *children(i) { for (let c = i + 1, end = i + this.len[i]; c < end; c += this.len[c]) yield c; }
}
// one class per ungrammar node, with named accessors, as in Rust:
export class FnDecl {
  constructor(tree, i) { this.tree = tree; this.i = i; }
  get name()   { return this.tree.childOfKind(this.i, K.NAME); }
  get params() { return this.tree.childOfKind(this.i, K.PARAM_LIST); }
  get body()   { return this.tree.childOfKind(this.i, K.BLOCK); }
}
```

No objects are built per node. The page creates a view object only for
the node it inspects, as Yuku's decoder does.

**Memory.** 14 bytes per node, about 10 nodes per line, plus layout
tokens: about 145 bytes per line. The wire form adds 4 bytes per line
for `line_utf16` and the source when sent.

**Accessors.** Rust: `tree.node(i) -> NodeRef`, generated `FnDecl::cast
(node)`, `fn_decl.params()`, `node.tokens()`, `node.span()`. Agents see
named accessors only; the raw columns are private to `hd_syntax`.

### 3.14 The Header Skeleton And The Item Index

**Purpose and consumers.** The skeleton is the header pass's output for
one file: its uses, its header tree, its items and two hashes. The item
index of a module is the union of its files' skeleton items, or, for a
module known only through its folder's blob, the blob's item records.
Resolution, `ModulePrep` and the checker look items up by name and walk
them in source order.

**Layout.** One structure for both (mine): the skeleton's item columns
are the item index, so nothing is copied between them.

```rust
pub struct Skeleton {
    pub source_hash: Hash128,
    pub api_text_hash: Hash128,          // §4.5
    pub tree: GreenTree,                 // headers, with SKIPPED_BODY nodes; or the full tree
    pub items: ItemIndex,
    pub uses: UseTable,
    pub doc: Vec<(u32, u32)>,            // doc comment ranges; the text stays in the source
    pub broken: bool,                    // some header failed to parse
}
pub struct ItemIndex {                   // ItemIdx = source order
    pub at:     Col<u32>,                // 4 B: header NodeIdx, or an interface item record (bit 31 set)
    pub parent: Col<ItemIdx>,            // 4 B: NONE at top level; methods, variants, fields
    pub name:   Col<Symbol>,             // 4 B: NONE for impls and statements
    pub kind:   Col<ItemKind>,           // 1 B: Fn | Data | Enum | Trait | Impl | Alias | Let | Stmt | Field | Variant | Member | Test
    pub flags:  Col<ItemFlags>,          // 1 B: PUB | RESULT_OMITTED | ROW_OMITTED | BROKEN | SYNTHETIC | TEST_ONLY
    pub by_name: HashTable<u32>,         // index: Symbol -> first top-level ItemIdx
}
pub struct UseTable {                    // one row per `use` line or group member
    pub segs:  Col<Range32>,             // into `seg_syms`
    pub seg_syms: Col<Symbol>,
    pub alias: Col<Symbol>,              // NONE when absent
    pub flags: Col<u8>,                  // PUB | TEST_ONLY | GROUP | GLOB
    pub node:  Col<NodeIdx>,
}
```

- `name` and `kind` are copied from the tree on purpose: name lookup and
  "all functions" filters run before any view decode, and the two columns
  cost 5 bytes per item. Everything else (visibility details, generics,
  spans) is read through the typed views or the interface reader.
- Derived impls and error impls are `SYNTHETIC` items in the module of
  their declaration
  ([`trait.own.module.generated`](../../spec/lang/09-traits.md#r-trait.own.module.generated)),
  whose `at` is the `@derive` node.

**Identity.** `ItemIdx(u32)` per module. An item's `DefId` is interned
from the module's `PathId` and the item's segment when resolution first
needs it, and stored in the module scope's `def` column (§3.15).

**Lifetime and owner.** Built by `Skim(file)` or `Parse(file)`; frozen
in the file's slot; freed with the module scope at the end of the run, or
with the tree for parsed files.

**Growth.** Exact-size columns (`Col`), built from scratch vectors at the
end of the header pass.

**Determinism.** Source order. `by_name` is an index only.

**Wire form.** The skeleton itself is not cached. Its hashes and the
`uses` paths are in the stat manifest (§3.20.3); its public content is in
the folder blob.

**Memory.** 14 bytes per item plus 9 per use: under 5 bytes per source
line. A skimmed file's header tree is about 15% of its full tree.

**Accessors.** `items.iter_top()`, `items.lookup(sym)`, `items.children
(i)`, `items.view(i) -> ItemView` (a typed view over the tree node or the
blob record, the same enum either way).

### 3.15 Name-Resolution Tables

**Purpose and consumers.** A module scope answers "what does this name
mean here" for headers and bodies. The folder's export index answers it
for uses from other folders. `ModulePrep` builds scopes; the checker
reads them on every unqualified name; the use worklist reads and writes
the folder's pending exports while it runs.

**Layout.**

```rust
pub struct ModuleScope {                 // frozen after ModulePrep
    pub module: ModuleId,
    // one row per name visible at module level
    pub name:    Col<Symbol>,
    pub binding: Col<Binding>,           // 8 B
    pub origin:  Col<u8>,                // Own | Use | PubUse | Prelude
    pub use_row: Col<u32>,               // the UseTable row that brought it, or NONE
    pub index:   HashTable<u32>,         // Symbol -> row
    // per item of the module, parallel to its ItemIndex
    pub def:     Col<DefId>,
    pub local_impls: LocalImpls,         // §3.17
}
#[repr(C)] pub struct Binding { pub kind: BindingKind /* u8 */, _pad: [u8; 3], pub value: u32 }
pub enum BindingKind { Item, Module, Prelude, Ambiguous /* value: range of candidates */, Poison }
const _: () = assert!(core::mem::size_of::<Binding>() == 8);

pub struct FolderExports {               // the use worklist's state; frozen into the blob's exports section
    pub module: Col<ModuleId>, pub name: Col<Symbol>,
    pub target: Col<DefId>,              // the real declaration, chains followed
    pub state:  Col<u8>,                 // Resolved | Pending | Visiting | Loop
    pub index:  HashTable<u32>,          // (module, name) -> row
}
```

- **Body scopes are not here.** Local names live in the checker's scope
  stack (§3.19).
- **Ambiguity** keeps its candidates in a side column, so the error lists
  them in content order.
- The module scope's `def` column is the bridge from `ItemIdx` to
  `DefId`: one load.

**Identity, lifetime.** Rows are module-local `u32`s. `ModuleScope` is
written once into its module's `OnceLock` slot and lives for the run.
`FolderExports` lives during the folder task, then its rows are written
into the blob's exports section sorted by bytes.

**Growth.** Built in scratch vectors, then frozen to `Col`s of exact
size.

**Determinism.** Rows in (origin, source order). The worklist runs in
(module path, source order), so the loop errors are the same on every
run (§4.9).

**Wire form.** None for scopes; exports go into the blob (§3.16).

**Memory.** 21 bytes per visible name plus 8 of index; a module with 100
names and 50 uses costs about 4 KB.

**Accessors.** `scope.lookup(sym) -> Option<Binding>`, `scope.def(item)`,
`exports.resolve(module, sym)`.

### 3.16 The Folder Interface: In Memory And As A Blob

**Purpose and consumers.** A folder's interface is everything a
dependent's check can read: public items and their signatures, impl
heads, templates, hidden template items, per-item hashes. Resolution
reads exports by name; the checker reads signatures, fields and variants
by item; the solver reads impl heads through impl tables (§3.17);
coherence reads the `heads` section; `hd doc` reads items in order; D2
reads layouts of public `data` types. Access is by name or item, a few
records per use.

**In memory the interface is its blob.**

```rust
pub struct FolderIface {
    pub folder: FolderId,
    pub blob: Blob,                          // mapped from the cache, embedded (std), or owned (just built)
    pub api_hash: Hash128, pub deep_hash: Hash128, pub heads_hash: Hash128,
    // run memo: blob-local index -> run ID, filled on first use, first writer wins
    ty_memo:   Box<[AtomicU32]>,             // per type record -> Index, NONE until interned
    path_memo: Box<[AtomicU32]>,             // per path record -> PathId
    sym_memo:  Box<[AtomicU32]>,             // per string -> Symbol
    pub impl_tables: Box<[(ModuleId, ImplTable)]>,   // §3.17
}
pub enum Blob { Mapped(memmap2::Mmap), Static(&'static [u8]), Owned(Box<[u8]>) }
```

- **Lazy decode.** `reader.item(name)` binary-searches the exports
  section by name bytes and decodes one record. `reader.ty(r)` interns
  type record `r` into the pool on first use: it interns its children
  first (records are written children first, so this is a loop, not
  recursion), then stores the `Index` in `ty_memo[r]`.
- **Racing readers are harmless.** Two body tasks that intern the same
  record get the same `Index`, because the pool hash-conses. Both store
  it; the second store writes the same value.

**Blob layout.** The cache entry container of §3.20.1 with kind `iface`.
All sections are little-endian, 8-byte aligned, and referenced by section
index; every offset inside a section is relative to the section.

| Section | Rows | Row layout (bytes) |
| --- | --- | --- |
| `strings` | one per distinct string, sorted by bytes | `off u32, len u32` (8) plus a bytes area |
| `paths` | one per stable path the blob names, parents first | `parent u32, seg u32, kind u8, pad 3` (12); `seg` is a string index or an `impl_segs` row |
| `impl_segs` | one per impl path segment | `head Hash128, ordinal u16, pad 6` (24) |
| `path_hash` | parallel to `paths` | `Hash128` (16); checked against the run's trie on re-intern |
| `ty_tag`, `ty_data`, `ty_extra` | one per type, list or row, children first | the pool encoding of §3.9.2 with blob-local operands: `DefId` as a path row, `Symbol` as a string row, `Ty` as a type row (1 + 4 + words) |
| `items` | public and hidden items, in (module path, source order) | `path u32, kind u8, vis u8, flags u16, module u32, sig u32, generics Range32, members Range32, span_lo u32, span_hi u32` (40) |
| `generics` | generic parameters | `name u32, variance u8, flags u8, index u16, bounds Range32, default u32` (20) |
| `bounds` | bounds of parameters, impls and supertraits | `trait_ty u32` (4): a `TraitValue` type row |
| `members` | fields, variants, trait members, associated types | `name u32, kind u8, flags u8, pad 2, ty u32, payload Range32, result u32, default_tokens Range32` (32) |
| `exports` | (module, name) -> item, sorted by (module path bytes, name bytes) | `module u32, name u32, item u32` (12) |
| `impls` | every impl head of the folder | `path u32, trait u32, self_ty u32, args u32, generics Range32, bounds Range32, owner u32, by_member u32, head_key u32, flags u32` (48); `flags` has `API` (in `api_hash`) or heads-only |
| `templates` | one per template | `impl u32, tokens Range32, resolve Range32` (20) plus the token text bytes and a resolution table of `(token offset u32, path u32)` |
| `item_hash` | parallel to `items` | `shallow Hash128, deep Hash128` (32) |
| `mentions` | other folders the api sections name | `folder path u32, deep Hash128` (20) |
| `diags` | header diagnostics | the `DiagBuf` wire columns (§3.8) |

- **Fixed-width rows, ranges for variable parts.** No row has a length
  field inside it. A reader computes a row's address as `base + i *
  width`, after one bounds and alignment check per section.
- **The type section is the pool's encoding (mine).** D1's 16-byte
  `TypeRecord { tag, flags, a, b, c }` is replaced by the pool's tag,
  data and `extra` columns with blob-local operands. The schema generates
  both, the remap between them is mechanical (§3.20.2), and one verifier
  covers both.
- **Canonical bytes.** Rows are written in content order: strings sorted
  by bytes, paths and types in first-use order of a walk over items in
  (module path, source order), exports sorted by bytes. Never by run IDs
  or hash-map order. Two runs on the same inputs write the same bytes;
  the determinism matrix compares them (§8.1).
- **Std's blobs** are one pack embedded in the binary: a table of
  `(folder path, offset, length)` followed by the blobs, each 8-byte
  aligned, read as `Blob::Static` with no copy.

**Identity.** Blob-local rows are `u32` indices; `NONE` for absent. The
run IDs they map to are memoized in the three memo arrays.

**Lifetime and owner.** One `FolderIface` per folder in a `OnceLock`
slot for the run. Mapped blobs stay mapped until the process exits; the
memo arrays are the only per-run allocation (4 bytes per record).

**Growth.** The writer sizes every section exactly in a counting pass,
then writes once into one `Vec<u8>`.

**Determinism.** As above: canonical bytes, and the three hashes over
them (§4.11.2, §4.11.3).

**Validation on load.** The container header and checksum (§3.20.1);
each section in bounds and aligned; the toolchain key equal to the run's.
Row contents are checked lazily: every index a row holds is checked
against its section's row count when the accessor decodes it, and an
out-of-range index is an internal error that names the blob and drops it
as a cache miss. Verify mode runs the full verifier on every blob.

**Memory.** About 40 bytes of blob per public source line, mapped and
shared across processes, plus 12 bytes of memo per type, path and string
record when the blob is opened. A 10k-line package with 3,000 public
lines has about 120 KB of blobs; the std pack is about 1 MB in the
binary, of which a check touches a few hundred KB of pages.

**Accessors.** Generated from `iface.ir`: `reader.item(i) -> ItemRec`,
`rec.generics() -> impl Iterator<Item = GenericRec>`, `reader.ty(rec.sig)
-> Ty`, `reader.def(rec.path) -> DefId`, `reader.export(module, name)`.
The builder side is `IfaceWriter` with one `push_*` per section. Agents
never compute an offset.

### 3.17 Impl Tables

**Purpose and consumers.** The solver asks for the impls of one trait in
one owner module, fast-rejected by the head's outer constructor
(§4.12.1). Coherence asks for all heads of one trait across folders.
Method lookup asks for the inherent impls of one type.

**Layout.** An index over the blob's `impls` section, built once when
the folder interface is frozen or opened:

```rust
pub struct ImplTable {                   // per module; rows sorted by (trait path hash, head key, source order)
    pub trait_:   Col<DefId>,            // 4 B; NONE for inherent impls
    pub head_key: Col<HeadKey>,          // 4 B; packed: kind in bits 29..31
    pub rec:      Col<u32>,              // 4 B; the blob's impls row
    pub generic:  BitBox,                // 1 bit: the head has type parameters
    pub by_trait: HashTable<u32>,        // index: trait DefId -> first row; rows of one trait are contiguous
}
#[repr(transparent)] pub struct HeadKey(u32);   // Ctor(DefId) | Prim(Prim) | Tuple(arity) | Fn | Param
pub struct LocalImpls { /* the same columns, for a body-visible local impl table of one module (§4.12.1) */ }
```

- **Sorting by the trait's path hash**, not by `DefId`: the hash is
  content, so the row order and therefore candidate order are the same
  on every run, and the solver returns candidates in content order
  (type-checking.md §12).
- Head keys pack the kind into the top 3 bits and a `DefId`, `Prim` or
  arity into the low 29. A `DefId` above 2^29 is an internal error; the
  path table's per-owner capacity keeps it far below.
- **Compiler-supplied impls** (tuples at every arity, numeric families)
  have no rows. The solver answers them with `Evidence::Builtin`, and TIR
  records a `Builtin` choice (§3.18).

**Identity, lifetime.** Rows are module-local. The table is frozen with
its folder and lives for the run.

**Growth.** Exact size, built from the blob section in one pass and one
sort.

**Determinism.** Content order as above.

**Wire form.** None: the `impls` section is the persistent form.

**Memory.** 12 bytes plus a bit per impl, plus the index. Std has about
2,000 impls: about 40 KB when every std folder is opened.

**Accessors.** `table.for_trait(def) -> &[row]`, `table.head(row) ->
ImplRec` through the blob reader.

### 3.18 TIR

The full encoding, instruction catalog and builder are in
[§4.13.11](checking-and-tir.md#41311-the-typed-ir-tir). This section is
the structure's design card.

**Purpose and consumers.** One typed IR per body (§3.9.1). The checker
writes it through the builder only. `ModuleFinish` fills pending
providers and writes it. Collection scans `tags` per instance for calls,
closures, coercions and host calls. Emission walks block lists in order
under a substitution.

**Layout.** Columns, one set per worker while a body is built, then one
exact-size copy per body in the module result:

| Column | Item | Bytes | Read by |
| --- | --- | --- | --- |
| `tags` | `TirTag` | 1 | everyone; collection scans it alone |
| `data` | `[u32; 2]` | 8 | emission, collection for calls |
| `ty` | `Ty` | 4 | the type sweep, emission |
| `syn` | `NodeIdx` | 4 | diagnostics; written to disk as spans |
| `extra` | `u32` | 4 each | operand lists, callee and coercion records |
| locals: `local_ty`, `local_name`, `local_syn`, `local_flags` | | 13 per local | emission (Wasm locals), the program database |
| subs: `sub_root`, `sub_params`, `sub_parent`, `sub_flags` | | 17 per sub-body | emission of closures |
| captures: `cap_local`, `cap_mode` | | 5 per capture | emission of closures |
| side: suspension points, origins, hole candidates | sorted by `Inst` | 12 to 16 per row | suspension lowering, tools |

**Identity.** `Inst`, `LocalId`, `SubId`, `CaptureId` are body-local
`u32`s. A `Ref` is an `Inst` below 2^31, or a global pool constant with
bit 31 set, so constants need no instruction and no column (mine).

**Lifetime and owner.** The worker's columns, truncated to empty at each
body's start; the body's copy in the module result until `ModuleFinish`
writes the `tir` entry; then freed. Emission maps the entry.

**Growth and scratch.** Worker columns keep their capacity, so after the
first few bodies a worker allocates only the per-body output copy.
Variable lists go through the scratch buffer (§3.9.5).

**Determinism.** Instructions are in evaluation order, locals and
captures in creation order, side rows in instruction order. The run IDs
inside are remapped on write (§3.20.2), so the entry's bytes and the
per-item TIR hash are run-independent.

**Wire form.** The `tir` entry (§3.20.4).

**Memory.** About 7 instructions per line at 17 bytes plus about 7 bytes
of `extra`, locals and side rows: about 180 bytes per line in memory.

**Accessors.** `tir.view(i) -> TirView` with named fields per tag;
`TirBuilder` (§4.13.11); `tir.blocks()`, `tir.locals()`. Generated from
`tir.ir` (§3.25).

### 3.19 Per-Body Checker Scratch

The algorithms are type-checking.md's; its §13 lists the fields. This
section fixes their storage.

**Purpose and consumers.** Everything a body check mutates, owned by one
`BodyCx` per worker and reused body after body.

**Layout.** Four groups, each columns indexed by a body-local ID:

| Group | Columns (bytes per row) | Row count, typical body |
| --- | --- | --- |
| inference table, by `InferVar` | `parent` 4, `rank` 1, `kind` 1, `value` 4, `blame` 4, `watch_head` 4 (18) | 10 to 500 |
| trail | `undo` 8 (tag 1 + index 4, padded), `old` 4 (12) | cleared at each statement boundary outside trials |
| obligations, by row | `goal` 16, `span` 4, `next_watch` 4, `state` 1 (25) | 0 to 50 |
| locals, by `LocalId` | `name` 4, `ty` 4, `span` 4, `flags` 1 (13); shared with TIR's local columns (one set, written by the builder) | 5 to 200 |
| spans, by `SpanIdx` | `lo` 4, `hi` 4 (8) | one per blamed site |
| context stacks | `scopes` 16, `loops` 12, `fns` 24, `avail` 4, `literal_scope` 4, `assigned` bit stack | depth of nesting |
| outputs | `DiagBuf` (§3.8), `row_facts` 20, `init_facts` 12, `arm_eqs` 8 | few |
| scratch | `scratch_tys`, `scratch_refs`, `scratch_args`, `spine` | cleared per use |

- **One local table (mine).** D1 had local columns in TIR and
  type-checking.md §13 had its own. They hold the same rows, so the
  builder owns one set and the checker reads it; the checker's flags are
  the TIR `local_flags` column, written through the trail.
- **`Goal` at 16 bytes** is a tag plus three `u32` operands (a type, a
  trait, an interned argument list); its size is asserted.
- **Checkpoint.** type-checking.md §3.5's `Checkpoint` holds the trail
  length, the output and obligation lengths, and a `TirCheckpoint`
  (§3.9.5). The local pool's length is in the latter.

**Identity.** All IDs are numbered from 0 per body (§6.5 rule 2).

**Lifetime and owner.** One `BodyCx` per worker, in the worker arena.
Each body truncates every column to zero, so capacity carries over and
no body allocates for scratch after warm-up. M1's depth-first walk keeps
one `BodyCx` per open body on its stack, bounded by the module's item
count.

**Growth.** Doubling vectors that never shrink within a run. A body
whose scratch grew past 4 MB releases the excess at its end (mine), so
one huge body does not pin memory for the rest of the run.

**Determinism.** Per-body numbering; obligations retried in creation
order.

**Wire form.** None.

**Memory.** A typical body uses a few KB; the cap per worker after a huge
body is 4 MB. Eight workers: well under 1 MB typical, 32 MB at most.

**Accessors.** Plain Rust methods on `BodyCx`; the inference table's
columns are private to the unifier module.

### 3.20 Cache Entries And The Manifest

#### 3.20.1 The Entry Container

Every cache entry kind (§5.2) uses one container, so one reader, one
checksum and one verifier serve all of them (mine).

```rust
#[repr(C)]
pub struct EntryHeader {        // 64 bytes, little-endian
    magic: [u8; 4],             // "HDCE"
    kind: u16,                  // EntryKind
    layout: u16,                // the entry kind's format number, from its schema's hash
    key: Hash128,               // the key this entry answers
    toolchain: Hash128,         // toolchain_key (§5.3)
    payload_len: u64,
    checksum: u64,              // xxh3-64 of everything after the header
    nsections: u32,
    flags: u32,                 // HAS_DIAGNOSTICS, ...
}
#[repr(C)]
pub struct SectionEntry { kind: u16, flags: u16, rows: u32, offset: u64 }   // 16 bytes, after the header
const _: () = assert!(core::mem::size_of::<EntryHeader>() == 64);
const _: () = assert!(core::mem::size_of::<SectionEntry>() == 16);
```

- **Validation on load:** magic, kind, layout, key and toolchain equal
  to the expected ones; `payload_len` equal to the file length minus 64;
  checksum; each section in bounds and 8-byte aligned. Any failure is a
  miss, and the file is deleted (§5.4). The checksum costs about 30 µs
  per MB, which is cheaper than one wrong answer.
- **Zero-copy.** Entries over 64 KiB are memory-mapped, smaller ones read
  into one aligned buffer. Sections are cast to `LeCol<T>` (§3.9.4).
- **Endianness.** Little-endian only. Every target host is
  little-endian, and a big-endian build refuses to compile `hd_cache`
  rather than byte-swap.
- **Versioning.** `layout` is generated from the schema of that entry
  kind (§3.25), so any change to a section's rows changes it. The cache
  layout version in `toolchain_key` covers the container itself. Old
  entries are never read, only evicted.

#### 3.20.2 The Wire Rule: Entry-Local Tables (mine)

**No run ID is ever written to disk or hashed.** Pool indices, `DefId`s,
`Symbol`s, `FileId`s and `NodeIdx`s depend on the run. An entry that
holds them is wrong on the next run, and a hash over them changes with
thread timing. So every entry that holds types or names carries:

| Table | Rows | Replaces |
| --- | --- | --- |
| `strings` | sorted distinct strings | `Symbol` |
| `paths` | stable paths, parents first, with their `Hash128` | `DefId`, `PathId`, `FileId` (a file is a path) |
| `types` | the pool encoding over these tables, children first | `Ty`, `TyList`, `RowId`, constants |

- **The writer** walks the columns once. For each word the schema marks
  as an ID kind, it looks the ID up in a per-write memo (`HashTable` from
  run ID to entry row) and appends a new row on a miss. Rows are created
  in first-use order of that walk, which is content order, because the
  columns are already in content order. Row keys in the `types` table are
  re-sorted by their canonical encoding before writing, so a `Row`'s key
  order is content order on disk.
- **The reader** keeps three memo arrays, as the interface does (§3.16),
  and translates a row to a run ID on first use.
- **Spans** are written as `(lo, hi)` byte pairs in the entry's module
  file; a span in another file adds the file's path row.
- **Canonical encoding for hashing.** A per-item hash (TIR hash, item
  interface hash) is xxh3-128 over the item's rows after remapping, with
  the referenced type and path rows hashed by content (their path
  hashes), never by row number. So two entries that list types in a
  different order still give an item the same hash.
- **Cost.** One memo probe per ID word. A `tir` entry of 1,000 lines has
  about 20,000 ID words; at about 10 ns each, 0.2 ms per module.

This rule corrects D2's "serialization is column copies" for TIR: the
`ty` column and ID operands go through the remap; tags, local indices,
instruction references and `extra` words that are not IDs are copied.

#### 3.20.3 The Stat Manifest

```rust
#[repr(C)]
pub struct ManifestRecord {     // 80 bytes; one per file, sorted by path bytes
    path: Range32,              // into the manifest's string area
    size: u64,
    mtime_ns: i64,              // nanoseconds since the epoch; covers years 1678 to 2262
    inode: u64,
    source_hash: Hash128,
    api_text_hash: Hash128,
    uses: Range32,              // into the use-path area: module paths this file's non-test uses reach
    role: u8, _pad: [u8; 7],
}
const _: () = assert!(core::mem::size_of::<ManifestRecord>() == 80);
```

- One file, `build/.hd/manifest`, in the container format of §3.20.1
  with kind `manifest` and key `H(package root path)` (the one key that
  may hold a path, since the file never leaves the worktree).
- **AoS on purpose**: each step of §5.5 reads a whole record.
- Read whole at start (a 100-file package is about 9 KB), written whole
  and atomically at the end when anything changed.

#### 3.20.4 Entry Sections By Kind

| Kind | Sections, beyond `strings`, `paths`, `types` | Typical size, 1,000-line module |
| --- | --- | --- |
| `iface` | §3.16 | 40 B per public line |
| `check` | `diags`; `init_summary` (per function: read bindings, calls, dispatched methods, as path rows); `row_results` (per private callee: solved keys); `facts` (the program-database records); `module_meta` (counts, `poisoned`) | 2 to 20 KB |
| `tir` | `bodies` (per body: item path, kind, column ranges, TIR hash, dependency range: 48 B); `tags`, `data`, `ty`, `span_lo`, `span_hi`, `extra`; `local_*`, `sub_*`, `cap_*`; side tables; `deps` (path row, item interface hash) | about 210 B per line |
| `check-test` | `diags`, `registrations` (name text, body path) | small |
| `coh`, `init`, `pkgres` | `diags` plus a few rows | small |
| `depfiles` | manifest-like records without stat fields | 56 B per file |
| `code` | §3.22 | about 40 B per line per instance |
| `link`, `cwasm` | one `bytes` section | the module |

The `tir` entry stores spans as two columns, not `syn`, so emission
never needs the syntax tree (mine). That is why it is about 30 bytes per
line larger on disk than in memory.

**Memory, for all cache structures.** Entries are mapped or read for the
moment they are needed. A warm check reads the manifest, about 100
`iface` and `check` headers (64 bytes each, plus their `diags`), and
writes one `check` and one `tir` entry.

### 3.21 The Scheduler's Task Graph

**Purpose and consumers.** The driver and finished tasks add tasks and
edges; the executor pops ready tasks; tasks read their dependencies'
result slots (§6.1).

**Layout.** A task is touched as a whole, so it is one struct (§3.9.4):

```rust
#[repr(C, align(16))]
pub struct TaskNode {                      // 48 bytes
    kind: TaskKind,                        // 12 B: tag u8 + two u32 payloads (an ID and an ItemIdx)
    waiting_on: AtomicU32,                 // unfinished dependencies
    state: AtomicU8,                       // Waiting | Ready | Running | Done | Cancelled
    _pad: [u8; 3],
    priority: u32,                         // §6.3
    succ_head: AtomicU32,                  // first edge in `edges`, NONE when empty
    created: u32,                          // creation order, the serial tie-break
    _rest: [u8; 12],
}
pub struct Edge { to: TaskId, next: u32 }  // 8 B, an append-only linked list per task
pub struct TaskGraph {
    nodes: AppendVec<TaskNode>,
    edges: AppendVec<Edge>,                // pushed with a CAS on the source's succ_head
    by_key: [Mutex<HashTable<u32>>; 16],   // (TaskKind) -> TaskId, so a task is created once
}
const _: () = assert!(core::mem::size_of::<TaskNode>() == 48);
```

- **Edges are a lock-free list (mine).** D1 had a `Mutex<SmallVec>` per
  task. An edge is pushed by CAS on `succ_head`; a task marks itself
  `Done` and then walks the list. An edge pushed to a task that is
  already `Done` is counted as satisfied by the pusher, which checks
  `state` after its CAS (§6.1's rule).
- **Result slots** are per-kind `Box<[OnceLock<T>]>` indexed by the
  dense file, folder or module ID, sized at discovery.

**Identity.** `TaskId(u32)`, creation order. `by_key` deduplicates.

**Lifetime.** One graph per run, freed at the end.

**Growth.** `AppendVec`s; a 10k-line package makes about 2,000 tasks
cold and a few hundred warm.

**Determinism.** Task IDs and executor order never reach output (§6.5).
The serial executor's tie-break is `created`.

**Wire form.** None.

**Memory.** 48 bytes per task plus 8 per edge: about 150 KB cold for 10k
lines.

**Accessors.** `graph.add(kind, deps) -> TaskId`, `graph.slot::<K>(id)`.

### 3.22 Codegen: The Instance Table And Code Entries

**Purpose and consumers.** Collection (§13.2) builds the instance set of
one program; emission reads one instance at a time; link reads every
code entry of the set in key order.

**Instance table layout.**

```rust
pub struct InstanceTable {                 // per program; columns by InstId
    item:   Vec<DefId>,                    // 4 B
    sub:    Vec<u16>,                      // 2 B; closure sub-body, 0 for the item itself
    args:   Vec<TyList>,                   // 4 B; pool list of concrete type arguments
    body:   Vec<BodyRef>,                  // 8 B: mapped `tir` entry index, body row
    depth:  Vec<u8>,                       // 1 B; type-argument nesting depth (§13.4)
    parent: Vec<InstId>,                   // 4 B; the requesting instance, for the depth diagnostic
    key:    Vec<Hash128>,                  // 16 B; instance key (§13.3), computed when first pushed
    index:  HashTable<u32>,                // (item, sub, args) -> InstId: dedup with run IDs, no hashing of canon types
    vtables: Vec<(Ty, DefId)>, vt_index: HashTable<u32>,
    imports: Vec<HostMethodId>,            // sorted and deduplicated at the end
    types:   Vec<Ty>, ty_index: HashTable<u32>,
}
```

- **Dedup in run IDs, keys in content (mine).** Collection dedups by
  `(DefId, sub, TyList)`, three `u32` compares, and computes the 128-bit
  instance key only once per new instance. The key is what sorts the
  set and names the code entry.
- At the end, a permutation sorted by `key` gives the emission and link
  order (§6.5); the columns themselves are not moved.

**Code entry layout** (the `code` entry's sections):

| Section | Row | Bytes |
| --- | --- | --- |
| `body` | Wasm locals and instructions; index immediates as 5-byte padded LEBs | bytes |
| `relocs` | `at u32, kind u8, pad 3, target u32` sorted by `at` | 12 on disk, 9 in memory (packed) |
| `targets` | `Hash128` instance keys and host method keys, deduplicated | 16 |
| `wasm_types` | canonical Wasm type descriptors, as a small type table | varies |
| `sites` | `at u32, kind u8, pad 3, span_lo u32, span_hi u32, file u32` | 20 |
| `lines` | `at u32, span_lo u32` | 8 |
| `sig` | the canonical signature, one row | small |

- A relocation's `target` indexes `targets`, `wasm_types`, or the
  entry's global and data symbol rows by its `kind`. So a relocation is
  9 bytes, and an instance key is stored once per entry, not once per
  call site.

**Identity.** `InstId` per program, `NONE` for absent. On disk, instance
keys.

**Lifetime.** The instance table lives for one program's build. Code
entries are written by `Emit` and mapped by `Link`.

**Growth.** Doubling vectors; a test plan collects programs in parallel,
each with its own table.

**Determinism.** Everything output-facing is sorted by instance key.

**Memory.** About 43 bytes per instance plus its index; a 10k-line
program with about 5,000 instances: about 0.3 MB. Code entries: about 40
bytes per line per instance, mapped.

**Accessors.** `set.push(item, sub, args) -> InstId`, `set.sorted()`,
generated `CodeEntryReader` and `CodeEntryWriter`.

### 3.23 Wasm Emission Buffers

**Purpose and consumers.** One `Emit(inst)` walk writes one function's
Wasm and its relocations. `Link` writes the module.

**Layout.** Per worker, reused per instance:

```rust
pub struct EmitBufs {
    code: Vec<u8>,                       // the body being written; wasm-encoder's sink
    relocs: Vec<PackedReloc>,            // 9 B: at u32, kind u8, target u32
    targets: Vec<Hash128>, target_index: HashTable<u32>,
    value_local: Vec<u32>,               // by Inst: the Wasm local holding the value, or NONE
    local_types: Vec<ValType>,           // the function's locals
    blocks: Vec<OpenBlock>,              // the open-block stack, 12 B each
    subst: HashTable<(u32, u32)>,        // generic Ty -> substituted Ty, per instance
    layout: HashTable<(u32, u32)>,       // substituted Ty -> layout class row, per worker (types are global)
    sites: Vec<SiteRow>, lines: Vec<(u32, u32)>,
}
#[repr(C, packed)] pub struct PackedReloc { at: u32, kind: u8, target: u32 }
const _: () = assert!(core::mem::size_of::<PackedReloc>() == 9);
```

- `value_local` is sized to the body's instruction count and filled with
  `NONE` per instance: a direct index, not a map.
- The layout memo is per worker for the whole build, since layouts are a
  function of global types; the substitution memo is per instance.
- **Link** writes into one `Vec<u8>` reserved at the sum of the entries'
  body sizes plus the sections' estimates, and patches relocations in
  place (§13.10).

**Lifetime.** Per worker, truncated per instance; released at the end of
the build.

**Growth.** Doubling; a body past 1 MB releases the excess afterwards,
as §3.19 does.

**Determinism.** Wasm locals are numbered in TIR order; relocation order
is offset order.

**Wire form.** The `code` entry (§3.22).

**Memory.** A few hundred KB per worker. Cranelift's own memory, outside
these buffers, dominates a build.

**Accessors.** `Emitter` methods per TIR tag; the relocation-recording
wrapper of §15.7.

### 3.24 Memory Budget

Assumptions: a generated 10k-line package of 100 files, 20 folders, 10k
lines at 35 bytes, 8 tokens, 10 nodes and 7 TIR instructions per line;
the largest module 1,000 lines; 8 workers; std as an embedded pack. The
binary's resident pages are an estimate that the `startup` metric
measures.

| Structure | Section | Warm check, one-function edit | Cold check | Build (cold, adds) |
| --- | --- | --- | --- | --- |
| `hd` binary pages touched, runtime, allocator | | 8.0 MB | 8.0 MB | +6 MB (Cranelift, wasmtime code) |
| stat manifest | §3.20.3 | 0.01 MB | 0.01 MB | |
| sources in flight | §3.5 | 0.04 MB | 0.3 MB (8 modules) | |
| tokens and line tables | §3.11 | 0.09 MB | 0.7 MB (8 × 1,000 lines) | |
| green trees | §3.13 | 0.15 MB | 1.2 MB | |
| skeletons of skimmed files | §3.14 | 0.3 MB (headers of the rest) | 0 | |
| item index, scopes, exports | §3.14, §3.15 | 0.1 MB | 0.4 MB | |
| strings and symbols | §3.3 | 0.3 MB | 0.5 MB | +0.1 MB |
| stable path trie | §3.2 | 0.3 MB | 0.6 MB | +0.1 MB |
| InternPool | §3.9.2 | 0.3 MB | 1.7 MB | +0.5 MB (instance types) |
| interface blobs and memos | §3.16 | 0.6 MB (std pages and package blobs) | 1.2 MB | |
| impl tables | §3.17 | 0.02 MB | 0.05 MB | |
| solver memo | §4.12.2 | 0.2 MB | 1.0 MB | +0.5 MB |
| TIR in module results | §3.18 | 0.18 MB (one module) | 1.4 MB (8 modules) | |
| checker scratch | §3.19 | 0.5 MB | 0.8 MB | |
| diagnostics | §3.8 | 0.01 MB | 0.05 MB | |
| cache entry write buffers | §3.20 | 0.4 MB | 1.0 MB | |
| task graph | §3.21 | 0.03 MB | 0.15 MB | +0.2 MB |
| instance table | §3.22 | | | +0.3 MB |
| emission buffers | §3.23 | | | +2 MB (8 workers) |
| mapped `tir` and `code` entries | §3.20, §3.22 | | | +2.5 MB |
| linked module and link buffer | §13.10 | | | +0.6 MB |
| Cranelift compile memory | §18 | | | +20 to 40 MB (estimate, 8 workers) |
| allocator slack, about 15% | | 1.7 MB | 2.9 MB | +5 MB |
| **Total** | | **about 13 MB** | **about 22 MB** | **about 60 to 80 MB** |
| **Target** | goals.md | **50 MB** (warm check) | none set | none set |

- **Warm check margin: about 37 MB.** The largest risks are the binary's
  pages, which `startup` measures, and the solver memo, which grows with
  distinct goals and is bounded by the program, not by time.
- **Startup (≤ 10 MB).** Pre-seeded symbols, paths and types are static
  data in the binary; the std pack is mapped lazily. An empty-file check
  touches the binary's pages, one skeleton and a few std blobs: about
  9 MB.
- **CPU (≤ 0.2 CPU-s warm).** The warm path's data work is linear in the
  edited module: lexing and parsing about 35 KB, checking 1,000 lines,
  remapping about 20,000 ID words for the `tir` entry, and reading about
  100 entry headers. The `edit-latency` and `resources` scripts measure
  it.
- **A build** is dominated by Cranelift; this design adds about 6 MB of
  its own structures to a cold check. The `resources` metric records it
  without a target.

### 3.25 The Schema Language

**One definition per structure** generates its Rust layout, its typed
views and builders, its verifier's structural checks, its printer, its
wire codec with the ID remap, and, for the tree, the JavaScript decoder.

**Files.**

| Schema | Generates | Crate |
| --- | --- | --- |
| `hd_syntax/hd.ungram` | syntax kinds, typed views in Rust and JS | `hd_syntax` |
| `hd_syntax/tree.ir` | the tree wire format, its JS decoder | `hd_syntax` |
| `hd_types/pool.ir` | pool tags, `TyView`, `mk`, meta computation, the type-table codec | `hd_types` |
| `hd_iface/iface.ir` | blob sections, `IfaceReader`, `IfaceWriter` | `hd_iface` |
| `hd_tir/tir.ir` | `TirTag`, `TirView`, builder helpers, verifier, printer, `tir` entry codec | `hd_tir` |
| `hd_cache/entries.ir` | the container, section kinds, manifest, `check`, `code` entries | `hd_cache` |
| `hd_diag/codes.ir` | `Code`, message templates and their typed builders | `hd_diag`; read from the spec's code list |

**The language.** A small declarative text format, parsed by the
generator. Operand kinds are its core: each kind names a Rust type, a
verifier check, a printer, and its wire treatment.

```text
# kinds: one line each; `id` kinds are remapped on the wire
kind inst   u32  id=none   check=before_self     print="%{n}"
kind ref    u32  id=const  check=before_self_or_const
kind local  u32  id=none   check=lt(locals)
kind ty     u32  id=type   check=global_after_finish
kind def    u32  id=path
kind sym    u32  id=string
kind block  u32  id=none   check=tag(Block)
kind list<K>     range     # {start, len} into extra, elements of kind K

record Callee  = Item { def: def, targs: list<ty> }
               | TraitMethod { trait: def, method: sym, self_ty: ty, targs: list<ty>, choice: Choice }
               | Evidence { value: ref, bound: u16, method: sym }
record Choice  = Impl { def: def } | Bound { index: u16 } | Builtin { which: BuiltinImpl }

inst Call {
  a: extra Callee
  b: extra { args: list<ref>, providers: Providers }
  ty: "the callee's result, substituted"
  verify: args_match_params, providers_match_row
}
inst LocalSet { a: local, b: ref, ty: void, verify: value_ty_eq_local }
```

- An instruction lists its two data words; `extra` records are named
  records. The generator computes each record's word count and emits a
  size assert per record and per tag.
- `verify:` names hand-written semantic checks; the structural ones
  (operand order, ranges in bounds, tags of referenced instructions,
  every ID word of the right kind) come from the operand kinds alone.
- Tables (interface sections, cache sections) use `table` blocks with
  the same kinds; a `table` row's width is computed, and `aos` marks a
  table stored as one struct (§3.9.4).

**What it generates, per schema.** The `#[repr(u8)]` tag enum; the size
asserts; one `View` enum of small `Copy` structs with named fields, where
lists are slice views; builder functions that take typed arguments; the
structural verifier; a printer for `hd debug tir` and snapshots; the
wire writer and reader with the remap of §3.20.2; the format number,
which is the hash of the schema's normalized text; and for the tree, the
JavaScript decoder.

**What agents see.**

```rust
match tir.view(i) {
    TirView::Call(Call { callee, args, providers }) => {
        for a in args.iter(&tir) { /* a: Ref */ }
        if let CalleeView::TraitMethod { choice: Choice::Builtin { which }, .. } = callee.view(&tir) { .. }
    }
    TirView::LocalSet(LocalSet { local, value }) => { .. }
    _ => {}
}
let r = b.call(Callee::item(def, targs), &args, Providers::Keys(&provs), ty, syn);
```

Raw columns are `pub(crate)` in each IR's crate, so other crates cannot
index words; a reviewer can grep for `data[` outside the generated
module to find a violation.

**Which tool generates (recommendation: a checked-in generator).**

| Option | For | Against |
| --- | --- | --- |
| `build.rs` | always in sync; no extra command | output hidden in `target/`; agents and reviewers cannot read or grep it; slows every build; a build script failure is hard to read |
| proc macro over Rust declarations | schema next to the code; IDE sees the source | generated code invisible; long compile times for big `match`es; cannot emit JavaScript or docs; macro errors are poor |
| **checked-in generator, `cargo xtask codegen`** | output is plain Rust and JS in the repo, readable, greppable and reviewable in diffs; zero cost per build; one tool emits Rust, JS and docs | can drift: CI regenerates and fails on any diff |

**Recommendation:** the checked-in generator, which D1's §2.2 already
names, and rust-analyzer uses for its syntax. Agents writing the
compiler read generated code as ordinary code, which matters more than
saving one command. The CI diff check removes the drift risk.

**What the accessor layer costs.** Decoding a view is a tag load, one
8-byte load and a slice computation. It is inlined, so it costs about a
nanosecond and no allocation. The costs that are real: a generated
module per IR to keep in sync (CI regenerates and diffs it), longer
compile times of the generated `match`es, and debugging that goes through
the printer instead of a derived `Debug` on a tree. The printer is
therefore part of the first slice of each IR.
