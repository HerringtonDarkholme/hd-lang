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

### R1. Triage The 290 `fail:no-diagnostic` Cases

`compiler/CONFORMANCE.md` lists 290 cases where the new compiler accepts a
program the fixture expects it to reject. Group them by the expected
diagnostic code (the fixture's `# diagnostic:` marker, or `cases.tsv`'s
`reject:CODE`) and the spec rule each fixture cites. For each code: the
count, the rule IDs, two or three fixture paths, and a one-line guess at
where the check belongs (resolution, header check, body checker, flow
analysis). Sort by count. Mark codes that the compiler never constructs
anywhere (`grep` `Code::Name` under `compiler/crates/`) as "check missing"
versus "check exists but misses this case". Write
`audit/compiler/no-diagnostic-triage-<short hash>.md`. Report only.
Timebox 60 minutes; push.

### R2. Triage The `unsupported:Body`, `Emit` And `Collect` Cases

From the same report (194 Body, 137 Emit, 88 Collect), group the cases by
the unsupported message the runner records (run `cargo test -q --release
-p hd_driver --test conformance` from `compiler/` with
`HD_CONFORMANCE_ONLY` on a sample, or read the runner's reason strings),
normalized to the construct (e.g. "ItemRef", "DefaultCall", "f32
arithmetic", "a call that collection did not resolve"). For each
construct: count, stage, two or three fixture paths, and the spec rule.
Sort by count. Write `audit/compiler/unsupported-triage-<short hash>.md`.
Report only. Timebox 60 minutes; push.

### R3. Triage The CLI Tier's 84 Failing Cases

`compiler/crates/hd_cli/tests/cli_conformance.rs` runs the spec's CLI
cases against `hd` (18 of 102 pass; section "CLI Conformance" at the end
of `compiler/CONFORMANCE.md`). Run it with `HD_CONFORMANCE_ONLY=<case>`
per case (or all) and record each failing case's first failed assertion.
Group by cause (command missing, flag missing, manifest section missing,
module resolution, compiler gap, wrong exit code, message difference) with
the case names and the `cli.*` rules they check. Write
`audit/compiler/cli-triage-<short hash>.md`. Report only. Timebox 45
minutes; push.

### D2l. Design Text After The Afternoon's Compiler Work

Update `future-work/compiler/*.md` (and `reconciliation.md` rows) for
these commits, docs only:
- "check: HeaderCheck findings are cached by hdr_key (#95)": the key's
  inputs (toolchain, package, folder path, own deep hash, every closure
  folder's deep hash), one `Graph` entry per folder (the design has one
  package-wide `graph` entry), and that header checks read only the
  interface (a future check of private items must widen the key).
- "emit: the primitives' intrinsic operator and comparison methods share
  the operator lowering (#91)": the intrinsic methods lower through the
  operator code in the emitter; `cmp`/`partial_cmp` build `Ordering` from
  the comparisons; f64 NaN gives `.None`.
- "resolve: mut over an unconstrained type parameter … (#70)": where the
  check runs (lowering, beside `mut-on-primitive`), "unconstrained" means
  no bound list, `mut Self` is exempt.
- "hd check: summary line, --summary, --max-errors (#85)" and
  "Conformance: run the CLI tier against the hd binary (#103)": the
  `commands.md` / `testing-the-compiler.md` text for the summary output
  and the CLI-tier runner (where it lives, its pass list section).
- `hd build --release` currently selects only the output folder; codegen
  takes no profile (record as a gap; Q24 found it).
Timebox 45 minutes; push.

### Q26. Where The Time Goes In map-count And string-build

Q22/Q24 found map-count 60x and string-build 84x slower than Node. Profile
both hd programs (`compiler/bench/runtime/progs/`) under Node
(`node --cpu-prof compiler/host/run.mjs …` or V8's `--prof`, whichever
works here) and attribute the time: which emitted functions or runtime
helpers dominate (hashing, equality calls, boxing/allocation, `bytes_concat`
copies), with percentages. For string-build, confirm the copy volume
(bytes copied per append) against the 320 KB result. End with the two or
three changes that would remove most of the gap, each marked
representation change or implementation slip. Write
`audit/compiler/runtime-hotspots-<short hash>.md`. Report only; no
`compiler/crates/` edits. Timebox 60 minutes; push.

### S17. Spec: What A Release Build Changes

Q24 found `hd build --release` emits the same code as debug. Before the
compiler changes, the spec must say what release changes. Collect every
rule in `spec/` that differs by build mode (debug, test, release): e.g.
`types.arith.checked` ("in a debug or test build"), `dbg-in-release`,
`instantiation-too-deep`, any `--release` rule in
`spec/cli/command-line.md`. If the spec already says, for every one, what
release does (e.g. integer overflow wraps), write a short table of them
into `future-work/compiler/commands.md` (or the design doc that owns build
profiles) and stop. If any is unstated (e.g. what an overflowing `+`
gives in release), don't decide: add each to this file's Questions
section with options and a recommendation, citing the rule. Timebox 30
minutes; push.

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
