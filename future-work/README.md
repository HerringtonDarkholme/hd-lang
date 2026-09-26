# Future Work

These documents cover deferred or exploratory work outside the accepted
language specification:

- [Roadmap](ROADMAP.md) orders the remaining work into grammar, type
  checking, runtime, standard library, packages, prototype, audit cleanup, and
  agent tooling areas.
- [Durable Replay: Core Or Library](DURABLE_REPLAY.md) decides which parts of
  durable replay the compiler and runtime provide and which a library builds.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work.
- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package managers
  and drafts the `hd.toml` schema, versioning, resolution, and lockfile for
  roadmap area 5.
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [Runtime and Library Design](RUNTIME_AND_LIBRARY.md) describes the broader
  standard-library, tooling, and runtime direction.
- [Wasm GC MVP Implementation Plan](../src/MVP_IMPLEMENTATION_PLAN.md) records the
  chosen fast-iteration compiler plan and its deliberately limited slices.

Accepted language behavior belongs in the [formal specification](../spec/README.md).
