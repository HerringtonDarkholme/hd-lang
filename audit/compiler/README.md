# Compiler Audit

Status: Preliminary review in progress. No proposed behavior is accepted, and no compiler or specification changes have been made.

The audit targets a naive implementation that correctly applies general rules and their interactions.
It reviews [compiler architecture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/README.md), [specification](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/README.md), and [conformance evidence](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/README.md) together.

| Item | Current status |
| --- | --- |
| Baseline | `823f346878028aad4a4c9351593217f04445bd4c` |
| PR base | `3f4e24c39bbb8fcf0aa96b207c9891786b98d9fb`; reviewed evidence remains pinned to the baseline above |
| Worktree | Separate isolated Git worktree |
| Original checkout | Unchanged by the audit |
| Coordination | One coordinator and three bounded reviewers |
| Manual review | Three first-pass architecture packets and three independent challenge packets completed |
| Behavioral checks | Pending; no audit test results claimed |
| Full file/rule coverage | Inventoried in file and rule ledgers; review remains partial |
| Final architecture judgment | Pending |

Start with [the consolidated first-wave audit](REPORT.md) for the main agent's reconciled findings and review limits.
See [the execution plan](PLAN.md) for assignments, review waves, evidence standards, and completion criteria.
Initial reports cover [types](types-first-pass.md), [source loading and generation](source-first-pass.md), and [lowering and runtime](lowering-first-pass.md).
Independent challenges cover [source findings](source-challenge.md), [type findings](types-challenge.md), and [cross-boundary representation](representation-challenge.md).
Coverage is tracked in [file-coverage.tsv](file-coverage.tsv) and [rule-coverage.tsv](rule-coverage.tsv).
Rule totals come from the repository's [spec counts output](spec-counts.txt).
Source links are pinned to the reviewed commit, including line references.
See [publication validation](VALIDATION.md) for the required documentation checks on the newer PR base.

Complete file reads are recorded separately from targeted inspection and behavioral validation.
The next evidence pass prioritizes the scope, type, alias, generated-name and nested suspension-exit predictions in the report.

The next wave covers lexical rules, names/modules, and foundational type semantics in bounded packets.
Subsequent waves cover remaining semantic subsystems, public commands, runtime behavior, adversarial tests, and independent challenge review.
