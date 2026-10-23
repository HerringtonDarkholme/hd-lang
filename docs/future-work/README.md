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

- [Module Hot Reload](MODULE_HOT_RELOAD.md): proposed compatible body updates, retained active code, explicit restart boundaries, and unchanged REPL reload (#182).
- [Host Embedding](HOST_EMBEDDING.md): proposed explicit library registration, generated core-Wasm adapters, application providers, and structured panic outcomes (#51).
- [hd lint](HD_LINT.md): proposed command, shared diagnostics, package allow/deny policy, and a first lint for lost temporary-receiver writes (#651).
- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes (#28).
- [Open Issues](OPEN_ISSUES.md): the open language and library design
  questions. It holds only open questions.
- [Serializable Closure Design](SERIALIZABLE_CLOSURE_DESIGN.md): the design
  study of serializable closures and remote calls (#166).
- [Serializable Closure Prototype Plan](SERIALIZABLE_CLOSURE_PROTOTYPE.md): 31
  hour-sized slices, each with its spec rules and fixtures, over an in-process transport.
- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): a design study for #163 after `Disposable`: a non-escaping `^` mode without lifetimes, worker isolation and shared bytes on Wasm today, frozen data later, and a re-plan of the ownership prototype.
- [Rust Model Study](RUST_MODEL_STUDY.md): whether hd should adopt Rust's ownership, borrows and lifetimes with no tracing collector, and whether a `Gc[T]` can replace `Rc<RefCell<T>>`, priced on three programs written three ways (#163).
- [Object Model, Tier 1](OBJECT_MODEL_TIER1.md): Go's object model on counted linear memory: struct values, global `&T` and `&mut T` references with no exclusivity check, copy-on-write collections, element references that pin their list, closures with one call kind, a tour, and a retrofit plan in hour-sized jobs (#426).
- [Refcount Runtime](REFCOUNT_RUNTIME.md): the linear-memory runtime of reference counting with cycle detection, priced on Wasm: count elision, allocators, interior references, collectors and module size, with the recommended design and hour-sized jobs (#426).
- [Object Model, Tier 2](OBJECT_MODEL_TIER2.md): opt-in lifetimes on tier 1: reference and closure types carrying the set of things they depend on, `global` as one more lifetime, static exclusivity and freezes for the value world, a RefCell-style borrow flag where a scoped borrow goes through a global reference, closure kinds `fn`, `fn mut`, `fn once`, NonCopyable structs, a score against the three costs of counting, a tour, and hour-sized jobs (#426).
- [Cancellation And Cleanup](CANCELLATION_AND_CLEANUP.md): cancelling a task gracefully with `cancel!()` or forcibly with `abandon()`; one cleanup per job, cleanup order under abandonment, and how `race!`, `all!` and `timeout!` compose and bound cleanup.
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).
- [Live Execution](live-execution.md): the REPL session model, replay and
  resume (#17, #18).
- [REPL Journal Checkpoints](REPL_CHECKPOINTS.md): measured checkpoint options, a shared base module, resource limits and the historical evidence needed by reload (#646).
- [Standard Library Calls](STDLIB_CALLS.md): every stdlib design choice the
  owner did not make explicitly, kept for the stdlib audit.
- [SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md): proposed std
  primitives for byte scans, and what blocks SIMD on GC strings (#428).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).
- [Native Backend](NATIVE_BACKEND.md): native executables, a collector
  and threads without a Wasm engine, priced against wasmtime AOT (#188).


The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).

- [WEAK_REFERENCES.md](WEAK_REFERENCES.md): weak references as a std type with side-table runtime (approved 2026-10-21).
