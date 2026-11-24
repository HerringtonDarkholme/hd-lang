# Module Graph And Visibility

Status: accepted by the owner, 2026-11-24, with the changes under Owner
Decisions; the final rule is in the specification (ch. 10, Dependency
Cycles) and built in #2347. The rest of this record is the design as first
proposed.
Domain: modules (spec ch. 10), std layout, the compiler's interface unit. Issues
[#2347](https://github.com/HerringtonDarkholme/hd-lang/issues/2347) and
[#2346](https://github.com/HerringtonDarkholme/hd-lang/issues/2346).
Stress test: [MODULES_STRESS_TEST.md](MODULES_STRESS_TEST.md).

## Owner Decisions

Owner, 2026-11-24 ("approved", "please build"):

1. Question 1, edges go to the declaring module: as recommended.
2. Question 2, a flat `x.hd` is in its children's folder: **yes**, as
   module.folder.parent-file says today, overriding the recommendation.
   `x.hd` and `x/mod.hd` stay interchangeable; no fix-it moves `x.hd`, and
   `folder-parent-file` keeps its expectation.
3. Question 3: **no `pub(super)` now** ("no need to add pub(super) now").
   Instead, the owner's original design: **child modules are private by
   default** ("always had children private"). A nested module is visible only
   within its parent's subtree; outside code reaches its items through the
   parent's `pub use`. The agent-written chapter 10 never stated this.
4. Question 4, the compiler unit is the sibling module loop: as recommended.
5. Question 5, `StringBuilder` stops holding `UniqList[u8]`: as recommended.

The owner's rule throughout: no module cycles, unless the modules are
siblings in one folder.

**Final rule** (owner, 2026-11-24, during the build; it replaces decisions 1
and 2 and phase 1's folder rules):

1. A `use` makes an edge to the module it **names**, even when that module
   only re-exports the item: `use std.collections.UniqList` is an edge to
   `std.collections`. This reverses Question 1.
2. A `pub use` makes **no** edge.
3. A loop is legal only when one **sibling group** holds all its modules:
   module P's group is P and its children. Folders play no part; `x.hd` and
   `x/mod.hd` stay interchangeable.
4. **Privacy is uniform** (decision 3): every module is visible only within
   its parent's subtree; other packages reach items only through the
   package root's `pub use`.

**Applied** (lane 268, 2026-11-24): module re-export `pub use M.{child}`
(`module.use.facade.module`; it publishes the child and binds no name),
nested-module privacy (`module.vis.nested`, error `private-module`), and
top-level privacy (`module.vis.top-level`): another package reaches only
what its root re-exports. #2356 was closed ("not needed"), so std keeps its
layout and gains a root file, `lib/std/mod.hd`, re-exporting every top-level
module but `std.rt`. A directory without `mod.hd` re-exports nothing, so its
modules are visible only inside it (`module.vis.nested.folder`). Outside
`std.rt`, std names only `std.rt` (#2355), which re-exports the runtime
names the rest of std uses.

The compiler keeps two graphs. The policy graph above decides
`module-cycle`, on its strongly connected groups. The dependency graph,
every use including `pub use`, orders initialization and forms the compile
units. Question 5's `StringBuilder` change is no longer needed: `text`'s
edge stops at `collections`.

## The Proposal

The owner's rule (2026-11-24): "no module cycles, unless modules are in the
same folder siblings". This design turns it into spec rules, a diagnostic,
a visibility form for helpers shared inside a folder, and a compiler unit.

- **Nodes are modules, never folders.** An edge goes from a module to the
  module that *declares* the name a `use` line reaches, through any
  `pub use` chain. Test code makes no edge, as today.
- **A loop is legal only among siblings**: modules whose source files sit in
  one directory. `shop/mod.hd` is in `shop/` with its children; a flat
  `shop.hd` is in its own parent directory.
- **Any other loop is `module-cycle`.** The diagnostic shows the real `use`
  lines of one shortest loop, and offers a fix-it when moving `x.hd` to
  `x/mod.hd` makes the loop siblings.
- **`pub(super)`** makes a child's helper visible to its parent module and
  the parent's subtree, so a `collections/` helper stays in `collections`.
- **The compiler unit becomes a loop of modules** (one strongly connected
  group, always inside one folder), in dependency order.

```text
# src/shop/mod.hd: the facade
pub use pkg.shop.cart.{Cart}

# src/shop/cart.hd and src/shop/item.hd use each other: siblings, legal
use pkg.shop.item.{Item}

# src/shop/orders/mod.hd: shop uses orders and orders uses shop
use pkg.shop.{money} # error: module-cycle

# src/collections/sort.hd: visible in collections and below, nowhere else
pub(super) fn drift_sort(len: i32) -> i32: # hypothetical syntax
    len
```

## Open Questions

1. Does a use through a facade make its edge to the declaring module? **Recommended: yes**; otherwise `lib.hd` re-exporting `handlers` while `handlers` names `pkg.{AppError}` is a loop (stress test P2).
2. Is a flat `x.hd` in its children's folder, as module.folder.parent-file says today? **Recommended: no**, the literal reading; `x/mod.hd` is, and the fix-it moves `x.hd` there (P1).
3. Which visibility shares a helper inside a folder? **Recommended: `pub(super)`** (Rust); alternatives are Go's `internal/` directory, folder-wide private names, and children reading their parent's private items (table below).
4. What is the compiler unit? **Recommended: the module loop**, not folders merged on a loop; merging would weld std's root folder and `collections/` into one unit (Appendix C).
5. How does `collections` leave std's root loop for #2346? **Recommended: `StringBuilder` stops holding `UniqList[u8]`**, its one edge back (P3); this is a std change, priced in the stress test.

## Problems It Solves

| Program | Today | Proposed |
| --- | --- | --- |
| `parentB -> parentC/useParentA -> parentA`, no module loop | `folder-cycle: src/main -> src/parentC/useParentA -> src/main`; `src/main` is on no edge, and the fix-it moves `main.hd` | checks |
| `lib.hd` re-exports `collections/list.hd`, which uses `iter.hd` | `folder-cycle: src/iter -> src/collections -> src/iter` | checks |
| `lib.hd` re-exports `handlers`, `handlers/mod.hd` uses `pkg.{AppError}` | `folder-cycle: src/error -> src/handlers -> src/error` | checks (edge goes to `error`) |
| `std/collections/{mod,list,sort}.hd` sharing `drift_sort` | `private-import` on each helper (#2346 report) | `pub(super)` helpers; loops inside `collections/` legal |
| `shop/mod.hd` and `shop/orders/mod.hd` use each other | `folder-cycle` | `module-cycle`, naming both `use` lines |

Probes on main 01de0e532 (`hd check`); sources in Appendix D.

## How It Works

**Edges.** Resolution already follows every `pub use` chain to its
declaration (`module.pub-use.chain`). The edge for a use is taken after
that: `use pkg.{AppError}` through `lib.hd` is an edge to `error`. A
`pub use` line is itself an edge from the facade to the declarer. A
namespace use (`use pkg.shop`) is an edge to `shop`.

**Siblings.** A module's folder is the directory of its source file:
`src/shop/mod.hd` and `src/shop/cart.hd` are in `src/shop`; `src/shop.hd`
and `src/lib.hd` are in `src`. Rules module.folder.* and the folder graph
go away; only this one-line notion stays.

**Check.** Tarjan's algorithm over the module graph, iterative, gives the
strongly connected groups (O(modules + edges), the cost of today's folder
graph). A group with modules in two folders is an error. For each, the
diagnostic takes the group's first module `u` in path order and the first
edge `w -> u` from another folder, then a breadth-first search from `u`
back to `w` inside the group: the shortest loop through that edge, in
O(group + its edges). It is reported at that edge's `use` line.

```
error: src/shop/orders/mod.hd:1:1: module-cycle: modules of different folders use each other
  src/shop/mod.hd:1:1: pub use pkg.shop.orders.{order}   shop -> shop.orders
  src/shop/orders/mod.hd:1:1: use pkg.shop.{money}       shop.orders -> shop
loop of 2 modules in 2 folders; only modules of one folder may loop
help: move `money` out of `shop`, into a module both use
```

The help names the cross-folder edge whose `use` line names the fewest
declarations. There is no fix-it that moves a flat `x.hd` (decision 2):
`x.hd` is already in the folder `x/` of its children, so std's `testing.hd`
and `testing/arbitrary.hd` are siblings and their loop is legal.

**Visibility.** Four candidates, each checked against the sibling rule:

| Form | Helper visible to | Sibling rule | Change kind |
| --- | --- | --- | --- |
| `pub(super)` (Rust) | parent module and its subtree | fits: no new edge needed | syntax (needs approval) |
| `internal/` directory (Go) | the subtree of `internal`'s parent | fits, unless `internal/` and a user loop | semantic rule |
| folder-wide private names (Go package) | every file of the directory | fits, but widens every existing private item | semantic rule |
| children read parent's private items (Rust) | the parent's subtree | helpers must move into `mod.hd`; `x.hd` form loops | semantic rule |

**Recommendation:** `pub(super)`. It is the owner's "only visible within
collections" with the helper left where it is, and it widens nothing that is
private today. The Design Cost Order ranks syntax last; `internal/` is the
cheapest form that also fits, at the price of a reserved directory name.

**Compiler unit.** Folder interfaces (`FolderIface`), impl tables and the
scheduler key on folders today. Folders can now depend on each other in a
loop without any module loop, so they are no longer a DAG. Each module loop
becomes the unit, in dependency order: an acyclic module is its own unit,
and a loop never spans folders, so the bound the folder rule gave (`Why` in
ch. 10) holds, tighter.

## Cost

| Component | Change | Size |
| --- | --- | --- |
| Spec | delete module.folder.* (6), module.cycle.folder-edge, `.nested`, `.acyclic`, `.within-folder`, names.use.folder-cycle, 4 diagnostic rules; add 6 rules (Appendix E) | about -15 +9 rule lines |
| Diagnostic code | `folder-cycle` renamed `module-cycle` | README, `hd_diag` |
| Conformance | 8 folder cases: 2 flip, 2 change code, 4 stay (Appendix D) | 4 fixture edits |
| Compiler, deleted | `FolderGraph` and `FolderSet` (`hd_project` 1611-1734), cycle recovery by merging folders, its display and tests (`hd_driver` 3405-3763) | about -450 lines, from those ranges |
| Compiler, added | module SCC, edge-to-declarer, diagnostic | about +200 lines |
| Compiler, unit | folder becomes module loop: 854 lines in 41 files mention folders, mostly renames | about +-0 |
| Compile time | Tarjan per run O(modules + edges), as today; std units 9 folders become 46 module groups (5x) | per-unit overhead unmeasured; phase 0 measures it |
| Per-unit state | `FolderGraph.closure` is a bitset per unit, O(units^2 / 64) words | must become on-demand before 10k-module packages (P6) |
| `pub(super)` | one modifier, resolver and interface visibility | about +120 lines |
| Runtime, Wasm size, web compiler | no change: initialization already orders loops (`module.init.cycle.*`) | 0 |

Warm runs stay proportional to the change: a unit's key is its modules'
hashes and the keys of the units it uses, as the folder key is now.

## Appendix A. Stress Test

Full report: [MODULES_STRESS_TEST.md](MODULES_STRESS_TEST.md). Eleven cases:
hd std, #2346's split, serde 1.0.219, regex-syntax 0.8.10, regex 1.12.3, Go
`net/http`, a web app, and four adversarial layouts. Six fit, four fit with
friction, one breaks until std changes (the `collections/` split). Top
problems: Rust's types-in-`mod.rs` layout loops a parent with its children
(P1, fine with `x/mod.hd`); facades loop unless edges reach the declarer
(P2); a member of a sibling loop cannot move into a subfolder without
cutting every path back (P3).

## Appendix B. Prior Art

**Rust** (rustc 1.97.1, probe `m.rs`): module loops inside a crate are legal;
a child reads its parent's private items; `pub(super)` reaches the parent's
subtree; a sibling's private item is `E0603`.

```rust
mod collections {
    fn parent_helper() -> i32 { 1 }
    mod sort {
        pub(super) fn drift(x: i32) -> i32 { x }   // collections and below
        fn hidden() -> i32 { 0 }                    // sort only
        pub(super) fn up() -> i32 { super::parent_helper() }
    }
    pub mod list {
        pub fn make() -> i32 { super::sort::drift(1) }
        // super::sort::hidden() -> error[E0603]: function `hidden` is private
    }
}
mod a { pub fn f() -> i32 { crate::b::g() } }      // a <-> b: legal
mod b { pub fn g() -> i32 { 1 } pub fn h() -> i32 { crate::a::f() } }
```

hd takes `pub(super)` and rejects loops across folders, which Rust allows.
Source: [Rust Reference, Visibility and Privacy](https://doc.rust-lang.org/reference/visibility-and-privacy.html).

**Go** (go 1.27.3): a package is one directory, its files share unexported
names, and any import loop is refused, parent and child directories
included. `internal/` limits a package to its parent's subtree.

```go
// coll/internal/sort/sort.go
package sort
func Drift(x int) int { return x }
// coll/list/list.go: allowed, inside coll/
import "ex/coll/internal/sort"
// text/text.go: "use of internal package ex/coll/internal/sort not allowed"
// cyc/coll imports ex, ex imports ex/coll: "import cycle not allowed"
```

hd keeps a file as a module (the owner's design), takes the sibling loop
from Go's package-as-directory, and offers `internal/` as the alternative to
`pub(super)`. Sources: [Go spec, Import declarations](https://go.dev/ref/spec#Import_declarations),
[Go command, Internal Directories](https://pkg.go.dev/cmd/go#hdr-Internal_Directories).

## Appendix C. Alternatives Considered

- **No loops at all** (coordinator draft, 2026-11-24): the owner corrected it to the sibling rule; std's root loop of 11 modules would need a full untangle.
- **Folders as nodes** (today, 919b5d2e2): false errors (Problems table), and labels naming folders that do not exist.
- **Keep folders as the unit and merge folders on a loop**: today's error recovery does this (`merge_folders`); after #2346 std's root and `collections/` would merge into one unit.
- **Edges to the module a path names, not the declarer**: simpler to state, but every facade that a child reaches back through is a loop (P2).

## Appendix D. Today In hd

- Rules: [Dependency Cycles](../../spec/lang/10-modules.md#dependency-cycles), [Folders](../../spec/lang/10-modules.md#folders), [Folder Graph](../../spec/lang/10-modules.md#folder-graph), [Cycle Diagnostic](../../spec/lang/10-modules.md#cycle-diagnostic), [`module.vis.no-package-private`](../../spec/lang/10-modules.md#r-module.vis.no-package-private), [`module.init.use-graph`](../../spec/lang/10-modules.md#r-module.init.use-graph).
- Std census (library `use` lines, test code excluded as `module.cycle.test-code` says; 60 modules, 9 folders, 251 edges): four loops. Siblings: rt.alloc, rt.cycle, rt.memory; http, http_wire; annotation, cmp, collections, format, hash, inspect, iter, num, ops, structure, text. Across folders: testing, testing.arbitrary (`testing.hd:20` and `testing/arbitrary.hd:8`). Counting `tests:` blocks too joins them into the 19-module loop #2347 quotes.
- Probes (main 01de0e532): `folderprobe2` and two more layouts give false `folder-cycle` errors (Problems table); a flat parent and its child using each other checks (module.folder.parent-file).
- Conformance re-judged: `folder-cycle-facade` flips to accept (edge to `error`); `folder-parent-file` stays accept (decision 2); `folder-cycle-nested` and `folders-use-each-other` become `module-cycle`; `folder-cycle-leaf-folder`, `folder-loop-within-folder` and both test-edge cases stay accept. The CLI cases `folder-cycle-*` became `module-cycle-*`; the flat-file case offers no fix-it.

## Appendix E. Details

Spec text as phase 1 applied it (ch. 10, [Dependency Cycles](../../spec/lang/10-modules.md#dependency-cycles)); the final rule under Owner Decisions replaced items 1, 3 and 5:

1. [`module.cycle.edge`](../../spec/lang/10-modules.md#r-module.cycle.edge), module.cycle.edge.pub-use (retired by the final rule), [`module.cycle.edge.namespace`](../../spec/lang/10-modules.md#r-module.cycle.edge.namespace): an edge from module M to a different module N when a use in M reaches a name N declares, after `pub use` chains; a `pub use` line is an edge from its own module; a namespace use reaches the module it names.
2. `module.cycle.same-package`, `module.cycle.test-code`, `module.cycle.package`: kept.
3. The folder rules module.folder.* stay (decision 2), with module.folder.parent-child replaced by module.folder.siblings (retired by the final rule): modules in one folder are siblings, a flat `x.hd` beside `x/` included.
4. [`module.cycle.siblings`](../../spec/lang/10-modules.md#r-module.cycle.siblings): a loop of the module graph is an error unless its modules are siblings. Error: `module-cycle`.
5. [`module.cycle.diagnostic.report`](../../spec/lang/10-modules.md#r-module.cycle.diagnostic.report), [`.entry`](../../spec/lang/10-modules.md#r-module.cycle.diagnostic.entry), [`.shortest-loop`](../../spec/lang/10-modules.md#r-module.cycle.diagnostic.shortest-loop), [`.group`](../../spec/lang/10-modules.md#r-module.cycle.diagnostic.group), [`.help`](../../spec/lang/10-modules.md#r-module.cycle.diagnostic.help): one error per group, at the first edge entering the group's first module from another folder; one shortest loop with its `use` lines; the group's module and folder counts; the declarations of one cross-folder edge to move. (module.cycle.diagnostic.loop was an ID used before, so the spec uses `.shortest-loop`.)
6. Not adopted (decision 2): a fix-it that moves `x.hd` to `x/mod.hd`; the two are interchangeable, so the move changes no folder.
7. Not adopted (owner: no `pub(super)` now). Was: new rule module.vis.super: `pub(super)` makes a declaration visible to its module's parent and the parent's descendants; a `pub use` of it is an error outside them. `module.vis.no-package-private` stays true: `pub(super)` is not package-wide.

Checklist: grammar adds one modifier form (`pub(super)`, formatter keeps it);
typing unchanged; patterns, derives and closures unchanged; initialization
unchanged except `module.init.use-graph` says "among siblings"; soundness
unchanged (visibility only narrows `pub`); diagnostics as above; the
no-feature alternative for helpers is `pub` in a child module, which in std
is public API.

Phases (about an hour each, each with its test):

1. Measure per-unit overhead on std and the large-scale check with folders split one module per unit (no rule change).
2. Module SCC and edge-to-declarer in `hd_project`, behind the old rule; unit tests at width 1000 and depth 20 that count visits.
3. `module-cycle` with its diagnostic and fix-it; delete `FolderGraph` and folder recovery; re-judge the 8 conformance cases.
4. The unit becomes the module group across `hd_resolve`, `hd_sched` and the cache; cache and incremental tests.
5. Spec text, README diagnostics, indexes; std: `testing.hd` to `testing/mod.hd`.
6. `pub(super)` (if chosen), then #2346's split after `StringBuilder` leaves the root loop.
