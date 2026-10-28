# `hd bench`

Status: design proposal, 2026-10-28; domains: CLI, stdlib, and measurement; not accepted.

This proposal revisits [#184](https://github.com/HerringtonDarkholme/hd-lang/issues/184)
and its [statistics review](https://github.com/HerringtonDarkholme/hd-lang/issues/184#issuecomment-6372421647).
It does not implement a command, benchmark API, clock, or optimization barrier.
The [checked package](hd-bench/) holds the proposed std signatures and examples.

## Proposal

`hd bench` discovers top-level `bench("name"):` registrations in `benches/**/*.hd`.
The form mirrors top-level `it("name"):` in `tests/**/*.hd`; there is no typed registry or benchmark `main`.
The runner owns its real clock, while each case keeps only the requirements of its work.

```text
# tests/split.hd
it("split returns four fields"):
    assert_equal("red,green,blue,black".split(",").len(), 4, reason="four fields")

# benches/split.hd
bench("text/split/21b"):
    input := black_box("red,green,blue,black")
    iter!(Throughput.Bytes(input.len()), fn!() -> Result[void, BenchError]:
        fields := input.split(",")
        if black_box(fields.len()) != 4:
            return .Err(.Work("split returned the wrong fields"))
        .Ok(())
    )
```

Illustrative text output, not a measurement from this proposal:

```console
text/split/21b  12.8 MiB/s  1.56 us/op  median  runs=5
```

## Open questions

1. Registration controls language cost: compiler-known top-level `bench("name"):` calls under `benches/` (**recommended**) or ordinary typed registries?
2. Pure-work timing needs opacity: intrinsic `black_box(value)` (**recommended**) or exclude pure microbenchmarks?
3. Default depth sets iteration time: five approximately 300 ms runs (**recommended**) or criterion-level statistics on every invocation?
4. CI needs a practical band: opt-in full comparison with `--fail-regression 3%` (**recommended**) or a default verdict?

## Problems it solves

| User task | Today | Proposed workflow |
| --- | --- | --- |
| Tune one std call | Write an ad-hoc script and interpret raw times | Add a named case, run `hd bench text/split`, inspect five-run throughput |
| Catch a regression | CI has no benchmark verdict | Run interleaved old/new compiler pairs with an explicit 3% threshold |
| Compare compiler versions | `bench/study` measures whole programs | `--against-compiler` measures identical operation roots; process mode stays separate |
| Compare two algorithms | Setup and reset boundaries are hand-written | Register two labels using the same seeded input and `iter_batched!` |

## How it works

### Registration and timing

`benches/**/*.hd` files use direct top-level `bench("name"):` calls to register cases.
This mirrors the compiler-known test position of top-level `it("name"):` calls in `tests/**/*.hd`.
The CLI builds the matching cases in the release profile; no `main`, registry value, or terminal `run!` call exists.

The runner retains the existing real `Clock`; a case never writes `$ Clock`.
It reads `Clock.monotonic()` before and after each work batch.
A workload may bind `ManualClock` inside that work call; its lexical binding ends before the outer meter reads again.

`iter!` uses the runner supplied to benchmark position and measures a complete callback invocation, including awaited completion and normal value release.
`iter_batched!` prepares fresh values outside timing and passes each as `&mut S` to one measured call.
Setup and cleanup policies are recorded; no empty-loop or timer constant is subtracted.

`black_box` is a best-effort compiler intrinsic with identity semantics.
It blocks hd and Wasm optimizers from assuming its input or result value.
Its overhead remains inside the batch and is shown by control cases; it is not a correctness or secrecy boundary.

### Warmup, samples, and statistics

Each run is a fresh process and instance.
The default performs five runs; each doubles iterations during untimed calibration, then retains one approximately 300 ms batch.
This quick mode records V8 or Wasmtime identity and flags but makes no tier-completion or statistical verdict claim.

Calibration chooses a fixed iteration count whose batch lasts at least 300 ms and 100 observed positive clock ticks.
It stops at bounded iteration, time, and prepared-input limits.
`--statistics` uses 15 interleaved fresh runs; `--runs 31` is reserved for a comparison near the 3% decision bar.

Quick mode reports the median, min/max, raw runs, and throughput, with no interval or verdict.
Statistics mode reports MAD, IQR, raw runs, and a seeded 95% percentile-bootstrap interval over whole processes.

Tukey 1.5/3-IQR fences flag mild and severe outliers on log-positive costs.
Valid outliers remain in every estimate.
Fewer than 15 runs, interval half-width above 5%, run MAD above 10%, or calibration/retained drift makes a statistics result inconclusive.

`Throughput.Operations`, `.Bytes`, and `.Items` name completed work per callback.
The primary display is operations, bytes, or items per second; time per callback is secondary.
A batch average never claims individual-request p95 or p99 latency.

### Comparison and noise

`--save-baseline PATH` atomically saves raw versioned JSON; `--baseline PATH` compares a fresh run with it.
Historical comparisons report median ratios, intervals, and a Mann–Whitney U result on independent run costs.
They remain descriptive because a file cannot remove host drift.

`--against-compiler PATH` opts into full statistics, freezes both artifacts, and runs 15 seeded AB/BA process pairs sequentially.
The primary comparison is the geometric mean of paired run ratios with a 99% paired-bootstrap interval.
Order subgroups and a time trace expose drift; incompatible workload fingerprints refuse comparison.

`--fail-regression 3%` is opt-in.
It fails when the paired interval lies wholly above 1.03, passes equivalence only wholly inside the ±3% band,
and otherwise fails as inconclusive; multiple cases adjust the interval level before collection.

The runner records allowlisted OS, CPU, load, engine, compiler, std, source, profile, seed, order, and cache metadata.
It flags own-process overlap, high load, drift, missing platform metrics, and failed output validation.
It never reads arbitrary environment variables, changes affinity or power settings, or retries until a favorable result.

### Reports and scope

Text gives the estimate, interval, spread, throughput, sample counts, identities, warnings, and verdict.
JSON Lines uses `schema: "hd-bench/1"` with plan, worker, sample, estimate, comparison, failure, and summary records.
Raw child output is framed separately so it cannot corrupt machine-readable stdout.

Names such as `parse/size=4096` carry explicit parameters; JSON stores their values and seed.
Suspending callbacks are awaited, so host and scheduler time is part of that benchmark.
Guest allocation counts are excluded until the runtime has a stable counter; optional process peak RSS is labeled separately.

## Cost

| Component | Proposed budget or effect |
| --- | --- |
| Existing scripts | delete nothing; `bench/runtime` and `bench/study` retain their coarse boundaries |
| Std API | at most 160 hd lines for registration signatures, batches, runner protocol, and typed errors |
| CLI | at most 450 Rust lines for discovery, five-run quick mode, schema, and reports; full statistics is a later slice |
| Compiler | at most 140 Rust lines for `black_box` typing, lowering, and both-engine code checks |
| Runtime | two clock reads per approximately 300 ms batch; measure actual overhead, never subtract it |
| Normal Wasm | byte-identical when no benchmark root is built |
| Benchmark Wasm | implementation budget: at most +8 KiB optimized over the same callback in a benchmark root |
| Web compiler | implementation budget: at most +3 KiB gzip under the 2 MB cap for the barrier support |
| Compile time | benchmark roots build once per compiler/profile/engine, never once per sample |

Stop when any component exceeds 1.5 times its budget.
The implementation must report total Wasm bytes, code-section bytes, function count, CLI size, and web gzip delta.

## Appendix A: Stress test

| Case adapted to hd | Result and design response |
| --- | --- |
| Criterion string split | Constant input and ignored output can disappear; input/output `black_box` calls are required |
| Go `B.Loop` setup | Preparing a list before measurement works with `iter_batched!`; reused mutation is rejected by the example contract |
| JMH hot method | V8 may tier during retained batches; record versions and drift rather than claiming warmup completion |
| `bench/study` compiler comparison | Whole-process compilation remains a separate mode; operation reports never mix in startup |
| ManualClock scheduler | The body sees virtual time while the retained outer real `Clock` measures completion |
| Suspending process/HTTP call | Each `fn!` invocation is awaited; cancellation or panic invalidates the worker and retains evidence |
| Gradually warming host | All-A-then-B order fabricates a difference; seeded AB/BA pairs and order subgroups expose it |
| One 20× pause | Statistics mode flags it, but the valid run remains in the estimate and raw record |
| Two runs with 100 batches | Still only two independent process units; the result is inconclusive |
| Allocation-heavy parser | Timing remains valid; guest allocation claims are excluded without a defined counter |

The stress test rejects one tempting simplification: a plain generic identity function is not an optimization barrier.
It also rejects automatic CI success from a stored baseline because environmental drift remains unpaired.

## Appendix B: Prior art

### Go `testing` and benchstat

```go
func BenchmarkSplit(b *testing.B) {
    input := strings.Repeat("a,", 4096)
    for b.Loop() {
        strings.Split(input, ",")
    }
}
```

`B.Loop` excludes setup and cleanup and uses compiler support to keep loop work alive.
Benchstat recommends at least ten interleaved runs, reports medians and intervals, and uses Mann–Whitney by default.
hd keeps that analysis opt-in, while the default stays at five quick runs.
[testing.B.Loop](https://pkg.go.dev/testing#B.Loop),
[benchstat](https://pkg.go.dev/golang.org/x/perf/cmd/benchstat).

### Criterion.rs and Cargo

```rust
fn split(c: &mut Criterion) {
    c.bench_function("split", |b| b.iter(|| black_box(INPUT).split(',')));
}
criterion_group!(benches, split);
criterion_main!(benches);
```

Criterion separates warmup, measurement, bootstrap analysis, outlier flags, and saved comparisons.
Cargo gives benchmarks their own `benches/` targets.
hd takes both shapes, but uses fresh process clusters and preserves inconclusive outcomes.
[analysis](https://bheisler.github.io/criterion.rs/book/analysis.html),
[benchmark target](https://bheisler.github.io/criterion.rs/book/getting_started.html).

### JMH

```java
@Warmup(iterations = 5)
@Measurement(iterations = 10)
@Fork(5)
@Benchmark
public void split(Blackhole bh) {
    bh.consume(INPUT.split(","));
}
```

JMH separates warmup, measurement, process forks, and output consumption for a tiered runtime.
hd takes those boundaries but records V8 and Wasmtime separately instead of assuming one tiering model.
[JMH project and samples](https://github.com/openjdk/jmh).

## Appendix C: Alternatives considered

**Compiler-known benchmark declarations.** They simplify discovery but add language-tier position and metadata rules.
Ordinary executable roots already provide scope, visibility, requirements, and one clear entry point.

**Only whole-process timing.** This is the cheapest and remains correct for startup and compiler studies.
It cannot exclude setup or resolve sub-millisecond operation costs, so it does not replace the requested tool.

**One long-lived process.** It is faster and gives more batches.
Its correlated samples hide process-to-process variance and make old/new drift harder to control.

**Automatic overhead subtraction.** Timer and empty-loop costs are not stable independent constants.
Control cases are reported beside the workload instead.

## Appendix D: Today in hd

`hd test` discovers standard tests and properties; no benchmark case exists.
`bench/runtime/run.mjs` and `bench/study/run.mjs` measure repository workloads with different process boundaries.
The preserved Node calibration measured startup, not benchmark precision or a subtractable clock cost.

`Clock.monotonic()` is the existing capability and returns millisecond `Instant` values.
Long batches amortize that granularity.
`hd bench` currently reports an unknown command, and `std.bench` does not exist.

The API is stdlib tier, command/discovery/reporting is CLI tier, and `black_box` plus benchmark position are language tier.
No new token, clock, or host ABI is proposed; top-level `bench` registration follows the existing test-position mechanism.

## Appendix E: Implementation phases

1. Approve benchmark position and std signatures; type-check registration, rows, duplicate names, and parameters.
2. Add `benches/` discovery, five-run calibration, and JSON framing; test filters, failures, Unicode, and bounds.
3. Add opt-in statistics and comparison; validate resampling, ties, drift, seeded order, and baseline writes.
4. Add `black_box`; inspect final Node and Wasmtime code for constant folding, hoisting, and result elimination.
5. Run no-change and known-change studies; approve CI policy only after repeatability evidence.

The rollback is removal of the new command, std module, and intrinsic before specification acceptance.
No existing command, package root, or runtime protocol changes meaning in this proposal.

## Parse log

`scripts/hd-conformance.sh parse` parses the proposal's `text` block.
This validates syntax only; the package checks provide type-level validation.
