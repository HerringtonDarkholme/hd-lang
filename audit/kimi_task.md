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

## Don't Touch

- `src/`, `lib/`, `test/`, `spec/conformance/` (fixtures and indexes), and
  `audit/codex_task.md`: other agents work there.
- Rule IDs: never rename, add, or remove an `r[...]` ID. Rewording a rule's
  sentence keeps its ID.

## Jobs

### K6. Split Long Sentences In The Language Chapters

Same method and rules as K1 (including the qualifier rule above), now for
`spec/lang/`. The audit counts sentences over 25 words per chapter:
06-control-flow (8), 08-data-and-enums (7), 07-functions (14),
01-lexical-structure (15), 02-grammar (17), 03-names-and-scopes (18),
14-annotations (23), 09-traits (24), 11-requirements-and-suspension (24),
05-expressions (26), 04-type-system (32), 10-modules (33). Work in that
order (smallest first). One commit per chapter; after each, run `bash
spec/check.sh` and the audit; that chapter's `sent>25` must drop to 0 and
nothing else may rise. Language rules are normative: if a split could
change what a rule allows or forbids, leave the sentence and list it under
Questions.

### K7. Usability Probe 3 (Read-Only)

Act as a new hd user. Using only `README.md`, `guide/`, and `spec/` when
the guide isn't enough, write four small realistic programs in a scratch
directory outside the repo (never in it): (1) a todo CLI that stores items
in a file and supports add/list/done via arguments; (2) a package with two
modules (`src/model.hd`, `src/report.hd`) where one imports the other, plus
a unit test and an integration test; (3) a word-frequency counter reading
stdin and printing the top 10 with `dbg` while you develop; (4) a typed
expression tree with a GADT-style `Expr[T]` and an `eval`. Run them with
`node --experimental-strip-types <your worktree>/bin/hd.js`. For every
mistake and message, add a row to `audit/hd-writing-log.md` in the existing
format (agent column `kimi`). Commit only the log. Report the five most
painful problems.

## Questions

(none)
