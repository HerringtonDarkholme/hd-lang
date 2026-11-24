# Module Graph Stress Test

Status: design review, 2026-11-24; changes no decision, design record, spec
text, or compiler code. It tests [MODULES.md](MODULES.md) against the
owner's rule of 2026-11-24, "no module cycles, unless modules are in the
same folder siblings" ([#2347](https://github.com/HerringtonDarkholme/hd-lang/issues/2347)),
and the current [Dependency Cycles](../../spec/lang/10-modules.md#dependency-cycles)
and [visibility](../../spec/lang/10-modules.md#public-uses-and-visibility) rules.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Parse Log](#parse-log)

## Surface Being Tested

- **R1, edges.** An edge goes from a module to the module that declares the name a library `use` reaches, after `pub use` chains (MODULES.md Question 1, recommended). Variant **R1b**: the edge goes to the module the path names, as `module.cycle.folder-edge` says today.
- **R2, siblings.** Modules are siblings when their files share a directory; `x/mod.hd` is in `x/`, a flat `x.hd` is not (Question 2, recommended). Variant **R2b**: `x.hd` counts as in `x/`, as `module.folder.parent-file` says today.
- **R3, loops.** A loop is legal only among siblings; any other is `module-cycle`.
- **R4, helpers.** `pub(super)`: visible to the parent module and its subtree (Question 3, recommended).
- Unchanged: `module.cycle.test-code`, `module.cycle.package`, `module.pub-use.*`, initialization of a loop as a group.

## Method

- Cases: hd std on main 01de0e532, #2346's `collections/` split, two Rust crates with loops and one without, Go's `net/http`, a web app, and four adversarial layouts.
- Rust and Go module graphs come from a census script over the crates' sources (`~/.cargo/registry`, Go 1.27.3's GOROOT). It reads `crate::`, `super::` and `self::` paths in code, counts inline `mod x { }` blocks as modules, skips `#[cfg(test)]` tails and `tests.rs`, and ignores macros. Library shapes are approximations; no source is copied.
- hd's census reads top-level `use` and `pub use` lines of `lib/std`; `tests:` blocks are indented, so they are skipped, as `module.cycle.test-code` says.
- Verdicts: **works** (fits as written), **friction** (fits after a move a user must make, or one question's answer decides it), **breaks** (cannot be expressed until something else changes).
- Parsing checks syntax only. No block is claimed to type-check.

## Summary

| # | Case | Source | Verdict | Element at fault |
| --- | --- | --- | --- | --- |
| 1 | std's loops | hd `lib/std` | friction | R2: `testing.hd` with `testing/arbitrary.hd` |
| 2 | `collections/` split | #2346 | breaks | R3: `text -> collections` closes a cross-folder loop |
| 3 | serde's `de` and `ser` | serde 1.0.219 | friction | R2: traits in `mod.rs`, children use them |
| 4 | regex-syntax's `ast` | regex-syntax 0.8.10 | friction | R2: same shape |
| 5 | regex | regex 1.12.3 | works | none: no loop |
| 6 | `net/http` one package | Go 1.27.3 | friction | R4: helpers shared by 33 files |
| 7 | web app with `AppError` | synthetic | friction | R3: error type wraps a subsystem's error |
| 8 | diamond through folders | `folderprobe2` | works | none (false error today) |
| 9 | root facade, child names `pkg.{AppError}` | probe p3 | works under R1, breaks under R1b | R1 |
| 10 | parent and nested child use each other | `folder-cycle-nested` | works (rejected, as wanted) | none |
| 11 | 10,000-module package | adversarial cost | friction | unit closure bitsets |

**Conclusions.** The rule holds on every real tree once two choices are
made: edges reach the declarer (R1) and `x/mod.hd` sits with its children
(R2). Under those, the Rust pattern of types in `mod.rs` used by children
ports by keeping `mod.hd`, and std's one violation is a file move. The one
real break is #2346: a member of std's root loop cannot move into a
subfolder while a root module still reaches back to it. That needs a std
change, not a rule change. Helpers shared across a folder need a visibility
narrower than `pub` in every split case (2, 6).

## Cases

### 1. hd std today

60 modules, 9 folders, 251 library edges, four loops:

| Loop | Modules | Folders | Under R2 | Under R2b |
| --- | --- | --- | --- | --- |
| root | annotation, cmp, collections, format, hash, inspect, iter, num, ops, structure, text | `lib/std` | legal | legal |
| runtime | rt.alloc, rt.cycle, rt.memory | `lib/std/rt` | legal | legal |
| http | http, http_wire | `lib/std` | legal | legal |
| testing | testing, testing.arbitrary | `lib/std`, `lib/std/testing` | `module-cycle` | legal |

The testing edges are `testing.hd:20: pub use std.testing.arbitrary.{With, with}`
and `testing/arbitrary.hd:8: use std.testing.Choices`. The fix-it moves
`testing.hd` to `testing/mod.hd`; no `use` line changes. Counting `tests:`
blocks as edges would merge testing into the root loop (19 modules, the
count #2347 quotes), but test code makes no edge.

**Verdict: friction** under R2 (one file move), works under R2b.

### 2. #2346: `std/collections/` as a folder

The split puts `collections/{mod,list,map,uniq_list,uniq_map,span,sort,text}.hd`
under the root. `collections` uses `iter`, `num`, `ops`, `format`, `hash`
and `annotation`, all in the root loop. Every path back runs through one
line:

```
collections -> format -> text -> collections      format.hd:20  use std.text.StringBuilder
collections -> num -> text -> collections         num.hd:7      use std.text.Parse
collections -> annotation -> structure -> inspect -> format -> text -> collections
                                                  text.hd:15    use std.collections.UniqList
```

So any child that `text` reaches (`uniq_list.hd`) and that reaches `format`
or `num` closes a loop across `lib/std` and `lib/std/collections`. R1 does
not help: the edge already reaches the declarer. The loop is real.

```text
# lib/std/text.hd today: StringBuilder's storage is the one edge back
use std.collections.UniqList

pub struct StringBuilder:
    bytes: UniqList[u8]
```

Leaving the loop is a std change. `StringBuilder` uses four `UniqList[u8]`
operations (`append_text`, `copy_text`, `spare`, `take_text`), the text-sink
impls at `collections.hd` 470-620. Candidates: hold `std.rt.bytes.TextRoom`,
moving those impls into `rt.bytes` (about 150 lines move, no behaviour
change); or move `StringBuilder` into `collections` and re-export it from
`text`. After that, `collections/` uses the root and nothing in the root
loop uses it back; `task`, `serde`, `time`, `regex`, `net` and `prelude`
still use it, and they are outside the loop.

The split's private helpers (`DriftByFn`, `DriftByKey`, `DriftByOrd`,
`drift_sort` at `collections.hd` 68 and 415) are used by `list` and
`uniq_list`. With R4 they stay in `sort.hd`:

```text
# lib/std/collections/sort.hd
pub(super) fn drift_sort(len: usize) -> usize: # hypothetical syntax
    len

# lib/std/collections/list.hd
use std.collections.sort.drift_sort
```

**Verdict: breaks** until `StringBuilder` leaves the loop; then works with R4.

### 3. serde 1.0.219: traits in `mod.rs`

The census finds `ser <-> ser::impossible` and `de <-> de::ignored_any`.
`ser/mod.rs` declares `Serializer` and re-exports `Impossible`;
`ser/impossible.rs` implements `Serializer` for it.

```rust
// ser/mod.rs
mod impossible;
pub use self::impossible::Impossible;
pub trait Serializer { /* ... */ }
// ser/impossible.rs
use crate::ser::{self, Serialize, SerializeMap /* ... */};
```

In hd, `ser/mod.hd` and `ser/impossible.hd` share `ser/`: legal under R2.
With a flat `ser.hd` it is `module-cycle`, and the fix-it moves it to
`ser/mod.hd`.

```text
# src/ser/mod.hd
pub use pkg.ser.impossible.{Impossible}

pub trait Serializer:
    fn serialize_i32(self, v: i32) -> void

# src/ser/impossible.hd: uses the parent's trait, legal under R2
use pkg.ser.{Serializer}

pub struct Impossible:
    pub never: i32
```

**Verdict: friction** (the `mod.hd` form is required); works as written under R2b.

### 4. regex-syntax 0.8.10: `ast` and its visitors

One loop: `ast`, `ast::print`, `ast::visitor`. `ast/mod.rs` declares `Ast`
and re-exports `visit` and `Visitor`; the children use `crate::ast::Ast`.
Same shape and verdict as case 3: **friction** under R2.

### 5. regex 1.12.3

14 modules (with inline ones), 6 edges, no loop. **Verdict: works.**

### 6. Go `net/http`: one package of 33 files

`net/http` is 33 non-test files sharing unexported helpers:
`validMethod` is declared in `request.go:854` and called from
`clientconn.go:237` and `pattern.go:103`. Its subtree shares
`net/http/internal/ascii` (used by `client.go`, `cookie.go`, `header.go`,
`httputil/reverseproxy.go` and others).

In hd each file is a module of one folder, so loops among them are legal,
but `validMethod` must be `pub` to be used by a sibling. In a package that
is public API. With R4 it is `pub(super)`: visible in `http` and below.

```text
# src/http/request.hd
pub(super) fn valid_method(method: string) -> bool: # hypothetical syntax
    method.len() > 0

# src/http/pattern.hd
use pkg.http.request.valid_method
```

**Verdict: friction**; it works with R4 or with `internal/`.

### 7. A web app with a shared error type

`src/error.hd` holds `AppError`, `src/db/mod.hd` the store, `src/models/`
the records, `src/handlers/` the routes. Models use `error`; `db` uses
models and `error`; handlers use all three. Acyclic: works.

The common next step breaks it: `AppError` wraps the store's error, and the
store returns `AppError`.

```text
# src/error.hd
use pkg.db.{DbError}

pub enum AppError:
    NotFound
    Db(DbError)

# src/db/mod.hd: error.hd is in src, db in src/db
use pkg.error.{AppError} # error: module-cycle

pub enum DbError:
    Closed
```

The fix is the layering Rust and Go code already uses: `db` returns its own
`DbError`, and `error.hd` wraps it with `@from`. **Verdict: friction**; the
diagnostic must point at `db/mod.hd:1` and help with "move `AppError` out of
`error`, or stop using it in `db`".

### 8. Diamond through folders (`folderprobe2`)

`parentB -> parentC.useParentA -> parentA`, with `parentA.hd` and
`parentB.hd` in `src`. No module loop. Today: `folder-cycle: src/main ->
src/parentC/useParentA -> src/main`, naming `src/main`, which is on no edge,
and offering to move `main.hd`. **Verdict: works.**

### 9. A root facade and a child that names a root re-export

```text
# src/lib.hd
pub use pkg.error.{AppError}
pub use pkg.handlers.{route}

# src/handlers/mod.hd
use pkg.AppError

pub fn route() -> AppError:
    .NotFound
```

Under R1 the edges are `lib -> error`, `lib -> handlers`, `handlers -> error`:
no loop. Under R1b, `handlers -> lib` and `lib -> handlers` loop across `src`
and `src/handlers`. Today: `folder-cycle: src/error -> src/handlers ->
src/error`. **Verdict: works under R1, breaks under R1b.**

### 10. A parent and its nested child (`folder-cycle-nested`)

`shop/mod.hd` re-exports `shop.orders.order`; `shop/orders/mod.hd` uses
`pkg.shop.{money}`, declared in `shop`. A real loop across `src/shop` and
`src/shop/orders`: `module-cycle`, as the owner's "folder A -> sub folder ->
folder" asks. **Verdict: works** (rejected, with both `use` lines).

### 11. 10,000 modules

Tarjan and the shortest-loop search are linear. The unit closure is not:
`FolderGraph.closure` keeps a bitset per unit, O(units^2 / 64) words. At
10,000 one-module units that is 1.56 million words, 12.5 MB, built on every
run. With folders as units it was bounded by the folder count. **Verdict:
friction**: the closure must become a query on demand (the reachability a
caller asks for) before the unit changes.

## Comparison With Other Languages

| Question | hd (proposed) | Rust | Go |
| --- | --- | --- | --- |
| Loops between modules | among files of one folder | anywhere in a crate | none; a package is a folder |
| Parent with its child | legal as `x/mod.hd` | legal | refused, even parent and child folders |
| Private helper shared in a folder | `pub(super)` | `pub(super)`, `pub(crate)`, `pub(in path)` | unexported names shared by the package |
| Helper for a subtree | `pub(super)` from a child | `pub(in crate::x)` | `internal/` |
| Facade re-export | `pub use`; edge to the declarer | `pub use`; no ordering edge | none |

Sources: rustc 1.97.1 probe (a module loop compiles; `E0603` for a
sibling's private function), go 1.27.3 probes (`import cycle not allowed`;
`use of internal package ... not allowed`). See MODULES.md Appendix B.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | P3 | moving a member of a sibling loop into a subfolder needs every path back cut | breaks | 2 |
| 2 | P2 | facades loop with the children that name them | breaks under R1b | 9, 7 |
| 3 | P1 | types in a parent, used by its children, loop parent and child | friction | 1, 3, 4 |
| 4 | P4 | helpers shared across a folder need a narrower `pub` | friction | 2, 6 |
| 5 | P5 | an error type that wraps a subsystem's error, used by it | friction | 7 |
| 6 | P6 | per-unit closure bitsets grow with the square of the units | friction | 11 |

### P3. A loop member cannot move into a subfolder

**Effect.** Case 2: `collections` is in std's root loop through
`text.hd:15`, so its split is `module-cycle` until that edge goes. Any large
module in a sibling loop has the same problem when it grows into a folder.
**Candidates.** (a) Does `StringBuilder` hold `rt.bytes.TextRoom`, moving
its sink impls down? (b) Does `StringBuilder` move into `collections`? (c)
Does #2346 wait until the root loop is layered more widely?

### P2. Facades and the children that name them

**Effect.** Cases 9 and 7: `use pkg.{AppError}` in a child that `lib.hd`
re-exports. Under R1b every such child loops with the root. **Candidates.**
(a) R1: the edge reaches the declarer (recommended in MODULES.md). (b) A
`pub use` line makes no edge at all. (c) Keep R1b and tell users to name
declaring modules.

### P1. Parent types used by children

**Effect.** Cases 1, 3, 4: Rust's common layout and std's `testing`.
**Candidates.** (a) R2: only `x/mod.hd` sits with its children, and the
fix-it moves `x.hd` (recommended). (b) R2b: `x.hd` and `x/mod.hd` stay
interchangeable, and `testing` is legal as is. (c) Neither: the parent is
never a sibling, so `mod.hd` must be a pure facade.

### P4. Helpers shared inside a folder

**Effect.** Cases 2 and 6. Today the only way is `pub`, which in std is
public API. **Candidates.** (a) `pub(super)` (recommended). (b) `internal/`
directories. (c) Private names shared by every file of a folder.

### P5. Error types that wrap a subsystem's error

**Effect.** Case 7, a common web-app shape. **Candidates.** (a) Keep it an
error; the help line names the cheapest edge to cut. (b) A dedicated note
in the guide's project-layout how-to.

### P6. Closure state per unit

**Effect.** Case 11: 12.5 MB of bitsets at 10,000 units, rebuilt per run.
**Candidates.** (a) Reachability on demand, memoized per query. (b) Keep
folders as a grouping for closure only, units for checking.

## Parse Log

Every `text` block of this file and of MODULES.md was parsed on 2026-11-24
with `scripts/hd-conformance.sh parse` (syntax only). Lines marked
`# hypothetical syntax` use `pub(super)`, which no chapter specifies; the
parser rejects them at the `(`, as expected.

| File | Block | Result |
| --- | --- | --- |
| MODULES.md | 1, the proposal | parse error at `pub(super)` only (hypothetical line) |
| MODULES_STRESS_TEST.md | 1, `text.hd` today | parse |
| MODULES_STRESS_TEST.md | 2, `collections/sort.hd` | parse error at `pub(super)` only (hypothetical line) |
| MODULES_STRESS_TEST.md | 3, `ser/mod.hd` | parse |
| MODULES_STRESS_TEST.md | 4, `http/request.hd` | parse error at `pub(super)` only (hypothetical line) |
| MODULES_STRESS_TEST.md | 5, `error.hd` and `db/mod.hd` | parse |
| MODULES_STRESS_TEST.md | 6, root facade | parse |
