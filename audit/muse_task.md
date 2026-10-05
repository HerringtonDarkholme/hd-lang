# Muse Task Queue

This file is Muse's work queue. The orchestrating session adds jobs here.
Muse does them top to bottom, one commit per job, and deletes a job's
section in the same commit that finishes it. Git history keeps the record.
When the queue is empty, report that and wait.

## How To Work

- Work in your own git worktree, never in the shared folder
  `/Users/hd/code/test/hd-lang`.
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

## Don't Touch

- `spec/` (including fixture file names), unless a job says so.
- Conformance harness, CLI and package mode (other agents are changing
  them): `src/cli-args.ts`, `src/commands/`, `src/package.ts` and any
  manifest or package-discovery code, `test/hd-adapter*.ts`,
  `test/hd-in-process.ts`, `test/run-portable.ts`, `spec/tools/`.

## Jobs

### AJ. Trial Rollback For `readLocals`

Review of AG (3f647f8a): the comment on `readLocals` (`src/checker/context.ts`)
says speculative trials roll it back, but `checker-trial-state.ts` never
snapshots it. A local read only inside a rejected candidate then counts as
read, and its unused-local warning is lost. Add `readLocals` to the trial
snapshot and restore. Add a test: a local used only in a rejected
overload candidate still warns.

### AK. Perf F5 And F6

From `audit/compiler/perf-audit.md`:

- **F5:** `withStandardSource` rebuilds every std object per compile.
- **F6:** the implementation clash scan is quadratic in the number of
  impls, per compile.

Fix both without a checked-std cache (the owner deferred that). Report
the tiny-program compile time (median of 5) and `pnpm run perf:check`
before and after, and keep the WAT byte-identical. Delete F5 and F6 from
`perf-audit.md` when fixed.

### AL. Review Finding O-03: Bounds Of Written Types

From `audit/compiler/opus.md` (O-03, with its repro): a written type
application such as `Box[T]` skips its declaration's generic bounds,
except a top-level `Map` key. Check every written type application's
arguments against the declaration's bounds, at the place it's written
(`trait.bound.no-implied`, `types.generic.*`). Remove the Map-key
special case once the general check covers it. Report the
KNOWN_FAILURES rows this moves. Fixtures likely exist; add a TS test
only where none does. Delete O-03 from `opus.md` when done.

### AM. Test Migration Batch 4

The remaining 52 home-A rows of the TS-test triage (call-speculation 12,
compiler-types 8, compiler 8, suspension 8, types 7, compiler-suspension
6, captured-cells 2, cli 1). Batches 1 to 3 (d082975f, 1d74c88c,
494f29aa) show the conventions; `test/MIGRATED.md` is the ledger.

- For this job you may add fixtures under `spec/conformance` and rows
  to its `cases.tsv`, but don't edit spec rule text or existing fixtures.
- A TS test whose behavior no spec rule states isn't migrated: write
  "not migrated: <reason>" in `MIGRATED.md`.
- Delete each TS test once its fixture passes in
  `test/portable/cases.tsv`.

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
