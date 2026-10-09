# Muse Task Queue

> **Active again, 2026-10-08 ~16:00 (owner: "muse is back").** Pull
> origin/main into your worktree first; S14, S15 and S16 were done by
> the orchestrator while you were away.

> **Owner, 2026-10-07 night:** Codex died; its open jobs moved here
> ("try offload work to muse"). Same rules as Codex had: small, checkable
> jobs; no conformance-fixture coverage jobs; never edit
> `compiler/crates/` product code (test files and generated files only
> where a job says so). The orchestrator reviews every commit.


This file is Muse's work queue. The orchestrating session adds jobs here.
Muse does them top to bottom, one commit per job, and deletes a job's
section in the same commit that finishes it. Git history keeps the record.
When the queue is empty, report that and wait.

## How To Work

- Work in **one** long-lived git worktree of your own, for example
  `git worktree add /private/tmp/muse-work -b muse/work origin/main`
  the first time, and reuse it for every job. Keep it; don't delete it.
- **Never write in the shared main checkout**
  `/Users/hd/code/test/hd-lang`: no edits, commits, or checkouts there.
  Your commits reach `main` only by `git push origin HEAD:main` from your
  worktree.
- Start each job from current main: `git fetch origin && git reset --hard
  origin/main` in your worktree, but only when it holds no unpushed work
  (check `git status` and `git log origin/main..HEAD` first).
- **Any change under `compiler/` (samples and bench inputs included) or
  to a conformance fixture runs the whole Rust suite before you push:**
  `cargo test -q --release --workspace` from `compiler/`. Crate tests
  read samples by line number (`hd_cli/tests/test_cmd.rs` asserts
  `compiler/samples/testing/cart.hd` lines), so the conformance gate alone
  is not enough; U1 turned CI red that way. If a crate test needs an
  update you can't make (`compiler/crates/` is not yours), stop and say
  so in the commit message instead of pushing.
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
- When a job's recipe conflicts with a spec rule, stop and ask under
  "Questions" instead of changing the recipe (F1, 2026-10-08).
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
- `spec/`: **frozen (owner, 2026-10-08 night: "freeze all spec", "no
  change, log them all")**. No edits to rule text, std chapters,
  conformance fixtures or indexes, whatever a job or an earlier answer
  says. Log every spec or fixture issue you find as a row in
  `audit/compiler/diagnostic-notes.md` and go on.

## Jobs

### R24. Design Note: Small Body Gaps (#123, Design Text)

From your R22 plan: `**` (14), open range patterns (9), full range `..`
(6), compound assignment (4), tuple patterns with a rest (4),
assignment targets (7), mixed positional and named variant fields (4),
`RestType` in a body (5). For each, the spec rule IDs and the TIR it
lowers to (a desugar into existing nodes where the design allows), one
row each in a table in `checking-and-tir.md`. Timebox 45 minutes.

### R25. Design Note: Type Names Used As Values (Design Text)

7 programs use a type name as a value (`Mailer`, `Priced`, `list`,
`Set`, `T`), 2 call a type name (R22). Read the spec rules (names,
`types.*` for type-as-value, constructors, `type-used-as-value`) and say
which of these are valid (and what they mean) and which must be
errors, citing rule IDs; then the lowering for the valid ones. If the
spec does not settle a case, log it in
`audit/compiler/diagnostic-notes.md` instead of deciding. Add to
`checking-and-tir.md`. Timebox 30 minutes.

### R26. Turn Your R5 Report Into Task Rows (Research)

`audit/compiler/triage-missing-errors.md` groups 250 accepted invalid
programs into 21 causes. For each cause not already matching an
orchestrator task number, write one proposed task line (title in the
form "[check] what is missing (N programs)", the spec rule IDs, the
stage) at the end of that file, biggest first. Read only; timebox 20
minutes.

### R27. Profile The Runtime Of The Samples (Research)

P1 measures compile time. Measure run time: build each sample under
`compiler/samples` and the six programs in `compiler/bench/runtime/progs/`
with the current compiler, run them (`hd FILE.wasm` or the bench's
`run.mjs`), and compare with the last runtime report in git history
(`runtime-vs-node-*.md`, `runtime-hotspots-*.md`). Flag only
order-of-magnitude problems, with the hot function if you can find it.
Write `audit/compiler/runtime-<date>-<short hash>.md`. Timebox 45
minutes.

### R28. Design Note: Header Trait Bounds And Associated-Type Bindings (Design Text)

About 26 valid programs fail with `unsatisfied-trait-bound` because header
bounds and associated-type bindings (`where`-style bounds on items, `T <
Trait[Item = U]`, bounds on impl headers) are not carried into bodies and
call sites. Shrink 3-4 to minimal programs, read the spec's bound and
associated-type rules (`spec/lang/09-traits.md`, `04-type-system.md`) and
`trait-solver.md` (bounds as subgoals §3.6, projections), and add to
`trait-solver.md` where each header bound enters the environment of a
body and of a call, and how a binding constrains a projection. Questions
with a recommendation. Timebox 45 minutes.

### R29. Design Note: Requirement-Row Aliases In Interfaces (Design Text)

About 26 programs stop at FolderIface on row aliases and 8 on context
types in headers. Read the row-alias rules (`req.row.alias.*` in
`spec/lang/11-requirements-and-suspension.md`) and
`resolution-and-interfaces.md` (folder interfaces), and add to it how an
alias is recorded in an interface, expanded at use, hashed for caches, and
printed in diagnostics (`req.row.alias.diagnostics.expanded`). Timebox
45 minutes.

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

(none open; earlier questions and answers are in git history)
