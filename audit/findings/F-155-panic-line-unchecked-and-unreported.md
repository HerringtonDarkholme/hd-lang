# F-155: Runtime panics carry no source location, so panic marker lines are never checked
Severity: minor
Area: runtime
Evidence: audit/evidence/01-harness/plant-run.log (plants C07, F16)
Effect: A panic marker on the wrong line still passes. The CLI prints only `CODE: runtime panic`, with no location. The spec says the runtime "must report at least a stable failure category and source location when one is available" (06-control-flow.md#runtime-panics). The conformance README also ties each panic to "the triggering source line". Neither the runtime nor the harness checks the line.
Recommendation: implementation change: report `path:line:col` for panics raised at a known site, and have the harness compare it with the marker line. OPEN_ISSUES question: is a panic location "available" for traps inside runtime helpers?
