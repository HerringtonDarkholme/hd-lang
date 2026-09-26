# F-151: A `# diagnostic:` marker is satisfied by a warning with the same code
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-harness/plant-run.log (plants C15, F12b)
Effect: A reject case passes when its marked code is emitted only as a warning, provided any other error makes the exit status nonzero. The line regex in `containsLocatedCode` is `path:line:col: (?:warning: )?CODE:`, which accepts both severities for both marker kinds. A regression that downgrades an error to a warning goes unnoticed if the fixture has any other error.
Recommendation: implementation change (harness): use `warning: ` only for `# warning:` markers, and forbid it for `# diagnostic:` markers.
