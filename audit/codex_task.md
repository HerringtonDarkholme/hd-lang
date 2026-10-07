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

### D2e. Reconcile M4b

M4b landed (74e2f624): collection crosses into std; the emitter is
rewritten on the layouts; a std-using exit program runs on V8. Run D2
for it, and record (citing "M4b gap n") with the intended rule:

1. Suspension is cut down: `$Suspend_L` is only a poll function
   reference (no state, flags, driver fields, wakers, cancel);
   `block_on` polls then calls `hd:rt.block` (suspension.md).
2. Vtables are `struct.new` at each coercion, not constant globals;
   supertrait vtable fields are not built (codegen.md, wasm-layout.md).
3. Erased scalars are always boxed: the `i31ref` fast path the design
   requires (wasm-layout.md, "Erased scalars") is missing.
4. Code keys lack the full §13.8 dependency list (impl interface hashes,
   inline summaries) (codegen.md §13.8, cache.md).
5. No panic sites: no `hd.sites`/`hd.folds`/`hd.runtime` sections, no
   fold step (runtime-and-host.md, codegen.md).
6. Every local is nullable and narrowed on read (wasm-layout.md).
7. Emission trusts TIR (indexes records directly).
8. Tiny-program size: hello is 4.8 KB with the name section; the 2 KB
   target is missed (std number formatting and helpers come along).
9. Checker bug for the M4 follow-up list: `"${ages["cy"]}"` leaks the
   index key's literal into the interpolation parts.
10. Still `unsupported` in emission: GlobalGet/GlobalSet (module init),
    all Await tags (state machines), With/ContextNew/ContextFor,
    ItemRef, Is, CallHost, DefaultCall, CopyData, SwitchStr, the For
    tags, Scope with defer, MapRemove/MapIter/StrIndex, ToAny,
    Supertrait, f32 arithmetic, wider conversions, shared captures,
    `dyn` generic methods, recursive types.

Update `reconciliation.md` and `footprint.md`. Docs only; timebox 45
minutes; push.

### Q14. Hello-World Size Breakdown

`hello` builds to 4.8 KB (the target is 2 KB, `size-startup-heap`).
Build `compiler/samples/hello` with the release `hd build`, then write
`audit/compiler/size-hello-<short hash>.md`: bytes per section, then
every function and data segment with its size and **why it is
reachable** (the call chain from `main`, e.g. `println` → std number
formatting → ...), and the name section's share. End with a ranked list
of the five biggest contributors, each marked implementation slip or
design issue, with the design section involved. Use a small script under
`compiler/bench/` (wasm-tools or a hand parser) so it can be rerun.
Report only; don't edit `compiler/crates/`. Timebox 30 minutes; push.

### Q15. Phase-2 Job Briefs From The Work Estimate

From `future-work/compiler/work-estimate.md`'s ordered job list, write
`future-work/compiler/phase2-jobs.md`: one section per phase-2 job
("make it work": the checker and emitter long tail, chapter by chapter),
each with: scope in one line; the TS files it uses as a checklist (not
to port); the spec chapters and design sections; the conformance cases
it should turn green (directories or case lists); its exit test; and
its size estimate. Order by dependency and by how many conformance cases
each unblocks. Tables and short lists; docs only. Timebox 45 minutes;
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
