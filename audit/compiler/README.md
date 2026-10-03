# Compiler Audit

Status: Open findings reconciled at `42770b9d`. The broader audit remains incomplete; no proposed behavior is accepted here.

Start with [the current report](REPORT.md) and [open finding ledger](findings.tsv).
They contain only remaining work, with repaired portions removed from partially open findings.
The target remains a naive compiler that correctly applies general rules and their interactions.

| Finding | Remaining scope |
| --- | --- |
| A01 | Package declaration ownership, module privacy, test isolation, and initialization scheduling |
| A02 | Semantic type representation and consistent expected-type coercion |
| A06 | Public pending/waker protocol and the deferred host-facing entry API |
| A07 | Shared host ABI contracts and replay identity |

[REPAIRS.md](REPAIRS.md) retains completed repair evidence, regression links, and edge cases for later conformance fixtures.
It is a repair history, not an open issue list.

The first-pass and challenge packets are historical evidence pinned to `823f346878028aad4a4c9351593217f04445bd4c`.
Their descriptions of fixed defects do not describe the current compiler.
They cover [types](types-first-pass.md), [source loading](source-first-pass.md), [lowering](lowering-first-pass.md), [source challenges](source-challenge.md), [type challenges](types-challenge.md), and [representation challenges](representation-challenge.md).

[File coverage](file-coverage.tsv), [rule coverage](rule-coverage.tsv), and [rule counts](spec-counts.txt) are also baseline snapshots.
They record review scope, not complete conformance or current coverage.
The [execution plan](PLAN.md) records the original audit process; [publication validation](VALIDATION.md) records the original publication checks.
