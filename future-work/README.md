# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../spec/README.md), which is authoritative. Once
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
- [NonEscapable Types And Lifetimes: Design](NONESCAPE_LIFETIME_DESIGN.md):
  the numbered-rule design (#163), with its three adversarial reviews
  ([1](NONESCAPE_LIFETIME_STRESS_TEST.md),
  [2](NONESCAPE_LIFETIME_STRESS_TEST_2.md),
  [3](NONESCAPE_LIFETIME_STRESS_TEST_3.md)).
- [Ownership And Escape Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md): the
  survey of foundations behind the NonEscapable design.
- [Live Execution](live-execution.md): the REPL session model, replay and
  resume (#17, #18).
- [Standard Library Calls](STDLIB_CALLS.md): every stdlib design choice the
  owner did not make explicitly, kept for the stdlib audit.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../docs/compiler/ARCHITECTURE.md).
