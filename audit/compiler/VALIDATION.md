# Audit Publication Validation

Status: Documentation checks only. These results do not establish accepted behavior or reproduce the architectural findings.

| Item | Value |
| --- | --- |
| Reviewed commit | `823f346878028aad4a4c9351593217f04445bd4c` |
| PR base after fetch and rebase | `3f4e24c39bbb8fcf0aa96b207c9891786b98d9fb` |
| PR branch | `audit/compiler-architecture-review` |
| Toolchain | Node `v24.19.0`; repository pnpm scripts |
| Tracked change scope | `audit/compiler/` only |

The audit worktree was rebased before checks, as required by the repository.
The original checkout's files and branch were unchanged.
Existing dependencies were read through a temporary worktree symlink; the build output stayed in that worktree's ignored `website/dist/`.
The dependency symlink was removed after validation.

| Check | Result | Meaning |
| --- | --- | --- |
| `bash spec/check.sh` | Passed | Fixture/index, spec links, rule citations, tiers, style, fuzzer imports, example inventory, and package-source parse gates succeeded |
| `node --experimental-strip-types test/run-portable.ts --changed` | Exited 0; no selected fixture differs from `origin/main` | No fixture executions were selected for this audit-only change |
| `pnpm run website:build` | Passed; 44 pages with playground | Existing website and playground build succeeded |
| Audit Markdown file/line target check | Passed; 316 targets, zero failures before this validation page was added | Linked repository paths exist at the reviewed commit; numeric line targets are within bounds; local report files exist |

The spec check reported eight warning-level dead citations in existing compiler sources and zero failing citations.
These warnings are outside the audit change scope.
The link check validates file existence and numeric line bounds, not every Markdown section anchor or each finding's interpretation.

All source links in the reports now point to the exact reviewed commit.
The file/rule ledgers and stored rule counts remain historical snapshots of that commit.
Newer upstream testing-capability changes have not been incorporated into a fresh architecture review.
The report's pending reproductions and remaining coverage therefore stay pending.
