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

### U1. `use std.testing…` Goes Inside The `tests:` Block (First)

Owner, 2026-10-08: "just fix your examples". About 895 files put
`use std.testing…` at the top level although only their `tests:` block
uses it (`rg -l '^use std.testing' | xargs rg -l '^tests:'`): 829
conformance fixtures, plus `guide/`, `website/`, `spec/lang`, `spec/std`,
`README.md`, `examples/`, `test/`, `compiler/samples`, `compiler/bench`
and code blocks in Markdown. The rules (spec 03 Tests Blocks,
`names.tests.inside-only`, `names.tests.shadow`): a `use` inside the block
is visible only inside it, so test-only imports belong there.

For each file (or each Markdown code block) that has both a top-level
`use std.testing…` line and a `tests:` block:
- if every name that line imports is used only inside the block, move
  the line to the top of the block (indented four spaces, then a blank
  line before the block's first other item); if the block already
  imports some of the same names, merge into one `use` without
  duplicates;
- if any imported name is also used outside the block, leave the file
  alone and list it in the commit message;
- leave alone `spec/conformance/runtime/valid/tests-block-use-shadow.hd`,
  `spec/conformance/typing/invalid/tests-block-use-leak.hd` and the
  Tests Blocks section of `spec/lang/03-names-and-scopes.md` (they test
  or show the rules on purpose);
- files without a `tests:` block (test modules, integration test
  modules) are test code throughout: their top-level imports are right.
  Don't touch them.

A small script is fine for the `.hd` files (don't commit it); read a
sample of its diffs before committing. Fixtures whose expectations name
line numbers (a `# line:` header, a panic frame `FILE:LINE`, an
`expect-stdout` with a line) shift when a line moves: check each such
fixture and fix the number in the same commit, or leave the file alone
and list it.

Checks: `cargo test -q --release -p hd_driver --test conformance` from
`compiler/` must pass (no case lost from `compiler/CONFORMANCE.md`; if a
case is lost, revert that file and list it); `cargo test -q --release -p
hd_syntax --test corpus`; and `bash spec/check.sh` run in a clean
worktree (the main checkout has an untracked file that aborts it). One
commit per area (fixtures; spec Markdown; guide and website; the rest).
Timebox 60 minutes; push.

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
- Solver commit and trials (commits "solver: commit on the head…" and
  "check: Instantiations and Methods goals…"; trait-solver.md §3.4–3.5,
  §6, type-checking.md §2.4–2.5): numeric-family heads match only the
  types their bound's trait lists (`family_excludes`); one impl seen in
  two tables is one head; a failing bound under a committed head answers
  with that bound's failure and the impl/step prepended to `chain`; on an
  overlap the first head in content order is used and the solver reports
  nothing; `Candidate` gained `impl_args`, and a fixed plan step that
  stalls on a variable of `S` becomes residual (§6.5 is silent); residual
  `Bind` steps are not obligations; `Methods` carries the trait list from
  the method index, not an `AvailKey`; the arguments checked once before
  trials versus per trial (closures, `.V` variants, `if`/`match`/blocks,
  empty collections) — record the list; the literal-default retry when
  several candidates fit. Record TS-3 (placeholder as `Maybe`, no teaching
  through a unique head) and §6.4 literal kinds as not yet done.
- Memo (§7, commit "solver: memoize canonical goals…"): canonical keys
  with environment, universe and scope; global memo first-writer-wins,
  published only for finished, uncut, unfueled goals; an entry records
  the `(trait, head key)` probes of its subtree, and a global entry
  serves a module only when its own table has no row for any of them
  (the own-table rule, absent from the design); depth cut stores an
  `AtLeast` entry in the body memo only; fuel charged once per proof
  node; `Methods` goals are not memoized (no availability key yet).
- Unowned rows (commit "solver: scope memo answers that read unowned
  rows…", #87): §3.2's claim that a known-argument goal never depends on
  the universe is wrong when its proof reads unowned rows (misplaced
  impls, built-in-target rows); such answers are "scoped" and published
  under the key with the universe filled in; the universe also includes
  folders with unowned rows; add the §7.1 row.
- Defaults (commit "resolve: fill trait-argument defaults…", #89): §2.1
  should say a projection written under a bound takes the bound's filled
  arguments, and `Self::X` inside a trait leaves them to each use.
- Header checks (commit "check: header checks ask the solver", #68):
  they live in `hd_check::header`, ask `TableSolver` under the item's
  environment with the folder's closure universe and no own table;
  resolution keeps only the overlap check. Record as not done: supertrait
  bindings (`Project`), a fuel diagnostic per item (§4.10.1), compiler-
  supplied traits answered in the checker not the solver (§3.9), and the
  header result not cached in the graph entry.
- Codegen selection (commit "codegen: select through owner lookup…",
  #66): a program build's universe is every folder; selection shares the
  run's global memo; §8.3's head-only `select` with its own table, and
  `Selection.args` returning every impl argument, are not done.

Docs only; no spec edits. Timebox 60 minutes; push.

### T1. Bench Generator Writes A Program That Checks

`compiler/bench/generated/generate.mjs` emits a 10k-line package whose
`main` fails `hd check` with `missing-requirement: $ Console` (both the
TS prototype and the new compiler reject it), so the larger generated
package can't be built or timed. Fix the generator so `main` declares
the requirement it uses (`pub fn main() -> void $ Console:`) and every
generated program checks clean with `compiler/target/release/hd check`
(build it with `cargo build --release -p hd_cli` in `compiler/`). Follow
house style: an i32 literal is `+N`; `:=` is never reassigned (a counter
is `let x = +0`). Generated output stays deterministic. Tooling only.
Timebox 30 minutes; push.

### Q24. Runtime Versus Node, Release Builds

Q22 (`audit/compiler/runtime-vs-node-6572b51d.md`) measured debug-profile
Wasm only. Rerun `compiler/bench/runtime/run.mjs` with release builds
(`hd build --release`; add a `--release` switch to the script if it has
none) on current main, same method (1 warmup + 5 runs, p50/p95), and
write `audit/compiler/runtime-vs-node-<short hash>.md` with both the
debug and release ratios side by side. For string-build and map-count,
say whether release changes the picture (checked arithmetic vs the
quadratic append and per-op boxing). Delete the Q22 report in the same
commit (git history keeps it). Report only. Timebox 45 minutes; push.

### Q25. Does `hd check --tests` Check `tests:` Blocks?

Q23 found `hd check FILE` silent on mistakes inside `tests:` blocks. By
the spec (`cli.check.default`, `cli.check.tests` in
`spec/cli/command-line.md`) a plain `hd check` skips test code, so that
part is correct. Find out, with the new compiler, whether
`hd check --tests FILE` (and package-mode `hd check --tests`) checks
`tests:` blocks and reports `duplicate-tests-block` and
`invalid-test-statement`. Update `compiler/bench/mistakes/run.mjs` to
pass `--tests` for the test-code mistake kinds, rerun it, and write the
new shares into a refreshed `audit/compiler/mistakes-<short hash>.md`
(delete the old one). List any spec rule the compiler misses as a
"compiler gap" line; don't touch `compiler/crates/`. Timebox 45 minutes;
push.

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
