# New Compiler Design: Resolution And Interfaces

Part of the [compiler design](README.md).

### 4.7 Discovery, Module Identity And Folders

`hd_project` maps the file list to modules and folders from paths only:

1. Walk the source root, the test root and `tasks` through `SourceSet`.
2. Apply the module rules: `mod.hd`, `lib.hd`, `main.hd`, `_test.hd`,
   `tests/` programs and shared modules
   ([Path-Inferred Modules](../../spec/lang/10-modules.md#path-inferred-modules),
   [Test Modules](../../spec/lang/10-modules.md#test-modules)).
3. Validate identities: NFC identifiers and case-folding collisions
   (`duplicate-module-name`, `invalid-module-path`, `reserved-module-name`).
4. Assign folders, including the rule that `x.hd` beside a directory `x/`
   of source files lives in folder `x/`
   ([`module.folder.parent-file`](../../spec/lang/10-modules.md#r-module.folder.parent-file)).

The module set always comes from this scan, never from cache entries, so a
deleted file is never served from the cache (Gleam #4320).

### 4.8 Folder Graph

1. For each non-test `use` in a file of folder `A`, find the module its
   path reaches: the longest prefix of the path that names a module.
   `use pkg.shop.Item` and `use pkg.shop.{Item}` both reach `shop`
   ([`module.cycle.folder-edge`](../../spec/lang/10-modules.md#r-module.cycle.folder-edge)).
2. Add the edge `A -> folder(module)` when the folders differ and the
   module is in the same package.
3. Run Tarjan's algorithm with folders visited in path order, so the SCCs
   and their order are deterministic.
4. For each SCC with more than one folder, report `folder-cycle` once: the
   shortest loop found by breadth-first search from the SCC's least
   folder, the `use` that makes each edge, the tangle size (the SCC's
   size), and the fix-it that moves the first file `x.hd` on the loop to
   `x/mod.hd` ([Cycle Diagnostic](../../spec/lang/10-modules.md#cycle-diagnostic)).
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
[Module Scope](../../spec/lang/03-names-and-scopes.md#module-scope).

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
  ([`module.pub-use.chain.loop-use`](../../spec/lang/10-modules.md#r-module.pub-use.chain.loop-use)).
- The work is bounded by the folder's uses times the chain length. It is a
  serial, monotone pass over one SCC (lesson 7).
- Other use errors: `unknown-module`, `unknown-import`, `private-import`,
  `direct-variant-use`, `ambiguous-import`, `test-only-use`.

**Paths in types and expressions** resolve through the module scope:
module aliases, qualified names, `Self`, type parameters, and the names
behind literal suffixes and string prefixes
([Forms Resolved By Name](../../spec/lang/02-grammar.md#forms-resolved-by-name)).
An unknown name is reported once per name per body; later uses are poison.

**Orphan and coherence inputs.** For each impl head, resolution records the
owning package and module of the trait, of the target's outer constructor
and of each trait argument's outer constructor. The orphan rule
(`orphan-impl`) and the module rule (`nonlocal-impl`) are local checks on
these facts ([Implementation Ownership](../../spec/lang/09-traits.md#implementation-ownership)).
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
   ([Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds)). For
   `@error`, `@from` and `@source`, the `Display`, `Error` and `From`
   heads.
5. Run the header checks that need nothing else: `private-type-leak`,
   `orphan-impl`, `nonlocal-impl`, `unconstrained-impl-parameter`,
   `ambiguous-row-pattern`, `misplaced-derivation`, a second template
   (`overlapping-impl`), a missing public result type or row
   ([`module.package.annotated`](../../spec/lang/10-modules.md#r-module.package.annotated)),
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
([`module.vis.signature`](../../spec/lang/10-modules.md#r-module.vis.signature)).
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
  deriving module ([`annot.template.checked`](../../spec/lang/14-annotations.md#r-annot.template.checked)),
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
  changes the key ([Go verdict](prior-art-issues.md#go-gotypes-the-build-cache-and-gopls)).

### 4.12 Traits, Impls And Coherence

#### 4.12.1 Owner-Module Impl Tables (mine)

hd's ownership rules say exactly where an impl may live: the module that
declares the trait, the target's outer constructor, or a trait argument's
outer constructor ([Implementation Modules](../../spec/lang/09-traits.md#implementation-modules)).
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
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)).
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
  ([Budgets And Schedule Independence](prior-art-issues.md#budgets-and-schedule-independence)).
- **Method lookup.** Inherent methods of the receiver's nominal type come
  first, from the target module's inherent table. Then the methods of
  traits available in the module
  ([Trait Availability](../../spec/lang/09-traits.md#trait-availability)):
  its uses, its declarations, the prelude and the receiver's bounds. Each
  such trait with the method name asks one goal. Several matches are an
  ambiguity error. None is an unknown-method error, with a `use`
  suggestion when an unavailable trait would match.

#### 4.12.3 Coherence

- Overlap is decided per trait from heads alone
  ([Overlap](../../spec/lang/09-traits.md#overlap)).
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
  ([`module.interface.coherence`](../../spec/lang/10-modules.md#r-module.interface.coherence)).
  It is cached and cheap, and agents see the error one command earlier.
