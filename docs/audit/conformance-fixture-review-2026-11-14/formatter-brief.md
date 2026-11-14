# Formatter-damage review brief

Commit `edea876ae` ("Apply the canonical hd format") reformatted hd-lang's
conformance fixtures. For layout- and token-sensitive fixtures this can erase
the exact feature a fixture tests: a line break inside brackets joined, a
same-line suite split, a leading `.` line merged, a trailing comma removed,
redundant parentheses removed, a missing final newline added, a BOM stripped.

Input: `fmt-diffs.txt` (same directory). Each block starts with
`===== PATH | EXPECTATION SPEC-ANCHOR` followed by the unified diff of that
fixture in the format commit (`-` = before, `+` = after). The `# test:` title
line usually appears in the diff context or you can read the current file at
`/Users/hd/code/test/hd-lang/spec/conformance/PATH`.

You are READ-ONLY: never edit repo files, never run git, cargo, or kill
processes. You may read the spec (`/Users/hd/code/test/hd-lang/spec/lang/01-lexical-structure.md`,
`02-grammar.md`) to see what a cited rule is about.

For each fixture decide one verdict:
- ERASED: the formatter removed or changed the very form the title or cited
  rule is about (the fixture no longer tests it). Restoring the `-` version
  is the fix.
- WEAKENED: some of the forms the title lists were erased, others remain.
- HARMLESS: the change does not touch what the fixture tests (e.g. it only
  re-indented, or reformatted lines unrelated to the rule).

Output: write `fmt-out.tsv` (same directory), no header, one row per fixture:
`PATH<TAB>VERDICT<TAB>what was erased, <= 20 words (empty for HARMLESS)`.
Include every fixture, HARMLESS too. Read the diff file in a few large reads.
Reply with one line: `reviewed N: erased E, weakened W, harmless H`.
