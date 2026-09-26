# F-306: `hd run` and `hd test` crash with a stack trace when there is no entry point
Severity: minor
Area: correctness
Related: F-162 and F-265 (other uncaught CLI exceptions); the no-entry-point case is not covered there
Evidence: audit/evidence/03-fuzz/replay.txt (F-306 block); round 1 contract signatures 04/05 (810 run and 672 test cases of 5000)
Effect: a library file, or an empty file, makes `run` exit 1 with an uncaught `Error: program has no exported main function` and a Node stack trace. `test` does the same with "no main function or test blocks". Nothing names a code or a location, so a harness can't tell a user error from a compiler crash.
Recommendation: implementation change: report a located diagnostic. OPEN_ISSUES question: which stable code reports "no entry point"? Is a library with no tests a `test` success or a failure?
