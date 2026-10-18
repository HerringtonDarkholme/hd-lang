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

- [Open Issues](OPEN_ISSUES.md): the open language and library design
  questions. It holds only open questions.
- [Serializable Closure Design](SERIALIZABLE_CLOSURE_DESIGN.md): the design
  study of serializable closures and remote calls (#166).
- [Serializable Closure Prototype Plan](SERIALIZABLE_CLOSURE_PROTOTYPE.md): 31
  hour-sized slices, each with its spec rules and fixtures, over an in-process transport.
- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): a design study for #163 after `Disposable`: a non-escaping `^` mode without lifetimes, worker isolation and shared bytes on Wasm today, frozen data later, and a re-plan of the ownership prototype.
- [Rust Model Study](RUST_MODEL_STUDY.md): whether hd should adopt Rust's ownership, borrows and lifetimes with no tracing collector, and whether a `Gc[T]` can replace `Rc<RefCell<T>>`, priced on three programs written three ways (#163).
- [Cancellation And Cleanup](CANCELLATION_AND_CLEANUP.md): cancelling a task gracefully with `cancel!()` or forcibly with `abandon()`; how `race!`, `all!` and `timeout!` compose.
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).
- [Live Execution](live-execution.md): the REPL session model, replay and
  resume (#17, #18).
- [Standard Library Calls](STDLIB_CALLS.md): every stdlib design choice the
  owner did not make explicitly, kept for the stdlib audit.
- [SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md): proposed std
  primitives for byte scans, and what blocks SIMD on GC strings (#428).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).
- [Native Backend](NATIVE_BACKEND.md): native executables, a collector
  and threads without a Wasm engine, priced against wasmtime AOT (#188).

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
