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

### D1. Apply The Reconciliation's Design Changes (First)

Owner, 2026-10-07: design-doc work goes to Codex.
`future-work/compiler/reconciliation.md`, "Design Changes Proposed",
items 1 to 13, and "footprint.md Corrections". Apply them as short edits
in the named docs, each citing "reconciliation, item n". All 13 are
accepted:

- Item 6 is decided by the owner (2026-10-07): allow one audited
  `unsafe` `AppendVec` in `hd_base` (raw chunk pointers, a published
  length); the workspace lint becomes `unsafe_code = "deny"` with an
  allow on that one module only, every block with a `// SAFETY:`
  comment. Write that into data-structures.md §3.9.4 and §3.24.
- Item 9: until wasmtime is approved, `hd run` runs on Node through an
  `hd_run::Engine` implementation (Wasm first).

Docs only; don't touch `compiler/`. The orchestrator's agent is changing
the code to match at the same time, so describe the design, not the
code's current state. `bash spec/check.sh` passing; push one commit.
Timebox 45 minutes.

### Q5. Parse-Gap Repros From lib/std

The new compiler's full parser rejects 19 of the 36 `lib/std` files
(`cargo run -p hd_driver --example stages`, footprint.md "SK-6"). For
each distinct construct that fails, write a minimal `.hd` repro under
`compiler/tests/parse-gaps/` (one construct per file, a comment naming
the `02-grammar.md` rule and the std file and line it came from). Add a
`README.md` there: a table of construct, rule, std files affected,
count. Don't edit `compiler/crates/`. Timebox 30 minutes; push.

### Q7. Profile The Foundation Commit

Run P1 on b4c14e39 (the architecture foundation): the samples, your
`compiler/bench/` inputs, and the stage driver on `lib/std`
(`cargo run --release -p hd_driver --example stages`). The bench is now
`cargo run --release -p hd_driver --example bench N` (not `hd bench`).
Same report format as P1. Timebox 30 minutes; push.

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

### C3. Research: Known Issues Of Prior Back Ends, Wasm And Runtimes (Last)

Documents only. Continue `future-work/compiler/prior-art-issues.md` with
"Part B: Back Ends, Wasm And Runtimes" (replace its placeholder), in the
style of Part A: per implementation, its choices, its documented problems
with links, and whether our design (`codegen.md`, `wasm-layout.md`,
`suspension.md`, `runtime-and-host.md`, `engines-and-test-runner.md`)
avoids, inherits or ignores each. Cover MoonBit, dart2wasm, Kotlin/Wasm,
wasm_of_ocaml, the Scala.js Wasm backend, Guile Hoot, AssemblyScript,
Grain, Go's Wasm target, rustc_codegen_cranelift, Koka/Effekt/OCaml 5
effect compilation, and wasmtime's GC. Add a ranked "Lessons for hd"
list and a "Changes suggested" list for the design files (don't edit
those files). Run `bash spec/check.sh` (it checks links) before pushing.

## Questions

(none open; the C1 and C2 questions are answered in C2c)
