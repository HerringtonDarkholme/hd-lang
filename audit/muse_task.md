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
- Literal typing: `src/checker/expression-literals.ts`,
  `literal-arguments.ts`, `literal-join.ts`, `literal-retry.ts`,
  `expression-operators.ts`, `calls.ts`, `statements.ts`.
- Conformance harness, CLI and package mode (other agents are changing
  them): `src/cli-args.ts`, `src/commands/`, `src/package.ts` and any
  manifest or package-discovery code, `test/hd-adapter*.ts`,
  `test/hd-in-process.ts`, `test/run-portable.ts`, `spec/tools/`.

## Jobs

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

### AE. `hd doc`

The spec is done: the "Documentation" section of
`spec/cli/command-line.md` (`cli.doc.*`), module docs in
`spec/lang/01-lexical-structure.md` (`lex.doc.module*`), and the doc
pages rules from commits 4856cb9e, aaa26b22 and 35465bdc. The CLI cases
are in `spec/conformance/cli/doc-*` (known failures, tag CLI-DOC), and
`parse/valid/module-documentation.hd` (tag MODULE-DOC).

- Put the generator in its own module (for example `src/doc/`): build the
  documentation from the checked package, render Markdown and HTML pages,
  `llms.txt` and `llms-full.txt`, check `` [`Name`] `` links (warning
  `broken-doc-link`), and print one item for `hd doc NAME`, including
  `std.` names.
- `hd doc --format json` reports diagnostics like `hd check` does
  (`cli.json.commands`, added in 5788a62b). The CLI cases `doc-outside-package`
  and `doc-check-error` check it.
- Module docs: a file's first `##` block followed by a blank line
  documents the module (lexer and parser change, `lex.doc.module`).
- Wire the `hd doc` command last. `src/cli-args.ts` is in the don't-touch
  list until the harness work lands. If it's still listed when you get
  here, do everything else and leave the wiring as the last step.
- Split this into several pushed commits: module docs, Markdown, HTML,
  links, `hd doc NAME`, then the command. Each one must leave
  `pnpm run check` green.
- Move each CLI-DOC and MODULE-DOC row to `test/portable/cases.tsv` once
  it passes.

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
