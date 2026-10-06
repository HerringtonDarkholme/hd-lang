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
- `usize` work (another agent is making it a distinct type): `src/types.ts`,
  `src/numeric.ts`, `src/checker/spelling.ts`, `src/checker/literal-join.ts`,
  `src/checker/numeric-family.ts`, `lib/std/num.hd`, and `usize`/`u32`
  sites in `lib/std`.

## Jobs

### BO. Speed Gate: Baselines Recorded On CI

Owner decision: record the speed-gate baselines on GitHub's runners, so a
slow runner stops failing the gate. Today `test/perf/inference/baseline.json`
holds numbers from one laptop; CI runners vary up to ~1.6x on identical
code (`occurs-check`, `retry-receivers` failed on guide-only commits).

1. Add a manual-dispatch workflow (`.github/workflows/perf-baseline.yml`)
   that runs `pnpm run perf:check --update --json FILE` several times (for
   example 5 runs) on ubuntu-latest, and keeps, per case and size, a high
   percentile of the score (say the 90th) so ordinary slow runners pass.
   It uploads the resulting `baseline.json` as an artifact (no push from
   CI). Read `test/perf/inference/gate.ts` and README for how scores and
   `--update` work; extend `gate.ts` only as needed to merge several runs.
2. Keep the gate's rules (1.5x slowdown limit, growth limits, the retry)
   unchanged; only the baseline numbers change.
3. Document in `test/perf/inference/README.md`: when and how to refresh
   the baseline (run the workflow, download the artifact, commit it).
4. Don't commit a new baseline yourself; say in the commit how to produce it.
Check: `pnpm run perf:check` still passes locally against the current
baseline; `actionlint`-style sanity by reading the YAML; lint/format.

### BP. Top-Level Effects In A Non-Entry Module Of A Script Package

`module.init.requirement-free` (spec/lang/10-modules.md): a module's
top-level statements must be requirement-free unless it is the entry. In a
package whose entry is a script (top-level statements, no `main`), the
prototype joins every module's top level into one script, so a non-entry
module's top-level `println("x")` is accepted (gap from c7d9c4e3; see the
script-entry rows in `src/KNOWN_ISSUES.md`). Track each top-level
statement's module of origin and report `module.init.requirement-free`'s
error for a requirement in a non-entry module's top level, while the
entry script keeps its row. Add fixtures (a package with `src/main.hd`
script plus `src/util.hd` printing at top level: error; the same with the
print inside a function: ok), run before and after, and update
KNOWN_ISSUES. Checker only; no rule changes.

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
