# Compiler Audit

Open compiler findings only; fixed ones are deleted (git history keeps them).

- [opus.md](opus.md): review findings O-03 (bounds of written types, task #280) and O-06 (whole-state speculation copies).
- [perf-audit.md](perf-audit.md): open speed findings F1, F3, F5, F6, F7, F10.

Other compiler work is tracked by tag in `test/portable/KNOWN_FAILURES.tsv`
and `src/KNOWN_ISSUES.md`. The package-scope, type-representation, wake and
host-ABI gaps of the old report are there and in
[`future-work/HOST_ENTRY_DRIVER.md`](../../future-work/HOST_ENTRY_DRIVER.md).
