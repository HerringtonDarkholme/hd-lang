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

1. **New compiler** (owner decision, 2026-10-06). The owner writes a new
   compiler and CLI. The prototype in [`src/`](../src/README.md) is frozen
   as a test oracle: bug fixes and spec-sync fixes only, no new features
   and no performance work. Its state is recorded in the
   [baseline report](../audit/compiler/baseline-2026-10-06.md). These
   move to the new compiler: the CLI redesign including `hd doc`, Wasm
   size, value-layout specialization (batch 37,
   [Shapes and Generic Code](../spec/lang/04-type-system.md#shapes-and-generic-code)),
   caching checked std, and the specializing iterator design.
2. **Conformance coverage.** Fixtures cite only about a third of the spec's
   rules. New fixtures serve both compilers. Thinnest first: data and enums,
   GADTs, serde, then traits and modules. A fixture the prototype fails
   gets a prototype fix or a tagged
   [known failure](../test/portable/KNOWN_FAILURES.tsv) row.
3. **Error-code revamp,** task #101
   ([codes waiting for it](OPEN_ISSUES.md#codes-waiting-for-the-code-revamp)),
   settled before the new compiler fixes its codes.
4. **Core decisions.** Before the new compiler's core is fixed, the owner
   rules on the parked items that shape it: NonEscapable (a checker
   rule), serializable closures (closure representation), and the
   observability and replay hooks (how suspension compiles).

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
