# Muse Task Queue

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

### T4b. Redo The Triage On Current Numbers

T4 used `compiler/CONFORMANCE.md` at 561efcfb (497 pass). Main now
passes 1,022 (P2-1a), and the buckets changed: `fail:unknown-import`
fell from 777 to 29; the largest are now `fail:no-diagnostic` 509,
`unsupported:Body` 274, `unsupported:Emit` 264, `fail:type-mismatch`
126. Regenerate the report first (`HD_UPDATE_CONFORMANCE=1 cargo test
-p hd_driver --test conformance`, in your worktree; about 8 minutes),
then redo the bucket-to-job table, and for `fail:no-diagnostic` split it
by expected code (the `reject:CODE` column). Weigh the order by cases
unblocked per job size; a dependency counts only when the bucket truly
needs the earlier job (say which code path). Docs only (and the
regenerated report). Timebox 45 minutes; push.

### D2h. Reconcile P2-1a And The Haiku Fixes

Landed: P2-1a (44ccaf3f, runner and checker breadth), f92730d4 (pattern
type errors skip the refutability cascade), d91db19e (scripts run:
`Roots::Script`, `script_entry`, entry rows inferred from the init's
calls), a5027712 (a FILE with no `hd.toml` above is a one-file program),
dc19fda8 / 60f37949 (`NotImplemented` carries a span; innermost wins).
Record in the owning docs (citing the commit): the script entry design
(codegen.md, commands.md); the single-file source rule (commands.md);
the span on structured errors (data-structures.md, the error type). Add
to the backlog: `hd run FILE` must be an error (`cli.run.file`) — the
new CLI still accepts `hd run FILE|DIR` and `hd build FILE|DIR -o`
(owner's task 10; see `cli-forms.md`); emit-stage `unsupported` sites
have no span; entry-row inference scans only direct item calls. Docs
only; timebox 30 minutes; push.

### S8. Spec: Where `req.row.alias.no-mut` Is Detected

T2 retagged `typing/invalid/row-alias-mut-key.hd` as a parse-phase
`syntax-error`, but detecting `mut` behind a type alias in a requirement
row needs the alias resolved, which a parser cannot do (the new
compiler ledgers it in `compiler/KNOWN_FAILURES.tsv`). Read
`req.row.alias.no-mut` and its neighbours in
`spec/lang/11-requirements-and-suspension.md`: either the rule is a
type-phase check with its own code (find the existing one or add one
per `spec/STYLE.md`), or the rule must be syntactic (then say how a
parser sees it). Fix the rule text, the fixture's expected code and its
`cases.tsv` phase together, remove the ledger row if the case's phase
moves, and run `cargo test -p hd_syntax --test corpus` and `bash
spec/check.sh`. Timebox 30 minutes; push.

### S9. Spec: `tests:` Imports May Shadow (Owner Decision)

Owner, 2026-10-07, on S7's `names.tests.no-shadow`: **allow
shadowing**. A `use` inside a `tests:` block may introduce a name the
module declares or uses outside the block; inside the block (its cases
and helpers) the block's import wins, as a nested scope; outside the
block nothing changes. `module.test.dev-dependency.in-tests` stays as
written (owner: keep). Codex's Q17 layout and recovery rules stay too
(owner: keep all).

Replace `names.tests.no-shadow` with the shadowing rule (a new rule ID;
retire the old one per `spec/STYLE.md`), drop its `duplicate-module-name`
fixture or turn it into a runtime-valid fixture where the block's import
shadows a module function and the module's own code still sees its own,
and update S7's example in 10-modules.md if it shows the error. Prototype
disagreement gets a `test/portable/KNOWN_FAILURES.tsv` row; run `cargo
test -p hd_syntax --test corpus` and `bash spec/check.sh`. Timebox 30
minutes; push.

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

- P2 (2026-10-07): `pnpm run check` is red on current main for a case
  outside this job: `typing/invalid/row-alias-mut-key.hd` is listed as
  phase `parse` in `spec/conformance/cases.tsv`, but `hd debug parse`
  exits 0 on it and only `hd check` rejects it (syntax-error at 8:1),
  so the spec-suite parse step fails. CI Test is already red on the
  T4/S7 commits for this. Left untouched (not this job's lane); the P2
  report commit changes no `src/`, spec, or fixture.
