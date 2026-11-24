# Standard Library Layout

Status: proposal, 2026-11-24; domain: the module layout of `lib/std` under
the module rules of #2347 (spec ch. 10, Dependency Cycles); not accepted.
Nothing here is accepted behavior. Issues
[#2347](https://github.com/HerringtonDarkholme/hd-lang/issues/2347) and
[#2346](https://github.com/HerringtonDarkholme/hd-lang/issues/2346); module
rules in [MODULES.md](MODULES.md).

## The Proposal

std is one package of 70 modules and 35,897 lines. Give it five layers. A
module uses only its own layer and the layers below it. One sibling loop
stays, the foundation, as Rust keeps cycles inside `core` and MoonBit inside
`builtin`. Runtime internals are reached only through `std.rt`'s facade.
A new root file re-exports every public top-level module, so every public
path stays. A layering check, after Go's `go/build/deps_test.go`, fails when
a module uses a layer above its own.

```text
# 5 host:       console fs host net http process task thread remote cli testing
# 4 values:     option result error contract arena sync encoding digest path
#               random time serde json regex validate url f32 f64 prelude
# 3 foundation: annotation cmp format hash inspect iter num ops structure text
#               (one sibling loop) and collections (pure facade, 11 children)
# 2 runtime:    std.rt and its children, used only through `std.rt`
# 1 core:       core function marker convert debug

# lib/std/lib.hd: std's root, the public index
pub use std.{cmp, collections, format, hash, iter, num, ops, text}
pub use std.{console, fs, http, json, regex, time}
```

The root file uses the module re-export the owner approved (phase 4c); it
is not yet in the spec. Each layer's line count is in "How It Works".

## Open Questions

1. Keep the foundation as one 10-module sibling loop? **Recommended: yes**;
   breaking it moves trait impls away from their traits (Appendix C).
2. Does `std.http.Url` move to a new public `std.url`? **Recommended: yes**,
   and `std.http` re-exports it. `validate` uses it, so `http` is
   `validate`'s only host-layer edge.
3. Where do `TextRoom` and its impls live? **Recommended: `std.text`**.
   Its impls use `@inline` from `std.annotation`, which the runtime layer may
   not use (`module.rt.leaf`). Today the struct is in `std.rt.bytes` and its
   impls are in `std.collections.uniq_list`.
4. `rt.checked` uses `std.format.Display`, above the runtime layer. Move
   the report's formatting to `rt.checked_report`'s caller? **Recommended:
   yes**, so `module.rt.leaf` holds as written.
5. Does `http_wire` become `std.http.wire`? **Recommended: yes**. It is an
   internal module of the host ABI, which `crates/hd_host_abi` names by path.
6. Is the layering check a CI test over the compiler's module graph?
   **Recommended: yes**. It costs O(modules + edges) per run.
7. May a parent re-export its own child's namespace, so outside code keeps
   the path `std.rt.bytes.panic_with`? **Recommended: yes**: `rt/mod.hd`
   needs one line, `pub use std.rt.{alloc, bytes, cycle, hash, memory,
   thread, wait}`, and no use changes. A flat facade re-exports 70 names,
   and `wait` is both a child module and an `rt.thread` function.

## Problems It Solves

| Program or change | Today | Proposed |
| --- | --- | --- |
| A child of `collections` uses `std.iter` while `collections/mod.hd` holds code | `module-cycle`, 12 modules through `std.annotation` (probe, #2346) | facades hold only `pub use`; the layer table says why |
| `use std.rt.bytes.panic_with` in `std.regex` | 70 names from 7 `std.rt` children, used outside `std.rt`; refused once privacy is uniform | `rt/mod.hd` re-exports the 7 children (question 7) |
| `use std.collections.UniqList` after uniform privacy | std has no root, so other packages see no module | `lib/std/lib.hd` re-exports `collections`; the path is unchanged |
| `std.validate` adds a URL check | an edge from the value layer into `std.http` | `std.url`, in the value layer |
| `std.text` starts using `std.json`, which uses `std.text` | legal: all 47 top-level modules are one sibling group, so any loop among them passes | the layering check fails on `text -> json` |

## How It Works

**Layers.** Each top-level module has one layer; a child has its parent's.
The check walks every written `use` (a `pub use` makes no edge) and
fails on an edge to a higher layer. Measured on the branch after the
collections split (`target/lane267/layers.py`), two edges point up:
`rt.checked -> format` and `validate -> http`. Questions 2 and 4 remove
them.

| Layer | Modules | Lines | Holds |
| --- | --- | --- | --- |
| 1 core | 5 | 297 | compiler-seeded declarations, function and marker traits |
| 2 runtime | 12 | 2,928 | allocation, cycles, raw memory, byte formatting, hashing, threads |
| 3 foundation | 21 | 11,538 | operator, ordering, hashing, formatting and iteration traits, numbers, text, collections |
| 4 values | 19 | 12,217 | plain-hd libraries over layers 1 to 3 |
| 5 host | 13 | 8,917 | capabilities, concurrency, testing and programs |

**Loops.** Layering removes no legal loop; it stops new ones crossing
layers. Four loops remain, each held by one sibling group. The foundation
has 10 top-level modules. `rt.alloc`, `rt.cycle` and `rt.memory` share the
`std.rt` group. `collections.list`, `set` and `view` share the
`collections` group. `http` and `http_wire` share std's root, and become
parent and child under question 5.

**`std.rt`.** Its children stay private to its subtree. Outside it, std
uses 70 names from 7 children: 28 from `rt.bytes`, 24 from `rt.memory`, 9
from `rt.thread`, and 9 more (Appendix E). `rt/mod.hd` re-exports those
children's namespaces (question 7), or else the 70 names one by one.

**Root and nested paths.** `lib/std/lib.hd` re-exports 45 public
top-level modules: the 47 less `rt`, which is std-only, and `http_wire`.
Uniform privacy then hides `std.f64.consts`, `std.f32.consts` and
`std.testing.arbitrary`, which conformance names 11 times. `f64/mod.hd`,
`f32/mod.hd` and `testing.hd` re-export the nested module, so those paths
stay.

**Facades.** A parent with children is a pure facade when it sits in a
loop with its own siblings; `collections` is the first. A parent outside
every loop, like `http`, may keep code.

## Cost

| Component | Change |
| --- | --- |
| Moves | 4 files (Url, TextRoom, `http_wire`, `rt.checked`'s formatting), about 400 lines, no body changes |
| New files | `lib/std/lib.hd` (about 50 lines), `f32/mod.hd` and `f64/mod.hd` (2 each) |
| Rewritten uses | none with question 7 (`rt/mod.hd` gains 1 line); else 70 names re-exported and about 40 use lines |
| Rust | the module re-export (phase 4c, about 60 lines); `hd_host_abi`'s `http_wire` path (2 lines); the layering check (about 80 lines) |
| Compile time, Wasm | none expected; each move is measured with the 35 bench programs |
| Per run | the check is O(modules + edges), 70 and 274 today, in CI only |

A move changes release Wasm bytes. The `hd.positions` section names each
function by its declaring module and file. The collections split added 10
to 339 bytes per bench program, all in that section (Appendix D).

## Migration Plan

Each batch is one green commit or two, gated by the stage suites, std's
tests and the 35 bench programs.

| Batch | Work | Price |
| --- | --- | --- |
| 0, done | collections split (#2346) | 3,276 lines moved into 11 files; Rust +17 −12 |
| 1 | module re-export: spec amends `module.use.facade`; resolver; conformance | about 60 Rust lines, 4 cases |
| 2 | `lib/std/lib.hd` and the nested re-exports | 3 files, about 55 lines |
| 3 | `std.rt` facade | 1 line with question 7 |
| 4 | the warm-run regression fix (#2347 task 1) | per-unit last-run record |
| 5 | questions 2 to 5: the four moves | about 400 lines moved, 2 Rust lines |
| 6 | the layering check | about 80 lines, one CI test |
| 7 | uniform privacy (phase 4a/b) | spec rule, diagnostic with fix-it, conformance, resolver |

Batches 1 to 3 come before 7: without them, privacy breaks std's own
uses and every `std.X` path. Batch 4 is independent and blocks the
phase 2 merge.

## Appendix A. Stress Test

| Case | What happened or happens | What the design does |
| --- | --- | --- |
| The collections split, done on the branch | the parent kept code: `module-cycle` through `std.annotation`, correct by the rule | facades of loop members are pure |
| `List.view` builds `ListView { items, start, end }` in another child | `private-member`; the fields would have to be `pub`, and the facade re-exports the type | the method moves to the type's module, in an `impl List` block |
| `drift_sort`, `before`, `list_reserve` shared by children | made `pub`; outside code can name `std.collections.sort.drift_sort` until privacy lands | uniform privacy hides them; no `pub(super)` |
| A child of `std.text`, say `text.unicode`, using `collections` | `text` would use its child, which reaches the foundation: a loop across groups | `text` becomes a pure facade first, as `collections` did |
| `rt.checked` formats with `Display` | an edge from layer 2 to 3, against `module.rt.leaf`; nothing caught it | the layering check fails on it |
| `impl TextRoom` in `collections`, `TextRoom` in `rt.bytes` | breaks `trait.own.module.inherent-target`; impl placement is not checked in std (`hd_resolve/src/lower.rs:8139`) | question 3; the placement gap is filed separately |
| A panic in `List.remove` | frame reads `std.collections.list.List.remove`, a module privacy will hide | open: frames could name the public path; not proposed |
| `docs/` hd examples use `std.io.LineReader` | no module `std.io` exists | out of scope; a docs sweep |

## Appendix B. Prior Art

**Rust** (1.97.1 `library/`, local rust-src). Three crates in a line:
`core` has no dependencies, `alloc` depends on `core`, `std` on both
(`*/Cargo.toml`). Cargo refuses crate cycles. Inside a crate modules
cycle freely: `core/src/fmt/num.rs:5` uses `crate::num`, and
`core/src/num/error.rs:5` uses `crate::fmt`. `Vec`, `String`, `VecDeque`
and `BTreeMap` sit in `alloc` with `fmt::format`. `HashMap` sits in `std`,
for its random seed. Runtime internals are the private `mod sys`
(`std/src/lib.rs:708`). `std` re-exports whole modules:

```rust
pub use core::cmp;        // std/src/lib.rs:513
pub use core::iter;       // :545
pub use alloc_crate::fmt; // :588
pub mod collections;      // :617, itself `pub use`s vec_deque::VecDeque
pub(super) fn wrap_index(..) // alloc/src/collections/vec_deque/mod.rs:3633
```

hd takes the whole-module re-export and a loop inside one group. It
rejects `pub(super)` (owner, MODULES.md decision 3).

**Go** (1.27.3, `go env GOROOT`). Packages never cycle: `import cycle not
allowed` (`src/cmd/go/internal/load/pkg.go:1461`). An `internal`
directory is importable only inside its parent's tree (`disallowInternal`,
`pkg.go:1485`). The layers are a checked table:

```go
// src/go/build/deps_test.go, inside the depsRules string
cmp, runtime, math/bits < iter < maps, slices;
RUNTIME < sort < container/heap < unique;
os, reflect < fmt;
```

Collections (`slices`, `maps`) sit just above the runtime; `fmt` sits high,
over `os` and `reflect`. hd takes the checked table (question 6) and the
parent-tree privacy, which uniform privacy already is.

**MoonBit** (core 0.10.17, `~/.moon/lib/core`). One package, `builtin`,
holds `Array`, `Map` (`linked_hash_map.mbt`), `StringBuilder`, `Show`,
`Hash` and `Iter`: the mutually dependent core, 95 source files.
Every other package imports it:

```moonbit
// array/moon.pkg, deque/moon.pkg, strconv/moon.pkg
import { "moonbitlang/core/builtin" }
// string/moon.pkg
import { "moonbitlang/core/internal/strconv" @internal/strconv }
```

Probed with moon 0.1.20261023, both are refused: `Import loop detected:
p/q/a -> p/q/b -> p/q/a`, and `Cannot import internal package ... due to
internal visibility rules`. hd's foundation layer is `builtin` split into
files that form a sibling loop.

## Appendix C. Alternatives Considered

- **Break the foundation loop into a DAG, as Go does.** Each trait's impls
  for other modules' types would move, for example `Ord` for numbers
  out of `cmp`. It needs a census of the loop's 30 edges, and it puts impls
  far from their traits. Not proposed.
- **One `builtin` module, as MoonBit does.** An 8,190-line file; the owner
  asked for smaller files (#2346).
- **Separate packages, as Rust's crates.** Packages may not cycle, so the
  foundation stays one package anyway; it adds manifests and gains nothing.

## Appendix D. Today in hd

Census after the collections split (`target/lane267/std_census.py`):
70 modules, layered by longest dependency path. Loops: the 10-module
foundation (8,190 lines); `rt.alloc`, `rt.cycle`, `rt.memory`;
`collections.list`, `set`, `view`; `http`, `http_wire`. Before the split
the foundation had 11 modules; the pure facade took `collections` out.
Release Wasm after the split: code identical up to function order, which
follows the declaring module. The `hd.positions` section grew by 10 to
339 bytes per program. Compile time over 36 programs: cold geomean
+1.3%, warm −1.2%, within noise.

## Appendix E. Details

- Names std uses from `std.rt` children outside `std.rt`: `rt.bytes` 28
  (`panic_with`, `format_signed`, `TextSink`, `ByteBuffer`, ...),
  `rt.memory` 24 (`Buffer`, `Ptr`, loads, stores, atomics), `rt.thread` 9
  (`run`, `take`, `wait`, ...), `rt.wait` 4, `rt.alloc` 2, `rt.hash` 2,
  `rt.cycle` 1.
- The check's table lives beside the module graph and is the only place a
  layer is written; a new top-level module must be listed or the check
  fails.
- Rollback: each batch is a move; reverting its commit restores the old
  paths.
- Parse log: the one `text` block parses with
  `scripts/hd-conformance.sh parse` (syntax only). Its namespace
  `pub use` is approved (phase 4c) but not yet in the spec.
