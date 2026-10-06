# Kimi Task Queue

This file is Kimi's work queue. The orchestrating session adds jobs here.
Kimi does them top to bottom, one commit per job (or per chapter where a
job says so), and deletes a job's section in the same commit that
finishes it. Git history keeps the record. When the queue is empty,
report that and wait.

These jobs are prose, docs, and probes. **Never change behavior:** no
edits under `src/`, no fixture expectation changes, and no change to what
a spec rule means. If a rewrite would change meaning, leave that sentence
and list it under Questions.

## How To Work

- Work in **one** long-lived git worktree of your own, for example
  `git worktree add /private/tmp/kimi-work -b kimi/work origin/main`
  the first time, and reuse it for every job. Keep it; don't delete it.
- **Never write in the shared main checkout**
  `/Users/hd/code/test/hd-lang`: no edits, commits, or checkouts there.
  Your commits reach `main` only by `git push origin HEAD:main` from your
  worktree.
- Start each job from current main: `git fetch origin && git reset --hard
  origin/main` in your worktree, but only when it holds no unpushed work
  (check `git status` and `git log origin/main..HEAD` first).
- Install dependencies in your worktree with `pnpm install
  --frozen-lockfile`; never symlink or modify the shared `node_modules`.
- To finish each job:

  ```sh
  git fetch origin && git rebase origin/main
  pnpm run check
  git push origin HEAD:main
  ```

  If the push is rejected because main moved, fetch, rebase, rerun
  `pnpm run check`, and push again. Never force-push.
- A job is done only when its commit is on `origin/main`. A local commit
  is not done: push it.
- While working, run only scoped checks: `node --experimental-strip-types
  test/run-portable.ts --changed` (or `--phase parse|type|runtime`) and
  `node --test --experimental-strip-types <the test files you touch>`. Run
  the full `pnpm run check` and `pnpm run test:ui` once, right before the
  push.
- Don't start a full `pnpm run check` or a full conformance run while
  another agent's full run is going (check the load average with
  `uptime`; above ~30, wait for it to drop). Two full runs at once push
  the load past 300 and make both time out.
- Never wait with an `until` or `while … sleep` loop: run a check in the
  foreground with a timeout. Use `gh run watch` if you ever need to wait for
  CI.
- Use pnpm only. Add files by name, never `git add -A`.
- For any change under `src/` or `lib/`, put the size-guard numbers in each commit message: `compileToWat` of the
  tiny program and the one-test program (today 16,350 B / 23 functions and
  3,143 B / 6 functions). Explain any growth.
- Print the CI run links once after a push and move on. Don't wait for CI.
- Never edit a fixture to make it pass. If the spec isn't clear-cut, skip
  the item, write the question under "Questions" at the end of this file,
  and go on.
- hd code style: an `i32` literal is `+N` (`total := +0`), not
  `let total: i32 = 0`, except in a group of annotated declarations,
  where the `i32` stays annotated so the widths read side by side.

- **Spec examples.** Adding, removing or moving a ```text block in a spec
  chapter renumbers every later block, so realign that chapter's rows in
  `spec/conformance/examples.tsv`, not only the new row. An earlier job missed
  this and broke `bash spec/check.sh` on main (fixed in c8c35a7e). After
  any rebase, rerun `bash spec/check.sh` before pushing; never push with
  it red.

- **When you split a sentence,** every qualifier that covered the whole
  sentence must still cover each part: a location ("at the range's first
  character"), a condition, a build mode, an error code. Review of K1
  found one dropped location (fixed in 111a9a0b). Reread each split
  rule and ask: does every case still carry everything it had?
- **A new guide or website page must be listed** in `website/src/pages.ts`.
  Then run `node --test --experimental-strip-types website/test/site.test.ts`
  before pushing. K16 added `guide/WORKING_WITH_DATA.md` without listing
  it, which turned CI red (fixed in 4acf0ee5).
- **Keep a condition a condition.** Don't turn "If an X does Y, it is
  rejected" into "An X may (or can) do Y. Such an X is rejected". The
  second form reads as rejecting every X. Write "Some X do Y. Such an X is
  rejected", or keep the `If`. Review of K6 found this twice
  (`annot.fact.no-block-on.unprovable` and `req.drive.block-on.unprovable`).

## Don't Touch

- `src/`, `lib/`, `test/`, `spec/conformance/` (fixtures and indexes), and
  `audit/codex_task.md`: other agents work there. The one exception is
  realigning `spec/conformance/examples.tsv` rows after you add, remove or
  move a ```text or ```hd block in a spec chapter.
- Rule IDs: never rename, add, or remove an `r[...]` ID. Rewording a rule's
  sentence keeps its ID.

## Jobs

### K20. Doc Summaries For lib/std (Comment Lines Only)

The owner decided (future-work/HD_DOC.md) that `lib/std` gets `##` doc
comments. Today it has none; every comment is `#`. For every `pub`
declaration in `lib/std/*.hd`, add a `##` summary above it:

- One or two sentences on what it does, for a user, in the guide's plain
  style.
- End with the spec rule it implements, as a Markdown link, e.g.
  `## See [std-json.decode](../../spec/std/json.md#r-std-json.decode).`
  Find the rule with `grep -rn 'r\[std-' spec/std/`.
- Keep any existing `#` comment that explains the implementation, below
  the `##` summary.

**This job is the one exception to "Don't Touch lib/":** you may add `##`
lines and nothing else. Before each commit, run `git diff -U0 lib/std`.
Every added line must start with `##` (after indentation), and no other
line may change. Then run `node --experimental-strip-types
test/run-portable.ts --tier std` (all must pass) and `pnpm run
format:check`.

Commit one std module per commit, largest public surface first:
collections, text, json, fs, time, testing, then the rest.

### K21. Usability Probe 7: The New Guide Pages (Read-Only)

Your K16/K17 pages (`guide/WORKING_WITH_DATA.md`) and K19
(`guide/COMMANDS.md`) are new. Test whether they teach. Using **only
those two pages** plus `guide/LEARN_IN_10_MINUTES.md`, write four
programs in a scratch directory outside the repo:

1. A config-driven report.
2. A log scanner with regex and time.
3. A package with a doc test, unit tests and an integration test, run
   with `hd test`.
4. A concurrent fetcher with a timeout.

Log every mistake in `audit/hd-writing-log.md` (task `probe 7: …`,
model `kimi`). For each place the guide page was missing something or
led you wrong, add a row of kind `api` that quotes the page section.
Commit only the log, and list the five most painful problems in the
commit message. Don't fix the guide pages in this job; that is K22.

### K22. Fix The Guide Gaps Probe 7 Found

For each guide-related row K21 logged, fix the guide page where the
spec agrees: add the missing step, correct the misleading text, add the
example. Every changed code block must run with hd. One commit per page.
Run `pnpm run website:build` and
`node --test --experimental-strip-types website/test/site.test.ts`
before pushing.

## Questions

(none)
