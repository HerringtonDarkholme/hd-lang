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
- Literal typing: `src/checker/expression-literals.ts`,
  `literal-arguments.ts`, `literal-join.ts`, `literal-retry.ts`,
  `expression-operators.ts`, `calls.ts`, `statements.ts`.
- Conformance harness and CLI flags (another agent is changing them):
  `src/cli-args.ts`, `src/commands/`, `test/hd-adapter*.ts`,
  `test/hd-in-process.ts`, `test/run-portable.ts`, `spec/tools/`.

## Jobs

### AA. Derivation And Defaults

If this is already committed locally, rebase and push it.

- Clear the KNOWN_FAILURES rows tagged DERIVE-DEFAULT (3), DEFAULT-FIELD,
  STD-DEBUG, and METHOD-DEFAULT. Each row's reason, and its Compiler
  Handoff row in `future-work/STDLIB_PLAN.md`, says what is missing.
- You may edit `lib/std` for this job only: the `default` marker in
  `lib/std/ops.hd`, and `Debug` for `std.inspect`'s `TypeId` and
  `std.structure`'s `SelfRef`.
- Root-cause fixes in the checker's derivation code and in the parser
  (parameter defaults on methods).
- Move the passing rows to `test/portable/cases.tsv` and fix the counts in
  `src/KNOWN_ISSUES.md`.

### AB. Review Findings O-08, O-09, O-11

If this is already committed locally, rebase and push it.

From `audit/compiler/opus.md`; each finding has a repro.

- **O-08:** a spread list rejects a function element written as `if`,
  `match`, or a closure.
- **O-09:** a derived `Arbitrary` error does not name a member whose type
  comes through an alias.
- **O-11:** derive diagnostics travel through global tables keyed by span
  objects. Key them by declaration identity instead.

Add a regression test under `test/` for each one. Delete each fixed
finding from `opus.md`.

### AC. Closure Capture Index

If this is already committed locally, rebase and push it.

`audit/job1-slow-compile-profiles.md`, case `closures-shared-var`: 71% of
the time is in `visibleCaptureSources` (`src/checker/context.ts`), because
every closure re-scans for the variables it captures.

- Build an index once per function body and look closures up in it.
- Report `closures-shared-var` at 6,000, 12,000 and 24,000 closures, before
  and after. Growth should be linear.
- Report `pnpm run perf:check`.

### AD. Read-Only: Re-Sweep After The Literal Change

The literal typing model changed on 2026-10-04 (62a084a2, 288105cf): a
bare literal defaults to `usize`, and a literal joins the typed side
within one statement.

- Rerun the Job 3 error-message sweep and the Job 4 known-failure sweep
  at the current HEAD.
- Commit one report, `audit/job11-resweep-after-literal-change.md`, with:
  - leaks and over-long messages;
  - known-failure rows whose reason no longer matches;
  - new messages that are confusing, such as `u32` shown where the user
    wrote nothing.
- Delete `audit/job3-error-message-quality.md`,
  `audit/job4-known-failure-reasons.md` and
  `audit/job9-known-failure-resweep.md` in the same commit, if the new
  report replaces them.

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
