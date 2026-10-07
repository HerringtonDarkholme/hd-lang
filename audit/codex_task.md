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

### C2. New Compiler, Slice 1b: Parser, Green Tree, `hd parse`

After C1. Same design files, plus `syntax.md` §4.4 (parser and green
tree), §4.5 (header extraction and the API text hash), §4.6 (item index),
`data-structures.md` §3.13 (green tree and its wire format) and
`live-execution.md` (how a REPL input is parsed).

- The hand-written resilient recursive-descent parser building the
  lossless flat green tree, typed views generated or written per the
  design, `dyn Trait` in type position, and a small `hd` binary (or a
  `hd_cli` crate) with `hd parse FILE` printing the tree or the
  diagnostics.
- Exit test (`build-order.md` slice 1): the parse-phase cases of
  `spec/conformance` pass (accept/reject and the diagnostic codes the
  fixtures expect; list any you cannot match under Questions); every
  fixture and std file round-trips byte for byte through the green tree;
  skim and full-parse skeletons agree on every file; a fuzz target
  (`cargo fuzz` or a property test) runs a fixed budget with no panic.
- Record parse throughput in the commit message.

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

- C1 validation: `pnpm run test:ui` consistently fails the existing test
  `hd repl in a terminal indents continuation lines but not pasted ones` because
  its simulated Backspace reaches the parser as DEL (`U+007F`). The other 84 UI
  tests pass, and C1 changes no REPL or TypeScript code. Should a later job fix
  this mainline terminal-test failure?
