# New Compiler: Architecture Research

Status: Research, not decided. Part 1 of 2: the front half, 2026-10-06.

This document surveys prior art for the front half of the new compiler:
the implementation language, the incremental model, parallel checking, the
parser and CST, the checker's structure, the program-database hook, and a
crate layout with a build order. Part 2 covers the back half.

Fixed inputs, not reopened here:

- The goals, the Arena pillars, the goal metrics, and the feature triage in
  [NEW_COMPILER_ARCHITECTURE.md](NEW_COMPILER_ARCHITECTURE.md).
- The owner's decisions listed there: Wasm-only v1 run on wasmtime inside
  `hd`, no daemon, parallel and incremental checking as must-haves, a
  lossless CST with recovery, per-item recovery with poison types, the CLI as
  a host on the embedding API, and the program database after v1.
- A requirement the owner added on 2026-10-06: **the compiler itself runs in
  the web playground.** Check, build and test compile to Wasm and run in a
  browser tab. The playground runs the user's program on the browser's own
  engine (V8), not wasmtime. See [Running In The Browser](#running-in-the-browser).

Conventions. "Unverified" marks a claim I could not confirm from a source.
"Mine" marks a design idea of mine rather than shipped practice. Confidence is
high (I would bet on it), medium (likely, with a named risk), or low (a lean).

## Summary Of Recommendations

| # | Recommendation | Confidence |
| --- | --- | --- |
| 1 | Write the compiler in **Rust**. One codebase builds the native `hd` (with wasmtime) and a browser Wasm build (without it). | medium-high |
| 2 | Incremental model: a **content-addressed on-disk cache per module**, keyed by the module's source hash and the interface hashes it reads. Early cutoff comes from interface hashes. No salsa and no persisted fine-grained dependency graph. | medium-high |
| 3 | The **folder** (the spec's acyclic compile unit) resolves interfaces together; the **module** is the cache unit; **items** are the parallel task unit inside a run. | medium-high |
| 4 | Parallel checking as a task graph: parse all files, then folder interfaces in dependency order, then every body as an independent task. The same code runs on one thread with byte-identical output. | high |
| 5 | Data-oriented core: interned 32-bit IDs, per-folder and per-body arenas, flat token and node arrays, a zero-copy interface format, no IDs in any output or hash. | high |
| 6 | A hand-written lexer that emits layout tokens, and a hand-written resilient recursive-descent parser that builds a lossless flat green tree. | high |
| 7 | Checker: bidirectional inference local to each body, a poison type with root-cause suppression, trait lookup through a head index with memoized, step-bounded search, and rows as small sorted sets. | medium-high |
| 8 | Program database hook: stable string symbol IDs and per-module fact records written into the module cache entry from day 1; the queryable store comes later. | medium |
| 9 | Browser: the same core compiled to `wasm32-unknown-unknown`, single-threaded by default, behind four host interfaces (files, cache store, scheduler, capability host). | medium-high |
| 10 | Build order: syntax first against the parse fixtures, then **lib/std as the first real program**, then the typing fixtures by chapter, with the cache and threads wired in from the second slice. | medium |

## Running In The Browser

The owner's requirement (2026-10-06): the compiler runs in a browser tab, and
the user's program runs on V8 there. Today's prototype does this from a
14.6 MB worker bundle, mostly Binaryen's JavaScript build
([playground README](../website/playground/README.md#how-it-works)).

The constraints, and how each recommendation meets them:

| Constraint | What it rules out | How the recommendations meet it |
| --- | --- | --- |
| The compiler is a Wasm module loaded over the network | languages with a large or slow Wasm runtime | Rust compiles to `wasm32-unknown-unknown` with no runtime; parser-sized tools ship at about 0.75 to 2.2 MB ([Q1](#q1-implementation-language)) |
| No file system | any direct `std::fs` call in the core | the core reads sources through a `SourceSet` interface and the cache through a `CacheStore` interface; the browser gives in-memory versions ([Q2](#q2-incremental-model)) |
| No persistent cache by default | a design that is slow without a warm cache | the std interface is precomputed and embedded in the binary; playground programs are small, so a cold check is the normal case |
| IndexedDB is asynchronous | a synchronous cache read during checking | the JS host loads a cache snapshot before a run and writes new entries after it; the core never awaits storage |
| Threads need `SharedArrayBuffer`, which needs cross-origin isolation (COOP and COEP headers) | a design that needs threads to be correct or fast enough | the scheduler has a serial mode with identical output; a threaded build is an option for an isolated page ([Q3](#q3-parallel-checking)) |
| The program runs on V8, not wasmtime | front-half code that links wasmtime | wasmtime and Cranelift live only in the native CLI crate; the front half never depends on them ([Q7](#q7-crate-layout-and-build-order)) |
| The host interface needs a JS implementation | a capability host written only against wasmtime's API | one host interface with two implementations: wasmtime in `hd`, and JS in the playground (Part 2 designs the host side) |
| Download size | an unbounded browser build | a size budget is an [open question](#open-questions-for-the-owner); my proposal is at most 5 MB uncompressed for check, build and test |

## Q1: Implementation Language

**Question.** Rust, Go, Zig or OCaml (C++ and Swift briefly)? Weigh native
speed, arenas and interning, parallelism, embedding wasmtime and Cranelift,
the compiler's own build speed, how well agents write the language, and the
browser build.

### Prior Art

- **TypeScript's native port (tsgo, "Corsa") chose Go.** The stated reasons:
  it is a *port*, so the code must stay structurally similar to the JS
  compiler; idiomatic Go resembles the existing code; Go gives control of
  memory layout without making every line manage memory; and GC costs little
  in a batch process. Hejlsberg credited about 3x from native code and about
  3x from parallelism. The team also said Go's in-process JS interop "is not
  as good as some of its alternatives"
  ([discussion #411](https://github.com/microsoft/typescript-go/discussions/411)).
  Asked about the browser, Hejlsberg answered "Yes!", but the team had not
  measured or shrunk the Wasm size
  ([discussion #458](https://github.com/microsoft/typescript-go/discussions/458)).
  Our case is different: we are not porting, so the main reason does not apply.
- **Roc moved its compiler from Rust to Zig.** Published 2026-07-15. 354K
  lines of Rust became 320K to 464K lines of Zig in 487 days. Reasons: an
  architecture problem that needed a rewrite anyway, build times, an
  allocator-centric ecosystem, struct-of-arrays as the norm, and help with the
  unsafe code of zero-parse deserialization. Incremental rebuild of the
  compiler: Rust 3.4 s, Zig 0.17 0.035 s (Zig 0.16 shipped with a bug that
  broke incremental builds on their code). Memory-safety bugs: 21 of 2,596
  (Rust) against 10 of 431 (Zig). Roc's browser compiler is 2.5 MB
  ([rtfeldman.com](https://rtfeldman.com/rust-to-zig)).
- **Bun moved from Zig to Rust, written by agents.** In May 2026, 535K lines
  of Zig were ported in 11 days by 64 parallel Claude agents, passing 99.8% of
  the test suite. The stated reason: most bugs were use-after-free,
  double-free and missed frees, which "in safe Rust are compiler errors"
  ([Simon Willison](https://simonwillison.net/2026/Jul/8/rewriting-bun-in-rust/),
  [devclass](https://www.devclass.com/software/2026/05/11/anthrophics-bun-team-trials-port-from-zig-to-rust/5237835)).
  This is the strongest data point on agents writing a large systems codebase.
- **Rust tools of this shape:** rust-analyzer, ty (Astral's Python checker,
  on salsa), Ruff, oxc, Biome and swc. oxc's parser is 3x faster than swc
  and 5x faster than Biome's CST parser on one benchmark; moving the AST to
  an arena gave about 20%
  ([oxc benchmarks](https://oxc.rs/docs/guide/benchmarks),
  [oxc performance](https://oxc.rs/docs/learn/performance)).
- **Zig's own compiler** is the reference for data-oriented design: an
  InternPool and 32-bit indices throughout
  ([PR #15569](https://github.com/ziglang/zig/pull/15569)).
- **Sorbet (C++)** checks about 100,000 lines per second per core and starts
  in under 30 ms ([Nelson Elhage](https://blog.nelhage.com/post/why-sorbet-is-fast/)).
  **Carbon (C++)** targets 1M lines per second for semantic analysis
  ([parse.md](https://github.com/carbon-language/carbon-lang/blob/trunk/toolchain/docs/parse.md)).
  Both are fast, but C++ gives agents no memory-safety net.
- **MoonBit's compiler** is reported to be written in OCaml (unverified).
  OCaml in the browser: wasm_of_ocaml targets Wasm GC and runs 2x to 8x faster
  than js_of_ocaml but about 2x slower than native, with larger output
  ([Tarides](https://tarides.com/blog/2025-02-19-the-first-wasm-of-ocaml-release-is-out/)).

### Browser Builds

| Language | Browser target | Size evidence | Threads in the browser |
| --- | --- | --- | --- |
| Rust | `wasm32-unknown-unknown` with wasm-bindgen, or `wasm32-wasip1` with a shim | oxc parser 0.75 MB (web build) and 2.2 MB (threaded WASI build); Biome's web package 26.6 MB unpacked ([oxc wasm](https://unpkg.com/browse/@oxc-parser/wasm@0.47.1/), [Biome](https://www.npmjs.com/package/@biomejs/wasm-web)) | yes, with `SharedArrayBuffer` ([wasm-bindgen-rayon](https://github.com/RReverser/wasm-bindgen-rayon)); oxc ships threaded and threadless builds ([oxc PR #26898](https://github.com/oxc-project/oxc/pull/26898)) |
| Go | `GOOS=js GOARCH=wasm` or `wasip1` | about 2 MB minimum, often 10 MB or more; TinyGo is smaller but limited ([Go wiki](https://go.dev/wiki/WebAssembly)) | no: goroutines share one thread ([Go wiki](https://go.dev/wiki/WebAssembly)) |
| Zig | `wasm32-freestanding` | output holds only what you wrote; Roc's whole compiler is 2.5 MB | possible with atomics, no shipped compiler example found |
| OCaml | wasm_of_ocaml (Wasm GC) | larger than js_of_ocaml | no shared-memory threads found (unverified) |

### Comparison

| Criterion | Rust | Go | Zig | OCaml |
| --- | --- | --- | --- | --- |
| Speed of a checker | native, no GC | native, GC (cheap in a batch run) | native, no GC | native, GC; good at tree code |
| Arenas, interning, SoA | good (bumpalo, index types); global allocator assumed | weak: GC heap, no arenas in practice | best: allocators everywhere | weak |
| Parallelism | rayon, scoped threads; data races are compile errors | goroutines, easy; races are runtime bugs | threads, manual | OCaml 5 domains, newer |
| wasmtime and Cranelift | native crates | cgo through the C API ([wasmtime-go](https://pkg.go.dev/github.com/bytecodealliance/wasmtime-go)) | C API, bindings "work in progress" ([wasmtime-zig](https://github.com/zigwasm/wasmtime-zig)) | C stubs |
| Browser build | good, with threads optional | large, single-threaded | small | Wasm GC, 2x slower than native |
| Build speed of the compiler | slow; needs crate splitting | fastest | fast, very fast incremental | fast |
| Agents writing it | strong evidence (Bun port); compiler errors catch agent mistakes | strong; simple language | weak: little training data, churn before 1.0 ([Zig challenge](https://akitaonrails.com/en/2026/01/11/ai-agents-comparing-top-llms-on-the-zig-challenge/)) | moderate |
| Ecosystem for this job | wasm-encoder, wasmparser, rayon, salsa, rowan, bumpalo, xxhash, blake3 | x/tools, no Wasm GC encoder found | small | menhir, ppx; small Wasm tooling |
| Stability | stable | stable | pre-1.0, breaking releases | stable |

C++ (Sorbet, Carbon) matches Rust's speed but gives agents no protection
against memory bugs, which is Bun's lesson. Swift has a thin story off Apple
platforms and in Wasm; I dropped it early.

### Recommendation

**Rust.** Confidence: medium-high.

- v1 embeds wasmtime and the post-v1 backend uses Cranelift. Both are Rust
  crates. Any other language reaches them through a C API, and Go also through
  cgo, which complicates cross-compiling and adds a second runtime.
- The browser build works with or without threads, and parser-sized Rust
  tools ship today at under 1 MB.
- Agents write Rust well at scale (Bun), and the borrow checker turns their
  memory and data-race mistakes into compile errors. For a parallel checker,
  that matters more than in a serial one.
- The ecosystem already has what the front half needs: an arena (bumpalo),
  work stealing (rayon), Wasm encoding and validation (wasm-encoder,
  wasmparser), and fast hashing (xxhash, blake3).

### Risks

- **Build time of the compiler itself.** Roc measured 3.4 s incremental
  rebuilds in Rust against 35 ms in Zig. Agents pay that on every iteration.
  Mitigation: many small crates in a strict layer order; wasmtime and Cranelift
  in a leaf crate so front-half work never rebuilds them; agents iterate with
  `cargo check` and crate-scoped tests. Rust's Cranelift codegen backend for
  debug builds may help (not measured here).
- **Indices fight the borrow checker less than pointers do, but bounds bugs
  remain.** Typed index newtypes catch mixing; debug builds keep bounds checks.
- **Global-allocator assumption.** Roc found this limiting. Per-phase arenas
  with bumpalo cover the front half; I found no blocker.

**What would change it.** If a scripted measurement shows Rust's edit-to-test
loop on the compiler above about 15 s for typical front-half crates even after
splitting, Go becomes the fallback for the front half. That would cost the
browser threads and add cgo for wasmtime. Zig becomes interesting after a
1.0 and mature wasmtime bindings.

## Q2: Incremental Model

**Question.** Which incremental model fits a compiler with **no daemon**,
where every `hd check` is a fresh process? What granularity, how does early
cutoff work, what is persisted, what does loading it cost, and how is it
kept sound? The hypothesis to test: a Go-style content-addressed per-module
cache keyed by source hash and dependency interface hashes, with in-process
parallelism, fits better than salsa.

### Prior Art

| System | Granularity | Early cutoff | Persisted | Works without a daemon | Correctness record |
| --- | --- | --- | --- | --- | --- |
| rustc queries | query (item level) | red-green marking; a recomputed result with an equal 128-bit fingerprint stays green | the whole dependency graph and cached query results, rewritten each session ([dev guide](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)) | yes | 1.52.1 disabled incremental after "unstable fingerprint" ICEs exposed bugs that could miscompile; re-enabled in 1.54 ([Rust blog](https://blog.rust-lang.org/2021/05/10/Rust-1.52.1/)); such ICEs are still filed in 2026 ([#163760](https://github.com/rust-lang/rust/issues/163760)) |
| salsa (rust-analyzer, ty) | query | backdating: an equal value keeps its old revision | nothing; disk persistence is a long-open request ([salsa #10](https://github.com/salsa-rs/salsa/issues/10)) | no; rust-analyzer takes a dozen seconds to rebuild state on start ([Ferrous Systems](https://ferrous-systems.com/blog/rust-analyzer-next-few-years/)) | good in-memory; cycles and memory were hard |
| Zig `-fincremental` | four kinds of analysis unit per declaration | source hashes per declaration | nothing yet: "incremental compilation currently only works if the compiler process stays running"; it needs `zig build --watch` ([issue #6538](https://github.com/ziglang/zig/issues/6538)) | no | new; Roc hit a release that broke it |
| Go build cache | package | export data hash: a dependent's action ID includes it | per-package compiled output and export data, keyed by an action ID hash ([cmd/go](https://pkg.go.dev/cmd/go#hdr-Build_and_test_caching)) | yes | very good: hermetic inputs |
| gopls v0.12 | package | same | per-package summaries in a file cache: declaration types, cross-references, method sets ([Go blog](https://go.dev/blog/gopls-scalability)) | yes; several instances share the cache | memory down about 75% on average |
| OCaml with dune | module | the `.cmi` digest; dune rules cut off on equal outputs ([Tarides](https://tarides.com/blog/2022-07-12-faster-incremental-builds-with-dune-3/)) | `.cmi`, `.cmx`, and dune's rule cache | yes | bugs where a rule read an input its key missed ([dune #16484](https://github.com/ocaml/dune/issues/16484)) |
| Swift | file, plus "fine-grained" names | per-file interface hash of the tokens other files can see | `.swiftdeps` per file | yes | cross-module edits still recompile every user of the module ([swift #92617](https://github.com/swiftlang/swift/issues/92617)) |
| Kotlin | class, member-level ABI snapshots | ABI snapshot comparison ([Kotlin docs](https://kotlinlang.org/docs/gradle-compilation-and-caches.html)) | classpath snapshots plus caches | yes (with a Gradle daemon in practice) | rewrote IC in 1.8.20 |
| TypeScript | file | a file's signature is the hash of its emitted `.d.ts`, so a body edit does not touch dependents | `.tsbuildinfo`: file hashes, signatures, graph, diagnostics ([DeepWiki summary](https://deepwiki.com/microsoft/TypeScript/8-incremental-and-project-builds)) | yes | good; coarse |
| Buck2 DICE | build key | equality on recomputed values | nothing; in-memory per daemon ([DICE](https://buck2.build/docs/insights_and_knowledge/modern_dice/)) | no | lesson: untracked reads break it |
| Bazel | action | content hashes of outputs | content-addressed action cache, local or remote | yes | good: hermetic sandboxes |

Lessons from the table:

- **Every fine-grained engine that works well is in memory.** salsa, DICE
  and Zig keep their graphs in a live process. rustc is the one that
  persists a fine-grained graph, and it has the worst correctness record and
  a load cost proportional to the crate.
- **Every system that works across processes uses coarse, hermetic units.**
  Go, Bazel, dune, TypeScript and Swift key a unit by its inputs' hashes. A
  dependent's key includes the dependency's *interface* hash, which gives
  early cutoff for free.
- **Correctness comes from hermeticity, not from tracking.** rustc's and
  dune's bugs are reads the key did not cover. A unit whose checker can only
  see its own sources and the interfaces it was handed cannot have that bug.

### What hd's Spec Already Gives

- **Explicit public signatures.** Public functions, methods and trait
  methods must write their result type and row
  ([functions](../spec/lang/07-functions.md#parameter-and-result-types),
  [rows](../spec/lang/11-requirements-and-suspension.md#omitted-requirement-clauses)).
  A body edit cannot change a module's interface.
- **An acyclic folder graph.** Files of one folder may use each other in
  loops, but folders form a DAG, and the spec says why: "a folder then
  compiles from the signatures of the folders it uses, as a Go package
  compiles from its imports' export data"
  ([folder graph](../spec/lang/10-modules.md#folder-graph)).
- **No wildcard imports.** A module's uses name exactly which declarations
  it reads, so dependency edges are known from syntax.
- **Inference stays inside a module.** Only private functions may omit a
  result type or row, and the inferred type never crosses the module.
- **Template bodies are interface.** A package interface carries each
  derivation template's body ([limits](../spec/lang/14-annotations.md#limits)).
  So an edit to a template body changes the interface, as an edit to an
  inline function's body does in C++ or Swift.

### Recommendation

**A two-level hybrid.** Confidence: medium-high. The hypothesis holds, with
one change: the folder resolves interfaces, and the module is the cache unit.

1. **On disk, per module.** A module's check result is a cache entry. Its
   key is a hash of:
   - the module's source bytes;
   - the interface hash of its own folder (the public signatures of every
     module in the folder, since siblings may use each other);
   - the interface hashes of the folders and packages it uses;
   - the compiler build ID, the target, and the options that change checking.

   The entry holds the module's diagnostics, its program-database facts
   ([Q6](#q6-program-database-hook)), and later its lowered IR for the back
   half.
2. **On disk, per folder interface.** A folder's interface is a serialized,
   indexed blob: public items, their signatures, impl heads, template bodies,
   and the stable path of each item. Its hash is the folder's interface hash.
   This is Go's export data with its lazy index
   ([Go compiler README](https://go.dev/src/cmd/compile/README)).
3. **Early cutoff from header hashes.** Like Swift's interface hash, each
   module also stores a hash of its header tokens: everything outside
   function bodies. After an edit, the driver parses only the changed file.
   An unchanged header hash means an unchanged folder interface, so no other
   module's key changes. A private body edit therefore rechecks exactly one
   module, which is the `recheck-precision` target.
4. **In memory, per item.** Inside one run, memo tables cover what several
   bodies share: trait lookups, template instantiations, generic
   instantiations. They are plain hash maps keyed by interned IDs, dropped
   at exit. No salsa.
5. **A stat manifest** per package, as Bazel and Go keep: path, size, mtime
   and content hash for each source. A warm check stats the files, hashes
   only those whose stat changed, and reads cached entries. That is how the
   `io-per-check` and warm `resources` targets are met.

Why not salsa: it gives what a long-lived process needs, and we have none.
On a fresh process it would rebuild everything unless we add persistence,
which salsa lacks and rustc shows is hard to get right. hd's explicit
signatures make the coarse unit almost as precise as the fine one: a body
edit rechecks one module, and a module is milliseconds of work.

**Cold-start cost.** Loading is lazy. A run maps the manifest, then opens
only the interface blobs its changed modules need, and decodes only the
items they name (Go's indexed format). The std interface is embedded in the
binary in the same format, as Sorbet links its serialized stdlib into the
executable. Roc's "zero-parse deserialization" is the same idea.

**Soundness and its tests.**

- **Hermetic by API** (mine, in Bazel's spirit). The module checker's entry
  takes the module's source and a set of interface handles. It has no
  access to the file system or to other modules' sources.
- **Stable, canonical hashing.** Interface blobs serialize in a canonical
  order keyed by stable paths, never by interned IDs or hash-map order.
  TypeScript had to add `stableTypeOrdering` for the same reason.
- **The `incremental-soundness` metric** runs random edit scripts and
  compares each incremental result to a clean check.
- **A verify mode**, like rustc's incremental verification: recheck a cache
  hit and fail if the result differs. Run it in CI on the conformance suite.
- **Atomic writes:** write to a temporary name, then rename. Entries are
  immutable and content-addressed, so concurrent writers of one key write the
  same bytes (`cache-contention`).

**Where the cache lives.** The cache sits behind a `CacheStore` interface
with `get(key)` and `put(key, bytes)`. Natively it is a directory under
`HD_CACHE`, shared across worktrees because keys are content hashes. In the
browser it is an in-memory map, optionally filled from and flushed to
IndexedDB by the JS host before and after a run.

### Risks

- **Large modules.** A module of several thousand lines rechecks whole. If
  edit latency suffers, add per-item body entries keyed by the item's body
  hash plus the module's header hash. That needs no new architecture.
- **Folder interface fan-out.** A signature edit rechecks every module of
  the folder and every direct dependent. That is what the metric asks for,
  but a large folder makes it costly. A finer key per used declaration (as
  Kotlin's member-level snapshots do) is a later refinement.
- **Private inference inside a module.** A private function with an omitted
  result type depends on its body. This stays inside the module, so it does
  not affect the cache, but it orders work inside the module ([Q3](#q3-parallel-checking)).
- **Coherence across packages.** The rule against duplicate impls in the
  whole dependency graph needs every package's impl heads. The check reads
  only impl-head lists from interfaces and is keyed by the set of interface
  hashes, so it stays cheap.

**What would change it.** If the owner later adds a daemon or a language
server, an in-memory salsa-style layer can sit on top of the same per-module
units. If edit latency misses its target on realistic modules, move to
per-item entries as above.

## Q3: Parallel Checking

**Question.** How did other compilers get parallel checking, what do hd's
rules allow, and what does a 10k-line check in 0.2 CPU-s require?

### Prior Art

| System | How it is parallel | Speedup or cost | Lesson |
| --- | --- | --- | --- |
| rustc parallel front end | retrofitted query parallelism on a rayon fork; `RefCell` became locks, plus per-thread arenas and a deadlock handler for query cycles ([dev guide](https://rustc-dev-guide.rust-lang.org/parallel-rustc.html)) | up to 50% less time at 8 threads; 0 to 2% slower on one thread; up to 35% more memory; parsing and macro expansion stay serial ([Rust blog](https://blog.rust-lang.org/2023/11/09/parallel-rustc/)) | retrofitting took years; design it in from the start |
| tsgo | parallel parse; a fixed pool of independent checkers (4 by default), each checking a slice of files over a shared immutable AST | duplicated work on shared types costs memory; results can depend on the checker count, so the order of types had to be made stable ([TS 7 deep dive](https://diegobetto.com/en/typescript-7-deep-dive/)) | avoid duplicated work; keep output independent of scheduling |
| Go | `go build` compiles packages in parallel along the import DAG; each package type-checks serially; backend functions compile in parallel | scales with package count | the unit is the package; export data decouples packages |
| Sorbet | serial global phases (index, resolve), then per-method inference in parallel | about 100k lines/s per core; "parallelize the inference half almost trivially" ([Nelson Elhage](https://blog.nelhage.com/post/why-sorbet-is-fast/)) | explicit signatures make bodies independent |
| Zig | per-declaration analysis units; codegen and linking on other threads (unverified detail) | 37 ms incremental updates in watch mode | InternPool and indices everywhere |
| Carbon | data-oriented, flat arrays; parallel design planned | goal 1M lines/s semantic analysis | postorder storage, no pointers |

### What hd's Rules Allow

The spec was written for this. Bodies never decide a public signature, and
folders form a DAG. So:

1. **Discover and read** all files, hashing them in parallel.
2. **Lex and parse** every file in parallel. Files are independent.
3. **Folder interfaces in dependency order.** A folder's interface task runs
   when the interfaces of the folders it uses are done. This is a task graph,
   not strict waves, so a deep chain does not stall wide levels. Inside a
   folder, each module's headers are lowered in parallel, then names and
   types are resolved for the whole folder at once (its files may loop).
4. **Global checks over interfaces:** overlap per trait, orphan rules, and
   the cross-package duplicate-impl check. Each trait is an independent task.
5. **Every body in parallel:** functions, methods, impl members, top-level
   statements and `tests:` blocks. Each body reads frozen interfaces and
   writes only to its own arena.
6. **Private inference inside a module.** A private function without a
   result type must be checked before its callers can be. Run such
   functions in a module-level task, in call order, before that module's
   other bodies. A cycle among them is `recursive-function-needs-result-type`.
   Bodies with explicit signatures stay fully parallel.

### Data-Oriented Design

| Technique | Source | Use in hd |
| --- | --- | --- |
| 32-bit indices instead of pointers | Sorbet's `Ref`s, Zig's InternPool, Carbon | item, type, symbol and node IDs are `u32` newtypes |
| Interning | rustc, Zig | identifiers and types are interned; equality is integer comparison |
| Struct-of-arrays tokens and nodes | Carbon's postorder parse tree, Zig's AST | tokens as parallel arrays of kind and offset; CST nodes in one array per file |
| Arenas | oxc (about 20% from an arena AST), rustc's `WorkerLocal` arenas | one arena per file's CST, per folder interface, and per body; a body's arena is freed when it finishes |
| Frozen shared tables | (mine; Sorbet's split of serial global phases and parallel inference is the nearest) | interface types are interned before the body phase and are read-only during it, so body checking takes no locks |
| A sharded interner for new shared types | rustc's sharded interners | only types that outlive a body (instantiations needed by codegen) go to a sharded, lock-striped table |

**No IDs in output.** Interned IDs depend on scheduling, so nothing prints,
sorts or hashes by them. Diagnostics are sorted by file, offset and code
before printing. Hashes use stable paths. This is the `determinism` target
and TypeScript's `stableTypeOrdering` lesson.

**The budget.** The warm `resources` target is 10k lines in 0.2 CPU-s and
50 MB. The `cold-check` target is 1 s of wall time.

- **Warm, no edit:** stat about 50 to 100 files, compare with the manifest,
  and read cached diagnostics. Single-digit milliseconds, if startup is cheap.
- **Cold:** at Sorbet's 100k lines/s per core, 10k lines cost 0.1 CPU-s.
  The std is not rechecked: its interface is embedded.
- **Startup ≤ 20 ms and ≤ 10 MB:** a Rust binary starts in about 1 ms. Do not
  decode std eagerly: map its interface and decode items on first use.
- **Memory:** tokens for 10k lines are under 1 MB in SoA form. The CST and
  interfaces are a few MB. Body arenas are freed as bodies finish.

**Scheduler interface.** The driver submits tasks to a `Scheduler` with two
implementations: rayon's work-stealing pool, and a serial executor. The
serial one is the browser default and the `--threads 1` mode, and the
`parallel-speedup` metric needs the documented thread count anyway.

**Browser threads.** Wasm threads need `SharedArrayBuffer`, which needs the
page served with COOP `same-origin` and COEP `require-corp` or
`credentialless` ([wasm-bindgen-rayon](https://github.com/RReverser/wasm-bindgen-rayon)).
COEP also blocks cross-origin resources that do not opt in. Playground
programs are small, so the default browser build is single-threaded. A
threaded build can load when `crossOriginIsolated` is true. Another option
(mine): shard folders across several Web Workers that exchange interface
blobs. This needs no shared memory because interfaces are already
serialized, the way tsgo's `--builders` and Bazel spread work.

### Recommendation

The task graph above, on a `Scheduler` interface, with frozen interface tables
and per-body arenas. Confidence: high. It is Sorbet's and Go's design,
fitted to hd's folder rule, and built in from the start rather than
retrofitted as rustc's was.

### Risks

- **Load imbalance.** One huge body or one huge folder can bound the wall
  time. Work stealing helps with bodies, but not with folder interface
  chains. The `pathological` cases will show it.
- **Duplicated instantiation.** Two bodies may instantiate the same generic
  at once. A sharded memo with "first writer wins" makes it correct; the
  duplicated work is small.
- **Lock contention at more than about 4 threads**, as rustc reports. Frozen
  tables avoid most locks. Measure with `parallel-speedup`, which asks for
  0.6 × cores up to 8.

## Q4: Parser And CST

**Question.** Which tree, which recovery method, how do `hd fmt` and fix-its
sit on it, and how do indentation-sensitive languages lex layout with
recovery?

### Prior Art

| Design | What it is | Strength | Weakness for hd |
| --- | --- | --- | --- |
| rowan (rust-analyzer) | green tree (immutable, typed by a `u16` kind and a width) plus a red tree built on demand with parents and offsets ([docs](https://docs.rs/rowan)) | lossless, proven, any node may hold any children, so errors fit | per-node reference counts; the red layer costs allocation; `cstree` is the thread-safe variant |
| Roslyn red-green trees | the original of the design; immutable green nodes shared across edits | incremental reparse in an editor | that benefit needs an editor, and hd v1 has none |
| tree-sitter | generated incremental LR parser in C; indentation needs a hand-written external scanner | editor highlighting | generic recovery gives weak diagnostics; scanner state complicates recovery ([external scanners](https://tree-sitter.github.io/tree-sitter/creating-parsers/4-external-scanners.html)) |
| oxc | arena AST, not lossless | fastest parser in its class | no trivia, so no formatter or exact fix-its on it |
| Carbon parse tree | flat postorder array of nodes over a token array | data-oriented, cheap to build and walk | typed access needs a layer on top |

**Recovery.** matklad's resilient LL parsing: a hand-written recursive
descent parser that emits start, token and finish events. On an unexpected
token it either wraps junk in an error node or stops at a recovery set, the
tokens that can start or end the enclosing construct. A fuel counter stops
loops that make no progress
([tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html)).

**Layout.** Python's tokenizer keeps a stack of indentation levels and emits
`INDENT` and `DEDENT`. Haskell's layout rule has the condition
`parse-error(t)`, which makes the lexer depend on the parser and makes
recovery after a syntax error hard
([Haskell 2010 report](https://www.haskell.org/onlinereport/haskell2010/haskellch10.html),
[amelia.how](https://amelia.how/posts/parsing-layout.html)). F#'s offside
rule pushes contexts from the parser, with the same coupling.

hd's layout is Python's kind. The lexer emits `NEWLINE`, `INDENT`, `DEDENT`
and `SUITE_END` from logical lines, with a delimiter stack for suites nested
inside brackets ([indentation levels](../spec/lang/01-lexical-structure.md#indentation-levels),
[suites inside delimiters](../spec/lang/01-lexical-structure.md#suites-inside-delimiters)).
As far as I can tell from the rules, no layout decision needs the parser.
That is worth keeping, and worth confirming against the nested-suite rules
while writing the lexer.

### Recommendation

Confidence: high.

1. **A hand-written lexer** that produces a token array (kind, offset) plus
   trivia, and then a layout pass that inserts layout tokens. On
   `invalid-dedent` it reports once and dedents to the nearest open level,
   then goes on. Python stops at an `IndentationError`; hd must not.
2. **A hand-written resilient recursive-descent parser** with matklad's
   events. Recovery sets come from layout: a `NEWLINE` or `DEDENT` ends a
   statement, and a line at column 0 always starts a new item. So one
   broken item never swallows the next, which serves the `errors-per-run`
   target (10 independent mistakes, two of them syntax errors).
3. **A lossless flat green tree** per file: nodes in one array with kind,
   token range and subtree size, plus typed accessor views generated from a
   grammar list (rust-analyzer's approach to typed views). Whole-file reparse
   is cheap, so v1 needs no red tree and no incremental reparse. rowan or
   `cstree` is the fallback if the custom tree costs too much time to build.
4. **Binary operators by precedence climbing** with an explicit stack, so
   deep nesting cannot overflow the native stack (the Day 1 "iterative
   passes" rule).
5. **`hd fmt`** prints from the CST through a Wadler-style document IR, as
   Ruff's and Biome's formatters do. Comments are trivia, so none are lost.
6. **Fix-its** are text edits on token spans. A fix-it is offered only if
   the edited text re-parses without a new error, which checks
   `fixit-safety` at the source.

### Risks

- A custom tree is more work than rowan. The flat tree is simpler, but
  agents must not write per-node allocations into it. Keep the API small.
- Closures inside brackets have stricter end rules
  ([closures inside delimiters](../spec/lang/01-lexical-structure.md#closures-inside-delimiters)).
  If any of them turns out to need parser feedback, keep the feedback to one
  narrow, tested hook.

## Q5: Checker Structure

**Question.** How should the checker recover per item, report one diagnostic
per root cause, resolve traits at scale, check requirement rows, GADTs and
templates, and stop with a limit instead of hanging?

### Recovery And Root Causes

| System | Mechanism |
| --- | --- |
| rustc | an error type that carries proof an error was reported (`ErrorGuaranteed`); anything touching it is silent |
| TypeScript | `errorType`, assignable to and from everything, which silences follow-up errors |
| Roslyn | error type symbols that remember candidate symbols for better messages |

**Recommendation.** Confidence: high.

- Each item and each body is checked in isolation. An error inside one never
  stops another, and a broken signature makes only that item's uses poison.
- The poison type unifies with everything and never reports. Any expression
  that would report through a poison operand stays silent. Only the root
  error reports.
- Unknown names report once per name per body, and later uses are poison.
- Syntax error nodes check as poison, so the rest of the item is still
  checked.

### Trait Resolution And Coherence

Lessons from systems that got this wrong:

- Swift's GenericSignatureBuilder used exponential time and memory on
  protocol hierarchies with fan-out. The Requirement Machine replaced it
  with a decision procedure based on rewriting
  ([Swift forums](https://forums.swift.org/t/the-requirement-machine-a-new-generics-implementation-based-on-term-rewriting/55601)).
- Lean 4's tabled typeclass resolution removed exponential time on diamond
  hierarchies and guaranteed termination under bounded term size
  ([paper](https://arxiv.org/pdf/2001.04301)).
- Rust's next-generation solver took about four years from chalk onward.
  It became the nightly default in August 2026, and chalk is no longer
  maintained ([Rust blog](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/)).

hd's rules keep the problem small. Impls live in the module of the trait or
the target, overlap is decided from heads alone, bounds never prove
disjointness, and there is no specialization
([overlap](../spec/lang/09-traits.md#overlap)).

**Recommendation.** Confidence: medium-high.

- **A head index.** Index impls by (trait, outer type constructor of the
  target), as rustc's fast-reject does. A lookup unifies with a handful of
  candidates, not every impl.
- **Memoized goals.** Cache fully concrete goals in a shared table, and goals
  with inference variables per body. Memoization removes the diamond blowup,
  as tabling does in Lean.
- **A step budget and a depth limit**, each with its own diagnostic. The spec
  already has `trait-resolution-depth`. The prototype's F-626 shows what
  happens without a budget.
- **Coherence per trait** in the global phase: unify heads pairwise within
  each trait's index buckets.

### Requirement Rows And Suspension

Row checkers elsewhere: Koka uses row polymorphism with scoped labels
([Leijen 2014](https://arxiv.org/abs/1406.2061)). Effekt treats effects as
capabilities ([OOPSLA 2020](https://dl.acm.org/doi/10.1145/3428194)). Flix
uses Boolean unification and needed a dedicated paper to make it fast:
compilation throughput went from 8,580 to 15,917 lines/s
([OOPSLA 2023](https://dl.acm.org/doi/10.1145/3622816)).

hd's rows are simpler than all three. They are sets, a pattern has at most
one unknown row parameter, and entailment is membership
([entailment](../spec/lang/11-requirements-and-suspension.md#entailment)).
**Recommendation:** represent a row as a small sorted vector of interned keys
plus an optional row parameter. Union and membership are linear merges.
Inferred private rows form a least fixpoint over each call cycle in a
module; it is monotone and ends after at most one pass per key. Suspension
needs no inference, because `!` is in the name. Confidence: high.

### GADTs, Tuples And Templates

- **GADTs:** first-order nominal unification per arm, with arm-local
  equalities that never escape ([refinement](../spec/lang/13-gadts.md#refinement-algorithm)).
  Cost is linear in the pattern. Store equalities in the body's trail and
  pop them at the arm's end.
- **Exhaustiveness:** Maranget's usefulness algorithm, as rustc and OCaml
  use. It can blow up on wide nested patterns, so it needs a size limit with
  a diagnostic.
- **Variadic generics are gone** ([chapter 12](../spec/lang/12-variadic-generics.md));
  `Args < Tuple` is ordinary tuple unification. No pack machinery.
- **Derive templates:** each derivation instantiates the template once, as an
  ordinary impl in the target's module, checked there
  ([templates](../spec/lang/14-annotations.md#templates)). Its result is part
  of that module's cache entry. Because template bodies are in the trait's
  interface, a template edit changes that interface hash and rechecks every
  module that derives it.
- **Literal widths:** the union-find plan already recorded under
  [Literal Inference](NEW_COMPILER_ARCHITECTURE.md#literal-inference) fits
  this design as is: per-body arrays, a trail for speculation, and no copied
  checker state.

### Hard Limits

| Limit | Guards against | Diagnostic in the spec today |
| --- | --- | --- |
| trait resolution depth and steps | runaway impl search (F-626) | `trait-resolution-depth` |
| embedding depth | deep `data` embedding | `embedding-too-deep` |
| generic instantiation depth | polymorphic recursion during codegen | none |
| expression and block nesting | parser and checker stack overflow | none |
| exhaustiveness matrix size | exponential pattern checks | none |
| type size | types that grow on each instantiation | none |

Four limits have no code. That is an [open question](#open-questions-for-the-owner).

## Q6: Program Database Hook

**Question.** What must the checker keep queryable from day 1, so the program
database (a Later feature) falls out without a rewrite?

### Prior Art

| System | Lesson |
| --- | --- |
| Glean (Meta) | typed schemas per language plus derived cross-language predicates; incremental by stacking immutable databases; facts come from compilers; lookups around 1 ms ([Meta](https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/)) |
| SCIP (Sourcegraph) | replaced LSIF's numeric graph IDs with human-readable string symbols; one record per document, so a subset of documents can be re-indexed; 4x to 5x smaller than LSIF ([announcement](https://sourcegraph.com/blog/announcing-scip)) |
| Kythe (Google) | names every node by a "VName" tuple (corpus, root, path, signature, language) so facts from separate compilations join |
| CodeQL | an extractor run during the build writes a relational database; the schema follows the compiler's internals, which ties it to compiler versions |

### Recommendation

Confidence: medium.

- **Stable symbol strings from day 1**, SCIP and Kythe style: package, version,
  module path, item path, member. The cache already needs stable paths for
  hashing, so this costs nothing extra.
- **Per-module fact records** in the module's cache entry: declarations with
  spans and signature text, requirement rows, resolved references and call
  edges, impls and derives, and diagnostics. One record per module mirrors
  SCIP's per-document design, so the database updates per module.
- **No public schema yet**, as the owner decided. `hd callers` and
  `hd needs Http` can later read the union of fact records, first by scanning
  and then from SQLite once the schema settles.

## Q7: Crate Layout And Build Order

### Proposed Crates

Layered so that a lower crate never depends on a higher one, and so that the
front half builds for the browser.

| Crate | Holds | Depends on | Browser build |
| --- | --- | --- | --- |
| `hd_base` | IDs, interners, arenas, stable hashing, spans, the `SourceSet` and `CacheStore` interfaces | none | yes |
| `hd_diag` | the diagnostic model, compact and JSON renderers, the fix-it shape | `hd_base` | yes |
| `hd_syntax` | lexer, layout pass, parser, flat CST, typed views | `hd_base`, `hd_diag` | yes |
| `hd_fmt` | the formatter on the CST | `hd_syntax` | yes |
| `hd_project` | manifests, module discovery, the folder and package graphs | `hd_syntax` | yes |
| `hd_iface` | interface blobs: serialize, index, hash, lazy decode | `hd_base` | yes |
| `hd_types` | types, unification, the poison type, rows, the trait index | `hd_base`, `hd_iface` | yes |
| `hd_check` | name resolution, folder interfaces, bodies, coherence, exhaustiveness, templates | the above | yes |
| `hd_driver` | the `Scheduler`, the task graph, cache keys, the session API | the above | yes, serial |
| `hd_std` | a build step that pre-checks `lib/std` into an embedded interface blob | `hd_driver` | yes |
| `hd_cli` | the native `hd`; Part 2 adds the backend and wasmtime here or below | `hd_driver` | no |
| `hd_web` | wasm-bindgen glue, the in-memory cache, the JS host bridge | `hd_driver` | only there |

The back half (lowering, Wasm emission, the host interface) slots between
`hd_check` and the two front ends. That is Part 2's call.

### Build Order

The portable suite runs any `hd` that answers `parse`, `check` and `test`
with exit codes and `code:` diagnostics
([portable README](../test/portable/README.md)). By the 2026-10-06
baseline it selects 261 parse cases and 1,389 type cases
([baseline](../audit/compiler/baseline-2026-10-06.md#21-full-run)).

1. **Slice 1, syntax.** Lexer, layout, parser and CST, behind `hd parse FILE`.
   Exit: the parse-phase cases pass, every fixture and `lib/std` file
   round-trips byte for byte, and the front crates build for
   `wasm32-unknown-unknown` in CI from this slice on.
2. **Slice 2, std interfaces.** Name resolution and folder interfaces, run on
   `lib/std` (about 11.5k lines in 35 files). A rough grep found std names
   (`println`, `List`, `Option`, `?` and similar) in more than half of the
   typing fixtures, so **the std is the first real program**, not a later
   one. This slice already runs through `hd_driver` with the serial scheduler
   and computes cache keys, so incrementality is never retrofitted.
3. **Slice 3, bodies.** Body checking, chapter by chapter in spec order. Exit:
   type-phase cases pass, with a known-failures list as the sync, as the
   prototype keeps today.
4. **Slice 4, cache and threads on.** The disk cache, the stat manifest,
   rayon, and the embedded std blob. Exit: `recheck-precision`,
   `edit-latency`, `resources`, `startup`, `determinism` and
   `incremental-soundness` pass.
5. **Slice 5, browser front end.** `hd_web` checks a playground program in a
   worker. Exit: within the size budget the owner sets.

Runtime and CLI cases (903 and 51) need the back half and wait for Part 2.

## Open Questions For The Owner

1. **Implementation language.** Recommendation: Rust. Go is the fallback if
   Rust's build loop proves too slow for agents (Q1).
2. **Incremental model.** Recommendation: a per-module on-disk cache keyed by
   source and interface hashes, with no salsa and no persisted fine-grained
   graph (Q2). A daemon or language server later can add an in-memory layer
   on top.
3. **Browser threads.** Recommendation: a single-threaded browser build for
   v1, and no COOP/COEP requirement on the playground. A threaded build is an
   optional later step.
4. **Browser download budget.** Recommendation: at most 5 MB uncompressed for
   the compiler Wasm that checks, builds and tests, measured by a script like
   the other metrics. Today's worker bundle is 14.6 MB.
5. **Diagnostic codes for hard limits.** The spec has `trait-resolution-depth`
   and `embedding-too-deep` only. Recommendation: add codes for instantiation
   depth, nesting depth, exhaustiveness size and type size, so each limit has
   its own diagnostic as the Day 1 list asks.
6. **`modules_checked` in the JSON summary.** The metrics harness reads it,
   but the CLI spec does not define it
   ([metrics README](../test/metrics/README.md#pillar-1-metrics)).
   Recommendation: specify it.

## Part 2: The Back Half (pending)

Placeholder for the next researcher: lowering and the IR boundary between
tiers, Wasm GC emission and value layouts, monomorphization limits, the
suspension state machines, the host and embedding interface with its
wasmtime and JS implementations, and the CLI's binary size.

Findings from Part 1 for Part 2:

- Wasmtime 47 (2026-07-20) enables Wasm GC and exceptions by default. Its
  collector is a new semi-space copying collector without read or write
  barriers, tuned for many small instances
  ([Bytecode Alliance](https://bytecodealliance.org/articles/wasmtime-gc)).
- The `disk` metric caps the toolchain at 10 MB. A minimal wasmtime C API
  without a compiler is about 0.7 MB
  ([minimal embedding](https://docs.wasmtime.dev/examples-minimal.html)). The
  size with Cranelift included is unmeasured here and may conflict with that
  cap.

## Sources

- TypeScript native port, why Go: <https://github.com/microsoft/typescript-go/discussions/411>
- tsgo in the browser: <https://github.com/microsoft/typescript-go/discussions/458>
- TypeScript 7 parallelism: <https://diegobetto.com/en/typescript-7-deep-dive/>
- Roc, Rust to Zig: <https://rtfeldman.com/rust-to-zig>
- Bun, Zig to Rust: <https://simonwillison.net/2026/Jul/8/rewriting-bun-in-rust/>
- Bun port report: <https://www.devclass.com/software/2026/05/11/anthrophics-bun-team-trials-port-from-zig-to-rust/5237835>
- Zig agent challenge: <https://akitaonrails.com/en/2026/01/11/ai-agents-comparing-top-llms-on-the-zig-challenge/>
- Zig InternPool: <https://github.com/ziglang/zig/pull/15569>
- Zig incremental state serialization: <https://github.com/ziglang/zig/issues/6538>
- Zig incremental internals (mlugg): <https://mlugg.co.uk/posts/incremental-compilation-internals/>
- Sorbet's speed: <https://blog.nelhage.com/post/why-sorbet-is-fast/>
- Carbon parse tree: <https://github.com/carbon-language/carbon-lang/blob/trunk/toolchain/docs/parse.md>
- oxc benchmarks: <https://oxc.rs/docs/guide/benchmarks>
- oxc performance: <https://oxc.rs/docs/learn/performance>
- oxc Wasm build: <https://unpkg.com/browse/@oxc-parser/wasm@0.47.1/>
- oxc threadless WASI builds: <https://github.com/oxc-project/oxc/pull/26898>
- Biome Wasm package: <https://www.npmjs.com/package/@biomejs/wasm-web>
- Go and WebAssembly: <https://go.dev/wiki/WebAssembly>
- wasmtime-go: <https://pkg.go.dev/github.com/bytecodealliance/wasmtime-go>
- wasmtime-zig: <https://github.com/zigwasm/wasmtime-zig>
- wasm_of_ocaml release: <https://tarides.com/blog/2025-02-19-the-first-wasm-of-ocaml-release-is-out/>
- wasm-bindgen-rayon: <https://github.com/RReverser/wasm-bindgen-rayon>
- rustc incremental compilation: <https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html>
- Rust 1.52.1: <https://blog.rust-lang.org/2021/05/10/Rust-1.52.1/>
- An unstable-fingerprint ICE in 2026: <https://github.com/rust-lang/rust/issues/163760>
- Parallel rustc: <https://blog.rust-lang.org/2023/11/09/parallel-rustc/>
- Parallel rustc, dev guide: <https://rustc-dev-guide.rust-lang.org/parallel-rustc.html>
- salsa persistence request: <https://github.com/salsa-rs/salsa/issues/10>
- rust-analyzer, next few years: <https://ferrous-systems.com/blog/rust-analyzer-next-few-years/>
- Go build cache: <https://pkg.go.dev/cmd/go#hdr-Build_and_test_caching>
- Go compiler and export data: <https://go.dev/src/cmd/compile/README>
- gopls scalability: <https://go.dev/blog/gopls-scalability>
- dune 3 incremental builds: <https://tarides.com/blog/2022-07-12-faster-incremental-builds-with-dune-3/>
- dune hidden-include dependency bug: <https://github.com/ocaml/dune/issues/16484>
- Swift incremental imports issue: <https://github.com/swiftlang/swift/issues/92617>
- Kotlin compilation and caches: <https://kotlinlang.org/docs/gradle-compilation-and-caches.html>
- TypeScript incremental builds: <https://deepwiki.com/microsoft/TypeScript/8-incremental-and-project-builds>
- Buck2 DICE: <https://buck2.build/docs/insights_and_knowledge/modern_dice/>
- rowan: <https://docs.rs/rowan>
- Resilient LL parsing: <https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html>
- tree-sitter external scanners: <https://tree-sitter.github.io/tree-sitter/creating-parsers/4-external-scanners.html>
- Haskell 2010 layout: <https://www.haskell.org/onlinereport/haskell2010/haskellch10.html>
- Parsing layout: <https://amelia.how/posts/parsing-layout.html>
- Swift Requirement Machine: <https://forums.swift.org/t/the-requirement-machine-a-new-generics-implementation-based-on-term-rewriting/55601>
- Tabled typeclass resolution: <https://arxiv.org/pdf/2001.04301>
- Rust next-generation trait solver: <https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/>
- Koka row-polymorphic effects: <https://arxiv.org/abs/1406.2061>
- Effekt, effects as capabilities: <https://dl.acm.org/doi/10.1145/3428194>
- Flix Boolean unification: <https://dl.acm.org/doi/10.1145/3622816>
- Glean: <https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/>
- SCIP: <https://sourcegraph.com/blog/announcing-scip>
- Wasmtime GC: <https://bytecodealliance.org/articles/wasmtime-gc>
- Wasmtime minimal embedding: <https://docs.wasmtime.dev/examples-minimal.html>
