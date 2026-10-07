# New Compiler: Known Issues Of Prior Implementations

Status: Research, not decided. Parts A and B, 2026-10-07.

This document surveys the newest front ends, type checkers and incremental
engines, and above all their documented problems. It checks each problem
against the recommendations in
[research.md](research.md) and
says whether we avoid it, inherit it, or ignore it. Part B applies the same
test to back ends, Wasm implementations and runtimes.

Fixed inputs, from the owner's decisions of 2026-10-06: the compiler is
written in Rust; the incremental model is a per-module on-disk cache with no
salsa and no daemon; generic code is compiled per concrete type, with
byte-identical functions merged; the compiler runs in the browser
playground, single-threaded at first.

Conventions. "Unverified" marks a claim I could not confirm from a primary
source. "Mine" marks a suggestion of mine, not shipped practice. Issue dates
are the dates shown on the issue pages. Depth: TypeScript 7, ty and salsa,
Pyrefly, rustc, rust-analyzer, Go and Zig got the most time. Kotlin, Swift,
Roc, Gleam, Carbon, Lean, Sorbet, Flow, Hack, Ruff, oxc and Biome are brief.

## Lessons For hd

Ranked by how much each would hurt us if ignored. "Change" means a
recommendation should change, "add" means something is missing, and "keep"
means the evidence supports what we have.

| # | Lesson | From | Action |
| --- | --- | --- | --- |
| 1 | A cache key must cover every interface a check can read, including through re-exports and through types named inside a used signature. Missed reads are the main source of stale results. | TypeScript #64386, #64552, tsgo #2666; gopls keys; Zig #25872; rustc, dune | **Change** Q2: make the folder interface hash deep (it folds in the hashes of every interface it mentions). Add re-export and option edits to `incremental-soundness`. |
| 2 | Parallel checkers produce different output unless order comes from content. Every counter and every hash-map walk leaks into output somewhere. | TypeScript 7 `stableTypeOrdering`, #64589; rustc parallel front end (MCP #1005); Gleam #6383 | **Add**: no global counters; a content order for anything shown or hashed; a `determinism` run that varies threads, hash seeds and checkout path. |
| 3 | Resource budgets must not depend on cache state or schedule, or the same program passes on one run and fails on the next. | TypeScript 7 (`--checkers` changes results); Lean heartbeats; Swift solver limits | **Add** (mine): memo hits charge their recorded cost; budgets count language-level steps, never time or allocations. |
| 4 | Native compilers lose the heap cap a VM gave them. A runaway type then freezes the machine instead of failing. | tsgo #2125, #1622 (116 GB), TS #64423; ty #4147 (21 GB); rust-analyzer #19402; Biome 2.3 | **Add**: a default process memory cap and a per-item budget, each with a diagnostic. This matters more when agents run many checks at once. |
| 5 | Per-thread copies of type state multiply memory. tsgo's four checkers each rebuild the types they touch. | tsgo design; zackoverflow analysis; tsgolint #1175 | **Keep** Q3's frozen shared interface tables. **Add** a memory-ratio target to `parallel-speedup`. |
| 6 | A coarse unit is right, but a large codebase soon needs per-name invalidation. Pyrefly, Sorbet and gopls all added it later. | Pyrefly (18x), Sorbet (19% to 10%), gopls typerefs, Kotlin ABI snapshots | **Change** Q2: store per-item hashes in the interface blob from day 1, so per-item keys are a later switch, not a format change. |
| 7 | Fine-grained in-memory engines still ship cycle bugs that return stale results, and their disk persistence is a prototype. | salsa #1326, #1336, #1338, #1349, #1350 (all 2026); ty cycle panics; salsa #967 | **Keep** no salsa. **Add**: each fixpoint in the checker is local, monotone, serial and bounded. |
| 8 | Content-addressed caches grow without bound unless eviction is designed in. | Go #81830 (100 GB+), #76946; gopls filecache; rustc #48172 | **Add** an eviction design to Q2: a size cap, Go-style use marking, and a trim step. |
| 9 | Irrelevant inputs in a key, such as paths or plugin locations, split one entry into many variants. | Swift explicit modules (about 16x slower builds); Go's executable digest rule | **Add** a key-hygiene rule and a path-independence test. |
| 10 | Stat-based change detection has known holes: deleted files, restored files and same-second writes. | Gleam #4320; git's "racily clean" rule | **Add** to the Q2 stat manifest: hash files whose mtime is too close to the manifest's write time, and handle deletions. |
| 11 | Implicit laziness hides order and locks. Explicit phases parallelize; retrofits take years. | Kotlin K2; Zig's 30,000-line type resolution redesign; rustc parallel front end | **Keep** the explicit task graph of Q3. |
| 12 | Interfaces leak private items when generated code copies bodies into public signatures. | Lean module system #15401; Swift interface verification failures | **Add**: validate each interface when it is written; every exported signature and template may mention only exported items. |
| 13 | Type checker performance cliffs come from language features: overloading, literal disjunctions, unbounded search. | Swift roadmap; Go #66699 | **Keep** hd's no-overloading rule and the memoized trait goals. |
| 14 | A new trait solver took about four years and broke real code. | rustc next solver | **Keep** hd's small trait rules and Q5's head index. |
| 15 | Data-oriented design added late gives little. Flat tokens and arenas retrofitted into rustc did not pay off. | rustc maintenance report, 2026 | **Keep** Q3's data-oriented core from the first slice. |
| 16 | Go's browser build is large, and tsgo has no measured browser story. Rust tools ship in the browser today. | tsgo #458; ty playground; Roc's 2.5 MB build | **Keep** Rust and the browser build in CI from slice 1. |

## TypeScript 7 (tsgo)

### What It Chose

TypeScript 7.0 shipped on 2026-07-08 as a Go port of the compiler
([announcement](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)).
Parsing, binding and emit run in parallel per file. Type checking uses a pool
of independent checkers, four by default (`--checkers`). Each checker has
its own types and symbols over a shared, immutable AST. Files go to
checkers in a fixed split. `--builders` checks several projects at once,
and the two flags multiply: `--checkers 4 --builders 4` can run 16
checkers. `--singleThreaded` makes parsing, checking and emit serial
([7.0 beta](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0-beta/)).

Full builds are about 10x faster than 6.0: VS Code 89 s to 8.7 s, Sentry
133 s to 16 s ([December 2025 progress](https://devblogs.microsoft.com/typescript/progress-on-typescript-7-december-2025/)).
Memory fell much less than time: VS Code 5.2 GB to 4.2 GB (18%), Sentry
4.9 GB to 4.6 GB (6%) ([7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)).
7.0 ships with no API; 7.1 is to bring a new one. The typescript-go staging
repository was archived on 2026-09-01 and work moved to microsoft/TypeScript
(as shown on [tsgo #2551](https://github.com/microsoft/typescript-go/issues/2551)).

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Type IDs came from the order types were met, so parallel checkers gave different unions, declaration files, and sometimes errors | TS 6.0 notes: parallel checkers "can produce different declaration files, or even calculate different errors when analyzing the same file". 7.0 sorts types and symbols by content. `--stableTypeOrdering` in 6.0 costs "up to 25%" ([6.0 notes](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-6-0.html)) | always on in 7.0 |
| Content order still fell back to IDs | `CompareTypes` ended with `t1.id - t2.id`; 10 of 946 declaration files changed between clean builds, making a CI build cache "close to useless" ([#64589](https://github.com/microsoft/TypeScript/issues/64589)) | fixed by PR #64621 |
| Results can depend on the checker count | "In rare cases, varying the number of `--checkers` may surface order-dependent results"; the advice is to fix the count across a team ([7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)) | by design |
| Per-checker duplication multiplies memory | Each checker rebuilds the types it needs; one analysis measured about 50 MB of checker memory single-threaded and about 400 MB with four checkers ([zackoverflow](https://zackoverflow.dev/writing/why-does-tsgo-use-so-much-memory/)); linters on tsgo repeat work across checkers ([tsgolint #1175](https://github.com/oxc-project/tsgolint/issues/1175)) | by design; [#2115](https://github.com/microsoft/typescript-go/issues/2115) refactors the pool |
| No heap ceiling | tsc failed cleanly at V8's heap limit; tsgo "continues to allocate memory until it exhausts both physical RAM and swap" on recursive conditional types ([#2125](https://github.com/microsoft/typescript-go/issues/2125)); `tsgo --build` reached 116 GB RSS on a 5,000-project graph ([#1622](https://github.com/microsoft/typescript-go/issues/1622)) | open |
| 7.0 runs out of memory where 6.0 passes | 6.0.3: 2.9 GB peak; 7.0.2: OOM at a 6 GB limit, in `recursiveTypeRelatedTo`, with no output ([#64423](https://github.com/microsoft/TypeScript/issues/64423), 2026-09-24) | open |
| Algorithmic blowups the port inherited | 2x slower than tsc on a NestJS app; one function allocated 46 GB, 87% of all allocations ([#2551](https://github.com/microsoft/typescript-go/issues/2551)) | fixed |
| Incremental results go stale | augmenting an interface through a re-export keeps an old error until the cache is deleted ([#64386](https://github.com/microsoft/TypeScript/issues/64386), 2026-09-22); changing `lib` or `target` keeps stale diagnostics, a 7.0 regression ([#64552](https://github.com/microsoft/TypeScript/issues/64552)); `tsgo --build` misses dependency updates ([#2666](https://github.com/microsoft/typescript-go/issues/2666)) | open |
| Watch mode | "may be less-efficient than the existing TypeScript compiler in some scenarios" (Dec 2025); 7.0 rebuilt it on Parcel's watcher | rebuilt |
| Browser | the team said yes to Wasm but had not measured size ([discussion #458](https://github.com/microsoft/typescript-go/discussions/458)); Go's Wasm output is several MB | no shipped browser build found |

### Verdict For hd

- **Ordering: we avoid it if we test it.** Q3 already says no interned ID
  is printed, sorted or hashed. tsgo shows two gaps. First, a content
  order can still fall back to an ID in a tie (#64589), so every
  comparison used for output needs a total content order. Second, unions,
  row sets, impl candidate lists and diagnostic notes are all "output" once
  rendered. hd rows are sorted sets of interned keys (Q5); the sort key
  must be the stable path, not the interned number.
- **Checker count: we avoid it by design.** Our unit of parallel work is a
  body over frozen tables, not a checker with private caches. Results can
  still vary if a budget depends on what a shared memo already holds. See
  [Budgets](#budgets-and-schedule-independence).
- **Duplication: we avoid it.** Q3's frozen interface tables are shared and
  read-only. Only body-local types go to per-body arenas. The one risk is
  the shared memo of instantiations, which Q3 handles with first writer wins.
- **Heap cap: we inherit it.** Rust has no heap limit either. Add one.
- **Stale incremental results: we inherit #64386's shape.** hd has `pub
  use` re-exports in `mod.hd` facades
  ([modules](../../spec/lang/10-modules.md#r-module.pub-use.facade)). See
  [Go](#go-gotypes-the-build-cache-and-gopls) for the fix.
- **Options in keys: covered.** Q2 puts "the options that change checking"
  in the key. #64552 shows the failure: an option that changes the *input
  set* (which lib files load) was missed. For hd, the std version and the
  target's capability set are such inputs.

## ty And Salsa

### What It Chose

ty (Astral, formerly red-knot) is a Python type checker and language server
in Rust, in beta since 2025-12-16 ([Astral](https://astral.sh/blog/ty)). It is
built on salsa for fine-grained, in-memory incrementality, down to a single
function. Claims: 2.19 s for a CLI check where Pyright takes 19.6 s and mypy
45.7 s, with no cache; 4.5 ms to update diagnostics after an edit in PyTorch,
against 370 ms for Pyright and 2.6 s for Pyrefly. The CLI has no disk
cache; every run is cold. A browser playground runs it as Wasm.

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Fixpoint cycles fail to converge | "too many cycle iterations" panics, salsa 0.28.5, regressions from a single PR ([ty #4607](https://github.com/astral-sh/ty/issues/4607), 2026-09-28; also [#4615](https://github.com/astral-sh/ty/issues/4615), [#4616](https://github.com/astral-sh/ty/issues/4616), [#4618](https://github.com/astral-sh/ty/issues/4618)) | open |
| Salsa returns stale results around cycles | a displaced memo is certified unchanged ([#1336](https://github.com/salsa-rs/salsa/issues/1336), 2026-09-25); cycle participants keep incomplete dependency lists and "doesn't re-execute after changing" an input ([#1350](https://github.com/salsa-rs/salsa/issues/1350), 2026-09-28); also [#1326](https://github.com/salsa-rs/salsa/issues/1326), [#1338](https://github.com/salsa-rs/salsa/issues/1338), [#1349](https://github.com/salsa-rs/salsa/issues/1349) | open |
| Concurrent cycles hang | threads "blocked on themselves or on a thread that no longer owns the query" ([salsa PR #1348](https://github.com/salsa-rs/salsa/pull/1348)) | fixed |
| Memory regressions | 0.0.65 reached 21 GB RSS in 9 seconds on one file, against 35 to 120 MB in 0.0.64; four runs took 73 GB and forced a power cycle ([ty #4147](https://github.com/astral-sh/ty/issues/4147)) | labelled regression |
| Persistence is a prototype | salsa's `persistence` feature: "All transitive dependencies are forced to be serializable"; "There is no easy way to update inputs after they are deserialized"; unverified queries must be re-verified after load ([salsa PR #967](https://github.com/salsa-rs/salsa/pull/967), merged 2025-08-11) | prototype |

### Verdict For hd

- **We avoid it.** The decision against salsa stands. The 2026 issues are
  not in salsa's normal path. They are all in cycle handling, and the
  failure mode is a silently stale answer. A compiler whose output feeds
  a cache cannot afford that.
- **But hd has cycles too.** Files of one folder may use each other.
  Private functions may omit result types and rows, and inferred rows form a
  least fixpoint (Q5). The lesson (mine): keep each fixpoint out of any
  generic memo layer. Solve it in one serial pass over a strongly connected
  component, with a monotone join and an iteration bound that reports a
  diagnostic rather than panicking.
- **Persistence.** Salsa's prototype needs every dependency serializable and
  cannot update inputs after load. This is the cost Q2 predicted.

## Pyrefly

### What It Chose

Pyrefly (Meta) is a Rust Python checker that replaced the OCaml Pyre. Its
design goal was "Build system, operate at the file level, evict old data"
([Mitchell slides](https://ndmitchell.com/downloads/slides-pyrefly-07_oct_2025.pdf)).
Each module moves through fixed steps: code, AST, exports, bindings,
answers, interface. The interface holds only the types of exports. Modules
are the incremental unit; the team chose against fine-grained incrementality
as "much more complex and harder to maintain" for little gain. After a
module is done, its AST, bindings and answers are dropped unless the file is
open. It checks about 1.85M lines per second on many threads.

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Pyre's OCaml design was hard to parallelize | "Parallelism was hard (multiprocess)"; no Windows (slides) | replaced by Rust threads |
| Module-level invalidation fanned out | a module was rechecked when *any* export of a dependency changed. One edit to a widely used file invalidated 2,000+ modules. Tracking "depends on type Y from module X" cut it to about 100, and update time from 3.6 s to under 200 ms ([Pyrefly blog](https://pyrefly.org/blog/2026/02/06/performance-improvements/), 2026-02-06) | fixed |
| Per-name tracking is hard | dependencies "extend beyond direct imports through method chains, class hierarchies, and transitive relationships" (same post) | ongoing |
| Easy to go quadratic; threads cost | "Super easy to go quadratic"; with many threads, "the expensive things are: thread communication, locks"; "We freely clone Type all the time. Should really have a heap" (slides) | ongoing |
| Cycles across modules | optimistic: recheck the changed module with stale values of its cycle, and widen only if its interface changes. "Might compute a module more than once, and less parallelism" (slides) | by design |

### Verdict For hd

Pyrefly is the closest shipped design to ours: per-module units, an
export-only interface, eviction after use, Rust threads. Its history
predicts ours.

- **Fan-out: we inherit it.** Q2 keys a module by the interface hash of each
  folder it uses. An edit to one signature in a large std folder rechecks
  every module that uses any item of that folder. Q2 lists this as a risk and
  defers a finer key. Pyrefly shows the deferral ends at scale, and hd's
  agent workloads edit shared modules often.
- **Our advantage.** hd has no wildcard imports, so the names a module reads
  are known from syntax. That makes a per-name key much easier than
  Pyrefly's. The hard part is the same as theirs: types reached through a
  used signature (a field of a returned struct, a method on it, its impls).
- **Action.** Store a hash per exported item in the interface blob from day
  1, where the item hash covers its signature and the item hashes it
  mentions. Then a later switch to per-name keys needs no format change.
- **Cycles: we avoid them.** Folders form a DAG; only files within one
  folder loop, and the folder resolves together.

## rustc

### What It Chose

rustc is a demand-driven query system with red-green incremental
compilation that persists its dependency graph. A parallel front end
(`-Zthreads`) has been in nightly behind a flag since 2023. A new trait
solver became the nightly default in August 2026.

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Parallel front end is not reproducible | 11 open reproducibility issues; diagnostic ordering makes "at least 5-10% of tests fail" in parallel mode; "leaking global IDs", with allocation IDs in const evaluation "the most notable example"; query cycle diagnostics are "hard" to make reproducible ([MCP #1005](https://github.com/rust-lang/compiler-team/issues/1005)) | proposal: ship with a `--deterministic` opt-in |
| Parallel front end crashes | 4 ICEs open; "no issues about rustc compiling something incorrectly in parallel mode" (same) | near nightly default in October 2026 ([maintenance report](https://kobzol.github.io/rust/2026/09/30/stf-august-september-2026.html)) |
| The retrofit took years | in nightly since 2023; still `-Z` in late 2026 | ongoing |
| Late data-oriented design pays little | a flat token representation won about 30% in lexing locally, but "overall compiler performance regressed"; arena experiments: "I did not achieve much success" ([maintenance report](https://kobzol.github.io/rust/2026/09/30/stf-august-september-2026.html)) | abandoned |
| Incremental still has gaps | async closures recompile poorly; a hack cut one benchmark from 14 s to 8 s (same report); unstable-fingerprint ICEs still filed ([#163760](https://github.com/rust-lang/rust/issues/163760)) | ongoing |
| Incremental disk use | 19 GB of incremental data for a small project ([#48172](https://github.com/rust-lang/rust/issues/48172)); old variant directories are never removed (secondary report) | ongoing |
| Trait solver rewrite | "nearly 4 years of active development"; "more than 200 issues on GitHub fixed"; "a non-trivial amount of breakage"; `datafusion` compiles "more than 8x faster", and a type-level chess program that hung now takes a minute ([Rust blog](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/)) | nightly default |

### Verdict For hd

- **Global IDs: we avoid them if we forbid them.** rustc's leak is a counter
  that is global to the session. hd will have such counters too: fresh
  inference variables, closure numbers, generated names for instances and
  state machines. Rule (mine): every counter is per body or per item, and
  every generated name comes from a stable path plus a local index.
- **Cycle diagnostics: we inherit a small version.** A cycle among private
  functions without result types reports `recursive-function-needs-result-type`
  (Q3). Which function the error points at must not depend on which
  thread found the cycle. Pick by source order.
- **Determinism opt-in: we reject it.** rustc proposes shipping parallelism
  first and determinism behind a flag. Our `determinism` metric requires
  byte-identical output always. Keep that, since agents compare outputs
  across runs.
- **Design in from day 1: confirmed.** Both the parallel retrofit and the
  late data-oriented experiments show the cost of adding these later.
- **Trait solver: we avoid most of it.** hd has no specialization,
  heads-only overlap, and impls only in the trait's or the type's module.
  It does have associated types, which is where much of rustc's solver
  work went (projection and normalization). Q5's memoized goals plus a
  budget fit. Keep it that way: each new trait feature costs the solver,
  and associated types deserve their own `pathological` cases.

## rust-analyzer

### What It Chose

rust-analyzer is a salsa-based IDE engine with lazy, on-demand analysis. In
2025 it moved to the new salsa ([PR #18964](https://github.com/rust-lang/rust-analyzer/pull/18964))
and replaced chalk with rustc's next trait solver
([PR #20329](https://github.com/rust-lang/rust-analyzer/pull/20329)).

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Memory after the salsa port | 5 to 6 GB became 22 GB at start and up to 30 GB on a 700-crate workspace ([#19402](https://github.com/rust-lang/rust-analyzer/issues/19402)); follow-up work cut usage from 8 GB to 4 GB and re-enabled LRU (secondary summary, unverified figures) | improved |
| Speed after the salsa port | cache priming went from 26 s to 113 s; random hangs at 100% CPU ([#19404](https://github.com/rust-lang/rust-analyzer/issues/19404)) | improved |
| Cold start | a dozen seconds to rebuild state on each start, since nothing persists ([Ferrous Systems](https://ferrous-systems.com/blog/rust-analyzer-next-few-years/)) | by design |
| Two trait solvers | chalk drifted from rustc and became unmaintained; sharing rustc's solver was the fix | fixed in 2025 |

### Verdict For hd

- **We avoid** the long-lived memory problem by having no daemon in v1.
  We do not avoid it in the browser, where a playground tab may run many
  checks. The `long-session` metric covers it; keep it in the browser too.
- **One checker for every tool: keep.** rust-analyzer's chalk years show the
  cost of a second type checker. hd's future language server should reuse
  `hd_check`, not reimplement it. Q7's crate split allows this.

## Go: go/types, The Build Cache And gopls

### What It Chose

`go build` compiles packages in parallel along the import graph. Each
package is checked from its imports' export data. The build cache is
content-addressed by action IDs. gopls v0.12 (2023) rebuilt the language
server on the same idea: it "works on one package at a time and saves
per-package results to files, just like a compiler emitting object code"
([Go blog](https://go.dev/blog/gopls-scalability)).

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Whole-program memory in the old gopls | typed syntax trees are "typically 30x larger than the source text"; users found memory "barely tolerable" ([Go blog](https://go.dev/blog/gopls-scalability)); 60 GB reported ([#37790](https://github.com/golang/go/issues/37790)) | redesigned |
| The redesign's costs | memory fell 88% and warm start 65% on x/tools, but "most benchmarks regressed 10-50%", and find-implementations regressed 1,389% because indexes moved to disk ([CL](https://groups.google.com/g/golang-checkins/c/wxBReKH7sVA)) | partly recovered |
| Keys must be transitive | the package key includes "all bits of the transitive closure of dependencies' Facts"; precise pruning came later (same CL) | done |
| Precise pruning needs syntax analysis | `typerefs` computes "a nearly minimal set of packages that could affect the type checking of P", syntactically; it must over-approximate aliases and initializers ([typerefs](https://pkg.go.dev/golang.org/x/tools/gopls/internal/cache/typerefs)) | shipped |
| Very large workspaces still blow up | 19M lines: about 30 GB in use, 307M heap objects, with a cold cache ([#73709](https://github.com/golang/go/issues/73709)) | backlog |
| Version skew in saved export data | "include a cryptographic digest of the executable in the key" ([gcexportdata](https://pkg.go.dev/golang.org/x/tools/go/gcexportdata)) | documented rule |
| The build cache has no size limit | it trims entries unused for 5 days, once a day; reports of 100 GB+; "generics bloats build cache" ([#81830](https://github.com/golang/go/issues/81830), 2026-09-28) | proposal open |
| Generics slowed the compiler | Go 1.18 compiled "roughly 15% slower" ([Go 1.18](https://go.dev/doc/go1.18)) | improved later |
| Uncached predicates | a 140k-line generated package spends almost all of 2 s in `assignableTo` ([#66699](https://github.com/golang/go/issues/66699)) | open |

### Verdict For hd

This is the design we chose, and its record is the best of any system here.
Three gaps remain.

- **Deep hashes: change.** Go's compiler export data is deep: it describes
  the types each exported declaration reaches, even from other packages
  (from memory of Go's format, unverified in detail). gopls stores
  shallow data and so keys each package by the transitive closure. Q2 says
  a module's key holds the interface hashes of "the folders and packages it
  uses", and the folder interface stores "the stable path of each item".
  If a blob refers to another folder's type by path only, then a module
  that calls `service.load_user()` and reads `.name` from the result
  never names `user/types`, so an edit to `User` does not change its key.
  The same holds for `pub use` chains. **Fix:** define a folder's interface
  hash as the hash of its own blob plus the interface hashes of every
  folder its blob mentions. This is a Merkle hash over the folder DAG, and
  it is cheap because the DAG is already known. Lesson 6's per-item hashes
  later narrow it.
- **Eviction: add.** The `cache-growth` metric asks for a cap, but Q2 has no
  eviction design. Go's scheme works: touch an entry's mtime on use at
  most once an hour, and trim unused entries. Add a size cap, which Go
  still lacks. Instances of generic code (Part B) will grow the cache
  fastest, as Go #76946 shows.
- **Global queries: plan for an index.** gopls's find-implementations got
  14x slower when it moved to per-package files. The program database (Q6)
  scans per-module fact records first. Plan a merged index before
  `hd callers` ships.
- **Predicate caches: keep.** Q5's memoized trait goals answer #66699. Cache
  "type T satisfies bound B" per body as well as globally.

## Zig

### What It Chose

The self-hosted compiler lowers each file to untyped ZIR, then analyzes only
what is reachable (Sema) over an InternPool of 32-bit indices. Incremental
compilation (`-fincremental`) tracks dependencies per declaration in memory
and needs `--watch`. Zig 0.17 made it usable for most x86_64 Linux projects
([0.17 notes](https://ziglang.org/download/0.17.0/release-notes.html)).

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Incremental needs a live process | "add support for using incremental compilation without --watch" is future work (0.17 notes) | open |
| Stale builds | changing a struct field's default value was ignored: "the output doesn't change", while a fresh build is right ([#25872](https://github.com/ziglang/zig/issues/25872), 2025-11-09); maintainers have said it may have "false-positive compile errors and even miscompilations" (secondary quote, unverified) | many fixed |
| Over-analysis | updates redid too much; a 30,000-line type resolution redesign in March 2026 made the compiler "lazier about analyzing the fields of types" ([devlog](https://ziglang.org/devlog/2026/)) | fixed |
| Sema is single-threaded | 19 s (Debug) and 27 s (ReleaseFast) of Sema on one core; the issue was closed as not planned ([#22236](https://github.com/ziglang/zig/issues/22236)); only codegen and linking moved to other threads ([PR #20632](https://github.com/ziglang/zig/pull/20632)); a parallel Sema exists in Bun's fork of Zig (unverified) | open |
| A release broke incremental | Zig 0.16 shipped with a bug that disabled incremental builds for Roc ([Roc](https://rtfeldman.com/rust-to-zig)) | fixed in 0.17 |

### Verdict For hd

- **Keep** the InternPool idea (Q3) and the per-module disk cache instead of
  Zig's in-memory graph. Zig's process-lifetime model is exactly what we
  cannot use without a daemon.
- **Single-threaded analysis: we avoid it.** Zig's lazy, reachability-driven
  Sema is hard to parallelize because analysis order is discovered on the
  fly. hd's explicit signatures give a fixed graph up front.
- **Lazy analysis skips errors.** Zig reports nothing for unreferenced
  code (Q4b already notes this). hd checks every body; keep that.

## Kotlin K2

### What It Chose

K2 rewrote the Kotlin front end around FIR, one tree that holds syntax and
semantics, resolved in explicit phases (supertypes, types, contracts, body
resolution). A declaration is "either unresolved or resolved up to a
specific phase" ([JetBrains](https://blog.jetbrains.com/idea/2025/04/the-story-behind-k2-mode-and-how-it-works/)).
Kotlin 2.0 made it the default. Reported speedups: up to 94% faster clean
builds and up to 376% faster analysis on one project (secondary summary of
[Kotlin 2.0](https://kotlinlang.org/docs/whatsnew20.html), unverified figures).

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| K1's implicit laziness forced a global lock | `StorageManager` meant "no more than one thread can reach those places at the same time"; implicit laziness "made the compiler code much less friendly when it came to debugging and optimization" ([JetBrains](https://blog.jetbrains.com/idea/2025/04/the-story-behind-k2-mode-and-how-it-works/)) | replaced |
| Migration of tools | highlighting, completion and refactorings were rewritten; compiler plugins (Compose, serialization) needed new releases; kapt had to be reimplemented ([migration guide](https://kotlinlang.org/docs/k2-compiler-migration-guide.html)) | done over several releases |
| Small files slower in the IDE | K2 mode is "sometimes slower for very small/empty files" (secondary summary) | ongoing |

### Verdict For hd

- **Keep** explicit phases. Q3's task graph is K2's phase model with the
  phase order fixed per folder.
- **No compiler plugins: keep.** hd derives through templates written in hd
  ([templates](../../spec/lang/14-annotations.md#templates)), not compiler
  plugins. Kotlin's plugin churn is a cost we do not take on.

## Swift

### What It Chose

Swift checks expressions with a constraint solver. Overloading, literal
types and bidirectional inference create disjunctions. The new
`swift-driver` and explicitly built modules move module builds into the
build graph. Incremental builds track per-file interface hashes.

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Exponential expressions | "in the worst case, there is no better approach except to attempt each combination of disjunction choices"; limits are one million disjunction attempts and a 512 MB arena per expression ([roadmap](https://forums.swift.org/t/roadmap-for-improving-the-type-checker/82952)) | improved in 6.2 and 6.3 |
| Error cases are the slowest | a 12-line URL concatenation with one type error took 42 s; fixed, it took 0.19 s ([Hooper](https://danielchasehooper.com/posts/why-swift-is-slow/)) | improved |
| Special-case hacks | "Special-case optimizations … create unpredictable performance cliffs"; a "salvage mode" rechecks failed expressions (roadmap) | planned removal |
| Language changes considered | pruning operator overloads, and requiring `1.0` for float literals (roadmap) | under discussion |
| Explicit modules made builds slower | about 1,577% slower on one workspace: per-project working directories and macro plugin paths created module variants with "identical hashes" ([forums](https://forums.swift.org/t/build-times-regression-with-explicitly-built-modules/77073)) | workarounds |
| Textual interfaces fail to verify | `.swiftinterface` files that the compiler then cannot re-read ([#71252](https://github.com/swiftlang/swift/issues/71252), [#64669](https://github.com/apple/swift/issues/64669)) | recurring |
| Cross-module over-invalidation | adding a private top-level function recompiles every user of the module ([#92617](https://github.com/swiftlang/swift/issues/92617)) | open |

### Verdict For hd

- **Exponential checking: we avoid it by design.** hd has no overloading
  ([types.infer.no-overload](../../spec/lang/04-type-system.md#r-types.infer.no-overload)),
  no user operators, and literal widths resolved by union-find, not
  disjunctions. Keep it so. Any proposal that adds overloading or
  literal-driven choice should cite this table.
- **Error paths: add a fixture.** Swift's worst cases are ill-typed
  expressions. The `pathological` metric should include ill-typed
  versions of each stress case, since agents produce those first.
- **Variants: add a key-hygiene rule.** A key may contain only normalized
  inputs: no absolute paths, no working directory, no tool locations. A
  test (mine): check one package from two checkout paths and compare
  cache keys byte for byte.
- **Binary, same-build interfaces: keep.** Our blobs carry the compiler
  build ID and are never re-parsed as text, so Swift's verification
  failures cannot occur. Packages are distributed as source.

## Roc

### What It Chose

Roc rewrote its compiler from Rust to Zig: 354K lines of Rust became 320K
to 464K lines of Zig in 487 days ([Feldman](https://rtfeldman.com/rust-to-zig)).
The new compiler uses 32-bit indices, struct-of-arrays layouts, file-level
caching, and "zero-parse" deserialization that loads cache bytes without
parsing them. Its browser build is 2.5 MB.

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| An architecture problem forced the rewrite | lambda set resolution for defunctionalization was "architectural across several compiler phases" | rewritten |
| Rust build times | "a major pain point, even for incremental builds": 3.4 s against 0.035 s with Zig's incremental mode | motive |
| Memory safety in Zig | 10 memory bugs out of 431 in the new compiler, against 21 of 2,596 in the old; two were use-after-frees in error messages | small |
| Toolchain dependence | Zig 0.16 broke incremental builds for Roc | fixed in 0.17 |

### Verdict For hd

- **Our build loop: inherited risk.** Roc's main complaint about Rust is
  build time for the compiler itself. This hits agents writing hd's
  compiler. Q1 already lists it; mitigate by keeping crates small (Q7), so
  most edits rebuild one crate.
- **A feature that spans phases forced a rewrite.** For hd, the candidate is
  "code per value layout" with merged bodies (Part B). Keep that decision
  inside the back half, so the front half never depends on it.

## Gleam

### What It Chose

Gleam's compiler is in Rust and targets Erlang and JavaScript. It caches per
module: a `.cache` file with type information and a `.cache_meta` file with
the module's imports and compile time. A module recompiles when its source
mtime is newer or an upstream module recompiles
([Gleam 0.26](https://gleam.run/news/v0.26-incremental-compilation-and-deno/)).
The language server is part of the compiler.

### Documented Problems

| Problem | Evidence | Status |
| --- | --- | --- |
| Removed and restored files are not rebuilt | caches of missing sources were not deleted, and mtimes counted only in the language server ([#4320](https://github.com/gleam-lang/gleam/issues/4320)) | fixed |
| Cache bytes differ between identical builds | "Randomized HashMap/HashSet iteration in serialized cache collections", including "cache-loaded imported type ID assignment"; 12 hashes from 12 cold builds ([#6383](https://github.com/gleam-lang/gleam/issues/6383), 2026-10-01) | closed |
| No early cutoff | any upstream recompile recompiles dependents (design above) | by design |

### Verdict For hd

- **Hash-map order: we must test for it.** #6383 is the plainest form of
  lesson 2. Rust's `HashMap` uses a random seed by default. Interface
  serialization must walk sorted keys, and the `determinism` run should use
  several hasher seeds.
- **Deletion: add.** Q2's stat manifest must treat a missing file as a
  change, and the module set must come from the scan, not from cache
  entries.
- **Early cutoff: we avoid it.** Interface hashes stop the cascade Gleam has.

## Carbon

Carbon's toolchain is data-oriented: a token array, a postorder parse tree
and a flat semantic IR, all indexed by 32-bit IDs
([architecture](https://docs.carbon-lang.dev/toolchain/docs/)). Its benchmarks
on generated source show about 6.6M lines/s for lexing, 2.9M for parsing,
and 0.87M for checking on large files
([PR #4408](https://github.com/carbon-language/carbon-lang/pull/4408)).

Issues: the numbers come from generated code, and Carbon has no large real
codebase yet, so the check rate says little about generics-heavy code. I
found no postmortem.

**Verdict.** Our budget of 100k lines/s per core (from Sorbet) is
conservative next to Carbon's 0.87M. Keep the budget, and measure lexing and
parsing separately, as Carbon does, so a regression points at a phase.

## Lean 4

Lean 4's elaborator runs proofs in parallel and stores compiled modules as
`.olean` files. A new module system (experimental since 2025) separates a
public scope from a private one, so that changes to "proofs, comments, and
docstrings" do not rebuild importers
([reference](https://lean-lang.org/doc/reference/latest/Source-Files-and-Modules/)).
Resource use is limited by "heartbeats", a deterministic count of
allocations.

| Problem | Evidence | Status |
| --- | --- | --- |
| Generated code leaks private items into public signatures | `@[simps]` copies a hidden body into a public theorem; importers fail with "(kernel) unknown constant"; the proposed fix is to validate "that the exported type of a non-private declaration mentions only exported constants" ([#15401](https://github.com/leanprover/lean4/issues/15401), 2026-09-29; also [#15393](https://github.com/leanprover/lean4/issues/15393)) | open |
| A deterministic budget still moves | an internal change added allocations, and tests had to raise `maxHeartbeats` by "20–50%" ([4.31.0](https://lean-lang.org/doc/reference/latest/releases/v4.31.0/)) | by design |
| Instance search grows with the hierarchy | synthesis fails only "after exploring the full search space"; unbundling gave a 33% speedup ([Mathlib paper](https://arxiv.org/html/2508.21593v1)) | ongoing |

**Verdict.**

- **Interface leaks: add a validator.** hd's derive templates generate
  impls in the target's module, and template bodies are interface. Validate
  each interface blob when it is written: every exported signature, impl
  head and template body mentions only items visible to importers. Report
  a compiler bug, not a user error, if a derive output fails it.
- **Budget units: change.** Count language-level steps, such as impl
  candidates tried and goals expanded, not allocations. Then a compiler
  refactor never changes which programs pass.

## Sorbet

Sorbet (C++) checks about 100k lines/s per core with flat arrays and local
inference only ([Elhage](https://blog.nelhage.com/post/why-sorbet-is-fast/)).
Its language server has a fast path for edits that change no definitions and
a slow path that rechecks everything.

| Problem | Evidence | Status |
| --- | --- | --- |
| Speed alone stopped being enough | after five years of codebase growth, 19% of edits took the slow path; "delete everything" for a changed file cut it to 10%; the remaining causes were class definitions (33%), new files (28%) and others ([Jez](https://blog.jez.io/making-sorbet-more-incremental/)) | ongoing |
| Incremental work found old bugs | each change "uncovered pre-existing bugs, including Sorbet's most common crash" (same) | fixed |

**Verdict.** Keep local inference. Add to `recheck-precision` a report of
the share of edits in a scripted session that recheck more than one module
(mine), so we see Sorbet's 19% creeping up before users do.

## Flow Types-First

Flow's types-first mode builds each module's signature from its annotated
exports only, then checks every module against those signatures in
parallel. It reports `signature-verification-failure` when an export needs
inference ([Flow docs](https://flow.org/en/docs/lang/annotation-requirement/)).
At Facebook, rechecks became about 6x faster at p90 and 2x at p99 (secondary
summary of the [types-first post](https://medium.com/flow-type/types-first-a-scalable-new-architecture-for-flow-3d8c7ba1d4eb), unverified figures).
The cost was a migration: existing code needed annotations.

**Verdict.** We avoid the migration: hd requires public result types and
rows from the start. The p99 gain was much smaller than p90, because some
edits change signatures. That is lesson 6 again.

## Hack

Hack's `hh_server` is an OCaml daemon whose forked workers share an
immutable hash table in shared memory and talk over pipes. It records
fine-grained dependencies as 32-bit hashes and downloads saved states to
skip cold starts (secondary sources, unverified detail:
[OCaml discuss](https://discuss.ocaml.org/t/a-parallel-and-shared-memory-library-based-on-hacks-implementation/3428)).
Meta's later checkers moved off this model: Pyre's authors cite hard
multiprocess parallelism in OCaml as a reason for Pyrefly.

**Verdict.** We avoid it. Rust threads share memory directly, and our
cache is on disk with no daemon. Hack's saved states are a precedent for
a later shared remote cache: our keys are content hashes, so entries can be
shared between machines.

## Ruff, oxc And Biome

| Tool | Choice | Documented issue | Lesson for hd |
| --- | --- | --- | --- |
| Ruff | replaced a generated LALRPOP parser with a hand-written recursive descent parser: over 2x faster, with error recovery ([Ruff 0.4](https://astral.sh/blog/ruff-v0.4.0)) | the old parser stopped at the first syntax error | confirms Q4 |
| oxc | arena AST; type-aware linting calls a Go program, `tsgolint`, that runs tsgo ([oxc](https://oxc.rs/blog/2025-12-08-type-aware-alpha.html)) | two languages and two processes; repeated work across tsgo checkers ([#1175](https://github.com/oxc-project/tsgolint/issues/1175)) | keep one language for the whole toolchain |
| Biome 2 | its own TypeScript inference over a module graph built by a project scanner ([Biome v2](https://biomejs.dev/blog/biome-v2/)) | inference could "enter a very nasty loop where tons of types are recursively indexed, consuming a lot of memory"; it fully inferred imported generic declarations just to apply type arguments ([Biome 2.3](https://www.biomejs.cn/en/blog/biome-v2-3/)) | instantiate lazily, and bound type size (Q5's open limit) |

## Budgets And Schedule Independence

This section collects lessons 2 to 4 into one design note (mine), since no
single system above gets all three right.

1. **Content order.** Anything printed, hashed or used to break a tie is
   ordered by stable paths and structure, never by interned IDs, pointer
   values or hash-map order. Ties fall back to source position, never to an
   ID (#64589).
2. **Local counters.** Inference variables, closure indexes and generated
   names are numbered per body or per item, never per session (rustc's
   leaked allocation IDs).
3. **Budgets that ignore the cache.** A trait goal's memo entry stores the
   steps it took to compute. A later hit charges those steps to the asking
   body's budget. Then whether a goal exceeds `trait-resolution-depth` does
   not depend on which body computed it first, on how many threads run, or
   on what a warm cache holds.
4. **Budgets in language units.** Count goals, candidates and
   instantiations, not allocations or time (Lean, Swift).
5. **A memory cap.** A default cap on the process heap (for example 2 GB,
   overridable) and a per-body arena cap (Swift uses 512 MB per
   expression). Hitting either reports a diagnostic that names the item.
   Agents run many `hd check` processes at once, so one runaway check must
   not take the machine down (tsgo #2125, ty #4147).
6. **The test.** The `determinism` metric runs each case with 1, 4 and 16
   threads, three hasher seeds, two checkout paths, and a cold and a warm
   cache, and compares all outputs and cache keys byte for byte.

## Changes Suggested To COMPILER_ARCHITECTURE_RESEARCH.md

Concrete edits for the owner to approve. I have not made them.

1. **Summary row 2** (incremental model): append "The folder interface hash
   is deep: it includes the interface hashes of every folder its blob
   mentions, so re-exports and types reached through signatures are
   covered."
2. **[Q2](research.md#q2-incremental-model),
   Recommendation item 2:** add "The blob also stores a hash per exported
   item, covering its signature and the item hashes it mentions. v1 keys by
   folder; per-item keys are a later switch with no format change
   (Pyrefly, gopls typerefs)."
3. **Q2, Recommendation item 1:** add the std version, the target and the
   capability set to the key, and add "the key contains no absolute paths
   and no tool locations (Swift's module variants)."
4. **Q2, item 5 (stat manifest):** add "A file whose mtime is within the
   file system's timestamp granularity of the manifest's write time is
   hashed anyway (git's racily-clean rule). A missing file is a change,
   and the module set comes from the scan, not from the cache (Gleam
   #4320)."
5. **Q2, new paragraph "Eviction":** "Entries are touched on use at most
   once an hour, as Go does. A trim step runs at most daily and removes
   entries unused for N days, then oldest-first until the cache is under a
   size cap. The cap serves `cache-growth` (Go #81830)."
6. **Q2, Soundness and its tests:** add edit scripts for `pub use` chains,
   for a field change in a type reached only through a used signature, for
   option and std-version changes, and for deleting and restoring a file
   (TypeScript #64386 and #64552, Gleam #4320).
7. **Q2, Soundness:** add "Every fixpoint (folder-internal resolution,
   private inference, inferred rows) runs as one serial pass per strongly
   connected component, with a monotone join and an iteration bound that
   reports a diagnostic (salsa's 2026 cycle bugs)."
8. **[Q3](research.md#q3-parallel-checking), "No IDs
   in output":** extend with items 1 and 2 of
   [Budgets And Schedule Independence](#budgets-and-schedule-independence),
   and add "a cycle diagnostic points at the first function of the cycle in
   source order."
9. **Q3, Risks:** add "Memory per thread. tsgo's per-checker state costs
   about 8x checker memory at four threads. Target: peak RSS at 8 threads at
   most 1.5x peak RSS at 1 thread on the 50k-line `parallel-speedup` case"
   (the ratio is mine).
10. **[Q5](research.md#q5-checker-structure), Trait
    Resolution:** add item 3 of the budgets note (memo hits charge stored
    steps), and "cache 'T satisfies B' per body and globally (Go #66699)."
11. **Q5, Hard Limits:** add rows for the process heap cap and the per-body
    arena cap, and state that all limits count language-level steps.
12. **Q5, new paragraph:** "Interface validation. When a blob is written,
    check that exported signatures, impl heads and template bodies mention
    only exported items (Lean #15401)."
13. **[Q6](research.md#q6-program-database-hook):**
    add "Scanning per-module records is slow for global queries; gopls's
    find-implementations got 14x slower after its move to per-package
    files. Build a merged index before `hd callers` ships."
14. **[Q7](research.md#q7-crate-layout-and-build-order),
    slice 4 exit:** the `determinism` run varies threads, hasher seeds,
    checkout path and cache warmth; `pathological` includes ill-typed
    variants (Swift).
15. **Open questions, new:** (a) the default heap cap and whether the
    spec names a diagnostic code for it; (b) the default cache size cap.

## Part B: Back Ends, Wasm And Runtimes

This is a deliberately narrow pass over failures and constraints that can
change hd's first Wasm-GC implementation. It does not rank benchmark wins:
engine and optimizer measurements belong to spike 0c. Sources are primary
project documentation, issue trackers and advisories. A limitation is not a
compiler defect unless the source says it is one.

### Ranked Lessons For hd

| # | Lesson | Evidence | Action |
| --- | --- | --- | --- |
| 1 | A timeout or cancellation point is a state boundary, not an arbitrary instruction boundary. An engine that yielded in the middle of table growth, memory copies and GC array copies exposed invalid intermediate state and stale raw pointers. | [Wasmtime advisory GHSA-2hw9-mc66-jc2q](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-2hw9-mc66-jc2q) | **Add:** charge before a bulk operation and yield only before or after it; cancellation must never publish a half-initialized hd value. |
| 2 | Wasm-GC references are not a portable host ABI. They are opaque to JavaScript, and host-side roots have lifetime and allocation costs. | [Hoot ABI](https://gitlab.com/spritely/guile-hoot/-/blob/main/design/ABI.md), [Wasmtime `Rooted`](https://docs.wasmtime.dev/api/wasmtime/struct.Rooted.html), Kotlin's [`JsReference`](https://kotlinlang.org/docs/wasm-js-interop.html) | **Keep:** hd crosses scalars or the exchange buffer, never a guest GC reference. Add a test that the public import/export surface contains no GC reference type. |
| 3 | Guest heap limits do not bound host resources. Handles, copied strings and lists, buffered I/O and owned roots can exhaust host memory independently. | [Wasmtime resource-exhaustion advisory](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-852m-cvvp-9p4w), [Wasmtime C-API root leak](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-vvp9-h8p2-xwfc) | **Change:** count handles and host-call copy work as well as bytes; poison or discard a store after an engine OOM until Wasmtime's OOM contract is complete. |
| 4 | "Wasm" is not one deployment profile. A module can require Wasm GC, a particular exception encoding, tail calls, JSPI, a JavaScript host, WASI or private imports. | [Kotlin configuration](https://kotlinlang.org/docs/wasm-configuration.html), [Scala.js requirements](https://www.scala-js.org/doc/project/webassembly.html), [MoonBit FFI](https://docs.moonbitlang.com/en/latest/language/ffi.html) | **Add:** one versioned feature/import profile, checked before instantiation and included in every engine and precompile cache key. |
| 5 | Host interop and support modules can dominate both speed and size even when generated Wasm is good. | [Scala.js performance and size](https://www.scala-js.org/doc/project/webassembly.html), [Hoot host modules](https://gitlab.com/spritely/guile-hoot/-/blob/main/README.md), [Grain browser loader](https://github.com/grain-lang/grain-web-example) | **Change:** size and startup metrics include generated JS glue, auxiliary modules and all transitive Wasm, not only the main module. |
| 6 | Fast development code and optimized release code are different products. A low-level codegen IR can stay fast by keeping a stronger optimizer above or beside it. | [Cranelift's rustc goals](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/rustc.md) | **Keep:** hd's dev and optimized pipelines are separate. Do not make release quality a prerequisite for the first dev backend. |
| 7 | General effect handlers pay for continuation machinery that ordinary suspension need not pay. Tail-resumptive operations can compile as direct calls; general operations capture a continuation. | [Koka evidence passing](https://github.com/koka-lang/koka/blob/dev/doc/spec/tour.kk.md), [OCaml 5 effects](https://ocaml.org/manual/5.1/effects.html) | **Keep:** ready `await` stays a direct path and a frame appears only on suspension. Do not generalize the implementation to multi-shot effects. |
| 8 | GC choice remains an engine policy with observable latency and capacity tradeoffs. Copying GC uses half the heap; deferred reference counting leaks cycles; a null collector never frees. | [Wasmtime collector API](https://docs.wasmtime.dev/api/wasmtime/enum.Collector.html) | **Keep:** V8 through Node is the primary execution engine. Treat wasmtime collector selection as measured configuration, not language semantics. |

### MoonBit

**Choice.** MoonBit has separate Wasm, Wasm-GC, JavaScript, C and
experimental LLVM back ends. Plain Wasm and C use compiler-managed reference
counting; Wasm-GC and JavaScript reuse the host collector. Its Wasm FFI maps
foreign values to `externref`, while some standard packages depend on
MoonBit-specific host imports
([FFI documentation](https://docs.moonbitlang.com/en/latest/language/ffi.html)).

**Documented problem.** Portability depends on the import set, and plain-Wasm
FFI authors must maintain MoonBit reference counts correctly or cause memory
errors or leaks. The documentation also says the default FFI ownership is
being changed from owned to borrowed.

**Verdict for hd.** **Avoids** the ownership problem by exposing no guest GC
object at the ABI and **inherits** the import-profile problem. The generated
ABI table is the right design, but its exact profile must be versioned.

### dart2wasm

**Choice.** dart2wasm targets Wasm GC with JavaScript glue and currently runs
in JavaScript environments, not general engines such as wasmtime; deferred
loading is experimental
([Dart Wasm documentation](https://dart.dev/web/wasm)).

**Documented problems.** A large application exceeded V8's operand limit for
one generated `array.new_fixed`
([dart-lang/sdk#55396](https://github.com/dart-lang/sdk/issues/55396)). Another
large dynamic-dispatch surface produced an invalid `ref.cast` module even at
`-O0`, before `wasm-opt`
([dart-lang/sdk#63301](https://github.com/dart-lang/sdk/issues/63301)). Moving
to the final Wasm-GC encodings was explicitly breaking: old encodings stopped
working when engines switched
([dart-lang/sdk#53517](https://github.com/dart-lang/sdk/issues/53517)).

**Verdict for hd.** **Avoids** one-instruction giant constants through pooled
data and explicit loops, but **inherits** scale-only type-section and cast
risks. Keep `wasmparser` validation and add large nominal-hierarchy and large
literal cases; pin the accepted Wasm feature encoding in the engine profile.

### Kotlin/Wasm

**Choice.** Kotlin has distinct `wasmJs` and `wasmWasi` targets. Browser output
requires Wasm GC and exception handling; the two targets even default to
different exception proposal versions
([configuration](https://kotlinlang.org/docs/wasm-configuration.html)).

**Documented problems.** JavaScript interop supports a restricted type set,
opaque `JsReference` handles and ES modules only; arrays and dynamic values do
not cross directly
([interop differences](https://kotlinlang.org/docs/wasm-js-interop.html)).
Array bounds failures trap unless an optional check changes them into Kotlin
exceptions, so an engine choice can otherwise leak into language behavior.

**Verdict for hd.** **Avoids** the semantic split because checked versus
wrapping behavior is selected before emission and is engine-independent.
**Inherits** the need for separate browser and standalone import profiles;
their modules may share code, but they must not pretend to be interchangeable.

### wasm_of_ocaml

**Choice.** wasm_of_ocaml forked js_of_ocaml and translated OCaml bytecode to
WebAssembly. The work was later merged back into js_of_ocaml and the separate
repository was archived
([project README](https://github.com/ocaml-wasm/wasm_of_ocaml)).

**Documented problem.** The independent back end no longer has its own
maintenance path. This is a lifecycle fact, not evidence of a correctness
failure, but it shows the cost of keeping a parallel compiler fork alive.

**Verdict for hd.** **Avoids** the fork by sharing checked TIR, layouts and
reachability across targets. A future native target should add an emitter,
not clone the compiler pipeline.

### Scala.js Wasm

**Choice.** The stable Wasm backend preserves Scala.js semantics and emits an
ES module for a JavaScript host. It requires Wasm 3.0; async/await additionally
requires JSPI
([backend documentation](https://www.scala-js.org/doc/project/webassembly.html)).

**Documented problems.** It cannot yet emit multiple modules, silently ignores
`@JSExport` methods, can be significantly slower when JS interop dominates,
and produces about twice the full-link code size of the JS backend.

**Verdict for hd.** **Avoids** export ambiguity with one generated ABI and
**inherits** the interop and glue costs. One linked program is acceptable for
v1, but the size budget must include glue and engine compile time.

### Guile Hoot

**Choice.** Hoot uses Wasm GC and tail calls. To support delimited
continuations it transforms all calls to tail calls and keeps explicit return
state; browser reflection also loads auxiliary Wasm modules
([ABI](https://gitlab.com/spritely/guile-hoot/-/blob/main/design/ABI.md),
[README](https://gitlab.com/spritely/guile-hoot/-/blob/main/README.md)).

**Documented problems.** Wasm-GC values are opaque to JavaScript and require
explicit conversion. The ABI lists weak vectors, regular expressions, random
states and first-class threads as unsupported, and notes that Wasm GC objects
do not have multithreaded support.

**Verdict for hd.** **Avoids** the all-calls continuation transform because
hd lowers only suspending bodies to state machines. It also **avoids** GC-ref
interop through its exchange buffer. Keep threads off until both the language
and the selected engines define GC-object threading.

### AssemblyScript

**Choice.** AssemblyScript stores managed objects in linear memory. Its
default runtime combines TLSF allocation with incremental GC; `minimal`
requires the host to call `__collect` at safe points and `stub` never frees
([runtime documentation](https://www.assemblyscript.org/runtime.html)).

**Documented problem.** A host that keeps an unpinned pointer across an
allocation can observe premature collection and undefined behavior. The
smaller runtimes move lifetime correctness into host policy or deliberately
leak until the instance dies.

**Verdict for hd.** **Avoids** manual heap pointers and collector variants by
using Wasm GC. The analogous danger remains host roots, which the scalar and
buffer ABI deliberately excludes.

### Grain

**Choice.** Grain compiles through Binaryen
([compiler README](https://github.com/grain-lang/grain)). In the documented
browser setup, a JavaScript runner locates and fetches the entry module plus
transitive program and stdlib Wasm dependencies
([browser example](https://github.com/grain-lang/grain-web-example)).

**Documented problem.** Deployment is a module graph, not one artifact: the
host must find compatible copies of `another.wasm`, `pervasives.wasm` and
other dependencies before the program starts. The reviewed primary sources do
not publish a backend failure postmortem, so no stronger defect claim is made.

**Verdict for hd.** **Avoids** loader/version skew by linking one program
module. **Inherits** the general risk that support bytes are hidden from a
main-module size number.

### Go Wasm

**Choice.** Go supports JS-hosted Wasm and WASI. Go 1.24 added reusable WASI
reactors with an explicit `_initialize` call and exported functions
([Go Wasm exports](https://go.dev/blog/wasmexport)).

**Documented problems.** Execution is single-threaded. A background goroutine
stops making progress after an exported function returns until the host calls
the module again. Pointer-rich values cannot cross the 64-bit-Go/32-bit-Wasm
boundary directly, and host calls block all goroutines
([WASI limitations](https://go.dev/blog/wasi)).

**Verdict for hd.** **Avoids** an implicit scheduler: `poll`, `wake` and the
one-entry-at-a-time rule state exactly when work advances. **Inherits**
single-threaded execution intentionally. Add a startup-order assertion that
`hd.init` completes before any callable export.

### rustc_codegen_cranelift

**Choice.** cg_clif is a near drop-in rustc backend aimed first at faster
debug compilation. Its platform matrix is narrower than LLVM's
([project README](https://github.com/rust-lang/rustc_codegen_cranelift)).
Cranelift's own design describes a fast base IR and a possible stronger
optimizer in a separate IR above it
([rustc design note](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/rustc.md)).

**Documented problem.** The design explicitly does not expect the basic path
to compete with LLVM release optimization, and cg_clif's target support is
uneven. Cranelift's APIs are also not stable
([Cranelift status](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/README.md)).

**Verdict for hd.** **Avoids** promising one pipeline for both goals: hd's
optimized passes precede Wasm emission while its dev path stays small. It
**inherits** Wasmtime/Cranelift version and target coupling, so those versions
belong in precompile keys.

### Koka, Effekt And OCaml 5 Effects

**Choices.** Koka uses generalized evidence passing and directly calls
tail-resumptive operations; only general operations yield, capture and later
resume a continuation
([Koka tour](https://github.com/koka-lang/koka/blob/dev/doc/spec/tour.kk.md)).
Effekt translates to capability-passing style and then Core, with backend-
specific lowerings; its LLVM and JIT routes are documented as work in progress
([implementation](https://github.com/effekt-lang/effekt-website/blob/main/docs/implementation.md)).
OCaml 5 implements handlers with runtime-managed stack fibers and restricts
captured continuations to one resumption
([OCaml manual](https://ocaml.org/manual/5.1/effects.html)).

**Documented problems.** Koka has an open report of multi-megabyte tiny
binaries, including a Wasm build failure on one platform
([koka#277](https://github.com/koka-lang/koka/issues/277)). Effekt's native
back ends remain research work. In OCaml, failing to resume or discontinue a
continuation leaks the fiber and resources; finalizers are more expensive than
capture, and effect safety is not statically enforced.

**Verdict for hd.** **Avoids** general handler machinery and multi-shot
continuations. Its lazy suspension frame is the same valuable split as Koka's
direct tail-resumptive path. Cancellation's mandatory child abort and `defer`
unwind avoid OCaml's abandoned-continuation leak only if every host handle is
owned by exactly one frame and tested on every exit.

### Wasmtime GC And Embedding

**Choice.** Wasmtime exposes copying, deferred-reference-counting and null
collectors. The copying collector has throughput but stop-the-world latency
and half-heap utilization; DRC cannot collect cycles; null GC never frees
([collector API](https://docs.wasmtime.dev/api/wasmtime/enum.Collector.html)).
Host references are either cheap LIFO-scoped roots or independently allocated
owned roots
([root API](https://docs.wasmtime.dev/api/wasmtime/struct.Rooted.html)).

**Documented problems.** The 2026 bulk-operation advisory showed that
mid-operation fuel/epoch callbacks could expose nulls in non-null tables, use
stale memory pointers or corrupt the GC heap. A separate advisory found
unbounded host resources and guest-sized copies in WASI. A pooled-GC metadata
cache bug could abort the host
([wasmtime#13417](https://github.com/bytecodealliance/wasmtime/issues/13417)),
and complete allocation-failure recovery remains tracked work
([wasmtime#12069](https://github.com/bytecodealliance/wasmtime/issues/12069)).

**Verdict for hd.** **Inherits** these engine risks. Store isolation, an
aggregate guest limit and host-buffer bytes are good starts, but handle count,
copy-work fuel, root lifetime, poisoned-store behavior and atomic timeout
boundaries are not yet complete. Node/V8 should remain the primary engine;
wasmtime conformance is valuable but must not define language semantics.

### Changes Suggested To The Back-End Design

Concrete edits for the owner to approve. I have not made them.

1. **`suspension.md` §14.6, cancellation:** state that generated code may
   observe cancellation only between state-machine steps. A bulk primitive
   charges its full work before entry and either completes atomically or traps;
   it never yields with a partially initialized hd value reachable.
2. **`runtime-and-host.md` §17.8, limits:** add a per-store handle-count limit
   and host-call work fuel proportional to bytes copied or decoded. Count
   owned GC roots, pending results and provider buffers against host resources,
   independently of the guest heap limit.
3. **`runtime-and-host.md` §17.8, engine failure:** after engine OOM, an epoch
   callback that mutates the store, or an interrupted non-hd bulk operation,
   poison and discard the store. Do not map such a failure to a resumable hd
   panic until the pinned engine documents that recovery as safe.
4. **`runtime-and-host.md` §§16.4 and 17.1:** give the ABI a versioned engine
   profile containing required Wasm proposals, exception encoding, import
   modules and host kind. Validate it before `hd.init`; include its hash in
   link, browser and precompile keys.
5. **`wasm-layout.md` §§15.2 and 15.7:** make "no GC references in public
   imports or exports" an emission invariant and validator test. Only scalars,
   handles and exchange-buffer lengths may cross the host ABI.
6. **`engines-and-test-runner.md` §18:** add a startup matrix that validates
   every emitted module on pinned Node/V8 and, where a decision rule names it,
   default wasmtime. Exercise the exact feature profile, host imports and
   exception behavior; do not compare Cranelift optimization levels or
   allocators.
7. **`engines-and-test-runner.md` §§18.1 and 18.3:** add collector cases for a
   cycle, a long-lived set near half the copying heap, pooled-store reuse,
   scoped and owned host roots, and allocation failure. Collector choice stays
   outside language semantics.
8. **`codegen.md` §12.8 and metrics:** report the main Wasm, generated JS,
   auxiliary runtime modules and transitive program modules separately and in
   total. Apply startup and download budgets to the total.
9. **`runtime-and-host.md` §17.9:** specify startup as validate profile,
   instantiate, bind providers, run `hd.init`, then expose callable entries.
   A host callback before the instance reaches Ready is a host-contract error.
10. **`suspension.md` §14:** record the non-goal: v1 suspension is a one-shot
    compiler state machine, not a general algebraic-effect or multi-shot
    continuation facility. Preserve the allocation-free ready path.

## Sources

TypeScript 7:

- https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/
- https://devblogs.microsoft.com/typescript/announcing-typescript-7-0-beta/
- https://devblogs.microsoft.com/typescript/progress-on-typescript-7-december-2025/
- https://www.typescriptlang.org/docs/handbook/release-notes/typescript-6-0.html
- https://github.com/microsoft/TypeScript/issues/64589
- https://github.com/microsoft/TypeScript/issues/64423
- https://github.com/microsoft/TypeScript/issues/64386
- https://github.com/microsoft/TypeScript/issues/64552
- https://github.com/microsoft/typescript-go/issues/2125
- https://github.com/microsoft/typescript-go/issues/1622
- https://github.com/microsoft/typescript-go/issues/2551
- https://github.com/microsoft/typescript-go/issues/2666
- https://github.com/microsoft/typescript-go/issues/2115
- https://github.com/microsoft/typescript-go/discussions/458
- https://zackoverflow.dev/writing/why-does-tsgo-use-so-much-memory/
- https://github.com/oxc-project/tsgolint/issues/1175

ty, salsa and Pyrefly:

- https://astral.sh/blog/ty
- https://github.com/astral-sh/ty/issues/4147
- https://github.com/astral-sh/ty/issues/4607
- https://github.com/astral-sh/ty/issues/4615
- https://github.com/astral-sh/ty/issues/4616
- https://github.com/astral-sh/ty/issues/4618
- https://github.com/salsa-rs/salsa/issues/1326
- https://github.com/salsa-rs/salsa/issues/1336
- https://github.com/salsa-rs/salsa/issues/1338
- https://github.com/salsa-rs/salsa/issues/1349
- https://github.com/salsa-rs/salsa/issues/1350
- https://github.com/salsa-rs/salsa/pull/1348
- https://github.com/salsa-rs/salsa/pull/967
- https://ndmitchell.com/downloads/slides-pyrefly-07_oct_2025.pdf
- https://pyrefly.org/blog/2026/02/06/performance-improvements/

rustc and rust-analyzer:

- https://github.com/rust-lang/compiler-team/issues/1005
- https://kobzol.github.io/rust/2026/09/30/stf-august-september-2026.html
- https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/
- https://github.com/rust-lang/rust/issues/163760
- https://github.com/rust-lang/rust/issues/48172
- https://github.com/rust-lang/rust-analyzer/issues/19402
- https://github.com/rust-lang/rust-analyzer/issues/19404
- https://github.com/rust-lang/rust-analyzer/pull/18964
- https://github.com/rust-lang/rust-analyzer/pull/20329
- https://ferrous-systems.com/blog/rust-analyzer-next-few-years/

Go:

- https://go.dev/blog/gopls-scalability
- https://groups.google.com/g/golang-checkins/c/wxBReKH7sVA
- https://pkg.go.dev/golang.org/x/tools/gopls/internal/cache/typerefs
- https://pkg.go.dev/golang.org/x/tools/go/gcexportdata
- https://github.com/golang/go/issues/73709
- https://github.com/golang/go/issues/37790
- https://github.com/golang/go/issues/81830
- https://github.com/golang/go/issues/66699
- https://go.dev/doc/go1.18

Zig and Roc:

- https://ziglang.org/download/0.17.0/release-notes.html
- https://ziglang.org/devlog/2026/
- https://github.com/ziglang/zig/issues/25872
- https://github.com/ziglang/zig/issues/22236
- https://github.com/ziglang/zig/pull/20632
- https://rtfeldman.com/rust-to-zig

Kotlin and Swift:

- https://blog.jetbrains.com/idea/2025/04/the-story-behind-k2-mode-and-how-it-works/
- https://kotlinlang.org/docs/whatsnew20.html
- https://kotlinlang.org/docs/k2-compiler-migration-guide.html
- https://forums.swift.org/t/roadmap-for-improving-the-type-checker/82952
- https://danielchasehooper.com/posts/why-swift-is-slow/
- https://forums.swift.org/t/build-times-regression-with-explicitly-built-modules/77073
- https://github.com/swiftlang/swift/issues/71252
- https://github.com/apple/swift/issues/64669
- https://github.com/swiftlang/swift/issues/92617

Gleam, Carbon, Lean, Sorbet, Flow, Hack:

- https://gleam.run/news/v0.26-incremental-compilation-and-deno/
- https://github.com/gleam-lang/gleam/issues/4320
- https://github.com/gleam-lang/gleam/issues/6383
- https://docs.carbon-lang.dev/toolchain/docs/
- https://github.com/carbon-language/carbon-lang/pull/4408
- https://lean-lang.org/doc/reference/latest/Source-Files-and-Modules/
- https://github.com/leanprover/lean4/issues/15401
- https://github.com/leanprover/lean4/issues/15393
- https://lean-lang.org/doc/reference/latest/releases/v4.31.0/
- https://arxiv.org/html/2508.21593v1
- https://blog.nelhage.com/post/why-sorbet-is-fast/
- https://blog.jez.io/making-sorbet-more-incremental/
- https://flow.org/en/docs/lang/annotation-requirement/
- https://medium.com/flow-type/types-first-a-scalable-new-architecture-for-flow-3d8c7ba1d4eb
- https://discuss.ocaml.org/t/a-parallel-and-shared-memory-library-based-on-hacks-implementation/3428

Ruff, oxc and Biome:

- https://astral.sh/blog/ruff-v0.4.0
- https://oxc.rs/blog/2025-12-08-type-aware-alpha.html
- https://biomejs.dev/blog/biome-v2/
- https://www.biomejs.cn/en/blog/biome-v2-3/
