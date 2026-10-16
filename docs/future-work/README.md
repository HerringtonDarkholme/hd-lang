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
  Rust-model redirect for #163: `own` types, `^` borrows, lifetimes named
  by variables. Nothing in it is accepted.
- [NonEscapable Types And Lifetimes: Design](NONESCAPE_LIFETIME_DESIGN.md):
  the numbered-rule design (#163). The cases of its three adversarial
  reviews are its regression suite, and its appendices keep the
  comparison with other languages.
- [Live Execution](live-execution.md): the REPL session model, replay and
  resume (#17, #18).
- [Standard Library Calls](STDLIB_CALLS.md): every stdlib design choice the
  owner did not make explicitly, kept for the stdlib audit.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
