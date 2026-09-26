# F-612: explain-requirements prints every call path, so output grows exponentially
Severity: note
Area: architecture
Evidence: audit/evidence/06-compiler/explain-requirements.md (last section)
Effect: If each function in a chain calls the next twice, the output is 521
  lines at depth 8, 8,205 at depth 12, and 131,089 at depth 16. A real program
  with shared helpers can produce output nobody can read.
Recommendation: implementation change. Print one shortest witness path per
  requirement key, with an option to list all paths.
