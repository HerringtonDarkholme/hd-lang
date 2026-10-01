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
  standard-library design. It also lists the applied decision batches, each
  linked to the Revision Notes.

Open questions for the owner:

- [Shape Review](SHAPE_REVIEW.md) traces where `shape`, `shape_of`, and
  the shape types came from, finds no owner decision that approved them,
  and compares removing them, keeping only a function-fact read, and
  keeping them, against `Structure` templates. Batch 42 applied option B:
  shapes are removed, and `facts_of` reads a function's facts.
- [Reified Review](REIFIED_REVIEW.md) answers the Shape Review's Q3:
  with shapes gone, nothing reads a `reified` descriptor. It traces the
  2026-08-08 decision, finds the fact lookup's key unstated, and compares
  removing, narrowing, and keeping `reified`.

Prototype plan:

- [Compiler/Library Audit](COMPILER_LIBRARY_AUDIT.md) sorts what the
  TypeScript prototype implements by hand into true intrinsics, library
  code movable to `lib/std` now or after a small hook, and language
  semantics. It plans eleven one-hour chunks that delete about 1,900
  lines of TS. The owner answered its four questions in batch 36.

Research and direction outside the specification:

- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package
  managers and drafts the `hd.toml` schema, the checked compatibility
  rule, and the agent-first CLI. The registry-free model of DEP1-DEP19 is
  in [Package Manifest](../spec/10-modules.md#package-manifest).
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
