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

### K10. Add Missing Examples To Spec Sections

`pnpm run spec audit` counts sections without an example (`no-ex`, 225
today). Work smallest chapter first: 13-gadts (6), 07-functions (7),
14-annotations (7), 02-grammar (10), 08-data-and-enums (13). For each
flagged section, add one short example that shows the section's rule at
work, following spec/STYLE.md.

- An example that should compile goes in an ```hd block, and you must run
  it with hd before committing. A rejected example uses the existing
  ```text convention with its error code.
- Realign `spec/conformance/examples.tsv` for every chapter where you add
  a block (see "Spec examples" above), and keep `bash spec/check.sh` green.
- If the compiler disagrees with the rule, don't change the example to
  match the compiler: list it under Questions.

One commit per chapter; that chapter's `no-ex` must drop and nothing else
may rise.

### K11. Add Missing Examples To The Remaining Language Chapters

Continue K10's method for the remaining chapters, smallest `no-ex` count
first: 01-lexical-structure, 06-control-flow, 03-names-and-scopes,
05-expressions, 11-requirements-and-suspension, 09-traits,
04-type-system, 10-modules. One commit per chapter. Every ```hd example
must run with hd before committing. Realign `examples.tsv` for each
chapter, keep `bash spec/check.sh` green, and make sure that chapter's
`no-ex` drops while nothing else rises. If the compiler disagrees with a
rule, list it under Questions rather than bending the example.

### K12. Usability Probe 5 (Read-Only)

Same method as K9, with new areas. Write four programs, using only
`README.md`, `guide/`, and `spec/`:

1. A concurrent fetcher that runs three suspending lookups with `all!`
   and a timeout with `race!`, tested with fake providers.
2. A small inventory using `Map`, `Set` and iterator chains (`filter`,
   `map`, `fold`, `sorted`), with unit tests.
3. A two-package workspace where an app depends on a local library by
   path. Use a temporary HOME, HD_CACHE and GIT_CONFIG_GLOBAL inside your
   scratch directory, and no network.
4. A config loader whose config type has fields with default values,
   loaded from JSON where some keys are present and some missing. Check
   that a present key's value wins over the default. This area has a
   known bug, F-616; log what you see.

Log every mistake and message in `audit/hd-writing-log.md` (task
`probe 5: …`, model `kimi`). Commit only the log, and list the five most
painful problems in the commit message.

## Questions

(none)
