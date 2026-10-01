# Roadmap

This page lists the remaining work in order. It points to where each
item lives; it holds no design. Open questions are in
[Open Issues](OPEN_ISSUES.md), and the
[specification](../spec/README.md) is authoritative.

## Direction

hd is written mostly by coding agents and read by humans. Tools answer
questions about a program by name and in machine-readable output. The
language prefers explicit, checked forms over inference, and forbids
action at a distance.

## Order

1. **Correctness.** Close the
   [known failures](../test/portable/KNOWN_FAILURES.tsv), and lower
   templates and derives as the
   [specification](../spec/14-annotations.md#typed-derivation) states.
2. **Library moves.** Move library code the prototype writes in
   TypeScript into `lib/std`, as the
   [Compiler/Library Audit](COMPILER_LIBRARY_AUDIT.md#migration-plan)
   plans.
3. **Packages.** Workspaces, version tags, minimal version selection, and
   `hd.sum` ([Package Manifest](../spec/10-modules.md#package-manifest),
   [Packages](PACKAGES.md)).
4. **CLI and Wasm size.** Redesign the CLI, and shrink the Wasm it emits.
5. **Performance.** Value-layout specialization (batch 37,
   [Shapes and Generic Code](../spec/04-type-system.md#shapes-and-generic-code)),
   measured by a microbenchmark suite.
6. **Error messages.** A Haiku probe of the messages
   ([hd writing log](../audit/hd-writing-log.md)), then the error-code
   revamp, task #101
   ([codes waiting for it](OPEN_ISSUES.md#codes-waiting-for-the-code-revamp)).

## Parked

- **NonEscapable**
  ([Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy),
  [research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)).
- **Tool adapters** ([Open Issues](OPEN_ISSUES.md#parked-tool-adapters)).
- **Access control and tenancy**
  ([Open Issues](OPEN_ISSUES.md#access-control-and-tenancy-expressibility)).
- **Serializable closures**
  ([Open Issues](OPEN_ISSUES.md#serializable-closures-and-incremental-computation)).

## On Hold

- **Program database.** A compiler-written SQL snapshot of program facts.
  The owner's 2026-09-27 decisions on it are in this file's git history.
