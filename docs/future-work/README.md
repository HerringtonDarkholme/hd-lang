# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../../spec/README.md), which is authoritative. Once
a record is applied or superseded, it is deleted; git history keeps it.

## Direction

hd is written mostly by coding agents and read by humans. Tools answer
questions about a program by name and in machine-readable output. The
language prefers explicit, checked forms over inference, and forbids
action at a distance.

## What Remains

Parked work is labelled `parked` on its own GitHub issue; the old index #833
is closed. The execution-boundaries design (serializable closures, formerly #166) is [EXECUTION_BOUNDARIES.md](EXECUTION_BOUNDARIES.md), from PR #1139.

### Application and host

- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Compile-Time Values](CONST_VALUES.md): typed const parameters and explicit `const fn`, staged; direction accepted 2026-10-28, not scheduled (#875).
- [Decimal](DECIMAL.md): parked by the owner on 2026-11-11 until after the ticket push and reference-counting work; the earlier fixed 128-bit package proposal is retained as a dated record (#840).
- [Function Values](FUNCTION_VALUES.md): proposal 2026-11-21, not accepted: a capture-free one-word `fn(..)`, a type per closure taken by `F < FnMut(..)`, and counted `*mut dyn FnMut(..)` for stored capturing closures (#1436 Route A, #1449 on top).
- [Execution Boundaries](EXECUTION_BOUNDARIES.md): accepted by the owner 2026-11-21 (proposed 2026-11-02): moving code and data between execution domains on four parts: `Send` and `Wire` bounds, `Remote` interfaces whose objects are reached as `*mut dyn T`, `CrossError` kept apart as `E | CrossError`, and one build per target; every domain runs the same program; domains are run-time values and their kinds are std library; with an 18-case stress test (#166, PR #1139).
- [Execution Boundaries Plan](EXECUTION_BOUNDARIES_PLAN.md): the staged compiler plan for #2288, stages 1 to 6 with sites, line prices and per-run costs, and the readings stage 0 wrote into the spec for the owner to confirm.
- [Standard Library](STDLIB.md): the #350 bar and shared module convention, and the std decisions not yet in `lib/std`.

### Tooling

- [Warm Check Cost](WARM_CHECK_COST.md): design exploration for #1801 draft 89 (owner decision): measurements of a warm `hd check` at 500 and 2,000 files, a survey of change detection in git, Go, Zig, TypeScript, Swift, rustc, Salsa, Bazel, Buck2, Ninja and Watchman, and five options from a content-checked racy rule to a resident checker.
- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).

### Memory model and performance

- Parallelism and locality: folded into the execution-boundaries record, PR #1139 (`EXECUTION_BOUNDARIES.md` Appendix D), owner 2026-11-13.
- [Backend-Neutral TIR](BACKEND_NEUTRAL_TIR.md): accepted design (owner, 2026-11-18), built through step 8 on 2026-11-21 (#2173, #2176 and #2177 closed): a last TIR phase, `Lowered`, is all a backend reads; drop flags, cells, loans, counts and suspension become ordinary locals, statements and blocks; layout moves to a shared per-type crate. It replaces the Instance TIR record (accepted 2026-11-17, built through step 7a; git history at 421bcec83) and folds in its open steps: #2102, #1420, #2107, #2127, #2165, #2166.
- [The Path To Returned Local References](RETURNED_REFERENCES_PATH.md): accepted 2026-10-29, stages 1 to 9 built (the checker's region solver is [crates/hd_check/DESIGN.md §15](../../crates/hd_check/DESIGN.md#15-lifetimes-one-region-solver)); kept for the parts accepted but not built: view types for `&mut` parameters (#1357) and phantom markers.

No native backend is planned (#188 closed), and no direct TLS streams are planned.
The owner's 2026-11-18 direction names LLVM as a later need; [Backend-Neutral TIR](BACKEND_NEUTRAL_TIR.md) keeps the TIR ready for it and plans no backend.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md). Built
designs leave this folder for it: counts per instance, pointer loans and
raw buffers are [docs/compiler/ownership.md](../compiler/ownership.md),
the lifetime solver is
[crates/hd_check/DESIGN.md §15](../../crates/hd_check/DESIGN.md#15-lifetimes-one-region-solver),
the enum layout decisions are [crates/hd_wasm/DESIGN.md §23.14](../../crates/hd_wasm/DESIGN.md#2314-inline-aggregate-layouts),
release summaries are [crates/hd_opt/DESIGN.md §9](../../crates/hd_opt/DESIGN.md#9-release-summaries),
the REPL and its journal are [docs/compiler/live-execution.md](../compiler/live-execution.md),
package host capabilities and their native bindings are [docs/compiler/package-host-capabilities.md](../compiler/package-host-capabilities.md),
and JSON decoding is [spec/std/json.md](../../spec/std/json.md) and [spec/std/serde.md](../../spec/std/serde.md#reading).
The JSON decode and native host binding designs are in git history at 10e74b76a.
