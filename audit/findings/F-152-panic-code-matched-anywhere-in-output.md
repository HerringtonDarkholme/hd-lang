# F-152: Panic expectations match the code anywhere in stdout or stderr
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-harness/plant-run.log (plants C06, F11)
Effect: A panic case passes when the program prints `CODE:` and then fails in some other way. The plant prints `index-out-of-bounds: decoy` and then raises `explicit-panic`, and it passes as an `index-out-of-bounds` case. `containsCode` checks for the substring `CODE:` in the combined stdout and stderr output.
Recommendation: implementation change (harness): match the panic report only on stderr, anchored, for example `^CODE: runtime panic$` per line, or through a machine-readable report line.
