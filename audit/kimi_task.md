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

### K18. Diagnostics Wishlist For The New Compiler (Read-Only Report)

`audit/hd-writing-log.md` records every mistake that agents (Haiku,
Sonnet, Kimi) made while writing hd, with the compiler's message and
whether it helped. The new compiler is judged by how few retries an agent
needs (future-work/NEW_COMPILER_ARCHITECTURE.md, "Arena", pillar 1).
Turn the log into a wishlist:

1. Group every row whose Helped? is `no` or `partly` by root cause: a
   message missing its fix, a confusing code, a missing did-you-mean, a
   doc gap, or a language rule that surprises newcomers.
2. For each group, give the row count, two or three representative rows
   quoted verbatim, the message today (run it with your worktree's hd,
   since some were improved on 2026-10-06), and the message an agent
   would need: the effect plus the fix, in one line.
3. At the top, rank the ten changes that would save the most retries.
4. Leave out rows the 2026-10-06 work already fixed: P1k hints,
   panic locations and the error-code revamp. Verify each by running
   it.

Write `audit/diagnostics-wishlist-2026-10-06.md`. Don't change any code
or messages. Commit only the report.

## Questions

(none)
