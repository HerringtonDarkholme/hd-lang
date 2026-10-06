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
- Module visibility (another agent is changing it): `src/checker/package-ownership.ts`,
  `src/checker/member-lookup.ts` visibility paths, `src/checker/module-paths.ts`,
  `src/package.ts` module scopes.

## Jobs

### BS. A Shift Count May Be Any Unsigned Type (Do First, Small)

Owner decision, 2026-10-05: `x << i` and `x >> i` accept any unsigned
integer type as the count (u8, u16, u32, u64, usize), like Rust's `<<`.
Today `expr.shift.count-u32` requires `u32`, so a `usize` loop index needs
`u32(i)`. Change the rule (retire the ID, add the new one), keep the
existing behavior for a count at or above the bit width, keep
`rotate_left`/`rotate_right` and checked/wrapping shift methods on `u32`
(as Rust), and keep a signed count an error. Checker and emitter (a
narrower or wider count converts to the operation's width the same way
`u32` does today). Fixtures: `x << i` with a `usize` index, a `u8` count,
a `u64` count, and a signed count rejected. Run them before and after.


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

### BQ. Intern Types (Compile Speed)

The checker represents types as strings and re-parses them on every
inspection (audit/compiler/perf-audit.md, finding F3: `functionParts`,
`nominalGenericParts`, `containsGenericParameter`, the `Name=type` regex;
about 8.6% of a profile, and the speed gate's `list-nest` case grows
11x per 3.33x because nested types re-scan inner text at every level).
Also fix the nested-tuple crash the same finding area mentions if it
still reproduces (try a deeply nested tuple type, e.g. 40 levels).

Do it in two steps, one commit each:
1. Memoize the pure text parsers in a Map keyed by the type string (the
   audit's low-risk fix). Measure.
2. Intern parsed types: one canonical parsed object per distinct type
   string, so the parsers and predicates read structure instead of
   scanning text. Keep the string API at module edges if a full
   migration is too big; say what you did and what's left.
Measure `pnpm run perf:check` (all cases; compare against origin/main
in a detached worktree on the same load, `uptime` below ~20) before and
after each step, and report the table. No behavior change: the type and
runtime phases must stay identical. Delete F3 from perf-audit.md when it
is fixed.

### BR. Microbenchmarks And Wasm Size

Measure, don't optimize. Add `test/perf/micro/` with a few small
programs written the same way in hd, Python and Node (for example: sum of
1..10M, string building of 100k parts, a map with 100k inserts and
lookups, recursive fib(30), sorting 100k items), and a script
(`node --experimental-strip-types test/perf/micro/run.ts`) that builds
the hd ones with `hd build --release`, runs each three times, and prints
a table of median times and the hd Wasm size of each program. Python
and Node are optional on the machine: skip a column if the tool is
missing. Then write the findings into `audit/compiler/perf-audit.md` as
a short new section: where hd stands (rough multiples), the biggest Wasm
size contributors (the std splice: which std modules a tiny program
pulls in and their size), and the two or three most promising fixes.
No compiler changes in this job.

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
