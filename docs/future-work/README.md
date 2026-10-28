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
stays open (#18, #163, #166, #181, #182, #184, #646, #804, #807).

### Application and host

- [Validation](VALIDATION.md): short design and [checked signatures and examples](validation/) for typed JSON/CLI/environment validation; owner review before implementation (#846).

- [Server Applications](SERVER_APPLICATIONS.md): an ordinary `main!` with worker loops over one state object, one retained instance per adapter, an SDK-owned `HttpServer` capability (#51), lifecycle and draining, limits, grants, and SDK packaging tasks for Cloudflare and Vercel (#806, #266).
- [Host Embedding](HOST_EMBEDDING.md): parked proposal for embedding hd in an application; package host capabilities are designed in #51.
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Standard Library](STDLIB.md): the #350 bar and shared module convention, and the std decisions not yet in `lib/std`.
- [Weak References](WEAK_REFERENCES.md): weak references as a std type with a side-table runtime; approved (#720).
- [Inline And Cold Decorators](INLINE_AND_COLD.md): `@inline(.Always)`, `@inline(.Never)` and `@cold` compiler decorators over the release inliner and table calls; accepted, phases 1-3 implemented, the std rare paths (#884) measured a miss, `.Always` open (#873).

### Tooling

- [hd fmt](HD_FMT.md): the approved formatter architecture: one fixed style, comment attachment and a bounded document printer (#906); implemented (#899).
- [hd lint](HD_LINT.md): the approved shared lint architecture: one rule registry, one analysis walk, policy applied once, a per-module findings cache (#908); the #855 migration onto it is complete.
- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).
- [Live Execution](live-execution.md): the built REPL, journal, replay and session rebuilds (#17).

### Memory model and performance


- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): workers, copied messages and shared bytes on Wasm today; frozen data later is parked. There is no `^` mode (#163).
- [Object Model, Tier 2](OBJECT_MODEL_TIER2.md): opt-in lifetimes on tier 1. Part 3 is its first cut, local references: step 1 is applied (#834); step 2 is not adopted (#704 shipped as a release-only fact instead), and later steps need approval.
- [Refcount Runtime](REFCOUNT_RUNTIME.md): the linear-memory runtime of reference counting with cycle detection, priced on Wasm; it is the only backend (#426).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).
- [Release-Only Interprocedural Optimization](RELEASE_INTERPROC.md): built 2026-10-25 in the optimized pipeline only: per-function summaries over SCCs (slices 2-5: kept parameters, writes, lent parameters). Slices 6-9 measured no gain and were not merged (#681 closed); results on #165.

No native backend is planned (#188 closed), and no direct TLS streams are planned.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
