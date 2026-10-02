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

Prototype plan:

- The Compiler/Library Audit's migration is finished (M1 to M11; the
  record is in git history). [`src/README.md`](../src/README.md) keeps the
  compiler/library boundary.

Research and direction outside the specification:

- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package
  managers and drafts the `hd.toml` schema, the checked compatibility
  rule, and the agent-first CLI. The registry-free model of DEP1-DEP19 is
  in [Package Manifest](../spec/lang/10-modules.md#package-manifest).
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
