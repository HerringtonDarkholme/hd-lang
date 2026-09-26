# F-156: `# expect-result` compares all of stdout, not the entry's return value
Severity: note
Area: test-integrity
Evidence: audit/evidence/01-harness/plant-run.log (plant F07)
Effect: A `void` main that prints `42` satisfies `# expect-result: main = 42`. Printed output and the returned value share one stream, so the directive cannot tell them apart.
Recommendation: implementation change (harness or CLI): print the result on a marked line, for example `result: 42`, or on a separate stream.
