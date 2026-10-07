# Tiered Compilation And The Pass Architecture

Status: Decided (owner, 2026-10-07). The two pipelines and the pass
architecture are adopted; the questions of section 8.1 are answered
there. Binaryen in release builds is decided after spike T2. The
statements of section 8.2 are applied to the other design files.

Part of the [compiler design](README.md). This file investigates the
owner's direction of 2026-10-07 and proposes a pass architecture and two
pipelines. Statements quoted below from other design files are as they
stood before the decision; section 8.2 lists where each now reads
differently. Proposals marked "mine" come from this study, not from the
owner. Numbers marked "estimate" are not measurements; the spike
experiments in section 7 measure them.

The owner's words (2026-10-07):

> "we have talked about tiered compiler before, so preferably each
> optimization can be a single pass, and we can compose them. this is very
> common. but how about its perf? do not rush to answer, investigate."
>
> "we need very fast dev time compilation that crappy output is allowed,
> and blazingly fast/lean artifact for release build, which can be slow."

## Summary

**The premise that changes.** The current design has one emission for
both tiers. Debug and release differ only in checks, because the
`release-check-cost` metric asks that debug run at most 1.3x slower than
release ([goals.md](goals.md), Pillar 1). Cranelift runs at `Speed` in
both tiers, folding runs in both, and there is no Binaryen. The owner now
wants a dev tier that compiles as fast as possible with poor code, and a
release tier that is slow to build but lean and fast. This study accepts
that direction and works out its cost.

**Composable passes are cheap if they don't rewrite the IR.** The cost of
"many small passes" in LLVM comes from rewriting a large, pointer-heavy IR
and recomputing what each rewrite invalidates. LLVM's own compile-time
work names "incremental IR rewriting" as its fundamental cost, and merges
passes that "do nothing" on most functions. Go runs about 51 SSA passes
and stays fast: each pass is linear over a compact, per-function IR, and
rule-based rewriting is fused into a few generated passes. Cranelift fuses
its mid-end rewrites into one e-graph pass, at about 10% of compile time.
Binaryen and MLIR run all passes on one function before moving to the
next, for cache locality. The lesson for hd:

- **Analysis passes are separate, composable units.** They read the
  immutable TIR and write side tables. Nothing invalidates them.
- **Decisions are passes too** (what to inline, what to scalar-replace).
  They write side tables and never rewrite TIR.
- **Local rewrites are fused** into the one emission walk: constant
  folding, dead branches and known-vtable calls.
- **All passes for one instance run back to back** on one worker, before
  the next instance. An instance's TIR (about 1 to 2 KB) stays in L1.

At an estimated 5 to 20 ns per instruction per pass, one analysis pass
over the 10k-line application costs 1 to 5 ms of CPU. Cranelift costs
about 640 ms of CPU on the same program. **Pass count is not where hd's
compile time goes; machine-code generation is.**

**The dev tier.** No optional TIR passes, Cranelift at `None` with the
single-pass register allocator, folding kept, and the same layouts as
release. Estimated cold codegen of the 10k-line application: about
**0.55 CPU-s**, against about 0.9 CPU-s today (−40%). Its code runs an
estimated 2 to 5x slower than release. The warm one-edit loop
(`test-latency`) barely moves, about 130 ms on one core either way. That
loop is dominated by cache lookups, not by code generation.

**The release tier.** Our own TIR passes (bounded inlining, closure
specialization, known-vtable devirtualization, escape analysis and scalar
replacement), then link-level passes (dead-instance removal, folding,
compact encodings), then Cranelift at `Speed`. Estimated **1.0 CPU-s**
for the 10k-line application, about 0.2 s on 8 cores. Binaryen is not in
the first release pipeline. Spike T2 measures what `wasm-opt` adds on
top of our passes. It is embedded only if it buys at least 15% size or
10% speed.

**Divergence.** Every release pass must preserve output, panics with their
sites, the order of effects, `is` and type ids, and termination. Overflow
behaviour already differs by spec. Stack depth, heap-exhaustion points and
backtrace frame lists may differ. Conformance runtime cases run on both
pipelines, and a deterministic bisect counter isolates a miscompiling
decision.

**Tests.** `hd test` stays in the dev tier. The spec ties tests to the
checked profile, so a "release test" must keep the checks. The study
separates the **profile** (checked or wrapping, observable) from the
**pipeline** (dev or optimized, not observable), as Zig's `ReleaseSafe`
does. Decided: `hd test --release` selects the optimized pipeline and
keeps the checks; there is no automatic mid tier.

**`release-check-cost`.** Decided: split in two (question 1):
`check-cost` (checks on against off, the optimized pipeline, ≤ 1.3x) and
`dev-speed` (dev pipeline against optimized, same profile, ≤ 4x geomean,
no case over 10x).

**Statements that must change** are listed in section 8.2. The largest
are codegen.md §12.6 ("one emission for both tiers"), the owner's
2026-10-07 decision that bounded inlining and scalar replacement run "the
same in debug and release builds", and engines §18.6's decision rule.

## 1. Pass Architecture: What Passes Cost

### 1.1 Where Compile Time Goes In Other Compilers

| Compiler | Share of compile time | Source |
| --- | --- | --- |
| Cranelift (wasmtime) | register allocation dominates; regalloc2 cut total compile time by about 20%. The mid-end optimizer is about 10% of compile time, and its e-graph version costs 7 to 8% more than the classic passes it replaced. Wasm-to-CLIF translation is about 9% (derived below) | [Fallin 2022](https://cfallin.org/blog/2022/06/09/cranelift-regalloc2/), [Fallin 2026](https://cfallin.org/blog/2026/04/09/aegraph/), [TPDE](https://arxiv.org/html/2505.22610) |
| LLVM `-O0` back end (x86-64, LLVM 18) | instruction selection is the largest bar, then a fixed "overhead and AsmPrinter" share, register allocation, other passes and IR passes. LLVM 20 cut the back end by 18% on x86-64 | [Engelke, EuroLLVM 2025](https://llvm.org/devmtg/2025-04/slides/technical_talk/engelke_faster.pdf), slide 6 |
| LLVM `-O0` pre-ISel | "15–20 passes to prepare LLVM IR for back-end … for many functions, these do nothing"; "iterating over LLVM-IR is not free → reduce number of passes" | same, slide 7 |
| LLVM in general | "LLVM's fundamental performance problem: incremental IR rewriting. Great for composability, but IR rewriting is expensive"; a separate `-O0` back end could be ">10x" faster | same, slide 15 |
| Go (`cmd/compile`) | 51 SSA passes in Go 1.24, 23 of them required even with optimization off (`-N`). Register allocation is "still the most expensive pass, sometimes taking up to 20%" of the SSA pipeline | [`ssa/compile.go`](https://github.com/golang/go/blob/release-branch.go1.24/src/cmd/compile/internal/ssa/compile.go), [Makarov 2024](https://developers.redhat.com/articles/2024/09/24/go-compiler-register-allocation) |
| Binaryen | on a 440 MB Emscripten module (about 40 MB of code after stripping): `-O0` 108 CPU-s, `-O1` 422 CPU-s, `-O2` 706 CPU-s, `-O3` 543 CPU-s. So `-O1` adds about 8 CPU-s and `-O2` about 15 CPU-s per MB of code (derived) | [binaryen#4619](https://github.com/WebAssembly/binaryen/issues/4619) |
| Binaryen on Dart | dart2wasm's pipeline on one large module: the first `-O3` took 836 s, of which `dae-optimizing` was 361 s; GUFA 31 s; the second `-O3` 410 s | [binaryen#6042](https://github.com/WebAssembly/binaryen/issues/6042) |
| rustc with `rustc_codegen_cranelift` | a debug build 20% faster in wall time and 40% less CPU than LLVM (29.6 s against 37.5 s; 125 against 211 CPU-s) | [LWN 2024](https://lwn.net/Articles/964735/) |
| Zig self-hosted x86-64 back end | "around a 5x decrease compared to LLVM in most cases" for debug builds | [Zig 0.15.1 notes](https://ziglang.org/download/0.15.1/release-notes.html) |

**Derived Cranelift numbers.** The TPDE paper (Fig. 9) compares Wasm
compilers on x86-64, normalized to Cranelift's default configuration.
From its ratios:

| Configuration | Compile time | Run time of the code |
| --- | ---: | ---: |
| Cranelift, backtracking allocator (default) | 1.00 | 1.00 |
| Cranelift, single-pass allocator | 0.63 (4.27 / 2.68) | about 2.15x slower (1.64 × 1.31) |
| TPDE | 0.23 | 1.64x slower |
| Winch | about 0.13 (TPDE is 1.74x slower to compile) | about 1.87x slower (1.64 × 1.14) |

The paper also says 37% of TPDE's Wasm pipeline time is the translation
from Wasm to CLIF. That is about 9% of Cranelift's default compile time.
These are derived from published ratios on linear-memory benchmarks, not
on Wasm GC code. Spike T1 measures hd-shaped code.

### 1.2 What A Pass Costs, Part By Part

| Cost | What drives it | LLVM | Go | Cranelift | hd (proposed) |
| --- | --- | --- | --- | --- | --- |
| IR walk per pass | IR size, pointer chasing | large, pointer-linked instructions with use lists | compact `Value` slices per block | arena of entities | column arrays per body; a walk is a sequential scan |
| analyses recomputed | what each rewrite invalidates | cached by the analysis manager, invalidated by `PreservedAnalyses` | recomputed on demand (dominators, loops), cheap at Go's function sizes | computed once per function | none invalidated: TIR is immutable; facts are side tables |
| memory traffic | IR copies, rewrites, allocation | rewrites allocate; "bump allocator … improved spatial locality" | per-function, reused caches | per function, reused contexts | per-instance arena, reset per instance |
| locality | how often each function is revisited | the new pass manager nests function passes per function | per function | per function | all passes for one instance run back to back |
| pass-manager overhead | dispatch, timers, verification | "timers are not free even if disabled"; passes that do nothing "still have a small cost" | a loop over a static table | none: a fixed sequence | a static table; a gate bit per pass skips an instance |
| phase ordering | passes that enable each other | repeated pass groups | `opt` and `lower` are generated rule sets, fused | e-graph applies all rewrites at once | decision passes in a fixed order; rewrites fused in emission |
| super-linear risk | fixed points, quadratic analyses, large functions | known per pass | known per pass | backtracking regalloc on large functions | fuel per pass per instance (section 6.5) |

### 1.3 When Fusing Pays

Fusing several passes into one walk pays when:

1. **Each pass does little work per node.** LLVM's pre-ISel passes "do
   nothing" on most functions, yet each costs a full walk. Fusion, or a
   cheap gate that skips the walk, removes that cost.
2. **The passes enable each other (phase ordering).** Constant folding
   exposes dead branches, which expose more folding. Cranelift's e-graph
   applies all rewrites as each node is created. Separately, "we need to
   run one full pass of RLE, then one full pass of GVN, then one
   additional full pass of RLE" ([Fallin 2026](https://cfallin.org/blog/2026/04/09/aegraph/)).
3. **The IR is expensive to rewrite.** Fusing writes the result once.

Separate passes are fine when:

1. **They are analyses over an immutable IR.** Nothing is invalidated,
   so order is just a dependency graph.
2. **They run nested per unit.** MLIR nests pass managers so the compiler
   is "only touching a single function at a time", which improves cache
   behaviour and cuts scheduling jobs. Binaryen does the same: "for
   locality it is better to run as many passes as possible on a single
   function before moving to the next" (`pass.cpp`).
3. **They are gated.** A pass that cannot apply to an instance (no loop,
   no closure, no allocation) is skipped by a summary bit, at the cost of
   one test.
4. **Fixed-point iteration is not needed.** GlobalISel made its combiners
   single-pass at `-O0`, because "fixed-point iteration [is] often not
   really beneficial" (Engelke, slide 10).

**For hd (mine):** analyses and decisions are separate passes; local
rewrites are fused into emission; passes are nested per instance; every
pass has a gate. That is the shape of Go's and Cranelift's fast paths,
not of LLVM's rewriting pipeline.

### 1.4 What This Costs On The 10k-Line Application

The model of [representation-compile.md](representation-compile.md)
gives about 4,800 instances, about 50 TIR instructions each, so about
240,000 instruction visits per pass over the program. The systems review
counts about 70,000 TIR instructions in the generic bodies.

| Work | Estimate, one core | Basis |
| --- | ---: | --- |
| one gated analysis pass (liveness, escape, use counts) | 1 to 5 ms | 5 to 20 ns per instruction visit, sequential columns (estimate) |
| pass-manager dispatch | under 1 ms per pass | 4,800 instances × 50 to 100 ns |
| six release TIR passes, with inlining growing visits 1.3x | 10 to 40 ms | the two rows above |
| emission (substitute, lay out, encode) | about 250 ms | 50 µs per instance (representation-compile.md §4) |
| link, with folding | 10 to 30 ms | with per-program packs (systems review, finding 1) |
| Cranelift at `Speed` | about 640 ms | 1.5 µs per code byte, 417 KB (estimate, representation-compile.md §2.1) |
| `wasm-opt -O2`, if it were run | 3 to 6 s | 8 to 15 CPU-s per MB of code, derived from binaryen#4619; Wasm GC code not measured |

**The finding:** hd's own passes are about 1 to 5% of a build. Emission
is about 25%, and Cranelift is the rest. A dev tier gains by cutting
Cranelift's work and by skipping passes, not by fusing them. A release
tier can afford many more passes of its own. It cannot afford Binaryen
without a measured gain.

## 2. The Dev Tier

### 2.1 Goal

The cheapest correct path from TIR to runnable Wasm. "Correct" means
the spec's debug or test profile: every overflow, shift and bounds check
is kept ([`types.arith.checked`](../../spec/lang/04-type-system.md#r-types.arith.checked),
[`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).

### 2.2 Candidates

| Candidate | Compile-time effect | Run-time effect | Divergence risk | Verdict |
| --- | --- | --- | --- | --- |
| no optional TIR passes: no bounded inlining, closure specialization, devirtualization, escape analysis, scalar replacement | emission −10 to 20% on instances with loops, closures or chains; no inliner re-emission after a callee edit | chains keep a `call_ref` and an allocation per adapter: 1.5 to 3x on chain-heavy loops (estimate) | none: these passes only remove work | **yes** |
| trivial inlining (callee ≤ 8 instructions, no loop) | fewer instances and call sequences: −15 to 25% of emitted instances if never-standalone callees are dropped (representation-compile change 2); but callers re-emit when a tiny callee changes | small gain | none | **measure (T4)**; default on |
| keep required lowering: counted loops, liveness for suspension, checks, `multi` layouts | none to remove: these are lowering rules, not optimizations | n/a | dropping them would change layouts or allocations | **yes, keep** |
| constant folding and dead branches in the emission walk | about free; fewer bytes for Cranelift | small gain | only if folding is wrong; the same code runs in release | **keep, fused** |
| folding byte-identical bodies | costs under 1 ms; saves 3% (exact types) to 15 to 25% (proposal A) of Cranelift work | none | none: folding is checked by fold key | **keep** |
| no folding | saves under 1 ms | none | none | no: it costs more Cranelift time than it saves |
| `eqref`-shared generic bodies everywhere, dev only (Go's GC shapes) | maybe −30 to 50% instances (estimate) | casts per read | a second lowering of every generic body; layouts differ by tier | **no for the first release**; proposal A1 at collection already shares move-only bodies in both tiers |
| skip LEB compaction, keep padded LEBs | saves a link pass | none | none | **yes** (already decided) |
| Cranelift `OptLevel::None` | −10% (the mid-end's share) | 0 to 15% slower (estimate) | none | **yes** |
| Cranelift single-pass register allocator | about −37% of Cranelift time | about 2x slower code (derived, section 1.1) | none if correct, but it is a tier-3 feature with recent bugs (below) | **yes, if T1 passes** |
| Winch | about 7x faster than Cranelift | about 1.9x slower | none | **not available**: no GC |
| per-function lazy compilation in wasmtime | would compile only what runs | none | none | **not available** (section 2.4) |

**The single-pass allocator's history.** It shipped in Wasmtime 28 as an
allocator "designed for compile-time performance". It was disabled in
April 2025 because exception handling made call sites define an
unbounded number of values ([wasmtime#10554](https://github.com/bytecodealliance/wasmtime/pull/10554)).
A fuzz bug reported `TooManyLiveRegs` ([wasmtime#11544](https://github.com/bytecodealliance/wasmtime/issues/11544)),
and a segfault with exceptions was fixed in November 2025 through
regalloc2 0.13.3 ([wasmtime#12027](https://github.com/bytecodealliance/wasmtime/pull/12027)).
hd does not use exceptions, but the allocator's support tier is low.
Spike T1 runs the full runtime conformance suite with it, and the dev
tier falls back to the backtracking allocator on any failure.

### 2.3 Winch

Winch is wasmtime's baseline compiler. The current proposal-status page
says, for `gc`, `function-references`, `exception-handling` and
`tail-call`: "Winch does not yet support this proposal at Tier 1"
([wasmtime proposals](https://docs.wasmtime.dev/stability-wasm-proposals.html)).
Winch handles reference values such as `externref` through locals and
calls, but not struct or array types. Every hd program uses GC structs.
**Winch stays out**, as the research found
([Q11](research.md#q11-engines-and-tiers)). Revisit when the page lists
GC for Winch: it would cut dev Cranelift time by about 7x.

### 2.4 Lazy Compilation

| Engine | Lazy per-function compilation | Source |
| --- | --- | --- |
| wasmtime | no: "all functions within a module are validated and compiled in parallel"; "modules are either entirely compiled with Winch or Cranelift" | [architecture](https://docs.wasmtime.dev/contributing-architecture.html), [Pulley docs](https://docs.wasmtime.dev/examples-pulley.html) |
| V8 | yes: Liftoff compiles each function on its first call; hot functions tier up to TurboFan in the background | [V8 pipeline](https://v8.dev/docs/wasm-compilation-pipeline) |

hd already has the coarse form of lazy compilation: **collection compiles
only what the roots reach**. Filtered test programs (representation-compile
change 4) narrow the roots to the selected cases. Real per-function
laziness on wasmtime would need split modules linked by imports, which
representation-compile §7.1 keeps as a fallback. Spike T3 measures what
fraction of compiled functions a test run executes. That fraction bounds
what laziness could save.

In the browser, V8 is already lazy and tiered, so the dev tier there is
our dev pipeline plus Liftoff.

### 2.5 Estimated Dev Compile Time

**Cold codegen of the 10k-line application** (`hd run` after a cold
check; check time not included):

| Step | Current design | Dev tier (proposed) |
| --- | ---: | ---: |
| collect | 5 ms | 5 ms |
| emission (4,800 instances) | 250 ms | 200 ms (no analyses or inliner work; estimate) |
| link with folding | 20 ms | 20 ms |
| Cranelift | 640 ms (`Speed`) | 340 ms (`None` + single-pass: 0.53 of `Speed`, derived) |
| **total CPU** | **about 0.9 s** | **about 0.55 s** |
| wall time, 8 cores | about 150 ms | about 90 ms |
| wall time, 1 core | about 0.9 s | about 0.55 s |

**`test-latency`** (edit, then `hd test --filter one`), from
representation-compile §2.4 with its changes 1 to 4:

| Step | Current design, 1 core | Dev tier, 1 core |
| --- | ---: | ---: |
| process start, check the edited module | 70 ms | 70 ms |
| collect, emit, link | 7 ms | 6 ms (no inliners to re-emit) |
| Cranelift cache lookups (translate and hash every function) | 45 ms | 45 ms |
| Cranelift real misses | 3 ms | 2 ms |
| assemble, load, run one case | 11 ms | 11 ms |
| **total** | **about 135 ms** | **about 135 ms** |

**The finding:** the dev tier saves about 40% of a cold build, of a wide
edit (a `data` type that many instances lay out), and of a std edit that
misses every code entry. It saves almost nothing on a one-function edit,
because the warm loop is cache lookups. The levers there remain the ones
the representation study ranked: stable numbering, packs and filtered
test programs. Translation to CLIF for a cache hit is about 9% of a
compile by the TPDE breakdown, below the 25% the study assumed. Spike S3
measures it.

**Cold `hd test` on 100 test programs** (systems review, finding 2): about
38 CPU-s of Cranelift at `Speed` without sharing. The dev tier cuts that
to about 20 CPU-s. The systems review's "one test program per package"
cuts it far more. The two compose.

### 2.6 Std In The Dev Tier (Later Option)

Cargo lets a dev build optimize its dependencies
(`[profile.dev.package."*"] opt-level = 2`, [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)).
The hd equivalent (mine): std's non-generic bodies are emitted with the
optimized pipeline in every tier, with checks on. They are cached once per
toolchain, and Cranelift's per-function cache shares them across
programs, so the warm cost is zero. Generic std bodies instantiated at
user types stay in the dev pipeline. This needs no new semantics, since
checks stay on. It is a later option, measured after T1.

## 3. The Release Tier

### 3.1 Our Own TIR Passes

| Pass | What it buys | Source of the estimate |
| --- | --- | --- |
| bounded inlining | a call per small callee; enables the rest | representation-runtime.md §9.2 budgets |
| closure specialization | an iterator chain becomes one loop: no adapter allocation, no `call_ref` per element | representation-runtime.md §8.1; chains "near 1x" Node |
| known-vtable devirtualization | one `call_ref` per call through a known trait value, such as `mut dyn Hasher` | representation-runtime.md §7.2: map lookups from about 5x to 1.5 to 2x Node, with the H1 `Hasher` change |
| escape analysis and scalar replacement | an allocation per non-escaping closure, cell or small value | goals.md `allocations` (0 per counted loop, ≤ 1 per chain) |
| constant folding, dead branches | fewer bytes, fewer checks | fused in emission |
| dead-code removal | an instance every caller inlined is dropped at link | codegen.md §13.10 step 1 |

These are the passes the lowering design already names. In the current
design they run in both tiers. In the proposed design they run only in
the optimized pipeline.

### 3.2 Binaryen's Wasm GC Optimizations

What Binaryen offers a GC language: escape analysis that moves allocations
to locals (`heap2local`), devirtualization, global dead-code removal,
GUFA (whole-program, type-aware content flow), cast removal, type pruning,
merging and refining ([V8 2023](https://v8.dev/blog/wasm-gc-porting),
[GC guidebook](https://github.com/WebAssembly/binaryen/wiki/GC-Optimization-Guidebook)).

**Published gains:**

| Toolchain | Gain | Note |
| --- | --- | --- |
| J2Wasm | `wasm-opt` made Java benchmarks "1.9× faster" on average | the front end erases types and emits virtual calls; Binaryen recovers them |
| Google Sheets (J2Wasm) | speculative inlining and devirtualization "sped up calculation time by roughly 40%" | those are V8 optimizations, not Binaryen ones ([web.dev](https://web.dev/case-studies/google-sheets-wasmgc)) |
| dart2wasm | pipeline `--closed-world -tnh --type-unfinalizing -O3 --type-ssa --gufa -O3 --type-merging -O1 --type-finalizing`, with `--inlining-limit 10` | no published before-and-after numbers found ([dart2wasm script](https://github.com/dart-lang/sdk/blob/f36c1094710bd51f643fb4bc84d5de4bfc5d11f3/sdk/bin/dart2wasm)) |
| Scala.js (no Binaryen) | its own optimizer: Wasm 15% faster than its JS output | research.md Q10 |
| MoonBit (no Binaryen) | its own whole-program optimizer; very small output | research.md Q10 |

**Why the gain should be smaller for hd.** Most of Binaryen's GC passes
recover types and call targets that an erasing front end threw away. hd
monomorphizes with exact types, direct calls and few casts (research.md
Q10). What is left for Binaryen: `heap2local`, inlining, and local
cleanups (`simplify-locals`, `coalesce-locals`, `vacuum`,
`merge-blocks`, `code-folding`). On wasmtime, Cranelift already does
local value numbering and constant folding. On V8, TurboFan does. So the
expected speed gain on top of our own passes is **0 to 15%** and the size
gain **10 to 25%** (estimates; nobody has published numbers for
monomorphized Wasm GC code). Spike T2 measures both.

**What Binaryen would break in hd** (mine):

| Binaryen feature | Problem for hd | Rule |
| --- | --- | --- |
| `--traps-never-happen` (`-tnh`) | lets the optimizer "remove code on paths leading to traps" ([cookbook](https://github.com/WebAssembly/binaryen/wiki/Optimizer-Cookbook)). An hd panic is a trap after two `global.set`s, and panics are observable | **never** pass `-tnh` or `--ignore-implicit-traps` |
| code motion | `hd.lines` and engine-trap mapping use code offsets (wasm-layout.md §15.5). `wasm-opt` rewrites the code section and does not update hd's custom sections | carry lines through a source map (`--input-source-map`, `--output-source-map`), or drop the Binaryen pass |
| `--closed-world` | assumes nothing outside the module inspects GC references | holds: hd's host ABI passes scalars and an exchange buffer in linear memory (runtime-and-host.md §17) |
| type merging | may merge Wasm types | holds only if `is` and type ids never depend on Wasm type identity; structural canonicalization already requires that (codegen.md §13.7) |
| whole-module passes | no per-function caching; minutes on large inputs (binaryen#6042) | acceptable for release only, keyed by the link key |
| determinism | Binaryen runs passes on all cores | verify byte-identical output across thread counts in T2 |

### 3.3 Embedding Binaryen, Or Not

| Option | Native `hd` cost | Browser | Gain |
| --- | --- | --- | --- |
| none (first release) | none | none | our passes only |
| external `wasm-opt` on the user's `PATH`, behind a flag | none | none | Binaryen's gain where installed; output depends on the installed version |
| embedded through the `wasm-opt` Rust crate | a C++17 compiler in every native build; build time and size not published (estimate: minutes of C++ build, several MB of binary); the crate bundles Binaryen 116, older than current releases | never shipped | Binaryen's gain, pinned |
| embedded, current Binaryen vendored by us | as above, plus our own build glue | never shipped | as above |
| our own Wasm-level passes for what T2 shows matters | Rust code we own | can ship | targeted |

**Recommendation (mine):** no Binaryen in the first release. Run T2 once
our release passes exist (slice 10). If Binaryen buys ≥ 15% size or
≥ 10% runtime geomean on the `runtime` suite, look at which passes give
the gain. If two or three passes give most of it, write those over our
emitted Wasm. Embed Binaryen only if the gain is spread across many
passes (question 2).

### 3.4 Cranelift And wasmtime In Release

- Cranelift at `Speed` with the backtracking allocator, as today.
- wasmtime's inliner landed in Wasmtime 36, off by default and "still
  baking". It prefers cross-module calls and generally avoids
  intra-module inlining ([inliner](https://bytecodealliance.org/articles/inliner)).
  hd does its own inlining with hd types in view, so the engine inliner is
  a T1 variant, not a plan.
- `OptLevel::SpeedAndSize` exists. It matters only for native code size,
  which hd does not ship. Not used.

### 3.5 Estimated Release Compile Time And Gains

**The 10k-line application, cold:**

| Step | Estimate, CPU |
| --- | ---: |
| collect | 5 ms |
| release TIR passes (section 1.4) | 10 to 40 ms |
| emission, with inlined callees walked | 300 to 350 ms |
| link: drop dead instances, fold, compact LEBs, compact names | 20 to 30 ms |
| Cranelift `Speed` (code bytes within ±10% of today) | about 640 ms |
| **total** | **about 1.0 CPU-s; about 0.2 s on 8 cores** |
| optional `wasm-opt` (not in the first release) | +4 to 11 CPU-s; 1 to 5 s wall |

The 50k-line application scales about 5x: about 5 CPU-s, about 1 s on 8
cores.

**Expected gains of release over dev** (estimates, measured by T1 and
slice 10):

| Measure | Release against dev |
| --- | --- |
| run time, scalar loops | 2 to 2.5x faster (allocator and opt level; checks off) |
| run time, iterator chains and map lookups | 3 to 6x faster (plus closure specialization, devirtualization, scalar replacement) |
| allocations | 0 per counted loop in both; chains ≤ 1 in release, one per adapter in dev |
| Wasm size | 0.6 to 0.75x of dev: compact LEBs, compact names, dropped inlined instances |

## 4. Divergence Between Tiers

### 4.1 What May Differ

| Behaviour | Differs? | Why |
| --- | --- | --- |
| integer overflow, invalid shift | yes, by profile | spec: [`types.arith.checked`](../../spec/lang/04-type-system.md#r-types.arith.checked) and [`types.arith.release`](../../spec/lang/04-type-system.md#r-types.arith.release); the profile decides, not the pipeline |
| closed-handle checks, deadlock frame lists | yes, by profile | debug-only checks (codegen.md §12.6) |
| the depth at which `stack-exhausted` fires | yes, by pipeline | inlining changes frame sizes; the limit is the engine's |
| the point at which `heap-exhausted` fires | yes, by pipeline | scalar replacement removes allocations |
| timing, and so `time-limit` | yes | always |
| backtrace frame lists | yes, by pipeline | inlined frames vanish unless `hd.lines` records inline call sites (mine: record them; a few bytes per inlined site) |
| program output, panic category, panic site, effect order | **never** | the release passes must preserve them |
| `is`, `TypeId`, `Debug` of a type | **never** | layouts are the same in both tiers |
| `MIN / -1`, division by zero, bounds checks | **never** | [`types.arith.always`](../../spec/lang/04-type-system.md#r-types.arith.always) |

### 4.2 Rules For A Release Pass

1. **A possible panic is an effect.** No pass deletes, duplicates or
   reorders a possible panic relative to output or host calls. Dead-code
   removal treats "may panic" as "has an effect".
2. **Evaluation order.** Inlining binds arguments to locals in source
   order before the callee body runs. Closure specialization keeps
   `iterator-invalidated` checks at the same points.
3. **Folding uses Wasm semantics.** Integer folding follows the profile
   (checked or wrapping). A folded operation that would panic emits the
   panic, not a compile error. Float folding is IEEE-exact.
4. **Scalar replacement keeps identity.** A value whose identity is
   observed (`ref.eq`, `is`, a host crossing) escapes.
5. **Sites travel with code.** An inlined instruction keeps its stable
   span; site records already carry it (codegen.md §13.8).

### 4.3 Testing Both Tiers

- **Conformance.** Check-phase fixtures do not depend on the tier. Every
  runtime fixture runs on both pipelines. The `suite-cpu` budget (≤ 60 s)
  grows by the runtime cases only. Outputs must match, except fixtures
  whose expectation names an overflow or shift panic, which run in their
  profile.
- **Differential testing.** The spec's fuzzer generates programs without
  overflow, and the harness compares dev and release output byte for
  byte (mine).
- **Bisecting a miscompile.** A deterministic counter limits how many
  optimization decisions are applied (`HD_OPT_BISECT=N`), like LLVM's
  opt-bisect and Go's `GOSSAHASH`. A bisection over N finds the one
  inlining or replacement that breaks a test. The counter counts decisions
  in content order, so N names the same decision on every run.
- **Dev keeps every check.** Yes, by spec: debug and test builds check
  overflow and shifts, and every build checks bounds and division.

## 5. Tests And Tiers

### 5.1 Prior Art

| Toolchain | Tests run in | Optimized tests | Source |
| --- | --- | --- | --- |
| Rust (Cargo) | the `test` profile, which inherits `dev` (`opt-level = 0`, `overflow-checks = true`) | `cargo test --release` uses the release profile, so overflow checks go off too; per-package `opt-level` overrides | [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) |
| Go | always optimized; there is one tier, fast enough | `-gcflags=-N -l` turns optimization off for debugging | `cmd/compile` |
| Zig | `Debug` by default | `-Doptimize=ReleaseSafe`: optimized, with safety checks kept | Zig build modes |
| Swift | `-Onone` for development | `-O` | [Swift tips](https://github.com/swiftlang/swift/blob/main/docs/OptimizationTips.rst) |

A cautionary number: under `rustc_codegen_cranelift`, an incremental
build got slower than LLVM's (7.98 s against 5.48 s), because the
`serde_derive` proc macro, itself compiled by Cranelift, ran slower
([Rust blog 2020](https://blog.rust-lang.org/inside-rust/2020/11/15/Using-rustc_codegen_cranelift/)).
When the dev loop runs dev-compiled code, slow code costs loop time. In
hd that code is tests.

### 5.2 Which hd Tests Feel Poor Code

| Test kind | Dominated by | Effect of 2 to 5x slower code |
| --- | --- | --- |
| unit tests | instantiation and `hd.init`, about 0.1 ms plus init per case | small |
| property tests, simple generators | host draws and the `Debug` crossing, a few µs per case | maybe 1.5x on cases per second (estimate) |
| property tests over heavy code; tests that sort or parse large inputs | user code | the full 2 to 5x |
| integration tests | file system and process setup | small |

**Break-even (estimate).** Optimizing a test program of 250 KB code adds
about 0.2 CPU-s of Cranelift and emission work. It pays when the
program's tests run more than about 0.3 s of compute.

### 5.3 Recommendation

- `hd test` stays in the dev pipeline with the test profile, as the spec
  says ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
- **Separate the profile from the pipeline** (mine). The profile is
  observable: checked or wrapping. The pipeline is not: dev or optimized.
  `--release` keeps meaning "release profile, optimized pipeline".
- **`hd test --release`** (owner, 2026-10-07; this study proposed a new
  `--optimized` flag): the test profile (checks on) with the optimized
  pipeline, as Zig's `ReleaseSafe`. On `hd build`, `hd run` and `hd FILE`,
  `--release` selects the optimized pipeline and the wrapping release
  profile together
  ([`cli.profile.pipeline.release`](../../spec/cli/command-line.md#r-cli.profile.pipeline.release)).
- **No automatic mid tier.** A tier chosen by heuristics makes test
  timing unpredictable, and a miscompile would appear only sometimes.
  wasmtime cannot tier up at run time anyway.

## 6. A Pass Manager For hd

### 6.1 Units And Levels

| Level | Unit | Runs in | Cached by | Examples |
| --- | --- | --- | --- | --- |
| TIR analysis | instance | `Emit` task, before the walk | inside the `code` entry | liveness, use counts, escape |
| TIR decision | instance | same | same | inline plan, closure specialization, devirtualization targets, scalar-replacement set |
| emission rewrites | instruction, fused | the emission walk | same | constant folding, dead branches, known-vtable calls, checks by profile |
| Wasm module | program | `Link` | the `link` entry | drop dead instances, fold, LEB compaction, compact names, optional external pass |
| engine | function | `Precompile` | `cwasm` and the Cranelift cache | Cranelift level and allocator |

TIR is never rewritten, and no instance IR is materialized. This keeps
the decision of codegen.md §12.1 ("no instance IR exists"). A decision
pass writes a side table. A later pass sees the planned result through an
`InstanceView`, which presents inlined callees in place with remapped
operands. Emission is the only writer.

### 6.2 A Pass Declaration

```rust
pub struct PassSpec {
    pub name: &'static str,
    pub version: u32,                 // bumped on any change of output; part of the pipeline hash
    pub level: Level,                 // TirAnalysis | TirDecision | WasmModule
    pub reads: &'static [FactId],     // facts it needs; the pipeline validator orders by them
    pub writes: &'static [FactId],    // side tables it produces
    pub gate: Gate,                   // summary bits: HAS_LOOP | HAS_CLOSURE | HAS_ALLOC | HAS_SUSPEND | HAS_DYN_CALL | HAS_CALL
    pub required_by: Requirement,     // Always | WhenGate | Optional
    pub run: fn(&mut InstanceCx, &mut Fuel) -> PassOutcome,
}

pub struct Pipeline {
    pub name: &'static str,           // "dev" or "optimized"
    pub tir: &'static [PassId],       // per instance, in order
    pub emit: EmitOptions,            // inline mode, peepholes; checks come from the profile
    pub link: &'static [PassId],      // per program
    pub engine: EngineOptions,        // Cranelift level and allocator
}
```

- **Facts never go stale.** TIR is immutable and every decision is a new
  table. A pass that needs the effect of an earlier decision reads it
  through the view. So there is no invalidation and no
  `PreservedAnalyses`.
- **The validator** checks a pipeline once at startup: each `reads` is
  produced earlier, and every `Always` pass is present. A missing
  suspension liveness pass is a startup error, not a miscompile.
- **Gates** come from per-item summary bits stored in the `tir` entry, so
  a trivial instance skips every optional pass with one test (codegen.md
  §12.1 already says "a trivial instance skips them").
- **Nesting.** All TIR passes for one instance run back to back on one
  worker, then the emission walk, then the arena resets.

### 6.3 Pipelines As Data

```text
dev:
  tir:    liveness(when HAS_SUSPEND)
  emit:   inline=trivial (T4 decides), peepholes=fold+dead-branch, checks=profile
  link:   fold
  engine: cranelift opt=none, regalloc=single_pass (T1 decides; fallback backtracking)

optimized:
  tir:    use_counts, inline_plan(budget), closure_spec, devirt_known,
          escape, scalar_replace, liveness(when HAS_SUSPEND)
  emit:   inline=plan, peepholes=fold+dead-branch+known-vtable, checks=profile
  link:   drop_dead, fold, leb_compact, compact_names
  engine: cranelift opt=speed, regalloc=backtracking
```

`--release` on `hd build`, `hd run` and `hd FILE` selects the release
profile and the optimized pipeline. `hd test --release` selects the test
profile and the optimized pipeline.

### 6.4 Keys And Determinism

- **The pipeline hash** replaces the tier in every key (mine):
  `pipeline_hash = H(name, each pass's name, version and parameters, emit
  options, engine options)`. The profile stays a separate key part.
  `code_key` (codegen.md §13.8) and `prog_key` (§11.3) take both. A pass
  version bump misses only the pipeline that holds the pass.
- **Per-instance passes read only what the code key covers.** A decision
  that needs whole-program facts (for example, "this trait has one impl in
  the program") cannot run per instance, because the code entry is shared
  across programs. It runs as a `WasmModule` pass in `Link`, keyed by the
  link key.
- **Deterministic output.** Passes iterate in instruction order, never
  over hash maps. Parallelism is across instances only, and output is
  content-ordered (§6.5 of the scheduler design). No pass reads time,
  thread count or addresses.

### 6.5 Fuel And Limits

| Pass | Fuel |
| --- | --- |
| inline plan | the caller-local budget of representation-runtime.md §9.2: callee size, caller size so far (cap 2x or 2 KB), depth |
| closure specialization | ≤ 4 rounds; ≤ 512 bytes of growth per chain site |
| liveness | rounds ≤ loop depth + 1 per loop; the dataflow is monotone, so this is exact for structured loops |
| escape analysis | a node cap per instance; past it, every value escapes |
| link fold | rounds bounded by call depth; a worklist after round one |

Running out of fuel gives the pass's conservative answer (no inlining, the
value escapes), never an error. Fuel counts steps, never time, so the
output stays deterministic. `hd build --stats` reports per pass: instances
visited, gated out, decisions made, fuel exhaustions, and time. Time
appears only in the report, never in a key.

### 6.6 Where The Named Passes Plug In

| Pass | Level | Reads | Writes | Gate | Pipeline |
| --- | --- | --- | --- | --- | --- |
| `liveness` | analysis | TIR (through the view) | live sets per block | `HAS_SUSPEND` (required); `HAS_LOOP` when scalar replacement runs | both |
| `use_counts` | analysis | TIR | uses per value | any | optimized |
| `inline_plan` | decision | TIR, callees' inline summaries and TIR | inline set, operand remap | `HAS_CALL` | optimized (trivial mode in dev) |
| `closure_spec` | decision | inline set, closure literals passed to known callees | extended inline set | `HAS_CLOSURE` | optimized |
| `devirt_known` | decision | the view, vtable constants | direct targets for `call_ref` sites | `HAS_DYN_CALL` | optimized |
| `escape` | analysis | the view | escape bit per allocation | `HAS_ALLOC`, `HAS_CLOSURE` | optimized |
| `scalar_replace` | decision | escape bits | values to keep in locals | same | optimized |
| folding, dead branches | fused | emission state | Wasm | none | both |
| `drop_dead`, `fold` | module | code entries | module | none | `fold` in both |
| `leb_compact`, `compact_names` | module | module | module | none | optimized |

## 7. Recommended Pipelines And Spike Experiments

### 7.1 The Pipelines

| | Dev (`hd run`, `hd test`, REPL) | Optimized (`--release`, including `hd test --release`) |
| --- | --- | --- |
| profile | debug or test: checks on | release (wrap) or test (checks on) |
| TIR passes | suspension liveness only | the seven of section 6.3 |
| inlining | trivial, if T4 confirms | bounded, with closure specialization |
| layouts | the same as release | the same as dev |
| link | fold | drop dead, fold, compact LEBs and names |
| Wasm-level optimizer | none | none in the first release; T2 decides |
| engine | Cranelift `None`, single-pass allocator (T1) | Cranelift `Speed`, backtracking |
| 10k app, cold codegen | about 0.55 CPU-s | about 1.0 CPU-s |
| 10k app, one-function edit | about 135 ms on one core (`test-latency`) | not a target |
| run time | 2 to 5x slower than optimized (estimate) | the `runtime` target: ≤ 1.5x Node |
| size | not a target | the `dead-code` and tiny-program targets |

### 7.2 Spike 0c Additions

These join S1 to S7 of [representation-compile.md](representation-compile.md)
and E8 of [representation-runtime.md](representation-runtime.md). Each
runs on the pinned wasmtime; T2 also runs on V8 through Node.

| ID | Program | Metric | Decision rule |
| --- | --- | --- | --- |
| T1 | the S1 module and the 10k-line application model (generated, or the largest hd program available), plus the `runtime` microbenchmarks; compiled at `Speed` + backtracking, `None` + backtracking, `None` + single-pass, and `Speed` + wasmtime's inliner | Cranelift CPU per byte; run time of each benchmark against `Speed`; any crash or `TooManyLiveRegs` over the runtime conformance cases | dev uses `None` + single-pass if it saves ≥ 30% Cranelift CPU, passes every runtime case, and dev stays within the `dev-speed` guard; else `None` + backtracking if that saves ≥ 10%; else `Speed`. The engine inliner joins the release pipeline only if it buys ≥ 5% run time after hd's own inlining |
| T2 | hd release output of four programs: derive-heavy data code, iterator chains, map and `Hasher` code, JSON round trips. Before our release passes exist, hand-emitted modules in the shape the lowering design gives. Then `wasm-opt` with `-O1`, `-O2`, `-Oz`, and dart2wasm's pipeline without `-tnh`, all with `--closed-world` and source maps | CPU per pass (`BINARYEN_PASS_DEBUG` timing) and per MB; bytes per section; run time on wasmtime and V8; byte-identical output across thread counts; panic sites still mapped through the source map | Binaryen joins the release pipeline (question 2) only if it buys ≥ 15% size or ≥ 10% run time geomean over our passes, at ≤ 10 CPU-s for the 10k app. If two or three passes give most of the gain, write those instead |
| T3 | a generated unit-test suite and the 10k application's own tests, with per-function execution counts (a counter per function in an instrumented build) | the share of compiled functions that execute; the share of `hd test` time in Cranelift lookups, misses and execution | if < 30% of functions run and Cranelift is > 50% of cold `hd test` time, raise split modules or per-case roots as an owner question; else no laziness work |
| T4 | the 10k model emitted three ways: no inlining; trivial inlining; trivial inlining with never-standalone callees dropped | instances, code bytes, emission CPU, Cranelift CPU, and instances re-emitted per scripted edit (20 edits) | dev keeps trivial inlining if it cuts emission plus Cranelift CPU by ≥ 5% and adds ≤ 10% re-emitted instances per edit |
| T5 | the release TIR passes over the 10k model, run nested per instance as separate passes, and as one fused walk | ns per instruction per pass; total share of codegen | keep passes separate unless they exceed 10% of emission time; fuse only the passes that do |

## 8. Owner Questions And Required Changes

### 8.1 Open Questions For The Owner

1. **The `release-check-cost` metric.** It compares debug and release on
   one test suite, at ≤ 1.3x. With a deliberately poor dev tier, that
   ratio would be 2 to 5x and the metric would fail by design. Options:
   (a) drop it; (b) relax it to about 4x; (c) **split it**:
   `check-cost`, the optimized pipeline with checks on against checks off
   (≤ 1.3x; this keeps its first purpose, pricing the overflow sequences),
   and `dev-speed`, the dev pipeline against the optimized one in the same
   profile (≤ 4x geomean, no case > 10x; a guard so tests don't crawl).
   **Recommendation: (c).** **Decided (owner, 2026-10-07): (c).**
2. **Binaryen in release builds.** **Recommendation:** not in the first
   release. Decide after T2, once our release passes exist. If it is
   adopted, embed it in the native `hd` only (never the playground), pin
   its version, and never use `-tnh`. **Decided (owner, 2026-10-07):**
   decided after spike T2.
3. **Optimizations only in the optimized pipeline.** The owner decided on
   2026-10-07 that bounded inlining and scalar replacement are "the same
   in debug and release builds, so the `runtime` and `allocations` targets
   can be met". Both targets measure release artifacts, so release-only
   passes still meet them. **Recommendation:** move bounded inlining,
   closure specialization, devirtualization and scalar replacement to the
   optimized pipeline. Keep counted loops, layouts and folding in both.
   **Decided (owner, 2026-10-07):** as recommended.
4. **`hd test --optimized`.** The spec says `hd test` always uses the
   checked test profile and that no other command takes `--release`
   (`cli.profile.flag-only`, since retired).
   **Recommendation:** add `--optimized` to `hd test` and `hd run`: the
   same checks, the optimized pipeline. No automatic mid tier.
   **Decided (owner, 2026-10-07):** no new flag. `hd test --release`
   selects the optimized pipeline and keeps the checks; on `hd build`,
   `hd run` and `hd FILE`, `--release` also selects the wrapping release
   profile. No automatic mid tier.
5. **No instance IR for the optimized pipeline.** The design walks generic
   TIR under a substitution and forbids materializing instance IR. That
   suits decision passes and fused rewrites (section 6). A full rewriting
   optimizer would need an instance IR. **Recommendation:** keep the rule
   for the first release. Reopen it only if T2 shows a large gain from
   passes that need rewriting, such as repeated inlining with cleanup.

### 8.2 Design Statements That Must Change If Adopted

Applied (2026-10-07). Each file now states the decided rule; the dated
study records ([representation-compile.md](representation-compile.md),
[representation-runtime.md](representation-runtime.md)) carry a
"superseded by tiering.md" note instead of a rewrite.

| File | Statement | Change |
| --- | --- | --- |
| [goals.md](goals.md), Pillar 1 table | `release-check-cost`: "debug ≤ 1.3x release" | `check-cost` and `dev-speed` (question 1) |
| [goals.md](goals.md), decided items | bounded inlining and scalar replacement "the same in debug and release builds" | optimized pipeline only (question 3) |
| [goals.md](goals.md), Pillar 1 Later | "The optimizing tier … (optimized Cranelift, later LLVM)" | the optimized pipeline is first release; LLVM stays Later |
| [codegen.md](codegen.md) §12.6 | "D2 keeps one emission for both tiers"; the table's "yes / yes" rows for trivial and bounded inlining and scalar replacement | two pipelines; rows by pipeline |
| [codegen.md](codegen.md) §11.3, §13.8, §13.10 | `tier` in `prog_key`, `code_key` and `link_key` | pipeline hash plus profile |
| [codegen.md](codegen.md) §12.1 | analyses run "when the instance has an allocation, a closure, a loop …" | analyses are passes in a pipeline, gated by summary bits |
| [engines-and-test-runner.md](engines-and-test-runner.md) §18.1 | debug: "Cranelift, `OptLevel::None` to start" | `None` with the single-pass allocator, by T1 |
| [engines-and-test-runner.md](engines-and-test-runner.md) §18.6 | "If debug code at `None` costs more than 1.3x release, debug moves to `Speed`" | T1's rule against `dev-speed` |
| [representation-compile.md](representation-compile.md) §6, change 7, S4 | "the debug tier should probably use Cranelift `Speed`"; inlining and scalar replacement not skipped in debug | superseded by T1 and question 3 |
| [representation-runtime.md](representation-runtime.md) §9.1 | policy 5: "Debug and release run the same optimizer" | layouts stay the same (policy 1); optimizers differ |
| [build-order.md](build-order.md) slice 10 and §22.1 | "Optimization within the shared tier"; the `release-check-cost` row | "the optimized pipeline"; the two new metrics |
| [research.md](research.md) Q10, Q11 | "No Binaryen" (confidence high); "The optimizing tier is ours" | unchanged for the first release; Binaryen becomes a T2 decision |
| [wasm-layout.md](wasm-layout.md) §15.5 | panic and line mapping by code offset | must survive any Wasm-level pass; record inline call sites for backtraces |
| [spec/cli/command-line.md](../../spec/cli/command-line.md) | `cli.profile.flag-only` | retired; replaced by [`cli.profile.release.commands`](../../spec/cli/command-line.md#r-cli.profile.release.commands), [`cli.profile.test.release`](../../spec/cli/command-line.md#r-cli.profile.test.release) and the [Pipelines](../../spec/cli/command-line.md#pipelines) rules |

## Sources

- [TPDE: A Fast Adaptable Compiler Back-End Framework](https://arxiv.org/html/2505.22610) (2025): Fig. 9 compile and run time of Cranelift, Cranelift with the single-pass allocator, Winch and TPDE on Wasm; the share of Wasm-to-CLIF translation.
- [The acyclic e-graph: Cranelift's mid-end optimizer](https://cfallin.org/blog/2026/04/09/aegraph/) (Fallin, 2026): the mid-end is about 10% of compile time; fused rewrites against phase ordering; eclass size 1.13.
- [Cranelift, Part 4: A New Register Allocator](https://cfallin.org/blog/2022/06/09/cranelift-regalloc2/) (Fallin, 2022): compile time "dominated by regalloc time"; regalloc2 cut it about 20%.
- [Cranelift progress in 2022](https://bytecodealliance.org/articles/cranelift-progress-2022) and [Wasmtime 28.0](https://bytecodealliance.org/articles/wasmtime-28.0): the e-graph mid-end; the single-pass allocator introduced.
- [wasmtime#10554](https://github.com/bytecodealliance/wasmtime/pull/10554), [#11544](https://github.com/bytecodealliance/wasmtime/issues/11544), [#11850](https://github.com/bytecodealliance/wasmtime/issues/11850), [#12027](https://github.com/bytecodealliance/wasmtime/pull/12027): the single-pass allocator disabled, its bugs, and the regalloc2 0.13.3 fix.
- [wasmtime `Config`](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html) (51.0.0-dev): `cranelift_regalloc_algorithm`, `cranelift_opt_level`, `enable_incremental_compilation`, `parallel_compilation`.
- [wasmtime Wasm proposals](https://docs.wasmtime.dev/stability-wasm-proposals.html): Winch lacks `gc`, `function-references`, `exception-handling`, `tail-call`.
- [wasmtime architecture](https://docs.wasmtime.dev/contributing-architecture.html) and [Pulley](https://docs.wasmtime.dev/examples-pulley.html): eager parallel compilation; no tiering between Winch and Cranelift.
- [A function inliner for Wasmtime and Cranelift](https://bytecodealliance.org/articles/inliner): Wasmtime 36, off by default, cross-module first.
- [Faster Compilation in LLVM 20 and Beyond](https://llvm.org/devmtg/2025-04/slides/technical_talk/engelke_faster.pdf) (Engelke, EuroLLVM 2025): the `-O0` back-end breakdown, the cost of IR rewriting and of passes that do nothing, single-pass combiners.
- [The new pass manager](https://blog.llvm.org/posts/2021-03-26-the-new-pass-manager/) (LLVM blog, 2021): the analysis manager, caching and `PreservedAnalyses`.
- [MLIR pass management](https://mlir.llvm.org/docs/PassManagement/): lazy cached analyses, nesting per operation for locality and threading, textual pipelines, timing.
- [Go `ssa/compile.go`, release 1.24](https://github.com/golang/go/blob/release-branch.go1.24/src/cmd/compile/internal/ssa/compile.go): 51 passes, 23 required. [Register allocation in the Go compiler](https://developers.redhat.com/articles/2024/09/24/go-compiler-register-allocation) (Makarov, 2024): regalloc up to 20% of the pipeline.
- [Binaryen](https://github.com/WebAssembly/binaryen) and its `src/passes/pass.cpp`: parallel passes stacked per function for locality; passes by optimize level. [GC Optimization Guidebook](https://github.com/WebAssembly/binaryen/wiki/GC-Optimization-Guidebook) and [Optimizer Cookbook](https://github.com/WebAssembly/binaryen/wiki/Optimizer-Cookbook): GC pipelines, `--converge`, `-tnh`, `--closed-world`.
- [binaryen#4619](https://github.com/WebAssembly/binaryen/issues/4619): `wasm-opt` times per level on a 440 MB module. [binaryen#6042](https://github.com/WebAssembly/binaryen/issues/6042): dart2wasm's pipeline with per-pass times.
- [dart2wasm script](https://github.com/dart-lang/sdk/blob/f36c1094710bd51f643fb4bc84d5de4bfc5d11f3/sdk/bin/dart2wasm): its `wasm-opt` flags.
- [`wasm-opt` Rust crate](https://docs.rs/wasm-opt/latest/wasm_opt/): bundles Binaryen's C++, needs a C++17 compiler.
- [A new way to bring garbage collected programming languages efficiently to WebAssembly](https://v8.dev/blog/wasm-gc-porting) (V8, 2023): Binaryen's GC passes; J2Wasm 1.9x from `wasm-opt`.
- [Google Sheets and WasmGC](https://web.dev/case-studies/google-sheets-wasmgc): speculative inlining and devirtualization about 40%.
- [V8's Wasm compilation pipeline](https://v8.dev/docs/wasm-compilation-pipeline), [Sparkplug](https://v8.dev/blog/sparkplug), [Maglev](https://v8.dev/blog/maglev): lazy Liftoff and dynamic tiering; a single-pass non-optimizing JS tier; a mid tier about 10x slower to compile than Sparkplug and 10x faster than TurboFan.
- [Cranelift code generation comes to Rust](https://lwn.net/Articles/964735/) (LWN, 2024) and [Using rustc_codegen_cranelift](https://blog.rust-lang.org/inside-rust/2020/11/15/Using-rustc_codegen_cranelift/) (2020): debug build speedups; slower proc macros under Cranelift.
- [Zig 0.15.1 release notes](https://ziglang.org/download/0.15.1/release-notes.html): the self-hosted x86-64 back end, about 5x faster than LLVM for debug builds; behaviour tests 1984/2008 against LLVM's 1977/2008.
- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html): `test` inherits `dev`; `cargo test --release`; per-package `opt-level`.
- [Swift optimization tips](https://github.com/swiftlang/swift/blob/main/docs/OptimizationTips.rst): `-Onone`, `-O`, `-Osize`, whole-module optimization.
- In this repository: [goals.md](goals.md), [codegen.md](codegen.md) §11 to §13, [engines-and-test-runner.md](engines-and-test-runner.md) §18 to §19, [representation-compile.md](representation-compile.md), [representation-runtime.md](representation-runtime.md) §7 to §9, [systems-review.md](systems-review.md), [research.md](research.md) Q10 and Q11, [wasm-layout.md](wasm-layout.md) §15.5, [build-order.md](build-order.md), and the spec's [build profiles](../../spec/cli/command-line.md#build-profiles).
