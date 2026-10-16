# New Compiler Design: Resolution And Interfaces

Status: design record, checked against the code at 3209d690. "Implementation
differs" notes mark where the code falls short of the design. The code and
the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

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

**Use roots.** `hd_project` owns one mapping, `module_path` (with
`module_below` for the path below the root): the package identifier
(each `-` as `_`, e.g. `acme-shop` → `acme_shop`), then the path below
the source root. `src/lib.hd` is the root module itself, `src/x/mod.hd`
is `x`, and a flat `main.hd` is `main`. Integration test programs,
shared test modules and tasks have no user-visible module path
(`module.test.integration.no-path`, `cli.task.no-path`):
`src/tests/checkout.hd` is `pkg.tests.checkout`, and `tests/checkout.hd`
is named by its file. Internally they get segments no source path can
spell (`$tests`, `$tasks`), and `user_text` names them by file in
messages.

Each module also records its relative base (its root for a root file:
`src/lib.hd`, `src/main.hd`, or a file directly under `tests/` or
`tasks`), so `self` starts there, and its floor (the package root, or the
test/task root), which a root `super` must stay within. Entry modules
(`src/main.hd`, test programs, tasks) are their own program, which no
`use` reaches. `UseRoots::absolute` maps `pkg`, `std`, `dep.NAME`, `self`
and `super` and rejects any other first segment; an unknown `dep.NAME`,
a `super` above the floor, and a dev dependency named from non-test code
are their own errors. Dependencies load only their library (test code
and executables never build), `requires` maps each `dep.NAME` to its
package, and a manifest loop is `package-cycle`. A package with `src/`
takes files only from `src`, `tests` and `tasks`.

The module set always comes from this scan, never from cache entries, so a
deleted file is never served from the cache (Gleam #4320).

### 4.8 Folder Graph

1. For each non-test `use` in a file of folder `A`, find the module its
   path reaches: the longest prefix of the path that names a module.
   `use pkg.shop.Item` and `use pkg.shop.{Item}` both reach `shop`
   ([`module.cycle.folder-edge`](../../spec/lang/10-modules.md#r-module.cycle.folder-edge)).
2. Add the edge `A -> folder(module)` when the folders differ and the
   module is in the same package. Fixed prelude uses participate exactly
   like written uses, except that the virtual `std.core` module does not
   import the prelude.
3. Run Tarjan's algorithm with folders visited in path order, so the SCCs
   and their order are deterministic.
4. For each SCC with more than one folder, report `folder-cycle` once: the
   shortest loop found by breadth-first search from the SCC's least
   folder, the `use` that makes each edge, the tangle size (the SCC's
   size), and the fix-it that moves the first file `x.hd` on the loop to
   `x/mod.hd` ([Cycle Diagnostic](../../spec/lang/10-modules.md#cycle-diagnostic)).
   The diagnostic's content is not built yet; see #171.
5. **Recovery.** The folders of a cyclic SCC are resolved together as if
   they were one folder. Bodies still check, and the cycle is the only
   error. Before scheduling, the driver skims the root package, finds its
   folder cycles, and merges each cycle's folders into one folder of the
   module table (`ModuleTable::merge_folders`, named after the first in
   path order), repeating until the graph is acyclic. The interface,
   cache keys and scheduling then see one ordinary folder.

Implementation differs: `FolderGraph::build`
(`crates/hd_project/src/lib.rs:1117`) takes its edges from written `use`
lines only. Leaving out prelude edges matters only inside std; not
judged.

`tests:` blocks make no edges: they and doc tests are checked in a
**test overlay** after the folders they use
(`crates/hd_tir/DESIGN.md` §4.13.9). Discovery puts every test module of
a package, wherever its file lies, in one folder, `PKG.$tests`, so no
library folder holds test code, and that unit may loop internally. A use
of a test module from non-test code is an error and makes no edge. Integration test
programs are their own roots. The package graph comes from manifests,
and `package-cycle` is found the same way.

### 4.9 Name Resolution

Resolution runs inside the `FolderIface` task for headers, and inside
`ModulePrep` for the module's private signatures and bodies.

**Module scope.** The prelude, the module's uses, and its own top-level
declarations, with the shadowing and duplicate rules of
[Module Scope](../../spec/lang/03-names-and-scopes.md#module-scope).
`ModuleScope` (`src/lib.rs`) holds `(name, binding, origin)` rows; a
binding is 8 bytes, a kind (`Item`, `Module`, `Prelude`, `Ambiguous`,
`Poison`) and a value.

**Uses into other folders.** The target folder's interface is complete and
frozen. Its export index maps a name to the **real declaration**: a
`pub use` re-export is followed to its end when the interface is built.
A dependent never follows chains. The folder that declares the item is
a mention of the re-exporting interface, so the deep hash covers it
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
  The interface keeps the looping names (`loop_names`), so a use through
  another folder fails quietly.
- The work is bounded by the folder's uses times the chain length.
- Other use errors: `unknown-module`, `unknown-import`, `private-import`,
  `direct-variant-use`, `ambiguous-import`, `test-only-use`.

**Failed uses bind poison.** A failed use (unknown module, unknown
import, private import, bad root, ambiguous import) still binds every
name it would have bound, as `BindingKind::Poison`, so later references
resolve to something and stay quiet. `ModuleScope::bind` gives poison
both directions of precedence: a poison binding never overwrites a real
one, and a real binding replaces poison. The checker reports nothing at
a poison reference: one error at the `use`, silence after.

**Private-name index.** Each interface carries the `(module path, name)`
of every top-level declaration without `pub` (`private_names`), as its
own cache-entry section outside every hash. A failed export lookup
checks it: a matching row reports `private-import`; no row reports
`unknown-import`. Dependents never see private items, and a private
header edit does not invalidate them.

**Paths in types and expressions** resolve through the module scope:
module aliases, qualified names, `Self`, type parameters, and the names
behind literal suffixes and string prefixes
([Forms Resolved By Name](../../spec/lang/02-grammar.md#forms-resolved-by-name)).

**Ownership.** `impl_placement` (`src/lower.rs`) takes the module of the
trait, of the target's outer constructor and of each trait argument's
outer constructor. No owner in the impl's module is `nonlocal-impl` when
an owner is in the package, `orphan-impl` otherwise
([Implementation Ownership](../../spec/lang/09-traits.md#implementation-ownership)).
The same facts make trait lookup local (§4.12.1).

### 4.10 Folder Interface Construction

```rust
pub struct FolderIface {
    pub folder: FolderId,
    pub blob: Arc<[u8]>,                     // the canonical bytes; also the cache payload; mapped when read from disk
    pub reader: IfaceReader,                 // typed views over the blob's sections (§4.11.1)
    pub api_hash: Hash128,                   // shallow: the blob's api sections
    pub deep_hash: Hash128,                  // §4.11.3
    pub heads_hash: Hash128,                 // every impl head, private targets included
}
```

The in-memory interface is its blob: the builder writes the sections as
struct-of-arrays records, and every reader, the building run included,
reads them in place. There is no second, decoded copy.

Implementation differs: `FolderIface` (`src/iface.rs:652`) holds decoded
`items` and `exports` vectors with hash-map indexes, the blob, `api_hash`,
`deep_hash`, anchors (`spans`), `private_names`, `loop_names` and a REPL
input's `module_uses`. A cache hit decodes every item
(`decode_items`, `src/iface.rs:1146`). This is worse: a run that names a
few items of a large std folder decodes all of them, and every interface
exists twice in memory.

**Algorithm** for folder `F`, once the interfaces of the folders it uses
are frozen (`build_folder`, `src/lower.rs`):

1. Collect the items of `F`'s modules.
2. Resolve uses (§4.9).
3. Lower every header to interface types: generics with bounds, variance
   and defaults; parameters with default presence; results; rows,
   normalized after alias expansion; `!`; fields; variants with payloads;
   trait members, associated types, supertraits and default presence;
   impl heads with `by` delegation; aliases and newtypes.
   While lowering a written trait reference, fill omitted trailing trait
   arguments from the declaration's defaults in declaration order
   (`fill_trait_args`). Substitute earlier arguments and the reference's
   `Self`: the bounded type in a bound, or the target type in an impl
   head. Thus a bound or head written `Add` lowers as `Add[Self]` for the
   declared `Rhs = Self` default.
   While lowering a `mut T` type, a type parameter with no written bound
   list is `mut-on-type-parameter` (`types.generic.no-mut-t`); a parameter
   with any bound list passes, and `mut Self` is exempt.
4. Build derived heads. For `@derive(X)`, the impl of `X` with `T < X` for
   each type parameter in a walked member
   ([Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds)). For
   `@error`, `@from` and `@source`, the `Display`, `Error` and `From`
   heads.
5. Run the header checks of stage A in §4.10.1: everything that reads
   only headers, F's own and its dependencies'.
6. Write the blob, then compute `api_hash` and `deep_hash`.

Each run builds a folder's impl table and its `arg_impls` rows from the
interface's impl items (`impl_table`, `src/iface.rs`; `FolderImpls` in
`crates/hd_types/src/lookup.rs`), including each impl's bound plan. A
trait's `dyn` availability is computed on demand from its items. Both
are cheap to derive, so the interface does not store them.

After step 6 the interface is usable: dependents may read it. The
stage-B checks of §4.10.1 run next, in their own task, and no dependent
reads their result.

Planned, not built: validate on write. Every `DefId` the api sections
mention is exported from its folder, or is a hidden item of this folder;
a failure is an internal compiler error naming the item. Debug builds
decode the written bytes and compare them with the in-memory interface.

**Row aliases and context types in interfaces.** A row alias is an alias
item whose body is a row (`TyData::Row`). Every use in a header expands
first (`req.row.alias.expand.first`), recursively, so headers store only
expanded normalized rows; all aliases of a folder lower first, in
dependency order. At use sites the same expansion runs before
normalization, entailment and solving. Cycles are `alias-cycle`
regardless of declaration order. `$.Context[$ R]` is its own type form,
`TyData::Context(RowId)`, with its row argument expanded. Expanded rows
hash by stable-sorted content, so two spellings of one row share cache
entries. Diagnostics print the row as written plus the expanded keys,
naming the missing one (`req.row.union.literal.diagnostics`,
`req.row.alias.diagnostics.expanded`).

**What the interface holds.** Public items, all impl heads, templates with
their bodies, hidden items, and per parameter and field whether it has a
default. It holds no private item record except hidden ones. It holds no
ordinary body, doc comment or fact value.

**Top-level bindings.** An ordinary top-level binding is
module-initialization state, not an interface item kind. Bodies in its
declaring module may read it, where the body checker assigns its global
slot and type, but another module cannot import or refer to it. So
`FolderIface` carries neither its declaration nor its initializer.

**Declaration locations.** Every item, impl head and stored header
diagnostic carries an anchor (`src/anchor.rs`) relative to its
declaration, kept in its own entry section outside every semantic hash.
Stage B and coherence resolve an anchor through the current file, never
the module file's zero offset.

- **Defaults are not in the interface as expressions.** A caller emits a
  call of the default's own body with the earlier arguments
  ([crates/hd_check/DESIGN.md (type checking) §1.7](../hd_check/DESIGN.md#17-body-tasks-and-the-exactly-once-rule)),
  so it needs only the parameter's type and the fact that a default
  exists.
- **Facts carry their type only.** A check needs only a fact's type.
  Values are computed once, at compile time, by the backend's evaluator.
- **Hidden items and their closure.** A template body may name private
  items of its trait's module. The interface holds each such item's
  record in the `hidden` section, plus, transitively, every private type
  its signature names, with that type's fields, variants and the methods
  the template body names. All of it comes from headers and the
  template's resolution table, never from a body, because every hidden
  function has a written result type and its omitted `$` clause means
  the empty row (§4.10.1). So a hidden helper's body edit changes no
  interface byte. User code cannot name a hidden item.

Implementation differs: interfaces carry no templates and no hidden
items, and the template body is read from source. Keys stand in for
the blob: a module's check key and `HeaderCheck` key take the template
text hash of each user template module in the closure
(`template_hashes`, `crates/hd_driver/src/lib.rs:4169`). That hash
(`template_text_hash`, `crates/hd_syntax/src/skim.rs:347`) covers the
module's API text and top-level statements, every method body of an
implementation written with `by` (the template's own), and every body
whose function a caller sees inferred parts of: an omitted result, or a
private function's omitted row. A check key takes a derivation
template's hash only when the module's text names `derive` or
`Structure` and a name of the template's trait; it takes every tuple
template's. The hidden-helper result rule is
checked in the template module's body task (`Results::template_names`,
`crates/hd_check/src/results.rs:140`), not in stage A. The row rule is
not: a hidden helper's omitted row is inferred from its body, so the
hash keeps such bodies. This is worse: a header edit anywhere in a
template's module, or a body edit of a private helper that omits its
row, rechecks every module that derives one of the module's templates.
A body edit elsewhere in the module rechecks no other module.

**Two hashes, two readers.** `api_hash` and `deep_hash` cover what a
dependent's check can read: public items, and impl heads whose target
and trait are both nameable outside their module. Impl heads for private
types go only into `heads_hash`, which coherence reads (§4.12.3). A
dependent cannot name a private type, and a private type cannot appear in
a public signature
([`module.vis.signature`](../../spec/lang/10-modules.md#r-module.vis.signature)).
So adding `impl Display for PrivateThing` rechecks no dependent.

The rule covers impls owned through a trait argument too.
`impl Pick[Product] for Receiver` in `Product`'s module has a nameable
trait and target, whatever `Product`'s visibility, so its head is in
`api_hash`. A module's `check` key holds the deep hash of every folder in
its dependency closure, the same closure that filters the candidate
directory (§4.12.1). So the deep hashes already cover every
argument-owned impl a check can see.

Implementation differs:
- **The blob.** It still carries every impl (`interface_items`).
- **The two hashes.** `heads_hash` is the hash of the whole blob.
  `api_hash` is the hash of the blob re-encoded without
  `private_impl_items`: the impls whose trait, or whose target's head
  type, is a private declaration of the folder, with their members. The
  driver stores that hash in the interface entry.
- **The readers.** Coherence and the folder's own header check key on
  `heads_hash` too.
- **Conservative.** A private type in an impl's arguments, as in
  `impl Show for List[Private]`, still counts toward `api_hash`.

**Complexity.** Linear in the folder's header tokens plus the use worklist.
A folder task cannot be split, so one very large folder bounds the wall
time of a cold run.

#### 4.10.1 Header Validation Stages

Every header rule runs in exactly one of three stages:

| Stage | Task | Reads | Rules |
| --- | --- | --- | --- |
| A | `FolderIface(F)`, step 5 | F's headers, its dependencies' interfaces | duplicate declarations; `private-type-leak`; `orphan-impl`; `nonlocal-impl`; `bare-parameter-impl-target`, `trait-value-impl-target`, `mutable-impl-target`, `invalid-impl-target`; `unconstrained-impl-parameter` (projections included); `sealed-trait-implementation`; `ambiguous-row-pattern`; `misplaced-derivation`; a second template or a derive beside a written impl (`overlapping-impl`); `mixed-derived-law`; `duplicate-inherent-member`; `missing-result-type`; `default-order`; `generic-kind-mismatch`; cycles of aliases and supertraits (`supertrait-cycle`); `ambiguous-associated-type`; declared variance against every use in fields, payloads and methods, private inherent methods included; impl members against the trait: completeness (`missing-trait-method`), signature equality, associated-type bindings complete |
| B | `HeaderCheck(F)`, after F's interface and its dependencies' interfaces | the solver over frozen impl tables | written types against their declared bounds, for every type written in a header (a `Set[K]` needs `K: Hash` from the item's own bounds: [`trait.bound.no-implied`](../../spec/lang/09-traits.md#r-trait.bound.no-implied)); supertraits of each impl and supertrait bindings (`missing-supertrait-implementation`); a derived newtype's base impl; a delegation target's impl |
| C | body tasks | bodies | derive-instance member obligations; local impls; everything in [crates/hd_check/DESIGN.md (type checking)](../hd_check/DESIGN.md) |

- **Why stage B is a task of its own.** Its goals need the impl tables of
  F and of every dependency. Its answers never change what a dependent's
  check reports, so dependents do not wait for it. A run with a stage-B
  error fails as any run with an error does, and codegen waits for every
  `HeaderCheck` task of the program graph.
- **Dependencies are checked too.** Stages A and B run for every folder
  of the program graph, std and dependencies included, whether or not
  the run checks that folder's bodies. So an imported impl that misses a
  member or a supertrait is reported, in the dependency's file, even by
  `hd check`.
- **Private items.** The interface holds no private item record, so
  `HeaderCheck(F)` checks only the items whose header names nothing but
  interface items. A private item, and an impl or public item that names
  one, gets its stage-B checks in its module's body task
  (`hd_check::header::private_items`). Each item is checked in exactly
  one of the two places.
- **Caching.** Stage A's diagnostics are their own section of the
  interface entry, outside every hash. Stage B's are the `HeaderCheck(F)`
  task's result; its key is F's interface key plus the deep hashes of the
  folders in F's dependency closure
  ([crates/hd_cache/DESIGN.md](../hd_cache/DESIGN.md)).
- **Fuel.** Each item's stage-B goals share one fuel budget, as a body's
  do, so one pathological header fails only itself.
- **The hidden-helper rule.** A private function that a template body
  names must write its result type (`missing-result-type`), and without
  a `$` clause it has the empty row, as a public function does. A
  template may not name a private top-level binding.

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
private_names (module, name), sorted; no signature
impls       ImplRecord: head, bounds, owner module, by-clause, api or heads-only
templates   template headers; bodies as token text plus a resolution table
            (path node -> stable path in the trait's module)
hidden      hidden items and their private type closure, in the item record shape
item_hash   (item index, shallow Hash128, deep Hash128)
mentions    (folder stable path, deep hash) of every other folder named
anchors     declaration-relative anchors for items and diagnostics
diags       stage-A header diagnostics, spans relative to their declaration (outside every api hash)
```

- Every section's row layout is in
  [docs/compiler/data-structures.md §3.16](../../docs/compiler/data-structures.md#316-the-folder-interface-in-memory-and-as-a-blob).
- Records are fixed-width, little-endian and 8-byte aligned, with `u32`
  offsets into the blob. A reader casts `&[u8]` to record slices after
  checking bounds and alignment once per section.
- `private_names` and `anchors` are outside `api_hash`, `deep_hash` and
  `heads_hash`. They affect diagnostics only.
- **Lazy decode.** `IfaceReader::item(name)` binary-searches the export
  index and decodes one record. A run that names 5 items of a 200-item
  std folder decodes 5 items.
- **Canonical bytes.** Records are ordered by stable content: module path,
  source order, sorted names. Never by run IDs or hash-map order. Two runs
  on the same inputs write the same bytes.
- **Templates.** A template body is checked again at each opt-in, in the
  deriving module ([`annot.template.checked`](../../spec/lang/14-annotations.md#r-annot.template.checked)),
  but its names resolve in the trait's module. So the blob stores the body
  as token text plus the resolution table.

Implementation differs: the interface entry is six sections
(`crates/hd_driver/src/lib.rs`, the `EntryKind::Iface` put): the items
blob (`encode_items`: entry-local string, path and type tables, then
items and exports sorted by module and name), stage-A diagnostics,
anchors, private names, loop names, and REPL module uses. Items are
variable-length records decoded all at once. This is worse for the same
reason as §4.10: no lazy decode. Canonical bytes hold.

#### 4.11.2 Per-Item Hashes

Planned, not built: each item has a shallow hash (its record and the
type records it reaches) and a deep hash folded per SCC of the folder's
item-mention graph, stored with the interface. Keys stay per folder; the
stored hashes would let a later switch to keys per used item (Pyrefly,
gopls typerefs) change keys, not the format.

#### 4.11.3 The Deep Hash

```text
deep_hash(F) = H("deep", api_hash(F), sorted [(path(G), deep_hash(G)) for G in mentions(F)])
```

- `mentions(F)` is every other folder, in any package or in std, whose
  items `F`'s interface names: in signatures, field types, bounds, impl
  heads and re-export targets (`mentioned_defs`, `src/iface.rs`).
- The folder and package graphs are acyclic, so this is a Merkle hash
  computed bottom-up in the order the tasks already run. A merged cycle
  is one folder.
- **What it fixes.** A module that calls `service.load_user()` and reads
  `.name` never names `user/types`. Its key holds the deep hash of
  `service`'s folder, which mentions `user/types`. An edit to `User`
  changes the key.

### 4.12 Traits, Impls And Coherence

#### 4.12.1 Owner-Module Impl Tables

hd's ownership rules say exactly where an impl may live: the module that
declares the trait, the target's outer constructor, or a trait argument's
outer constructor ([Implementation Modules](../../spec/lang/09-traits.md#implementation-modules)).
So no global impl index is needed. Each folder gets an `ImplTable`
(`crates/hd_types/src/solver.rs`), built once per run from its interface:

```rust
pub struct ImplTable {                    // per folder, SoA
    pub trait_: Vec<DefId>,
    pub def: Vec<DefId>,
    pub head_key: Vec<HeadKey>,
    pub arg_key: Vec<[HeadKey; 2]>,       // head keys of the first two trait arguments; Any when absent
    pub n_params: Vec<u8>,
    pub head_self: Vec<Ty>,
    pub head_args: Vec<TyList>,
    pub plan: Vec<Vec<PlanStep>>,         // the bound plan
    pub assoc: Vec<Vec<(DefId, Ty)>>,     // associated-type bindings
    pub origin: Vec<ImplOrigin>,          // Written | Derived | Delegated | Error | NumericFamily | TupleTemplate
    pub rank: Vec<u64>,                   // content rank
    pub by_trait: BTreeMap<u32, (u32, u32)>,  // trait -> range of rows
    pub by_key: Vec<u32>,                 // head index: rows per trait sorted by head key
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), TupleAny, Fn, SuspendFn, Param, Any }   // fast reject
```

Rows are sorted by the trait's path hash, then the impl's, so candidate
order is content order. `candidates(trait, key)` returns the exact
head-key bucket plus the `Param` rows and, for a tuple, the `TupleAny`
rows. The column layout is in
[docs/compiler/data-structures.md §3.17](../../docs/compiler/data-structures.md#317-impl-tables).

To solve `Target: Trait[Args]` with known arguments, look in the tables
of at most `2 + len(Args)` folders: the trait's, the target
constructor's, and each argument constructor's. Blanket impls
(`impl[T] Tr for T`) can only be in the trait's module. Block-local
impls are rows of their module's own table, hidden from contexts that do
not see them. `FolderImpls` (`crates/hd_types/src/lookup.rs`) sorts each
folder's other rows into two lists that cover what owner lookup misses:

- **Unowned rows.** std's impls for built-in targets kept in another std
  folder, and impls the ownership check rejected, which stay visible so
  their uses add no second error.
- **Open trait arguments.** An impl owned only through a trait argument
  is in its folder's `arg_impls` list.

An **impl universe** merges both lists over the folders of a dependency
closure that hold any (`UniverseImpls`): the synthetic unowned table and
the per-trait **candidate directory**. A goal with an open argument reads
the directory of its own closure. The deep hashes of that closure already
cover those heads (§4.10, "Two hashes, two readers"), so the `check` key
needs no extra hash. The solver memo keys the view by its
`ImplUniverseId`.

Every folder consulted declares something the goal mentions, or is in
the asking module's dependency closure. Trait lookup therefore reads
nothing that the module's cache key does not cover. And the table a goal
needs is ready as soon as its one folder's interface is, so bodies can
run while unrelated folders are still being resolved.

#### 4.12.2 Solving

The solver is designed in
[crates/hd_check/DESIGN.md (trait solver)](../hd_check/DESIGN.md#new-compiler-the-trait-solver)
and built in `crates/hd_types/src/solver.rs`. Candidates come, in order,
from the parameter environment, a trait value as self, compiler-supplied
impls of sealed traits, owner tables and the candidate directory, and
local impls. The solver commits on the one head that matches and never
backtracks among impls. A goal already on the stack is `Overflow`,
reported as `trait-resolution-depth`.

#### 4.12.3 Coherence

- Overlap is decided per trait from heads alone
  ([Overlap](../../spec/lang/09-traits.md#overlap)).
- One `Coherence(trait)` task runs per trait with more than one impl in
  the program's graph (root package, dependencies, std). Its input is that
  trait's heads from the `heads` sections of every folder interface,
  derived, generated and delegated heads and tuple templates included.
  The task's key is the trait's stable path plus the sorted head hashes,
  so it reruns only when a head of that trait changes. A head hash covers
  the head and its content rank, since the rank picks the reported impl.
  Built as one `Coherence` task keyed by every built folder's deep hash;
  per-trait keys are not built yet, see #177.
- **Near-linear, not pairwise.** Numeric-family heads expand to their
  members first. All ground heads go into a hash set and a ground trie.
  Then each generic head, in content order, queries the generic trie (a
  discrimination tree with impl parameters as wildcards) and the ground
  trie before it is inserted, so the result does not depend on source
  order. Each candidate pair is confirmed by unification after renaming
  apart ([crates/hd_check/DESIGN.md (trait solver) §5.2](../hd_check/DESIGN.md#52-the-overlap-check)).
  Heads that differ at a constructor cost time linear in their total
  size, not one check per pair.
  Implementation differs: `Universe::overlaps` keeps no tries.
  - **Keys.** It sorts each trait's heads in content order and keys each
    head by the outer constructor of its target and of its first trait
    argument (`head_key`). A parameter, or a tuple with a rest, is a
    wildcard.
  - **Candidates.** It unifies a head only with the earlier heads whose
    keys can match its own, in the same order, so it reports the
    pairwise result.
  - **Cost.** Many impls for distinct types cost near-linear time.
    Generic targets still compare with every earlier head.
- An overlap is reported once, at the later impl in content order (std
  first, then generated `@error` impls, written impls, `@derive`,
  derivation blocks; then module path and position), naming the earlier
  one and a **witness** type that both heads match, such as "both apply
  to `Box[Plain]`". A head written for a tuple also overlaps a tuple
  template.
- `hd check` runs coherence over the whole graph, not only at link time
  ([`module.interface.coherence`](../../spec/lang/10-modules.md#r-module.interface.coherence)).
  It is cached and cheap, and agents see the error one command earlier.
