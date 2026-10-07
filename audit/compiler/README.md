# Compiler Audit

Reports on the frozen prototype compiler that the new compiler needs.
Fixed findings are deleted (git history keeps them).

- [baseline-2026-10-06.md](baseline-2026-10-06.md): the measured state of
  the prototype and CLI when it was frozen.
- The owner's 2026-10-04/05 architecture audit of the current compiler and
  directions for the next one: [status-quo-2026-10-04.md](status-quo-2026-10-04.md),
  [findings-2026-10-04.md](findings-2026-10-04.md),
  [architecture-directions-2026-10-05.md](architecture-directions-2026-10-05.md),
  and the per-area notes and scripts beside them.

Open prototype defects, speed findings included, are tracked by tag in
`test/portable/KNOWN_FAILURES.tsv` and by F-id in
[`src/KNOWN_ISSUES.md`](../../src/KNOWN_ISSUES.md). Notes for the new
compiler are in
[`future-work/compiler/goals.md`](../../future-work/compiler/goals.md).
