# Muse Task Queue

This file is Muse's work queue. The orchestrating session adds jobs here.
Muse does them top to bottom, one commit per job, and deletes a job's
section in the same commit that finishes it. Git history keeps the record.
When the queue is empty, report that and wait.

## How To Work

- Work in your own git worktree, never in the shared folder
  `/Users/hd/code/test/hd-lang`.
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
  `spec/conformance/examples.tsv`, not only the new row. Job AN missed
  this and broke `bash spec/check.sh` on main (fixed in c8c35a7e). After
  any rebase, rerun `bash spec/check.sh` before pushing; never push with
  it red.

## Don't Touch

- `spec/` (including fixture file names), unless a job says so.
- Test runner work (another agent is changing them): `lib/std/testing.hd`,
  `src/commands/test*`, `src/test-runner*`, and in `spec/`: the Testing
  sections of `lang/10-modules.md`, `std/testing.md`, and `hd test` in
  `cli/command-line.md`.

## Jobs

### AP. Hints From The Usability Probe

The Haiku probe (rows dated 2026-10-05 in `audit/hd-writing-log.md`) hit
messages that were right but didn't say the fix. Add a hint (a note plus,
where the edit is mechanical, a fix-it) to each:

- `fn f(mut todos: List[Todo])` gives `syntax-error: expected a parameter
  name`. Say "write `todos: mut List[Todo]`; `mut` goes on the type".
- `loop:` gives `unknown-name: unknown function 'loop'` (two agents hit
  it). Say "hd has no `loop`; write `while true:`".
- `let todos = []` with no use that fixes it: the message already says to
  annotate. Check that the suggested annotation uses the element type when
  a later statement shows it, or a placeholder otherwise.
- `unknown-method` on a std type: list up to three similarly named methods
  that do exist (edit distance), as other "did you mean" hints do.

Message text only; no language rule changes. Add a test per hint.

### AQ. Supertrait Bounds Imply Their Supertraits

Job AL (d2fa2a71) now rejects code it used to accept. `Ord < PartialOrd <
Eq` in `lib/std/cmp.hd`, yet:

```
data Box[T < Eq]:
    value: T

data Good[T < Ord]:
    b: Box[T]      # unsatisfied-trait-bound: 'T' does not implement Eq

fn f[T < Ord](b: Box[T]) -> bool:   # same error
    b.value == b.value
```

`trait.bound.supertraits` (spec/lang/09-traits.md) says a bound implies its
supertraits. Fix the root, not the written-type path alone: one lookup
"does enclosing bound set B imply trait X" that walks supertraits, used by
declarations (`enclosingBoundImplies`), signatures (`signatureBoundScope`)
and the call checker. That should also clear the KNOWN_FAILURES row
`typing/valid/bound-implies-supertrait.hd` (TYPE-GAPS); move it. Add
tests for the two cases above plus a call site.

### AR. A User Type Named `T` Or `E` Breaks Every Program

```
data T:
    x: usize

pub fn main() -> void $ Console:
    println("done")
```

fails with `lib/std/cmp.hd:42:5: private-type-leak: public function
'__std_cmp_min' exposes private type or trait 'T'` (also `max`, `clamp`,
`debug`, `hash_of`). `enum E` breaks `lib/std/task.hd` `retry` the same
way. Std's generic parameters resolve against the user's top-level types.
Generic parameters must shadow outer type names in every scope (std or
user), so find where the private-type-leak check (or the signature
resolver feeding it, `src/checker/program-signatures.ts`,
`program-types.ts`) looks a parameter name up as a type, and fix it
there. Add a test: user types named `T`, `E`, `K`, `V` each compile and
run. Check user code too: `data T` plus `fn id[T](x: T) -> T` must use
the parameter.

### AS. Migrate `test/cli.test.ts` To The CLI Tier

Job AM marked all 16 `test/cli.test.ts` tests "not migrated: the portable
suite has no CLI tier". It has one: `spec/conformance/cli/` with
`spec/conformance/cli-cases.tsv` (36 cases, run by
`node --experimental-strip-types test/run-portable.ts --tier cli`; see
`spec/conformance/README.md` for the case format). Move each test whose
behavior a `cli.*` rule states (exit status, unknown command, `hd test`
verdicts and output, snapshots, timeouts) into a CLI case, delete it from
the TS file, and update its `test/MIGRATED.md` row. Tests that check
runner internals stay, with the reason.

### AT. Child-Path Error Names The Real Problem

Review of AN (ee489e28). Non-blocking.

```
use std.testing

pub fn main() -> void $ Console:
    x := testing.arbitrary.With   # unknown-name: unknown name 'testing'
```

The user did import `testing` (`testing.assert(...)` works), so "unknown
name 'testing'" sends them the wrong way. In a type position the fixture
gets `unknown-type`, with the same problem. Keep the codes, and make the
message say what happened: "'arbitrary' is a child module of
'std.testing', which a path can't reach through its parent; import it
with `use std.testing.arbitrary`" (for a package module, also mention that
the parent can `pub use` it). Cover value and type positions, std and
package modules. Add a test per case.

### J. Ongoing: Review New `src/` Commits

For each new commit on `origin/main` that touches `src/`, review the diff
against the spec rule IDs or known-failure tags its message cites. Add the
findings to `audit/job6-src-commit-review.md`, in this order: verdict
(Blocker / Non-blocking / Nit), effect, fix, file:line.

Large recent ones to review:

- the literal join model, 62a084a2 and 288105cf, against
  `types.literal.local.*` in `spec/lang/04-type-system.md`;
- overflow build modes, 65a84de1 and c05682dc, against `types.arith.*`.

## Questions

(none)
