# F-611: Replay code identity changes on whitespace or comment edits inside a function
Severity: note
Area: runtime
Evidence: audit/evidence/06-compiler/replay-identity.md
  (`hd record` then `hd replay` on formatting-only variants)
Effect: Adding a comment line, or changing `value + 2` to `value  +  2`, makes
  replay fail with `replay event 1 does not match function code identity`.
  The failure is an uncaught JavaScript stack trace, not a diagnostic. A
  comment added above the function keeps replay working. A formatter run
  invalidates every recording.
  codeId is sha256 of the raw source slice of the function span
  (src/compiler.ts:215-226). spec/07-functions.md:258 defers code identity.
Recommendation: OPEN_ISSUES question. Should code identity be derived from
  normalized HIR or tokens rather than raw text?
