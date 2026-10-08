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

### D2f. Reconcile M4d

M4d landed (b756387e): module init, suspension state machines and the
`defer` exit ladder run end to end. Run D2 for it, and record (citing
"M4d gap n") with the intended rule:

1. No liveness (suspension.md §14.2 step 1): frames save every local at
   every point; dead references are not cleared (Codex re-review N-B6).
2. Resume uses pc-guarded block lists, not the `br_table` dispatch loop
   §14.2 chose for code size. Orchestrator decision: keep it for phase
   1; restore the designed dispatch in phase 3 ("make it wonderful")
   when size work starts. Record it as a known deviation.
3. The callee's frame type: §14.1 types `f$body`'s frame as `$F_f`,
   unknown to callers until the callee is emitted; M4d uses
   `$Suspend_L` plus a cast, and a layout-independent `$Task` prefix
   (cancel, state, flags) so parents cancel children of any result
   type. Adopt or correct in suspension.md.
4. No waker objects or wake masks: a completed-handle table; `all!`
   re-polls every unfinished child; no handle generations or slot reuse.
5. Not built: the competing-driver field and check, the
   forbidden-context counter (indirect `block_on`/`println`), the debug
   deadlock report, hook emission.
6. Init: statement order inside a multi-module group with statements in
   more than one module; providers in the entry module's top level
   (`module.init.script-row`).
7. Checker gaps for the follow-up list: S5 shared enum data (constructor
   expressions not checked into the Init body; field reads fail
   `unknown-data-field`); `"$x"` interpolation of a top-level binding
   fails `unknown-name`; every `for` lowers to `iter`/`next`, never the
   For tags, so `for i in 0..3` fails at emission; a `main` returning
   `.Err` exits 0.

Update `reconciliation.md` and `footprint.md`. Docs only; timebox 45
minutes; push.

### Q18. Conformance Runner For The New Compiler (A Cargo Test)

Phase 2 ("make it work") is driven by the conformance suite. Add one
test target, `compiler/crates/hd_driver/tests/conformance.rs` (never an
`hd` subcommand), that runs every `spec/conformance/` case through the
new compiler by its phase (parse, type, runtime with expected stdout and
exit code, cli where it can), and writes `compiler/CONFORMANCE.md`: pass
/ fail / unsupported counts per chapter and per directory, plus a
bucketed failure list (first diagnostic code or `unsupported` stage).
It fails only on a crash or when a previously passing case regresses
(a checked-in pass list it updates with `HD_UPDATE_CONFORMANCE=1`), not
on unsupported cases. Run it at the commit you start from and commit
the numbers. Test code and `compiler/CONFORMANCE.md` only; fmt and
clippy `-D warnings` clean. Timebox 60 minutes; push.

### Q19. Diagnostic Rendering Versus The Spec

The new compiler prints `error: main.hd:0..39: error unknown-module:
unknown-module ...` (byte offsets, "error" and the code repeated).
Compare its rendering against the spec's diagnostic format rules
(`spec/cli/command-line.md` and wherever the spec defines rendering,
positions and the JSON form) on five broken programs (parse, name,
type, row, runtime panic). Write `future-work/compiler/diagnostics-format.md`:
a table per rule (spec rule ID, required form, new compiler's output,
match / differs), and the list of changes the compiler needs. Report
only. Timebox 30 minutes; push.

### Q20. One Known-Gaps Backlog

Merge every open gap from `reconciliation.md` (D2a–D2f findings),
`skeleton-findings.md` and the M-reports' "still unsupported" lists into
one backlog table in `reconciliation.md`: gap, owning design section,
phase (1 move / 2 work / 3 wonderful), size (S/M/L), and which
`phase2-jobs.md` job absorbs it (or "none: add a job"). Remove items
already fixed on main (check the code). Docs only. Timebox 45 minutes;
push.

### T1. CI: The New Compiler's Gates (After Q18)

Edit `.github/workflows/test.yml` (never trigger workflows; a push runs
it): in the Rust job, add the conformance cargo test from Q18 (it fails
only on a regression), cache `~/.cargo` and `compiler/target` keyed on
`compiler/Cargo.lock` and the toolchain, make sure Node is installed
for the runtime tests (`hd run` uses Node), and build the `hd` release
binary once so the CLI tests reuse it. Keep job names stable. Check the
YAML locally with a parser (e.g. `node -e` with a YAML lib already in
node_modules, or `python3 -c 'import yaml'` if available). Timebox 30
minutes; push, then read the run's result with `gh run list` (read
only) and fix if red.

### T2. Fixture Phase-Tag Audit

`spec/conformance/cases.tsv` tags each case with a phase (parse, type,
runtime, ...). M2 found cases tagged `type` that expect parse codes.
Write `spec/tools/phase-audit.ts` (and a `pnpm run spec phase-audit`
entry if `spec.ts` has a subcommand table): for every row, the expected
diagnostic code's phase (from the spec's diagnostics tables, the same
source as Q9's generator) must match the row's phase; report
mismatches. Add a test beside `spec-tools.test.ts`. Then fix every
mismatch in `cases.tsv` (tags only; one line of reasoning per group in
the commit message). Add the audit to `spec/check.sh`. Timebox 45
minutes; push.

### T3. Spec Rule Coverage By The New Compiler

Extend `pnpm run spec coverage` (spec/tools/spec-coverage.ts) with a
`--compiler` mode: read Q18's pass list and report, per chapter, how
many rule IDs are cited by at least one fixture the **new compiler**
passes, next to the existing "cited by any fixture" share. That is
phase 2's progress number. Test beside `spec-tools.test.ts`; document
in `spec/tools/README.md`. Timebox 30 minutes; push.

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
