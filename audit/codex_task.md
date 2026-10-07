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

- `spec/`, `lib/std/`, `guide/`, `website/` and `src/`: the orchestrator's
  spec passes are rewriting them now (S1d: `dyn Trait`; S1e: removing
  GADTs). Read them; don't edit them. `src/` is the frozen prototype.
- `future-work/compiler/*.md` except where a job says so.

## Jobs

### C2a. Review Fixes For C1 (b224b04c)

Review of C1 (tests and clippy pass; 7 tests):

1. Non-blocking. A non-UTF-8 corpus file silently skips the round-trip
   check. Assert every corpus file is UTF-8, or compare bytes.
2. Non-blocking. `hd_syntax` has 4 unit tests. Add focused tests for
   `"""` strings, raw strings, nested `${...}` interpolation, number
   forms, bracket continuation, comment-only lines and tabs, and the
   error token for an unterminated string.
3. Non-blocking. Skim runs at 46.7 MB/s, slower than lexing alone
   (116.3 MB/s). Skim should cost no more than lexing. Profile it and
   fix, or explain the cost in the commit message.
4. Speed targets (release build, `lib/std`): lexing at least 300 MB/s,
   skimming at least as fast as lexing, and (in C2) a full parse to the
   green tree at least 100 MB/s. Keep the throughput example as a
   benchmark, report all three numbers in each commit message, and treat
   a drop of more than 10% as a regression to explain.
### C2c. A Complete Parser (Before C3)

Owner, 2026-10-07: "the parser isn't full and the parse speed is slow".
C2's parser is structural: it accepts 98 of 99 valid parse cases but
matches only 96 of 172 invalid ones. Slice 1's exit test is all of them.

- Every grammar production in `spec/lang/02-grammar.md` (and the forms
  the other chapters add) gets a green-tree node kind and a typed view:
  items, statements, expressions (with precedence), patterns, types
  (`dyn`, `mut`, optionals, tuples, functions), annotations, `tests:`
  blocks, `with`, pipes, string interpolation.
- Every `spec/conformance/parse/` case passes with its expected
  diagnostic code. `compiler/KNOWN_FAILURES.tsv` shrinks to zero, apart
  from rows tagged as waiting on S1e (GADT removal). Error recovery must
  still report every independent error once.
- Then speed (C2a item 4): profile and reach lexing at least 300 MB/s,
  skimming at least as fast as lexing, and parsing at least 100 MB/s on
  `lib/std` in release. Report all three in the commit message.
- Answer to your Questions: prioritize all diagnostic families; zero
  ledger rows is the exit. The `test:ui` Backspace failure is specific to
  your environment (it passes 85/85 in every merge run today); don't
  chase it.

### C3. Research: Known Issues Of Prior Back Ends, Wasm And Runtimes

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
