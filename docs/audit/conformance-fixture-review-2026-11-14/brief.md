# Conformance fixture review brief (v2)

You review hd-lang conformance fixtures. Repo root: /Users/hd/code/test/hd-lang.
Fixtures live in spec/conformance/ (paths in your batch file are relative to it).
You are READ-ONLY on the repo: never edit, create, or delete repo files, never run git or cargo.
Never read or print ~/.npmrc, .env files, ssh keys, or git credentials.

## Context to read first (skim, don't read whole files)
- spec/conformance/README.md: sections "Layout", "Comment Directives", "Package Trees".
  A reject fixture fails with the code in its one `# diagnostic: CODE` marker, on that line.
  A `# fixture-package-tree: TREE/PATH` fixture also sees files under spec/conformance/trees/TREE/.
  A panic fixture's `# panic:` marker documents the line that raises the panic.
- .agents/skills/write-hd/SKILL.md: hd syntax, so you can read the code.
- The spec rule each fixture cites: the `specification` column is `FILE#ANCHOR`, FILE relative to spec/
  (e.g. `lang/09-traits.md`, `std/collections.md`). If ANCHOR starts with `r-`, grep spec/FILE for `r[REST]`
  (e.g. `#r-annot.cfg.pair` -> `r[annot.cfg.pair]`). Otherwise it is a heading slug; find that heading.
  Read only that rule or section, plus rules it links to if needed.
- Std signatures: grep lib/std/*.hd (e.g. `grep -n "trait Debug" lib/std/*.hd`).

## Tool you may run
`HD_CACHE=/private/tmp/claude-501/-Users-hd-code-test-hd-lang/a58638f2-f726-4f8b-844f-fbc7ea208858/scratchpad/hdcache/BATCH /Users/hd/code/test/hd-lang/target/release/hd check --tests spec/conformance/PATH` (the runner always passes --tests, so `tests:` blocks are checked too) (read-only type check). Replace BATCH with your batch name (e.g. b031). ALWAYS set this HD_CACHE: the shared default cache makes each check take 5+ seconds; with your own cache it takes under 0.1 s.
Use it to test a claim that a line is invalid or that a file type-checks. Note: hd stops at the first
error, so a clean or single-error result does not prove there is no second problem.
Do not run anything else that writes files. Never kill or signal processes (no kill/pkill): other reviewers run hd at the same time. Always run hd check in the foreground (never run_in_background, never Monitor), wrapped as `timeout 60 .../hd check FILE`, one file per call; if it times out, judge from source and move on. Leave nothing running when you reply.
A fixture with `# fixture-package-tree:` cannot be checked with plain `hd check`; judge it from source.

A `parse`-phase case is judged on parse errors only: ignore name, type and other check-only errors in it (B2 and B6 do not apply to them).

## Work efficiently (few tool calls)
- Read all fixtures of the batch in ONE or two Bash calls, e.g.
  `cd /Users/hd/code/test/hd-lang/spec/conformance && for f in $(tail -n +2 BATCH | cut -f1); do echo "=== $f"; cat -n "$f"; done`
- Grep the spec for all cited rule IDs in one call, not one call per fixture.
- Run hd check only to confirm a problem you suspect, and batch those files in one loop call:
  `for f in A B C; do echo "== $f"; timeout 60 .../hd check $f 2>&1 | grep -E '^error|^warning|result'; done`
- Aim for under 15 tool calls in total.

## For every fixture in your batch, check:
- B1 Tests its claim: the code exercises the cited rule and the `# test:` title (or file name). Fail if
  the title promises something the code never does, or says "errors" on an accept case, etc.
- B7 Right citation (every cited anchor exists; a script checked that, so never flag a missing heading): the `specification` column names the rule the fixture actually tests. Fail if it
  cites an error-only rule for an accept case, or a rule whose Error code differs from the expectation.
- B2 Reject for the right reason (reject/warn/panic fixtures): the marked line is the only thing wrong.
  Fail if some OTHER line is also invalid (e.g. a method that does not match the real std trait signature),
  or the marked line also breaks a second, unrelated rule. Common hidden one: `println`/`print` need `$ Console`, so a `fn` without a `$` row that prints has a second error (unless the missing row is the point of the case). For panic fixtures: is the marker on the line
  the rule says raises the panic?
- B3 Meaningful checks (runtime accept): each assert or expected output depends on the behavior under
  test. Fail on assert(true), assert_equal(x, x), or output that would be the same if the feature were broken.
  A runtime accept with no assert and no expect-stdout is fine only if "runs without panicking" is the point.
- B4 Reached: the code under test runs (not behind a never-taken branch or an uncalled function).
- B6 Broken elsewhere (accept cases): a line unrelated to the rule looks invalid. Check with `hd check`
  before flagging. Remember a `T` is accepted where `T?` is expected.
- B8 Portable: an expected value or output must not depend on map or set iteration order (types.map.order.unspecified: only determinism is promised), on the target's usize width, or on exact float results the spec allows to vary (e.g. within 1 ULP). Fail if it does.
- B5 Minimal (low priority): declarations that play no part in the case.
## Across the batch:
- C1 Redundant sibling: two fixtures citing the same rule, same expectation, where the differing lines
  change nothing the rule talks about (only names, literal values, or order). A difference in a boundary,
  type, position, syntactic form, or std item is NOT redundant. Name both paths.

Already covered by scripts, so do NOT report: the title of a `*-legal.hd` fixture copied from its reject twin; an accept case under `typing/warnings/`; a missing citation anchor.

Most fixtures are fine. Flag only a real, specific problem you can state. Do not flag style.
If unsure, use verdict UNSURE rather than guessing.

## Output
Write a TSV file (path given in your task) with NO header, one row per problem:
path<TAB>verdict<TAB>rubric-id<TAB>reason
verdict is FIX, MERGE, DELETE, or UNSURE (fixtures with no problem get no row). reason <= 25 words,
concrete, naming the line or the sibling path. Write the file even if it has no rows (empty file).
Then reply ONLY with one line: `reviewed N, rows M`, plus at most one sentence if the brief was unclear.
