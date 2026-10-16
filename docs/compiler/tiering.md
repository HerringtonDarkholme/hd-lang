# Tiered Compilation And The Pass Architecture

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](ARCHITECTURE.md). The pass manager is
built in `crates/hd_opt` ([crates/hd_opt/DESIGN.md](../../crates/hd_opt/DESIGN.md));
this file gives the model behind it and the two pipelines. Numbers marked
"estimate" are not measurements.

The owner's direction:

> "we need very fast dev time compilation that crappy output is allowed,
> and blazingly fast/lean artifact for release build, which can be slow."
>
> "preferably each optimization can be a single pass, and we can compose
> them."

## Summary

- **Two pipelines.** The **dev** pipeline (`hd run`, `hd test`, the
  REPL) runs no optional passes, and compiles with Cranelift
  `OptLevel::None`. The **optimized** pipeline (`--release`) runs hd's
  own passes, then Cranelift `OptLevel::Speed`. Layouts are the same in
  both.
- **Profile and pipeline are separate.** The profile (checked or
  wrapping) is observable; the pipeline (dev or optimized) is not, as in
  Zig's `ReleaseSafe`. `hd test --release` keeps the test profile's checks
  and selects the optimized pipeline; on `hd build`, `hd run` and
  `hd FILE`, `--release` selects the release profile and the optimized
  pipeline together.
- **Passes never rewrite TIR.** Analyses and decisions write side tables
  that collection and emission read. Nothing is invalidated, so pass order
  is only a dependency graph.
- **Pass count is not where compile time goes; machine code is.** One
  linear pass over the 10k-line application costs an estimated 1 to 5 ms;
  Cranelift costs an estimated 640 ms on the same program.

## 1. Pass Architecture: What Passes Cost

### 1.1 Where Compile Time Goes In Other Compilers

| Compiler | Share of compile time | Source |
| --- | --- | --- |
| Cranelift (wasmtime) | register allocation dominates; the mid-end optimizer is about 10%; Wasm-to-CLIF translation about 9% (derived from TPDE) | [Fallin 2022](https://cfallin.org/blog/2022/06/09/cranelift-regalloc2/), [Fallin 2026](https://cfallin.org/blog/2026/04/09/aegraph/), [TPDE](https://arxiv.org/html/2505.22610) |
| LLVM `-O0` | "15–20 passes to prepare LLVM IR for back-end … for many functions, these do nothing"; "LLVM's fundamental performance problem: incremental IR rewriting" | [Engelke, EuroLLVM 2025](https://llvm.org/devmtg/2025-04/slides/technical_talk/engelke_faster.pdf) |
| Go (`cmd/compile`) | 51 SSA passes, each linear over a compact per-function IR; register allocation up to 20% | [`ssa/compile.go`](https://github.com/golang/go/blob/release-branch.go1.24/src/cmd/compile/internal/ssa/compile.go), [Makarov 2024](https://developers.redhat.com/articles/2024/09/24/go-compiler-register-allocation) |
| Binaryen | `-O1` adds about 8 CPU-s and `-O2` about 15 CPU-s per MB of code (derived) | [binaryen#4619](https://github.com/WebAssembly/binaryen/issues/4619) |

From the TPDE paper's Fig. 9, normalized to Cranelift's default
(backtracking allocator): the single-pass allocator compiles in about
0.63 of the time and its code runs about 2.15x slower; Winch compiles in
about 0.13 and runs about 1.87x slower. These are linear-memory
benchmarks, not Wasm GC code.

### 1.3 When Fusing Pays

Fusing passes into one walk pays when each pass does little per node,
when passes enable each other (phase ordering), or when the IR is
expensive to rewrite. Separate passes are fine when they are analyses over
an immutable IR, run nested per unit for locality (MLIR, Binaryen), are
gated so a pass that cannot apply is skipped by one test, and need no
fixed point.

**For hd:** analyses and decisions are separate passes over immutable
TIR, local rewrites belong in the emission walk, and every pass has a
gate. That is the shape of Go's and Cranelift's fast paths, not of LLVM's
rewriting pipeline. A dev tier gains by cutting Cranelift's work, not by
fusing passes; a release tier can afford many passes of its own.

## 2. The Dev Tier

### 2.1 Goal

The cheapest correct path from TIR to runnable Wasm. "Correct" means the
spec's debug or test profile: every overflow, shift and bounds check is
kept ([`types.arith.checked`](../../spec/lang/04-type-system.md#r-types.arith.checked),
[`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).

### 2.2 What It Runs

- No body passes: the dev pipeline's planner plans nothing, so collection
  does no plan work.
- Cranelift `OptLevel::None` with the backtracking register allocator
  (`hd_run_wasmtime::EngineConfig::pipeline`). On V8, Liftoff and tier-up
  already give a fast first tier.
- The same layouts as release, so `is`, `TypeId` and output never depend
  on the pipeline.

Planned, not built: the single-pass register allocator for dev, adopted
only if spike T1 (§7.2) shows it saves at least 30% of Cranelift CPU and
passes every runtime conformance case. It shipped in Wasmtime 28 and had
bugs after ([wasmtime#10554](https://github.com/bytecodealliance/wasmtime/pull/10554),
[#11544](https://github.com/bytecodealliance/wasmtime/issues/11544),
[#12027](https://github.com/bytecodealliance/wasmtime/pull/12027)).

Planned, not built: trivial inlining (callee of at most 8 instructions,
no loop) in dev, if spike T4 shows it saves compile time.

### 2.3 Winch

Winch is wasmtime's baseline compiler, about 7x faster to compile than
Cranelift. It does not support `gc`, `function-references`,
`exception-handling` or `tail-call` ([wasmtime proposals](https://docs.wasmtime.dev/stability-wasm-proposals.html)),
and every hd program uses GC structs, so it cannot be the dev tier.
Revisit when it supports GC.

### 2.4 Lazy Compilation

wasmtime compiles every function of a module eagerly; V8 compiles each
function in Liftoff on its first call. hd's coarse form of laziness is
that collection compiles only what the roots reach, and `--filter` narrows
a test program's roots to the selected cases.

## 3. The Release Tier

### 3.1 hd's Own Passes

Built (optimized pipeline only):

| Pass | Unit | What it buys |
| --- | --- | --- |
| `value_flow` | generic body | the body's data flow, which the next two read |
| `devirt_known` | generic body | a `CallValue` whose closure is made in the same sub-body calls its code directly |
| `closure_spec` | generic body | a small callee that calls a parameter gets one instance per closure literal passed for it, with those calls direct |

The design ran every pass per instance at emission. The code runs these
as body-unit passes over each generic body, before and during collection,
and collection applies their plans ([crates/hd_opt/DESIGN.md](../../crates/hd_opt/DESIGN.md) §2).
This is better: a body is planned once whatever its instance count, an
edit re-plans one body, and specialization makes instances instead of
needing an instance IR.

Declared in the pass table but with no body (they report `Skipped`):
`use_counts`, `inline_plan`, `escape`, `scalar_replace`. Planned, not
built: bounded inlining, escape analysis and scalar replacement, which
the `allocations` target (0 per counted loop, at most 1 per iterator
chain) needs. The link passes `drop_dead` and `leb_compact` are hashed
into the pipeline but never run; planned, not built. Merging
byte-identical functions (`fold`): not built yet; see #175. The compact
`hd.names` section replaces `name` in release builds; the emitter writes
it, not a link pass.

### 3.2 Binaryen

Most of Binaryen's Wasm GC passes recover types and call targets that an
erasing front end threw away. hd monomorphizes with exact types and
direct calls, so the expected gain on top of hd's own passes is small
(estimate: 0 to 15% speed, 10 to 25% size). Binaryen is not in the
pipeline.

If it is ever run, these rules hold:

| Binaryen feature | Rule |
| --- | --- |
| `--traps-never-happen`, `--ignore-implicit-traps` | never: an hd panic is a trap, and panics are observable |
| code motion | `hd.lines` maps panics and backtraces by code offset (crates/hd_wasm/DESIGN.md §15.9), so lines must travel through a source map, or the pass is dropped |
| `--closed-world` | holds: the host ABI passes scalars and an exchange buffer in linear memory |
| type merging | holds only while `is` and type ids never depend on Wasm type identity |
| whole-module passes | release only; no per-function caching |

Planned, not built: spike T2 (§7.2). Binaryen joins the release pipeline
only if it buys at least 15% size or 10% runtime geomean over hd's own
passes. If two or three passes give most of the gain, hd writes those
over its own Wasm instead of embedding Binaryen.

### 3.4 Cranelift In Release

Cranelift `OptLevel::Speed` with the backtracking allocator. wasmtime's
own inliner (off by default, cross-module first) is not used; hd inlines
with hd types in view.

## 4. Divergence Between Tiers

### 4.1 What May Differ

| Behaviour | Differs? | Why |
| --- | --- | --- |
| integer overflow, invalid shift | yes, by profile | spec: [`types.arith.checked`](../../spec/lang/04-type-system.md#r-types.arith.checked) and [`types.arith.release`](../../spec/lang/04-type-system.md#r-types.arith.release); the profile decides, not the pipeline |
| debug-only checks (closed handles, deadlock frame lists) | yes, by profile | crates/hd_mono/DESIGN.md §12.6 |
| the depth at which `stack-exhausted` fires | yes, by pipeline | inlining and specialization change frame sizes |
| the point at which `heap-exhausted` fires | yes, by pipeline | scalar replacement removes allocations |
| timing, and so `time-limit` | yes | always |
| backtrace frame lists | yes, by pipeline | inlined frames vanish unless `hd.lines` records inline call sites |
| program output, panic category, panic site, effect order | **never** | every release pass must preserve them |
| `is`, `TypeId`, `Debug` of a type | **never** | layouts are the same in both pipelines |
| `MIN / -1`, division by zero, bounds checks | **never** | [`types.arith.always`](../../spec/lang/04-type-system.md#r-types.arith.always) |

### 4.2 Rules For A Release Pass

1. **A possible panic is an effect.** No pass deletes, duplicates or
   reorders a possible panic relative to output or host calls.
2. **Evaluation order.** Inlining binds arguments to locals in source
   order before the callee body runs. Closure specialization keeps
   `iterator-invalidated` checks at the same points.
3. **Folding uses Wasm semantics.** Integer folding follows the profile.
   A folded operation that would panic emits the panic, not a compile
   error. Float folding is IEEE-exact.
4. **Scalar replacement keeps identity.** A value whose identity is
   observed (`ref.eq`, `is`, a host crossing) escapes.
5. **Sites travel with code.** An inlined instruction keeps its stable
   span.

### 4.3 Testing Both Tiers

- **Conformance.** Check-phase fixtures do not depend on the tier. Every
  runtime fixture runs on both pipelines, and outputs must match, except
  fixtures whose expectation names an overflow or shift panic, which run
  in their profile. `HD_CONFORMANCE_PIPELINE=optimized` builds the suite
  through the optimized pipeline; CI runs the driver tier once per
  pipeline, the optimized run on both Node and wasmtime.
- **Differential testing.** The spec's fuzzer generates programs without
  overflow, and the harness compares dev and release output byte for
  byte. Planned, not built.
- **Bisecting a miscompile.** A deterministic counter limits how many
  optimization decisions are applied (`HD_OPT_BISECT=N`), like LLVM's
  opt-bisect and Go's `GOSSAHASH`. It counts decisions in content order,
  so N names the same decision on every run. Planned, not built.

## 5. Tests And Tiers

### 5.3 Recommendation

- `hd test` runs in the dev pipeline with the test profile, as the spec
  says ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
- `hd test --release` keeps the test profile's checks and selects the
  optimized pipeline
  ([`cli.profile.test.release`](../../spec/cli/command-line.md#r-cli.profile.test.release)).
- **No automatic mid tier.** A tier chosen by heuristics makes test
  timing unpredictable, and a miscompile would appear only sometimes.
  wasmtime cannot tier up at run time anyway.

Most unit tests are dominated by instantiation and init, so slow code
costs them little. Property tests over heavy code and tests that sort or
parse large inputs feel the full slowdown; `--release` is their escape.

## 6. A Pass Manager For hd

### 6.1 Units

| Level | Unit | Runs in | Examples |
| --- | --- | --- | --- |
| body pass | generic body | collection, through the pipeline's planner | `value_flow`, `devirt_known`, `closure_spec` |
| instance pass | instance | emission | `liveness`, which suspension lowering owns; the declared passes with no body |
| engine | function | the engine | Cranelift level |

TIR is never rewritten and no instance IR is materialized
(crates/hd_mono/DESIGN.md §12.1). A body pass writes body facts
(`ValueFlow`, `KnownCallees`, `ClosureSpec`) that collection reads
through `hd_mono::plan::BodyPlan`.

### 6.2 A Pass Declaration

`hd_opt::passes::PassSpec` holds a pass's name, version, level, the facts
it reads and writes, its gate bits, whether it is required, its run
function (per body or per instance), and its knobs with their allowed
ranges. A `Pipeline` holds its passes in order, its link passes, and a
value for each knob of each pass it holds.

- **Facts never go stale.** TIR is immutable and every decision is a new
  table, so there is no invalidation.
- **The validator** (`validate`) checks each pipeline once per process:
  each read is produced earlier, every required pass is present, a body
  pass reads only body facts, only body passes write them, and every knob
  is known, in range and given once.
- **Gates.** `body_gate` sets `HAS_CALL` and `HAS_DYN_CALL` from the
  body's instructions; the other gate bits are always set, so a gate on
  them skips nothing yet.

### 6.3 Pipelines As Data

`hd_opt::passes::pipeline(target, level)` is a total `match` over the
engine target (`V8`, `Wasmtime`, `Browser`) and the level (`Dev`,
`Optimized`). Every target has the same two rows today, because the knob
measurements found one best value for both engines
([crates/hd_opt/DESIGN.md](../../crates/hd_opt/DESIGN.md) §8). Keying by
target, not by level alone, is better than the design: lowering that
depends on the engine (the closure code field) has one place to live.

### 6.4 Keys And Determinism

- **The pass hash** replaces the tier in every key: each pass's name,
  version and knob values in order, then the link pass names. The
  pipeline's name and its target are not hashed, so two rows with equal
  content share cache entries. A version bump or a knob change misses
  only the pipelines that hold the pass. The profile stays a separate
  key part.
- **Per-body passes read only what the code key covers.** A decision that
  needs whole-program facts (for example, "this trait has one impl in the
  program") cannot run per body, because code entries are shared across
  programs. It would run at link, keyed by the link key.
- **Deterministic output.** Passes iterate in instruction order, never
  over hash maps, and read no time, thread count or address.

### 6.5 Fuel And Limits

Each body gets `Fuel::BODY_DEFAULT`; a body pass pays one step per
instruction it walks. Running out of fuel leaves the body unplanned,
which is plain code, never an error. Fuel counts steps, never time, so
output stays deterministic. `closure_spec` is bounded by its knobs:
callees of at most 64 TIR instructions (`max_insts`), and a specialized
instance requested inside a specialized instance is the last
(`max_depth` 2).

Planned, not built: `hd build --stats`, reporting per pass the bodies
visited, gated out, decisions made and fuel exhaustions.

## 7. Pipelines And Spike Experiments

### 7.1 The Pipelines

| | Dev (`hd run`, `hd test`, REPL) | Optimized (`--release`, including `hd test --release`) |
| --- | --- | --- |
| profile | debug or test: checks on | release (wrapping) or test (checks on) |
| hd passes | none | `value_flow`, `devirt_known`, `closure_spec` |
| layouts | the same as release | the same as dev |
| names | the `name` section | `hd.names` |
| engine | Cranelift `None`, backtracking | Cranelift `Speed`, backtracking |

### 7.2 Spike 0c Additions

Planned, not built. None of these has run.

| ID | Measures | Decision rule |
| --- | --- | --- |
| T1 | Cranelift CPU and run time at `Speed`, `None` with backtracking, `None` with the single-pass allocator, and `Speed` with wasmtime's inliner, over the runtime benchmarks and conformance cases | dev uses `None` with single-pass if it saves at least 30% of Cranelift CPU, passes every runtime case and stays within `dev-speed`; the engine inliner joins release only if it buys at least 5% after hd's own inlining |
| T2 | `wasm-opt` at `-O1`, `-O2`, `-Oz` and dart2wasm's pipeline without `-tnh`, over hd release output | Binaryen joins release only at 15% size or 10% runtime geomean, at most 10 CPU-s for the 10k app |
| T3 | the share of compiled functions a test run executes | if under 30% run and Cranelift is over half of cold `hd test`, raise split modules as an owner question |
| T4 | no inlining, trivial inlining, and trivial inlining with never-standalone callees dropped | dev keeps trivial inlining if it cuts emission plus Cranelift CPU by at least 5% |
| T5 | release passes run separately and fused | fuse only passes over 10% of emission time |

## Sources

- [TPDE](https://arxiv.org/html/2505.22610) (2025): Fig. 9, Cranelift, its single-pass allocator, Winch and TPDE on Wasm.
- [The acyclic e-graph](https://cfallin.org/blog/2026/04/09/aegraph/) (Fallin, 2026) and [regalloc2](https://cfallin.org/blog/2022/06/09/cranelift-regalloc2/) (Fallin, 2022).
- [Faster Compilation in LLVM 20 and Beyond](https://llvm.org/devmtg/2025-04/slides/technical_talk/engelke_faster.pdf) (Engelke, EuroLLVM 2025).
- [MLIR pass management](https://mlir.llvm.org/docs/PassManagement/) and Binaryen's `src/passes/pass.cpp`: passes nested per function for locality.
- [Go `ssa/compile.go`](https://github.com/golang/go/blob/release-branch.go1.24/src/cmd/compile/internal/ssa/compile.go) and [Register allocation in the Go compiler](https://developers.redhat.com/articles/2024/09/24/go-compiler-register-allocation).
- [binaryen#4619](https://github.com/WebAssembly/binaryen/issues/4619), the [GC Optimization Guidebook](https://github.com/WebAssembly/binaryen/wiki/GC-Optimization-Guidebook) and the [Optimizer Cookbook](https://github.com/WebAssembly/binaryen/wiki/Optimizer-Cookbook).
- [wasmtime Wasm proposals](https://docs.wasmtime.dev/stability-wasm-proposals.html) and [A function inliner for Wasmtime](https://bytecodealliance.org/articles/inliner).
- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) and Zig's `ReleaseSafe`: the profile kept apart from optimization.
