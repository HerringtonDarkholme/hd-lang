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

### D2i. Reconcile P2-1b And Fix Two Fixtures

P2-1b landed (53563951): the access-permission model (`hd_check/src/access.rs`),
variance checking (`hd_resolve/src/variance.rs`), trait-signature
conformance (`hd_check/src/conform.rs`), severities in the check cache
(layout 3), code keys including a hash of the program's data layouts
(fixes cross-fixture code reuse). Record these in the owning design
docs (type-checking.md, cache.md, codegen.md), citing the commit. Then
the fixtures it found contradicting the spec:

1. `typing/invalid/orphan-impl-nested-trait-argument.hd` contradicts
   `types.fresh.element-permission` and `types.fresh.element-no-weaken`
   (`[[Word { ... }]].iter()` is `mut Iterator[mut List[mut Word]]`;
   returning it as `mut Iterator[List[Word]]` is a second error). Fix
   the fixture so it tests only the orphan rule; then tell the
   orchestrator in the commit message so the checker's readonly-element
   fallback in `list_expr` can be removed.
2. `typing/warnings/unused-cold-suspension.hd` expects only the
   `unused-local-binding` warning, but `flow.unused.must-use` makes an
   unread must-use binding (`mut Suspend[T]`) a `discarded-must-use-value`
   error. Decide from the spec which is right, fix the fixture or the
   rule text, and say which.

Docs and fixtures only; `bash spec/check.sh` and `cargo test -p
hd_syntax --test corpus` green. Timebox 45 minutes; push.

### S11b. Terminology Sweep, Redone With Evidence (Owner, Next)

S11 kept all 186 terms with "ordinary English" / "standard vocabulary"
verdicts and no evidence, including `ghost entry`, `law partners`,
`draw budget`, `coherence slot`, `take part`, `fits`. That is not the
sweep the owner asked for ("don't invent unnecessary new words"). Redo
`future-work/spec-terms.md` with a stricter test:

- **keep** only with evidence: name the language or source and the page
  where the term means the same thing (Rust reference, Go spec, Swift
  book, Kotlin docs, Haskell report, TAPL, QuickCheck docs, ...), or it
  is a keyword hd spells, or plain English used in its everyday sense
  (say which sense). One short citation per row.
- **replace** every term that fails that test and has a plain phrase or
  established term; give the replacement and apply it (spec text, rule
  IDs with retirements, citations via `pnpm run spec refs`, fixtures,
  guide, lib/std doc comments) in commits grouped by chapter.
- **owner** only for a real hd-only concept with no existing word: one
  plain alternative each; list them at the top of the file.
- Expect real replacements: a sweep that replaces nothing has not looked.

`bash spec/check.sh` green after each commit. Timebox 75 minutes; push.

### S12. Glossary Hygiene

`spec/README.md` and `spec/std/README.md` glossaries: merge duplicate
entries (S11 found `same compiled program` / `the same compiled
program`), make every glossary entry link to its defining rule, and drop
entries for terms no spec text still uses (`pnpm run spec glossary` and
a grep). Add a `spec/check.sh` step if `spec glossary` can detect
duplicates and dangling entries cheaply. Timebox 30 minutes; push.

### Q21. Writing-Log Audit: Diagnostics Worth Improving

`audit/hd-writing-log.md` has rows from Haiku and Sonnet sessions with
the compiler's verbatim message and whether it helped (yes / partly /
no). For every "no" and "partly" row, check what the **new** Rust
compiler says today for the same mistake (write the one-line repro, run
`compiler/target/release/hd FILE.hd`), and write
`audit/compiler/diagnostics-from-log.md`: mistake, old message, new
message, helped now?, the spec rule, and a proposed better message (one
line). Rank by how often the mistake appears. Report only; don't edit
the compiler. Timebox 45 minutes; push.

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
