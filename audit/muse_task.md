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

### BD. Hints From Usability Probe 2

Probe 2 (Haiku, rows dated 2026-10-05 "usability probe 2" in
`audit/hd-writing-log.md`) wrote three real programs. Its two worst
problems had unhelpful messages. Message and docs only; no rule changes.

1. **Importing the package under test.** In `tests/integration.hd`,
   `use self.{Item, total_value}` gives `unknown-module: no package module
   'tests'`, and the probe gave up and copied the code into the test.
   The right form is `use pkg.{Item, total_value}`, which the file that
   `hd new` generates already uses. In an integration test module, when a
   `self` (or bare) path doesn't resolve, add: "an integration test sees
   the package as a dependent does: write `use pkg.{Item, total_value}`".
   Also make the base message name the real path instead of 'tests'.
2. **`?` in a function that returns `void`.** `main!() -> void` using
   `write_line!(...)?` gives `invalid-result-propagation: Result
   propagation requires a function with a compatible Result error type`.
   Add the fix: "change the result type to `Result[void, ConsoleError]`
   (the error type of `write_line!`)", naming the actual error type, or
   suggest `println` when the call is `Console` output.
3. **Guide.** `guide/LANGUAGE_TOUR.md` (and `guide/LEARN_IN_10_MINUTES.md`
   if it covers tests): in the testing section, show an integration test
   that imports the package with `use pkg.{...}`, reads a fixture through
   its relative path (tests run from the package root), and has its cases
   at the top level, not in a `tests:` block. Keep examples realistic.

Add a test per message.

### BB. Messages Never Show Hidden Names

Found while reviewing AT. A message printed the linker's hidden name:

```
use std.testing.arbitrary

fn f(x: arbitrary.With) -> usize:
    0
```

gives `partial-generic-arguments: '__std_testing_arbitrary_With' needs a
type argument for 'F'`. The user wrote `arbitrary.With`. Messages, hints,
fix-its, the REPL, and `hd doc` must show the name as the user can write
it (`arbitrary.With`, or the qualified `std.testing.arbitrary.With` when
no import names it), never `__std_*` or `__pkg_*`. Fix it once, where
types and names are displayed (`displayType` and the diagnostic message
path), not per message. Then sweep: grep the conformance expectations and
a run of `--phase type` output for `__std_` and `__pkg_`, fix what turns
up, and add a test that fails if any diagnostic text contains them.

### BC. A Leading `_` Must Silence The Unused Warning

`flow.unused.underscore` (spec/lang/06-control-flow.md) says names
beginning with `_` suppress `unused-local-binding`. The prototype ignores
it, and has since at least e0401f5b:

```
pub fn main() -> void $ Console:
    _g := 5          # warning: unused-local-binding: local binding '_g' is never read
    let _f: fn(i32) -> i32 = twice   # same
    println("ok")
```

Fix the warning (every binding form: `:=`, `let`, pattern bindings,
parameters if they warn, loop variables), add a `typing/valid` fixture
with no expected warning for each form (and keep a plain unread name
warning), and find why no conformance case caught it: add the fixture
row for `flow.unused.underscore` if it is missing.

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

### AX. `it_each` At The Top Level Of An Integration Test

Found by #321. A top-level `it_each(...)` in `tests/x.hd` is rejected as
`misplaced-test-case`, but test position includes integration test
modules (see the testing rules in spec/lang/10-modules.md; `it(...)` there
works). Fix the checker so `it_each` is accepted wherever `it` is, add a
fixture for an integration module using `it_each`, and check the other
test-case forms (property tests) the same way.

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
