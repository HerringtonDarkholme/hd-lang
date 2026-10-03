# Compiler Architecture And Conformance Audit: First Wave

Status: Open findings reconciled against repairs and the specification at `42770b9d`. No proposed behavior or owner decision is accepted here.

This report is the current work list.
Original source deductions remain in the historical first-pass and challenge packets.
[REPAIRS.md](REPAIRS.md) records completed repairs and validation; [findings.tsv](findings.tsv) contains only open findings.
This reconciliation does not establish a complete compiler or specification audit.

## Current Assessment

Remaining architectural risks concern module ownership, semantic type representation, coercion, and host execution contracts.
Scope-aware std bindings, optional boundaries, method variance, least-common-type inference, candidate transactions, capture traversal, suspension exits, and generated placeholders have recorded repairs.
Their original failure predictions are removed from this work list.

## Prioritized Findings

| ID | Priority | Remaining finding | Evidence and limits |
| --- | --- | --- | --- |
| A01 | High | Package linking loses module ownership and statement-level initialization order | SOURCE-1; std binding and local impl defects repaired |
| A02 | High | Semantic types remain string-encoded and expected-type coercion remains distributed | Remaining architecture review; original optional, method-variance, callable-row, and LCT defects repaired |
| A06 | High | Public suspension execution lacks the specified pending/waker protocol | L2; host-facing entry API deferred by owner to future work |
| A07 | Medium | Host ABI definitions and replay identity need a common contract | L4/L5; captured-cell conversion defects repaired |

### A01: Preserve Declaration Identity, Ownership, And Lookup Extent

Remaining scope: package declaration ownership, module-private visibility, independent declaration spellings, integration-test isolation, and initialization groups.
Flattening package modules into one namespace prevents later checking from applying rules that depend on their original owners.
Ordering whole files also cannot express dependency-ready initialization of individual statements across a module group.

Review against [Modules and Packages](../../spec/lang/10-modules.md), especially `module.vis.private-default`, `module.vis.no-package-private`, and `module.init.group.step`.
Historical evidence is [SOURCE-1](source-first-pass.md#source-1-the-package-linker-erases-the-information-needed-for-correct-modules).
Current deviations are tracked in [compiler known issues](../../src/KNOWN_ISSUES.md).

The next packet should test independent same-named declarations, cross-module private access, integration tests' library view, and interleaved initialization dependencies.
Preserving declaration ownership and import bindings through resolution remains the architectural direction to investigate.

### A02: Keep Semantic Type Structure And One General Conversion Relation

Remaining scope: string-encoded semantic types, binder identity, and the consistency of expected-type coercion across checker paths.
Audit nested custom-container conversions against [Type System](../../spec/lang/04-type-system.md).
Shared Wasm layouts alone do not establish legal conversions.
Original optional collision, readonly method variance, callable requirement rows, generic provider-key erasure, and specified least-common-type sites have recorded repairs.

The next packet should compare arguments, bindings, assignments, fields, and results using identical source and destination types.
Any further mismatch needs a current reproduction before it becomes a confirmed defect.
The prohibition on combining outer permission weakening with a variance step remains part of the specification.

### A06 And A07: Specify And Share The Runtime Boundary

A06 concerns public execution when a host operation returns `Pending`.
Rules [`req.entry.pending`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.pending) and [`req.entry.busy-poll`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll) require returning control and waiting for wake delivery.
Deterministic poll callbacks do not establish an event-loop or waker contract.
The owner deferred the host-facing entry API to [Wake-Driven Host Entries](../../future-work/HOST_ENTRY_DRIVER.md); this audit keeps the conformance gap visible.

A07 retains the shared host ABI and replay-contract questions from [L4/L5](lowering-first-pass.md#l4-host-abi-behavior-is-duplicated-and-only-partially-validated).
Emitter and host descriptions should agree on admitted types, encoding, and invalid host values.
Malformed callback handling needs an explicit contract before it can be classified as a language defect.
Replay code identity requires reconciliation with owner records and normative guarantees.

Captured-cell conversion now uses explicit closure indices and exhaustive HIR traversal, with regression coverage recorded in the repair history.
That repaired mechanism is removed from A07's remaining scope.

## Specification Findings And Questions

The original testing contradictions S01/S02 are resolved in the current specification.
[Grammar](../../spec/lang/02-grammar.md#r-grammar.tests.registration) delegates the registration set to the language-tier [test-position rule](../../spec/lang/10-modules.md#r-module.testing.position-statements).
[Std testing](../../spec/std/testing.md#registration-functions) assigns compiler-checked registration names, options, and positions to the language tier.
Neither remains an open finding.

| Remaining question | Evidence needed |
| --- | --- |
| Host scalar contract | Locate the ABI contract for malformed callbacks and identify compiler obligations |
| Replay identity | Reconcile owner runtime decisions, normative determinism, and prototype identity experiments |

## Next Review And Migration Priorities

| Order | Evidence to obtain |
| --- | --- |
| 1 | Package privacy, independent names, integration-test isolation, and statement-level initialization |
| 2 | Consistent expected-type coercion and nested container conversions at every typing site |
| 3 | Generated helper/local-name hygiene beyond repaired placeholders |
| 4 | Public pending/wake behavior, admitted host types, ABI encoding, and replay identity |
| 5 | Untouched semantic paths, all three specification tiers, fixtures, and judging logic |

Generated helper hygiene and broader lowering composition are coverage gaps, not replacement defect IDs for closed findings.
Each new claim needs its own current evidence.
Completed repair edge cases remain available in REPAIRS.md for later fixture work.

## Supporting Reports And Coverage Limits

Original [type](types-first-pass.md), [source](source-first-pass.md), and [lowering](lowering-first-pass.md) packets and their [source](source-challenge.md), [type](types-challenge.md), and [representation](representation-challenge.md) challenges are historical evidence.
They describe baseline `823f346878028aad4a4c9351593217f04445bd4c`, including defects subsequently repaired.
Their original claim status must not be read as current issue status.

[File coverage](file-coverage.tsv), [rule coverage](rule-coverage.tsv), and [counts](spec-counts.txt) remain baseline inventories.
The audit still lacks complete review of checker branches, grammar rules, CLI commands, library APIs, runtime helpers, fixtures, and owner records.
Only the four findings above remain in the active consolidated ledger.
