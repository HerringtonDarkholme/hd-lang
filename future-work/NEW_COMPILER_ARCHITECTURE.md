# New Compiler: Architecture Notes

The owner's direction for the new compiler and CLI, recorded as given.
Nothing here is spec; the [specification](../spec/README.md) stays
authoritative, and anything that changes language behavior goes through it.

## Context

- **Decision, 2026-10-06:** start the new compiler now; the prototype in
  [`src/`](../src/README.md) is frozen as a test oracle
  ([Roadmap](ROADMAP.md#order)).
- **Measured state of the prototype:**
  [baseline report](../audit/compiler/baseline-2026-10-06.md).
- **Earlier analysis:**
  [architecture directions, 2026-10-05](../audit/compiler/architecture-directions-2026-10-05.md),
  [status quo](../audit/compiler/status-quo-2026-10-04.md).
- **Core decisions already made (owner, 2026-10-06):**
  - NonEscapable comes after the first release.
  - No serializable closures in the first release.
  - Suspension lowering reserves no-op observability and replay hook
    points from day one.
  - `usize` is target-defined: 32 bits on Wasm32.

## Owner's Notes

<!-- Each entry: date, topic, the owner's point as stated. -->

### Goals (2026-10-06)

1. **Fast.**
2. **Parallel.**
3. **Supports incremental builds.**

### How To Judge It: The Agentic Programming Language Arena (2026-10-06)

The owner's framework for comparing languages and toolchains for agentic
development. It has three pillars. They are not collapsed into one score;
the comparison looks at the **Pareto frontier**.

**1. Single-agent development cost.** What does it cost one agent to
produce a correct final artifact?

- Two measures only: **elapsed time** and **token cost**. Dollar cost is
  token use priced per model, so a separate dollar metric would count the
  same cost twice.
- Iterations, retries, compiler errors and debugging loops already show up
  in those two numbers, so they need no separate metrics.
- Failed attempts count too. A failed run still adds time and tokens.
  Comparing only successful runs misleads: a language that succeeds 95% of
  the time beats one that succeeds 60% even if its successful runs cost
  the same.
- The measure: **the expected elapsed time and token cost for one agent to
  obtain a correct solution.**

**2. Agent scalability.** Agentic development needs many agents working in
parallel, not one worker at a time.

- The question: **given a fixed compute or infrastructure budget, how much
  useful development work can run concurrently?**
- The resources: memory, CPU and disk footprint.
- The measure is the aggregate useful work of **N concurrent agents**, not
  the footprint of one agent. An environment that supports 100 parallel
  workers has a different economic profile from one that supports 20.

**3. Artifact quality.** What did the agents actually produce? Measure
observable properties of the result, not whether the source "looks good".

- **Correctness:** predefined unit tests, integration tests, property-based
  tests, end-to-end and computer-use tests.
- **Runtime performance:** execution time, throughput, latency, CPU use,
  memory use, executable size.
- Correctness is a gate: a faster wrong program is not a better outcome.

## What The Arena Asks Of The Compiler (orchestrator's reading, not owner text)

- **Pillar 1, development cost:**
  - Each check-edit loop is part of an agent's elapsed time, so check
    latency on a small edit matters more than full-build speed. This is
    where incremental builds pay.
  - Diagnostics that name the fix cut retries, and so cut tokens. The
    [hd writing log](../audit/hd-writing-log.md) records which messages
    failed agents.
  - Deterministic, machine-readable output (`--format json`) keeps an
    agent from re-running commands to parse them.
- **Pillar 2, scalability:**
  - The measure is per-agent memory, CPU and disk when N agents check and
    test at once.
  - That favors a shared, content-addressed cache of checked std and
    dependencies over a per-process copy. Today each process re-checks
    std (task P4c, deferred).
  - It also favors a small resident memory per check, and no heavyweight
    daemon per agent.
  - The prototype's numbers: about 0.3 s to check a tiny program, and 15–17
    minutes for a full suite when several worktrees ran it at once,
    against 1.7 minutes alone.
- **Pillar 3, artifact quality:**
  - Correctness is pinned by the portable conformance suite, which is the
    gate.
  - Runtime performance and executable size are what the prototype does
    worst: 2.5–11x slower than Node outside recursion
    ([baseline](../audit/compiler/baseline-2026-10-06.md)).

## Follow-Up Questions

<!-- Questions that the notes raise, each with a recommendation. -->

- **Cleanup that suspends** (from usability probe 6). `defer` can't make
  a bang call (`flow.defer.suspend`), so a temp file can't be removed in
  a `defer`. This is queued for the owner. The recommendation is to keep
  the rule for the first release and revisit it with NonEscapable. It
  touches how suspension is lowered, so the new compiler should leave
  room for it.
