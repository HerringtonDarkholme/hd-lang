# F-605: Error recovery stops at the first parse error and the first error per function
Severity: minor
Area: architecture
Evidence: audit/evidence/06-compiler/error-recovery.md
Effect: A file with three independent parse errors reports one
  (errors-parse2.hd). If the lexer finds an unclosed delimiter, it reports only
  lexer errors and hides the parse errors (errors-parse.hd, 2 of 3). The
  checker reports 5 of 6 type errors, one per function; the second error in
  `e` is dropped. A signature error (unknown parameter type) suppresses every
  body error in the file (errors-signature.hd). Users fix errors one rebuild
  at a time.
  Mechanism: `ParseFailure`/`CheckFailure` exceptions unwind to the program or
  function root. `check()` returns early after signature diagnostics
  (src/checker/program.ts:43).
Recommendation: implementation change. Add statement-level parser
  synchronization, and check bodies whose own signatures resolved.
