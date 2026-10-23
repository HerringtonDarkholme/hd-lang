# Benchmarks with an explicit runner

Status: design proposal, 2026-10-23; nothing here is accepted behavior or specification.
This answers [#184](https://github.com/HerringtonDarkholme/hd-lang/issues/184), for owner approval before implementation.
It reviews [test registration](../../spec/std/testing.md#registration-calls), [Clock](../../spec/std/time.md#clock), and today's [runtime benchmark script](../../bench/runtime/run.mjs).

## Today and a small measurement

`hd test` has test and property-test registration; it has no benchmark case kind or benchmark reporting contract.
The existing script compiles programs, compares their output with JS, and times fresh Node processes.
Its `measure` function runs one warmup, five further unretained calls, and five retained calls; validation adds another process beforehand.
The printed p95 is the maximum of those five samples, and timed calls' statuses are not checked.

Those are observations, not script changes in this ticket.
Source SHA-256 is `6d71fe627cbbc2e36bac806b4ecbc2ed8a32fd2f0716b2315d0bf70edb3cd4a8`, unchanged between the calibration base and this note's base.
Calibration pins `d4b85c0be845fb426bc1476d65693d35f9dae3b0`; this note starts at `1e33ff3886d6d897ae70b2c2fbe745b7b6526df4`.

Nine `node -e ''` subprocesses on Node 24.19.0 took these wall times, in milliseconds:
51.964, 28.525, 28.603, 30.655, 27.683, 48.099, 29.450, 28.928, 29.622; median **29.450 ms**.
Nine separate processes each measured 10,000 intervals between consecutive `performance.now()` readings; each process's median interval was approximately 42 ns.
That interval excludes part of the reading machinery and is neither complete timer overhead nor hd host-import cost.

All samples, process/load snapshots, exact scripts and commands are retained in the [measurement report](https://github.com/HerringtonDarkholme/hd-lang/issues/184#issuecomment-6370947447).
The host was shared, including possible Rust compilation; this is no controlled comparison or proposed-runner performance result.
It illustrates why process startup and steady execution need separate measurements, without establishing a portable overhead to subtract.

## The work a runner should support

| Case | Measurement boundary |
| --- | --- |
| Pure parser or arithmetic | Repeated operations with observable inputs and results |
| A destructive operation | Fresh input setup separated from the selected operation |
| Allocation-heavy work | Clearly state whether release and collection work is included |
| Suspending HTTP or process call | End-to-end elapsed completion, not just task construction |
| Scheduling under ManualClock | Real elapsed time outside the fake clock's domain |
| CLI startup | Fresh-process elapsed time, kept separate from the cases above |

Assertions about correctness belong outside the timed interval where possible, but failure must invalidate the measurement.
No runner can make a remote service or shared machine deterministic simply by collecting more samples.

## Useful precedents

| Tool | Useful distinction |
| --- | --- |
| [Go `B.Loop`](https://pkg.go.dev/testing#B.Loop) | Setup/cleanup sit outside the loop; compiler support keeps loop arguments and results alive |
| [Criterion timing loops](https://bheisler.github.io/criterion.rs/book/user_guide/timing_loops.html) | Batched setup separates mutable input preparation; different loops choose whether result destruction is timed |
| [Python timeit](https://docs.python.org/3/library/timeit.html) | Repeats a batch, separates setup, and documents how garbage-collection policy affects interpretation |
| [pyperf](https://pyperf.readthedocs.io/en/latest/run_benchmark.html) | Distinguishes worker processes, warmups, values and loop calibration; runs benchmarks sequentially |
| [JMH dead-code example](https://github.com/openjdk/jmh/blob/master/jmh-samples/src/main/java/org/openjdk/jmh/samples/JMHSample_08_DeadCode.java) | An unused pure result can disappear; returning or consuming it changes what is measured |
| [Rust `black_box`](https://doc.rust-lang.org/std/hint/fn.black_box.html) | Optimization resistance is best effort and backend-dependent, not a correctness or security guarantee |

My inference is to separate registration, timing and optimization resistance.
Go's loop is not merely an ordinary library loop with a convenient name; copying its spelling would not reproduce its compiler support.
The smallest hd runner can use existing functions, closures and requirement rows, while keeping optimization barriers explicit.

## Three mechanisms

| Mechanism | Registration and timing | Cost and limitation |
| --- | --- | --- |
| **Ordinary benchmark suite** | A `benches/` executable registers callbacks through std code, then runs them in process | CLI discovery and std APIs; explicit barrier support is still needed for reliable pure microbenchmarks |
| Compiler-known benchmark calls | Add another compiler-recognized case kind alongside existing test registrations | Language position/registration rules and compiler changes; no new grammar is necessary, but this is more machinery |
| Whole-program process runner | Each file is a benchmark; time its process and check output/status | Simplest option; useful for startup and coarse workloads, not a replacement for in-process operation timing |

All three can benchmark the same existing operation:

```text
fn square(n: usize) -> usize:
    n * n

println(square(17))
```

The process option times the executable containing this code.
Either callback option times repeated calls to `square`, with explicit optimization resistance around inputs and results.
The file above validates existing syntax only; printing a constant result once is not itself a trustworthy microbenchmark.

## Recommendation: ordinary registration, timed batches

Use `benches/*.hd` as executable roots with ordinary `main` functions and a std benchmark-suite API.
`hd bench` builds and runs them separately from `hd test`; test runs do not execute benchmark bodies.
Helpers remain ordinary modules; each root opts into registration explicitly, avoiding compiler discovery of special function names or a new `bench:` block.
The CLI needs a separate root/discovery contract and name filtering, not a second language test-registration mechanism.

The following API names are illustrative and unimplemented; the spelling uses existing grammar:

```text
use std.bench.{Benchmarks, black_box}
use std.time.Clock

fn square(n: usize) -> usize:
    n * n

pub fn main!() -> void $ Clock + Console:
    let mut cases = Benchmarks::new()
    cases.add("square", fn() -> usize: square(black_box(17)))
    cases.run!()
```

Collect registrations before running anything; reject duplicate qualified names and invalid options before invoking timed callbacks.
Use file/module path plus the explicit label as the result identity; changes to a label create a different case.
Captured setup values are constructed during registration, outside measurement, and reconstructed in every fresh process.
The runner should report registration/setup time separately when it matters.

### Clock and requirement rows

Use the real host's existing monotonic `Clock` for timing; do not add another clock capability merely to obtain a different name.
Capture that provider in the harness before installing any body-specific providers.
A benchmark body can install `ManualClock` for the code under test while the harness measures actual elapsed time with its retained real provider.
Refuse a non-advancing measurement clock instead of returning a zero-time result.

Only the harness needs timing authority; `square` remains pure, and a send operation keeps its own actual requirements.
Callbacks needing other providers capture the appropriate requirement environment through existing rules.
Suspending callbacks must be awaited to completion; their reported latency includes host waits and scheduling.
Implementations must prove this provider separation before claiming fake-clock benchmarks work.

Today's [Instant](../../lib/std/time.hd) stores whole milliseconds.
Keep that representation initially and amortize timing over sufficiently long batches, instead of silently changing the public time types.
Report the observed timer resolution and raw elapsed milliseconds; a calculated ns/op number does not mean a nanosecond-resolution clock.
A high-resolution timer can be considered separately if measured batch limits make it necessary.

### Sampling and setup

Recommend three sequential fresh processes per case, each with unreported warmup/calibration and ten retained samples.
Double iterations until a batch lasts at least 250 ms and spans at least 100 observed timer ticks, with iteration/time limits.
Then freeze that process's batch size for its retained samples; report it with every raw elapsed value.
Warmup and calibration invoke the real operation, so effecting benchmarks must opt into repetition and supply disposable inputs or services.

These numbers are proposed defaults, not measured convergence guarantees.
Expose warmup, sample, process and duration budgets; report a capped or unstable result rather than looping indefinitely.
Run one case at a time and retain every valid sample, including slow ones.
Fresh processes limit inherited state; they do not clear filesystem caches, engine caches or external service state.

The default repeated-operation callback includes ordinary callback, loop, result consumption and release costs in the measured interval.
Provide a separate batched-setup form for destructive inputs; generate a bounded batch before timing, then run the operations over it.
Its result-retention and release policy must be explicit, including memory limits; do not retain an unbounded list of outputs to hide teardown.
End-to-end allocation benchmarks must include the release/collector work their chosen boundary promises.

No automatic subtraction of empty-loop, timer or process overhead: subtraction can amplify noise and hide real harness interactions.
An explicitly named empty case can expose overhead alongside the real results.
Any asynchronous timeout, panic, failed assertion, nonzero child status or invalid result marks that sample and case failed.
Preserve failed evidence, exclude it from successful timing statistics, and return a failing command status.

### Keeping pure work present

A normal identity function or checksum printed after the loop does not prevent constant folding or hoisting invariant work.
Recommend one explicit best-effort `black_box` intrinsic, exposed through std, for inputs and consumed results.
The library runner consumes each returned result through it; the author makes constant inputs opaque where that is necessary.
Do not add Go-style special treatment of every statement inside a syntactically recognized loop.

This intrinsic is the recommendation's costliest addition under the design cost order; ordinary registration alone cannot supply the required barrier.
It needs owner approval and backend-specific evidence that representative pure operations remain in final code on Node and Wasmtime.
A Wasm host-import barrier may have appreciable overhead; neither zero cost nor perfect optimization resistance is promised.
Unsupported lowering must disable pure microbenchmark claims, rather than quietly substitute an ordinary identity function.

The barrier preserves value semantics, ownership and release behavior; it cannot retain a borrowed value beyond its call or invent an external effect.
Start with a documented supported set of value representations and reject unsupported ones until their lowering is proven.
Inspect generated code alongside an empty case, constant folding, unused results, counted outputs and suspending callbacks.
This is measurement support, never a tool for security guarantees or program correctness.

### Reports and comparison

Text output should show qualified name, engine/profile, processes, samples, iterations, median ns/op and spread, with warnings or failures.
Keep raw per-process samples; show each process's median before the aggregate median so a noisy process remains visible.
Do not label the maximum of a tiny sample set p95 or confuse batch-throughput averages with individual-request latency percentiles.
Startup, compilation and steady execution are different measurements and must have different labels.

Versioned JSON should include case identity, input/config fingerprint, compiler/stdlib/runtime identities, engine flags, target, host context and all raw sample records.
Record elapsed time, iteration count, timing unit, warmup/calibration decisions, setup/release policy, status and failure details.
Program stdout belongs in a captured stream, separate from machine-readable runner output.
Store a baseline only at an explicit destination; no automatic source or baseline edits.

Comparison requires matching workload, timing policy, target/engine and host context, while displaying changed code/toolchain identities.
Report the ratio and absolute difference; flag incompatible records instead of manufacturing a percentage.
Recommend report-only comparison initially, without an automatic significance or regression verdict from thirty samples.
CI thresholds should wait for repeatability evidence on the actual runner machines.

## Approval before implementation

Approval is needed for ordinary suite registration, `benches/` roots and `hd bench`, the explicit barrier intrinsic, measurement defaults and report-only baselines.
This favors std implementation over compiler-known registration, paying for one intrinsic only where ordinary code cannot provide the intended measurement property.
The next-best simplification is the whole-program runner when only end-to-end workloads matter.
The spelling and callback signatures above remain proposals, not available APIs.

Future CLI rules belong in [command-line.md](../../spec/cli/command-line.md), and ordinary runner APIs in a stdlib-tier chapter.
The compiler-known intrinsic and any new diagnostic codes belong in the language tier; no rule IDs are allocated by this note.
An implementation order should prove timing-provider isolation, batching, barrier code generation, destructive setup, awaited completion and failed-sample handling first.
Then compare raw results across repeat runs on both engines before claiming precision or choosing CI thresholds.

## Example validation

Both examples parse using the frozen compiler; the first checks against existing std with zero diagnostics.
The second uses hypothetical APIs and is parse-only evidence, not an implementation or type-check claim.
No benchmark code, compiler, spec, fixture or ad-hoc script changes accompany this proposal.
