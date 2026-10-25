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

- [Server Applications](SERVER_APPLICATIONS.md): an ordinary `main!` with worker loops over one state object, one retained instance per adapter, an SDK-owned `HttpServer` capability (#51), lifecycle and draining, limits, grants, and SDK packaging tasks for Cloudflare and Vercel (#806, #266).
- [Host Embedding](HOST_EMBEDDING.md): parked proposal for embedding hd in an application; package host capabilities are designed in #51.
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Policy Composition](POLICY_COMPOSITION.md): decided design of lean policy values, tuple composition and one `apply!` block, the only public way to apply a policy; not yet implemented (#783).
- [Pipe Steps Without Placeholders](PIPE_STEPS.md): stress-tests standalone unary function steps, compatibility, effects, inference, and all three policy ports; no semantics are accepted (#818).
- [Pipe Styles For Shell Work](PIPE_STYLES.md): compares Elixir, F#, Hack, and general-placeholder pipes on sixteen shared ports, current inference, std costs, and 21-run V8/Wasmtime measurements; no style is chosen (#822).
- [Standard Library](STDLIB.md): the #350 bar and shared module convention, and the std decisions not yet in `lib/std`.
- [Weak References](WEAK_REFERENCES.md): weak references as a std type with a side-table runtime; approved (#720).

### Tooling

- [hd lint](HD_LINT.md): proposed command, shared diagnostics, package allow/deny policy, and a first lint for lost temporary-receiver writes (#651).
- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).
- [Live Execution](live-execution.md): the built REPL, journal, replay and session rebuilds (#17).

### Memory model and performance

- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): workers, copied messages and shared bytes on Wasm today; frozen data later is parked. There is no `^` mode (#163).
- [Object Model, Tier 2](OBJECT_MODEL_TIER2.md): opt-in lifetimes on tier 1. Its first cut, local references, is under investigation in #834 and will be folded in here (#704).
- [Local References](LOCAL_REFERENCES.md): tier 2's first cut, decided: counted `*T` references to whole objects, and local `&T` references in argument position only, like Swift's `inout`; the rules, error messages, counting rules, escape hatches and the implementation plan (#834).
- [Refcount Runtime](REFCOUNT_RUNTIME.md): the linear-memory runtime of reference counting with cycle detection, priced on Wasm; it is the only backend (#426).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).

No native backend is planned (#188 closed), and no direct TLS streams are planned.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
