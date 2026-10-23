# Benchmarks with an explicit runner

Status: design proposal, 2026-10-23; domains: CLI/tooling, stdlib API,
and runtime measurement; not accepted. This rewrites
[#184](https://github.com/HerringtonDarkholme/hd-lang/issues/184) under its
[owner review](https://github.com/HerringtonDarkholme/hd-lang/issues/184#issuecomment-6372421647).
No command, library API, intrinsic, statistical guarantee, or specification
is implemented by this document.

## Contents

- [Today and a small measurement](#today-and-a-small-measurement)
- [The work a runner should support](#the-work-a-runner-should-support)
- [Useful precedents](#useful-precedents)
- [Semantics and measurement boundaries](#semantics-and-measurement-boundaries)
- [Three mechanisms](#three-mechanisms)
- [Recommendation: ordinary registration, timed batches](#recommendation-ordinary-registration-timed-batches)
- [Statistics](#statistics)
- [Warm-up and calibration](#warm-up-and-calibration)
- [Comparing an old and a new implementation](#comparing-an-old-and-a-new-implementation)
- [Noise control](#noise-control)
- [The rest of the state of the art](#the-rest-of-the-state-of-the-art)
- [Architecture](#architecture)
- [Command, configuration and scope](#command-configuration-and-scope)
- [Extension points](#extension-points)
- [Cost](#cost)
- [Stress test](#stress-test)
- [Phasing](#phasing)
- [Approval before implementation](#approval-before-implementation)
- [Example validation](#example-validation)

## Today and a small measurement

[`hd test`](../../spec/cli/command-line.md#test-runs) discovers tests and
property tests through [registration calls](../../spec/std/testing.md#registration-calls).
There is no benchmark case kind, `hd bench`, std benchmark module,
or public optimization barrier. A current-source dev CLI returns usage
and status 101 for `hd bench`.

Two scripts measure different existing workloads. The 143-line
[runtime runner](../../bench/runtime/run.mjs) performs one warmup, five
unretained calls and five retained calls; validation adds another process.
Its p95 is the maximum of five and timed statuses are not checked.
Those are historical source observations, not fixes made here.

The 447-line [study runner](../../bench/study/run.mjs) validates every
measured stdout/status and normally measures fresh-process wall time.
Wasmtime compilation is included in that process boundary.
Its separate Node warm-module mode reuses the compiled module but creates
fresh hosts and instances, discarding three activations before measurement.
Neither mode is an operation-level benchmark harness.

The preserved [calibration](https://github.com/HerringtonDarkholme/hd-lang/issues/184#issuecomment-6370947447)
used nine blank Node 24.19.0 subprocesses, with wall times in milliseconds:
51.964, 28.525, 28.603, 30.655, 27.683, 48.099, 29.450, 28.928, 29.622.
Median startup wall time was 29.450 ms on that shared host.
This does not measure the proposed runner or isolate engine initialization.

Each calibration process also measured 10,000 intervals between consecutive
`performance.now()` readings; process median intervals were approximately 42 ns.
That is neither complete timer overhead nor hd host-import cost, and
must not become a correction subtracted from results.
All 90,000 intervals, process/load records and commands remain preserved.

Calibration pins `d4b85c0be845fb426bc1476d65693d35f9dae3b0`.
The runtime-script SHA-256 remains
`6d71fe627cbbc2e36bac806b4ecbc2ed8a32fd2f0716b2315d0bf70edb3cd4a8`
at this revision's base `8c5aada08acad5e1ccf8cc6c7fb76ca9ba9a605a`.
No isolated-machine, portable overhead or implementation-speedup claim follows.

## The work a runner should support

An *operation* is one declared unit of work. A *batch* times many operations
between two readings. A *worker* is a fresh OS process that performs
warmup and several retained batches; its batches are not independent processes.
These distinctions determine both the reported unit and the statistical unit.

### Use cases, end to end

The command forms below are proposed; their example decisions are illustrative,
not measurements of hd workloads.

| Fixed use case | Commands, observations and decision |
| --- | --- |
| U1: tune one function | Add `parse/1024`, run `hd bench --filter parse/1024 --save-baseline before.json`, change the implementation, save `after.json` with the same command and its new destination, then use `hd bench compare before.json after.json`. Inspect raw worker medians, interval and warnings; choose a fresh paired run before trusting a small difference. |
| U2: catch a CI regression | On a dedicated runner, run `hd bench --filter critical --against-compiler /tools/old-hd --pairs 20 --fail-regression 3% --format json`. Archive both artifacts and raw samples. Fail only when the compatible, stable paired interval lies wholly above the approved threshold; inconclusive results need rerunning, not a green performance verdict. |
| U3: compare compiler versions | Freeze benchmark sources, inputs and the harness protocol; build identical workloads with both binaries. Use the same-engine paired command and inspect both compiler/artifact identities. Keep code-generation time, fresh-process wall time and in-process operation timing as separate records; do not compare one boundary with another. |
| U4: compare two std implementations | Register explicit labels `concat/old/4096` and `concat/new/4096` with identical generated input seeds and units. Request `--pair concat/old/4096,concat/new/4096`; run balanced order, verify outputs, and examine elapsed plus separate allocation diagnostics before choosing the implementation. |

U1's saved historical baseline is convenient but cannot undo machine drift.
U2 and U3 require both compilers to support the benchmark protocol/API.
For older compilers, the existing whole-program study remains a different,
explicitly labeled experiment; unsupported versions must not silently switch boundaries.

| Work shape | Promised boundary |
| --- | --- |
| Pure parser/arithmetic | Batch includes operation calls, explicit input/result barriers, loop and normal result release. |
| Destructive operation | Bounded fresh input preparation outside each measured batch; mutation and the selected release policy inside it. |
| Allocation-heavy operation | State precisely which input/result releases and collector work are included. |
| Suspending HTTP/process work | Awaited completion and its scheduling/host waits, not construction of a cold suspension. |
| ManualClock scheduling | Body uses virtual time; retained outer real clock measures elapsed execution. |
| CLI startup or compilation | Separate process/build modes, never relabeled as ns/op inside a warm instance. |

## Useful precedents

### Criterion

Criterion separates warmup, measurement, analysis and comparison. Warmup
uses repeated increasing iteration counts; collection estimates cost across
batches. Its analysis uses resampling and reports outliers, while retaining
them rather than treating every slow result as an invalid run.
See [analysis](https://bheisler.github.io/criterion.rs/book/analysis.html).

Its [timing loops](https://bheisler.github.io/criterion.rs/book/user_guide/timing_loops.html)
make setup and destruction boundaries explicit. A sorting input prepared
inside a timed loop measures something different from a prepared batch.
Retaining large outputs outside the interval can consume substantial memory.
Its [async support](https://bheisler.github.io/criterion.rs/book/user_guide/benchmarking_async.html)
requires an executor, whose costs remain part of that experiment.

The interface includes `--save-baseline` and `--baseline`, plus configurable
sample, confidence and noise settings.
These are useful models, but stored baselines do not remove environmental
confounding, and adding samples cannot repair a wrong measurement boundary.
See [options](https://bheisler.github.io/criterion.rs/book/user_guide/command_line_options.html)
and [configuration](https://bheisler.github.io/criterion.rs/book/user_guide/advanced_configuration.html).

### Go testing and benchstat

Go's `B.Loop` coordinates calibration and timing, with compiler support
against eliminating work. Benchmark names can encode input cases;
`ReportAllocs` and `-benchmem` expose allocation metrics.
That support is more than an ordinary function named Loop.
See [testing](https://pkg.go.dev/testing#B.Loop).

Benchstat consumes repeated benchmark records, reports medians and confidence
intervals, and compares independent samples with a Mann–Whitney U test.
Its documentation recommends repeated runs, including interleaving configurations
to reduce environmental bias. It does not make a historical file a randomized experiment.
See [benchstat](https://pkg.go.dev/golang.org/x/perf/cmd/benchstat).

A rank test asks about distributions; calling its result a median test
requires additional shape assumptions. A small p-value says neither how
large the difference is nor whether it matters.
Ties, dependence and small sample counts need explicit treatment.
See [Mann–Whitney assumptions](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.mannwhitneyu.html).

### JMH

JMH separates warmup iterations, measurement iterations and forked JVMs.
Its samples expose these controls as annotations and command options.
A benchmark method can return a result or use a consumer; unused pure
work may disappear, making a low time misleading rather than impressive.
See [iteration controls](https://github.com/openjdk/jmh/blob/master/jmh-samples/src/main/java/org/openjdk/jmh/samples/JMHSample_20_Annotations.java)
and [dead code](https://github.com/openjdk/jmh/blob/master/jmh-samples/src/main/java/org/openjdk/jmh/samples/JMHSample_08_DeadCode.java).

Its profilers provide metrics separate from ordinary elapsed timing.
JMH's JVM machinery does not automatically transfer to hd's V8/Wasmtime
execution or establish that three Wasm activations reach a steady tier.
See [profiler examples](https://github.com/openjdk/jmh/blob/master/jmh-samples/src/main/java/org/openjdk/jmh/samples/JMHSample_35_Profilers.java).

### pyperf and timeit

pyperf distinguishes calibration, workers, warmups and measured values;
workers run sequentially. It preserves JSON records and exposes raw data,
histograms and instability checks, including process metadata.
Its strength is reproducibility infrastructure, not an assurance that a
shared machine or bimodal workload becomes trustworthy through repetition.
See [running](https://pyperf.readthedocs.io/en/latest/run_benchmark.html)
and [analysis](https://pyperf.readthedocs.io/en/latest/analyze.html).

Python timeit separates setup and repeated timing, with an explicit
GC policy affecting what is measured. hd's RC and cycle collection differ;
copying a foreign harness's default collector policy would change the experiment.
See [timeit](https://docs.python.org/3/library/timeit.html).

### Optimization resistance and engines

Rust's `black_box` is a best-effort optimization hint, not a correctness
or security guarantee. Inputs and results both need attention: consuming a
constant-folded result cannot restore the missing operation.
See [black_box](https://doc.rust-lang.org/std/hint/fn.black_box.html).

V8 uses Liftoff for fast compilation and TurboFan for hot code; background
tiering and call boundaries affect warmup. Its documented experimental flags
can change that policy, so record actual Node/V8 versions and flags.
A flat-looking trace does not prove which tier executed.
See [V8's pipeline](https://v8.dev/docs/wasm-compilation-pipeline).

Wasmtime exposes compilation strategies such as Cranelift and Winch;
it must not be described as automatically following V8's tier-up sequence.
The pinned hd path configures Cranelift optimization before loading modules
and caches compilations within its engine context.
Record strategy/version/cache policy separately from hd optimization level.
See [strategies](https://docs.wasmtime.dev/api/wasmtime/enum.Strategy.html),
[hd's engine](../../crates/hd_run_wasmtime/src/engine.rs), and
[module cache](../../crates/hd_run_wasmtime/src/cache.rs).

## Semantics and measurement boundaries

The benchmark runner is a consumer of hd semantics, not permission to
weaken them. It does not add implicit Clock requirements to arithmetic,
parsing, HTTP sending or any other operation solely because it is measured.

| Concept and rule | Handling | Failure of a naive harness |
| --- | --- | --- |
| [Generics](../../spec/lang/04-type-system.md#generics) and [associated types](../../spec/lang/09-traits.md#associated-types) | Instantiate each concrete input/operation registration; preserve its declared bounds and result checking. | Timing a generic function value's lookup or setup instead of the concrete algorithm measures the wrong work. |
| [Trait values](../../spec/lang/09-traits.md#trait-value-references) | Dynamic dispatch is allowed and included; record when an implementation comparison changes dispatch mode. | Comparing an inlined concrete call against an erased call attributes dispatch overhead to the algorithm. |
| [Value copies](../../spec/lang/04-type-system.md#r-types.coll.value) and [references](../../spec/lang/13-values-and-references.md#places-and-new-objects) | Fresh input means an actual intended semantic copy or regeneration; aliases are not resets. | Repeated pop sees lengths 3 then 2, while fresh copies preserve the original (P3). |
| [Writable places](../../spec/lang/13-values-and-references.md#writable-places-of-references) | Destructive atoms take writable input explicitly; no hidden ownership upgrade. | A reference to the original dataset changes later samples instead of preparing independent inputs. |
| [Bindings and shadowing](../../spec/lang/03-names-and-scopes.md#lexical-scopes) | Registration captures actual binding identities; report input fingerprints supplied by the author. | Reusing a label after shadowing a size variable does not prove the workload stayed identical. |
| [Captures](../../spec/lang/07-functions.md#copied-and-shared-captures) | Construct captures per worker; shared mutable captures persist between its batches unless explicitly reset. | A closure's cache or counter makes later samples cheaper; recreating the closure at registration once does not reset every operation. |
| [Rows](../../spec/lang/11-requirements-and-suspension.md#requirement-polymorphism) | Keep the work's actual row in its function type; the outer runner owns timing/report authority. | Installing ManualClock around the whole runner measures virtual sleep rather than elapsed execution (P1). |
| [Cold suspensions](../../spec/lang/11-requirements-and-suspension.md) | Create rerunnable work; await each invocation and include completion. | A plain call constructs a suspension without running it; P4's count remains zero until driven. |
| [Branches, loops and exits](../../spec/lang/06-control-flow.md) | Every declared operation must complete or invalidate its batch; no selective timing of only a successful branch. | `break`, `return` or `?` before a batch ends cannot leave the original iteration denominator intact. |
| [Defer/using](../../spec/lang/06-control-flow.md#deferred-cleanup) | Include cleanup performed before callback completion; cancellation is not successful completion. | Stopping the timer when cancellation is requested excludes disposal that has not finished. |
| [Panics](../../spec/lang/06-control-flow.md#runtime-panics) | A panic poisons that worker; retain its output and fail the case. | Catching a host trap and reusing the same guest instance violates instance failure semantics. |
| [Element references](../../spec/lang/13-values-and-references.md#element-references) | A batched input can be passed by element reference only for that call; preserve pinning and escape rules. | Growing a retained-input list while a callback holds an element reference changes the access contract. |
| [Modules, visibility and tests](../../spec/lang/10-modules.md) | Explicit executable roots import ordinary helpers; tests do not execute benchmark bodies. | Discovering every function named bench, including dependency/private helpers, changes the language's name and execution rules. |

No source move semantics or destructor mechanism is added. The default
interval includes callback-produced values' ordinary releases; cycle collection
runs according to the normal runtime policy. A different release boundary
is a different experiment and must be labeled and bounded.

## Three mechanisms

**A: ordinary suite registration plus an explicit barrier.** Each `benches/`
root is an ordinary executable registering library callbacks, then entering
one terminal runner. The CLI manages workers, builds, records and analysis.
This supports all four use cases without compiler-known registration syntax.

**B: compiler-known benchmark registration.** Add a distinct recognized
registration item beside tests and emit callable case entry points.
This could simplify discovery and avoid executing registration for listing,
but adds language-tier position/metadata rules and compiler pipeline work.
It still needs timing, process isolation, analysis and optimization resistance.

**C: whole-program process measurements, the smallest option.** Each ordinary
program prints a checked answer and is timed as a process.
This can support all four use cases for sufficiently coarse workloads,
including old compilers, while honestly including startup and compilation.
It cannot substitute for fine operation or setup-excluded measurements.

| Fixed use case | A: ordinary suite | B: recognized registrations | C: process programs |
| --- | --- | --- | --- |
| U1 | Operation-level tuning with explicit boundaries | Same | Coarse end-to-end work only |
| U2 | Paired workers and checked outcomes | Same statistical infrastructure | Same analysis, different process metric |
| U3 | Requires compatible suite protocol/API | Requires compatible compiler registration | Existing ordinary-entry compatibility is widest |
| U4 | Named variants, seeded inputs, batched destructive setup | Same, compiler metadata | Manual input/iteration plumbing |
| Costliest change | One compiler-known barrier intrinsic; library/CLI elsewhere | Barrier plus language-tier registration semantics | CLI/script machinery only |
| Soundness and interpretation | Must preserve rows, completion and boundaries | Same plus generated-entry mapping | Simpler, but never claim inner-loop precision |
| Writing/reading cost | Ordinary functions and explicit labels | Another recognized call contract | Ordinary programs, repeated harness boilerplate |

AGENTS.md's cost order favors C when it answers the actual question.
For the requested operation-level tool, A avoids B's additional language
mechanism. No new keyword or `bench:` grammar is proposed.

## Recommendation: ordinary registration, timed batches

Recommend A, with explicit limitations until the barrier and timing boundaries
are proven. Register all cases before measurement; reject duplicate qualified
labels, invalid units and incompatible options before invoking work.
A label includes package, root and explicit parameter identity.

The API below is proposed, not present in std. `Benchmarks[W]` has an
ordinary type parameter, never a row parameter.
Here `B(R)` is explanatory shorthand for the complete batch function type
`fn!(&mut dyn Clock, usize) -> Result[Batch, BenchError] $ R`, not new hd syntax.

| Public item | Proposed complete signature or fields |
| --- | --- |
| `Units` | `Operations(count: usize)`, `Bytes(count: usize)`, `Items(count: usize)`; count must be positive and describes one callback invocation. |
| `BenchError` | `Invalid(message: string)`, `Work(message: string)`, `Clock(message: string)`, `Limit(message: string)`, `Protocol(message: string)`; derives Error through existing mechanisms. |
| `BenchError::from_error` | `fn from_error[E < Error](error: E) -> BenchError`; preserves the display report for measurement failure, without changing the operation's typed error API. |
| `Benchmarks::new` | `fn new[$R]() -> &mut Benchmarks[B(R)]`; R is inferred or explicitly supplied from callbacks/expected type. |
| `add` | On `Benchmarks[B(R)]`: `fn add(&mut self, name: string, units: Units = .Operations(1), work: fn!() -> Result[void, BenchError] $ R) -> void`. |
| `add_batched` | On `Benchmarks[B(R)]`: `fn add_batched[S](&mut self, name: string, setup: fn!() -> Result[S, BenchError] $ R, units: Units = .Operations(1), max_batch: usize = 1024, work: fn!(&mut S) -> Result[void, BenchError] $ R) -> void`. |
| `run` | `fn run![$R](suite: &Benchmarks[B(R)]) -> Result[void, BenchError] $ R + Clock + Console + Args`; one terminal, with explicit R permitted. |
| `black_box` | `fn black_box[T](value: T) -> T`, initially supported only for scalar representations proven by each backend; unsupported concrete types are rejected with a proposed `benchmark-value-unsupported` diagnostic. |

`Batch` is harness data: completed count, positive elapsed milliseconds,
setup/release policy and validation status. It is returned by generated
ordinary adapter closures, not constructed by benchmark users.
`add` creates one adapter around the work; `add_batched` captures its
concrete S internally before storing the common batch function type.

All callbacks in one suite use the same explicitly compatible row union R.
The measured operation keeps its own actual requirements; widening an adapter
for a mixed suite is not an implicit Clock requirement on that operation.
The runner adds Clock, Console and Args only because it reads timing/options
and reports. A pure suite's work row can be empty.

Current syntax/type evidence supports storing full function types in a
normal type parameter. Inference through an invariant container needed an
explicit row argument in P2. Prove the future constructor's nested-row
inference before promising it.
No new row-parameterized data-type feature is required or recommended.

The proposed user form is shown in P5. It parses but fails checking with
`unknown-module: no module named std.bench`, the exact current API gap.
The scalar barrier additionally needs an approved compiler-known intrinsic;
an ordinary identity implementation cannot satisfy its intended purpose.

### Clock and requirement rows

Capture the real host Clock before applying any callback-specific provider.
Pass its reference explicitly to the batch adapter; do not look up a
possibly replaced Clock again around each operation.
A body using ManualClock then changes virtual scheduling, not the meter.

[Clock](../../spec/std/time.md#clock) already provides monotonic readings;
there is no new timing capability in this design.
[Instant](../../lib/std/time.hd) stores whole milliseconds. Retain that contract
initially and use long batches; calculated ns/op does not imply a nanosecond clock.

Reject a decreasing meter or a meter that cannot advance within the bounded
calibration. Record the observed positive step q, not a claim about the
clock's true hardware resolution. Provider separation is directly exercised
in P1; its zero real-ms result also demonstrates why a tiny batch is unsuitable.

## Statistics

### What is estimated and reported

For batch s in worker p, record elapsed time `t[p,s]` and completed
callback invocations `n[p,s]`. For the default one-operation callback, compute
`y[p,s] = t[p,s] / n[p,s]`.
The primary standalone estimate is the median of the worker medians:
`theta = median_p(median_s(y[p,s]))`.
Every process receives equal weight, regardless of its calibrated iteration count.

When Units declares k logical operations per invocation, normalize the displayed
cost by `n × k`. Byte/item workloads display time per invocation and their
throughput separately; do not label that time as per-byte latency. Record
the denominator and use the same normalized metric for comparisons.

Show each worker median, aggregate median, unscaled median absolute deviation,
IQR, min/max, raw batch values, and a 95% interval for theta.
Report the estimator explicitly: this is not total elapsed divided by all
operations, a mean latency, or a percentile of individual requests.
A batch average cannot establish individual-request p95 or p99.

Use a seeded, 10,000-resample percentile bootstrap of whole worker clusters.
Resample worker medians with replacement, recompute theta, then use the
2.5th and 97.5th percentiles. Define quantiles by linear interpolation at
`(N-1) × p`, so implementations and stored reports agree.

This is an approximate frequentist confidence interval, not a credible
interval or a 95% posterior probability about this run's true cost.
It assumes sufficiently independent, representative worker outcomes.
Ten workers are a starting budget, not proof that this assumption or
nominal coverage holds. Degenerate or sparse distributions must be flagged.

Percentile bootstrap is simple but can have poor finite-sample coverage;
BCa and other methods address different limitations and require validation.
The first implementation must pass synthetic/reference tests before labeling
its intervals; it must not substitute thousands of correlated batches for workers.
See [bootstrap methods](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.bootstrap.html).

### Outlier detection

Compute Tukey-style fences on log-positive batch costs within each worker:
Q1/Q3 plus 1.5 IQR for mild and 3 IQR for severe flags.
Keep every valid observation; report the flagged counts and raw locations.
A pause may be real runtime work, collection, scheduling or tiering.

When IQR is zero, report that quantization/degeneracy explicitly rather
than silently deleting every differing sample. A flagged observation is not
an invalid sample. Wrong output, failed status or a missing count is invalid
and fails the case instead of becoming a convenient outlier exclusion.

### How many samples is enough

The normal proposal collects ten fresh workers with ten retained batches each.
The independent-unit count is ten, not one hundred.
Quick mode uses three workers/five batches and is explicitly exploratory;
it cannot produce a CI regression pass.

The proposed precision target is a relative interval half-width of 2%;
worker-median MAD above 10% of the median also flags instability. These
are reviewable defaults, not empirical accuracy claims.

A local interval wider than the requested precision, a warmup drift flag,
or too few completed workers produces an inconclusive result.
Do not keep adding workers until a favorable p-value appears.
Choose the worker/sample budget before retention; a larger budget is a new,
fully recorded experiment, with its earlier result preserved.

No universal sample count answers every effect size, tail distribution or
machine. Calibrate these defaults on actual runner machines and report
repeatability before recommending a CI threshold for a project.

## Warm-up and calibration

### Engine tiering

Within one worker, reuse the same compiled module and instance for a
selected operation benchmark. Fresh workers reconstruct captures and host state.
Instantiation, loading and compilation are recorded separately, outside the
steady-state batches; a startup experiment measures them explicitly instead.

Warmup repeatedly invokes the actual case, including its input/result
consumption. Start with one operation and increase bounded iteration counts.
One giant call does not demonstrate that all relevant functions have tiered;
inspect the warmup trace, versions and optional profiler evidence.

V8's Liftoff/TurboFan transition can continue during measurement; a fixed
number of discarded calls is no steady-state proof.
Wasmtime's selected Cranelift or Winch configuration has a different lifecycle;
loading/compilation caches and host page/cache warming still matter.
Do not change engine flags secretly to make one implementation look better.

### Sampling and setup

Proposed defaults are a minimum three-second warmup, then calibrated batches
lasting at least `max(250 ms, 100 × q)`. Prepared inputs default to
a maximum batch of 1,024, with an iteration-count ceiling of `2^30`.
A ten-second warmup cap and ninety-second standalone case limit prevent
unbounded calibration; insufficient data remains inconclusive.

Zero-duration early calibration batches are retained and trigger more iterations;
they are never divided into ns/op. Calibration doubles until the target
is reached, using checked
arithmetic and a per-callback watchdog. Then freeze that worker's iteration
count for all retained samples. If the operation is slow, one invocation
may be a complete sample; record that fact and its actual duration.

After warmup, compare the medians of its last two equal-size windows.
A difference above 5%, or strong drift between the first/last retained
sample halves, adds an instability flag. These proposed heuristics do not
prove stationarity, and all warmup/calibration values remain in the raw record.

Preparation and warmup can perform real network, filesystem or application
effects. Benchmark authors must explicitly provide disposable/resettable services
and inputs; the harness cannot infer whether repetition is safe.
No failure is retried invisibly, including a failed warmup validation.

The ordinary atom includes loop, callback, barriers, validation written in
the callback, release and normal collector work completed before it returns.
Do not subtract an empty loop, timer, import or startup constant.
An empty control case is reported alongside, never subtracted automatically.

The batched atom prepares a bounded list of fresh S values before timing,
then calls work through each writable element place. Input and output
retention policy is recorded; it releases prepared inputs after the timed
interval in this atom. An end-to-end allocation benchmark should instead
prepare and release its data inside the ordinary callback.

If the prepared-input count cap prevents reaching the timing target, report
insufficient timer granularity; do not silently include setup or reuse mutated
inputs. Authors may explicitly increase the bounded pool or choose a different
end-to-end experiment.

A prepared-count bound is not a byte-accurate heap bound for arbitrary S.
Use existing host/runtime limits where available and fail cleanly on exhaustion;
otherwise disclose that the memory limit is unverified.
Do not retain an unbounded result list to hide destruction cost.

### Keeping pure work present

Recommend explicit scalar `black_box` input and result barriers, rather than
special treatment of all code inside a magic loop.
The intrinsic returns the same scalar value and creates no reference,
ownership transfer, or provider requirement. Unsupported representations fail
rather than falling back to an ordinary inline identity.

Its implementation must remain opaque across hd optimization, inlining and
engine optimization. A host import can supply opacity but adds measurable
call/wire overhead; a Wasm local identity does not supply that property.
The owner must approve its lowering and supported types before implementation.

Per-operation barrier/import costs remain included and must be exposed by
an empty/control benchmark. No zero-overhead claim is made.
Inspect emitted code on both engines for constant folding, hoisting and
unused-result removal before permitting pure-microbenchmark claims.
This barrier has no cryptographic or program-correctness guarantee.

## Comparing an old and a new implementation

### Saved baselines

`--save-baseline PATH` writes a new versioned result file only when explicitly
requested; existing files require `--replace-baseline` explicitly.
`hd bench compare OLD NEW` reads both files without compiling or running code.
It displays code/runtime differences and rejects mismatched workload boundaries,
units, parameters, profiles, engine configuration or schema.

Changing code or the compiler is the intended comparison axis, so those
hashes must be displayed rather than blindly required equal.
Keep a separate workload contract fingerprint: comparison key, input/seed, units,
setup/release policy, validation contract and measurement mode.

The comparison key defaults to the qualified case label. An explicit `--pair`
mapping assigns its two variant labels one shared key, while requiring every
other contract field to match. This permits U4 without inferring equivalence
from similar names.

A changed contract is a new experiment unless explicitly reviewed as compatible.

Historical comparisons report median ratios, absolute changes, approximate
intervals and an optional Mann–Whitney test on worker medians.
Use a validated implementation with tie handling and explicit insufficient-data
results; do not use the deprecated benchstat library as an implicit guarantee.
Independent-sample rank tests are not paired tests or proofs of equal medians.

### Interleaved A/B runs

For a fresh comparison, freeze both executables/artifacts before measurement.
Choose a seed and precompute balanced AB/BA order for twenty process pairs.
Each member independently warms and measures; run one member at a time
on the same host. Compilation is outside operation timing.

For pair i, let `d[i] = log(median(B_i) / median(A_i))`.
Report `rho = exp(mean(d))`, the geometric mean of paired ratios,
plus absolute differences and each member's worker median.
This differs from the standalone median estimator and must be labeled.

Bootstrap whole pairs, keeping A/B members together, to obtain the paired
ratio interval. Pairing reduces slow drift but cannot eliminate thermal
carryover, external changes or order effects. Report AB and BA subgroup
ratios and a time-order trace; disagreement flags the result as unstable.

Compiler comparisons freeze source/input/harness versions and record both
compiler/stdlib/artifact hashes. Implementation-variant comparisons additionally
record each case label and the explicitly declared pair mapping.
No automatic matching by similar names or disappearance of a missing case.

### Statistical change detection and a CI threshold

Default comparisons are report-only. An explicit `--fail-regression 3%`
requests an opt-in, proposed CI policy after the project's repeatability study.
It is not a default assurance based on the old thirty-sample proposal.

For one predeclared case, use a 99% paired interval. A lower bound above
1.03 is regression; an upper bound below `1/1.03` is improvement.
An interval entirely inside `[1/1.03, 1.03]` is equivalent within that practical
band. Everything else is inconclusive, not "no regression".

Require at least ten complete pairs, the planned sample budget, compatible
metadata and no instability/validation flags before these decisions.
A point estimate crossing 3%, or a p-value below 0.01 alone, is insufficient.
Do not switch between paired and unpaired tests according to which wins.

For K predeclared cases, use per-case alpha `0.01/K` and disclose the
Bonferroni-style adjustment. Choose resamples before seeing results, at least
`max(10000, 5000 × K)`, capped at one million; exceeding the cap is inconclusive.
That controls Monte Carlo tail resolution, not finite-sample bootstrap validity.

The nominal family error target remains approximate and assumption-dependent;
it must be validated with repeated no-change experiments on the real runner.
Do not repeatedly rerun until green or combine partially collected runs
without declaring the selection and preserving all attempts.

## Noise control

Run cases and workers sequentially; `--jobs` can parallelize builds only.
Record worker order, wall timestamps, elapsed/CPU time where available,
load and logical CPU count, OS/architecture, engine versions/flags and limits.
Record known instrumentation, compilation/module-cache policy and benchmark seed.

Collect an allowlisted context, never arbitrary environment variables or secrets.
Windows lacks Unix load averages: represent unavailable metrics as null with
a reason, not a fabricated zero. An unknown affinity/governor state is unknown.
The tool does not change power settings, affinity, caches or background processes.

Flag known oversubscription, unexpected concurrent own workers, warmup/retention
drift, process spread above the configured tolerance, and insufficient samples.
Persist all samples despite warnings; a contaminated run can be useful evidence
without qualifying for a CI performance pass.

A median and more samples do not remove systematic bias. Do not launch
all A workers followed by all B workers, discard slow workers, choose the
best run, or claim fresh processes clear disk/service caches.
Fresh process, fresh instance and cold machine are different conditions.

## The rest of the state of the art

| Topic | Initial behavior or explicit exclusion |
| --- | --- |
| Parameterized inputs and sizes | Ordinary code registers exact labels/parameter maps; record seed, encoded parameter values and workload fingerprint. No new generic benchmark syntax. |
| Throughput | Declare positive operations/bytes/items per invocation; report units/s using completed counts and elapsed time. Reject zero, overflow and mismatched units. |
| Allocation counts | Separate opt-in diagnostic experiment only after a defined counter boundary exists. Normal timing cannot infer counts from RSS or RC retains. |
| Memory | Record worker peak RSS where supported, labeled process peak; it includes runtime/engine and is not peak live guest bytes. Unknown is null. |
| RC and cycle collection | Include normal runtime policy by default; do not disable collection silently or call retained/released counts allocator counts. |
| Async/suspending bodies | Await each operation, include scheduler/host waits, invalidate timeout/cancellation; no concurrent throughput mode initially. |
| Returned large objects | Author validates/consumes them inside the callback; default release is included. Setup-excluded atoms disclose deferred prepared-input release. |
| Parallel work | Allowed inside the measured operation with its declared providers; runner parallelism stays one. Record concurrency separately from operation count. |
| Histograms and request tail latency | Raw batch/worker traces included; individual-operation p95/p99 excluded until a separate per-request measurement mode is designed. |
| Profiler/perf counters | Explicit external profiler runs with matching labels; instrumentation results are not silently merged into ordinary elapsed timings. |
| Browser | Initial CLI workers are Node/Wasmtime only. Browser background throttling/isolation and a browser worker protocol are a separate design, not claimed parity. |
| Machine-readable output | Versioned parent JSON records, raw samples and final summary; captured user output is data, not extra stdout JSON. |

Go's `-benchmem` motivates useful allocation reporting, but hd's collector,
allocator and host allocations need their own counter definitions.
The current study's checked-mode allocation/page encoding is a diagnostic
local-patch path, not an available public benchmark API or a timing estimate.
That metric is explicitly deferred rather than promised by the first command.

## Architecture

### Pipeline and data structures

Keep sampling/analysis in CLI-side Rust modules such as `bench_cmd`,
`bench_worker`, and a testable `bench_stats` module.
Use `hd_driver`'s ordinary executable build path and existing engine/profile
selection. New roots are CLI discovery, not new TIR case tags.

The std suite produces ordinary closures/loops. Its registration adapters
hold concrete setup types internally and expose a common full batch-function
type. This avoids new row-parameterized data types, heterogeneous erased
error channels, and compiler enumeration of magic benchmark functions.

Only `black_box` needs a compiler-known intrinsic, with a verifier/lowering
contract for supported scalar types. It must preserve value and ownership
semantics and survive the inliner; normal programs that do not reference
it must not link its helper/import.

| Structure | Fields |
| --- | --- |
| `CaseDescriptor` | Package/root/label, canonical parameter map, seed, Units, boundary version, validation-contract fingerprint, comparison key, workload and implementation hashes. |
| `RunPlan` | Selected cases/pairs, engine/profile/compiler artifacts, grants, process/sample/warmup limits, fixed order seed, bootstrap method/alpha/count and CI policy. |
| `WorkerRecord` | Case, variant, worker/pair/order IDs, host context, load/timestamps, setup/compile/instantiate durations, warmup/calibration trace and status. |
| `Sample` | Worker and sample ordinal, completed invocations and declared units, positive elapsed integer milliseconds, flags and setup/release policy. |
| `Estimate` | Estimator/method/version, independent-unit count, median or paired ratio, interval level/bounds, spread, outlier flags and inconclusive reasons. |
| `ArtifactIdentity` | Compiler binary/stdlib/profile/target/engine settings, source/dependency digests, Wasm bytes hash and harness protocol version. |

Use vectors of fixed sample records and shared worker/case tables, rather
than duplicating metadata strings for every batch. Decode and validate counts
with checked integers; serialize large integers as decimal strings when
JSON's safe integer range would otherwise lose information.

```mermaid
flowchart LR
  S[Selection and fixed run plan] --> B[Build both variants once]
  B --> W[Sequential fresh workers]
  W --> M[Warmup and calibrated batches]
  M --> V[Validate frames and process outcome]
  V --> R[Persist all raw records]
  R --> A[Cluster or paired analysis]
  A --> O[Text or versioned JSON summary]
```

### Core sampling pseudo-code

This is a proposed algorithm, not a current internal API:

```rust
fn worker(case, plan, real_clock) -> WorkerRecord {
    let meter = retain(real_clock);
    let mut record = WorkerRecord::start(case, plan);
    let q = observe_positive_step(meter, plan.clock_deadline)?;
    let target = max(plan.min_sample_ms, 100 * q);
    let mut n = 1;
    while record.warmup_elapsed() < plan.warmup_ms {
        let sample = case.batch(meter, n)?;
        validate_count_and_nonnegative_elapsed(&sample, n)?;
        record.keep_warmup(sample);
        n = bounded_double_if_short(n, target, plan)?;
        check_deadline_and_limits(plan)?;
    }
    while record.last_warmup_or_calibration_ms() < target {
        let sample = case.batch(meter, n)?;
        validate_count_and_nonnegative_elapsed(&sample, n)?;
        record.keep_calibration(sample);
        n = bounded_double_if_short(n, target, plan)?;
        check_deadline_and_limits(plan)?;
    }
    record.freeze_iterations(n);
    for index in 0..plan.samples {
        let sample = case.batch(meter, n)?;
        validate_completed_count_and_elapsed(&sample, n)?;
        record.keep_retained(index, sample);
        emit_framed_record_outside_timing(&record, index)?;
        check_deadline_and_limits(plan)?;
    }
    record.add_drift_and_outlier_flags();
    record.finish()
}
```

The supervisor records failed/partial evidence if the worker traps or exits;
`?` in this sketch never turns failure into a successful partial average.
A callback that never returns is cut off by the supervisor watchdog.
Setup and frame emission are outside each interval but remain in total
worker wall time and can influence subsequent cache state.

### Comparison pseudo-code

```rust
fn compare_paired(plan, workers) -> Comparison {
    verify_artifact_axes_and_workload_contracts(plan, workers)?;
    let pairs = complete_valid_pairs_in_planned_order(workers)?;
    if pairs.len() < plan.minimum_pairs { return Inconclusive("too few pairs"); }
    if !planned_budget_completed(plan, &pairs) { return Inconclusive("partial plan"); }
    if has_instability_flags(&pairs) { return Inconclusive("unstable run"); }
    let logs = pairs.map(|p| log(
        median(batch_costs(p.b.samples)) / median(batch_costs(p.a.samples))));
    let estimate = exp(mean(&logs));
    let mut rng = SeededRng::new(plan.analysis_seed);
    let mut boot = Vec::with_capacity(plan.resamples);
    for _ in 0..plan.resamples {
        let whole_pairs = resample_with_replacement(&logs, &mut rng);
        boot.push(exp(mean(&whole_pairs)));
    }
    let alpha = plan.family_alpha / plan.predeclared_case_count;
    let low = quantile(&boot, alpha / 2);
    let high = quantile(&boot, 1 - alpha / 2);
    let upper_equivalence = 1 + plan.threshold;
    let lower_equivalence = 1 / upper_equivalence;
    let verdict = if low > upper_equivalence { Regressed }
        else if high < lower_equivalence { Improved }
        else if low >= lower_equivalence && high <= upper_equivalence { Equivalent }
        else { Inconclusive("interval crosses a practical boundary") };
    Comparison { estimate, low, high, verdict, method: plan.analysis_version }
}
```

With P pairs, S batches and B resamples, worker medians cost O(P×S log S).
Paired bootstrap and quantiles cost O(B×P + B log B).
It is outside measured execution. Bound all vectors/resample counts before
allocation; retain the random seed and analysis version for reproducibility.

### Protocol, caching and platforms

The ordinary suite uses existing Console for framed result records outside
timing. The parent captures child stdout/stderr and extracts only versioned,
nonce-tagged frames; all other bytes remain captured program-output data.
That nonce prevents accidental collisions, not a malicious author forging results.
Benchmark code is trusted to describe its own workload; this is no attestation boundary.

Default captured stdout/stderr limits are 8 MiB each per worker. Parse frames
while spooling; exceeding either limit fails the worker with explicit truncation
metadata. Keep the bounded capture and prior valid sample frames, rather than
silently dropping output to obtain a successful estimate.

The parent owns final stdout JSON, validates frame count/order/schema/units,
and fails missing, duplicate or malformed records. A crash after earlier frames
preserves them but fails the worker/case. Test escaping, arbitrary user output,
partial lines, Windows CRLF, Unicode and failed process exits.

Ordinary build caches use their existing compiler/source/stdlib/engine keys.
A compiled worker artifact is immutable during its run; analysis changes do
not rebuild it, and old raw records can be reanalyzed explicitly.
Results carry their independent schema/method versions, not compiler cache keys.

macOS, Linux and Windows follow the same timing/record contract with available
host metrics labeled. Node and Wasmtime require separate platform/engine results;
never aggregate their timings. This design does not require native code-cache
serialization or new unsafe host APIs.

## Command, configuration and scope

Proposed commands, all unavailable today:

```sh
hd bench --filter parse/1024 --engine node --format json
hd bench benches/parse.hd --save-baseline before.json
hd bench compare before.json after.json --format json
hd bench --filter critical --against-compiler /tools/old-hd --pairs 20 --fail-regression 3%
hd bench --pair concat/old/4096,concat/new/4096 --engine wasmtime
```

Default scope is the current package's `benches/*.hd` roots; `-p NAME`
selects workspace members using existing selection conventions.
A FILE selects one root, not arbitrary dependency benches. Helper modules
are imported explicitly; no automatic recursion into dependency benchmark roots.

`--list` executes registration in selected roots but no benchmark callbacks;
it is not static source inspection. Registration can execute ordinary code
under its grants, so listing is not claimed effect-free.
`compare` only reads result files and never loads benchmark code.

```toml
[bench]
processes = 10
samples = 10
warmup = "3s"
min_sample = "250ms"
case_limit = "90s"
precision = "2%"
```

`--precision` overrides the interval-width target; it does not promise the
requested precision can be achieved within a fixed case budget.

CLI values override the selected package's manifest; otherwise use documented
defaults. No implicit parent/workspace inheritance. Reject unknown keys, zero
counts, unsupported engines, malformed durations/percentages, missing pair members,
integer overflow and conflicting comparison axes before measurement starts.

`--processes`, `--samples`, `--warmup`, `--min-sample`, and `--case-limit`
provide exact CLI overrides for those keys. Profile selection follows ordinary
build/run: dev by default, with explicit `--release` for optimized measurement.
Record that choice; a dev result does not predict optimized performance.

Quick mode explicitly selects its lower exploratory budget. CI paired mode
uses twenty planned pairs and a default 300-second per-case limit; no fixed
budget guarantees it finishes for slow work. An unfinished plan is inconclusive.
The CLI must display these potentially substantial planned costs before running.

Use existing `--cap` selection for body authority. Timing/reporting requirements
belong to the executable runner; operations keep their own rows.

The harness itself adds no network access, privilege escalation or power tuning;
it writes normal build artifacts and explicit result destinations. Benchmark code
can use its granted providers, as any ordinary program can. It is not a
read-only sandbox merely because its purpose is measurement.

### Reports and comparison

Text shows case/parameters, metric boundary, engine/profile, worker/batch counts,
estimator, interval and spread, warnings and verdict.
A result such as `ratio 1.050 [1.048, 1.052], regression >3%` is a
comparison statement, not a standalone ns/op value or a p95 latency.

Parent JSON uses `schema: "hd-bench/1"` and record kinds `plan`, `worker`,
`sample`, `estimate`, `comparison`, `output`, `failure`, and final `summary`.
Each sample references case/worker IDs; each estimate names its method, unit,
confidence level, independent-unit count and raw-record references.

Preserve integer elapsed milliseconds and operation counts, converted ns/op
and units/s, the full run plan, flags and all input/artifact/host identities.
Output records escape user stdout/stderr; no raw child text leaks into the
parent's machine-readable stdout. Unknown metrics are null with reasons.

| Final condition | Exit status and summary |
| --- | --- |
| Valid measurement or report-only comparison | 0, with any inconclusive/instability flags visible |
| Work/validation/worker failure | 1; failed evidence retained, no successful estimate for that case |
| Explicit CI policy: regression or inconclusive | 1; distinct verdict/reason, never an ambiguous green pass |
| Usage, configuration, incompatible requested comparison, internal failure | 101; diagnostic and final failed summary |

Baseline writes are explicit and atomic after schema validation; compare
never overwrites either input. A stale/incompatible file is rejected instead
of silently treated as a previous successful measurement.
Conformance should judge this contract, without flaky assertions about elapsed speed.

## Extension points

Authors provide ordinary callback functions, prepared-input generators, typed
error conversion, parameters, seeds and units. They can implement algorithm
variants without a compiler plugin or magic function name.
Registering code uses existing permissions; adding a benchmark is executing code,
not consuming trusted declarative metadata.

Analysis consumes versioned JSON, so external tools can draw histograms,
compute alternative statistics or integrate dashboards without loading code
inside the compiler. An external conclusion must name its method and cannot
masquerade as the built-in CI verdict.

Custom host counters/profilers are later explicit, versioned measurements,
with their authority and perturbation priced separately.
Browser workers and concurrent-request latency modes need separate designs.
Neither is a hidden first-release promise or a reason to grow the core language.

## Cost

These are bottom-up production allowances, not measured implementation size
or runtime speed. Existing literal line counts include comments/blanks and
are measured with `wc -l` at the proposal base.

| Existing component | Measured lines | Code replaced/deleted initially |
| --- | ---: | ---: |
| `bench/runtime/run.mjs` | 143 | 0; preserve historical coarse experiments |
| `bench/study/run.mjs` | 447 | 0; preserve process/warm-module comparisons |
| `crates/hd_cli/src/test_cmd.rs` | 1828 | 0; reuse ordinary selection/build/report patterns, no second test engine |
| `lib/std/testing.hd` | 731 | 0; benchmarks remain separate from test registration |
| `lib/std/time.hd` | 799 | 0; existing millisecond Clock/Instant retained |

The downloaded Go `cmd/benchstat/main.go` is 538 literal lines, SHA-256
`b43e6f48691e2b298afe8a8ebe79d70d179522192c4bb39d0d372ccd6db213c1`.
It excludes its parsing, table and statistical dependencies; it is not
a 538-line implementation of this whole feature.
The [upstream file](https://github.com/golang/perf/blob/master/cmd/benchstat/main.go)
and a 100-line JMH iteration-control example are retained byte-for-byte as scale references.

| Component | Production allowance | Contents |
| --- | ---: | --- |
| CLI plan/options/discovery | 180 Rust lines | Scope, conflicts, manifests and compatible comparison axes |
| Worker supervisor and framed protocol | 240 Rust lines | Sequential scheduling, watchdogs, captures, validation and failure records |
| Raw results/baseline I/O | 160 Rust lines | Versioned schema, atomic saves, hashes and explicit compare input |
| Estimators/outlier/bootstrap analysis | 220 Rust lines | Pure bounded vector algorithms, method versions and synthetic-reference tests |
| Pairing/CI policy | 120 Rust lines | Balanced order, thresholds, multiplicity and inconclusive results |
| std registration and batch atoms | 220 hd lines | Concrete captures, typed failures, timing provider and reset/release boundaries |
| Scalar intrinsic registration/lowering | 140 Rust lines | Supported types, opaque barrier, verifier and both-engine boundary |
| Text/JSON reporting adapter | 100 Rust lines | Existing diagnostic conventions plus benchmark records |
| **Total** | **1160 Rust + 220 hd lines** | Sum of components; excludes tests/spec/docs |
| Focused tests/fixtures | 420 Rust/hd lines | Protocol, semantic boundaries, statistics and CLI contract |

Stop above 1.5 times any component's allowance; moving code between modules
does not evade review. A required new host ABI or compiler refactor beyond
the scalar barrier is a new estimate before code, not budget hidden in std.
No implementation is authorized by these estimates.

With ten workers/ten batches, 100 fixed records at an allowance of 48 bytes
need 4,800 bytes before metadata/vector capacity. Ten thousand bootstrap
f64 values add 80,000 bytes; raw log captures require their own bounded spool.
With twenty pairs, records and resamples remain small relative to engine state,
but the one-million resample cap alone can use 8 MB.

Compilation uses ordinary roots once per compiler/profile/engine, not once
per sample. Measure actual compile CPU/wall, sample overhead, peak memory,
barrier calls per operation, emitted Wasm and binary import counts before
claiming an acceptable harness cost. There are no current measurements of these additions.

The suite and barrier link only into benchmark roots that reference them;
normal hello's Wasm should remain byte-identical. CLI analysis/protocol code
stays out of the web dependency graph. Any intrinsic match-arm addition
must measure its gzip delta and preserve the hard 2 MB web compiler cap.
Neither zero gzip growth nor a code-size gain is asserted in advance.

## Stress test

### Surface and method

The surface is ordinary registration, typed rows/errors, real outer timing,
explicit scalar barriers, bounded batches and cluster/paired analysis.
The following twelve cases adapt public tool/library shapes and add adversarial
inputs. Results combine runnable current-API probes and synthetic data,
not execution of the proposed benchmark command.

| Case / source shape | Exercise and verdict |
| --- | --- |
| S1: Criterion/Go tiny pure operation | Repeated constant square could fold or hoist; **breaks without a proven barrier**. P5 records the missing std/API surface; no reliable pure timing claimed. |
| S2: pyperf worker grouping | 100 batches in ten workers have ten independent units; **works in the analysis model**, not 100 independent observations. |
| S3: Go-style mutable input operation | Reusing one List changes its length from 3 to 2; **breaks naive repeat**, repaired by explicit fresh-copy setup in P3. |
| S4: ManualClock simulation | Virtual 100 ms elapsed with real meter reading 0 ms in these short probes; **works for provider isolation**, invalid as a tiny timing sample (P1). |
| S5: typed callback registry | Full function-type storage runs clock-free work under Console while the outer meter uses Clock; **works with explicit row argument**, inference friction documented (P2). |
| S6: async completion and cancellation | Constructing cold work performs zero operations; awaited timeout runs its cleanup; **breaks construction-only timing**, P4 exercises the correct completion boundary. |
| S7: noisy outlier | Synthetic 1000 among values near 100 is flagged but retained; **works as a flag**, not justification for deleting slow values. |
| S8: process pseudoreplication | Two workers with many batches still have only two independent units; **inconclusive**, regardless of a narrow pooled-batch interval. |
| S9: gradual host drift | Synthetic all-A-then-B reports +18.265%; balanced pairs report +0.0072%, interval crossing zero change; **breaks blocked order**, pairing reduces this modeled drift only. |
| S10: known 5% effect | Synthetic paired geometric ratio 1.049991, 99% interval [1.047592,1.052394]; **works in the model** for a 3% threshold. |
| S11: rank-test reference | Six untied values below another six give U=0 and exact two-sided p=2/924; **works as an algorithm control**, six observations still fail the proposed paired CI minimum. |
| S12: failed/partial/mismatched work | Wrong output, zero elapsed, duplicate frame, early exit, incompatible units or missing pair cannot yield a successful mean; **required protocol tests**, not claimed implemented. |

Criterion's timing loops motivate setup separation; Go's examples motivate
operation counts and completed work; pyperf motivates independent workers.
These are small shape translations, not ports of their entire harnesses.
A real implementation must repeat the matrix against actual generated Wasm,
including output validation, collection, panic, cancellation and platform protocols.

### Current-API prototypes

P1 proves provider identity without pretending a short interval has useful precision:

```text
use std.time.{Clock, ManualClock, Timestamp, sleep}

pub fn main!() -> void $ Clock + Console:
    meter := $.use(Clock)
    before := meter.monotonic()
    fake := ManualClock::new(Timestamp::from_unix_milliseconds(0))
    $.with(Clock=fake):
        sleep!(100ms)
    virtual := fake.now().unix_milliseconds()
    real_ms := meter.monotonic().since(before).as_milliseconds()
    if virtual != 100:
        panic("virtual clock did not advance")
    println("virtual=$virtual real_ms=$real_ms")
```

Both engines printed `virtual=100 real_ms=0` in the recorded runs.
The real reading is retained as measured data, not a portable zero-cost claim.

P2 stores a concrete function type rather than declaring a row parameter
on a struct. The explicit row argument is necessary for today's invariant-container
inference in this probe. Both engines print `checked`.

```text
use std.time.Clock

@error
pub enum ProbeError:
    @error("invalid result")
    Invalid

struct Registry[F]:
    cases: List[F]

impl[F] Registry[F]:
    fn add(&mut self, work: F) -> void:
        self.cases.push(work)

fn execute![$R](suite: &Registry[fn!() -> Result[void, ProbeError] $ R]) -> Result[void, ProbeError] $ R:
    for work in suite.cases:
        work!()?
    .Ok(())

fn square(n: usize) -> usize: n * n

pub fn main!() -> Result[void, ProbeError] $ Clock + Console:
    meter := $.use(Clock)
    let mut suite: Registry[fn!() -> Result[void, ProbeError] $ Console] = Registry { cases: [] }
    suite.add(fn!() -> Result[void, ProbeError] $ Console:
        if square(17) != 289:
            return .Err(.Invalid)
        println("checked")
        .Ok(())
    )
    before := meter.monotonic()
    execute!::[$ Console](&suite)?
    delta := meter.monotonic().since(before)
    if delta.as_milliseconds() < 0:
        return .Err(.Invalid)
    .Ok(())
```

P3 approximates destructive collection workloads through pop rather than sorting;
no sorting implementation or algorithm is changed. Both engines print `3/3/2/4`.
The original input remains length 3 while reused input shrinks.

```text
fn pop_copy(input: List[usize]) -> usize:
    let mut values = input
    values.pop().unwrap_or(0)

pub fn main() -> void $ Console:
    input := [3, 1, 2]
    let mut reused = input
    let mut observed: List[usize] = []
    for _ in 0..2:
        observed.push(reused.len())
        _ := reused.pop()
    let checksum: usize = 0
    for _ in 0..2:
        checksum += pop_copy(input)
    println("${input.len()}/${observed[0]}/${observed[1]}/$checksum")
```

P4 uses a real timeout to exercise awaited work and synchronous cleanup.
Both engines print `awaited=1 cleanup=closed`. These runs validate that
boundary, not a timeout-latency distribution or benchmark stability.

```text
use std.time.{Clock, sleep}
use std.task.timeout

pub fn main!() -> void $ Clock + Console:
    let count = 0
    let mut events: List[string] = []
    work := fn!() -> void $ Clock:
        count += 1
        defer: events.push("closed")
        sleep!(200ms)
        events.push("finished")
    let cold: mut Suspend[void] = work()
    if count != 0:
        panic("construction ran the work")
    result := timeout!(20ms):
        cold!()
    if !result.is_err() || count != 1 || events != ["closed"]:
        panic("completion boundary was lost")
    println("awaited=$count cleanup=${events[0]}")
```

P5 is the proposed API/intrinsic spelling, not a working runner or a
substitute identity barrier. It parses, then checking rejects `std.bench`
with `unknown-module: no module named std.bench`.

```text
# Proposed library and intrinsic: unavailable today.
use std.bench.{Benchmarks, BenchError, black_box, run}
use std.time.Clock
use std.host.Args

fn square(n: usize) -> usize: n * n

pub fn main!() -> Result[void, BenchError] $ Args + Clock + Console:
    suite := Benchmarks::new()
    suite.add("square/17", fn!() -> Result[void, BenchError]:
        result := black_box(square(black_box(17)))
        if result != 289:
            return .Err(BenchError.Invalid("wrong square"))
        .Ok(())
    )
    run!::[$()](suite)
```

### Synthetic analysis evidence

A deterministic Python model, seed 184, retained every generated dataset
and all 10,000 resamples per estimate. Values are invented cost units,
not timing measurements or a runner performance claim.
Whole-worker medians estimate 100 with a 95% interval [98,103].

Two-worker input [100,120] gives [100,120] and remains insufficient regardless
of repeated batches. The exact untied rank control enumerates all 924
assignments; its two-sided p is 0.0021645021645021645.
The outlier control retains all ten observations, including the flagged 1000.

Balanced drift pairs estimate ratio 1.000072 with a 99% interval
[0.995114,1.004684], versus 1.182648 for the artificial blocked-order comparison.
The known-effect control has the interval shown in S10.
These checks can expose algorithm mistakes, not establish real-machine coverage
or validate an unimplemented CLI, engine barrier or worker protocol.

### Problems found, ranked

| Rank | Problem | Consequence |
| --- | --- | --- |
| 1 | Pure work may disappear; no existing barrier API | Do not ship pure-microbenchmark claims until the approved intrinsic survives both engines' final code. |
| 2 | Correlation and drift manufacture confidence/effects | Treat workers/pairs as units, retain order, flag instability and preserve inconclusive results. |
| 3 | Reset, completion and release boundaries change workloads | Expose ordinary versus batched atoms, typed failure and cleanup semantics; validate against current-API probes. |
| 4 | Data types do not declare rows; nested row inference is limited | Store full function types, use explicit row arguments when needed, and prove constructor/adaptor typing before promising ergonomic inference. |
| 5 | Cross-version protocol and allocation metrics are not free | Reject unsupported versions; keep coarse study results and diagnostic allocation runs separate. |

## Phasing

Each slice targets about one hour of agent work and stops with a concrete
estimate if it grows beyond that scope. No full local suite is implied;
focused checks follow the current Muse policy, and CI covers platform breadth.

| Slice | Deliverable and proof |
| --- | --- |
| 1 | Approved schema/plan and pure analysis core; exact quantile, worker-group, paired-resample and rank-reference controls pass. |
| 2 | Existing executable-root selection plus `compare` read path; malformed metadata and incompatible boundaries are rejected. |
| 3 | std registry/adapters with complete row typing; P1/P2 and mixed-provider checks pass without row erasure or new data-type semantics. |
| 4 | Scalar barrier intrinsic contract/lowering; generated-code controls retain representative pure work on Node and Wasmtime; unsupported types reject. |
| 5 | Bounded ordinary sampling/calibration; fake/decreasing/coarse clocks and count/overflow/watchdog cases pass. |
| 6 | Setup-excluded atom; destructive-input and release/heap-limit cases show the documented boundaries. |
| 7 | Sequential workers and framing; stdout/CRLF/Unicode/partial/crash/failed-result fixtures retain evidence and fail correctly. |
| 8 | Paired scheduling/baseline reporting; fixed order seeds, missing pairs, multiplicity and CI inconclusive outcomes pass deterministic tests. |
| 9 | Actual runner repeatability study and overhead/footprint/gzip checks; calibrate defaults and thresholds before recommending CI enforcement. |

Every implementation slice ships the corresponding approved CLI/std/language
tier rules and fixtures together. CLI discovery/reporting is CLI tier;
ordinary suite APIs are stdlib tier; the compiler-known barrier and diagnostic
codes are language tier. This note allocates or retires no rule IDs.

## Approval before implementation

**Recommendation:** ordinary library registration and bounded timed batches,
with a CLI process supervisor and explicit scalar barrier. Prefer report-only
historical comparisons and opt-in paired CI decisions with an inconclusive outcome.
The next-best simplification is whole-program timing when that boundary answers
the question; compiler-known registration is unnecessary first-release machinery.

Approval covers the API/errors, command scope, barrier lowering, timing/release
boundaries and statistical estimators. It also covers default budgets, paired
threshold policy and component allowances.
The interval and CI claims require empirical no-change validation before implementation
is considered ready for users. Browser and allocation-counter APIs remain separate work.

## Example validation

The complete proposed suite above is not implemented; the API gap is recorded,
not filled with a weaker substitute runner. The frozen compiler/parser were built at `a00c6b374358fb2992f046695ea0f6639b5ab542`.
The entire `crates/`, `lib/std/`, `host/`, Cargo manifest and lock inputs
are byte-identical to this proposal base, verified before reusing those tools.
Compiler SHA-256 `c79c3325b16b441b3e68d5c1b618b1eb2b68b2c117309c9d0cc1da89f40958d8`;
parser SHA-256 `714ee363867f02fd4819ed64599a7be7143c1c0630c0b5e14c6264b5cb846271`.

| HD block | Validation |
| --- | --- |
| P1 real/fake clock | Parse, format and check pass; Node/Wasmtime run; real elapsed retained without a cross-engine timing equality assertion. |
| P2 typed registration | Parse, format and check pass; both engines print `checked`. |
| P3 destructive input | Parse, format and check pass; both engines print `3/3/2/4`. |
| P4 cold work/cleanup | Parse, format and check pass; both engines print `awaited=1 cleanup=closed`. |
| P5 proposed API | Parse and format pass; expected unknown-module rejection; no runtime or barrier implementation claim. |
| Sampling/comparison pseudo-code | Proposed Rust-like interfaces, not current compiled exports. |
| Statistical controls | Deterministic Python executed; synthetic data and raw resamples retained, not workload performance. |

Raw commands, source/tool hashes, exact programs, failed construction attempts,
statistical datasets, reference source files and final gates are preserved in
`/private/tmp/muse-184-rewrite-1792783924512568000`.

Thirteen actual construction diagnostics are logged verbatim. The deliberate
missing-module probe is not counted as an accidental writing mistake.
No compiler, std, spec, fixture, ad-hoc runner or sorting implementation is edited.
