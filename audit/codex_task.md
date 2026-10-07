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

### Q13. Work Estimate From The TS Prototype (Next)

Owner, 2026-10-07: the Rust compiler is written from scratch; the TS
prototype in `src/` (read-only) is the checklist of what must be done at
minimum. Write `future-work/compiler/work-estimate.md`:

1. **Inventory.** Every TS file in `src/` (76k lines): its lines, what
   it does in one line, and which designed Rust crate and design section
   own that responsibility. Group by component (lexer and parser,
   resolution and packages, checker by area, emitter, runtime and host,
   test runner, CLI commands, REPL, docs, other).
2. **Coverage today.** For each group, what the Rust in
   `compiler/crates/` already covers (check the code; `ast-grep outline`
   per crate), as covered / partial / missing, with the Rust lines.
3. **Checklist per group.** The constructs, cases and diagnostics the TS
   handles that the Rust does not yet (names, not prose; enough for an
   agent brief to tick off).
4. **Not needed.** TS code the new design drops or replaces (GADTs,
   removed features, prototype-only workarounds, legacy paths): list it
   so it is not counted as work.
5. **Estimate.** Remaining work per group in TS lines in scope and in
   agent jobs, at about 4 to 7k Rust lines per 2 to 2.5 hour job; then
   an ordered job list for phase 1 ("make it move": M4a-1 to M4a-4, M4b,
   M4c are the current plan) and phase 2 ("make it work"), each job
   with its TS checklist files.

Tables, not prose. Docs only; don't edit code or `src/`. The
orchestrator's M4a agent is changing `hd_check` meanwhile: measure
coverage at the commit you start from and name it. Timebox 60 minutes;
push.

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
