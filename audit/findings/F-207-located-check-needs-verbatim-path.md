# F-207: The located-diagnostic check requires the implementation to echo the runner's absolute path verbatim
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-test-quality/stub-relative-hd.log (STUB_MODE=relative-hd delegates to bin/hd.js unchanged but prints paths relative to the working directory: 239 of 551 cases fail, all `missing CODE at line N`; baseline passes 551)
Effect: an implementation that prints a relative, normalized, or URI path fails every reject and warn case with correct codes and lines. The runner also forces cwd to the repository root, which is not documented.
Recommendation: implementation change in the runner (match `LINE:COL: CODE:` after any path that resolves to the fixture), plus spec text for the command contract.
