# F-154: Runtime scenario cases never run their test blocks
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-harness/plant-run.log (plant C10)
Effect: A runtime case with `# fixture-runtime-scenario:` passes even when one of its `test` blocks panics. With `--scenario`, `hd test` runs only the scenario and returns before selecting test blocks. The harness reports the case as passed, yet the portable README says `test` "runs `main` and every named hd-lang test block". The CLI also reports a failed scenario as an uncaught `Error` stack trace, not as a structured failure (gap probe G03b).
Recommendation: implementation change: after a scenario, run the file's test blocks too, or reject scenario files that contain test blocks. Report scenario failures as a structured line, not a stack trace.
