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

## Jobs

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

### AW. Timestamp Serializes As RFC 3339 Text

Owner decision, 2026-10-05. `Timestamp`'s `Serialize`/`Deserialize`
(lib/std/time.hd, spec/std/time.md `std-time.serde.*`) write and read
RFC 3339 text in UTC with milliseconds, the same text its `Display` gives
(`"2026-10-05T15:50:48.076Z"`), instead of an integer of milliseconds.
`Duration` and `Instant` stay integers of milliseconds. Decoding accepts
what `std.time`'s RFC 3339 parser accepts and reports a bad string as a
decode error naming `Timestamp`. Update the rules, the `time-serde-millis`
fixture (rename it if its name no longer fits, and fix its rows), and add
a JSON round-trip test. Spec examples: realign `examples.tsv` if a block
moves.

Also, in the same job: since `dbg` landed (16df2295), the REPL prints
values the way `dbg` does, through `Debug` or else the structure, so a
`Timestamp` now shows as `Timestamp { millis: 1791225123833 }`. Give the
three time types hand-written `Debug` implementations (std calls are
ours; log them in `future-work/STDLIB_CALLS.md`): `Timestamp` writes its
RFC 3339 text, `Duration` its `Display` form (such as `250ms`), and
`Instant` its milliseconds as `Instant(…ms)`. Update
`test/cli-default-profile.test.ts` back to an RFC 3339 match and add a
REPL test.

### AX. `it_each` At The Top Level Of An Integration Test

Found by #321. A top-level `it_each(...)` in `tests/x.hd` is rejected as
`misplaced-test-case`, but test position includes integration test
modules (see the testing rules in spec/lang/10-modules.md; `it(...)` there
works). Fix the checker so `it_each` is accepted wherever `it` is, add a
fixture for an integration module using `it_each`, and check the other
test-case forms (property tests) the same way.

### AY. Follow-Ups From The Review Of AP (cc5b1679)

Both non-blocking. The four hints work as a user sees them.

1. **The intrinsic method list can drift.** `INTRINSIC_METHODS` in
   `src/checker/member-lookup.ts` hard-codes `List` and `Map` intrinsics
   (`len`, `iter`, `push`, `get`, `remove`), copying the built-in methods
   table. A method added to that table later is never suggested. Read the
   names from the same source the call checker uses for intrinsics
   (`src/checker/expression-calls.ts`), so there is one list.
2. **The empty-list message names the type it says it can't infer.**
   `let names = []` then `names.push("ada")` gives: cannot infer `T` in
   `List[string]`; annotate the binding: `let names: List[string] = ...`.
   Write instead: cannot infer the element type of `[]`; annotate the
   binding: `let names: List[string] = ...` (keep the placeholder
   `List[T]` when no later push shows the type). Update its test.

### AZ. `usize` Display Follow-Ups (F-611)

Task #325 (8e03da04) made `u32` the one type again and moved the `usize`
spelling into display-only data in `src/checker/spelling.ts`. Never change
type identity here; only what messages print.

1. These messages print `u32` where the source said `usize` (see F-611 in
   `src/KNOWN_ISSUES.md`): the expected type when it was written `usize`
   ("expected u32, found string" for a `usize` parameter); a field read of
   a field declared `usize`; the types listed by `no-common-type`; and the
   generic-inference conflict ("both solve T"). Route each through
   `spelling.ts`, add a test per message, and delete F-611 when done.
2. `xs := [1]` then `take_i32(xs.len())` adds the note "'xs' has usize in
   its type… write '+1'". That is wrong: `len()` is a size whatever the
   list holds, so no edit to `xs` helps. Give no literal hint when the
   mismatched value is a length (or any value whose type does not come
   from the named literal). Add a test.

### BA. Defer The Dead-Fact Warnings

Owner decision, 2026-10-05: defer the `unused-derivation-fact` warnings
that guess a fact's readers from the package that defines the fact type.
Deciding whether a fact is dead needs non-local knowledge of every
template the type derives. For this job you may edit
`spec/lang/14-annotations.md` (Facts section), fixtures, and indexes.

1. Remove the rules `annot.fact.unused-non-std`, `annot.fact.unused-std`,
   `annot.fact.unused-self-line`, and
   `annot.fact.unused-self-line.per-trait`, and the checker code behind
   them. Keep `annot.fact.unused-block-decorator`, which is local.
2. Fixtures that expect those warnings (for example
   `typing/warnings/per-trait-self-line-unused-fact.hd`) become cases
   with no warning, or are deleted if they test nothing else; update
   `cases.tsv`, `test/portable/cases.tsv`, `examples.tsv` (realign the
   chapter's rows if a block moves), and any KNOWN rows that name them.
   Keep `unused-derivation-fact` in the Diagnostics table while the
   block-decorator rule uses it.
3. **Park the removed rules; the owner wants to revisit them.** In
   `future-work/OPEN_ISSUES.md`, add a section "Parked: Dead-Fact
   Warnings" that quotes the four removed rules verbatim with their IDs,
   names the commit that removed them and the fixtures it deleted or
   changed (so `git show` can restore them), and then explains the
   deferral below.
   Also add to that section: dead facts are worth a warning;
   the accurate rule is read-set based (warn when no template the type
   derives, or for a `Self` line the block's template, reads facts of
   that type, using the fact types each template looks up with
   `find::[F]`; a template that reads facts dynamically counts as reading
   all). Revisit when the checker can see templates' read sets across
   packages, such as from a dependency's checked interface. Note the
   false positive that motivated deferring it: a shared vocabulary
   package (one that only defines fact types, like a `Label` read by
   both a `Form` and a `Grid` template) warned wrongly.
4. Run `bash spec/check.sh` and the retired-ID check; no rule may still
   cite a removed ID.

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
