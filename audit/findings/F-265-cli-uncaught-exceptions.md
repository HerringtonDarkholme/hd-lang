# F-265: Replay rejection and several CLI errors exit through uncaught JavaScript exceptions
Severity: minor
Area: architecture
Duplicates: F-162 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-claims/repros.log (F-265, F-262); `hd run --entry s FILE` for a string-returning entry prints `Error: s has no runnable export` with a stack trace
Effect: Replay mismatches, missing replay sidecars (ENOENT), code-generation failures, and
unsupported entry types all print a Node stack trace and no located stable code. A
portable runner cannot tell a rejection from a compiler crash.
Recommendation: implementation change: catch these in src/cli.ts and print a stable
code (for example `replay-mismatch`) with exit 1.
