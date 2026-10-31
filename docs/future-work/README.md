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

Parked designs are not kept here; each lives in its GitHub issue, indexed by #833, which
stays open (#18, #163, #166, #181, #182, #646, #807).

### Application and host

- [HTTP Transport Settings](HTTP_CLIENT_SETTINGS.md): accepted 2026-10-29 (chaining on the `Http` provider; literal-IP DNS servers; roots extend the platform store; a zero phase timeout expires at once) for DNS, TLS, and phase timeouts (#950).
- [Server Applications](SERVER_APPLICATIONS.md): an ordinary `main!` with worker loops over one state object, one retained instance per adapter, an SDK-owned `HttpServer` capability (#51), lifecycle and draining, limits, grants, and SDK packaging tasks for Cloudflare and Vercel (#806, #266).
- [Host Embedding](HOST_EMBEDDING.md): parked proposal for embedding hd in an application; package host capabilities are designed in #51.
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Compile-Time Values](CONST_VALUES.md): typed const parameters and explicit `const fn`, staged; direction accepted 2026-10-28, not scheduled (#875).
- [Positional Union Types](UNION_TYPES.md): ordered anonymous enums with explicit positional construction; design complete 2026-10-28, not scheduled (#804).
- [Decimal](DECIMAL.md): fixed 128-bit exact decimal with explicit rounding and exact string serde; design accepted 2026-10-28 as a future package, not in std and not scheduled (#840).
- [Scope Guards](SCOPE_GUARDS.md): accepted: sync `using`, `using!` and error-level `unbound-disposable` built (#962); `on_exit` with tier-2 capture lifetimes, `@auto_closing` (#1099) and library `defer`/`ensure` (#1101) built; the `defer:` statement is being removed (#1117).
- [Standard Library](STDLIB.md): the #350 bar and shared module convention, and the std decisions not yet in `lib/std`.
- [Design By Contract](DESIGN_BY_CONTRACT.md): `require(message, condition)` as ordinary std code, checked in every profile until `@cfg` splits it; `ensure` deferred to tier 2 (#978); design accepted 2026-10-28 (#880).
- [Inline And Cold Decorators](INLINE_AND_COLD.md): `@inline(.Always)`, `@inline(.Never)` and `@cold` compiler decorators over the release inliner and table calls; accepted, phases 1-3 implemented, the std rare paths (#884) measured a miss, `.Always` open (#873).

### Tooling

- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).
- [Live Execution](live-execution.md): the built REPL, journal, replay and session rebuilds (#17).

### Memory model and performance


- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): workers, copied messages and shared bytes on Wasm today; frozen data later is parked. There is no `^` mode (#163).
- [Object Model, Tier 2](OBJECT_MODEL_TIER2.md): opt-in lifetimes on tier 1. Part 3 is its first cut, local references: step 1 is applied (#834); step 2 is not adopted (#704 shipped as a release-only fact instead), and later steps need approval.
- [The Path To Returned Local References](RETURNED_REFERENCES_PATH.md): accepted 2026-10-29: Rust lifetime variables and elision over counted references, in stages; stages 1 to 8 built (6i not planned for now; stage 8 without borrow flags); stage 9 and #1115 open.
- [One Lifetime Solver](LIFETIME_SOLVER.md): accepted 2026-10-30: region variables per reference slot, outlives constraints per TIR instruction and one SCC solver in place of the per-shape paths; S1 to S12 built, and every spec union is dropped; stage 7 built on it.
- [Returned References, Stage 3: Source Sets Or Lifetimes](RETURNED_REFERENCE_SOURCE_SETS.md): decided 2026-10-29: single lifetime variables spelled `'name` (#1005).
- [Stage 3 Source Sets: Stress Test](RETURNED_REFERENCE_SOURCE_SETS_STRESS_TEST.md): design review 2026-10-29: 18 real-code shapes compare lifetime variables, public paths and root sets; paths add no annotation reduction and hit eight abstraction boundaries (#1005).
- [Refcount Runtime](REFCOUNT_RUNTIME.md): the linear-memory runtime of reference counting with cycle detection, priced on Wasm; it is the only backend (#426).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).
- [Release-Only Interprocedural Optimization](RELEASE_INTERPROC.md): built 2026-10-25 in the optimized pipeline only: per-function summaries over SCCs (slices 2-5: kept parameters, writes, lent parameters). Slices 6-9 measured no gain and were not merged (#681 closed); results on #165.

No native backend is planned (#188 closed), and no direct TLS streams are planned.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
