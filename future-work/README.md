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

- [New Compiler: Architecture Notes](NEW_COMPILER_ARCHITECTURE.md) holds
  the owner's goals for the new compiler and CLI, and implementation
  notes carried over from the prototype's records. The prototype in
  [`src/`](../src/README.md) is frozen as a test oracle.

Research and direction outside the specification:

- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [Standard Library Calls](STDLIB_CALLS.md) logs every stdlib design
  choice the owner did not make explicitly, with its status, for the
  stdlib audit.
- [Standard Library Plan](STDLIB_PLAN.md) surveys what single-file
  scripts need from `std` across twenty standard libraries, reviews
  Effect, and ranks ten one-hour tiers, with six owner questions.
