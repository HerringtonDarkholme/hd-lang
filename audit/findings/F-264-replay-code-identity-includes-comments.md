# F-264: A comment inside an executed function invalidates a recorded replay
Severity: minor
Area: runtime
Duplicates: F-611 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-coverage/s5-replay.tsv row comment-inside-executed (command: `node --experimental-strip-types audit/scripts/coverage/s5-replay.ts audit/evidence/02-coverage/s5-replay.tsv`)
Effect: Adding `# unrelated comment` inside `add_two` makes `hd replay` fail with
`replay event 1 does not match function code identity`. The identity is a hash of the raw
source slice (src/compiler.ts line 219, UNVERIFIED as the cause), so formatting and comment
edits count as code changes. Unrelated declaration insertion does survive, as claimed.
Recommendation: OPEN_ISSUES question: should replay code identity hash a normalized
form (tokens or typed IR) so that comment and whitespace edits keep recordings valid?
