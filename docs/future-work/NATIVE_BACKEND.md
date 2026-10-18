# Native Backend: Design Study

This is a proposal for issue #188, the native backend umbrella. Nothing
in it is decided, and nothing in it is in the spec. Numbers marked
"estimate" are not measurements; line counts of existing crates were
counted on 2026-10-17.

## 1. Motivation

hd compiles to one Wasm GC module and runs it on V8 or on wasmtime inside
`hd`, using the engine's collector
([goals.md](../compiler/goals.md), "Later"). Two things make the other
path worth pricing now:

- **The owner wants real parallelism.** Today one program instance runs
  on one thread, and instances exchange only boundary-safe values
  ([Instances And Threads](../../spec/lang/10-modules.md#instances-and-threads)).
  Wasm GC cannot share a reference between threads at all; that waits
  for the shared-everything-threads proposal, which is at phase 1 and
  not implemented in wasmtime (tracked as wasmtime#9466). The
  [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md) study stages
  parallelism in two steps: isolated workers with copied messages and
  shared bytes on Wasm today (stage A), and frozen data shared by
  structured parallel children once GC objects can cross threads
  (stage C). A native backend
  is the other road to stage C.
- **The CLI already must not need Node.** `hd` embeds wasmtime 49, so
  the machine code path exists inside the binary today: Cranelift
  compiles hd's Wasm on every run.

The question is not "native or Wasm". The Wasm path stays: the
playground compiles and runs hd in a browser tab, and browsers only run
Wasm. The question is whether hd needs a second backend, which one, and
in what order the work pays.

## 2. The Proposal On One Page

1. **Ship native executables from the Wasm path first.** `hd build
   --target exe` writes one file: a runtime-only wasmtime host (no
   Cranelift inside) plus the program's precompiled machine code. It
   needs no Node, compiles nothing at start, and reuses every host
   provider. It is a few jobs (phase 1).
2. **Get parallelism without shared GC objects first.** This is the
   parallelism study's stage A: one program instance per worker thread,
   boundary-safe values copied between them, large bytes in shared
   linear memory. It works on wasmtime and on V8 today and needs no new
   collector.
3. **Build a true native backend only on a measured need.** If
   benchmarks show Wasm GC's layout costs (casts, boxed parts, no
   interior pointers, per-access checks) are large, or the owner wants
   stage C before shared-everything threads mature, then build it with
   **Cranelift** from a lowering plan shared with `hd_wasm`, and a
   **precise, non-moving mark-sweep** collector that grows into Immix.
4. **Not LLVM, not C, not Boehm, not reference counting** for the first
   native backend. LLVM stays a candidate for a later optimizing tier,
   as goals.md already lists.

## 3. What Any Backend Must Keep

These come from the spec and from the compiler's own rules. A native
backend that breaks one of them is wrong, however fast.

| Requirement | Where it comes from | What it costs a native backend |
| --- | --- | --- |
| The official compiler targets Wasm only | [`module.host.wasm`](../../spec/lang/10-modules.md#r-module.host.wasm), [`module.host.runtime`](../../spec/lang/10-modules.md#r-module.host.runtime) | a spec change naming the native target; until then it is an extension |
| Instances share nothing: heaps, globals and drivers are disjoint | [`module.instance.disjoint`](../../spec/lang/10-modules.md#r-module.instance.disjoint) | holds for free with one heap per instance; stage C needs the spec to change first |
| A fresh instance per test case | [`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance) | module globals live in a per-instance context, not in static data; many instances per process |
| Panics are observable: category, message, site, backtrace | crates/hd_run_wasmtime/DESIGN.md §16.2 | a panic unwinds or abandons a stack to the case boundary; code offsets map to lines |
| `stack-exhausted` and `heap-exhausted` are panics, not crashes | same | stack limit checks or guard pages; an allocator that can refuse |
| Output never depends on the pipeline, thread count or addresses | [tiering.md](../compiler/tiering.md) §4.1, ARCHITECTURE.md | no address-based hashing, no GC timing visible, deterministic scheduling |
| Suspension (`!`) is a cold state machine | [`req.lowering.representation`](../../spec/lang/11-requirements-and-suspension.md#r-req.lowering.representation) | the same frames, or another representation with the same behaviour |
| Host calls go through `hd_host_abi` | crates/hd_run_wasmtime/DESIGN.md §17 | a native call path into the same providers |
| `unsafe_code` is denied workspace-wide | ARCHITECTURE.md, "Invariants" | loading precompiled code, a collector and stack walking need `unsafe`; see question 5 |
| Behaviour equals V8 and wasmtime | issue #188, "Conformance" | the conformance suite runs on the new target in CI |

## 4. The Options

### 4.1 Keep Wasm, Compile It Ahead Of Time Through wasmtime

wasmtime can compile a module once (`Engine::precompile_module`), store
the result as a `.cwasm`, and load it later with
`Module::deserialize_file`, which maps the machine code without running
Cranelift. A wasmtime built without its `cranelift` feature can still
load and run such files. The compiler design planned a `cwasm` cache on
this, but it is not built: deserializing is `unsafe`, and
`hd_run_wasmtime` forbids `unsafe`
([engines-and-test-runner.md](../compiler/engines-and-test-runner.md)
§18.2). So this option needs one approved `unsafe` call (question 5).

- **Startup:** a file map and a relocation pass; no compile. Estimate:
  under 5 ms for a small program, which meets the `size-startup-heap`
  target.
- **Peak speed:** Cranelift `OptLevel::Speed` over Wasm GC. The Cranelift
  README cites third-party numbers of about 2% slower than V8's
  optimizing tier and about 14% slower than an LLVM-based Wasm engine,
  on linear-memory code. Wasm GC adds costs no native layout pays: a
  `ref.cast` on every erased read, embedded parts as separate objects,
  no interior pointers, and GC heap accesses through 32-bit indices
  with checks.
- **Binary size:** wasmtime's minimal-embedding guide reports about 1 to
  2 MB for the runtime without a compiler, before GC support and before
  hd's providers. Estimate for an hd executable: 3 to 6 MB plus the
  program's code.
- **Parallelism:** one store per thread, no GC object shared. That is
  real parallelism for workers that exchange copied messages, plus
  shared linear memory for bytes (stage A); it is not stage C.
- **GC:** wasmtime's collectors: copying (the default, and what `hd`
  selects), deferred reference counting without cycle collection, or
  null. hd does not control pauses or layout.
- **New code:** small. A runner binary, an embed step, and a CLI flag.
- **Security:** a `.cwasm` is trusted native code, which is why
  deserializing it is `unsafe`. An executable that embeds code built on
  the user's own machine, and loads only that, is no worse than any
  compiled program; a `.cwasm` from anywhere else must never be loaded.
  The Wasm sandbox still contains a miscompile.

### 4.2 Cranelift From hd's Own Lowering

hd emits Cranelift IR directly and owns the object layout, the
collector and the calling convention. Cranelift has what a GC'd language
needs: `declare_value_needs_stack_map` marks a value as a GC reference,
spilled to a stack slot at each safepoint and listed in a stack map,
which also allows a moving collector to update it; `try_call` gives
exception edges for unwinding; and `cranelift-object` writes object
files. It is the same code generator hd already runs, so the dev and
optimized pipelines keep their meaning (`OptLevel::None` and `Speed`).

- **Startup:** native; no engine to initialize.
- **Peak speed:** better than 4.1 by whatever the Wasm GC costs are,
  which phase 0 measures. Inline embedded parts, interior pointers into
  arrays and strings, untagged small integers in erased slots, and
  checks hd can prove away are all open to it.
- **Binary size:** the runtime (collector, scheduler driver, intrinsics,
  providers) plus code. Estimate: 1 to 3 MB for a small program.
- **Parallelism:** whatever the collector supports; a shared heap is
  possible.
- **GC:** hd's own (section 5).
- **New code:** large (section 7). This is the real cost.
- **Security:** no sandbox. Memory safety rests on the emitter and the
  collector being right.

### 4.3 LLVM

- **Peak speed:** the best of the four, from mature optimizations.
- **Compile time:** the worst. The Cranelift README cites about an order
  of magnitude faster compiles than LLVM, and tiering.md quotes an
  EuroLLVM 2025 talk on its `-O0` cost. hd's dev pipeline cannot use it.
- **GC:** precise roots through `gc.statepoint`, which is harder to use
  and less tested than Cranelift's stack maps. scriptc avoids that by
  using reference counting with LLVM.
- **Toolchain:** a large C++ dependency in the `hd` binary, or an LLVM
  helper shipped beside it, as scriptc does.
- **Fit:** a later optimizing tier for `--release`, not the first native
  backend. goals.md already lists "LLVM for the optimizing tier" under
  Later.

### 4.4 C As A Target

hd emits C and a system C compiler builds it, as Nim and Koka do.

- **Good:** a simple emitter, every platform, the system debugger.
- **Bad:** the user needs a C toolchain, which is the same kind of
  requirement the owner removed for Node. Precise roots in C need a
  shadow stack (slower) or conservative stack scanning (section 5).
  Checked arithmetic, tail calls and stack limits need compiler
  builtins that differ per compiler. Compile time is the C compiler's.
- **Fit:** none of hd's goals favours it over 4.2.

### 4.5 Side By Side

| | Wasm AOT (4.1) | Cranelift (4.2) | LLVM (4.3) | C (4.4) |
| --- | --- | --- | --- | --- |
| Startup | ms, map and relocate | native | native | native |
| Peak speed | Cranelift over Wasm GC | Cranelift over hd layouts | best | near LLVM |
| Compile time | today's | today's, roughly | much slower | C compiler's |
| Binary size (small program, estimate) | 3 to 6 MB | 1 to 3 MB | 1 to 3 MB | 1 to 3 MB |
| GC objects shared across threads | no, until shared-everything threads | yes | yes | yes |
| Collector | wasmtime's | hd's | hd's | hd's, likely conservative |
| User toolchain | none | none, or a linker (section 7.3) | LLVM helper | a C compiler |
| Sandbox | yes | no | no | no |
| New code (estimate) | about 1k lines | 20k to 28k lines | 30k+ lines | 20k+ lines |

## 5. A Collector On Native

### 5.1 What hd Needs

- **Precise roots.** Monomorphized code knows the type of every value,
  so precise stack maps cost nothing at the language level. Conservative
  scanning would only buy a simpler emitter.
- **Moving is optional at first.** Wasm GC exposes no addresses, so no
  hd program can depend on one today, and a moving collector stays
  possible later. A non-moving one is simpler and lets the host hold raw
  pointers during a call.
- **Many heaps per process.** A fresh instance per test case, run by a
  pool of worker threads, means one small heap per instance, created and
  thrown away fast. This favours a heap you can discard whole.
- **Threads, depending on the stage** (section 6): none for stage A
  (one heap per instance and thread), a shared heap or shared region for
  stage C.
- **No finalizers.** The owner decided that resources use `Disposable`
  and `using`, and a leak is a report from the host's descriptor table,
  never a GC finalizer. So prompt freeing, the main argument for
  reference counting, buys hd nothing for resources.
- **A refusing allocator.** `heap-exhausted` must be a panic with a
  site.

### 5.2 Candidates

| Collector | How it works | Gains | Costs |
| --- | --- | --- | --- |
| Precise mark-sweep, non-moving | segregated size classes, mark bits, lazy sweep, roots from stack maps | small (estimate: 2k to 3k lines); no barriers; simple to verify with a collect-at-every-allocation stress mode | fragmentation; allocation slower than a bump pointer; pauses grow with the heap |
| Immix (mark-region) | 32 KB blocks of 128-byte lines; bump allocation into free lines; evacuates only fragmented blocks | bump-pointer speed, good locality, defragments without moving everything; Scala Native ships it; MMTk has it in Rust | more code (estimate: 4k to 6k lines), or a dependency on MMTk; evacuation needs precise roots, which hd has |
| Reference counting with cycle collection | counts on every pointer copy; a cycle collector at defined points (scriptc's model) | prompt freeing; short pauses | every copy pays an increment and a decrement; atomic counts if objects cross threads, so scriptc keeps heaps per thread and makes shared graphs immortal; cycles still need a tracing pass |
| Boehm, conservative | scans stacks and objects for anything that looks like a pointer | no stack maps; drops into any emitter | false retention; cannot move; weak `heap-exhausted` story; unpredictable pauses; no help with per-instance heaps |
| Generational copying (Go's opposite, OCaml's minor heap) | young objects in a bump nursery, survivors copied out | the fastest allocation; most hd objects die young | write barrier on every pointer store into an old object; moving |

Two runtimes show where a parallel version ends up. Go's collector is a
non-moving concurrent mark-sweep with a write barrier, tuned by one
knob (`GOGC`) and a memory limit. OCaml 5 gives each domain its own
minor heap, collects the minor heaps together in a short stop-the-world
pause, and runs a mostly concurrent mark-sweep over a shared major heap
with per-domain size-segregated pools.

### 5.3 Recommendation

Start with **precise, non-moving mark-sweep, one heap per instance**,
roots from Cranelift stack maps, and a stress mode that collects at every
allocation. It is the smallest collector that is correct, it needs no
barriers, and it is enough to run conformance. When benchmarks ask for
more, move to **Immix** (written, or MMTk's), keeping roots precise. If
stage C runs on native, follow OCaml 5's shape: per-thread allocation, a
shared heap marked in parallel during short pauses, then concurrent
marking only if pauses are measured as too long.

## 6. Threads And Shared Data

The [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md) study
(§4 and §6) sets the model this section prices. Its stages, and what
each asks of the runtime:

| Stage | What a program does | Wasm | Native runtime needs |
| --- | --- | --- | --- |
| A. Isolated workers | starts workers of the same build; sends boundary-safe values, which are copied; hands large bytes over as `SharedBytes` in shared linear memory | today: one instance per thread, values through the existing boundary codecs, Wasm threads for the shared bytes | one heap per thread; no locks in the collector; a shared byte region outside the GC heap |
| C. Structured parallel children | children read the parent's `Frozen` (deeply immutable) data without copying, write only what they allocate, and return results | waits for shared-everything threads | a shared heap the children allocate into, or a shared read-only region that per-thread collectors trace but never free; thread-local or forbidden writes to top-level `let` bindings |
| Shared mutable GC objects behind locks | `Mutex` over a GC object | the study leaves it out | write barriers, a memory model and a concurrent collector; not proposed |

Stage A has precedent inside `hd`: the test runner already runs cases on
a pool of threads, one store each (engines-and-test-runner.md §19.3).
It is also the worker fan-out of
[Serializable Closures](SERIALIZABLE_CLOSURE_DESIGN.md) §3.4, where a
job's code travels as a content identity and its captures are
boundary-safe. Its cost is the copy, paid per message. A native backend
changes nothing about stage A except that its workers start faster.

Stage C is where a native backend would matter. On native, a frozen
graph could be shared in place, as scriptc's `publish` does, and freed
by a collector that traces the shared region. But the study's `freeze`
copies into shared types, because shared-everything threads forbid a
shared object from pointing to an unshared one. The rule here: **stage C
has one meaning on both backends.** If `freeze` copies on Wasm, it
copies on native too, or Wasm adopts the cheaper meaning when it can.
Native may only be faster, never different.

**Determinism.** The goals put every source of nondeterminism behind a
capability or the scheduler. Both stages keep that if the order in which
results come back is fixed by the API (a parallel map returns results in
input order), not by which thread finishes first.

## 7. What Stays Shared With The Wasm Path

### 7.1 Shared Unchanged

Everything up to and including the pass pipeline: parsing, resolution,
checking, TIR (`hd_tir`), monomorphization (`hd_mono`), the pass manager
(`hd_opt`, keyed by engine target, so a `Native` row has a place), the
cache, the driver, `lib/std`, the host ABI table (`hd_host_abi`), and the
engine-neutral poll and wake driver and test model (`hd_run`). TIR is
already the contract between the halves.

### 7.2 Backend-Specific Today

`hd_wasm` is 19.6k lines. Part of it is not Wasm-specific but lives
there: value layout shapes (`layout.rs`, 2.0k), liveness across
suspension points (`live.rs`), and boundary type planning (part of
`boundary.rs`, 1.6k). Before a second emitter exists, those decisions
should move into a shared lowering-plan crate, so the two emitters cannot disagree on what a value looks like or
which locals a suspension frame saves. The planned move of `Map`'s table
out of the emitter and into `lib/std` (#180) also shrinks what a second
emitter must write.

The host providers in `hd_run_wasmtime` (files, sockets, HTTP,
processes, about 22k lines with tests) are written against wasmtime's
`Caller`. A native backend calls them through a neutral interface
instead, which is a refactor of that crate (issue #178 already notes its
quality).

### 7.3 Executables Without A Linker

Cranelift writes object files, and linking them normally needs a system
linker. Two ways around that requirement:

- Ship a prebuilt runtime executable per target and append the program's
  code to it as a blob that the runtime maps at start and relocates, as
  wasmtime does with a `.cwasm`. No user toolchain.
- Link with a system linker when one exists, for builds that want a
  normal object layout (for code signing or for profilers).

The first is the default proposed here; the second is an option for
release builds.

### 7.4 Size Of The New Code (Estimate)

| Piece | Lines (estimate) | Note |
| --- | --- | --- |
| Shared lowering plan (moved, not new) | 3k to 4k moved | no behaviour change; same Wasm bytes |
| Cranelift emitter | 12k to 16k | `hd_wasm` minus its Wasm encoding and linking |
| Collector (mark-sweep) | 2k to 3k | Immix later: 4k to 6k more, or MMTk |
| Runtime: instance context, panics and unwinding, stack limits, code loader | 2k to 3k | the `unsafe` part |
| 90 intrinsic bodies | 1.5k to 2.5k | each a few Wasm instructions today |
| Provider interface off wasmtime | 2k to 3k changed | the providers themselves are reused |
| Conformance target and CI | 0.5k | a target column in the harness |
| **Total for 4.2** | **about 20k to 28k new, plus the move** | a little smaller than `hd_check` (33k) |

For comparison, the Wasm AOT executable of 4.1 is about 1k lines.

## 8. A Phased Plan

Each step is one agent job of about an hour, and each phase ends with
something it proves. A phase gate decides whether the next phase starts.

### Phase 0: Measure The Wasm Path (no backend code)

1. Run the `runtime`, `allocations` and `long-run-memory` metric scripts
   on wasmtime and on Node; record GC count and pause per case.
   **Proves** whether hd's runtime speed is a problem at all.
2. Profile three allocation-heavy and three erased-heavy (`dyn`,
   generics through `dyn`) benchmarks on wasmtime; attribute time to
   GC, `ref.cast`, bounds checks and part loads. **Proves** the upper
   bound a native layout could win.

Gate: a native backend for speed is worth starting only if phase 0 shows
at least 1.5x to win on the geomean. Stage C before shared-everything
threads is a separate reason, which only the owner can give (question 2).

### Phase 1: Self-Contained Executables On Wasm AOT

1. A runtime-only runner: wasmtime without `cranelift`, hd's providers,
   loading an embedded `.cwasm` through the one approved `unsafe` call.
   **Proves** the binary size and startup numbers of 4.1.
2. `hd build --target exe` writes runner plus code, honouring the same
   grant flags as `hd run`. **Proves** that a shipped hd program needs
   neither Node nor a compile at start.
3. Run the runtime conformance fixtures through the executable path in
   CI on Linux x86, macOS ARM and Windows. **Proves** equal behaviour.

### Phase 2: Stage A Parallelism On Wasm

This is the parallelism study's stage A. It needs that study's approval
and its own std and host proposal; it is listed here because it decides
whether phase 3 is ever needed for parallelism.

1. Host: start a second instance of the same module on a worker thread
   and pass it a boundary-safe value through the existing codecs.
2. `SharedBytes` over a shared linear memory imported by every instance.
3. The std API that proposal names, on wasmtime, then on V8 through
   workers.
4. A speedup benchmark for user programs (the existing
   `parallel-speedup` script measures the compiler). **Proves** the
   model, its API and its cost on both engines, with no new collector.

### Phase 3: Shared Lowering Plan (refactor)

1. Move layout shapes from `hd_wasm` into a shared crate.
2. Move suspension liveness.
3. Move boundary type planning.

Each step keeps the Wasm bytes identical (the hello build's size and
hash do not change). **Proves** that a second emitter will not repeat
decisions. Starts only after the phase 0 gate passes or the owner
asks for stage C on native.

### Phase 4: Native Spike, One Thread

1. Cranelift emitter for integers, floats, calls, structs, `if` and
   loops; run 20 runtime fixtures.
2. Mark-sweep collector with stack-map roots, and the
   collect-at-every-allocation stress mode.
3. Panics with category and site, `stack-exhausted` through Cranelift's
   stack limit check, `heap-exhausted` from the allocator.
4. Benchmark against the phase 1 executable on the phase 0 suite.
   **Proves** the speed gain is real.

Gate: continue only if step 4 meets the phase 0 target, or the owner
wants stage C on native.

### Phase 5: Full Coverage

Jobs, one each: enums and matches; closures; `dyn` and erased bodies;
strings and arrays; `Map`; suspension frames and the poll driver; the
host call path through the neutral provider interface; fresh instances
for the test runner; DWARF lines for debuggers and backtraces. The
conformance suite runs on the native target in CI, with failures listed
in `KNOWN_FAILURES.tsv` like any other. **Proves** native equals V8 and
wasmtime.

### Phase 6: Threads And A Better Collector

Only for stage C: per-thread allocation, a shared region for frozen
data, a parallel stop-the-world mark, then Immix, all behind the same
`freeze` meaning as on Wasm. **Proves** stage C on native with measured
pauses.

## 9. Risks

- **Two backends forever.** The playground keeps Wasm, so every
  language feature lands in two emitters and runs conformance on both.
  This is the largest cost, and it never ends. It is why phases 0 to 2
  come first: they may show that one backend is enough.
- **The sandbox goes away.** On Wasm, a miscompile or a missing check
  traps inside the engine. On native, a collector bug or a wrong stack
  map is memory corruption in a server, which can be exploitable.
  Security weighs against native for any program that handles untrusted
  input, until the emitter and collector are fuzzed as hard as
  Cranelift is.
- **`unsafe` code.** A collector, a code loader and stack walking cannot
  be written in safe Rust. Today only two modules may use `unsafe`.
- **Collector bugs are hard.** A missed root frees a live object, and
  the crash happens far away. Stress modes, a heap verifier and
  differential runs against wasmtime are needed from the first day.
- **Shared-everything threads may arrive.** If wasmtime and V8 ship
  shared GC references, stage C works on Wasm and the parallelism
  argument for native goes away. The proposal is at phase 1, so this is
  years, not months; it is still a reason not to start native early.
- **Platforms.** Three targets in CI (Linux x86, macOS ARM, Windows),
  each with its own unwinding, stack probes and executable format;
  macOS code signing may dislike code mapped from an appended blob.
- **Cranelift's API is not stable.** Pin it exactly, as wasmtime is
  pinned today.
- **Determinism under threads.** Any API that exposes completion order
  breaks byte-identical output.

## 10. Questions For The Owner

1. **Direction.** Take the Wasm path to native executables first
   (`hd build --target exe` over wasmtime AOT), and build a true native
   backend only when phase 0 benchmarks show at least 1.5x to win, or
   the owner wants stage C on native? *Recommendation: yes.* It is
   about 1k lines, keeps the sandbox, and does not commit hd to two
   backends.
2. **Stage C and native.** The parallelism study parks stage C until
   shared-everything threads reach phase 3. Is a native backend an
   acceptable earlier road to stage C, on the condition that `freeze`
   and every stage C rule mean the same on both backends? *Recommendation:
   yes, but do not start it for that reason alone.* Stage A covers
   worker pools and parallel maps over copied or byte data on both
   engines today, and a native-only stage C would make programs behave
   differently in the playground.
3. **Code generator.** If a native backend is built, use Cranelift from a
   lowering plan shared with `hd_wasm`, and keep LLVM for a later
   optimizing tier? *Recommendation: Cranelift.* It is already in `hd`,
   it compiles fast enough for the dev pipeline, and its stack maps fit
   precise GC.
4. **Collector.** Start with a precise, non-moving mark-sweep, one heap
   per instance, then Immix; rule out reference counting and Boehm?
   *Recommendation: yes.* With `Disposable` deciding resource cleanup,
   prompt freeing buys nothing, and precise roots are free after
   monomorphization.
5. **`unsafe`.** Phase 1 needs one `unsafe` call, wasmtime's
   `Module::deserialize`, to load the executable's own embedded code.
   A native backend later needs a whole runtime crate of it (collector,
   code loader, stack walking). Allow the first now, and the second only
   when phase 4 starts? *Recommendation: yes.* The phase 1 call loads
   only bytes embedded in the same file at build time. The phase 4 crate
   is the only other one, gated by a stress mode, a heap verifier and
   fuzzing.

## Sources

- Cranelift: [README](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/README.md)
  (backends, code quality and compile speed against V8 and LLVM, citing
  [arXiv 2011.13127](https://arxiv.org/abs/2011.13127));
  [`FunctionBuilder::declare_value_needs_stack_map`](https://docs.rs/cranelift-frontend/latest/cranelift_frontend/struct.FunctionBuilder.html);
  [`InstBuilder::try_call`](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/ir/trait.InstBuilder.html);
  [cranelift-object](https://docs.rs/cranelift-object/latest/cranelift_object/).
- wasmtime: [Pre-compiling Wasm](https://docs.wasmtime.dev/examples-pre-compiling-wasm.html)
  (`precompile_module`, `.cwasm`, `deserialize_file` without the
  compiler); [Building a minimal embedding](https://docs.wasmtime.dev/examples-minimal.html)
  (runtime size); [`Collector`](https://docs.rs/wasmtime/latest/wasmtime/enum.Collector.html)
  (copying, deferred reference counting, null);
  [Wasm proposals](https://docs.wasmtime.dev/stability-wasm-proposals.html)
  (GC supported; shared-everything-threads not implemented, #9466).
- [Shared-everything threads overview](https://github.com/WebAssembly/shared-everything-threads/blob/main/proposals/shared-everything-threads/Overview.md)
  ("There is no way to use threads with WasmGC programs at all") and the
  [proposals list](https://github.com/WebAssembly/proposals) (phase 1).
- scriptc: [llms.txt](https://scriptc.dev/llms.txt): "Native runtime"
  (reference counting with a cycle collector, stackless coroutines),
  "Compiler pipeline" (typed IR to LLVM, precompiled runtime packs),
  "Sharing Data Between Workers" (`publish` makes a graph immutable and
  shares it by reference; published graphs are immortal), `Acyclic`.
- Go: [A Guide to the Go Garbage Collector](https://tip.golang.org/doc/gc-guide)
  (non-moving mark-sweep, `GOGC`, the memory limit).
- OCaml 5: [Parallel programming, §3 Parallel garbage collection](https://ocaml.org/manual/5.2/parallelism.html)
  (per-domain minor heaps, stop-the-world minor collection, concurrent
  mark-sweep major heap) and [Retrofitting Parallelism onto OCaml](https://arxiv.org/abs/2004.11663)
  (ICFP 2020).
- Immix: [Blackburn and McKinley, PLDI 2008](https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf);
  [MMTk](https://github.com/mmtk/mmtk-core) (Immix, mark-sweep and
  generational plans in Rust).
- Boehm: [A garbage collector for C and C++](https://www.hboehm.info/gc/).
- In this repository: [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md)
  (stages A and C, `SharedBytes`, `Frozen`),
  [goals.md](../compiler/goals.md),
  [tiering.md](../compiler/tiering.md),
  [engines-and-test-runner.md](../compiler/engines-and-test-runner.md),
  [lowering-catalog.md](../compiler/lowering-catalog.md) ("Embedded
  Parts": inline parts need interior pointers),
  crates/hd_run_wasmtime/DESIGN.md §16.
