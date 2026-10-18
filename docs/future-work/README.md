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
- [Ownership And Borrows: Design Study](OWNERSHIP_DESIGN.md): the owner's
  Rust-model design for #163: `own` at the use site, `^` borrows, lifetimes
  named by variables, `DropOwn`. Fully decided by the owner (2026-10-17);
  not yet in the spec.
- [Ownership Prototype Plan](OWNERSHIP_PROTOTYPE.md): 23 hour-sized
  slices to prototype the design behind an opt-in.
- [Ownership Adversarial Review](OWNERSHIP_STRESS_TEST.md): twelve cases
  and the same eleven evaluation dimensions, including cancellation,
  linear collections, and exclusive callable/provider access.
- [Ownership Adversarial Review 2](OWNERSHIP_STRESS_TEST_2.md): reevaluates
  the destructor-based revision against the same dimensions, including
  partial moves, drop-aware lifetimes, and move-only borrowed holders.
- [Ownership Adversarial Review 3](OWNERSHIP_STRESS_TEST_3.md): checks the
  `OwnOnly` revision, confirms earlier safety repairs, and tests generic
  mode substitution, conditional impl coherence, and guard lowering.
- [Ownership Adversarial Review 4](OWNERSHIP_STRESS_TEST_4.md): evaluates
  structural ownership and generic normalization, with probes for receiver
  reservation, classifier base cases, and generic body checking.
- [Ownership Adversarial Review 5](OWNERSHIP_STRESS_TEST_5.md): confirms the
  fourth round's repairs and tests generic copying, guarded consuming
  patterns, and replacement through exclusive borrows.
- [Ownership Adversarial Review 6](OWNERSHIP_STRESS_TEST_6.md): retests the
  fifth round's repairs, then probes global cleanup dependencies,
  cancellation around optional extraction, and always-owned generics.
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).
- [Live Execution](live-execution.md): the REPL session model, replay and
  resume (#17, #18).
- [Standard Library Calls](STDLIB_CALLS.md): every stdlib design choice the
  owner did not make explicitly, kept for the stdlib audit.
- [SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md): proposed std
  primitives for byte scans, and what blocks SIMD on GC strings (#428).

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
