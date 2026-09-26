# F-200: Fixture directives and the runner command contract are defined outside spec/
Severity: major
Area: spec
Evidence: audit/evidence/01-test-quality/directives.tsv (counts and definition sites); test/run-portable.ts; src/cli.ts; audit/evidence/01-test-quality/stub-runner.log
Effect: an implementer who reads only spec/ cannot run the conformance suite the way the repository does. `# expect-result:` (169 uses), `# expect:` accept/parse/test (62), `# test:` (342), the phase-to-command mapping, the `run --entry`/`--profile`/`--scenario`/`--pending-function` options, exit statuses, and the `PATH:LINE:COL: CODE: message` format are specified only in test/README.md or only in code. `# expect: accept` and `# expect: parse` appear in no document at all. Four runtime profiles (ready-counter, ready-float, ready-gate, ready-text) exist only in src/cli.ts.
Recommendation: spec change. Apply audit/proposals/fixture-format.md to spec/conformance/README.md (owner decision), then move any test/fixtures case that needs a directive into spec/conformance/.
