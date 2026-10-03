# Compiler Architecture And Conformance Audit: First Wave

Status: Preliminary audit evidence. No proposed behavior is accepted, and no owner design decision is made here.

Baseline: `823f346878028aad4a4c9351593217f04445bd4c`.
This report combines three focused architecture reviews and the coordinating reviewer's source/spec reconciliation.
Finding-specific compiler reproductions and full behavioral suites have not run for this audit.
Descriptions of predicted failures below are source deductions, not recorded execution results.
The audit-only PR is based on `3f4e24c39bbb8fcf0aa96b207c9891786b98d9fb`, after the required fetch and worktree rebase.
Source links and coverage refer to the reviewed baseline above; [publication validation](VALIDATION.md) records checks on the newer base.

The scope is [compiler architecture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/README.md), [language semantics](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/README.md), [stdlib semantics](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/std/README.md), [CLI semantics](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/cli/README.md), and [conformance evidence](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/README.md).
Relevant recorded directions appear in [AGENTS.md](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/AGENTS.md).
Owner decision history still requires broader reconciliation.
See [PLAN.md](PLAN.md) for the remaining review waves and completion criteria.

## Current Assessment

The compiler contains useful general mechanisms: typed HIR expressions, dictionary-based dispatch, shared capture cells, explicit suspension frames, and a suspension CFG.
Several surrounding passes discard identities, scope, or type structure before enforcing the rules that require that information.
Other passes substitute spelling and expression shape for semantic resolution.
Those mechanisms are the principal obstacles to a naive but correct implementation.

This first wave supports that architectural assessment, but does not establish complete conformance coverage.
The [rule inventory](spec-counts.txt) reports 4,273 rules: 3,810 language, 338 stdlib, and 125 CLI.
The [rule ledger](rule-coverage.tsv) and [file ledger](file-coverage.tsv) keep incomplete review visible.
Neither inventory is evidence that its entries have been fully audited.

## Actual Pipeline And Boundary Problems

| Stage | Current implementation | Information or invariant at risk |
| --- | --- | --- |
| Package loading | Parse files, validate selected imports, order modules, concatenate edited source | Declaration ownership, lexical imports, module-local identity, statement-level initialization |
| Std loading | Expand intrinsic text, parse, rename source, reparse, respan, join ASTs | Canonical declaration identity, alias bindings, lexical resolution, original provenance |
| Derivation | Emit source with placeholders, parse it, patch arbitrary object fields | Hygienic names, typed positions, generated-origin tracking |
| Type preparation | Store type syntax and semantic types in strings; hoist local declarations | Constructor nesting, binder identity, local implementation availability |
| Checking | Inherited mutable contexts; partial expression handlers; candidate trials | Complete trial isolation, one conversion relation, exhaustive semantic coverage |
| Closure conversion | Rewrite captured locals using object identity and reflective traversal | Stable storage identity and a checked post-conversion HIR contract |
| Control-flow lowering | Ordinary emission, linear suspension continuation, or suspension CFG | A single meaning of return, propagation, cleanup and completion |
| Host execution | Generated synchronous drivers and callback/replay bridge | Waker delivery, shared ABI definitions, durable code/site identity |

## Prioritized Findings

Priority reflects correctness impact and architectural reach, rather than code size or runtime speed.
The finding IDs below consolidate the subsystem reports; one root cause can account for several failed programs.

| ID | Priority | Consolidated finding | Verification status | Source findings |
| --- | --- | --- | --- | --- |
| A01 | High | Resolution loses declaration ownership and binding scope | Confirmed transformations; behavioral probes pending | SOURCE-1, SOURCE-2, SOURCE-4, T5 |
| A02 | High | Encoded types lose structure; conversion and inference use inconsistent relations | Confirmed collision and algorithm limitations; probes pending | T1, T3, T4 |
| A03 | High | Three control-flow paths can disagree on language return and suspension completion | Confirmed duplicated paths; specific nested-exit failure requires reproduction | L1 |
| A04 | High | Generated-source patching can capture legal user identifiers | Confirmed substitution mechanism; end-to-end probe pending | SOURCE-3 |
| A05 | High | Generic method selection substitutes a syntax blacklist for isolated candidate checking | Confirmed selection branch; unique-fit probe pending | T2 |
| A06 | High | Public suspension drivers contradict the specified waker protocol | Confirmed emitted polling loops and contradictory rule; public execution probe pending | L2 |
| A07 | Medium | Runtime contracts depend on reflective rewrites, duplicated ABI rules, and experimental replay identity | Confirmed mechanisms; several behavioral requirements remain unresolved | L3, L4, L5 |
| S01 | High | Normative test-block registration rules conflict | Confirmed textual contradiction | SOURCE-5 |
| S02 | High | Compiler-enforced testing rules cross the stated specification tier boundary | Confirmed direction/text mismatch; intended resolution remains an owner question | SOURCE-5 |

### A01: Preserve Declaration Identity, Ownership, And Lookup Extent

[Package linking](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/package.ts#L482) deletes package imports and joins modules into one namespace.
That prevents later resolution from applying module-private visibility or distinguishing repeated declaration spellings in independent modules.
Whole-file ordering also fails to represent initialization groups that schedule individual statements across modules.
The relevant rules include `module.vis.private-default`, `module.vis.no-package-private`, and `module.init.group.step`.

[Std module-call rewriting](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts#L607) recognizes receiver spellings before resolving local scope.
A parameter shadowing an imported module alias can therefore have its method call rewritten as a std function call.
[Std name mapping](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts#L491) gives each declaration one local spelling, so a second alias replaces the first declaration name.
Both problems follow from treating bindings as source substitution instead of references to declarations.

[Local implementation hoisting](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/local-declarations.ts#L137) removes implementation statements and appends them to a program-wide list.
It retains no declaration-point or suite availability for later method lookup.
Local type names can still remain lexically scoped; the narrower risk concerns calls whose local nominal identity is already visible.
Rules `names.local-impl.extent` and `trait.impl.local.lookup` require that availability information.

### A02: Keep Semantic Type Structure And One General Conversion Relation

[HIR types](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/hir.ts#L3) are strings, and parsing plus optional construction collapses `(mut User)?` and `mut User?` into one encoding.
These denote different constructor/permission structures under `types.option.sugar` and `types.option.invariant`.
The source trace explains the existing VARIANCE-UNWRAP finding without reproducing it.
Downstream code cannot recover a distinction already discarded upstream.

[Variance verification](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/variance.ts#L86) checks fields and enum payloads before inherent methods are prepared.
It cannot enforce `types.variance.surface` against method signatures it never receives.
[Least-common-type candidates](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/assignability.ts#L143) specialize List covariance and lack general declared variance information.
Ordinary coercion uses another relation, which itself does not recurse through custom variance like built-in variance.

[Branch joins](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/expression-control.ts#L94) and [inferred returns](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/context.ts#L1189) use yet other paths: equality, `never`, or function-row union.
They do not consistently use the candidate algorithm, so repairing that helper alone would leave these sites incomplete.
Explicit `Option[mut User]` is normalized through the same optional helper and does not avoid the representation collision.
The independent [representation challenge](representation-challenge.md) verified these distinctions and the backend's inability to restore erased semantic structure.

The consequence is broader than a missing case for one container.
Inference, validation, coercion, and representation need an explicit common semantic model, with intentional restrictions represented separately.
The exact least-common-type candidate rules still need spec interpretation in ambiguous optional-conversion cases.
The documented prohibition on combining outer permission weakening with a variance step must remain part of that model.

### A03: Give Language Exits One Owner

[Suspension CFG selection](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/suspension.ts#L138) depends on where drives occur.
[Linear continuation emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L1005) handles top-level returns specially, while a non-driving nested branch reaches ordinary emission.
[Ordinary return emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L128) returns the language value directly from the current Wasm function.
Inside a poll function, that can bypass frame result storage, completion state, and registered outer cleanup.

The prediction needs a minimal compiled reproduction, including non-i32 results and separate postfix propagation.
The architectural risk is already established: three paths separately implement return, cleanup, and propagation.
Rules `flow.return.value` and `flow.defer.run` apply regardless of the placement of a bang call.
One explicit lowered control-flow representation is a candidate remedy to evaluate after reproduction.

### A04: Generated Nodes Need Structural Placeholders And Hygienic Identity

[Generated-source patching](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/generated-source.ts#L69) replaces `HDTYPE<n>X` in arbitrary AST string fields except `value`.
Generated member access also embeds the user's original field spelling.
A legal field named `HDTYPE0X` consequently matches an internal type placeholder and receives a different name.
The substitution mechanism is proven by inspection; the precise end-to-end diagnostic remains unverified.

Source generation can be correct when syntax, bindings, placeholders, and provenance are preserved.
Distinctive legal identifiers and broad object traversal do not provide that guarantee.
Typed AST construction or explicitly tagged placeholder nodes are candidate mechanisms.
The audit must also examine expression-placeholder hygiene before settling the migration boundary.

### A05: Make Candidate Checking Independent Of Argument Shape

[Generic trait selection](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/expression-calls.ts#L757) rejects speculation-unsafe arguments when several candidates are present.
[The blacklist](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/call-speculation.ts#L7) includes closures, branches, loops, comprehensions, provider expressions, and suspension calls.
The specified selection rule uses argument and expected-result fit, without that syntactic restriction.
This affects multiple candidate instantiations; a single candidate does not pass through this rejection branch.

Removing the blacklist alone would expose shared mutable checker state to speculative trials.
The current trial restores diagnostic length rather than a complete checking environment.
The useful architectural question is whether explicit constraints, isolated state, or transactions give each candidate an independent result.
The language behavior is already defined by `trait.resolve.fits` and `trait.resolve.one-fit`.

### A06 And A07: Specify And Share The Runtime Boundary

[Generated suspension drivers](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L881) repeatedly poll while pending.
Rules `req.entry.pending` and `req.entry.busy-poll` require returning control to the host and waiting for wake delivery.
The bridge offers deterministic pending callbacks but no equivalent wake-delivery contract in the reviewed API.
Low-level start/poll exports are a useful foundation; they do not establish public driver conformance.

Shared capture cells, boxing, and dictionary passing are reasonable simple mechanisms.
The risks concern object-identity-dependent rewriting, non-exhaustive reflective visitors, and separate emitter/host descriptions of the ABI.
Malformed host scalar normalization needs an explicit host-input contract before it can be classified as a language defect.
Replay source hashing is established, while normative code equivalence and the applied Replay Rules still need reconciliation.

## Specification Findings And Questions

| Finding | Conflicting evidence | Required resolution |
| --- | --- | --- |
| S01: test registrations | `grammar.tests.statements` permits only `it`; `module.testing.position-statements` and `std-testing.registration` permit additional registrars | Determine accepted registration set from owner decisions and correct the inconsistent rule |
| S02: testing tier | AGENTS tier test assigns compiler-known names, position checks and item diagnostics to language; std/testing holds such rules | Establish the intended language/stdlib boundary before changing compiler recognition |
| Variance surface clarification | “Readonly public surface” and “every inherent method available with the nominal type” leave private-method scope unclear | Reconcile visibility rules and owner decisions; this does not excuse checking no methods |
| Least-common-type clarification | Broad implicit-conversion wording and narrower candidate exclusions do not fully settle optional insertion | Resolve only the ambiguous portion; declared-variance coverage has independent evidence |
| Host scalar contract | Host bridge silently normalizes numbers; reviewed rules do not define malformed callback handling | Locate the ABI contract and distinguish invalid host behavior from compiler obligations |
| Replay identity | Normative determinism names code identity; recorded runtime decisions describe a stronger identity scheme | Reconcile runtime decisions, language guarantees and prototype experiments |

The fixture that uses `it_prop` cannot settle S01 by itself.
Its acceptance agrees with one normative passage and conflicts with another.
Similarly, a prototype replay test cannot establish the intended program-identity contract.
Each question needs evidence from the applicable owner record.

## What Remains Acceptably Naive

The reviewers found explicit recursion tracking for signature inference, fixed-point row inference, shared heap cells, and dictionary dispatch.
These are plausible general algorithms whose complexity alone does not make them incorrect.
The bound-depth limit of 64 is specified behavior, so its presence is not evidence of an arbitrary shortcut.
Byte-at-a-time host copying and linear dispatch also require separate semantic and performance assessments.

An association-list Map cannot be condemned solely because lookup is linear.
Its correctness still depends on specified equality, hashing, insertion order, key mutation and alias behavior.
Those obligations remain for a later library/runtime packet.
Optimization proposals must not obscure the earlier loss of semantic information.

## Next Review And Migration Priorities

| Order | Evidence to obtain | Architectural direction to evaluate |
| --- | --- | --- |
| 1 | Reproduce optional collision, scope-blind rewrite, lost aliases and nested suspension exits | Retain type structure, declaration identity and explicit exits |
| 2 | Test local impl extent, method variance and custom-container joins | Preserve lexical availability and centralize conversion/variance obligations |
| 3 | Challenge generated-name hygiene and candidate trial state | Typed transformations and isolated checking state |
| 4 | Exercise public pending/wake behavior and admitted host capability types | One executor protocol and shared ABI description |
| 5 | Audit untouched semantic paths, all three spec tiers, fixtures and judging logic | Complete file/rule coverage before final judgment |

Implementation priorities are provisional until behavioral reproduction and peer challenge complete.
Every migration step must preserve observable rules and retain explicit diagnostics for unsupported behavior.
Changes to accepted language behavior require an owner decision and the repository's spec-update process.

## Supporting Reports And Coverage Limits

| Report | Focus |
| --- | --- |
| [Types first pass](types-first-pass.md) | Type structure, inference, candidate selection, variance and local impl scope |
| [Source first pass](source-first-pass.md) | Module identity, std bindings, generated-source hygiene and testing spec consistency |
| [Lowering first pass](lowering-first-pass.md) | Control flow, suspension protocol, capture conversion, ABI and replay |
| [Source challenge](source-challenge.md) | Independent checks of ownership loss, shadowing, hygiene, aliases and testing spec conflicts |
| [Types challenge](types-challenge.md) | Independent checks and qualifications of candidate selection, variance, joins and local implementation scope |
| [Representation challenge](representation-challenge.md) | Checker/emitter reconciliation, alias identity and nested suspension exits |

All three focused reviews and all three challenge packets are complete and incorporated into this first-wave synthesis.
The challenge process corrected overbroad ownership, candidate-selection, and scope claims, and identified additional join sites.
This wave does not fully review every checker branch, grammar rule, CLI command, library API, runtime helper, fixture, or owner record.
Known failures have been inspected rather than rerun.
The next waves must provide behavioral evidence and fill the explicit coverage gaps before a full audit claim is made.
