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

**Use roots (#106).** `hd_project` owns one mapping, `module_path` (with
`module_below` for the path below the root): the package identifier
(each `-` as `_`, e.g. `acme-shop` → `acme_shop`), then the path below
the source root. `src/lib.hd` is the root module itself, `src/x/mod.hd`
is `x`, and a flat `main.hd` is `main`. The test and task roots get
segments no source path can spell, `$tests` and `$tasks`, so
`tests/checkout.hd` is `pkg.$tests.checkout` while `src/tests/checkout.hd`
stays `pkg.tests.checkout` (a different module), and no `pkg` path
reaches test or task code (`module.test.no-tests-root`). Provisional:
an owner question on the `tests/checkout.hd` vs `src/tests/checkout.hd`
collision is open. Each module also records its relative base (its root
for a root file: `src/lib.hd`, `src/main.hd`, or a file directly under
`tests/` or `tasks`), so `self` starts there, and its floor (the package
root, or the test/task root), which a root `super` must stay within.
Entry modules (`src/main.hd`, test programs, tasks) are their own
program, which no `use` reaches. `UseRoots::absolute` maps `pkg`, `std`,
`dep.NAME`, `self` and `super` and rejects any other first segment; an
unknown `dep.NAME` and a `super` above the floor are their own errors.
Dependencies load only their library (test code and executables never
build), `requires` maps each `dep.NAME` to its package, and a manifest
loop is `package-cycle`. A package with `src/` takes files only from
`src`, `tests` and `tasks`; `[dev-dependencies]` parses but does not
load yet.

The module set always comes from this scan, never from cache entries, so a
deleted file is never served from the cache (Gleam #4320).

### 4.8 Folder Graph

1. For each non-test `use` in a file of folder `A`, find the module its
   path reaches: the longest prefix of the path that names a module.
   `use pkg.shop.Item` and `use pkg.shop.{Item}` both reach `shop`
   ([`module.cycle.folder-edge`](../../spec/lang/10-modules.md#r-module.cycle.folder-edge)).
2. Add the edge `A -> folder(module)` when the folders differ and the
   module is in the same package.
   Fixed prelude uses participate exactly like written uses, except that
   the virtual `std.core` module does not import the prelude.
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

**M3 gap 3.** The fixed-use rule intentionally makes every non-core folder
depend on folder `std`, because every ordinary prelude origin lives there.
Folder `std.prelude` also reaches `std.testing` through its child test
module. These edges are the specified prelude semantics, not bootstrap
exceptions.

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

**Failed uses bind poison (#110).** A failed use (unknown module,
unknown import, private import, bad root, ambiguous import) still binds
every name it would have bound — each listed name alias-aware, or the
module alias — as `BindingKind::Poison`, so later references resolve to
something and stay quiet instead of cascading new diagnostics.
`ModuleScope::bind` gives poison both directions of precedence: a poison
binding never overwrites a real one, and a real binding replaces
poison. The checker reads poison in references, calls, members,
interpolations, literal functions and type positions, and
reports nothing there: one error at the `use`, silence after.

**Private-name index (M3 gap 2).** Each interface has a sorted
`private_names` section of `(module path, name, kind, declaration anchor)`.
It contains no signature or item record, and it is outside `api_hash`,
`deep_hash` and every item hash. A failed export lookup checks this index:
a matching row reports `private-import`; no row reports `unknown-import`.
This preserves the required diagnostic without exposing private items to
resolution or invalidating dependents for private header edits.

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
   (GADT result types are removed with GADTs); trait members, associated types, supertraits and
   default presence; impl heads with `by` delegation; aliases and
   newtypes.
   While lowering a written trait reference, fill omitted trailing trait
   arguments from the declaration's defaults in declaration order.
   Substitute earlier arguments and the reference's `Self`: the bounded
   type in a bound, or the target type in an impl head. Thus a bound or
   head written `Add` lowers as `Add[Self]` for the declared
   `Rhs = Self` default ([M3 gap 4](#410-folder-interface-construction)).
   While lowering a `mut T` type, a type parameter with no written bound
   list is `mut-on-type-parameter` (`types.generic.no-mut-t`), checked
   here in lowering beside the primitive and tuple `no-mut` errors; a
   parameter with any bound list is constrained and passes, and `mut
   Self` is exempt (it names no declared type parameter).
4. Build derived heads. For `@derive(X)`, the impl of `X` with `T < X` for
   each type parameter in a walked member
   ([Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds)). For
   `@error`, `@from` and `@source`, the `Display`, `Error` and `From`
   heads.
5. Run the header checks of stage A in §4.10.1: everything that reads
   only headers, F's own and its dependencies'. Among them: impl
   parameters that appear only inside a projection in the head are
   unconstrained (`unconstrained-impl-parameter`, owner, 2026-10-07); and
   every private item that a template body names must have a written
   signature (§4.10.1, the hidden-helper rule). In the same pass, compute
   each impl's **bound plan** (trait-solver.md §3.6), each trait's
   per-member `dyn` availability (an unavailable flag with its reason,
   and the associated items each member mentions) and its **vtable
   shape** (trait-solver.md §9.1 and §9.2), and list each tuple template
   in the `heads` section with head key `TupleAny` (trait-solver.md
   §3.9).
6. Build each module's `ImplTable` (§4.12.1), the `arg_impls` section,
   and, for std, the synthetic table of built-in targets.
7. Write the blob (§4.11), then compute item hashes, `api_hash`,
   `deep_hash` and `heads_hash`.
8. **Validate on write** (lesson 12). Every `DefId` that the api sections
   mention is exported from its folder, or is a hidden item of this
   folder that a template body or another hidden item's signature names.
   A failure is an internal compiler error naming the item, not a user
   diagnostic, since user-facing leaks are already `private-type-leak` in
   step 5. Debug builds and verify mode also decode the written bytes and
   compare them with the in-memory interface.

After step 8 the interface is usable: dependents may read it. The
stage-B checks of §4.10.1 run next, in their own task, and no dependent
reads their result.

**What the interface holds.** Public items, all impl heads, templates with
their bodies, hidden items, and per parameter and field whether it has a
default. It holds no private item record except hidden ones. Its
`private_names` section carries only enough information to distinguish a
private import from an absent name. It holds no ordinary body, doc comment
or fact value.

**Top-level bindings (M4a gap 5).** An ordinary top-level binding is
module-initialization state, not an interface item kind. It may be read
by bodies in its declaring module, where the body checker assigns its
global slot and type, but another module cannot import or refer to it.
Consequently `FolderIface` carries neither its declaration nor its
initializer; cross-module state is exposed through an ordinary declared
item instead.

**Declaration locations (M3 gap 5).** Every item, impl head and stored
header diagnostic carries a `TokenAnchor` from data-structures.md §3.7.
The anchor is an index into the blob's `anchors` section and is excluded
from every semantic hash. Stage B and coherence retain the relevant item
row, then resolve its anchor through the current file's `locs` entry.
They must never replace that location with the module file's zero offset.

- **Defaults are not in the interface as expressions.** A caller emits a
  call of the default's own body with the earlier arguments
  ([type-checking.md §1.7](type-checking.md#17-body-tasks-and-the-exactly-once-rule)),
  so it needs only the parameter's type and the fact that a default
  exists. Codegen reads the default's TIR from its module's `tir` entry,
  as for any generic function.
- **Facts carry their type only.** A check needs only a fact's type.
  Values are computed once, at compile time, by the backend's evaluator
  (review finding 4).
- **Hidden items and their closure (review finding 6).** A template body
  may name private items of its trait's module (owner, 2026-10-07). The
  interface holds each such item's record in the `hidden` section, plus,
  transitively, every private type its signature names, with that type's
  fields, variants and the methods the template body names. All of it
  comes from headers and the template's resolution table, never from a
  body, because every hidden function has a written result type and its
  omitted `$` clause means the empty row (§4.10.1). So "interfaces come
  from syntax alone" stays true, and a hidden helper's body edit changes
  no interface byte. User code cannot name a hidden item.

**Two hashes, two readers (mine).** `api_hash` and `deep_hash` cover what
a dependent's check can read: public items, and impl heads whose target
and trait are both nameable outside their module. Impl heads for private
types go only into `heads_hash`, which coherence reads (§4.12.3). A
dependent cannot name a private type, and a private type cannot appear in
a public signature
([`module.vis.signature`](../../spec/lang/10-modules.md#r-module.vis.signature)).
So adding `impl Display for PrivateThing` rechecks no dependent.

The rule covers impls owned through a trait argument too (Codex
re-review N-A1). `impl Pick[Product] for Receiver` in `Product`'s module
has a nameable trait and target, whatever `Product`'s visibility, so its
head is in `api_hash`, and adding it changes the folder's deep hash. A
module's `check` key holds the deep hash of every folder in its
dependency closure, the same closure that filters the candidate
directory (§4.12.1). So the deep hashes already cover every
argument-owned impl a check can see, and a separate `arg_impls` closure
hash would be redundant. The `arg_impls` section keeps its own hash only
so the driver can rebuild the directory cheaply.

**Complexity.** Linear in the folder's header tokens plus the use worklist.
A folder task cannot be split, so one very large folder bounds the wall
time of a cold run. That is a measured risk, not designed away.

#### 4.10.1 Header Validation Stages

The review (T7) found that the header checks had no complete stage
contract. Every header rule runs in exactly one of three stages:

| Stage | Task | Reads | Rules |
| --- | --- | --- | --- |
| A | `FolderIface(F)`, step 5 | F's headers, its dependencies' interfaces | duplicate declarations; `private-type-leak`; `orphan-impl`; `nonlocal-impl`; `bare-parameter-impl-target`, `trait-value-impl-target`, `mutable-impl-target`, `invalid-impl-target`; `unconstrained-impl-parameter` (projections included); `sealed-trait-implementation`; `ambiguous-row-pattern`; `misplaced-derivation`; a second template or a derive beside a written impl (`overlapping-impl`); `mixed-derived-law`; `duplicate-inherent-member`; `missing-result-type` for public items and hidden helpers; `default-order`; `generic-kind-mismatch`; cycles of aliases and supertraits (`supertrait-cycle`); `ambiguous-associated-type`; declared variance against every use in fields, payloads and methods, private inherent methods included; impl members against the trait: completeness (`missing-trait-method`), signature equality, associated-type bindings complete |
| B | `HeaderCheck(F)`, after F's interface and its dependencies' interfaces | the solver over frozen impl tables | written types against their declared bounds, for every type written in a header (a `Set[K]` needs `K: Hash` from the item's own bounds: [`trait.bound.no-implied`](../../spec/lang/09-traits.md#r-trait.bound.no-implied)); supertraits of each impl and supertrait bindings (`missing-supertrait-implementation`); a derived newtype's base impl; a delegation target's impl |
| C | body tasks (M2) | bodies | derive-instance member obligations; local impls; everything in [type-checking.md](type-checking.md) |

- **Why stage B is a task of its own.** Its goals need the impl tables of
  F and of every dependency, which step 6 builds after stage A. Its
  answers never change what a dependent's check reports, so dependents
  do not wait for it. A run with a stage-B error fails as any run with an
  error does, and codegen waits for every `HeaderCheck` task of the
  program graph.
- **Dependencies are checked too.** Stages A and B run for every folder
  of the program graph, std and dependencies included, whether or not
  the run checks that folder's bodies. So an imported impl that misses a
  member or a supertrait is reported, in the dependency's file, even by
  `hd check`. When a dependency's bodies are checked is the command's
  policy ([commands.md](commands.md)), not the interface's.
- **Caching.** Stage A's diagnostics are in the blob's `diags` section,
  outside every api hash. Stage B's are the `HeaderCheck(F)` task's
  result; its key is F's interface key plus the deep hashes of the
  folders in F's dependency closure. That entry kind is the
  backend lane's ([cache.md](cache.md)).
- **Fuel.** Each item's stage-B goals share one fuel budget per item, as
  a body's do, so one pathological header fails only itself.
- **The hidden-helper rule.** A private function that a template body
  names must write its result type (`missing-result-type`), and without
  a `$` clause it has the empty row, as a public function does. A
  template may not name a private top-level binding. This is a proposed
  language rule
  ([type-checking.md §16](type-checking.md#16-open-questions-for-the-owner),
  question 1).

### 4.11 The Interface Blob

#### 4.11.1 Layout

```text
header      magic "HDIF", format version, toolchain key, folder stable path,
            flags, section count
sections    (kind, offset, length, Hash128) per section
strings     length-prefixed UTF-8, sorted, deduplicated
paths       StablePath records over string offsets
types       the InternPool's tag, data and extra columns, with blob-local operands
items       ItemRecord, in (module path, source order)
exports     (module, name) -> item, sorted by bytes, for binary search
private_names (module, name, kind, declaration anchor), sorted; no signature
impls       ImplRecord: head, bounds, bound plan, owner module, by-clause, api or heads-only
arg_impls   heads owned only through a trait argument, by (trait, target head key); own hash
templates   template headers; bodies as token text plus a resolution table
            (path node -> stable path in the trait's module)
hidden      hidden items and their private type closure, in the item record shape
item_hash   (item index, shallow Hash128, deep Hash128)
mentions    (folder stable path, deep hash) of every other folder named
anchors     declaration-relative TokenAnchor records for items and diagnostics
diags       stage-A header diagnostics, spans relative to their declaration (outside every api hash)
```

- Every section's row layout is in
  [data-structures.md §3.16](data-structures.md#316-the-folder-interface-in-memory-and-as-a-blob).
- Records are fixed-width, little-endian and 8-byte aligned, with `u32`
  offsets into the blob. A reader casts `&[u8]` to record slices after
  checking bounds and alignment once per section. Natively the blob is
  memory-mapped from the cache. In the browser it is a byte array. Std's
  blobs are embedded in the binary as one pack.
- `private_names` and `anchors` are outside `api_hash`, `deep_hash` and
  `heads_hash`. They affect diagnostics only.
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
- **Implementation gap (M1 finding 5).** M1 currently supplies every
  folder reached by F's `use`s to `deep_hash`, rather than only this
  `mentions(F)` set. That is sound but over-invalidates; interface
  construction must emit the mention set and the hash must fold only it.
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
pub struct ImplTable {                    // per module, SoA, frozen with its folder
    pub trait_: Box<[DefId]>,             // DefId::NONE for inherent impls
    pub def: Box<[DefId]>,
    pub head_key: Box<[HeadKey]>,         // packed u32: kind in the high bits
    pub arg_key: Box<[[HeadKey; 2]]>,     // head keys of the first two trait arguments; ANY when generic
    pub n_params: Box<[u8]>,
    pub head: Box<[HeadRef]>,             // target and trait arguments, as interface type records
    pub plan: Box<[PlanRange]>,           // the bound plan (trait-solver.md §3.6)
    pub assoc: Box<[AssocRange]>,         // associated-type bindings, by associated item DefId
    pub origin: Box<[ImplOrigin]>,        // Written | Derived | Delegated | Error | NumericFamily | TupleTemplate
    pub rank: Box<[u64]>,                 // content rank: (module path rank, source position)
    pub by_trait: HashMap<DefId, (u32, u32)>,  // an index: trait -> range of rows
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), TupleAny, Fn, SuspendFn, Param, Any }   // fast reject
```

Rows are sorted by `(trait rank, head key, arg key, rank)`, so candidate
order is content order. The probe and its fast reject are
[trait-solver.md §3.3](trait-solver.md#33-the-head-index) (change 12).

The columns' final form, sorted by the trait's path hash so candidate
order is content order, is
[data-structures.md §3.17](data-structures.md#317-impl-tables).

To solve `Target: Trait[Args]` with known arguments, look in the tables
of at most `2 + len(Args)` modules: the trait's, the target constructor's,
and each argument constructor's. Blanket impls (`impl[T] Tr for T`) can
only be in the trait's module. Local impls, which must involve a local
type or trait, are in a body-local table. Two additions cover what that
bound misses (trait-solver.md §3.2):

- **Built-in targets.** std may hold impls and inherent impls for
  primitives, `List`, `Map`, tuples, `Fn`, `Option` and `Result` in any
  of its modules. std's interface gathers all of them, inherent impls
  included, into one synthetic table, which lookup reads instead of an
  "owner" module.
- **Open trait arguments.** An impl owned only through a trait argument
  is listed in its folder's `arg_impls` section. Once per run, the driver
  merges those sections into a per-trait **candidate directory**, frozen
  before any body that needs it. A goal with an open argument reads the
  directory, filtered to the asking module's dependency closure (owner,
  2026-10-07). The deep hashes of that closure already cover those heads
  (§4.10, "Two hashes, two readers"), so the `check` key needs no extra
  hash. The solver memo keys the filtered view by an `ImplUniverseId`
  (trait-solver.md §3.2).

Every module consulted declares something the goal mentions, or holds a
directory row from the dependency closure, so it lies in the asking
module's deep dependency closure. Trait lookup therefore reads
nothing that the module's cache key does not cover. And the table a goal
needs is frozen as soon as its one folder's interface is, so bodies can
run while unrelated folders are still being resolved.

#### 4.12.2 Solving

The solver is designed in [trait-solver.md](trait-solver.md). In short:

- **Goals** are canonical: variables resolved, concrete projections
  normalized, the rest numbered by
  first occurrence (trait-solver.md §2.2).
- **Candidates,** in order: the parameter environment, a trait value as
  self, compiler-supplied impls of sealed traits, owner tables and the
  candidate directory, local impls. The solver commits on the one head
  that matches and never backtracks among impls (rule TS-2).
- **Cycles.** A goal already on the stack is `Overflow`, reported as
  `trait-resolution-depth`. Derive instances need no coinductive
  assumption: the derived head is in the table, so the member check
  meets no cycle (trait-solver.md §3.10).
- **Memo.** Keys hold the canonical goal plus the environment, the
  visible local impls and, for `Methods`, the availability key. Only
  completed, context-free answers are published to the run's global
  memo; depth is a stored height; fuel and depth exhaustion are never
  cached as failures (trait-solver.md §7.1 to §7.3).
- **Budget.** The first time a body meets a goal it pays the goal's
  intrinsic cost, and 1 on each repeat, so whether a goal runs out
  depends neither on which body computed it first, nor on the thread
  count, nor on cache warmth (trait-solver.md §7.4).
- **Method lookup.** Inherent methods first, then the methods of traits
  available in the module
  ([Trait Availability](../../spec/lang/09-traits.md#trait-availability)),
  one goal per trait that declares the name (trait-solver.md §6.5).

#### 4.12.3 Coherence

- Overlap is decided per trait from heads alone
  ([Overlap](../../spec/lang/09-traits.md#overlap)).
- One `Coherence(trait)` task runs per trait with more than one impl in
  the program's graph (root package, dependencies, std). Its input is that
  trait's heads from the `heads` sections of every folder interface,
  derived, generated and delegated heads and tuple templates included.
- **Near-linear, not pairwise (change 14, review T6).** Numeric-family
  heads expand to their members first. All ground heads go into a hash
  set and a ground trie first. Then each generic head, in content order,
  queries the generic trie (a discrimination tree with impl parameters as
  wildcards) and the ground trie before it is inserted, so the result
  does not depend on source order (Codex re-review N-T6). Each candidate
  pair is confirmed by unification after renaming apart
  ([trait-solver.md §5.2](trait-solver.md#52-the-overlap-check)). Heads
  that differ at a constructor cost time linear in their total size,
  not one check per pair.
- **Known M3 simplification (M3 gap 6).** M3 sorts heads per trait and
  compares each later head with every earlier head. It reports the right
  first overlap, but has quadratic work. Replace this pairwise loop with
  the two-trie algorithm above; do not preserve it as an alternative
  coherence design.
- An overlap is reported once, at the later impl in content order
  (package, module path, item index), naming the earlier one and a **witness**
  type that both heads match, such as "both apply to `Box[Plain]`".
  The task stops after the first overlap per impl.
- The task's key is the trait's stable path plus the sorted head hashes,
  so it is cached and reruns only when a head of that trait changes. A
  head hash covers the head and its content rank, since the rank picks
  the reported impl (trait-solver.md §5.3). Its
  spans are relative to their declaration (review A3).
- `hd check` runs coherence over the whole graph, not only at link time
  ([`module.interface.coherence`](../../spec/lang/10-modules.md#r-module.interface.coherence)).
  It is cached and cheap, and agents see the error one command earlier.
