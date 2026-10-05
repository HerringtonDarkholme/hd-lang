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

### AG. Perf F7 And F10

From `audit/compiler/perf-audit.md`:

- **F7:** `collectReadLocals` walks each function body after checking.
  Collect the read locals while checking instead.
- **F10:** the WAT is parsed twice after it is generated. Parse it once.

Report the tiny-program compile time before and after (median of 5) and
`pnpm run perf:check`. Delete F7 and F10 from `perf-audit.md` when they
are fixed.

### AI. Read-Only: What The CLI Promises But Doesn't Do

The owner found `hd doc` and `hd new --pages` unusable (2026-10-05). Find
every other user-facing promise that doesn't work. Act as a user, with a
fresh package from `hd new --app` and `hd new --lib`:

- run every command and flag listed in `hd help` and `hd help COMMAND`;
- read every generated file (`hd.toml`, `src/`, `tests/`, `.gitignore`,
  the `--pages` workflow);
- follow every hint and suggestion a diagnostic prints;
- compare each with `spec/cli/command-line.md`.

Report each broken, missing or misleading item: what you ran, what
happened, what the spec says, and a suggested fix (fix it, or hide it until
implemented). Commit the report as `audit/cli-promises-2026-10-05.md`.
Don't fix anything in this job. `hd doc` itself is already being
redesigned (orchestrator task #309); list it in one line.

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
