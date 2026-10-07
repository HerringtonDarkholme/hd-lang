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

### Q9. Diagnostic Codes Generated From The Spec

Reconciliation item 7: `hd_diag`'s code enum is written by hand and
holds only the 28 syntax codes. Write a generator under `spec/tools/`
that reads every diagnostic code the spec defines (the chapters'
diagnostics tables, the same source `spec check` uses) and writes
`compiler/crates/hd_diag/src/codes.rs` (generated; a header says so):
the enum, `as_str`, `from_str`, and the phase each code belongs to.
Add a `spec check` step that fails when the generated file is stale.
This one generated file is the only `compiler/crates/` file you may
write; keep the existing enum's public names compiling. `cargo test`
green, `cargo fmt --check` clean. Timebox 45 minutes; push.

### D2a. Reconcile M1 (Now)

M1 landed as 07c74892. Run D2 for it, and record these M1 findings in
the owning design docs (each a short rule, citing "M1 finding n"):

1. The TIR hash excludes location columns (any header edit shifted every
   later body's node indices and re-emitted all of them; checking-and-tir.md
   §4.13, cache.md §5.3).
2. `ir::Body` has a per-body constant table behind `Ref::konst`
   (data-structures.md, the TIR section).
3. Block ownership: a block owned by an `If` or `Loop` is not also listed
   in its parent's block list; state one rule and the verifier invariant
   (checking-and-tir.md).
4. The solver answers `Fails(NoImpl)` when every candidate head is exact
   and none matches (trait-solver.md).
5. `deep_hash(F)` over-invalidates (hashes all used folders, not only
   those F's interface mentions): record as an open gap with the
   intended rule (cache.md, resolution-and-interfaces.md).
6. Every diagnostic carries a real primary span; name which phases emit
   span-less diagnostics today (folder-cycle, overlapping-impl,
   missing-entry-point, unsupported) as a gap.

Then update reconciliation.md (mark fixed items fixed) and footprint.md
(the rows M1 made real: data-structures §3.18, §3.21; scheduler §6.2;
wasm-layout §15.1, §15.2; cache §5.4; runtime-and-host §17.9; commands
§7.5, §20.2, §20.3). Docs only; timebox 45 minutes; push.

### Q10. Std Bootstrap Inventory (For M3)

Research only, one doc: `future-work/compiler/std-bootstrap.md`. What
the new compiler needs so `lib/std` reaches programs, read from
`lib/std/*.hd`, the spec's prelude and intrinsic rules, and
`compiler/crates/hd_host_abi`:

- every `@intrinsic` (or equivalent) in lib/std: name, signature, which
  design section owns its lowering, and whether `hd_host_abi::TABLE`
  has it;
- the prelude: which names every program sees, and from which modules;
- every derive and annotation lib/std uses, with the design section
  that handles it;
- the folder graph of lib/std (which folders, their uses, any cycles);
- a dependency order for M3: which pieces unblock the most.

Tables, not prose. Don't edit code. Timebox 45 minutes; push.

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
