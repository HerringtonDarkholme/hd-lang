# New Compiler: Architecture Research

Status: Research, not decided. Both parts are done: Part 1, the front half,
and Part 2, the back half, 2026-10-06. Q4b, on pre-parsing, was added the
same day as a revisit of the front half.

Known issues of prior implementations: [prior-art-issues.md](prior-art-issues.md)

This document surveys prior art for the front half of the new compiler:
the implementation language, the incremental model, parallel checking, the
parser and CST, the checker's structure, the program-database hook, and a
crate layout with a build order. [Part 2](#part-2-the-back-half) covers the
back half: the IR and monomorphization, suspension, Wasm GC codegen, engines,
the host interface, runtime pieces, hot reload, the native backend, and the
back-half crates.

Fixed inputs, not reopened here:

- The goals, the Arena pillars, the goal metrics, and the feature triage in
  [goals.md](goals.md).
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
| 6b | **A header pass** ([Q4b](#q4b-pre-parsing-and-header-extraction)): a skim mode of the same lexer skips bodies exactly by indentation, strings and brackets, and yields each module's `use` list, exported skeleton and interface hash. It skips about 60% of std's tokens. The one body-derived interface fact was the transitive `block_on` ban; the owner made that ban direct-only (answer 13), so the interface comes from syntax alone and no drive summary is needed. | high (exactness), medium (payoff) |
| 7 | Checker: bidirectional inference local to each body, a poison type with root-cause suppression, trait lookup through a head index with memoized, step-bounded search, and rows as small sorted sets. | medium-high |
| 8 | Program database hook: stable string symbol IDs and per-module fact records written into the module cache entry from day 1; the queryable store comes later. | medium |
| 9 | Browser: the same core compiled to `wasm32-unknown-unknown`, single-threaded by default, behind four host interfaces (files, cache store, scheduler, capability host). | medium-high |
| 10 | Build order: syntax first against the parse fixtures, then **lib/std as the first real program**, then the typing fixtures by chapter, with the cache and threads wired in from the second slice. | medium |
| 11 | Back-half IR: a **structured, typed MIR** (Wasm wants structured control flow; hd source has no `goto`). Generic MIR is stored in the module cache entry. | medium-high |
| 12 | "Code per value layout" means **monomorphization per concrete type, then folding identical bodies**; dictionaries only for trait values and GADT evidence; polymorphic recursion stops at an instantiation depth limit. | medium-high |
| 13 | Instances live in a **content-addressed codegen cache** keyed by MIR hash and type arguments; a per-program link step patches relocations. | medium |
| 14 | Suspension: **state machines with lazily materialized frames** (C#-style; no allocation on the ready path), hook points as MIR instructions that normal builds drop, and a poll-and-wake entry driver. | high (state machines), medium (lazy frames) |
| 15 | Wasm GC values: exact struct types, `Option` as a nullable reference or a scalar pair, `Result` returned as values, enums as tagged subtype hierarchies, per-layout lists, UTF-8 strings as `(array i8)`, panics as traps with a site table, no Wasm exceptions. | medium-high |
| 16 | **No Binaryen.** Emit binary with `wasm-encoder`; the optimizing tier is our own MIR passes; `wasm-opt` stays a Later option. | high |
| 17 | **wasmtime with Cranelift** for both tiers (Winch lacks GC); precompiled-module and per-function caches through `CacheStore`; epochs for time limits, a `ResourceLimiter` for the heap. | medium |
| 18 | Host interface: a **hand-rolled core-Wasm ABI** shaped after WASI 0.3, structured values through one exchange buffer, start-and-poll async on both engines, JS glue generated from one description. The Component Model waits for GC support. | medium-high |
| 19 | Native backend after v1: the MIR, layouts, state machines and host ABI carry over; Cranelift user stack maps; a first collector through MMTk. | low-medium |
| 20 | Back-half build order: slices 6 to 10, ending with the portable runtime cases passing on wasmtime and in a headless browser. | medium |
| 21 | Versus Vx: keep 32-bit IDs in memory and 128-bit stable hashes at boundaries; deduplicate instances by hash; a checked, hand-written zero-copy format. | medium |

## Running In The Browser

The owner's requirement (2026-10-06): the compiler runs in a browser tab, and
the user's program runs on V8 there. Today's prototype does this from a
14.6 MB worker bundle, mostly Binaryen's JavaScript build
([playground README](../../website/playground/README.md#how-it-works)).

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
  ([functions](../../spec/lang/07-functions.md#parameter-and-result-types),
  [rows](../../spec/lang/11-requirements-and-suspension.md#omitted-requirement-clauses)).
  A body edit cannot change a module's interface. (Corrected in
  [Q4b](#contradictions-with-part-1-and-part-2): the transitive `block_on`
  ban makes some body edits visible to dependents, and template bodies
  are interface.)
- **An acyclic folder graph.** Files of one folder may use each other in
  loops, but folders form a DAG, and the spec says why: "a folder then
  compiles from the signatures of the folders it uses, as a Go package
  compiles from its imports' export data"
  ([folder graph](../../spec/lang/10-modules.md#folder-graph)).
- **No wildcard imports.** A module's uses name exactly which declarations
  it reads, so dependency edges are known from syntax.
- **Inference stays inside a module.** Only private functions may omit a
  result type or row, and the inferred type never crosses the module.
- **Template bodies are interface.** A package interface carries each
  derivation template's body ([limits](../../spec/lang/14-annotations.md#limits)).
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
   module, which is the `recheck-precision` target. [Q4b](#q4b-pre-parsing-and-header-extraction)
   refines this: the hash covers exported items, impl heads and template
   bodies, not every token outside bodies, and a separate drive-summary
   hash covers the one body-derived fact.
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
   writes only to its own arena. Exception found in
   [Q4b](#q4b-pre-parsing-and-header-extraction): `defer` suites,
   defaults, facts and module initialization also read the drive
   summaries of the folders they call into, so that check runs late.
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
inside brackets ([indentation levels](../../spec/lang/01-lexical-structure.md#indentation-levels),
[suites inside delimiters](../../spec/lang/01-lexical-structure.md#suites-inside-delimiters)).
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
7. **A header pass** shares this lexer and the parser's item code, and
   skips bodies. [Q4b](#q4b-pre-parsing-and-header-extraction) designs
   it. Edited files still get the full lossless tree.

### Risks

- A custom tree is more work than rowan. The flat tree is simpler, but
  agents must not write per-node allocations into it. Keep the API small.
- Closures inside brackets have stricter end rules
  ([closures inside delimiters](../../spec/lang/01-lexical-structure.md#closures-inside-delimiters)).
  If any of them turns out to need parser feedback, keep the feedback to one
  narrow, tested hook.

## Q4b: Pre-Parsing And Header Extraction

> **Update after the owner's answers (2026-10-06).** The `block_on` ban
> in `defer`, defaults, facts and module initialization is now
> direct-only, with a run-time panic for an indirect call (answer 13).
> Everything below about the **drive summary** and the late check of
> those contexts no longer applies: a module's interface comes from
> syntax alone.

**Question.** Can the compiler run a cheap pass, like V8's preparser, that
extracts a module's `use` list and its exported skeleton (types,
signatures, fields, traits, impl heads) without parsing function bodies?
What does hd's spec allow, what does it save, and how does it feed the
scheduler, the cache and the playground? Added 2026-10-06 as a revisit of
the front half.

### Prior Art

| System | Skips | Keeps | Cost or saving | What went wrong |
| --- | --- | --- | --- | --- |
| V8 preparser and lazy parsing | inner function bodies, until first call | syntax validity, and per-function variable allocation data: "a dense array of flags per variable" ([V8](https://v8.dev/blog/preparser)) | the preparser is about 2x faster than the full parser (secondary source; V8 gives no figure) ([Over Explained](https://dev.to/scmmishra/over-explained-javascript-and-v8-2cei)) | before v6.3 a function was preparsed once per nesting level; heuristics (PIFE: `(function(){…})`) misfire, and eager compilation of everything "comes at a significant memory cost" ([V8](https://v8.dev/blog/preparser)) |
| Go `go/build`, `go list` | everything after the imports | the package clause and imports: `readGoInfo` "reads the file up to and including the import section" ([read.go](https://go.dev/src/go/build/read.go)) | the package graph costs a few hundred bytes per file | one feature reads the rest anyway: a file that imports `embed` is read fully to find `//go:embed` lines (same source) |
| Go export data | bodies of other packages | per-package export data with a lazy index ([compiler README](https://go.dev/src/cmd/compile/README)) | a package compiles from its imports' export data only | bodiless declarations exist (assembly); the `-complete` flag says there are none, so a missing body is an error (from memory, unverified) |
| TypeScript `preProcessFile` | everything but imports and references | import specifiers, from the scanner alone ([wiki](https://github.com/microsoft/typescript/wiki/using-the-language-service-api)) | no parse | a scanner without full lexer state misread template strings and comments ([#30878](https://github.com/Microsoft/TypeScript/issues/30878), [#47597](https://github.com/microsoft/TypeScript/issues/47597)) |
| TypeScript `isolatedDeclarations` (5.5) | the type checker | `.d.ts` emitted per file, because exports must carry explicit types ([TS 5.5](https://devblogs.microsoft.com/typescript/announcing-typescript-5-5/#isolated-declarations)) | oxc's emitter: "40x faster than TSC on typical files, 20x faster on larger files" ([oxc](https://oxc.rs/blog/2024-09-29-transformer-alpha.html)) | users must annotate exports; I found no source on tsgo using it |
| Java Turbine and ijar | method bodies | signatures, constants and annotations in header jars ([Bazel](https://bazel.build/docs/bazel-and-java)) | Chromium reports 10 to 30% faster incremental Java builds ([commit](https://github.com/chromium/chromium/commit/578730be19ac76113cd9da3ff2c2566c2f16fc32)) | header jar generation can cost more than it saves: one toolchain found it "much slower than compilation, resulting in a net penalty" ([salesforce](https://github.com/salesforce/bazel-jdt-java-toolchain)); constant initializers must still be evaluated, because `javac` inlines them into dependents (from the JLS, not checked here) |
| Rust pipelined compilation | codegen, until `.rmeta` is written | metadata, including MIR of generic and inline functions | "10-20% compilation speed increases for optimized, clean builds of some crate graphs" ([Rust 1.38](https://blog.rust-lang.org/2019/09/26/Rust-1.38.0/)) | rustc cannot skip bodies: macros create items, `impl Trait` leaks auto traits from the body, and consts run `const fn` bodies (known design, not cited here) |
| Swift | `-experimental-skip-non-inlinable-function-bodies` skips type checking and SILGen of bodies not serialized | signatures and `@inlinable` bodies | SwiftLint's module interface: 13.2 s to 1.7 s (7.7x); the stdlib only 1.09x to 1.43x, because it is full of `@inlinable` code ([PR #20420](https://github.com/swiftlang/swift/pull/20420)) | the per-file interface hash covers every token outside bodies, so adding a private top-level function recompiles every user of the module ([#92617](https://github.com/swiftlang/swift/issues/92617)) |
| Zig | semantic analysis of unreferenced declarations | AstGen turns every file into untyped ZIR, with its own errors ([AstGen](https://mitchellh.com/zig/astgen)) | only what `main` reaches is analyzed ([Sema](https://mitchellh.com/zig/sema)) | errors in unreferenced code are never reported |
| Kotlin `jvm-abi-gen` | method bodies | public signatures, and inline function bodies (unverified detail) ([rules_kotlin](https://github.com/bazel-contrib/rules_kotlin/blob/master/CompileAvoidance.md)) | downstream recompiles avoided on non-ABI edits | known bugs "affect less than 1% of targets" (same source) |
| Dart outlines | method bodies and comments | the API, and "enough information to evaluate constant expressions" ([dart-lang #1483](https://github.com/dart-lang/language/issues/1483)) | body and comment edits do not invalidate dependents | compile-time user code would need full transitive sources and lose that benefit (same issue) |
| Flow types-first | dependency bodies | signatures, which must be fully annotated at module boundaries ([Flow](https://flow.org/blog/2020/05/18/Types-First-A-Scalable-New-Architecture-for-Flow/)) | rechecks "multiple times faster" (the Medium post with figures returned 403) | users had to annotate exports |

Lessons:

- **Every system that skips bodies needs explicit signatures at the
  boundary.** TypeScript and Flow had to add the rule. hd already has it
  ([`module.package.annotated`](../../spec/lang/10-modules.md#r-module.package.annotated)).
- **What breaks skipping is code that runs at compile time.** Java's
  constants, Dart's const expressions, Rust's `const fn` and macros, and
  Swift's and Kotlin's inline bodies are the exceptions in every system.
- **A skimmer must share the real lexer.** TypeScript's import scanner
  had bugs exactly where its lexer state was simpler than the parser's.
- **Hash only what dependents can see.** Swift hashes every token outside
  bodies, private ones included, and pays for it.

### What hd's Interface Needs Beyond Syntax

I went through the spec for every place an exported skeleton might need
more than the header text. "Token range" means the header pass keeps the
tokens unparsed or parsed only as an expression, and checking happens in
the declaring module.

| Construct | Needed by dependents | Header pass handles it | Spec change |
| --- | --- | --- | --- |
| `use` and `pub use` | yes: edges and re-exports | yes; uses are top-level items only ([`grammar.suite.use-top-level`](../../spec/lang/02-grammar.md#r-grammar.suite.use-top-level)); chains are resolved per folder | none |
| uses in `tests:` blocks and doc tests | only by `hd test`; they make no folder edge ([`module.cycle.test-code`](../../spec/lang/10-modules.md#r-module.cycle.test-code)) | yes, marked test-only | none |
| parameter, field and shared-parameter defaults | the presence of a default; the expression runs per call ([`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval)) | token range; compile it as a callee-side default thunk (mine), so it is never inlined into a caller | none |
| enum shared constructor data, `NotFound -> StatusCode(404)` | its type only; the value is evaluated once, on first read ([`data.shared.eval-as-fact`](../../spec/lang/08-data-and-enums.md#r-data.shared.eval-as-fact)) | token range | none |
| facts, decorators, member lines | the type for checking; the value for builds ([`module.interface.fact-values`](../../spec/lang/10-modules.md#r-module.interface.fact-expressions)) | token range in the check interface; values evaluated at build time from MIR | none, but see contradiction 2 below |
| `@derive(X)` | the generated impl head and its bounds | yes: bounds come from member types and omitted members ([`annot.bound.params`](../../spec/lang/14-annotations.md#r-annot.bound.params)), after name resolution | none |
| `@error`, `@from`, `@source` | the `Display`, `Error` and `From[P]` heads and their bounds | yes: bounds depend on which members a message interpolates ([`annot.error.bound.display`](../../spec/lang/14-annotations.md#r-annot.error.bound.display)), which the lexer sees in the message string | none |
| derivation templates, `impl[T] X for T by Structure` | the body: a dependent checks the instantiated template at its opt-in ([`annot.limit.interfaces`](../../spec/lang/14-annotations.md#r-annot.limit.interfaces)) | yes: the header names it, and the template must sit in the trait's module ([`annot.template.module`](../../spec/lang/14-annotations.md#r-annot.template.module)); keep the body as an interface body | none |
| walker, describer and source bodies a template names | only for codegen | no, but builds read MIR anyway | none |
| trait default methods | that a default exists | yes; the body is checked once in the trait ([`trait.default.checked-once`](../../spec/lang/09-traits.md#r-trait.default.checked-once)) | none |
| impl heads, intrinsic methods, delegation `by E` | yes, for coherence and lookup | yes | none |
| local declarations and impls inside bodies | no: a local impl must involve a local type or trait ([`names.local-impl.involve`](../../spec/lang/03-names-and-scopes.md#r-names.local-impl.involve)), which no other module can name | skipped with the body | none, but [`module.interface.contents`](../../spec/lang/10-modules.md#r-module.interface.contents) lists local impl heads "needed for coherence"; I believe none are |
| top-level statements | no: their bindings cannot be used ([`names.exec.not-usable`](../../spec/lang/03-names-and-scopes.md#r-names.exec.not-usable)) | skipped like bodies | none |
| private functions without a result type | no: inference stays in the module | the header pass records "result omitted", so the scheduler orders them (Q3 step 6) | none |
| doc comments | `doc` values of members and variants ([`lex.doc.field`](../../spec/lang/01-lexical-structure.md#r-lex.doc.field)), read by the module's own derivations; `hd doc` | kept as trivia text; outside the interface hash | none |
| the transitive `block_on` and `println` ban | **yes, and it is body-derived** | **no** | see below |

**The one real exception.** `block_on`, and `println` which drives a
call with it, are forbidden in default expressions, `defer` suites,
non-entry module initialization, and fact and metadata expressions. The
ban is "transitive through the statically known call graph"
([`req.drive.block-on.transitive`](../../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.indirect),
[`module.console.println-block-on.contexts`](../../spec/lang/10-modules.md#r-module.console.println-block-on.direct)).
So a `defer` suite that calls `log.flush()` from another package is legal
only if `flush`'s body, and every body it calls, never reaches `block_on`.
That is a property of bodies in other modules. Adding a `println` to a
helper's body can break a dependent that never changed. The prototype
computes it as a whole-program fixpoint over called names
(`src/checker/program-effects.ts`), which a per-module compiler cannot do.

**Design without a spec change (mine).** After a folder's bodies are
checked, each function gets a three-valued **drive summary**: never
drives, drives, or unprovable (a call through a function value or a
dynamic trait method). Summaries combine bottom-up over the call graph,
with a fixpoint inside a folder. A folder exports them beside its
interface, under a separate **summary hash**. A dependent checks its
`defer`, default, fact and initialization contexts in a late per-module
task that waits for the summaries of the folders it calls into. Other
bodies do not wait. The summary changes only when a bit flips, so early
cutoff survives most body edits.

**The spec change I do not recommend, listed for the owner.** Ban only
direct `block_on` and `println` calls in those contexts, and panic at run
time when a non-entry initializer or a default reaches `block_on`
without a driver. That makes the interface purely syntactic again, but
moves an error from compile time to run time. The summary costs little,
so I would keep the spec as written.

One question the spec leaves open: whether a call through a generic bound
is "statically known". A generic helper checked once cannot know which
implementation runs. I ask it below.

### Skipping Bodies By Indentation

A body is everything after a header's suite colon until the first
logical line whose indentation is at most the header's. The header pass
skips it with the lexer in a **skim mode** (mine): it still walks every
byte, but builds no tokens and no tree. The rule alone is not enough:

| Breaker | Example | What the skimmer tracks |
| --- | --- | --- |
| multiline strings | `std/text.hd` holds a `"""` Unicode table whose 115 lines start at column 0 inside a body | string mode, including `"""` and prefixed raw strings, where `\"` does not end the string |
| interpolation | `"${f("a)")}"` nests code and strings | a stack of code and string frames |
| bracket continuation | a closure body or `)` left of its statement ([`lex.nested.body-depth`](../../spec/lang/01-lexical-structure.md#r-lex.nested.body-depth)); three conformance fixtures do this | bracket depth; any line inside brackets belongs to the body |
| comments and blank lines | a `#` line at column 0 inside a body does not end it ([`lex.indent.blank`](../../spec/lang/01-lexical-structure.md#r-lex.indent.blank)) | comment-only and blank lines are ignored |
| char literals | `'"'` and `'#'` | the `'` literal form |
| same-line suites | `fn f() -> i32: +1` | the first suite colon at bracket depth zero; the rest of the logical line is the body |
| leading-dot and `\|>` lines | always deeper than their statement, so never a body end | nothing extra |
| tabs, bare CR, BOM | lexical errors | the shared lexer reports them; the skimmer only stops on them |

The state is a frame stack (code with a bracket depth, or a string with
its delimiter and raw flag) plus the current line's indentation. The same
skim applies to methods inside `trait` and `impl` bodies, because a
method header is a `fn` line ending in `:` at member indentation.

**Exactness.** My throwaway script implements this state machine (about
230 lines of TypeScript, not committed). On every file that the
prototype parses without a diagnostic, 2,888 of them, the skimmer found
the same end line for every function and method body as the prototype's
parser. A skimmer that tracked indentation and comments but not strings
and brackets would have ended bodies early in 4 files: one std file and
three fixtures.

**Design rule.** The skim mode is a mode of the one hand-written lexer
of Q4, not a second scanner. Its frame stack and string rules are the
lexer's own, so the two cannot disagree. A differential test runs both
on every fixture and std file and compares body ranges.

### Measured Opportunity

Lines, bytes and approximate tokens by class, from the script. Bodies
include comments inside them. "Interface bodies" are bodies of
`impl ... by Structure` blocks, kept by the header pass.

| Corpus | Files | Lines | Fn and method bodies | Interface bodies | `tests:` blocks | Top-level statements | Skippable, lines / bytes / tokens |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `lib/std` | 36 | 11,549 | 43.1% | 0.6% | 0% | 0% | 43% / 42% / 62% |
| examples and playground | 25 | 1,128 | 40.1% | 0% | 13.4% | 0.8% | 54% / 67% / 73% |
| `test/` (metrics, std, fixtures, perf) | 111 | 1,997 | 15.5% | 0.2% | 27.3% | 0.4% | 43% / 64% / 73% |
| `spec/conformance` fixtures | 2,940 | 48,190 | 17.2% | 0.3% | 17.1% | 1.0% | 35% / 48% / 56% |

The rest of std is declaration lines (22%), doc comments (9%), plain
comments (14%) and blank lines (12%). Conformance fixtures are small and
declaration-heavy by design, so they understate real code.

How approximate: lines are classed by their first token; a same-line
body counts its bytes after the colon; tokens are counted with a regular
expression; every body under `by Structure` counts as an interface body,
derivation blocks included. Body ends are exact, as checked above.

**Reading the numbers.** On real code, a header pass handles about 40%
of the tokens a full parse would, or about a quarter for a test-heavy
user module. The skim still reads every byte, so the saving is in
tokens, nodes and memory, not in reading.

### Design

**The pass.** One function, `header_pass(bytes) -> Skeleton`, pure and
deterministic. The skeleton holds:

1. The `use` list with spans, each marked normal, `pub`, or test-only.
2. Every declaration header: name, visibility, generics with bounds and
   defaults, parameters, result, row, `!`, fields, variants with payloads
   and `->` results, trait members, impl heads with `by` clauses,
   associated types, aliases and newtypes.
3. Token ranges for default expressions, fact and decorator expressions,
   member lines and shared-data expressions, parsed as expressions but not
   resolved.
4. Body ranges: start and end byte offsets per body, marked ordinary,
   interface (templates), test, or top-level statement.
5. The **interface hash** (below) and the module's own source hash.

The skeleton parser is the full parser's item-level code with one change:
where the grammar expects `suite_body`, it emits a single skipped-body
node over the skimmed range. So the header grammar has one
implementation, matklad-style events serve both, and a skeleton derived
from a full CST equals one from the header pass. Slice 1 tests that.

**The interface hash (corrects Q2).** Hash the skeleton's exported part,
in token kinds and text: `pub` items with their members, every impl head,
derived impl heads, `pub use` lines, template bodies, and the token
ranges of defaults and facts. Leave out private items, ordinary bodies,
comments and plain uses. A private helper added at the top of a file then
leaves dependents alone, which is the Swift problem above. Coherence and
method lookup need impl heads of private types too, so all impl heads go
in.

**When a file is skimmed and when it is fully parsed.**

| Situation | What runs |
| --- | --- |
| warm run, file unchanged, cache hit | nothing: the stat manifest and the cached entry answer (Q2) |
| file changed, or its module entry misses | full parse; the skeleton comes from the CST, so the file is never read twice |
| cold run, file not yet needed | skim first, so its folder interface can start; full parse later as its own task |
| codegen needs a reachable item | its module's cached MIR; if the module missed, the module is checked first |
| `hd test` | test bodies and doc tests are parsed only then |
| `hd fmt`, `hd fix`, fix-its | always the lossless CST of Q4 |

V8's cost of parsing twice appears only in the cold row, and only for
files whose bodies are then checked. The skim is the cheap part, so this
costs a little on a cold run in exchange for an earlier start of every
folder interface.

**How it feeds the rest.**

- **Scheduler (Q3).** Discovery skims every file in parallel. The folder
  graph, `folder-cycle` and `package-cycle` come from the use lists before
  any full parse. A folder interface task needs only the skeletons of its
  files and the interfaces of the folders it uses. Body parsing and body
  checking become per-module tasks after that. This is Rust's pipelining
  and Turbine's header jars inside one process.
- **Early cutoff (Q2).** After an edit, the driver compares the new
  interface hash with the cached one before any body is checked.
  Dependents' cache lookups can start at once.
- **Module cache key (Q2).** It gains the summary hashes of the folders
  the module calls into (the drive summary above). Nothing else changes.
- **Affected tests.** Given the changed files, the use graph from
  skeletons says which test modules can be affected, without parsing
  anything else. This serves "test results cached by content" in the
  Arena features.
- **Program database (Q6).** Skeletons give every declaration with its
  span and signature text for every module cheaply, enough for an outline,
  `hd doc` navigation, or "which modules declare `X`". References and
  call edges still need checked bodies.
- **Browser.** Playground programs are small and std is precomputed, so
  the gain there is small. A multi-file playground gets an instant outline
  and use errors. I would not count on it for first feedback.

**What "check" means for a dependency whose bodies are never parsed.**
Part 1 and the Day 1 list say dependency bodies are skipped. That holds
on a warm machine: a dependency package's bodies are checked once, their
diagnostics are not shown, and their interface and drive summaries are
cached for every later run and worktree. On a cold machine they must be
checked once, at least far enough to resolve calls for the drive
summary, which needs types. std ships its summaries in the embedded blob.

**Error recovery.** The header pass reports nothing. If it hits a broken
header, it records the item as broken, which the checker treats as poison
(Q5), and resynchronizes at the next column-0 line, as Q4's parser does.
A broken file is always fully parsed when its module is checked, and the
full parser reports the error once. The skimmer's recovery for an
unterminated single-line string, which ends at the line end, is the
lexer's own, so body ranges still agree.

**Determinism.** The pass is a function of the file's bytes. Skeletons
use stable paths and source order, and the hash never sees interned IDs.

### Contradictions With Part 1 And Part 2

1. **Q2, "a body edit cannot change a module's interface", and step 3,
   "an unchanged header hash means an unchanged folder interface".** Not
   quite. Template bodies are function bodies and are interface, so the
   hash must include them. And the transitive `block_on` ban makes a body
   edit able to break a dependent. The fix is the drive summary with its
   own hash, and an interface hash over exported items only.
2. **Facts in the interface.** The spec's package interface records fact
   *values* ([`module.interface.fact-values`](../../spec/lang/10-modules.md#r-module.interface.fact-expressions)),
   and a value can depend on a body in another file. Checking needs only a
   fact's type, so the check interface keeps the expression. Values are
   computed at build time from MIR and reach the codegen cache through
   MIR hashes (Q8). The spec's package interface is a later distribution
   format, so this is not a spec conflict.
3. **Q3, step 5, "each body reads frozen interfaces".** Bodies with
   `defer`, defaults, facts or module initialization also read drive
   summaries of the folders they call into. Only that late check waits.
4. **Day 1, "dependency bodies skipped".** True warm, not cold, as above.
5. **Outside this question:** [`module.interface.dictionaries`](../../spec/lang/10-modules.md#r-module.interface.generic-compilation)
   says each generic function compiles once in its defining package with
   dictionaries, which Q8's monomorphization contradicts. Open question 8
   should cover that rule when the owner answers it.

### Recommendation

**Build the header pass as a skim mode of the Q4 lexer plus the full
parser's item code with skipped bodies. Use it for discovery, the folder
graph, folder interfaces and the interface hash; derive skeletons from
the CST for files being fully parsed anyway.** Confidence: high that it
is exact and cheap (measured agreement on 2,888 files); medium on its
payoff, because the per-module cache already removes most repeated
parsing.

Keep the spec as written. Add the drive summary for the `block_on` ban,
which is the one interface fact that needs bodies.

### Risks

- **The gain is mainly on cold runs.** Warm runs already parse only
  changed files. Measure the cold `cold-check` target with and without
  the skim before tuning it.
- **Two code paths for headers.** Sharing the item parser and the
  differential test keep them equal; without them, they will drift.
- **The drive summary is a cross-module, body-derived fact.** It needs
  its own soundness tests in `incremental-soundness`: edit scripts that
  add and remove `println` in helpers used from `defer` suites.
- **Generic calls in the ban.** If calls through bounds count as
  statically known, the ban can only be checked per instantiation, which
  needs bodies at codegen and moves errors to build time.

**What would change it.** If a cold check of a large package spends
little time before folder interfaces, drop the skim and full-parse
everything in parallel (Q3 as written), deriving skeletons from CSTs. If
the owner adopts the direct-only ban, drop the drive summary.

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
([overlap](../../spec/lang/09-traits.md#overlap)).

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
([entailment](../../spec/lang/11-requirements-and-suspension.md#entailment)).
**Recommendation:** represent a row as a small sorted vector of interned keys
plus an optional row parameter. Union and membership are linear merges.
Inferred private rows form a least fixpoint over each call cycle in a
module; it is monotone and ends after at most one pass per key. Suspension
needs no inference, because `!` is in the name. Confidence: high.

### GADTs, Tuples And Templates

- **GADTs:** removed from the language (owner, 2026-10-07), so no arm-local
  equalities.
- **Exhaustiveness:** Maranget's usefulness algorithm, as rustc and OCaml
  use. It can blow up on wide nested patterns, so it needs a size limit with
  a diagnostic.
- **Variadic generics are gone** ([chapter 12](../../spec/lang/12-variadic-generics.md));
  `Args < Tuple` is ordinary tuple unification. No pack machinery.
- **Derive templates:** each derivation instantiates the template once, as an
  ordinary impl in the target's module, checked there
  ([templates](../../spec/lang/14-annotations.md#templates)). Its result is part
  of that module's cache entry. Because template bodies are in the trait's
  interface, a template edit changes that interface hash and rechecks every
  module that derives it.
- **Literal widths:** the union-find plan already recorded under
  [Literal Inference](goals.md#literal-inference) fits
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
| `hd_syntax` | lexer (with its skim mode), layout pass, parser, flat CST, typed views, skeletons and the header pass | `hd_base`, `hd_diag` | yes |
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
([portable README](../../test/portable/README.md)). By the 2026-10-06
baseline it selects 261 parse cases and 1,389 type cases
([baseline](../../audit/compiler/baseline-2026-10-06.md#21-full-run)).

1. **Slice 1, syntax.** Lexer, layout, parser and CST, behind `hd parse FILE`.
   Exit: the parse-phase cases pass, every fixture and `lib/std` file
   round-trips byte for byte, and the front crates build for
   `wasm32-unknown-unknown` in CI from this slice on. The header pass
   lands here too ([Q4b](#q4b-pre-parsing-and-header-extraction)), with
   a differential test: on every fixture and std file, its body ranges
   and skeleton equal those derived from the full CST.
2. **Slice 2, std interfaces.** Name resolution and folder interfaces, run on
   `lib/std` (about 11.5k lines in 35 files), built from skeletons rather
   than full trees, with the folder graph from use lists. A rough grep found std names
   (`println`, `List`, `Option`, `?` and similar) in more than half of the
   typing fixtures, so **the std is the first real program**, not a later
   one. This slice already runs through `hd_driver` with the serial scheduler
   and computes cache keys, so incrementality is never retrofitted.
3. **Slice 3, bodies.** Body checking, chapter by chapter in spec order. Exit:
   type-phase cases pass, with a known-failures list as the sync, as the
   prototype keeps today. The drive summary and the late check of `defer`,
   default, fact and initialization contexts land here.
4. **Slice 4, cache and threads on.** The disk cache, the stat manifest,
   rayon, and the embedded std blob, with the interface hash and the
   drive-summary hash in cache keys. Exit: `recheck-precision`,
   `edit-latency`, `resources`, `startup`, `determinism` and
   `incremental-soundness` pass.
5. **Slice 5, browser front end.** `hd_web` checks a playground program in a
   worker. Exit: within the size budget the owner sets.

Runtime and CLI cases (903 and 51) need the back half and wait for Part 2.

## Open Questions For The Owner

**Answered (owner, 2026-10-06).** The answers below settle questions 1–14;
the question texts stay for their reasoning.

| # | Answer |
|---|---|
| 1 | Rust. |
| 2 | A per-module on-disk cache; no salsa, no daemon. |
| 3 | Single-threaded in the browser for v1. |
| 4 | No download budget yet; set it after the first browser slice is measured. |
| 5 | Add diagnostic codes for the hard limits (orchestrator's call). |
| 6 | Specify `modules_checked` (orchestrator's call). |
| 7 | Reword `req.host-wait.leaf`: the host ABI is an implementation detail; v1 uses hd's own core-Wasm imports. |
| 8 | Code per concrete type, then merge byte-identical functions; the same in debug and release. |
| 9 | In the browser: JSPI where present; else a synchronous same-origin XHR for HTTP; else a `host-contract` panic. |
| 10 | The `disk` metric counts per-worktree artifacts; the `hd` binary gets its own budget once measured (orchestrator's call). |
| 11 | Add `heap-exhausted` and `time-limit` panic categories (orchestrator's call). |
| 12 | Require Wasm GC only (orchestrator's call). |
| 13 | The ban becomes direct-only: a direct `block_on` or `println` in `defer`, defaults, facts or module initialization is rejected; an indirect one panics at run time, as Rust's tokio does. Interfaces come from syntax alone. |
| 14 | Moot after 13. |

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
   ([metrics README](../../test/metrics/README.md#pillar-1-metrics)).
   Recommendation: specify it.
7. **`host_wait!` and the Component Model.** The spec rule
   [`req.host-wait.leaf`](../../spec/lang/11-requirements-and-suspension.md#r-req.host-wait.abi)
   names the Component Model async ABI, and HOST_CAPABILITIES' boundary
   table names the Component Model as the official runtime. Neither works
   for a Wasm GC program until the canonical ABI gets a GC option (Q12).
   Recommendation: reword the rule so the wait operation's ABI is an
   implementation detail, use hd's own core imports in v1, and keep the
   Component Model mapping for a later `--target wasi`.
8. **What "code per value layout" means.** Recommendation: monomorphize per
   concrete type and fold identical bodies, in debug and release alike, and
   make polymorphic recursion an error at an instantiation depth limit
   (Q8). The alternative is Go-style sharing per layout with dictionaries,
   whose indirect calls wasmtime does not inline.
9. **`block_on` waiting on the host in a browser.** It needs JSPI, which
   only Chrome ships today, or cross-origin isolation. Recommendation: use
   JSPI where present, and otherwise panic with `host-contract` and a
   message naming the missing feature (Q9).
10. **The `disk` metric and the `hd` binary.** wasmtime with Cranelift has
    no published size, and `hd` will likely pass 10 MB. Recommendation:
    count only per-worktree artifacts in `disk`, since one binary serves
    every worktree, and set a separate binary budget after slice 6
    measures it (Q12).
11. **Panic categories for resource limits.** `--max-heap` and the time
    limit are v1, but the stable categories have no entry for either.
    Recommendation: add two, such as `heap-exhausted` and `time-limit`.
12. **Browser baseline.** Recommendation: require Wasm GC only (Chrome
    119, Firefox 122, Safari 18.2, the baseline wasm_of_ocaml documents),
    and make JSPI, JS string builtins, exceptions and stack switching
    optional or unused.
13. **The transitive `block_on` ban across modules.** The ban in `defer`
    suites, defaults, facts and non-entry initialization follows bodies
    into other modules and packages, so it is the one interface fact that
    syntax cannot give (Q4b). Recommendation: keep the spec, and compute a
    per-folder drive summary with its own hash. The alternative, a spec
    change, bans only direct calls and panics at run time instead.
14. **Generic calls in that ban.** The spec does not say whether a call
    through a generic bound, such as `x.close()` with `T < Close`, is
    "statically known". If it is, the ban can only be checked per
    instantiation, at build time. Recommendation (low confidence): treat
    it as unprovable, as a dynamic trait call is, and let a stress test
    of real `defer` code show whether that rejects too much.

## Part 2: The Back Half

Part 2 continues the numbering: Q8 to Q16 answer the brief's questions 1
to 9, and a last section compares the whole design with the Vx proposal.
Part 1's choices are inputs here: Rust, the per-module cache, the task
graph, the data-oriented core, and the four portability interfaces.

Measured costs of the frozen prototype that Part 2 explains:

| Cost | Value | Cause, read from `src/` and `lib/std` |
| --- | --- | --- |
| `sum` loop vs Node | 11x in the [baseline](../../audit/compiler/baseline-2026-10-06.md#62-runtime-microbenchmarks); 17x in the M1c metrics run (reported to this research, not in the repo) | the range loop runs the iterator protocol with boxed `i32` cells and an `Option` struct per step ([Prototype Baselines](goals.md#prototype-baselines-to-beat-2026-10-06)) |
| counted loop allocations | 80 B per iteration (M1c run) | the same boxes |
| JSON vs Node | 3 to 4% (M1c run) | `List[u8]` stores one boxed `anyref` per byte (F-505), `encode` builds a whole `Json` tree before writing text, and trait calls go through rebuilt dictionaries (F-502) |
| `list_dir!` per call | 52 µs (M1c run) | structured values cross as node trees, and strings cross one host call per byte (F-558) |
| Binaryen | 14.6 MB of the playground worker | used only to parse WAT, validate and write the binary; `src/wasm.ts` runs no optimization pass |

The last row matters most for the browser: the prototype pays for Binaryen
and gets none of its optimizations.

## Q8: Mid-Level IR And Monomorphization By Layout

**Question.** What does "code per value layout" mean for hd? How are trait
dictionaries passed, how is code size bounded, where do instantiations live
in the per-module cache, and is the IR SSA or structured?

### Prior Art

| System | Strategy | Evidence |
| --- | --- | --- |
| Go 1.18+ | **GC-shape stenciling**: one copy per shape (same underlying type, or any pointer); a dictionary is the hidden first argument, holding type descriptors, sub-dictionaries and itabs; the linker deduplicates instantiations by name ([design](https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md)) | generic code with interface type arguments ran about 2x slower than monomorphized code; "more often than not, makes Generic code slower" ([PlanetScale](https://planetscale.com/blog/generics-can-make-your-go-code-slower)) |
| .NET | reference-type instantiations share one canonical body (`__Canon`); value-type instantiations get their own; a hidden context argument finds a lazily filled dictionary ([BOTR](https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/shared-generics.md)) | ships everywhere; the JIT sees exact types for value types |
| Swift | every generic has an unspecialized body taking witness tables; the optimizer specializes where the body is visible; across modules only `@inlinable` bodies specialize ([Swift forums](https://forums.swift.org/t/brave-new-world-best-practices-for-cross-module-optimization/66869)) | unspecialized paths can be about two orders of magnitude slower ([Pestov, Compiling Swift Generics](https://download.swift.org/docs/assets/generics.pdf)) |
| Rust MIR | full monomorphization per type; `-Zpolymorphize` (share code that ignores a parameter) was removed in December 2024 as buggy and of little use ([PR #133883](https://github.com/rust-lang/rust/pull/133883)) | fastest code; compile time and size are the cost |
| OCaml | uniform representation: one body for all types; ints tagged, everything else boxed | small code; boxing costs in numeric code |
| MLton | whole-program monomorphization and defunctorization ([MLton](http://mlton.org/Monomorphise)) | fast code from a whole-program compiler |

Two facts decide the choice for hd:

- **Shared code needs indirect calls, and wasmtime does not undo them.** V8
  speculatively inlines `call_ref` and `call_indirect` from runtime
  feedback since Chrome 137, worth 1.59x on Dart microbenchmarks
  ([V8](https://v8.dev/blog/wasm-speculative-optimizations)). Cranelift
  compiles ahead of time without feedback, and its own inliner is off by
  default ([Fitzgerald](https://fitzgen.com/2025/11/19/inliner.html)). v1
  runs on wasmtime, so dictionary calls stay indirect calls there.
- **hd builds are whole programs.** Only reachable items are compiled (a
  Day 1 decision), and every Wasm module is linked from one entry. MLton
  and Rust's final link step show that a whole-program build can afford
  per-type code.

### What "Per Value Layout" Should Mean

**Recommendation (confidence medium-high): monomorphize per concrete type,
then fold identical bodies.**

1. **Instantiate per type.** The reachability walk from the entry collects
   `(item, type arguments)` pairs, as rustc's collector does. Each instance
   is compiled with exact Wasm types, so a field read needs no cast and a
   trait call is a direct call.
2. **Fold by layout after emission** (identical code folding, as linkers'
   ICF and LLVM's MergeFunctions do). `List[Point].push` and
   `List[User].push` emit the same bytes when both elements are references
   and no trait call differs, so they fold into one function. This gives
   the .NET sharing for references without a dictionary. The fold key is a
   hash of the emitted body with its relocations, so folding is
   deterministic. The application of ICF to this problem is mine.
3. **Layout classes** decide the Wasm value type of each type argument:

   | Layout class | hd types | Wasm |
   | --- | --- | --- |
   | `i32` | `bool`, `char`, all integers of 32 bits or less, `usize` on Wasm32, payloadless enums | `i32` |
   | `i64` | `i64`, `u64` | `i64` |
   | `f32`, `f64` | floats | `f32`, `f64` |
   | `ref T` | `data`, enums with payloads, `string`, lists, maps, closures, frames | `(ref $T)` or `(ref null $T)`, exact type |
   | `pair` | `Option` of a scalar, small `Result`s | two or three Wasm values in locals, parameters and multi-value results; two fields in a struct |
   | `erased` | trait values, `Any`, a GADT existential payload | `anyref` plus a vtable; scalars as `i31ref` or a box |

   `i31ref` is the owner's first step and stays in the erased positions,
   where a scalar must become a reference. In monomorphized code, scalars
   are never boxed at all. This narrows the earlier i31ref decision; it
   does not contradict it.
4. **Dictionaries exist only where the spec needs runtime evidence:** trait
   values carry a vtable struct, a Swift-style witness table built once
   per (type, trait) pair as an immutable global. (GADT existentials, which
   also stored evidence, were removed on 2026-10-07.)
5. **Polymorphic recursion** (a generic call that instantiates itself at a
   growing type) has no finite instantiation. Stop at an instantiation
   depth limit with its own diagnostic, as rustc does. Part 1 lists this
   limit without a code ([Hard Limits](#hard-limits)).
6. **GADTs** are removed (owner, 2026-10-07).
   **Variadic generics** are gone; `Args < Tuple` instantiates like any
   other type, and `all!` gets one frame type per tuple of child types.

**Bounding code size.**

- Only reachable instances are emitted, and identical ones fold.
- Requirement rows are not type arguments for layout. A row-polymorphic
  function takes its providers as one context reference, so rows never
  multiply instances (mine; the prototype's F-550 and F-551 show what the
  alternative costs).
- A per-item instance budget, with its own diagnostic, is the fallback if
  the `dead-code` metric (10 KB per 1,000 lines) fails in practice. I would
  not add it before a measurement says so.
- Debug and release use the same instantiation strategy. The
  `release-check-cost` metric wants debug within 1.3x of release; a debug
  tier with shared generics would miss it on any generic-heavy test.

### The IR

| Option | Used by | Fit for hd |
| --- | --- | --- |
| SSA CFG, then a relooper or stackifier to rebuild Wasm blocks | LLVM (CFGStackify), Binaryen's Relooper, Cranelift as a *consumer* of Wasm | needs control-flow recovery; [Ramsey's "Beyond Relooper"](https://dl.acm.org/doi/10.1145/3547621) shows a dominator-tree method that always succeeds on reducible graphs |
| structured, typed tree IR with explicit locals | Binaryen IR, dart2wasm, Kotlin/Wasm, Scala.js | emits Wasm blocks directly; optimizations are tree rewrites |
| structured IR with SSA-like single assignment per local | MoonBit's multi-level IR (reported, unverified detail) | both |

hd source has no `goto`. Loops, `break`, `continue`, `match` and `?` map to
Wasm `block`, `loop`, `br`, `br_if` and `br_table`. The only unstructured
graph is the suspension state machine, which is a `loop` around a
`br_table` on the state ([Q9](#q9-suspension-lowering)).

**Recommendation (confidence medium-high): a structured MIR.**

- A typed tree of blocks, loops and branches over numbered locals, one
  arena per body, in Part 1's flat-array style. Each local is assigned in
  one place where that is cheap; otherwise it is a plain mutable local.
- Generic MIR is lowered once per module from checked bodies, before
  instantiation. Instantiation substitutes types and picks layouts.
- The native backend (Q15) lowers the same MIR to Cranelift IR, which takes
  structured input as easily as a CFG, since a tree is a CFG.

### Where Instances Live In The Cache

Go compiles an instantiation in every package that needs it and lets the
linker drop duplicates. Rust (with `-Zshare-generics` in debug) reuses an
upstream crate's copy. Swift specializes in the caller.

**Recommendation (confidence medium):**

1. The **module cache entry** from Part 1 stores the module's generic MIR,
   serialized with stable paths, beside its diagnostics and facts.
2. A second **codegen cache**, content-addressed through the same
   `CacheStore`, maps an instance key to the instance's emitted Wasm body
   with symbolic relocations. The key hashes the item's MIR hash, the
   canonical type arguments, the MIR hashes of the items it inlines, the
   tier and the compiler build ID.
3. A per-program **link step** walks reachability, fetches or emits
   instances in parallel, folds identical ones, assigns function and type
   indices, and patches relocations. LLVM's Wasm objects use padded 5-byte
   LEBs at relocation sites so patching never moves code
   ([tool conventions](https://github.com/WebAssembly/tool-conventions/blob/main/Linking.md));
   the cached bodies can do the same, and a release build can re-encode
   compactly.

A private body edit then changes one MIR hash and re-emits only that
item's instances; `main`, the tests and other worktrees share every other
cached instance. Cranelift adds its own per-function cache below this
(Q11).

### Risks

- **Compile time of full monomorphization.** rustc's cost is mostly LLVM.
  Here each instance is one walk of a small MIR tree into `wasm-encoder`,
  and instances are cached. Measure with `cold-check`-style build timing in
  slice 7.
- **Folding hides names.** A folded function has one name in backtraces.
  Keep a list of folded names per body in the symbol table, as linkers do
  with ICF.
- **Exact types make many Wasm types.** Each instance of a generic `data`
  is its own struct type. Type sections grow; V8 and wasmtime canonicalize
  equal types, so the cost is size only. Watch `dead-code`.

**What would change it.** If the `dead-code` or build-time metrics fail on
generic-heavy programs even after folding, add .NET-style sharing for
reference-only instances with a context argument, in the debug tier first.

## Q9: Suspension Lowering

**Question.** State machines, Wasm stack switching, JSPI or CPS? It must
work on wasmtime and on V8 in a browser, keep the ready path free of
allocation, support `all!`, `race!` and cancellation, reserve hook points,
and let host calls suspend.

### What The Spec Already Fixes

The spec describes a poll-based, cold, one-shot protocol: a plain call
builds a cold suspension that captures arguments and providers; a bang call
drives it; `cancel` is synchronous and runs registered `defer` suites; and
`fn name!` "is source sugar for a compiler-generated cold state machine"
([Compilation Strategy](../../spec/lang/11-requirements-and-suspension.md#compilation-strategy)).
The representation may differ only if it keeps those behaviors
([`req.lowering.representation`](../../spec/lang/11-requirements-and-suspension.md#r-req.lowering.representation)).

### Prior Art

| Mechanism | Who ships it | Status, mid-2026 | Cost model |
| --- | --- | --- | --- |
| State machine, frames nested by value | Rust `async` | stable | no allocation; recursion needs a `Box` |
| State machine, frame boxed on the first real wait | C# `async` | stable | the state machine is a struct; it moves to the heap only when an awaited task is not complete ([Toub](https://devblogs.microsoft.com/dotnet/how-async-await-really-works/)) |
| CPS state machine, one continuation object per call | Kotlin coroutines ([KEEP](https://github.com/Kotlin/KEEP/blob/master/proposals/coroutines.md)) | stable, including Kotlin/Wasm | an allocation per suspending call |
| Wasm stack switching (`cont.new`, `resume`, `suspend`) | proposal at phase 3 | wasmtime: tier 3, x86_64 Linux only, off by default ([wasmtime](https://docs.wasmtime.dev/stability-wasm-proposals.html)); V8: wasm_of_ocaml's README names Chrome 148+ (flag status unverified); not shipped in any engine by default ([tracking](https://github.com/theSherwood/temen/issues/1651)) | a stack per task |
| JSPI | phase 4 (April 2025) | Chrome 137+; Firefox intends 153; Safari 27 beta ([OpenReplay](https://blog.openreplay.com/jspi-javascript-wasm-bridge/)); not a wasmtime feature | suspends the whole Wasm stack at a Promise-returning import |
| CPS in JS | wasm_of_ocaml's fallback for effects | works everywhere | "slower, larger" ([README](https://github.com/ocsigen/js_of_ocaml/blob/master/README_wasm_of_ocaml.md)) |
| Component Model async, stackless "callback" ABI | WASI 0.3 | wasmtime on by default | the export returns an exit, yield or wait code, and the runtime calls a callback later; it needs linear memory ([Concurrency.md](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Concurrency.md)) |

Stack switching is the only option that is not available on both v1
engines, and JSPI is browser-only. State machines are the only mechanism
that runs today on wasmtime, V8, Firefox and Safari alike.

### Recommendation

**State machines, with lazily materialized frames.** Confidence: high for
state machines, medium for the lazy frame.

1. **One function per suspending body**, `f$run(frame, args...)`. With a
   null frame it starts at state 0 with its arguments in locals. With a
   frame it reloads the live locals and jumps to the saved state through a
   `br_table` inside a `loop`. Code grows linearly: one resume point and
   one save sequence per suspension point. (The prototype grows about N^3,
   F-552.)
2. **The ready path allocates nothing** (C#'s design, mapped to Wasm by
   me). A bang call `g!(x)` calls `g$run(null, x)` directly. If `g`
   finishes, the result comes back as a plain Wasm result. Only when a
   callee returns Pending does the caller allocate its own frame, store
   its live locals and the child's frame, and return Pending in turn. So an
   await that completes at once costs one direct call and one test, which
   fits the 100 ns budget with room to spare.
3. **A cold call allocates.** `g(x)` without a bang must capture arguments
   and providers, so it builds a frame at once. That is the spec's
   semantics, not overhead.
4. **Frames are GC structs** that extend one `$Suspend[T]` struct per
   result layout. The base holds the state and a vtable with `poll` and
   `cancel`, so a stored `mut Suspend[T]` is polled through one
   `call_ref`. Wasm GC cannot nest a struct inside another by value, so
   Rust's nested frames are not available; the lazy frame recovers most of
   their benefit.
5. **Cancellation** reads the state and runs that state's registered
   `defer` suites, innermost frame first, after cancelling the child, as
   [`req.cancel.defer`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.defer)
   orders. The emitter generates one cleanup table per suspending body.
6. **`all!` and `race!`** are intrinsic frames holding their children.
   `all!` polls children in argument order and records results in the
   tuple frame; `race!` cancels losers synchronously. Two ready children
   cost their two cold frames and no frame for `all!` itself when it is
   bang-called, which meets 1M tasks per second by a wide margin. A later
   optimization (mine): when `all!(a(x), b(y))` names cold calls directly,
   start them with null frames too.
7. **Hook points live in the MIR, not in the binary.** Each suspension
   point, frame creation and completion carries a `Hook(kind, site)` MIR
   instruction. The normal emitter drops it, so it costs nothing at run
   time; a trace or replay build emits a call to a hook import. This is
   how I read the owner's "no-op hook points": reserved in the IR, absent
   from release code. Cranelift's inliner is off by default, so an empty
   function call would not be free on wasmtime.
8. **The entry driver** is a pair of exports, `hd_poll_main` and
   `hd_wake(id)`. The host calls `hd_poll_main`; on Pending it returns to
   its event loop; a host completion calls `hd_wake` and polls again. Wakes
   are coalesced, as
   [`req.waker.coalesced`](../../spec/lang/11-requirements-and-suspension.md#r-req.waker.coalesced)
   says. This is the stackless callback ABI of WASI 0.3 in hd's own core
   imports, so a later move to the Component Model is a re-encoding, not a
   redesign (Q12).

### The `block_on` Problem

`block_on` drives a suspension synchronously from non-suspending code
([`req.drive.block-on`](../../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on)).
When its argument waits on the host, the Wasm stack must stay intact while
the host does I/O:

| Host | How `block_on` waits |
| --- | --- |
| wasmtime in `hd` | a host import runs the Rust event loop until the wake arrives, on the same native thread; the Wasm stack just sits below it |
| a browser with JSPI | the import returns a Promise and JSPI suspends the stack (Chrome 137+ today) |
| a browser without JSPI | only `Atomics.wait` in a Worker with a helper Worker, which needs cross-origin isolation, as the prototype's synchronous fetch does |

This is an [open question](#open-questions-for-the-owner): my
recommendation is JSPI where present, and otherwise a `host-contract`
panic that names the missing feature, so the playground keeps Part 1's "no
COOP/COEP" choice.

### Risks

- **Two kinds of call per suspending function** (bang and cold) and a save
  sequence per suspension point add code. Count it in `dead-code`.
- **Deep chains of Pending** allocate one frame per level on the first real
  wait. That is Kotlin's steady-state cost, paid only once per wait.
- **Stack switching may ship on both engines** within v1's life. It would
  not replace state machines, because the spec's cold suspensions and
  synchronous cancellation map to frames directly. It could later serve
  `block_on` in browsers without JSPI.

**What would change it.** Only an owner change to the suspension protocol,
such as making suspensions multi-shot, would reopen this.

## Q10: Wasm GC Codegen

**Question.** How should values look in Wasm GC, what do other GC-language
toolchains do, which choices need Binaryen, and should the compiler emit
binary directly?

### Prior Art

| Toolchain | Optimizer | Notes |
| --- | --- | --- |
| dart2wasm | Binaryen `wasm-opt` | classes as struct subtypes with a class-id field; uses `wasm:js-string` with a polyfill ([flutter/engine #51488](https://github.com/flutter/engine/pull/51488)) |
| Kotlin/Wasm | Binaryen | JS string builtins; coroutines as CPS state machines |
| J2Wasm (Java) | Binaryen | `wasm-opt` made benchmarks 1.9x faster on average ([V8](https://v8.dev/blog/wasm-gc-porting)) |
| wasm_of_ocaml | Binaryen, required on the PATH | effects through JSPI by default, CPS as fallback, stack switching optional ([README](https://github.com/ocsigen/js_of_ocaml/blob/master/README_wasm_of_ocaml.md)) |
| Scala.js Wasm backend | its own IR optimizer, no Binaryen | 15% faster than its JS output, geomean; code twice the JS size ([Scala.js 1.19](https://www.scala-js.org/news/2025/04/21/announcing-scalajs-1.19.0/)) |
| MoonBit | its own whole-program, multi-level IR optimizer | emits Wasm features `wasm-opt` did not accept ([forum](https://discuss.moonbitlang.com/t/support-for-binaryens-wasm-opt/209)); JS string builtins in the browser |
| Guile Hoot | its own Scheme toolchain | no Binaryen (unverified detail) |
| Go `wasm` | no Wasm GC: its own GC in linear memory | goroutines by rewriting functions for resumption (unverified detail); large binaries ([Go wiki](https://go.dev/wiki/WebAssembly)) |

What Binaryen buys a GC language, per V8's porting guide: whole-program type
refinement, GUFA (type-aware content flow), escape analysis that moves
allocations to locals, devirtualization, cast removal, and type merging
([V8](https://v8.dev/blog/wasm-gc-porting)). Most of these exist to recover
types a uniform front end erased. Per-type monomorphization (Q8) emits
exact types, direct calls and no casts in the first place, which is why
Scala.js and MoonBit get by without it.

### Representation

**Recommendation (confidence medium-high):**

| hd value | Wasm GC representation | Why |
| --- | --- | --- |
| `data` | an immutable or mutable struct per type, exact `(ref $T)` | shared reference semantics ([`data.ref.shared`](../../spec/lang/08-data-and-enums.md#r-data.ref.shared)) |
| payloadless enum | `i32` | no allocation |
| `Option` of a reference | `(ref null $T)`, null is `.None` | v1 decision; nested `T??` keeps an outer tagged pair, since [`types.option.nest`](../../spec/lang/04-type-system.md#r-types.option.nest) must tell `.None` from `.Some(.None)` |
| `Option` of a scalar | a `pair` (`i32` tag plus the value) in locals and results; two fields in a struct | v1 decision |
| `Result[T, E]` | a `pair` or triple in returns; a struct only when stored | every `?` and every serde call returns one (mine: the same treatment as `Option`) |
| enum with payloads | an abstract base struct with an `i32` tag and the shared fields, one subtype per variant; `match` is a `br_table` on the tag, then a `ref.cast` the engine knows succeeds | the dart2wasm class-id pattern; the owner's per-enum choice stays open for small enums |
| closure | a struct per capture shape, subtype of one base per signature holding a typed `funcref`; a capture-free closure is a global constant | v1 decision; a `mut` capture becomes a field, with no cell when only the closure writes it after capture |
| `string` | an immutable `(array i8)` | see Q13 |
| `List[T]` | a struct with a length and a `(ref (array (mut T')))` where `T'` is the element's layout: `i8` for `u8`, `i32`, `f64`, or a reference | fixes F-505; JSON's `List[u8]` becomes a byte array |
| trait value | a struct of `anyref` plus a vtable reference | erased position; scalars as `i31ref` |
| panic | record the category and site in globals, then `unreachable` | panics are not catchable and poison the instance ([`flow.panic.poison`](../../spec/lang/06-control-flow.md#r-flow.panic.poison)) |

**No Wasm exceptions in v1.** hd has no exceptions, `?` is result-based, and
a panic ends the instance. Exceptions are on in wasmtime 47, but nothing in
v1 needs them. They become useful only if a later feature catches panics
per test without re-instantiating.

### Binaryen Or Not

| Option | Browser cost | Native cost | Gain |
| --- | --- | --- | --- |
| Binaryen in-process, as today | 14.6 MB worker today | links C++ | its passes, if enabled |
| our own MIR optimizations | none extra | none extra | inlining, counted loops, scalar replacement, iterator fusion, done where types are known |
| `wasm-opt` as an optional external native tool for `hd build --release --opt` | none | user installs it | J2Wasm saw 1.9x; for monomorphized code the gain is unmeasured |

**Recommendation (confidence high):** emit binary directly with
`wasm-encoder` ([docs](https://docs.rs/wasm-encoder)), validate in debug
builds of the compiler with `wasmparser`, and print WAT for the playground's
view with `wasmprinter`. All three are small Rust crates. Do the
optimizations that matter (Q11) in the MIR. Leave `wasm-opt` as the Later
feature it already is in the triage. Dropping Binaryen alone frees most of
the playground's 14.6 MB.

### Risks

- **Without GUFA-style analysis, some casts remain** in erased positions
  (trait values, `Any`). They are rare by construction; count them in a
  compiler statistic.
- **Exact struct types per instance** inflate the type section. Measure
  the tiny program against the 2 KB budget early (slice 6).

## Q11: Engines And Tiers

**Question.** How mature is wasmtime's GC against V8? Is Winch the dev
tier? How do startup caches, fuel, epochs and heap limits work, and where
does the optimizing tier fit?

### wasmtime

- **GC is on by default since Wasmtime 47** (2026-07-20), with a
  Cheney-style semi-space copying collector. The team "mainly focused …
  on the correctness of our collector … and less so on its performance",
  and says its throughput and latency "won't match" V8 or SpiderMonkey
  ([Bytecode Alliance](https://bytecodealliance.org/articles/wasmtime-gc)).
  The other collectors are deferred reference counting (no cycles) and a
  null collector that never frees
  ([Collector](https://docs.wasmtime.dev/api/wasmtime/enum.Collector.html)).
  I found no published benchmark of wasmtime GC against V8.
- **Winch cannot be the dev tier.** Winch supports none of `gc`,
  `function-references`, `exception-handling` or `tail-call`
  ([proposal status](https://docs.wasmtime.dev/stability-wasm-proposals.html)).
  Every hd program uses GC, so the dev tier is **Cranelift at its lowest
  optimization level**.
- **Compile caches.** `Module::serialize` and `deserialize` store
  precompiled code
  ([pre-compiling](https://docs.wasmtime.dev/examples-pre-compiling-wasm.html)),
  and `Config::enable_incremental_compilation` caches Cranelift's output
  per function through a `CacheStore` trait
  ([Config](https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html)).
  Both plug into Part 1's `CacheStore`. An edited test module then
  recompiles only changed functions, because the codegen cache (Q8) keeps
  unchanged instances byte-identical.
- **Interruption.** Epoch interruption is "up to 2-3x" faster than fuel;
  fuel is for deterministic yielding
  ([Config](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html)).
- **Heap limits.** `gc_heap_reservation` and a `ResourceLimiter` bound
  "linear memory + GC heap" (same source).

### V8 In The Browser

V8 starts every function in Liftoff and tiers hot ones up to TurboFan
([dynamic tiering](https://v8.dev/blog/wasm-dynamic-tiering)). Chrome caches
optimized code for modules loaded with `compileStreaming`
([code caching](https://v8.dev/blog/wasm-code-caching)). TurboFan inlines
indirect calls speculatively. Playground programs are small, so Liftoff
start-up matters more than peak speed.

### Recommendation

Confidence: medium (wasmtime's GC throughput is the open risk).

1. **wasmtime for `hd run` and `hd test`**, Cranelift only. Debug builds
   use a low Cranelift opt level; release uses `Speed`. Measure the
   compile time of both in slice 6.
2. **Precompiled code in the cache**, keyed by the Wasm hash and the
   wasmtime config, plus the per-function Cranelift cache.
3. **Time limits by epoch**, ticked by a host timer thread. Fuel only for
   the later deterministic simulation mode, where a reproducible yield
   point is the point.
4. **`--max-heap` through `gc_heap_reservation` plus a `ResourceLimiter`.**
   The spec has no panic category for heap or time exhaustion yet (an
   [open question](#open-questions-for-the-owner)).
5. **The optimizing tier is ours.** Inlining small functions, counted
   range loops, scalar replacement of non-escaping closures and frames, and
   iterator fusion run on the MIR. Cranelift `Speed` does local work below
   that. This is the IR boundary the triage reserves, and the same MIR
   feeds the native backend later.
6. **A null collector for short test instances** is a cheap experiment
   (mine): a unit test that allocates under a fixed budget never pays for a
   collection. Measure before adopting.

### Risks

- **wasmtime GC throughput.** The `runtime` target is 1.5x Node, and
  Node's GC is generational. A semi-space collector copies every live
  object on each collection. Allocation-heavy cases (`map`, `sort`) may
  miss the target on wasmtime while passing on V8. Mitigation: fewer
  allocations by design (Q8, Q10), and the native backend with its own GC
  (Q15) right after v1.
- **Cranelift compile time on large tests.** Mitigated by the two caches.

**What would change it.** If wasmtime's GC misses `runtime` or
`long-run-memory` by a wide margin, an option is running `hd test` on V8
through a bundled engine. That costs the binary size and the start-up time
that led to wasmtime, so I would rather bring the native backend forward.

## Q12: Host Interface And Embedding

**Question.** Hand-rolled core-Wasm imports or the Component Model? How are
startup refusal, async host calls on both engines, the JS host, and the
cost of structured values handled? How big is wasmtime with Cranelift?

### The Component Model Today

- **No GC types in the canonical ABI.** Both the stackful and the stackless
  async ABIs pass values through linear memory
  ([Concurrency.md](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Concurrency.md)).
  A GC option is a pre-proposal
  ([issue #525](https://github.com/WebAssembly/component-model/issues/525)).
  Wasmtime names GC integration its "next big milestone"
  ([Bytecode Alliance](https://bytecodealliance.org/articles/wasmtime-gc)),
  and Component Model 1.0 is planned without it
  ([roadmap, 2026-06-08](https://bytecodealliance.org/articles/the-road-to-component-model-1-0)).
- **No native browser support.** Browsers run components through `jco`,
  which transpiles them to core Wasm and JS (same source).

So an hd program built on Wasm GC cannot be a component today without
copying every string and list through a linear memory. That is fine for a
later `--target wasi` export, but not as v1's internal ABI.

### Recommendation

**A small hand-rolled core-Wasm ABI, shaped after WASI 0.3.** Confidence:
medium-high.

1. **One import per host method**, named `hd:<trait>/<method>`, as the
   prototype already does. Startup refusal keeps reading the import list
   ([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs)),
   and dead-code removal keeps that list exact.
2. **Scalars cross as Wasm values.** A `println` of a string is one call.
3. **Structured values cross as bytes in one exchange buffer.** The module
   exports a small linear memory. Generated code writes the argument's
   bytes into it with Wasm loops (fast in compiled code), and the host
   reads the whole buffer in one go; results come back the same way. Wasm
   GC has no bulk copy between arrays and memory, so the copy is a loop on
   both engines. The encoder for each boundary type is generated like a
   derive template. This replaces F-558's call per byte, and should bring
   `list_dir!` from 52 µs under the 5 µs budget (an estimate, to be
   measured).
4. **Async host calls are start-and-poll.** A host method that can wait
   returns either its result at once or a pending handle. `host_wait!`
   registers the current waker against the handle and returns Pending; the
   driver (Q9) returns to the host. The host completes the operation,
   calls `hd_wake`, and polls again.
   - **wasmtime:** Wasm calls stay synchronous Rust calls. Host operations
     run on a single-threaded async reactor in the `hd` process (tokio's
     current-thread runtime is one choice). No fibers and no
     `func_wrap_async` are needed, except for `block_on` (Q9).
   - **Browser:** the JS host starts a Promise, keeps it in a handle
     table, and on settle calls `hd_wake` and polls. No `Atomics.wait`, no
     `SharedArrayBuffer`. Cancellation calls `AbortController.abort()`,
     which gives `send!` the real cancellation the prototype lacks.
5. **One ABI description, two hosts.** A Rust crate describes every import
   (name, trait, parameter codecs, whether it can wait). The wasmtime host
   implements it in Rust; a build step generates the JS glue for the
   browser from the same description. Grants are checked in both hosts'
   providers, as HOST_CAPABILITIES requires.
6. **Components later.** The import set mirrors WASI 0.3 interfaces where
   they exist (`wasi:filesystem`, `wasi:http`), so a `--target wasi`
   adapter can map them once the GC ABI lands.

**This contradicts two texts.** [HOST_CAPABILITIES](../HOST_CAPABILITIES.md#boundary-abi)
labels its second column "Official runtime (Component Model)", and the spec
says `host_wait!` "maps an opaque host wait operation to the WebAssembly
Component Model async ABI as used by WASI 0.3"
([`req.host-wait.leaf`](../../spec/lang/11-requirements-and-suspension.md#r-req.host-wait.abi)).
Neither is reachable for a Wasm GC program in v1. See the
[open questions](#open-questions-for-the-owner).

### Size Of `hd` With wasmtime

| Build | Size | Source |
| --- | --- | --- |
| wasmtime runtime only, `--release --no-default-features` | 2.1 MB | [minimal embedding](https://docs.wasmtime.dev/examples-minimal.html) |
| the same with LTO | 1.2 MB | same |
| with nightly `build-std` and `panic_immediate_abort` | 0.7 MB | same |
| with Cranelift | not published: "no effort has yet been put into minimizing the code size of Cranelift" | same |
| the `wasmtime` CLI | "~30 MB+" | a third-party tutorial ([wasmruntime.com](https://wasmruntime.com/en/tutorials/wasmtime)), unverified |

Part 1's handoff note gave "about 0.7 MB" for a minimal wasmtime; that is
only the nightly, `panic_immediate_abort` build. The stable minimal build is
2.1 MB. With Cranelift, plus the compiler itself and the embedded std, `hd`
is likely above 10 MB. Two ways out: measure the real binary in slice 6,
and read the `disk` metric as per-worktree artifacts, since one `hd` binary
serves every worktree. That is an
[open question](#open-questions-for-the-owner).

### Risks

- **The exchange buffer adds a linear memory** to every module that crosses
  structured values. It is a few pages; modules that cross only scalars
  have none.
- **Two host implementations drift.** Mitigation: generated JS glue and
  cross-backend conformance on every change.

## Q13: Runtime Pieces

Strings, maps, serde, panics, backtraces and debug checks, more briefly.

### Strings

The spec fixes the representation: valid UTF-8 bytes, `len()` in bytes in
constant time, and "at every Wasm host boundary, a string crosses as its
UTF-8 bytes with no conversion"
([`types.string.host-bytes`](../../spec/lang/04-type-system.md#r-types.string.host-bytes)).
JS string builtins are UTF-16 and exist only in browsers (Chrome, Firefox,
and Safari since 26.2, [WebKit](https://webkit.org/blog/18178/webkit-features-for-safari-26-6/)),
not in wasmtime. They cannot carry hd's semantics. A UTF-8 text-encoding
builtin is only an open design issue
([design #1583](https://github.com/WebAssembly/design/issues/1583)).

**Recommendation (high):** one representation on both engines, an
immutable `(array i8)`. Substrings copy, as Java has since 7u6; the
prototype's struct of array, start and length costs an extra indirection on
every access. A `StringBuilder` is a growable `(array (mut i8))` with a
length, copied once at the end, and `join` sums lengths first, which fixes
the O(L log n) join of the [baseline](../../audit/compiler/baseline-2026-10-06.md#81-string-join-is-ol-log-n).
In the browser, `TextDecoder` and `TextEncoder` convert at the boundary.

### Maps And Hashing

Monomorphized `Hash` and `Eq` calls are direct, so a map needs no stored
function references (the prototype keeps `key-eq` and `key-hash` funcrefs
in every map). **Recommendation (medium):** an open-addressing table over
per-layout key and value arrays, with a fast non-cryptographic hasher.
Whether iteration order is insertion order is a stdlib question, not a
compiler one.

### Serde And JSON

The 3 to 4% has three causes, each with a fix:

| Cause | Fix |
| --- | --- |
| one boxed `anyref` per byte in `List[u8]` | per-layout lists (Q10): a byte array |
| `encode` builds a `Json` tree, then renders it | a streaming `JsonWriter` that implements `Serializer` and writes bytes directly; decode reads bytes into the target type through `Deserializer` without a tree. This is a stdlib change |
| trait calls through dictionaries and `Result` structs | `W < Serializer` is monomorphized, so every call is direct and inlinable; `Result[void, E]` returns as a pair |

With those, the encoder is a byte loop over direct calls, which is what
`serde_json` is. Reaching 0.5x of Node's native `JSON` is plausible but
unmeasured. Confidence: medium.

### Panics And Backtraces

- **Category and site.** Every panic stub stores its category and a site
  ID in globals before `unreachable`. Engine traps map to categories too:
  an integer division by zero trap is `integer-division-by-zero`, and a
  stack overflow (wasmtime's trap code, V8's `RangeError`) is
  `stack-exhausted`. `List` indexing still checks against the length,
  because the backing array's capacity is larger; a string index can rely
  on the engine's array bounds check, which then maps to
  `index-out-of-bounds` by its code offset.
- **Locations from code offsets, not text.** wasmtime returns a
  `WasmBacktrace` whose frames carry function indices and module offsets;
  V8's `Error.stack` has `wasm-function[i]:0xOFF` frames, which the
  prototype already parses. One site table, from code offset to source
  span, serves both. Keep it in a custom section or a side file next to
  the build.
- **Symbolized release backtraces.** Always emit the `name` section; it is
  small and both engines use it in traces. Emit DWARF only on request, for
  native debuggers. For browser devtools, emit a source map with the
  `sourceMappingURL` section, as the prototype already does through
  Binaryen.

### Debug-Tier Checks

- **Overflow.** Wasm has no overflow flag. For 32-bit types, compute in
  `i64` and compare; for 64-bit, use the sign-xor test. Each costs two to
  four instructions. `release-check-cost` (debug ≤ 1.3x release) is the
  check.
- **Bounds.** Explicit for lists; free for strings and arrays, from the
  engine.
- **Closed handles and deadlocked suspension** (v1 items): a closed flag in
  each handle struct; a driver that polls with no pending host operation
  and no wake reports the deadlock.

## Q14: Hot Reload (Later, Brief)

| System | Mechanism |
| --- | --- |
| Erlang | two versions of a module live at once; a fully qualified call switches to the new one |
| Dart | the VM swaps function bodies and keeps state; on the web, hot reload works with DDC's JS output but "is not supported" for dart2wasm, which offers hot restart only ([Flutter issue](https://github.com/flutter/flutter/issues/190777)) |
| Zig | incremental compilation patches the binary in place |
| JVM | HotSwap replaces method bodies; a schema change restarts |

Dart's own Wasm backend has no hot reload, which says the problem is not
solved on Wasm. One fact helps: Wasm GC types are canonicalized
structurally across modules, so a new module that declares the same rec
groups can read and write the old module's objects.

**What the dev tier should reserve now** (mine, from the Dart and JVM
model the triage names):

- Dev-tier calls between user functions go through one `funcref` table, so
  a new module can overwrite entries.
- Type declarations are emitted in a canonical, deterministic order, so
  the same `data` layout gives the same Wasm types in the next build.
- Module state (globals) is exported, never private, in the dev tier.

The cost is one `call_indirect` per call in the dev tier, which also feeds
`release-check-cost`. Reserve the table in the emitter, but turn it on only
when hot reload is built, after a measurement.

## Q15: Native Backend With Its Own GC (After v1, Brief)

- **Precise roots.** Cranelift's user stack maps let the front end mark
  values that hold GC references; `cranelift-frontend` spills them at
  safepoints and the stack map tells a moving collector where to update
  them ([Bytecode Alliance](https://bytecodealliance.org/articles/new-stack-maps-for-wasmtime)).
  wasmtime itself has used them since version 25.
- **Collector options:**

  | Collector | Moving | Notes |
  | --- | --- | --- |
  | Immix / Sticky Immix via MMTk (Rust) | opportunistic | MMTk is a Rust GC framework; Julia ships it as an alternative collector; "not yet ready for production use" per its status page ([MMTk](https://www.mmtk.io/status), [mmtk-julia](https://github.com/mmtk/mmtk-julia)) |
  | generational copying | yes | simple and fast allocation; needs write barriers |
  | OCaml 5 | minor heap copying, major mark-sweep | one minor heap per domain |
  | Go | no; concurrent tri-color mark-sweep | low pause, no compaction |

  **Recommendation (low-medium):** start with a non-moving or sticky-Immix
  collector through MMTk, since hd instances run on one thread
  ([`req.schedule.one-thread`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.one-thread))
  and need no concurrent collector. Keep stack maps from day one so a
  moving collector stays possible.
- **Async I/O.** The same start-and-poll host ABI (Q12) over epoll or
  kqueue through a Rust reactor; io_uring later on Linux.
- **What carries over unchanged:** the MIR, monomorphization, layout
  classes (a `ref` becomes a pointer), the suspension state machines and
  frames, the host ABI description and its Rust providers, and the
  conformance suite. What is new: object layout in memory, write barriers,
  stack maps, and a linker for native executables.

## Q16: Back-Half Crates And Build Order

### Crates

Continuing Part 1's [layered crates](#proposed-crates):

| Crate | Holds | Depends on | Browser |
| --- | --- | --- | --- |
| `hd_mir` | the structured MIR, lowering from checked bodies, the suspension transform, MIR serialization into the module cache entry | `hd_check` | yes |
| `hd_opt` | MIR passes: counted loops, inlining, scalar replacement, iterator fusion (release tier) | `hd_mir` | yes |
| `hd_mono` | reachability, instantiation, layout classes, the instance key | `hd_mir` | yes |
| `hd_host_abi` | the import list: names, traits, codecs, wait flags; generates the JS glue | `hd_base` | yes |
| `hd_wasm` | Wasm GC emission with `wasm-encoder`, the codegen cache, the linker, folding, name section, site table, source map | `hd_mono`, `hd_host_abi` | yes |
| `hd_run` | the `Runner` interface: instantiate, poll, wake, limits, backtrace to sites | `hd_wasm` | interface only |
| `hd_run_wasmtime` | wasmtime embedding, providers, grants, the reactor, epoch timer, compile caches | `hd_run`, wasmtime | no |
| `hd_web` (extended) | the JS host: providers, grants, the wake loop, `TextDecoder` boundary | `hd_run`, generated glue | only there |
| `hd_cli` | `run`, `test`, `build`, `hd FILE.wasm` | all native crates | no |

wasmtime sits only in `hd_run_wasmtime`, so no front-half or back-half
crate rebuilds it, and the browser build never sees it.

### Build Order

Part 1 ended at slice 5. The back half adds:

6. **Slice 6, scalars end to end.** MIR, monomorphization and emission for
   integers, floats, bools, functions and `println`, run on wasmtime.
   Exit: the scalar cases of `runtime/valid` pass; `size-startup-heap`'s
   tiny program is under 2 KB; `hd` binary size and Cranelift compile time
   are measured and recorded.
7. **Slice 7, data and the std runtime.** `data`, enums, closures, strings,
   lists, maps, `Option` and `Result` layouts, panics with categories and
   sites. Exit: `runtime/valid` and the 99 `runtime/panic` cases chapter by
   chapter, with a known-failures list; `runtime` and `allocations` run.
8. **Slice 8, suspension and the host.** State machines, `all!`, `race!`,
   cancellation, the reactor, capabilities and startup refusal, `hd test`.
   Exit: the suspension and capability cases, the CLI cases of
   [`cli-cases.tsv`](../../spec/conformance/cli-cases.tsv), and
   `suspension-overhead` and `host-call-overhead`.
9. **Slice 9, the same Wasm in the browser.** `hd_web` builds and runs a
   program on V8 through the generated JS host. A browser adapter runs the
   portable runtime cases in a headless browser, as the playground's
   [e2e test](../../website/playground/e2e.ts) drives a page today.
   Exit: the runtime cases pass on wasmtime and in the browser with the
   same known-failures list, which is the Day 1 cross-backend rule.
10. **Slice 10, the optimizing passes.** `hd_opt` against the pillar 3
    metrics: `runtime`, `serde-throughput`, `text-throughput`, `dead-code`.

## Comparison With Vx

The owner asked for a comparison with the
[Vx architecture summary](https://github.com/vx-lang/Vx/blob/main/docs/architecture_executive_summary.md):
a Rust compiler with flat HIR and type streams, 256-bit hashed global IDs
(module hash, symbol hash, generic context, flags), eight phases from
parallel parsing to zero-copy `.vxm` metadata, a frozen `GlobalSession`
with private per-worker state, and a cross-thread merge per epoch. It is a
proposal without measurements, and it does not cover incremental
compilation, backends, GC or Wasm.

**Where it agrees.** Rust, data-oriented flat arrays, parallel parsing,
frozen shared tables during body checking (Part 1's Q3), a deterministic
hash for identity, and zero-copy metadata (Part 1's interface blobs). Its
phases 1 to 3 are Part 1's task graph with generics deferred, which is
what Q8 does too: bodies record instantiation requests, and the link step
instantiates.

**(a) 256-bit hashed IDs everywhere, or 32-bit IDs in memory.**

- Size: a 32-byte ID is eight times a `u32`. Type and HIR streams are
  mostly references, so every cache line holds an eighth as many. That
  works against the `resources` target (10k lines in 50 MB).
- Collisions: Vx calls words 0 and 1 "a composite 128-bit cryptographic
  hash", but they hash different inputs (module path, then symbol name).
  Two symbols of one module collide when their 64-bit name hashes do, so
  the strength inside a module is 64 bits, not 128. rustc's `DefPathHash`
  has the same split (crate ID plus a 64-bit local hash) and therefore
  checks every crate exhaustively for collisions and aborts on one
  ([rustc docs](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefPathHash.html)).
- Determinism: hashed IDs do not depend on thread order, which is their
  real advantage. Part 1 reaches the same result by never printing,
  sorting or hashing an interned ID.
- **Recommendation (medium-high):** keep Part 1's 32-bit interned IDs in
  memory, and use a 128-bit stable hash of the stable path only at the
  boundaries: interface blobs, cache keys, instance keys and program
  database symbols. Check collisions per module when an interface is
  written, as rustc does. This is rustc's `DefId` plus `DefPathHash`.

**(b) Per-worker interning and a merge, or a sharded interner.**

- Body checking creates mostly types that die with the body, and Part 1
  already frees them with the body's arena. Only instantiation requests
  outlive a body, and under Q8 they are recorded in the MIR by stable type
  descriptions, not interned on the spot.
- **Recommendation (medium):** Vx's model for bodies (private state,
  nothing shared to merge), and Part 1's sharded table only for the one
  shared structure left: the instance table of the link step. An epoch
  merge with a SIMD index-patching pass solves a problem this design does
  not have.

**(c) Deduplicating monomorphized instances by hash.**

- Vx routes instances to their origin module's bucket and deduplicates
  with sort and dedup. hd links whole programs, so an instance belongs to
  no module; it belongs to the content-addressed codegen cache (Q8), keyed
  by a hash of the item's MIR and its type arguments.
- **Recommendation (medium-high):** dedup twice by hash, once by instance
  key before emission and once by emitted body for layout folding. Both
  keys are stable across runs, so the cache is shared by `main`, tests and
  worktrees, which per-origin buckets would not give.

**(d) Zero-copy metadata.**

- `bytemuck` casts only plain-old-data slices: no `Vec`, no strings, no
  offsets. `rkyv` archives nested data with relative offsets and can
  validate untrusted bytes with `bytecheck` ([rkyv](https://rkyv.org/)).
  Both need aligned buffers.
- In the browser, IndexedDB returns an `ArrayBuffer` that is copied once
  into linear memory, then read in place. The embedded std blob is an
  aligned `include_bytes!`. Wasm, x86-64 and ARM64 are all little-endian,
  so one format serves every build.
- **Recommendation (medium):** Part 1's own indexed format (flat tables of
  `u32` offsets, `bytemuck`-cast where the data is plain) for interface
  blobs and MIR, validated on load, since cache files can be truncated or
  written by another compiler build. `rkyv` is the fallback if writing the
  format by hand costs too much. The compiler build ID is already in every
  cache key, so a format change never reads stale bytes.

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
- Go generics implementation (GC shapes, dictionaries): <https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md>
- Generics can make your Go code slower: <https://planetscale.com/blog/generics-can-make-your-go-code-slower>
- .NET shared generics: <https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/shared-generics.md>
- Swift cross-module optimization: <https://forums.swift.org/t/brave-new-world-best-practices-for-cross-module-optimization/66869>
- Compiling Swift Generics: <https://download.swift.org/docs/assets/generics.pdf>
- Rust, remove polymorphization: <https://github.com/rust-lang/rust/pull/133883>
- MLton monomorphisation: <http://mlton.org/Monomorphise>
- V8 speculative Wasm optimizations: <https://v8.dev/blog/wasm-speculative-optimizations>
- A function inliner for Wasmtime and Cranelift: <https://fitzgen.com/2025/11/19/inliner.html>
- Beyond Relooper: <https://dl.acm.org/doi/10.1145/3547621>
- Wasm linking conventions: <https://github.com/WebAssembly/tool-conventions/blob/main/Linking.md>
- How async/await really works in C#: <https://devblogs.microsoft.com/dotnet/how-async-await-really-works/>
- Kotlin coroutines design: <https://github.com/Kotlin/KEEP/blob/master/proposals/coroutines.md>
- Wasmtime proposal status: <https://docs.wasmtime.dev/stability-wasm-proposals.html>
- Stack switching and JSPI status: <https://github.com/theSherwood/temen/issues/1651>
- JSPI explained: <https://blog.openreplay.com/jspi-javascript-wasm-bridge/>
- wasm_of_ocaml README: <https://github.com/ocsigen/js_of_ocaml/blob/master/README_wasm_of_ocaml.md>
- Component Model concurrency: <https://github.com/WebAssembly/component-model/blob/main/design/mvp/Concurrency.md>
- Wasm GC in the canonical ABI, pre-proposal: <https://github.com/WebAssembly/component-model/issues/525>
- The road to Component Model 1.0: <https://bytecodealliance.org/articles/the-road-to-component-model-1-0>
- V8, porting GC languages to WasmGC: <https://v8.dev/blog/wasm-gc-porting>
- dart2wasm and JS string builtins: <https://github.com/flutter/engine/pull/51488>
- Scala.js 1.19: <https://www.scala-js.org/news/2025/04/21/announcing-scalajs-1.19.0/>
- MoonBit and wasm-opt: <https://discuss.moonbitlang.com/t/support-for-binaryens-wasm-opt/209>
- wasm-encoder: <https://docs.rs/wasm-encoder>
- Wasmtime collectors: <https://docs.wasmtime.dev/api/wasmtime/enum.Collector.html>
- Wasmtime `Config`: <https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html>
- Wasmtime pre-compiling: <https://docs.wasmtime.dev/examples-pre-compiling-wasm.html>
- V8 dynamic tiering: <https://v8.dev/blog/wasm-dynamic-tiering>
- V8 Wasm code caching: <https://v8.dev/blog/wasm-code-caching>
- Wasmtime tutorial (CLI size claim): <https://wasmruntime.com/en/tutorials/wasmtime>
- Safari 26.6 WebKit features: <https://webkit.org/blog/18178/webkit-features-for-safari-26-6/>
- JS text encoding builtins issue: <https://github.com/WebAssembly/design/issues/1583>
- Flutter `run --wasm` and hot reload: <https://github.com/flutter/flutter/issues/190777>
- New stack maps for Wasmtime and Cranelift: <https://bytecodealliance.org/articles/new-stack-maps-for-wasmtime>
- MMTk status: <https://www.mmtk.io/status>
- MMTk Julia binding: <https://github.com/mmtk/mmtk-julia>
- Vx architecture summary: <https://github.com/vx-lang/Vx/blob/main/docs/architecture_executive_summary.md>
- rustc `DefPathHash`: <https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefPathHash.html>
- rkyv: <https://rkyv.org/>
- V8 preparser and lazy parsing: <https://v8.dev/blog/preparser>
- V8 preparser speed (secondary): <https://dev.to/scmmishra/over-explained-javascript-and-v8-2cei>
- Go `go/build` import reading: <https://go.dev/src/go/build/read.go>
- TypeScript Language Service API, `preProcessFile`: <https://github.com/microsoft/typescript/wiki/using-the-language-service-api>
- `preProcessFile` and template strings: <https://github.com/Microsoft/TypeScript/issues/30878>
- `preProcessFile` and comments after a template literal type: <https://github.com/microsoft/TypeScript/issues/47597>
- TypeScript 5.5, isolated declarations: <https://devblogs.microsoft.com/typescript/announcing-typescript-5-5/#isolated-declarations>
- oxc transformer and isolated declarations speed: <https://oxc.rs/blog/2024-09-29-transformer-alpha.html>
- Bazel and Java, ijar and header jars: <https://bazel.build/docs/bazel-and-java>
- Chromium, turbine for Java headers: <https://github.com/chromium/chromium/commit/578730be19ac76113cd9da3ff2c2566c2f16fc32>
- ECJ toolchain for Bazel (header jar cost): <https://github.com/salesforce/bazel-jdt-java-toolchain>
- Rust 1.38, pipelined compilation: <https://blog.rust-lang.org/2019/09/26/Rust-1.38.0/>
- Swift, skip non-inlinable function bodies: <https://github.com/swiftlang/swift/pull/20420>
- Zig AstGen: <https://mitchellh.com/zig/astgen>
- Zig Sema: <https://mitchellh.com/zig/sema>
- rules_kotlin compile avoidance: <https://github.com/bazel-contrib/rules_kotlin/blob/master/CompileAvoidance.md>
- Dart outlines and compile-time code: <https://github.com/dart-lang/language/issues/1483>
- Flow types-first: <https://flow.org/blog/2020/05/18/Types-First-A-Scalable-New-Architecture-for-Flow/>
