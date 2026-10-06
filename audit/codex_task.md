# Codex Task Queue

This file is Codex's work queue. The orchestrating session adds jobs here.
Codex does them top to bottom, one commit per job, and deletes a job's
section in the same commit that finishes it. Git history keeps the record.
When the queue is empty, report that and wait.

## How To Work

- Work in **one** long-lived git worktree of your own, for example
  `git worktree add /private/tmp/codex-work -b codex/work origin/main`
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
- Put the size-guard numbers in each commit message: `compileToWat` of the
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

## Don't Touch

- `spec/` (including fixture file names), unless a job says so.

## Jobs

### BL. A Nested Variant Pattern Before A Positional Binding Loses The Binding

Found by task #278; fails on main too. In a match arm, a nested variant
pattern in an earlier position makes a later positional binding unknown:

```
enum Plain:
    Lit(i64)
    Add(Plain, Plain)

fn simplify(e: Plain) -> Plain:
    match e:
        Plain.Add(Plain.Lit(0), right) => right    # unknown-name: 'right'
        _ => e
```

Find the root cause (likely the nested pattern's bindings or test path
replaces the outer arm's binding list instead of extending it), fix it,
and add a runtime fixture with nested patterns in first, middle and last
positions, including two nested patterns in one arm. Run the parent
commit and yours, and check `--phase type` and `--phase runtime`.

### J. Ongoing: Review New `src/` Commits

For each new commit on `origin/main` that touches `src/`, review the diff
against the spec rule IDs or known-failure tags its message cites. Add the
findings to `audit/job6-src-commit-review.md`, in this order: verdict
(Blocker / Non-blocking / Nit), effect, fix, file:line.

Large recent ones to review:

- the literal join model, 62a084a2 and 288105cf, against
  `types.literal.local.*` in `spec/lang/04-type-system.md`;
- overflow build modes, 65a84de1 and c05682dc, against `types.arith.*`.

## Questions

(none)
