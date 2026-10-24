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

Entries marked parked stay as proposals; the owner has deferred them and
the design stays.

### Application and host

- [Server Applications](SERVER_APPLICATIONS.md): ordinary-`main!` application lifetime, shared state and deployment SDKs; direction A is approved, with hooks and isolated jobs compared and not chosen; the package/SDK boundary, deployment profile and lifecycle policies stay open (#806).
- [HTTP Server](HTTP_SERVER.md): an ordinary `main!`, one retained instance per adapter, an SDK-owned `HttpServer` capability, and SDK packaging tasks with no `hd build` flag; Cloudflare, Vercel and local adapters follow #51's minimum (#266).
- [Host Embedding](HOST_EMBEDDING.md): parked proposal for embedding hd in an application; package host capabilities are designed in #51.
- [GUI Applications](GUI_APPLICATIONS.md): parked exploration of host UIs, message/view models, signals and immediate frames, with explicit embedding and renderer gaps (#807).
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Policy Composition](POLICY_COMPOSITION.md): decided design of lean policy values, tuple composition and one `apply!` block, the only public way to apply a policy; not yet implemented (#783).
- [Pipe Steps Without Placeholders](PIPE_STEPS.md): stress-tests standalone unary function steps, compatibility, effects, inference, and all three policy ports; no semantics are accepted (#818).
- [Positional Union Types](UNION_TYPES.md): parked stress test of unique static injection versus explicit construction, real error handling, policy ports, source chains and constructor inference (#804).
- [Standard Library Redesign](STDLIB_REDESIGN.md): the review of the public `lib/std` surface against the #350 bar; the owner questions are answered and most changes are in std.
- [Standard Library Calls](STDLIB_CALLS.md): every stdlib design choice the owner did not make explicitly, kept for the stdlib audit (#350).
- [SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md): five scan primitives; SIMD128 intrinsics are approved after TIR v2 stage 3 (#428).
- [Weak References](WEAK_REFERENCES.md): weak references as a std type with a side-table runtime; approved (#720).
- [Open Issues](OPEN_ISSUES.md): the open language and library design questions. It holds only open questions.

### Tooling

- [hd lint](HD_LINT.md): proposed command, shared diagnostics, package allow/deny policy, and a first lint for lost temporary-receiver writes (#651).
- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).
- [Release Stress Test](RELEASE_STRESS.md): real release histories, typed API hazards, command-by-command prior art and version/tag edge cases (#28).
- [hd bench](HD_BENCH.md): parked proposal for ordinary suite registration, batched real-clock measurement and baseline reports (#184, deferred until real users).
- [Program Database](PROGRAM_DATABASE.md): parked proposal for versioned source-fact snapshots and queries without an editor protocol (#181, deferred until real users).
- [Live Execution](live-execution.md): the REPL session model, journal and rebuild, which are built, and resume, which is Later (#17, #18).
- [Durable Resume](DURABLE_RESUME.md): parked plan for durable host-call replay, strict identity, interrupted-effect retries and journal growth policy; the record/replay plan is kept and implementation comes later (#18).
- [REPL Journal Checkpoints](REPL_CHECKPOINTS.md): parked measured checkpoint options, a shared base module and resource limits (#646, deferred until real users).
- [Module Hot Reload](MODULE_HOT_RELOAD.md): parked proposal for compatible body updates, retained active code and explicit restart boundaries (#182).

### Memory model and performance

- [Serializable Closure Design](SERIALIZABLE_CLOSURE_DESIGN.md): the design study of serializable closures and remote calls; not approved (#166).
- [Serializable Closure Prototype Plan](SERIALIZABLE_CLOSURE_PROTOTYPE.md): 31 hour-sized slices, each with its spec rules and fixtures, over an in-process transport; no slice is scheduled.
- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): workers, copied messages and shared bytes on Wasm today; frozen data later is parked. There is no `^` mode (#163).
- [Rust Model Study](RUST_MODEL_STUDY.md): the study that priced Rust's ownership for hd; decided against ownership (#163).
- [Object Model, Tier 1](OBJECT_MODEL_TIER1.md): Go's object model on counted linear memory: struct values, global `&T` and `&mut T` references, copy-on-write collections and closures with one call kind (#426).
- [Object Model, Tier 2](OBJECT_MODEL_TIER2.md): parked proposal for opt-in lifetimes on tier 1 (#426, #163).
- [Refcount Runtime](REFCOUNT_RUNTIME.md): the linear-memory runtime of reference counting with cycle detection, priced on Wasm; it is the only backend (#426).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).

No native backend is planned (#188 closed), and no direct TLS streams are planned.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
