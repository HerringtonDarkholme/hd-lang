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

### S15. Error Code For `mut T` On An Unconstrained Generic

Owner decision, 2026-10-08: `types.generic.no-mut-t` (spec 04 Mutable
Bounds) gets an error code, `mut-on-type-parameter`, after
`mut-on-primitive` and `mut-on-tuple`. A program that writes `mut T` for
an unconstrained `T` reports that code and not `invalid-variance` too.

- In 04, add `Error: \`mut-on-type-parameter\`` to the rule, naming where
  it points (the `mut T` type) the way `mut-on-tuple` does. Add the code
  wherever the other codes are listed (README code table, any phase or
  glossary list), following `spec/STYLE.md`.
- New fixture `spec/conformance/typing/invalid/mut-on-type-parameter.hd`
  citing the rule, with the `# diagnostic:` marker; register it in
  `cases.tsv` and `examples.tsv` as the other fixtures are.
- Rewrite `typing/invalid/covariant-mut-method-parameter.hd` so it still
  tests a covariant parameter beneath `mut` but is legal under
  `types.generic.no-mut-t`: `other: mut List[T]` in place of
  `other: mut T`, still expecting `invalid-variance`.
- Regenerate `compiler/crates/hd_diag/src/codes.rs` with
  `node --experimental-strip-types spec/tools/diagnostic-codes.ts` (the
  only `compiler/` file you may touch; it is generated).
- Checks: `bash spec/check.sh`; `cargo test -q --release -p hd_driver
  --test conformance` from `compiler/` must stay green (the new fixture
  is expected to fail until the checker implements it; if the gate
  breaks, report instead of editing the pass list).

Spec, fixtures and the generated file only. Timebox 30 minutes; push.

### S16. Literal Style In The Q22 Runtime Programs

`compiler/bench/runtime/progs/*.hd` writes `let x: i32 = 0` (16 lines).
House style (owner): an i32 literal is written `+N`, and `:=` is
non-reassignable. So `let total: i32 = 0` becomes `let total = +0`
(it is reassigned); a never-reassigned binding becomes `name := ...`
(`let r: i32 = i % 10007` becomes `r := i % 10007`); annotate only
non-i32 widths. Rerun `node compiler/bench/runtime/run.mjs` to confirm
every hd/JS checksum still matches; numbers needn't be re-reported.
Timebox 20 minutes; push.

### D2k. Design Text After The Pool And Solver-Lookup Work

Two architecture commits landed: "pool: …" (#61, ends at 97ef2c30) and
"solver: owner lookup, candidate directory and impl universes" (#62a).
Their implementers listed where the design text was wrong or vague.
Update `future-work/compiler/data-structures.md` and
`future-work/compiler/trait-solver.md` so the text matches the code, and
mark any matching `reconciliation.md` rows fixed:

- Pool (§3.3, §3.9): list items live in a typed `tys` column with an
  `[start, len]` record in `extra` (lending `&[Ty]` from `u32` words
  would need unsafe outside `AppendVec`); the body-local pool is
  hash-consed (the checker compares variable-holding types with `==`)
  and is not truncated on rollback; one fresh local pool per body, not
  one per worker; local index bits 27..30 hold a 4-bit pool generation
  (overlaps the carry/module tier bits 29..30 — record as an open
  layout decision); per-owner columns, the static pre-seed table and the
  node count in `meta` are not built yet.
- Union-find (§3.19): union by rank with trailed link/rank/kind changes,
  no path compression (`root` stays a read).
- Solver lookup (§3.2, §3.3): tables are per folder, not per module;
  the directory is merged once per impl universe from that universe's
  folders (no barrier, so scheduler.md §6.1's "universes add no edge"
  holds) rather than one global directory frozen before bodies;
  `STD_BUILTIN_TABLE` became an "unowned" table that also holds impls
  rejected as `nonlocal-impl`/`orphan-impl`, and a folder joins a
  universe if it has `arg_impls` or unowned rows; the universe is in the
  key of every goal with an open argument (the checker asks
  instantiations and method traits as `Implements` with fresh
  variables); a `Bind` step whose bound leaves trait arguments implicit
  does read the directory; no `arg_impls` section hash (nothing persists
  the directory); `HeaderCheck(F)` has no universe because header checks
  run a separate solver; the memo is not consulted yet; the head index
  is a per-trait permutation, and the `arg_key` fast reject is not done.

Docs only; no spec edits. Timebox 45 minutes; push.

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
