# New Compiler Design: Core Data Structures

Status: design record; may lag the code. The code and the spec win.
Each "Implementation differs" note marks a place where the code is
behind this design.

Part of the [compiler design](ARCHITECTURE.md).

## 3. Core Data Structures

This chapter covers the data structures the compiler keeps. §3.1 to
§3.10 give the shared rules: IDs, paths, interners, types, arenas,
poison, spans, diagnostics, the data-oriented encoding and each IR's
contract. §3.11 to §3.23 describe each structure.

| Structure | Section | Code |
| --- | --- | --- |
| IDs and sentinels | §3.1 | `hd_base` |
| stable paths, path hashes | §3.2 | `hd_intern::PathTable` |
| strings and symbols | §3.3 | `hd_intern::ShardedInterner` |
| InternPool: types, rows, lists | §3.4, §3.9.2 | `hd_types::pool` |
| diagnostics and fix-its | §3.8 | `hd_diag::buf::DiagBuf` |
| tokens and line tables | §3.11 | `hd_syntax::TokenBuf` |
| layout cursor | §3.12 | `hd_syntax::LayoutCursor` |
| green tree and its wire form | §3.13 | `hd_syntax::GreenTree` |
| header skeleton, item index | §3.14 | `hd_syntax::HeaderSkeleton`, `ItemIndex` |
| name-resolution tables | §3.15 | `hd_resolve::ModuleScope` |
| folder interface and blob | §3.16 | `hd_resolve::FolderIface` |
| impl tables | §3.17 | `hd_types::solver::ImplTable` |
| TIR | §3.18, [§4.13.11](../../crates/hd_tir/DESIGN.md#41311-the-typed-ir-tir) | `hd_tir::ir::Body` |
| per-body checker state | §3.19 | `hd_check::body::Ck`, `hd_types::unify::InferTable` |
| cache entries | §3.20 | `hd_cache::store` |
| task graph | §3.21 | `hd_sched::TaskGraph` |
| instance table, code entries | §3.22 | `hd_mono::layout::InstanceTable`, `hd_wasm::Code` |
| Wasm emission state | §3.23 | `hd_wasm::emit` |

### 3.1 IDs And Their Scopes

Every ID is a `u32` newtype made by `hd_base`'s `id!` macro, with
`NONE = u32::MAX`, `get() -> Option<Self>`, `idx()`, `raw()` and
`from_raw()`. IDs derive `Eq` and `Hash`, not `Ord`, and have no
`StableHash` impl, so code that sorts by an ID or hashes one into a key
does not compile.

| ID | Scope | Indexes | Stable form at boundaries |
| --- | --- | --- | --- |
| `Symbol` | run | interned string (§3.3) | the string |
| `PackageId` | run | resolved package | the package identity |
| `FileId` | run | source file | package plus package-relative path |
| `ModuleId`, `FolderId` | run | module, folder; dense | package plus path |
| `PathId` | run | a node of the stable-path trie (§3.2) | the printed path |
| `DefId` | run | any declaration, member, variant or impl; a `PathId` whose node is an item | its path (§3.2) |
| `Ty`, `TyList`, `RowId` | run (global) or body (local) | an InternPool item (§3.9.2) | the entry's own type table (§3.20.2) |
| `TokenIdx`, `NodeIdx` | file | token, green node | byte offsets |
| `ItemIdx` | module | item in source order | item path |
| `InferVar`, `LocalId`, `SubId`, `CaptureId`, `LabelId` | body | inference variable, local, sub-body, capture, label | none: never leaves the body |
| `InstId` | build | collected instance | instance key (§3.22) |

**Sentinel rule.** `NONE` means "absent" in every column, in memory and
on disk. Index 0 is a real item, so a column is indexed without an
offset. Storage never holds `Option<Id>`, which would be 8 bytes.

### 3.2 Stable Paths And Stable Hashing

A stable path names a declaration across runs, machines and checkouts.
Interface blobs, cache keys, instance keys and diagnostics print or hash
it. In a run, code compares `DefId`s and never builds a path.

**Layout: a hash-consed trie.** Every package, module segment and item
segment is one node of `PathTable`. Equal paths are the same node, so
path equality is one compare. Columns are `AppendVec`s: `parent`, `seg`
(the segment text), `kind` (`Package | Module | Item | Member | Variant
| Impl | Hidden`), `hash` and `def_module`. A dedup index of 16 locked
shards maps `(parent, kind, segment)` to the node; appends take one lock.

- **The path hash is incremental.** `hash(node) = H("path", hash(parent),
  kind, segment)`, computed once at intern time, so every `DefId` has its
  128-bit stable hash in one load. A package root hashes against zero.
- `Hash128` is xxh3-128.

**Stable hashing.** `StableHasher` is streaming xxh3-128 that starts
from a tag string. Integers hash little-endian at their declared width;
strings and lists hash their length first. No `usize` is hashed.

**Hash tables in memory** are indexes, never the only storage of data.
The design used fixed-seed tables and forbade iterating one outside
`hd_base` (a clippy `disallowed_methods` entry), so no table order can
reach output; the determinism matrix would vary the seed (§8.1).

In-run tables are `hd_base::HashMap` and `HashSet`: `std`'s tables with
a fixed seed (`hd_base/src/hash.rs`). `clippy.toml` bans `std`'s randomly
seeded `HashMap`, `HashSet` and `RandomState` outside that module. The
determinism tests (`hd_driver/tests/all/*_determinism.rs`) also build at
two other seeds and compare the bytes. Implementation differs: iterating
a fixed-seed table is not banned (about 35 `iter`, `keys`, `values` and
`drain` calls, plus `for` loops), so a leak is found only when the second
seed changes a tested output.

**Wire form.** None: blobs and entries carry their own path rows of
`(parent, kind, segment)`, written from the trie and re-interned on read
(§3.20.2).

### 3.3 Interners

| Interner | Holds | Concurrency | Lifetime |
| --- | --- | --- | --- |
| strings (`Symbol`) | identifiers, module and package names | `ShardedInterner`: 16 locked dedup shards, one append lock | run |
| stable paths (`PathId`) | the trie of §3.2 | the same scheme | run |
| InternPool (`Ty`) | types with no inference variables, rows, lists | one set of columns, one append lock, 64 dedup shards, a per-thread read-through table | run |
| body-local pool | types holding `InferVar`s | hash-consed through a body-local table; one pool per body | body |

The parser interns names into a per-parse `hd_intern::Interner`.

1. **Lookup** hashes the content, locks one shard and probes it.
2. **Miss**: append under the append lock, publish the length, insert
   into the shard.
3. **Read** of an existing ID takes no lock. `AppendVec` never moves an
   item (§3.9.4).
4. **Pre-seeding.** `InternPool::default` interns the primitives in
   `Prim::ALL` order, then `Never`, `Poison`, the empty list, the empty
   row and `void` (the empty tuple), so `Ty::I32` and friends are
   constants.
5. **Per-thread read-through table (pool only).** The hottest keys
   (`i32`, `string`, `Option[i32]`) land on fixed shards. So each thread
   keeps a direct-mapped table of 4,096 slots in front of the shards. A
   hit is checked against the item's content, with no lock. An interned
   item never changes, so a slot is never stale.

The design also gave the string and path interners per-thread columns
with owner bits in the ID, a compiled static table for pre-seeded
names, and the read-through table. The code locks one shard per lookup
instead. That is simpler, and with string and path interning off the
hottest path the saving was never measured.

An ID's value depends on which thread interned first. That is harmless
because no ID is printed, sorted or hashed.

### 3.4 Types

A type is an index into the InternPool (§3.9.2). Storage is a one-byte
tag, a `u32` data word, a `u32` meta word and, for variable parts, words
in the pool's `extra` and `tys` columns. `TyData` is the decoded view,
owned for simplicity; `Types` reads and builds through the global pool
and, inside a body, the body's local pool.

`TyData` forms: `Prim` (`i8` to `u64`, `usize`, `f32`, `f64`, `bool`,
`char`, `string`), `Never`, `Poison`, `Adt { def, args }`,
`Tuple { elems, rest }`, `Option`, `Fn { params, result, row, suspends,
inputs }`, `TraitValue { def, args, bindings }`, `Param`,
`Assoc { assoc, trait_, self_ty, args }`, `Mut`, `Infer`, `Canon` (a
solver placeholder), `Row` (a row in a row-argument slot) and `Context`.
`void` is the empty tuple.

- **Projections name their item.** `Assoc` holds the associated type's
  `DefId` and the full trait reference, so `<S as Add[i32]>::Out` and
  `<S as Add[i64]>::Out` are different types.
- **Bindings are keyed by item**, not by name.
- **`Mut`, not `Readonly`.** In the spec `T` is the readonly view and
  `mut T` the marked form ([Views](../../spec/lang/04-type-system.md#views)).
- **One inference form.** A literal is an `Infer` variable whose kind
  (general, integer literal, signed integer literal, float literal)
  lives in the inference table (§3.19), because unification merges
  kinds.
- `TyList` and `RowId` are interned, so type equality is one integer
  comparison for global types.
- A global type never contains a local one: building a type that holds
  an inference variable without a local pool panics.
- Body types with variables live in the body's local pool. `resolve`
  replaces variables by their bindings and interns the result globally.
  Equal content gives the equal `Ty` whichever thread interns it.

**Rows.** A row is one pool item: its keys, sorted, and its declared row
parameters. Printing and hashing re-sort keys by canonical content
(§3.20.2), never by `Ty` value.

**Omitted rows.** A private callable with an omitted row has its row
solved from row facts beside the types (`hd_check/src/results.rs`:
`RowFacts`, `RowEdge`), so no type holds a pending row. The design kept
pending rows inside types, in a "row tier" of the pool that lived until
`ModuleFinish`, then swept them out. Keeping them out of the pool is
simpler: the pool needs no tier bits, and nothing has to be rewritten
before an entry is written.

### 3.5 Arenas And Lifetimes

| Arena | Holds | Lifetime |
| --- | --- | --- |
| session | interners, path table, global pool, frozen interfaces, impl tables, solver memo, task result slots | the run (a REPL keeps its session) |
| file | source text, `TokenBuf`, `GreenTree`, item index | the run, once parsed |
| body | the local pool, the inference table, the checker state, the body's TIR columns | one body check; the TIR moves into the module's result |
| build | the instance table, layouts, code entries | one program's build |

**Frozen versus per-task.** A task writes its result once into a
`OnceLock` slot, read only by tasks that the graph orders after it. So
body tasks read shared data with no locks. The only shared mutable
structures are the append-only interners and the solver memo, whose
answers are pure functions of frozen inputs.

The design reused one set of per-worker columns body after body and
copied each body out at its end. The code builds each body's columns
fresh. That is simpler, and the saved allocation was never measured.

### 3.6 The Poison Type

1. `Ty::POISON` is a pre-seeded global type.
2. It unifies with every type, satisfies every bound, has every member, and
   entails every row.
3. Any check whose operand or expected type contains poison stays silent.
   Only the error that created the poison reports. The `HAS_POISON` meta
   flag (§3.9.2) makes "contains poison" one load.
4. Sources of poison: an unknown name, a broken header, a syntax error
   node, a failed limit, and an item whose interface is poison.
5. A poisoned item still exports: its interface record carries `Poison`
   where its type is unknown, so dependents check the rest without noise.

### 3.7 Spans And Files

`Span { file: FileId, lo: u32, hi: u32 }` is 12 bytes of byte offsets.

- Line and column are computed only when output is rendered.
- A body stores spans as `NodeIdx` (TIR's `syn` column).
- In a `check` entry, positions are written relative to the module's
  code lines (the skeleton's `code_lines`), so a comment or blank-line
  edit moves them without changing the module's semantic key.
- **Token anchors.** An `iface` entry stores locations as
  `hd_resolve::anchor::Anchor { decl, member, tok, count }`: the
  declaration's index among the file's top-level nodes, the block
  member, the first token counted from the declaration's first token,
  and the token count. A blank line above a declaration, or spaces and
  comments inside it, change no interface key and leave no stale
  position.

Implementation differs: the design's anchor named the declaration by
its path row and kept byte offsets inside the first and last token; the
code names it by position among top-level nodes, with whole tokens; not
judged.

- Doc tests map their spans back to the `##` line that holds them
  ([`cli.test.doc.location`](../../spec/cli/command-line.md#r-cli.test.doc.location)).

### 3.8 Diagnostic Records

Every pass reports through a `DiagBuf`. The driver sorts diagnostics
into content order, keeps one per root cause, and writes them to the
`check` entry. The renderer reads them at output; `hd fix` reads
fix-its.

**Layout.** A `DiagBuf` is columns, one row per diagnostic: `code`,
`severity`, `primary` span, `message`, `labels`, `fixes` and
`root: Option<RootKey>`, plus pooled label, fix-it and edit columns and
one `text` buffer. A `RootKey` is `(item DefId, cause, value)`.
`FixSafety::Exact` marks a fix-it that passed the re-parse check, which
`hd fix` applies.

- **Checkpoint.** `DiagMark` holds the column lengths; rollback
  truncates them, as a trial needs (TC-5).
- Messages are stored as text, rendered when reported through stable
  paths, so a message is the same on every run. Rendering into compact
  or JSON form happens at output, so `--format` never enters a key.
  The design's typed message arguments and generated per-template
  builders are not built; text is simpler and already deterministic.
- `--max-errors` caps output, not storage, so summary counts stay exact.

**Structured `NotImplemented` carries an optional span.**
`hd_base::unsupported::NotImplemented` has `span: Option<Span>`;
`.at(span)` sets it only when none is set, so the innermost node that
knows its position wins as the error unwinds, and the driver falls back
to a sentinel span when no stage knew the position.

### 3.9 Data-Oriented Encoding

Tokens, the green tree, the InternPool, TIR and diagnostics are stored as
columns of fixed-size items with `u32` indices between them. Each
encoding is chosen by who reads it. The ideas come from Zig (AIR, the
InternPool, `MultiArrayList`), Carbon's SemIR, and the Yuku parser's
flat nodes and scratch buffers. Interfaces, impl tables and the checker
state are ordinary Rust structs.

#### 3.9.1 Four IRs, And The Stopping Point Of Each

There are four: tokens with the syntax tree, the interface, TIR, and
Wasm.

```text
tokens ─► syntax tree ─► interface (cached) ─► TIR, one per body ─► Wasm
```

- **TIR** is one typed IR per body, emitted directly by the checker.
  There is no checked tree that is later lowered to a mid-level IR. hd
  has no borrow checker, so it has no reason for rustc's THIR and MIR
  split.
- **No monomorphized IR.** Optimization plans rewrite TIR bodies;
  emission walks TIR under a type substitution and writes each
  instance's Wasm at once. TIR is cached per module; Wasm per instance.

| IR | Consumers | Encoding |
| --- | --- | --- |
| tokens | the parser, skim, the formatter | columns (§3.11) |
| syntax tree | the checker, `hd fmt`, `hd fix`, `hd doc` | columns with typed views (§3.13) |
| folder interface | resolution, the checker, `hd doc`, collection | its blob, read in place (§3.16) |
| TIR | collection and emission | tag + 8-byte data + `extra` (§3.18) |
| Wasm code entries | the linker | bytes plus relocations (§3.22) |

Implementation differs: an interface blob's tables and item index are
decoded when the entry is read, and each item record when first used
(`decode_lazy`, `Items` in `crates/hd_resolve/src/iface.rs`). Entries
are copied out of the store, and TIR is decoded in `Collect`. The design
read all of them in place, with no decode step and no second copy.

#### 3.9.2 The InternPool

One pool holds every global type, interned list and row. Resolution
writes header types; interface readers write types decoded from blobs;
the checker writes body types; the solver, collection and emission
read them.

**Layout.** One set of columns, shared by every thread:

| Column | Item |
| --- | --- |
| `tag` | `PoolTag`, 1 byte |
| `data` | `u32`: an inline payload, or an offset into `extra` |
| `meta` | `u32` flags: `HAS_INFER`, `HAS_POISON`, `HAS_PARAM`, `HAS_ASSOC`, `HAS_CANON` |
| `extra` | `u32` words: variable parts |
| `tys` | `Ty`: list items, lent as `&[Ty]` |

Tags: `Prim`, `Never`, `Poison`, `Adt`, `Tuple`, `TupleRest`, `Option`,
`Fn`, `TraitValue`, `Param`, `Assoc`, `Mut`, `Infer`, `Canon`, `List`,
`Row`, `RowTy`, `Context`. Tags with a record keep its first `extra`
word in `data`; the record's length follows from the tag and, for rows
and trait values, from counts inside it.

- **Lists are items in their own column.** A list's record is
  `[start, len]` into `tys`, so `list_items` lends `&[Ty]` with no cast.
- `meta` is computed from the children's flags at intern time.
- **No per-thread copies.** Threads share one pool. Appends serialize on
  the one append lock, which is Zig 0.14's scheme.
- **Constants are not in the pool.** TIR keeps each body's constants as
  `(Ty, u64)` rows and its string literals in the body (§3.18). The
  design pooled them globally; per-body rows are simpler, and a
  constant never needed sharing across bodies.

**The body-local pool** has the same columns plus a body-local dedup
table, so interning the same content twice returns the same index. It
must be hash-consed because the checker compares variable-holding types
with `==`. It is never truncated on rollback, so an index stays valid
after a trial. A local index sets bit 31, carries its pool's 4-bit
generation in bits 27..30, and the row in bits 0..26; reading it against
another body's pool fails loudly.

**Determinism.** Index values depend on thread order. They never reach
output: printing goes through stable paths, hashing through the
canonical encoding of §3.20.2.

**Wire form.** None for the pool itself. Each cache entry holds a type
table over its own path, string and type rows (§3.20.2).

#### 3.9.3 The Dense Encoding

TIR uses the pool's shape with two more columns (its full definition is
§4.13.11): `tags` (1 byte), `data` (`[u32; 2]`), `ty` (the result type),
`syn` (the syntax node) and `extra` (operand lists and records), plus
side columns.

- **Operands by index.** An operand is a `u32` reference. A list of
  operands is `{start, len}` in `extra`.
- **Rare data in side columns**, keyed by instruction index, so common
  instructions stay small: suspension points (`susp`) and literal
  fallbacks.
- **Order.** Every operand has a lower index than its user, so a loop
  from 0 to n visits operands first. Each block lists its instructions
  in `extra`, so emission walks block lists front to back.

#### 3.9.4 Tables As Struct-Of-Arrays

The pool, the path table, the item index, the module scope, impl tables
and TIR locals are columns indexed by an ID. A pass that reads one
property reads one dense column. A record whose every access reads all
its fields stays one struct, such as a task node (§3.21).

**Building blocks** in `hd_base`:

- `AppendVec<T>`: a growable column whose items never move. Chunk `k`
  holds `2^(k+10)` items; 22 chunks. Readers index without a lock;
  appends hold the column's writer lock.
- `Col<T> = Box<[T]>`: an exact-size frozen column.
- `Range32 { start, len }`: a range of `extra` words or of another
  column.

`AppendVec` is the workspace's one audited unsafe exception in the
compiler proper. Only its module in `hd_base` has `#[allow(unsafe_code)]`.
A chunk pointer is stored with a release store before any item in it is
published; slot `i` is written once, before `len` is raised past it with
a release store; readers acquire `len` and read only slots below it.

#### 3.9.5 Building: Scratch Buffer, Checkpoints, Truncation

- **Scratch buffer.** A builder that collects a variable list (call
  arguments, a block's instructions) pushes onto a scratch buffer, then
  flushes the range into `extra` and truncates the scratch back. Nested
  lists nest checkpoints.
- **Rollback is truncation.** Every column is append-only while a body
  is checked, so a checkpoint is a tuple of lengths. `TirCheckpoint`
  holds the lengths of instructions, `extra`, scratch, locals, captures,
  sub-bodies, labels, suspension rows, constants and strings. The
  checker's `Checkpoint` (`hd_check/src/trial.rs`) adds the inference
  trail, the diagnostics' `DiagMark`, pending goals and its own stacks.
  Undoing a trial truncates them all and unwinds the trail. The local
  pool is never truncated.
- **Writes after the fact** are allowed only as a fixed-size word or a
  column written once: the final type sweep rewrites `ty`; `finish`
  writes the capture modes; closing a loop writes its label row; a
  **reserved slot** is filled once. None moves an instruction.
- **Reserved slots.** The checker does not always check in evaluation
  order. A block's list is evaluation order, so the builder appends a
  placeholder instruction (`reserve`, tag `Hole`) and the checker fills
  it later (`fill`), once. The TIR verifier rejects an unfilled slot.

Implementation differs: the design filled a slot reserved before an open
trial's checkpoint only after every trial closed, through a
`deferred_fills` column, and let a slot become a `Splice` of several
instructions. The code has neither; not judged.

Implementation differs: the design patched pending provider records
(`Providers::Pending`) in `ModuleFinish` once omitted rows were solved.
The tag exists in `hd_tir`, but no code writes it; how such calls get
their providers is in `hd_check/src/results.rs`; not judged.

#### 3.9.6 One Schema, Generated Accessors

Planned, not built: see §3.25.

#### 3.9.7 Byte Budgets And Linear Passes

| Item | Size | Fields |
| --- | --- | --- |
| token | 9 B + 1 bit | kind 1, start 4, end 4; line-first bit (§3.11) |
| physical line | 10 B + flags | start 4, first token 4, indent 2 (§3.11) |
| syntax node | 14 B | kind 2, first token 4, last token 4, subtree length 4 (§3.13) |
| pool item | 9 B + `extra` | tag 1, data 4, meta 4 (§3.9.2) |
| TIR instruction | 17 B + `extra` | tag 1, data 8, type 4, syntax node 4 (§3.18) |

Passes that are one linear scan over columns: lexing; tree building from
events; TIR's final type sweep; collecting calls for init summaries (a
scan of `tags`); TIR serialization (column copies plus a remap of ID
words, §3.20.2). Emission walks block lists in order with a stack of
open blocks.

Planned, not built: CI measurement of these averages on std, failing on
a regression past the budget.

#### 3.9.8 What It Buys: Locality And Memory

- **Locality.** A pass touches only the columns it reads. The final type
  sweep reads `ty` and the local pool; the call scan reads `tags`. A
  column of 4-byte items puts 16 to a cache line, where a tree of boxed
  enums would put one node per line or less.
- **Serialization is copying plus a remap.** TIR columns of plain
  numbers are copied; columns of run IDs go through the entry's tables
  (§3.20.2).

### 3.10 IR Abstraction Contracts

Each IR has a contract: what is already decided in it, what is
deliberately not in it, and the invariants its verifier checks. A pass
may rely on the contract of its input and must establish the contract of
its output.

#### 3.10.1 What Each IR Holds

| IR | Resolved in it | Deliberately not in it |
| --- | --- | --- |
| tokens | token kinds and spans, line facts | decoded literal values, names, layout decisions |
| syntax tree | full syntax, every layout token, every comment by position | names, types, desugaring |
| item index | which syntax node each item is, its parent, its name lookup | anything the node already holds |
| folder interface | every public signature as pool types; re-exports followed to the real declaration; impl heads; templates; hashes | bodies, private items (except hidden template items), fact values, doc text, layouts |
| TIR | every name as a `DefId` or a local; every type; every callee (static, trait method with its choice, trait value, function value); every coercion as an instruction; operators on primitives as primitive instructions and all others as trait calls; indexing as calls; pipe, interpolation, string prefixes, literal suffixes, compound assignment, comprehensions and `?` desugared; evaluation order as instruction order; default and named arguments mapped to parameter slots; match decision trees; cleanup scopes with their `defer` suites; suspension points; providers per call; closure captures with their sharing mode; structured control flow | layouts; boxing; capture cells; vtables and dictionaries; frame layouts and state-machine dispatch; overflow and bounds check sequences (a profile choice); instantiation |
| Wasm code entry | instructions, locals, symbolic relocations | indices, section layout |

Everything in TIR's right column is decided during emission, per
instance and per tier. Nothing in it needs a second IR.

#### 3.10.2 Lifetimes, Sizes And Peak Memory

Planned, not built: per-IR size estimates checked against a measured
peak. The design's estimates assumed layouts (§3.5, §3.16) that the code
does not use.

#### 3.10.3 Invariants And Verifiers

A verifier failure is an internal error naming the item, never a user
diagnostic.

| IR | Verifier checks | Where |
| --- | --- | --- |
| tokens and tree | text round-trips byte for byte | `GreenTree::reconstruct`, `hd_syntax` tests |
| TIR | operands precede users; every reserved slot is filled; the structural rules of §4.13.11 | `hd_tir::ir::verify`, on every `finish` |
| cache entries | magic, kind, length, checksum and section bounds on every read | `hd_cache::store::decode_entry` |
| Wasm | `wasmparser` validation of the linked module | driver tests |

Planned, not built: verifiers for the item index, the interface (beyond
decode checks) and the pool, and a verify mode that recomputes cache
entries on hits.

### 3.11 Tokens And Line Tables

The lexer writes `TokenBuf`. The parser reads `kind` with one token of
lookahead; the layout cursor reads `kind`, the line-first bits and the
line table; the tree, the formatter and diagnostics read `start` and
`end`.

**Layout.** Token columns `kind` (1 byte, significant tokens only),
`start`, `end` and `line_first` (a bit set in `u32` words); line columns
`line_start`, `line_tok` (first token, or `NONE` for a blank or
comment-only line), `line_indent` and `line_flags`; comment columns
`com_start`, `com_end` and `com_kind` (`Plain | Doc | ModuleDoc`).

- **Why `start` and `end` (not Zig's start only).** Zig recomputes a
  token's end by re-lexing it. hd's string frames and interpolation make
  re-lexing a single token depend on the frame stack, and diagnostics,
  the formatter and node spans read `end` constantly.
- **Indentation is a line column**, not a token column. Bracket depth is
  tracked by the layout cursor as it walks, the only reader.

**Identity.** `TokenIdx` per file. Layout tokens are not in `TokenBuf`;
the tree holds them (§3.13).

**Accessors.** `kind(i)`, `span(i)`, `text(i, src)`, `is_line_first(i)`.

### 3.12 Layout Cursor And Skim State

The layout cursor turns tokens into tokens plus virtual layout tokens
(`Newline | Indent | Dedent | SuiteEnd`) for the parser. It holds a
stack of open indented blocks (statement column, bracket depth, header
column), the bracket depths of open same-line suites, and a queue of
pending layout tokens. Its decisions are recorded as zero-width positions
in the tree's layout columns, not in the token stream.

Skim (`hd_syntax::skim`) lexes the file and finds body ranges, `use`
lines and the hashes from the token columns (§3.14).

### 3.13 The Green Tree, Its Wire Format And The JS Decoder

The tree is the full syntax of one file. The header pass and the checker
read it through typed views; the formatter walks it in preorder with
tokens and layout tokens interleaved; fix-its and `hd doc` find nodes by
offset.

**Layout.** Node columns `kind` (`SyntaxKind`, 2 bytes), `first_token`,
`last_token` (inclusive; first > last for an empty node) and
`subtree_len`; layout columns `layout_at` and `layout_kind`; parse-error
columns `err_node` and `err_code`.

- **Preorder with subtree sizes.** The first child is `i + 1`, the next
  sibling `j + subtree_len[j]`. Skipping a subtree is one addition. A
  node's tokens not covered by its children are its own tokens.
- **Parent links** are built on demand (`parent_links`) in one pass.
- **Recovery by truncation.** The parser's event vector is append-only
  and truncated to a checkpoint after a failed attempt. The tree is built
  from the events in one pass after parsing.

**Wire format.** `GreenTree::to_wire` writes one little-endian buffer:
magic `HDST`, format 1, flags, a table of 16 section offsets, then
8-byte aligned sections: token kinds, starts and ends, line-first bits,
line starts, UTF-16 line starts (for JavaScript columns), line flags,
comments, node kinds, first and last tokens, subtree lengths, layout
positions, errors and the source.

Planned, not built: a generated JavaScript decoder over this buffer for
the playground, with one view class per node kind.

### 3.14 The Header Skeleton And The Item Index

**The skeleton** is the header pass's output for one file,
`HeaderSkeleton`: `source_hash`, `api_text_hash`, `token_hash` (the code
lines without plain comments, so a comment edit keeps it), `code_lines`,
`bodies` (byte ranges of each header and body, with the header's kind),
`uses` (byte ranges of `use` lines) and `broken`.

**The item index** comes from the full parse (`hd_syntax::ItemIndex`, in
`Parse`): columns `at` (the header node), `parent`, `name`, `kind`
(`Function | Data | Enum | Trait | Impl | Alias | Test | Statement`) and
`flags`, plus a `by_name` index. `name` and `kind` are copied from the
tree on purpose: name lookup and "all functions" filters run before any
view decode.

Implementation differs: the design's skeleton also carried a header-only
tree and the item index, so `FolderIface` built a folder's interface
without parsing bodies, and skim stored no tokens. The code fully parses
every file of a folder whose interface misses
(`crates/hd_driver/src/lib.rs:2523`), and skim lexes the whole file
(`crates/hd_syntax/src/skim.rs:44`). An interface rebuild costs a full
parse of the folder.

That full parse is not wasted today (#214). The interface key and every
check key of the folder's modules hold the deep hashes of the folder's
closure, and a check key also holds the module's token hash. So a
folder whose interface misses also rechecks its modules in the same
run, and the check needs the full parse anyway. A header-only build
would add a pass: on the 200-function file of bench/profile-signature,
the header pass takes 0.52 ms and the full parse 0.92 ms (release, best
of 30). It pays only once a check key can hit while its folder's
interface misses.

### 3.15 Name-Resolution Tables

A module scope answers "what does this name mean here" for headers and
bodies. `ModulePrep` builds it; the checker reads it on every
unqualified name.

`hd_resolve::ModuleScope`, frozen after `ModulePrep`: one row per name
visible at module level, with `name`, `binding` (an 8-byte `Binding`:
`Item | Module | Prelude | Ambiguous | Poison` plus a value), `origin`
(`Own | Use | PubUse | Prelude`) and `use_row`; an `index` from name to
row; `def`, the module's items as `DefId`s; and the module paths that
`Module` bindings name.

- **Body scopes are not here.** Local names live in the checker's scope
  stack (§3.19).
- A second binding of a name from a different origin is ambiguous.

A folder's exports are part of its interface (§3.16).

### 3.16 The Folder Interface: In Memory And As A Blob

A folder's interface is everything a dependent's check can read: public
items and their signatures, impl heads, templates, hidden template
items, hashes. Resolution reads exports by name; the checker reads
signatures, fields and variants by item; the solver reads impl heads
through impl tables (§3.17); coherence reads all heads; `hd doc` reads
items in order.

**In memory.** `FolderIface` holds the decoded `items` (with a `by_def`
index), `exports` (with an index by module and name), the `blob` bytes,
`api_hash` and `deep_hash`, and, outside every hash, item anchors,
private names, `pub use` loop names and a REPL input's module uses.

**The `iface` entry** has six sections: the blob, header diagnostics,
anchors, private names, loop names and module uses. The blob is the
entry-local tables (§3.20.2), then the items, then the exports sorted by
(module path, name).

- `private_names` lets cross-folder lookup tell `private-import` from
  `unknown-import` without exposing a private item. Anchors keep
  positions out of the interface; no record stores a file offset. Both
  are outside `api_hash` and `deep_hash`.
- **Canonical bytes.** Rows are written in content order: table rows in
  first-use order of a walk over items in source order, exports sorted
  by bytes. Never by run IDs or hash-map order.
- `deep_hash(F) = H("deep", api_hash(F), sorted [(path(G), deep_hash(G))
  for G in mentions(F)])`.

Implementation differs: the design kept the blob as the in-memory
interface, decoded one record on first use through memo arrays, and
memory-mapped it from the cache.
- **What is built.** The blob carries an item index: each item's
  definition, record offset and parameter names. Reading an entry
  decodes its tables and that index. `Items` decodes a record on first
  use and counts it (`Items::decoded`).
- **Index scans.** The passes that used to scan every item read the
  index instead: each item's tag marks an impl (and its kind) or a
  trait, so coherence's `Universe`, the solver's impl tables, the
  conflict checks, template lookup and the method index decode only
  those records.
- **Mentions.** An interface entry stores the folders its interface
  names, so a hit derives its deep hash without decoding.
- **Cost.** A warm `println` check decodes std's impls, which the
  solver's tables hold whole, plus the three other records it names
  (`Counters::iface_other_records`, crates/hd_driver/tests/all/lazy_ifaces.rs).
- **What is not built.** The blob is not memory-mapped, the tables still
  intern every row up front, and an impl table is not built per trait on
  demand.

Planned, not built: std's blobs embedded as one pack in the binary
(`hd_stdpack`). Today the driver embeds std's sources and builds std's
interfaces into the cache on first use.

### 3.17 Impl Tables

The solver asks for the impls of one trait, fast-rejected by the head's
outer constructor. Coherence asks for all heads of one trait. Method
lookup asks for the inherent impls of one type.

`hd_types::solver::ImplTable`, built by `hd_resolve::impl_table`, has row columns `trait_`, `def`,
`head_key`, `arg_key`, `n_params`, `head_self`, `head_args`, `plan` (the
bound plan), `assoc`, `origin` and `rank`; `by_trait`, each trait's row
range; and `by_key`, the rows of each trait sorted by head key.

- `HeadKey` is `Ctor(DefId) | Prim | Tuple(arity) | TupleAny | Fn |
  SuspendFn | Param | Any`, a full-width payload per kind.
- Rows are sorted by `rank`, a content hash, so candidate order is the
  same on every run.
- **Tuple templates and numeric families are rows**, with head keys
  `TupleAny` and `Param`
  ([crates/hd_check/DESIGN.md (trait solver) §3.9](../../crates/hd_check/DESIGN.md#39-compiler-supplied-impls)).
- **Sealed traits have no rows.** They are answered from the type's form
  (`hd_types::sealed`).

### 3.18 TIR

The full encoding, instruction catalog and builder are in
[§4.13.11](../../crates/hd_tir/DESIGN.md#41311-the-typed-ir-tir).

**Layout.** `hd_tir::ir::Body`, one per body:

| Column | Item | Read by |
| --- | --- | --- |
| `tags` | `Tag`, 1 byte | everyone; collection scans it alone |
| `data` | `[u32; 2]` | emission, collection |
| `ty` | `Ty` | the type sweep, emission |
| `syn` | `NodeIdx` | diagnostics |
| `extra` | `u32` | operand lists, callee and coercion records |
| locals: `local_ty`, `local_name`, `local_syn`, `local_flags` | | emission, the checker |
| subs: `sub_root`, `sub_params`, `sub_parent`, `sub_flags` | | emission of closures |
| captures: `cap_local`, `cap_mode` | | emission of closures |
| `label_inst` | | emission |
| `consts`, `strings` | `(Ty, u64)`; literal text | emission; indexed by a constant `Ref` |
| `susp` | `SuspRow`, 16 bytes | suspension lowering |
| `fallback` | | panic reports for defaulted literals |

**Identity.** Instruction, local, sub-body, capture and label indices are
body-local `u32`s. A `Ref` is an instruction below 2^31, or, with bit 31
set, a row in the body's `consts`, so constants need no instruction.

**Determinism.** Instructions are in evaluation order, locals and
captures in creation order. Run IDs are remapped on write (§3.20.2), so
the entry's bytes and the TIR hash are run-independent.

**Wire form.** The TIR section of the `check` entry (§3.20.4), each body
under its item's path hash, so `Collect` decodes only the bodies it
reaches.

### 3.19 Per-Body Checker Scratch

Everything a body check mutates is in `hd_check::body::Ck`, made per
body: the TIR builder, the local pool, the inference table, scope and
loop stacks, pending goals, fuel and the body's memo. `BodyCx` holds the
module-level context the bodies share.

- **Inference table** (`hd_types::unify::InferTable`): columns `parent`,
  `rank`, `binding` and `kind`, plus the trail.
- **One local table.** The checker uses TIR's local columns, written
  through the builder.
- **Union-find: union by rank, undone by the trail.** Links, rank
  changes and kind changes are each trailed (`Parent`, `Rank`, `Kind`,
  plus `Bind`/`Rebind`), and a rollback restores each one exactly. There
  is deliberately no path compression: `root` is a read, and each
  compressed link would need its own trail entry.
- **Checkpoint** (§3.9.5): the trail length and variable count
  (`TrialMark`), the output lengths and a `TirCheckpoint`. The local pool
  has no length in it: it is never truncated, so an index survives a
  rollback.

The design reused one scratch set per worker; the code allocates per
body, which is simpler (§3.5).

### 3.20 Cache Entries And The Manifest

#### 3.20.1 The Entry Container

Every cache entry kind uses one container, so one reader and one
checksum serve all of them (`hd_cache::store`).

- **Header**, 64 bytes, little-endian: magic `HDCE`, kind (`u16`),
  layout (`u16`), key and toolchain key (`Hash128` each), payload length
  (`u64`), checksum (the low 64 bits of xxh3-128 over everything after
  the header), section count and flags (`HAS_DIAGNOSTICS`, `HAS_TIR`).
- **Section table**: 16 bytes per section, `kind u16, flags u16, rows
  u32, offset u64`. Each payload starts with its `u32` length.
- **Validation on load:** magic, kind, length, checksum and section
  bounds. Any failure is a miss (§5.4).
- **Versioning.** `hd_cache::LAYOUT_VERSION` is part of every toolchain
  key, so a layout change misses instead of misreading. Old entries are
  never read, only evicted.

#### 3.20.2 The Wire Rule: Entry-Local Tables

**No run ID is ever written to disk or hashed.** Pool indices, `DefId`s,
`Symbol`s and `FileId`s depend on the run. So every entry that holds
types or names carries its own tables (`hd_types::wire`):

| Table | Rows | Replaces |
| --- | --- | --- |
| paths | `(parent, kind, segment)`, parents first | `DefId`, `PathId` |
| types | the pool encoding over these tables, children first | `Ty`, `TyList`, `RowId` |
| symbols | strings | `Symbol` |

For TIR, each body owns its own tables. Its TIR hash is therefore a
function of that body alone: adding or changing another body cannot
renumber its rows. Shared rows are repeated once per body on purpose.

- **The writer** looks each ID up in a per-write memo and appends a new
  row on a miss, so rows are in first-use order of a walk that is
  already in content order. Row keys, bindings and row parameters are
  re-sorted by their own encoding before writing, so they are in content
  order on disk.
- **The reader** re-interns each row into the run's tables.
- `hd_tir::wire` knows the ID-carrying words of each tag. A tag whose
  words are not mapped is refused with a structured error, never written
  with run IDs.

#### 3.20.3 The Stat Manifest

Planned, not built: a stat manifest in `build/.hd/manifest`, one record
per file (path, size, `mtime_ns`, `ctime_ns`, inode, source hash, api
text hash, use paths, role), read whole at start, so a warm run hashes
only files whose stat changed. It would also hold the last run's keys.
`hd_cache::store::ManifestRecord` defines the record; nothing reads or
writes it.

#### 3.20.4 Entry Sections By Kind

| Kind | Content |
| --- | --- |
| `iface` | §3.16 |
| `check` | diagnostics; module meta; items (private signatures and inferred results); the TIR, one section of bodies; row results; the init summary; `dbg` references; frame names; a REPL input's bindings |
| `graph` | one part: a folder's header findings (`hdr`), the coherence result (`coh`) or a folder's init order (`init`) |
| `depfiles` | a dependency package's file skims |
| `parse` | empty: the source parses cleanly |
| `codepack` | one folder group's Wasm code (§3.22) |
| `packhint` | each folder group's recent `codepack` keys, per package |
| `link` | the linked module |

Positions in `check` entries are relative to code lines (§3.7), so
emission never needs the syntax tree. The kinds `check-test`, `pkgres`,
`clpack`, `cwasm` and `code` are declared in `EntryKind` but not
written.

### 3.21 The Scheduler's Task Graph

The driver and finished tasks add tasks and edges; the executor pops
ready tasks; tasks read their dependencies' result slots.

`hd_sched::TaskGraph` holds a `Vec` of task nodes and a ready queue. A
node is `kind`, `waiting_on`, `successors`, `state` (`Waiting | Ready |
Running | Done | Cancelled`) and `priority`. `TaskId` is its index, in
creation order.

- **An edge from a finished task counts as satisfied at once.**
- **Creation guard.** `add_held` starts a task with `waiting_on = 1`; the
  creator adds its edges, then `release` drops the guard. So a task never
  becomes Ready while its edges are still being added.
- **The pool executor** (feature `threads`) runs ready tasks on rayon's
  work-stealing pool; the graph sits behind one mutex, held only to add,
  edge, release or complete.
- **Result slots** are per-kind `OnceLock`s in the driver, indexed by
  the dense file, folder or module ID.

The design had atomic per-node counters and a lock per node's edge list.
One mutex over the graph is simpler and sound, and a run has a few
hundred tasks, each far longer than the lock.

**Determinism.** Task IDs and executor order never reach output. The
serial executor picks FIFO, by priority then creation order, or by a
seeded shuffle.

### 3.22 Codegen: The Instance Table And Code Entries

Collection builds the instance set of one program; emission reads one
instance at a time; link reads every code entry of the set in key
order.

**Instance table** (`hd_mono::layout::InstanceTable`), columns by
`InstId`: `item`, `sub` (the item or a closure sub-body), `args` (the
concrete type arguments), `known` (the known closures of a specialized
instance), `depth` (type-argument nesting, §13.4), `parent` (the
requesting instance, for the depth diagnostic) and `key` (the instance
key). An index maps `(item, sub, args, known)` to the instance.

- **Dedup in run IDs, keys in content.** Collection dedups by run IDs
  and computes the 128-bit instance key once per new instance. The key
  sorts the set and names the code entry.

**Code entry** (`hd_wasm::Code`): parameter and result types, the body
bytes (locals and instructions, index immediates as 5-byte padded LEBs),
relocations sorted by offset, line anchors, and a suspending instance's
other functions.

**Code packs.** A folder group's code entries are one `codepack`
entry, never files of their own (crates/hd_cache/DESIGN.md §5.4). Its
first section lists the code keys. Its second is `encode_codes`: one
type table, one table of relocation targets, then the entries. A
relocation is 8 bytes, its offset and its target's index. So a target
that several functions of the group call is written once.

### 3.23 Wasm Emission Buffers

One `Emit` task writes one instance's Wasm and its relocations; `Link`
writes the module. Emission state (`hd_wasm::emit::Em`) is made per
instance: the assembler buffer, value and local maps, the control stack,
cleanup scopes and suspension state.

The design reused per-worker buffers with direct-indexed value columns.
Per-instance state is simpler, and the saving was never measured.

**Determinism.** Wasm locals are numbered in TIR order; relocation order
is offset order.

### 3.25 The Schema Language

Planned, not built: one declarative schema per IR and wire form,
checked in with its generator (`cargo xtask codegen`), producing tag
enums, size asserts, typed views and builders, structural verifiers,
printers, wire codecs with the ID remap, and the tree's JavaScript
decoder. A checked-in generator was preferred over `build.rs` and proc
macros because its output is readable, greppable and costs nothing per
build; CI would regenerate and fail on a diff. Today TIR's tags and
operand kinds come from one `macro_rules!` table in
`crates/hd_tir/src/ir.rs`, and the diagnostic codes from
`spec/tools/diagnostic-codes.ts`.
