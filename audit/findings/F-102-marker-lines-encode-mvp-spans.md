# F-102: Marker lines are normative, but the spec defines no location rule and every checked line equals the MVP's span
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-fixtures/markers.tsv, audit/evidence/01-fixtures/impl-lines.tsv
Effect: Commit 78af8aa replaced file-level `# expect-error:` headers with inline markers. The conformance README now says "Marker codes and their source lines are stable conformance expectations." The numbered chapters never say which line a diagnostic belongs to. For 109 of the 109 selected diagnostic and warning fixtures, the marker line equals the line the MVP reports. In `typing/invalid/pack-length-mismatch.hd`, the call spans lines 8-12 and the marker sits on the extra argument at line 11. A second implementation that reports at the call (line 8) fails the case with the correct code. Other examples: `missing-return-value.hd` (the `if` header) and `requirement-subtract-absent.hd` (the call site, not the subtracting declaration).
Recommendation: OPEN_ISSUES question: should the spec define a location rule for each diagnostic, or should the portable runner accept the code anywhere inside the offending construct's span?
