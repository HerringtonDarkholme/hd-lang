# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../spec/README.md), which is authoritative.

Once everything a design record decided is applied, superseded, or moved
to [Open Issues](OPEN_ISSUES.md), the record is deleted. Git history keeps
it.

## Active Records

Planning and backlog:

- [Roadmap](ROADMAP.md) is one page: the order of the remaining work, and
  what is parked or on hold.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work, including undecided
  standard-library design. It holds only open questions; the commit message
  records what a pass applied.

Compiler:

- [New Compiler: Architecture Notes](compiler/goals.md) holds
  the owner's goals for the new compiler and CLI, and implementation
  notes carried over from the prototype's records. The TypeScript
  prototype (`src/`) was deleted on 2026-10-12 (#145); git history keeps it.

Research and direction outside the specification:

- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [NonEscapable Types And Lifetimes: Design](NONESCAPE_LIFETIME_DESIGN.md)
  states the round-2 NonEscapable design as numbered rules (#163), with a
  stress-test suite, a checker cost model, and owner questions.
- [NonEscapable Types And Lifetimes: Adversarial Stress Test](NONESCAPE_LIFETIME_STRESS_TEST.md)
  attacks recovered write authority, cleanup execution, ordinary locality,
  function values, frame outputs, and optimizer proofs across twenty cases.
- [NonEscapable Types And Lifetimes: Adversarial Review 2](NONESCAPE_LIFETIME_STRESS_TEST_2.md)
  rechecks rounds 4 and 5 against the earlier findings and evaluation dimensions,
  with thirteen cases targeting providers, invariant aggregation, and callable contracts.
- [Standard Library Calls](STDLIB_CALLS.md) logs every stdlib design
  choice the owner did not make explicitly, with its status, for the
  stdlib audit.
- [Standard Library Plan](STDLIB_PLAN.md) surveys what single-file
  scripts need from `std` across twenty standard libraries, reviews
  Effect, and ranks ten one-hour tiers, with six owner questions.
