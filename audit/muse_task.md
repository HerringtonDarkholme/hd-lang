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

### R1. Triage False Errors On Valid Programs (#125, Research)

About 160 valid fixtures (expectation `accept`) fail with an error the
spec does not allow. Group them by **root cause**, so the orchestrator
can queue one compiler task per cause.

- Run the per-case listing from `compiler/`:
  `HD_CONFORMANCE_ONLY=.hd cargo test -q --release -p hd_driver --test
  conformance -- --nocapture > r1-cases.log` (each line: verdict, path,
  expectation, first diagnostic). Keep the lines whose expectation is
  `accept` and whose verdict is `fail:<code>`, leaving out
  `fail:runtime-exit` and `fail:stdout` (those are R2).
- For each group, find the cause: shrink one or two cases to the
  smallest program that still shows the false error (run it with
  `target/release/hd check` or `hd test` on a scratch file inside your
  worktree), and name the spec rule the program relies on.
- Write `audit/compiler/triage-false-errors.md`: one table row per root
  cause with count, the cases, the minimal program, the spec rule, and
  the stage that raises the error (Check, Collect, ...). Mark a cause that
  matches a known task: private `main` taken as an entry (#127), void
  final value (#128), literal-variable inference (#30), omitted result
  type `fn f(): +42` (#80), slices (#58), method choice among several
  traits (#42), header bounds and associated-type bindings (#15), row
  aliases (#49), literal-kind canonicalization (#77). Biggest cause first.
- Read only: never edit `compiler/crates/` or a fixture. A fixture you
  think is wrong per the spec goes under "Questions" with the rule.
- Timebox 60 minutes; push what you have, with the untriaged cases listed
  at the end.

### R2. Triage Wrong Run Results (Research)

`fail:runtime-exit` (57) and `fail:stdout` (10): the program compiles but
traps, panics or prints the wrong thing. Same method as R1 (same log,
those two verdicts only). For each root cause record whether it is a
Wasm trap, an hd panic with the wrong code, or wrong output, with a
minimal program and the stage you suspect (Emit, runtime host).
Write `audit/compiler/triage-wrong-runs.md`. Read only; timebox 45
minutes.

### R3. Design Note: Functions As Values (#46, Design Text)

Next compiler task after the current one. 41 programs stop at Emit on an
`ItemRef` (a named function used as a value) and 14 at Body on "a path
used as a value". The intended model is a generated adapter per
instance. Read spec 07 (`fn.ref.*`: unbound method references take the
receiver first, bound references `counter::bump`, generic functions
referenced with or without type arguments), `codegen.md` (closures,
function values, `§13` instances) and `checking-and-tir.md` (how a
path expression becomes TIR). Then add a section to
`future-work/compiler/codegen.md` that says, for each reference kind,
what TIR node carries it, what instance key the adapter gets, what the
adapter's body is, and how a bound reference captures its receiver;
include the Wasm shape (the closure struct and `call_ref` type) and the
footprint cost per adapter. Reuse the existing closure representation; no
new runtime form. Design gaps go under "Questions" with a recommendation.
This job lifts the `future-work/compiler/*.md` rule for codegen.md only.
Timebox 45 minutes.

### R4. Design Note: Spreads (#122, Design Text)

Same format as R3, for spreads: a data literal spread with copy-update
(`Point { ...base, x: 1 }`, 23 programs), a spread argument (21), and a
list or tuple spread (6). Spec: the data-literal and call chapters
(`data.literal.*`, the spread rules in 05/07/08). Say which TIR node each
lowers to, the evaluation order (spec rules), and the emitted code
(field copies, a list builder of known size). Add it to
`future-work/compiler/checking-and-tir.md` (lowering) and `codegen.md`
(emission); this job lifts the rule for those two files. Timebox 45
minutes.

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

- **Q-F1: option 1, for all 122 fixtures; F1 is withdrawn and the
  partial commit 3b3a9181 is reverted.** F1's premise was the
  orchestrator's mistake: a non-pub `main` is an ordinary function
  (`module.entry.private-main`), so `module.entry.result-termination`
  never applies to these fixtures. The partial rewrite also left non-void
  final expressions in `-> void` bodies (`ord-supertrait-dispatch`,
  `u8-checked-add`, ...), which `fn.body.void-final` rejects; the four new
  passes came from a compiler gap, not a fix. The real bugs are in the
  compiler (it treats a private `main` as an entry point, and it does not
  enforce `fn.body.void-final`), and the orchestrator's agents fix them.
  Lesson for fixture jobs: when a job's recipe conflicts with a spec rule,
  stop and ask instead of changing the recipe.
