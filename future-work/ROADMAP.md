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
   serde, then traits and modules. A fixture the prototype fails
   gets a prototype fix or a tagged
   [known failure](../test/portable/KNOWN_FAILURES.tsv) row.
3. **Error-code revamp,** task #101, done (owner, 2026-10-06): the new
   compiler takes its codes from the
   [Diagnostics](../spec/README.md#diagnostics) table.
4. **Core decisions** (owner decisions, 2026-10-06). The parked items that
   shape the new compiler's core are ruled on. NonEscapable comes after
   v1. Serializable closures are not in v1. Suspension lowering reserves
   no-op observability and replay hook points from day one; the hook API
   and replay come later.

## Parked

- **NonEscapable**, planned after v1 (2026-10-06)
  ([Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy),
  [research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)).
- **Tool adapters** ([Open Issues](OPEN_ISSUES.md#parked-tool-adapters)).
- **Access control and tenancy**
  ([Open Issues](OPEN_ISSUES.md#access-control-and-tenancy-expressibility)).
- **Serializable closures**, not in v1 (2026-10-06)
  ([Open Issues](OPEN_ISSUES.md#serializable-closures-and-incremental-computation)).
- **Observability and replay hooks**: the hook API and replay come after
  the reserved no-op hook points (2026-10-06)
  ([Open Issues](OPEN_ISSUES.md#observability-hooks)).

## On Hold

- **Program database.** A compiler-written SQL snapshot of program facts.
  The owner's 2026-09-27 decisions on it are in this file's git history.
