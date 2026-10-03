# Remaining Compiler Audit

Status: Unfinished architecture and conformance work, reconciled at `f9760e81`. No proposal here is an owner decision.

Only remaining work belongs in this report and [findings.tsv](findings.tsv).
Completed evidence is available in Git history and the compiler's regression tests.
This is a bounded review, not a claim of full compiler or specification coverage.

## Findings

| ID | Priority | Remaining scope | Evidence status |
| --- | --- | --- | --- |
| A01 | High | Package ownership, module privacy, import aliases, test isolation, initialization scheduling | Linker mechanisms and known failures confirmed |
| A02 | High | Semantic type representation and expected-type coercion consistency | Architecture confirmed; additional behavior needs current reproduction |
| A06 | High | Public pending/waker protocol | Confirmed gap; owner deferred implementation |
| A07 | Medium | Host ABI consistency and replay identity | Contract review and code reconciliation remain |

### A01: Package Scope And Initialization

[src/package.ts](../../src/package.ts) joins modules into one namespace, rejects independent same-named declarations, and removes package imports.
This loses ownership needed for module-private lookup and integration-test isolation.
Whole-file ordering cannot schedule dependency-ready statements across a module initialization group.

Relevant rules are in [Modules and Packages](../../spec/lang/10-modules.md):
`module.vis.private-default`, `module.vis.no-package-private`, `module.test.integration.view`, and `module.init.group.step`.
[Known issues](../../src/KNOWN_ISSUES.md) track DC7, TASK-PROGRAMS, and SELF-CURRENT.
Namespace imports and package aliases remain explicitly unsupported.

Required next evidence: independent same-named declarations; implicit cross-module access; integration tests' library view; interleaved initialization; package import aliases.
Preserve declaration owners and lexical bindings until resolution is complete.
A spelling-only workaround would leave the scope rules incomplete.

### A02: Semantic Types And Coercion

Semantic types remain encoded as strings across checking and emission.
Expected-type coercion is distributed across checker paths.
Those architectural facts do not establish a new behavioral failure by themselves.

Compare identical source and destination types at arguments, bindings, assignments, fields, and results.
Include nested custom containers and binder shadowing.
Use [Type System](../../spec/lang/04-type-system.md) as the oracle, including its restriction on combining outer permission weakening with variance.
Any remaining discrepancy needs a current reproduction.

### A06: Pending And Wake Delivery

Public synchronous suspension drivers repeatedly poll while pending.
[`req.entry.pending`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.pending) requires returning control to the host and waiting for a wake.
[`req.entry.busy-poll`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll) rejects polling without returning control.
Low-level poll exports and deterministic pending callbacks do not establish the public scheduler contract.

The owner deferred implementation.
[Wake-Driven Host Entries](../../future-work/HOST_ENTRY_DRIVER.md) holds the proposal and verification plan.

### A07: Host ABI And Replay Identity

[Host provider emission](../../src/emitter/host-providers.ts) and [host instantiation](../../src/compiler.ts) describe boundary types separately.
Compare their admitted scalar and `Result` types, payload encoding, and callback normalization.
Locate the contract for malformed host values before classifying normalization as a language defect.

[Known issue F-401](../../src/KNOWN_ISSUES.md) records replay identity based on function source rather than module semantic content.
Reconcile [runtime decisions](../../future-work/OPEN_ISSUES.md), normative determinism, and existing replay tests before changing identity.
Determine how callee changes, formatting changes, and code dependencies affect replay acceptance.

## Further Coverage

Generated helper and local-name hygiene beyond placeholders remains unreviewed.
Broader lowering composition and untouched checker, CLI, library, and fixture paths also require review.
These are coverage gaps, not confirmed defects.
Record a current reproduction before promoting one into the finding ledger.
