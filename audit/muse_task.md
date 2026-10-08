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

### Y1. String Appends: Design Options For A Rope Or Builder (Research)

Q26 (`audit/compiler/runtime-hotspots-efc99933.md`) found string append
is O(n²): 40k appends copy ~6.4 GB for a 320 KB result (84x vs Node).
Read `future-work/compiler/representation-runtime.md` §6 Strings (and
what `lib/std` and the spec promise about `string`: immutability, UTF-8,
O(1) `len`, slicing/indexing on views, hashing, equality, boundary
crossing to the host). Then compare how three or four real systems avoid
the quadratic append — V8 cons-strings flattened on demand, Java/Go
builders (`StringBuilder`, `strings.Builder`), Rust `String` with
amortized growth plus `+=` reusing the left buffer when it is uniquely
owned, Swift/Koka-style in-place append on a unique reference — and for
each: what it costs hd (heap layout under Wasm GC, every string op that
must flatten, hashing/equality cost, host boundary, code size in a
hello-world), and what it needs from the language (a builder type in
std, or nothing visible). End with a recommendation and the smallest
first step. Write it as a proposal section appended to
`future-work/compiler/representation-runtime.md` §6 (clearly marked
"Proposal, not accepted") — no spec edits, no new user-visible names
without listing them as owner questions in this file's Questions
section. Timebox 60 minutes; push.

### L1. Std Inventory: Spec'd Items Missing From `lib/std`

Some fixtures fail because a std module the spec defines does not exist
in `lib/std` (e.g. `std.sys`, `std.net`). Compare `spec/std/*.md` (every
module and its public items: types, traits, functions, constants, with
their rule IDs) against `lib/std/*.hd` (and the compiler's seeded std
items in `compiler/crates/hd_resolve/src/seed.rs`). List per module:
items in the spec but missing from std, items in std with a different
signature than the spec, and items in std the spec doesn't mention. For
each missing item, note whether it needs a compiler intrinsic (host
capability call, runtime primitive) or can be plain hd. Count the
conformance fixtures that use each missing item (grep
`spec/conformance`). Write `audit/compiler/std-inventory-<short hash>.md`.
Report only. Timebox 60 minutes; push.

### D2m. Design Text For Module Paths And Poison Names

Update `future-work/compiler/resolution-and-interfaces.md` (and
`reconciliation.md` rows) for two commits, docs only:
- "Module paths: use roots for the source, test and task roots and path
  dependencies (#106)": one mapping in `hd_project` (`module_path`,
  `module_below`); the package name with `-` as `_`; `src/lib.hd` is the
  root, `x/mod.hd` is `x`; test and task roots use internal prefixes
  `P.$tests…` / `P.$tasks…` (record as provisional: an owner question on
  the collision between `tests/checkout.hd` and `src/tests/checkout.hd`
  is open); where `self` starts and the root `super` stays within; entry
  modules not usable; `UseRoots::absolute` for `pkg`, `std`, `dep.NAME`,
  `self`, `super`; dependencies load only their library; `package-cycle`;
  a package with `src/` takes files only from `src`, `tests`, `tasks`.
- "Resolver: a failed use binds its names as poison (#110)": where poison
  is produced and read, and that a real binding replaces poison.
Timebox 40 minutes; push.

### D2. Reconcile After Each Orchestrator Milestone (Standing)

Each time a commit titled "M1:", "M2:", "M3:" or "M4:" lands on main,
rerun the reconciliation of `future-work/compiler/reconciliation.md`
against the new code: update its top-10 and table (mark fixed rows
fixed, add new gaps), apply design-doc corrections the code proves
right, and update `footprint.md` counts. Docs only. Timebox 45 minutes
per milestone; push.

### P2. Hello-World Size Growth

`audit/compiler/size-hello-32d0f278.md` recorded hello at 4,775 bytes
(`println(42)`, dev names, debug). Main now builds it at 4,914 bytes. Find
which commits since `32d0f278` added the 139 bytes (bisect with the same
reproduction and `compiler/bench/wasm-size.mjs`), say per commit what
grew (section, function) and whether the commit's feature explains it.
Refresh the size report (delete the old one) with the per-section table
at current main. Report only. Timebox 40 minutes; push.

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
