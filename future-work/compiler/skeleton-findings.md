# New Compiler: Walking Skeleton Findings

Part of the [compiler design](README.md).

Status: report, 2026-10-07. A walking skeleton took a tiny hd subset
through every stage of the design and ran the result on V8 through Node.
The point was to test whether the design holds together, not to build
the compiler. This file lists what the design said, what the skeleton
built, and every place the design did not fit.

## Summary

The design holds together. Every stage was built from the design's own
structures: `TaskKind` and a dynamic task graph, cache keys composed as
cache.md §5.3 says, deep interface hashes, TIR columns with the catalog's
operand shapes, the callee record and its choices, remapped wire form,
instance keys, A1 class instances, and code entries with padded
relocations. None of them needed a shortcut. Five places need a design
change, and all of them are small:

| ID | Finding | Section | Kind |
| --- | --- | --- | --- |
| SK-1 | The `check` entry has no private signatures or private data layouts, and D2 needs them when the entry is a hit | cache.md §5.2, data-structures.md §3.20.4 | architecture |
| SK-2 | The A1 representation summary, read from the item's own body only, is unsound for a bounded parameter passed on to a bounded callee | codegen.md §13.2 | architecture |
| SK-3 | A caller's code depends on its callees' representation summaries, which no code key holds | codegen.md §13.2, §13.8 | architecture |
| SK-4 | The check-key lookup belongs before `ModulePrep`, not on the `ModuleFinish` path; `Parse(f)` is a demand task, not a static predecessor | scheduler.md §6.1, design-overview.md §1.3 | architecture (text) |
| SK-5 | A comment-only edit rechecks its module, because `check_key` holds `source_hash(m)` and the entry holds spans | cache.md §5.3, checking-and-tir.md §4.13.11 | architecture (owner choice) |

The rest are implementation notes. Nothing measured was atrociously
slow; see [Timings](#timings).

## What Was Built

- Code: `compiler/crates/hd_walk` (one crate; its modules are named after
  the design's crates), plus `compiler/crates/hd_syntax/src/subset.rs`.
- The subset: two folders, `app` and `geo`; `pub fn`, `pub data`,
  `pub trait`, `impl Trait for Type`, `use pkg.geo.shapes.{...}`; i32
  and bool; `x := +0`; assignment; `return`; `if`, `else if`, `else`;
  `while`; arithmetic and comparison with precedence; calls; data
  construction and field reads; `fn first[T](a: T, b: T) -> T` at i32 and
  at `Point`; `fn total[T < Shape](s: T)` calling `s.area()`; `println`
  of an i32 as a host import.
- `hd_walk` opts out of the workspace's pedantic clippy lints; it is
  throwaway code that tests the design, not the start of the compiler.
- `hd-walk run DIR` compiles a folder tree and runs it on Node.
  `hd-walk bench N` times a synthetic two-folder package.

### Exit Tests

All in `compiler/crates/hd_walk/tests/skeleton.rs`; `cargo test` is
green.

| Test | Result |
| --- | --- |
| hello, arithmetic and control, data across folders, generic `first`, trait through a bound: run on V8 | pass: output equal to the expected lines |
| Wasm byte-identical across two runs, each with a fresh store and fresh run IDs | pass, for all five programs |
| warm run, no edit | pass: `iface` 2 hits, `check` 2 hits, `link` 1 hit, nothing checked |
| private body edit in B | pass: only B's module checked; no interface rebuilt; A's `check` entry hit; the program relinks; one instance re-emitted, the rest hit their code entries |
| comment-only edit in B | pass: no interface rebuilt; A's entry hit; B rechecked (SK-5); `prog_key` hit, so no collection, emission or link, and the Wasm bytes are equal |
| public signature edit in B | pass: B's interface rebuilt with a new deep hash; A's key missed and A was rechecked; with A's source left alone, A reports the arity error |
| the A1 rule as codegen.md §13.2 words it (feature `a1-literal`) | fails at collection with `select: no impl at a concrete type` (SK-2) |

## Findings By Stage

### 1. Parse

Design: syntax.md §4.4, a recursive-descent parser over the layout
cursor, emitting events into the green tree.

Built: `hd_syntax::subset::parse_subset`, recursive descent for items and
statements and a Pratt loop for expressions, over the existing
`LayoutCursor`, emitting `Event`s into the existing `green::build`.

- **SK-N1 (implementation note).** The green tree stores only each
  node's first and last token; tokens are not children (syntax.md §4.4).
  So a typed view finds names and operators by token position. A binary
  operator is the token after the left operand's last token; a function's
  name is the first identifier in its range. It works, but each typed view
  repeats this arithmetic. A generated typed-view layer should own it.
- **SK-N2 (implementation note).** Pratt parsing over an event list needs
  "precede": wrap a finished left operand in a new node. The skeleton
  inserts a `Start` event earlier in the list, which costs O(n) per
  insertion. rust-analyzer's `forward_parent` link avoids that; the real
  parser should use it.
- **SK-N3 (implementation slip, fixed).** `skim`'s api text hash kept the
  bodies of `pub fn`, because it classified a header by its first token,
  `pub`. So editing any public function's body changed the folder's
  `api_text_hash` and rebuilt its interface. The deep hash still cut the
  rebuild off, but the two-level cutoff of syntax.md §4.5 lost its first
  level. Fixed in `skim.rs` (`head_kind`).
- **SK-N4 (implementation note).** `skim`'s use list holds every line
  that starts with an identifier, not only `use` lines. The skeleton
  filters by text. The current `skim` also builds no header tree, though
  the design's `Skeleton` has one (syntax.md §4.3), so the skeleton parses
  a whole file whenever its folder interface misses.

### 2. Header And Interface Per Folder

Design: resolution-and-interfaces.md §4.10 and §4.11: lower headers to
interface types, write a canonical blob, `api_hash`, per-item hashes,
`deep_hash` over `mentions(F)`.

Built: `iface.rs`. Headers lower to content items with types by stable
path; the blob holds public items and every impl; `api_hash` hashes the
api bytes; per-item hashes hash each item's bytes; `deep_hash(F) =
H("deep", api_hash, sorted (folder, deep_hash) of mentions(F))`. Only
syntax is read; no body is consulted.

- It fit. The deep hash behaved as §4.11.3 says: the private body edit
  and the comment edit kept B's deep hash, and the signature edit changed
  it.
- **SK-N5 (implementation note, by design).** `iface_key(F)` holds the
  deep hash of every folder that F's uses reach (cache.md §5.3), not only
  the folders F's api mentions. So a public edit in B rebuilds A's
  interface even when A exports nothing about B. The rebuilt blob keeps
  A's deep hash, so the cutoff works one level down. The test asserts
  this. It is cheap and correct, since resolving A's `use` lines reads
  B's interface, but it is one more rebuild per edit than "only folders
  whose deep hash changes" in design-overview.md §1.4 suggests.
- **SK-N6 (implementation note).** The design reads the blob in place
  (§4.10). The skeleton decodes it into a `Vec` of items. Per-item hashes
  are shallow; the per-SCC deep item hashes of §4.11.2 were not needed.
- **SK-N7 (implementation note).** Impls get a readable stable path,
  `module::impl.Trait.Type`. The design's impl segment is a head hash plus
  an ordinal (data-structures.md §3.2). Nothing in the skeleton depended
  on the difference.

### 3. Resolution Across The Two Folders

Design: resolution-and-interfaces.md §4.9: module scope from the module's
own items plus imports resolved against frozen interfaces.

Built: `module_scope`: own items, then each `use` name looked up in the
used folder's interface (public items only), else `unknown-import`.

- It fit. The `FolderIface` tasks ran in DAG order, and `ModulePrep`
  waited for the interface of every folder in its closure, as
  scheduler.md §6.1 says.
- **SK-N8 (implementation slip).** The import lookup scans the
  interface's item list per name, which is quadratic. At 2,000 imports it
  costs 14 ms (see [Timings](#timings)). The design's `by_name` index
  (syntax.md §4.6) removes it.

### 4. Body Checking That Emits TIR

Design: checking-and-tir.md §4.13: `ModulePrep` (M1), `Body` (M2),
`ModuleFinish` (M3); one TIR per body in columns; the instruction catalog;
the wire form with remapping; the TIR hash without spans; `deps`.

Built: `check.rs` and `tir.rs`. One `TirBody` per function and impl
method, with the design's columns (`tags`, `data: [u32; 2]`, `ty`,
`extra`, locals, `sub_root`, `label_inst`). The tags used: `LocalGet`,
`LocalSet`, `Prim`, `Call`, `Intrinsic`, `NewData`, `Field`, `Block`,
`If`, `Loop`, `Break`, `Return`. Constants are `Ref`s with bit 31 set
into the run's constant pool. The callee record is `Item(DefId, type
arguments)` or `TraitMethod(trait, method, self type, choice)`, with the
`Impl` and `Bound` choices. `while` lowers to `Loop { If c { body } else
{ Break } }`. `ModuleFinish` writes the entry with every ID word remapped
to entry rows, and D2 reads TIR back from the entry bytes, never from the
checker's memory, with a fresh `World` per run.

- It fit. The catalog's shapes expressed the whole subset. `Bound`
  choices became concrete at collection through `select`, as codegen.md
  §13.2 step 4 says. TIR hashes were equal across runs with different run
  IDs, and equal after a comment edit, as §4.13.11 promises.
- **SK-1 (architecture issue).** The `check` entry has no section for the
  module's private signatures or private data layouts (cache.md §5.2;
  data-structures.md §3.20.4 lists `diags`, `init_summary`,
  `row_results`, `facts`, `module_meta`, `reads`, `locs` and TIR). The
  interface holds only public items. On a `check` hit, M1 does not run, so
  nothing in the run knows a private function's result type or a private
  data type's fields. D2 needs both: `Emit` needs every callee's result
  layout, and the type section needs every data type's fields. TIR keeps
  parameter types as locals, but not the result type. The skeleton adds a
  `headers` section with the module's lowered headers, read on a hit.
  Change: add that section (private signatures and private data and enum
  layouts, by stable path) to the `check` entry, or add a result type to
  each `bodies` row and a layout table to the TIR sections.
- **SK-N9 (implementation note).** The wire form's tables are per entry
  (§4.13.11: "`strings`, `paths`, `types`: the entry's own tables"), so a
  body's remapped words are row numbers that depend on the other bodies of
  the module. A TIR hash over those words would change when an unrelated
  body adds a type. The design says the hash covers "referenced types and
  paths by content", so it is right in words; the hasher must resolve rows
  to content, or each body needs its own rows. The skeleton gives each
  body its own tables.
- **SK-N10 (implementation note).** Unary minus on a literal folds into
  the constant, so `-1` is one `Ref`, as the constant rule intends.
- **SK-N11 (implementation note).** The skeleton does no solver memo, no
  `ImplUniverseId` and no fuel: impl lookup scans the run's impl table.
  Nothing in the subset tested those structures.

### 5. Monomorphizing Collection

Design: codegen.md §13.1 to §13.3: roots, a worklist over TIR, `select` for
`Bound` choices, A1 class instances, instance keys `H("inst", path,
sub-body, [canon(arg) or class])`.

Built: `mono.rs`. The root is `main`; the walk pushes `Item` callees with
substituted type arguments and resolves `TraitMethod` callees through
`select`; the result is sorted by instance key. A1 is implemented: each
body's TIR carries a representation summary per type parameter (in the
TIR hash), and collection replaces a move-only reference argument by the
`REF` class. `first[Point]` becomes `first[REF]`, whose Wasm parameters
and result are `eqref`, and the caller casts the result back with
`ref.cast`.

- **SK-2 (architecture issue).** §13.2 says the summary marks a
  parameter exact for "a field access or construction of `T`, a trait call
  on `T`, or a closure type that mentions `T`", reading only the item's
  own body, and that "the callees of a class instance see the class as
  their argument too". A body that passes a bounded `T` on to another
  bounded generic makes no trait call itself:

  ```hd
  fn relay[T < Shape](a: T, b: T) -> i32:
      return total(first(b, a))
  ```

  So `relay[Point]` becomes `relay[REF]`, its callee `total[REF]` then
  needs `select(Shape, REF)`, and collection fails. The skeleton
  reproduces it with the `a1-literal` feature. Fix used: a parameter with
  a bound is always exact. That keeps the summary a property of the
  signature, which the interface hash already covers. The alternative,
  "exact when it is a type argument of a callee that needs it exact",
  reads callees' summaries and makes SK-3 worse.
- **SK-3 (architecture issue).** A caller's code depends on each callee's
  summary: the summary picks the callee's symbol (`first[REF]` or
  `first[Point]`) and whether the caller casts the result. The code key of
  §13.8 holds the caller's own TIR hash and its `deps` interface hashes,
  not its callees' summaries. A body-only edit that flips a summary, such
  as adding a closure that mentions `T`, would leave callers' code entries
  hitting with a stale symbol and cast. This is the same failure as Codex
  review finding 2 (§13.3). Change: put each callee's summary in the
  caller's code key, or make the summary a function of the signature
  only. With SK-2's fix, only closures still read the body.
- **SK-N12 (implementation note).** `prog_key` here lists every module of
  the package, not only those the entry reaches through the use graph
  (codegen.md §11.3). That is coarser but sound.

### 6. Wasm GC Emission And Link

Design: codegen.md §12, §13.8, §13.10; wasm-layout.md §15.1 to §15.3.

Built: `wasm.rs`. `Emit` walks the generic TIR under the substitution.
Each value-producing instruction gets a Wasm local; `Loop` becomes
`block` plus `loop` with depth-computed `br`. Layouts follow §15.1 and
§15.2: i32 and bool are `i32`; a data type is `(ref $T)` to a struct with
one mutable field per declared field, `bool` fields packed to `i8` and
read with `struct.get_u`; the `REF` class is `eqref`. A code entry is the
body's bytes with every index immediate (call targets, struct type
indices in instructions and in local declarations, cast targets) as a
5-byte padded LEB, and a relocation list. `Link` assigns indices, emits
all data types in one rec group, deduplicates function types, patches the
slots and adds the bodies with `CodeSection::raw`.

- It fit. V8 validated every module, padded LEBs included.
- **SK-N13 (implementation note).** `wasm-encoder`'s instruction API
  writes minimal LEBs only, so "emit with `wasm-encoder`" and "index
  immediates as 5-byte padded LEBs" (§13.8) do not combine directly. The
  skeleton writes relocated instructions as raw opcode bytes plus a padded
  slot, and every other instruction through `InstructionSink`. Local
  declarations of reference type carry a type immediate too, so they need
  relocations; §13.8's `Reloc::Type` covers that, but the text speaks only
  of instruction immediates.
- **SK-N14 (implementation note).** The skeleton keeps one Wasm local per
  value instruction. Correct and simple; the dev pipeline will want stack
  reuse for single-use values, which is the emission-time liveness of
  codegen.md §12.1.
- **SK-N15 (implementation note).** The code key here is `H("code",
  pipeline, instance key, TIR hash)`. The `deps` interface hashes of §13.8
  were not needed for the subset, since the callee symbol and every type
  are named by content and resolved at link. SK-3 is the one dependency
  they miss.

### 7. Running On V8

Built: `compiler/crates/hd_walk/host/run.mjs`: instantiate with
`hd.println_i32`, call the `main` export. Node 24 runs Wasm GC with no
flag. Instantiation took 0.15 to 0.25 ms and `main` 0.05 to 0.09 ms for
every sample. Module sizes: hello 66 bytes; the arithmetic program 486;
data 492; generic 361; trait 511.

### Scheduler

Design: scheduler.md §6.1 and §6.2: `TaskKind`, a dynamic graph, the
serial executor; results in slots; cache checks as the first step of a
path; per-module `Body` tasks.

Built: `sched.rs`, with the design's `TaskKind` names and `ExtTask`
(`Collect`, `Emit`, `Link`). `FolderGraph` creates the `FolderIface`
tasks in DAG order, a `ModulePrep` per module and `PackageResult`. On a
miss, `ModulePrep` creates `Body(m)` and `ModuleFinish(m)` and adds an
edge to `PackageResult`. `Collect` creates the `Emit` tasks and `Link`.

- **SK-4 (architecture issue, text).** scheduler.md §6.1 says "compute
  key, look up, skip or run is the first step of each `FolderIface` and
  `ModuleFinish` path". `ModuleFinish` runs after `ModulePrep` and
  `Body`, so a lookup there saves nothing. The key needs only the closure's
  deep hashes and `source_hash(m)`, so the skeleton looks it up as the
  first step of `ModulePrep` and creates `Body` and `ModuleFinish` only on
  a miss. Likewise design-overview.md §1.3 draws `Parse(f)` as a static
  predecessor of `ModulePrep`, but a hit needs no parse. The skeleton
  parses on demand, after a miss, and counts it as a `Parse` task. Change
  the text: the lookup is `ModulePrep`'s first step, and `Parse(f)` is
  created by the task that missed.
- Dynamic creation with "an edge from a finished task counts as
  satisfied" was enough for every case. `Link` is created before its
  `Emit` tasks and becomes ready at once; adding the edges must take it
  back out of the ready queue. The design's creation guard
  (data-structures.md §3.21) covers this for threads.

### Cache

Design: cache.md §5.3 keys; deep hashes; `prog_key` by TIR content
(codegen.md §11.3); code entries (§13.8).

Built: an in-memory store keyed by `(kind, key)` with byte entries, plus
code entries holding symbolic, ID-free code. `iface_key`, `check_key` and
`prog_key` are composed field by field as the design writes them, with a
toolchain key, a package key, `(module, role, api_text_hash)` lists and
closure deep-hash lists. Each run uses a fresh `World`, so no run ID can
cross a boundary unnoticed.

- **SK-5 (architecture issue, owner choice).** The brief expected "a
  comment-only edit reuses everything except locations". The design gets
  close but not there: the interface and every other module hit, and
  `prog_key` hits, so nothing is collected, emitted or linked. The edited
  module is rechecked, because `check_key(m)` holds `source_hash(m)`
  (cache.md §5.3) and the entry holds the module's spans and `locs`. The
  skeleton measures that as the whole recheck: 3 of 4 ms at 3,000 lines,
  30 of 40 ms at 30,000 lines in one module. To reuse the check, key
  `check` by a token hash that ignores comments and whitespace (as
  `api_text_hash` does, but over bodies too), and move spans and `locs`
  to a small entry keyed by `source_hash(m)`. The cost is a second entry
  per module and a token-hash pass per changed file. The current design
  is simpler and costs one module recheck per comment edit, so this is a
  choice, not a bug.
- **SK-N16 (implementation slip).** On a `check` hit the skeleton decodes
  the module's whole TIR into the run, even when `prog_key` then hits.
  That is 12 of 18 ms of a warm no-edit run at 30,000 lines. The design
  already avoids it: `tir_meta` holds the module's TIR content hash, so
  `prog_key` can be computed without reading TIR, and the fast key skips
  even that.

## Timings

Release build, Apple Silicon, one thread, wall time per stage, summed
over tasks. "Sample" is the trait program, the largest sample.

| Stage | Sample (about 40 lines) | Bench, 3,000 lines, cold | Bench, 30,000 lines, cold |
| --- | --- | --- | --- |
| Skim | 19 µs | 0.5 ms | 4.2 ms |
| Parse | 29 µs | 0.9 ms | 8.8 ms |
| FolderIface (resolution and lowering) | 66 µs | 1.3 ms | 14.2 ms |
| ModulePrep | 19 µs | 0.3 ms | 6.3 ms |
| Body (checking to TIR) | 40 µs | 1.5 ms | 16.1 ms |
| ModuleFinish (TIR wire form) | 62 µs | 2.3 ms | 24.3 ms |
| Collect | 12 µs | 0.14 ms | 1.9 ms |
| Emit | 44 µs | 2.2 ms (401 instances) | 24.7 ms (4,001 instances) |
| Link | 11 µs | 0.13 ms | 1.1 ms |
| Total | under 0.4 ms | 8.5 ms | 94 ms |

| Bench run | 3,000 lines | 30,000 lines |
| --- | --- | --- |
| warm, no edit | 1.8 ms | 18 ms (SK-N16) |
| private body edit in every function of B | 6.2 ms; 200 of 401 instances re-emitted | 64 ms |
| comment edit in B | 4.0 ms; `prog_key` hit | 40 ms |

Eyeballed, nothing is atrociously bad, and every stage grows about
linearly from 3,000 to 30,000 lines.

- `ModuleFinish` and `Emit` are the largest. Both are implementation
  slips, not architecture: the wire writer searches its row tables
  linearly and re-hashes, and `Emit` re-serializes a body to compute its
  code key, which the entry's stored TIR hash makes unnecessary.
- `FolderIface` at 30,000 lines is SK-N8's quadratic import lookup.
- Warm no-edit time is SK-N16.

## What The Skeleton Did Not Test

Rows, suspension, closures and capture modes, enums and matching,
strings, `dyn` and the erased method ABI, init order, coherence, the
solver memo and `ImplUniverseId`, early cutoff by recorded reads, the
disk store and atomic publish, threads, the stepping executor, panics and
sites, and folding. Each of these has its own slice in build-order.md.
