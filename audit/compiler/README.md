# Compiler Audit

Open compiler findings only; fixed ones are deleted (git history keeps them).

- [opus.md](opus.md): review finding O-06 (whole-state speculation copies).
- The owner's 2026-10-04/05 architecture audit of the current compiler and
  directions for the next one: [status-quo-2026-10-04.md](status-quo-2026-10-04.md),
  [findings-2026-10-04.md](findings-2026-10-04.md),
  [architecture-directions-2026-10-05.md](architecture-directions-2026-10-05.md),
  and the per-area notes beside them.
- [perf-audit.md](perf-audit.md): open speed findings F1, F5, F6, F7, F10.

Other compiler work is tracked by tag in `test/portable/KNOWN_FAILURES.tsv`
and `src/KNOWN_ISSUES.md`. The package-scope, type-representation, wake and
host-ABI gaps of the old report are there and in
[`future-work/HOST_ENTRY_DRIVER.md`](../../future-work/HOST_ENTRY_DRIVER.md).
