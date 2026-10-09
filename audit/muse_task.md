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

### F1. Fixtures: `fn main() -> i32` Becomes `-> void` (Owner, 2026-10-08)

122 conformance fixtures declare `fn main() -> i32`, which
`module.entry.result-termination` rejects (an entry returns `void`,
`ExitCode` or `Result`). The owner chose to fix the fixtures, not the
spec. Find them with `grep -rlE '^fn main\(\) -> i32' spec/conformance`.
Rewrite each to `fn main() -> void:` and keep the body's effects: a body
that is one expression becomes `_ := EXPR` on its own line; a block body
keeps its statements and turns a final value line into `_ := EXPR`. Do
not change anything else in a fixture (its tests, comments, expected
output). This job lifts the fixture rule for these files only.
Checks: `bash spec/check.sh` in a clean worktree, and the full
`cargo test -q --release --workspace` in `compiler/` with
`HD_UPDATE_CONFORMANCE=1` for the driver conformance test; the
CONFORMANCE.md diff may only add passes (list them). One commit; push.
Timebox 45 minutes.

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

### Answers (orchestrator, 2026-10-08 evening)

- **Q-L3: option 1.** The language rules (`trait.dyn.keyword`,
  `types.trait.value.dyn-required`) stand. Add the missing `dyn` in
  `spec/std/net.md` (this answer lifts the spec rule for those three
  signatures only) and in `runtime/valid/net-own-provider.hd`, keep
  `lib/std/net.hd` with `mut dyn`, and land it with L3.
- **Prototype regressions:** `src/` stays frozen, and TS checks no longer
  gate lib/std changes (only `src/` or website changes run `pnpm run
  check`). Do not edit `src/` (no auto-declare fix, no whitelist lines).
  Record each prototype regression or blind spot (the two Inspectable
  fixtures, `map-sys.hd`, `net-own-provider.hd`) as a row in
  `test/portable/KNOWN_FAILURES.tsv` with a finding tag `PROTO-STD`.
- **`hd_driver/tests/checker.rs`:** update its std module count (38 to
  40) in the same commit; this answer lifts the `compiler/crates/` rule
  for that test file only.
- **`retry_with!` and `Backoff`: the owner lifted the batch-73 hold
  (2026-10-08 evening).** Push them with L2, and delete the
  `runtime/valid/retry-with-backoff.hd` row (finding `RETRY-WITH`) from
  `test/portable/KNOWN_FAILURES.tsv` in the same commit. Report hello and
  the 3,000-line bench build before and after, so the std.task to
  std.time cost is on record.

### Q-F1: 87 fixtures' tests assert `main()`'s i32 value (F1 partial: 34/122 done)

F1's recipe (signature to `-> void`, bodies kept) is done and green for
34 fixtures whose tests never use `main()`'s value (list in the commit).
It cannot extend to the other 87: their `tests:` blocks assert the
value, e.g. `assert_equal(main(), 42)`, and with `-> void` that is
`unsatisfied-trait-bound: type 'void' does not implement Eq` (verified
on `comprehensions.hd`, then reverted). "Do not change tests" and
"rewrite each" conflict there. Also found while here: a trailing value
in a `-> void` body is silently dropped (no mismatch error), and
integer literals widen without the `i32` context, so two panic
fixtures needed `let _: i32 = …` to keep their overflow (done), one
marker file (`function-result-type-mismatch.hd`) tests the `i32`
mismatch itself and was left alone, and the 2 `pub fn main() -> i32`
fixtures (`expected-i32-found-usize`, `entry-result-not-termination`)
are deliberate rule coverage, out of F1's grep. Options:

1. **Leave the 87 as `-> i32` (recommended).** Non-pub `main` is an
   ordinary function per `module.entry.private-main`; nothing rejects
   its result type, so the F1 premise does not apply to them. The 34
   already done still silence their `private-main` warnings.
2. **Rewrite the 87 tests to not need the value.** Contradicts "do not
   change tests"; a follow-up job with the fixture rule lifted could
   rename the helpers (e.g. `compute()`) and keep every assertion.
3. **Accept the breakage now, fix tests after.** Not recommended:
   87 red suites for an intermediate commit.

