# F-265: Replay rejection and several CLI errors exit through uncaught JavaScript exceptions
Severity: minor
Area: architecture
Duplicates: F-162, F-306 (merged)
Evidence: `hd run --entry s FILE` for a string-returning `fn s()` prints `Error: s has no runnable export` with a stack trace; `hd run audit/evidence/03-fuzz/findings/F-306-impl-run-without-main.hd` prints `Error: program has no exported main function` (src/cli.ts) with a stack trace; `hd replay FILE` without a sidecar prints an uncaught ENOENT
Effect: Replay mismatches (`throw new Error` in src/compiler.ts), missing replay
sidecars, code-generation failures, a program without a public `main`, and an
`--entry` target that has no runnable export all print a Node stack trace and no
located stable code. src/cli.ts rethrows anything that is not a `DiagnosticError`
or `RuntimePanicError`. A portable runner cannot tell a rejection from a compiler crash.
Recommendation: implementation change: catch these in src/cli.ts and print a stable
code (for example `replay-mismatch`) with exit 1.
