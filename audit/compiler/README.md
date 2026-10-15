# Compiler Audit

Reports on the Rust compiler and CLI. Fixed findings are deleted (git history
keeps them); open ones live in a GitHub issue or in one of the files below.

- [diagnostic-notes.md](diagnostic-notes.md): findings about error messages,
  codes, positions and fix-its, one table row each. The compiler does what it
  does until the owner picks rows from it.
- [coverage-2026-10-15.md](coverage-2026-10-15.md): which spec rules have no
  conformance case or driver test, and the bugs found by probing them. It goes
  when issue #169 closes.
- [later.md](later.md): compiler work that is noted but deliberately not
  scheduled.

Notes for the compiler's design are in
[`docs/compiler/goals.md`](../../docs/compiler/goals.md). The
conformance state is in [`CONFORMANCE.md`](../../CONFORMANCE.md) and
[`KNOWN_FAILURES.tsv`](../../KNOWN_FAILURES.tsv).
