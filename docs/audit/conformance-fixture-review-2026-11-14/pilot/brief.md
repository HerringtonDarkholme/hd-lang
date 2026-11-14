# Conformance fixture review brief

You review hd-lang conformance fixtures. Repo root: /Users/hd/code/test/hd-lang.
Fixtures live in spec/conformance/ (paths in your batch file are relative to it).
You are READ-ONLY on the repo: never edit, create, or delete repo files, never run git.
Never read or print ~/.npmrc, .env files, ssh keys, or git credentials.
Do not run `hd` or cargo. Judge from source text only.

## Context to read first (skim, don't read whole files)
- spec/conformance/README.md: sections "Layout", "Comment Directives", "Package Trees".
  A reject fixture fails with the code in its one `# diagnostic: CODE` marker, on that line.
  A `# fixture-package-tree: TREE/PATH` fixture also sees files under spec/conformance/trees/TREE/.
- .agents/skills/write-hd/SKILL.md: hd syntax, so you can read the code.
- The spec rule each fixture cites: the `specification` column is `lang/14-annotations.md#ANCHOR`.
  If ANCHOR starts with `r-`, grep spec/lang/14-annotations.md for `r[REST]` (e.g. `#r-annot.cfg.pair` -> `r[annot.cfg.pair]`).
  Otherwise it is a heading slug: grep `^#.* ` headings. Read only that rule or section.

## For every fixture in your batch, check:
- B1 Tests its claim: the code exercises the cited rule and the `# test:` title (or file name). Fail if title says X but the code tests Y.
- B2 Reject for the right reason (reject/warn fixtures only): the marked line is the only thing wrong. Fail if some OTHER line is also invalid, or the marked line would fail for a reason unrelated to the cited rule.
- B3 Meaningful assertions (runtime/accept with asserts): each assert depends on the behavior under test. Fail on assert(true), assert_equal(x, x), or an expected value that holds even if the feature were broken.
- B4 Reached: the code under test runs (not behind a never-taken branch or an uncalled function).
- B6 Broken elsewhere (accept/runtime only): a line unrelated to the cited rule looks invalid (signature mismatch, wrong type). Remember a `T` is accepted where `T?` is expected.
- B5 Minimal (low priority): declarations that play no part in the case.
## Across the batch:
- C1 Redundant sibling: two fixtures citing the same rule that cover the same path with no new boundary, type, position, or form. Also check each pair in your pairs file (near-identical code, same expectation).
- C2 Missing contrast: a rule with only reject fixtures and no accept one, or the reverse. Report only.
  Skip a rule whose text is only an error case (it names an `Error:` and states no allowed form); its accept side belongs to another rule.
- Redundant means: same rule, same expectation, and the differing lines change nothing the rule talks about (only names, literal values, or order). A difference in a boundary, type, position, or syntactic form is NOT redundant.

Most fixtures are fine. Only flag a real, specific problem you can state. Do not flag style.
If unsure, use verdict UNSURE rather than guessing.

## Output
Write a TSV file (path given in your task) with NO header, one row per problem:
path<TAB>verdict<TAB>rubric-id<TAB>reason
verdict is one of FIX, MERGE, DELETE, UNSURE (KEEP fixtures get no row). reason <= 25 words, concrete,
naming the line or the sibling path. For C2 use path = the anchor, verdict = REPORT.
Then reply with: number of fixtures reviewed, number of rows written, and any problem with the brief itself
(something confusing or missing). Keep the reply under 100 words.
