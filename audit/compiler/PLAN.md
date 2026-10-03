# Compiler Architecture And Conformance Audit

Status: Audit plan and preliminary evidence only. No proposed behavior is accepted, and this document records no owner design decisions.

## Objective And Baseline

The objective is a simple compiler whose mechanisms implement general language rules correctly, including their composition.
Slow representations and algorithms are acceptable when they preserve the specified behavior.
Special handling requires a semantic reason that applies to every relevant program.

The specification, compiler, and fixtures are each subjects of review.
Recorded owner decisions establish intent when current texts conflict.
Unresolved meaning becomes an owner question supported by the conflicting evidence.

| Item | Value |
| --- | --- |
| Initial baseline | `823f346878028aad4a4c9351593217f04445bd4c` |
| Original checkout | Unchanged by the audit |
| Audit worktree | Separate isolated Git worktree |
| Git mode | Initially detached; publication branch `audit/compiler-architecture-review`; original branch remains unchanged |
| Authorized writes | Audit reports, evidence, and audit tooling under this worktree's `audit/` |
| Initial verification | Source inspection; compiler tests have not run for this audit |

Publication rebased the audit-only worktree onto `3f4e24c39bbb8fcf0aa96b207c9891786b98d9fb`.
The reports and coverage ledgers describe the original reviewed baseline; source links preserve that commit.
[Publication checks](VALIDATION.md) validate the documentation change and do not revalidate every finding against newer source.

Scope includes [compiler sources](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/README.md), [language rules](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/README.md), [stdlib rules](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/std/README.md), and [CLI rules](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/cli/README.md).
It also includes [fixtures and their contract](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/README.md), [implementation tests](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/test/README.md), and relevant accepted decisions.
[Known issues](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/KNOWN_ISSUES.md) and [excluded cases](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/test/portable/KNOWN_FAILURES.tsv) are evidence leads requiring revalidation.

## Team Structure

There are four available execution slots: the coordinating reviewer and three subagents.
Subagents receive bounded packets rather than the entire compiler.
Every subsystem reviewer reads its rules, implementation, and tests together.
The coordinator retains responsibility for completeness and independently verifies important findings.

| Role | First assignment | Owned report |
| --- | --- | --- |
| Coordinator | Baseline, architecture map, pass contracts, spec consistency, coverage ledger, reconciliation | `PLAN.md`, `README.md`, later consolidated report and ledgers |
| Types reviewer | Type representation, checker state, inference, coercion, call resolution, dispatch | `types-first-pass.md` |
| Source reviewer | Modules, identity, standard library loading, source rewriting, derivation and templates | `source-first-pass.md` |
| Lowering reviewer | HIR, captured cells, ordinary and suspending control flow, Wasm representation, runtime bridge | `lowering-first-pass.md` |

These first assignments identify the architectural fault lines and support narrower follow-up packets.
They do not constitute full coverage of their subsystems.
Large areas, particularly the checker, require several packets before completion.

## Review Waves

| Wave | Reviewer A | Reviewer B | Reviewer C | Coordinator |
| --- | --- | --- | --- | --- |
| 1: Architecture reconnaissance | Types and checker invariants | Module and generated-source architecture | Lowering and runtime invariants | Scope, coverage method, pass boundaries, evidence review |
| 2: Foundational semantics | Lexer, layout, parser, spans, grammar/spec agreement | Name resolution, imports, privacy, package roots, initialization | Inference, assignability, mutability, type identity and variance | Baseline test execution, fixture judging, contracts between subsystems |
| 3: Semantic composition | Generics, traits, associated types, coherence, method resolution | Data, enums, patterns, exhaustiveness, GADTs | Functions, closures, captures, coercion, ordinary control flow | Cross-feature cases and spec interaction questions |
| 4: Advanced paths and libraries | Requirements, row inference, provider resolution, suspension typing | Derivation, annotations, facts, templates, standard library semantics | Backend, suspension lowering, cancellation, runtime representations | ABI and checker/emitter consistency |
| 5: Public behavior and adversarial evidence | CLI, package commands, REPL, diagnostics | Test runner, snapshots, fixture oracle, excluded-case reconciliation | Host bridge, replay, panic behavior, fuzz triage | Full rule/file coverage reconciliation and reproduction |
| 6: Challenge review | Challenge source/loading findings | Challenge lowering/runtime findings | Challenge type/checker findings | Consolidate confirmed evidence and incremental migration proposal |

Packets may move between waves when dependencies require it.
Critical contradictions are sent to the coordinator immediately, rather than waiting for a complete report.
Related findings across subsystem boundaries become shared probes with one report owner.

## Packet Contract

| Field | Required content |
| --- | --- |
| Scope | Exact files, rule IDs or sections, decisions, and fixtures under review |
| Baseline | Commit, toolchain if executed, and working-directory identity |
| Semantic model | Rules the subsystem is intended to enforce and relevant ordering or phase requirements |
| Invariants | Inputs promised by upstream passes and guarantees provided downstream |
| Architecture | Identity, representation, state ownership, repeated logic, and compiler-known names |
| Evidence | Precise locations, execution commands when applicable, expected and observed results |
| Composition | At least one interaction with another subsystem; identify assumptions it introduces |
| Coverage | Files and rules reviewed, depth of review, unchecked areas, and tests still needed |
| Disposition | Confirmed defect, architectural risk, spec issue, fixture issue, acceptable simplicity, or unresolved hypothesis |
| Handoff | Next smallest packet or experiment needed to resolve uncertainty |

Subagents write disjoint report paths and use explicit worktree paths in every filesystem command.
They do not edit compiler, standard library, specification, fixtures, or existing tests.
The coordinator owns shared ledgers and finding IDs, preventing conflicting edits and duplicate findings.
Audit reproductions belong under `audit/`; promoting them into the conformance suite requires a subsequent authorized change.

## Evidence Standards

A source mechanism can be confirmed by inspection while its behavioral consequence remains unverified.
Reports distinguish those two claims explicitly.
A passing fixture demonstrates its own case; it does not establish a general rule or validate the fixture's expectation.

| Classification | Evidence needed |
| --- | --- |
| Compiler defect | Coherent applicable rule, contrary observed behavior, and traceable implementation cause |
| Spec defect | Conflicting passages, undefined necessary interaction, or mismatch with a recorded owner decision |
| Fixture defect | Fixture expectation or judging behavior conflicts with the cited semantics |
| Architectural risk | Concrete representation or pass-boundary weakness and the class of programs it affects |
| Correctly naive | General mechanism, explicit invariants, and evidence that simplicity preserves observable behavior |
| Unsupported feature | Missing implementation plus explicit rejection behavior; diagnostic adequacy audited separately |
| Open design question | Existing decisions and rules cannot determine required behavior |

Severity, confidence, architectural scope, and verification status are separate fields.
One root cause can own several manifestations; fixture failures remain individually traceable.
A known finding receives fresh evidence and an assessment of whether its original explanation remains accurate.

## Spec And Fixture Review

Each reviewer checks relevant language, stdlib, and CLI rules, including cross-references between tiers.
Where current texts disagree, the reviewer locates the relevant recorded owner decision and documents any mismatch.
The compiler's behavior cannot settle ambiguous intent.
Examples, diagnostic inventories, fixture citations, and judging logic are also checked against normative rules.

The rule ledger has one row per rule, with primary owner, implementation locations, fixtures, disposition, and verification status.
Rules without automated evidence remain marked accordingly.
Individual or grouped rules require a manual rationale before being marked conforming or outside compiler scope.
Rule counts come from `pnpm run spec counts`, as required by the repository.

The file ledger records every scoped compiler, runtime, library, adapter, and test-tool file.
It distinguishes inventory, targeted inspection, full review, and behavioral verification.
Missing coverage remains visible until it has an owner and a completed packet.

## Execution And Reproduction

The coordinator schedules expensive checks so three reviewers do not launch competing full suites.
Raw command evidence records arguments, cwd, commit, timeout, exit status, stdout, stderr, and a report link.
Baseline execution covers the supported selection and the complete normative corpus, including excluded cases.
Runtime success, rejection, panic category, output, and located diagnostics are judged through the documented command contract.

Follow-up experiments include renaming, declaration reordering, aliasing, imports, generic nesting, and ordinary-versus-suspending forms.
An expected equivalence must first be justified from the rules; source transformations are not assumed harmless.
Fuzzing uses deterministic seeds, recorded budgets, timeouts, and minimized samples.
Grammar-generated failures require manual triage because the generator's layout rendering is approximate.

Execution follows repository checkout/check prerequisites in the audit worktree.
If that changes the reviewed commit, the coordinator records a new baseline and explicitly revalidates affected evidence.
The original checkout is not rebased or edited.
Required dependencies and generated test outputs must remain isolated; installation or execution cannot silently modify tracked files outside `audit/`.

New hd examples used as specification/conformance evidence follow the repository's fixture rules.
Other hd implementation or test writing follows the model and diagnostic-log instructions in AGENTS.md.
Model availability must be resolved before assigning such deliverables.

## Challenge Review And Synthesis

Before final acceptance, another reviewer challenges each high-impact conclusion.
The challenge checks for a missed lang item, legitimate specified exception, stale decision, invalid reproducer, or incorrect test oracle.
The coordinator checks interactions that individual subsystem reviewers could miss.
Disagreements are documented until evidence resolves them or they become owner questions.

Migration proposals follow confirmed invariants and failure mechanisms.
Candidate changes include structural type representation, qualified identities, explicit pass outputs, centralized lowering, and isolated runtime interfaces.
These are hypotheses until the audit establishes their need and sequencing.
Each proposed step includes affected paths, dependencies, removal opportunities, regression evidence, and owner decisions required.

## Deliverables And Completion

| Artifact | Purpose |
| --- | --- |
| `README.md` | Status, baseline, navigation, completed and pending work |
| `PLAN.md` | Review structure and evidence standards |
| `*-first-pass.md` | Bounded preliminary architecture evidence |
| Later subsystem reports | Detailed rule/implementation/fixture reviews |
| Later file and rule ledgers | Explicit coverage and remaining gaps |
| Later finding ledger | Deduplicated findings, severity, confidence, evidence and owner questions |
| Later evidence and reproductions | Commands, logs, minimized inputs, fuzz outputs |
| Later final report | Architecture judgment, confirmed defects, spec questions, and migration sequence |

Completion requires every scoped source file to be reviewed and every applicable rule to have an explicit disposition.
It also requires known failures to be reconciled, important findings to be challenged, and proposed migrations to have reviewable evidence.
Pending tests, missing tools, ambiguous semantics, and unreviewed areas remain explicit limitations.
The final report does not certify general correctness solely from passing tests.
