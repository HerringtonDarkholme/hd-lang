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

### S11d. Apply The Next Term Replacements (Orchestrator Decision)

From S11c's list, apply (wording only; same process as S11c: spec text,
rule IDs retired and renamed, citations, glossaries, fixture comments,
guide, lib/std doc comments, spec-terms rows):

| Coined | Use instead |
| --- | --- |
| collect target | the collection being built |
| bound-only parameter | a parameter named only in bounds |
| template helper | a private item the template uses |
| test registration function | test registration call |
| trait-less derivation block | derivation block with no trait |
| unit test case | unit test |
| initialization group | module cycle (modules that use each other; one module alone is a trivial cycle) |
| executable entry point | entry point |

Leave filenames and test-name strings alone, as S11c did. `bash
spec/check.sh` green. Timebox 45 minutes; push.

### S13. Spec: Embedded Fields Count As Promoted Members

The promotion work (#16) found that `names.promote.member` must count a
part's embedded fields as promoted members, though embedded fields are
never `pub`; otherwise `diamond-different-depths` cannot hide the deeper
copy while `diamond-same-depth-conflict` still reports. Add one
sentence (and a rule ID per `spec/STYLE.md`) to the promotion rules in
`spec/lang/03-names-and-scopes.md` / `08-data-and-enums.md`, with a
one-line example; cite the two fixtures. `bash spec/check.sh` green.
Timebox 20 minutes; push.

### D2j. Reconcile The Overnight Compiler Work

Since 21aee16d's neighbours, main gained (titles in `git log` since
c20e4ca6): item spans for header findings, missing-trait-method,
coherence skipping orphan impls, decorator target checking, literal
suffixes/prefixes/ranges, literal default types (bare usize, signed
i32), diagnostic rendering and messages, embedded member promotion, row
polymorphism (with per-row specialization — a known deviation from
`req.poly.one-body`), row and generics determinism, associated-type
projections, StrIndex and byte intrinsics, shared cells for mutable
captures, char intrinsics, and `hd check`. Run D2 for all of it:
update `reconciliation.md` (mark fixed rows fixed; add each commit's
"not fixed" findings to the backlog with its task number from the
orchestrator's list where the commit message or report names one) and
`footprint.md` counts; record design decisions the code made where the
design was silent (e.g. anchors for header spans in a third Iface
section; shared cells; the closure provider context). Docs only;
timebox 60 minutes; push.

### Q22. Runtime Versus Node (Goal `runtime`)

Under `compiler/bench/runtime/`: six small user-style programs the new
compiler runs today (integer loops, string building, list sort, map
counting, trait dispatch through a bound, closures), each with an
equivalent hand-written JavaScript program. A rerunnable script runs
both on the same Node (hd via `compiler/target/release/hd build` then
the host's runner, JS directly), 5 runs, p50/p95, warm-up excluded, and
writes `audit/compiler/runtime-vs-node-<short hash>.md`: per program the
ratio hd/JS, the geomean, and anything over 1.5x flagged with a guess
at the cause (allocation, boxing, checked arithmetic, call overhead).
Report only. Timebox 60 minutes; push.

### Q23. Mistake Corpus And Diagnostic Location (Goals `mistakes`, `diag-location`)

From `audit/hd-writing-log.md` and Q21's report, build
`compiler/bench/mistakes/`: one small `.hd` file per logged mistake
kind (up to 40), each with its expected diagnostic code and expected
line in a header comment. A script runs the new compiler's `hd check
--format json` on each and reports: share with the expected code, share
whose reported line is the mistake's line (goal ≥ 95%), diagnostics per
mistake, and the worst offenders. Write
`audit/compiler/mistakes-<short hash>.md`. Report only. Timebox 45
minutes; push.

### S14. Spec: `hd check --summary` And The Text Summary Line

The `hd check` implementer found the per-code and per-file counts of
`--summary` (`cli.check.summary-mode`) and the text-mode summary line
unspecified. Specify them in `spec/cli/command-line.md` (numbered rules
with IDs per `spec/STYLE.md`, one example each; keep it minimal and
consistent with `hd test`'s summary from S6), and add CLI cases to
`spec/conformance/cli-cases.tsv` where it has none. Timebox 30 minutes;
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

(none)
