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
- **No more fixture jobs** (owner, 2026-10-07: "kill all codex's fixture
  jobs"). Don't write conformance fixtures unless a job explicitly asks.
- **Fixture jobs also run the Rust parse tests:** after adding or
  changing any `spec/conformance/parse/` case, run `cd compiler && cargo
  test -p hd_syntax --test corpus`. The old Rust parser is being replaced,
  so a case it gets wrong goes into `compiler/KNOWN_FAILURES.tsv` (path,
  tab, expected code or `accept`) in the same commit. Never push with
  `cargo test` red (S1, f1a266a6, turned main red this way).
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

- **`compiler/crates/`:** the orchestrator's lane. You may add
  `compiler/bench/` (Q4, P1).
- `src/`: the frozen prototype. Bug fixes only through a KNOWN_FAILURES
  row, never an edit.
- `lib/std/`, `guide/`, `website/`: read only, unless a job says so.
- `future-work/compiler/*.md`: only where a job says so.
- `spec/lang/*.md` rule text: only where a job says so. Fixtures under
  `spec/conformance/` and `test/portable/` are yours in the fixture jobs.

## Jobs

### S6. Spec: `hd test` Output Format

Check `spec/cli/command-line.md` for the test runner's report: per-case
result lines, the failure block (assertion message, source position),
panic reporting, the summary line, ordering (content order, streamed),
exit codes, `--filter`. Fill any gap with numbered rules (IDs per
`spec/STYLE.md`, an example each), consistent with
`future-work/compiler/engines-and-test-runner.md` §19.3 and §19.5 and
with what the TS prototype prints when the spec is silent. Add CLI cases
where `spec/conformance/cli-cases.tsv` has none for a rule. The
orchestrator's M4c agent is implementing `hd test` now: describe the
format, don't change compiler code. Timebox 45 minutes; push.

### D2g. Reconcile M4c (Phase 1 Complete)

M4c landed (4c4b3b92): `hd test` runs `tests:` blocks end to end; shared
enum data initializes with its module; `"$x"` of a top-level binding;
`for` over ranges; `main` returning `.Err` exits 1. Phase 1 ("make it
move") is complete. Run D2 for it, and record (citing "M4c gap n") with
the intended rule:

1. `use` lines inside a `tests:` block join the whole module's scope
   (the spec does not allow it): say how the checker scopes them.
2. A build error in one module's tests fails all of `hd test`; §19.1
   wants per-root isolation (drop only the roots that reach the error),
   which Collect and Emit do not support.
3. Test cases are synthesized items (`module.$test<i>`) stored in a
   test-role check entry, not a separate `TestOverlay` stage or entry:
   record or correct in scheduler.md and cache.md.
4. The case list passes from driver to CLI directly; §19.2 puts it in
   the `hd.runtime` section.
5. Panic categories are parsed from the stderr report, not read from
   instance globals (§15.5).
6. `std.rt` is a new compiler-supplied virtual module (like `std.core`)
   for entry reports: name it in codegen.md and std-bootstrap.md.
7. `dyn Error` does not satisfy `Display` in the solver; a `dyn Error`
   value cannot be emitted (recursive vtable type); `for` over a map
   (`MapIter`) and `Debug` for `string` (mutable captures, `StrIndex`)
   are not emitted. Add to the backlog with their phase-2 job.
8. Also mark phase 1 complete in `footprint.md` and the README's status
   line.

Docs only; timebox 45 minutes; push.

### S7. Spec: `use` Lines Inside `tests:` Blocks

Check `spec/lang/10-modules.md` (and 14-annotations if `tests:` lives
there) for how a `use` line inside a `tests:` block is scoped: visible
only to that block's cases, or to the module; shadowing against the
module's own names; whether it may name test-only dependencies. If the
spec is silent or ambiguous, write numbered rules with IDs per
`spec/STYLE.md`, an example each, consistent with the TS prototype when
it has settled behaviour (flag real conflicts in the commit message).
One type-phase fixture per new rule. Timebox 30 minutes; push.

### T4. Conformance Triage For Phase 2

From `compiler/CONFORMANCE.md` (Q18) at the current main, map every
failure bucket (first diagnostic code or `unsupported` stage) to the
`future-work/compiler/phase2-jobs.md` job that fixes it (P2-1 to
P2-10), with the number of cases each bucket holds. Write the result as
a table at the top of `phase2-jobs.md` and reorder the jobs by cases
unblocked per estimated job size, respecting dependencies. Docs only.
Timebox 30 minutes; push.

### D2. Reconcile After Each Orchestrator Milestone (Standing)

Each time a commit titled "M1:", "M2:", "M3:" or "M4:" lands on main,
rerun the reconciliation of `future-work/compiler/reconciliation.md`
against the new code: update its top-10 and table (mark fixed rows
fixed, add new gaps), apply design-doc corrections the code proves
right, and update `footprint.md` counts. Docs only. Timebox 45 minutes
per milestone; push.

### P1. Profile The New Compiler (After S4; Standing Job)

Owner, 2026-10-07: "you write the code, codex do the profiling. move
fast". The orchestrator writes `compiler/crates/*`; you measure it.

- Each time a new compiler commit lands on main (`git log -- compiler/`),
  profile it: `hd run` / `hd build` on the samples and on the generated
  bench (`cargo run --release -p hd_driver --example bench N`) at 3,000 and
  30,000 lines, cold, warm, body edit, signature edit, comment edit.
  Use `samply` or `cargo flamegraph` if installed, else `perf`-style
  timers already in the driver's counters.
- Write `audit/compiler/profile-<date>-<short hash>.md`: per-stage time,
  the top 10 hot functions with their share, allocations if measurable,
  and for each hotspot one line: **implementation slip** (name the fix)
  or **architecture issue** (name the design section). Compare with the
  previous report.
- Only flag what is atrociously bad (order-of-magnitude, superlinear,
  or a stage that dominates for no design reason). Perf is eyeballed,
  not gated, and micro-tuning is out of scope.
- You may add benchmark inputs or a harness under `compiler/bench/`
  (new directory). Never edit `compiler/crates/`; the orchestrator
  applies fixes from your report.
- Push each report within 30 minutes of starting it.

## Questions

- **Q17 nested-suite middle-column `else`.** The Rust parser reports
  `invalid-dedent` when a non-closure suite inside brackets has its `else`
  between the header and body columns, while the TypeScript prototype accepts
  it. Should `lex.nested.*` reject it as a dedent to an inactive column, or
  should nested control-flow headers permit that alignment?
- **Q17 recovery across statements.** The Rust parser reports the first error
  in each later independent statement, while the TypeScript prototype returns
  only the first parse error in the file. Should
  `grammar.recovery.statement` require continued reporting after each
  statement boundary?

- **Q16 benchmark example regression.** At `212f4de9`,
  `cargo run -p hd_driver --example bench -- 20 1` exits with
  `missing-requirement`: its generated `fn main()` calls `println` but does
  not declare `$ Console`. May the orchestrator restore the row in
  `compiler/crates/hd_driver/src/bench.rs`?

- **Q9 integration scope.** May the Q9 commit also replace the hand-written
  `Code` block in `compiler/crates/hd_diag/src/lib.rs` with `mod codes; pub use
  codes::{Code, Phase};`? The job permits only generated `codes.rs` under
  `compiler/crates/`, but without that one-line module hook the generated enum
  cannot become `hd_diag::Code`.
- **Q10 std folder cycle.** M3's folder graph rejects the current
  `std -> std.testing -> std` loop before it can build std interfaces. Should
  `With` and `with` move into `std.testing` (changing
  `use std.testing.arbitrary.with` to `use std.testing.with`), should std get
  a bootstrap-only SCC exception, or should the prerequisite modules move to
  lower folders so the public child module can remain? Example:
  `use std.testing.arbitrary.with` is the current public path across the loop.
