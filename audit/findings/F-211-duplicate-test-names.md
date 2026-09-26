# F-211: 118 fixtures share `# test:` names, so portable failures cannot be traced to a file
Severity: note
Area: test-integrity
Evidence: audit/evidence/01-test-quality/oracle.tsv (e_scope_flags `shared-test-name`); audit/evidence/01-test-quality/stub-runner.log (36 names cover 98 failure reports; "checker rejects name, mutability, and type errors" appears 7 times)
Effect: when a fixture fails, the runner prints the shared name rather than the path, and the reader must guess which of up to seven files failed. The names also describe the group, not the single rule each file checks (for example compiler/58-...-diagnostic-4 checks `unused-local-binding` under the name "optional and Result context errors have stable diagnostics").
Recommendation: implementation change in the runner (report the path), and fixture change (one name per rule).
