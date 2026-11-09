# Shared Rules For hd Agent Skills

Every skill in this folder follows these rules. Read this file before
starting, and reread it before committing.

## Who Decides

1. The owner decides every design question. An agent researches,
   stress-tests, compares, and asks. It never picks an option on the owner's
   behalf, and never records a decision the owner did not state. Only the
   `spec-update` skill edits a spec chapter, numbered or in `spec/std/`,
   and only to apply a decision the owner made.
   The other skills write reports and questions.
2. A recommendation is allowed only where the skill says so, and it is
   labeled **Recommendation** and kept apart from the facts.
3. Fix-style work adds no features. When a fix would need a design choice,
   the choice becomes an owner question (in the report, or in
   [issue #833](https://github.com/HerringtonDarkholme/hd-lang/issues/833)), never a
   default to implement.
4. Settle core design before advanced features. Do not ask about an
   advanced feature while the core it depends on is still open; say that it
   waits.
5. Respect recorded owner decisions. Do not reopen an alternative a design
   record already rejected unless new evidence shows the rejection's reason
   no longer holds; then say which evidence.

## Owner Principles To Respect

These come from recorded owner decisions. Check the current design records
before relying on one, because they change.

- hd is written mostly by coding agents and read by humans: explicitness
  over inference where it adds a guarantee, and no action at a distance
  ([docs/future-work/README.md](../../docs/future-work/README.md#direction)).
- Fewer mechanisms beat more: reuse an existing rule before adding one. Past
  cuts: `mut fn` removed (closures mutate captures freely), errors derived by
  the one `@error` intrinsic instead of derivation machinery, no marker templates,
  explicit `@from` and `@source` instead of inferred `From` and cause.
- Derivation is written as one block per concern. Never propose merging all
  derivations at the declaration.
- Resource safety is planned through a NonEscapable-style trait, and
  `defer` stays. Do not propose `Drop` or `using` as the answer.
- Per-tool authority reports are application-level. Do not raise them as a
  language next step.
- Follow prior art's best practice by default: the behavior of the
  established reference (serde and serde_json for serialization, Go
  net/http and reqwest for HTTP, Rust std for collections). A design that
  differs states the difference, the anti case where the prior art fails
  or doesn't fit hd, and the trade-off. An undiscussed difference is a
  review finding.

## Cost Model

Anything that runs on every command (a cache, the driver's warm path, a
checker, a gate, a script in `pnpm run check`) exists to be fast; a slow one
is wrong, whatever else it guarantees. Owner, 2026-11-08: "having a fast
rebuild is what a cache for, you give me a slow cache and claim it is
correct??"

- Every design and every change on such a path states its cost per run as a
  function of the input or change, and of the total accumulated state (cache
  entries, index rows, packs, source files, examples, history).
- Work proportional to total state on every run is a defect, found by
  reading the loops and call sites, not by measuring afterwards: a
  `read_dir` of the whole store, a whole-file read or rewrite to change a
  few records, one process per item, a check repeated by the next stage.
- A warm run with nothing changed does work proportional to nothing. A run
  that changes k things does work proportional to k.
- Where state can grow, a test grows it tenfold and asserts the run cost
  stays flat.
- A compiler pass that retries, rechecks or iterates to a fixpoint states its
  bound in both **width** (items in a group, ring length) and **depth**
  (nesting), and ships a generated test at width 1000 and depth 20 that
  counts the work (bodies checked, iterations), not the time. #1512's body
  retry was exponential in nesting depth (#1529) and its ring propagation
  overflowed the stack at 400 (#1530). Both passed a width-only cost test.
  Owner, 2026-11-08: "probably it is quadratic", then "bad impl".

## Spec Tiers

The specification has three tiers; AGENTS.md "Spec Scope For The Standard
Library" states the tier test and where each kind of rule goes.

- The **language tier** is the numbered chapters `spec/lang/01-*.md` to
  `spec/lang/14-*.md`: syntax, semantics, intrinsics, and anything the compiler
  knows by name.
- The **stdlib tier** is [spec/std/](../../spec/std/README.md), one file
  per std module, with rule IDs `std-<module>.*`: std APIs that `lib/std`
  can write in plain hd over the language tier.
- The **CLI tier** is [spec/cli/](../../spec/cli/README.md), with rule IDs
  `cli.*`: what the `hd` command does, such as package mode, `hd run`,
  tasks, and the REPL.
- Search every tier when you list the spec text an area depends on.
- When a report proposes a new rule or std item, name its tier by the test.
  A core library addition, the cheapest change in AGENTS.md "Design Cost
  Order", is a stdlib-tier item.
- A language-tier rule, example, or fixture never depends on a
  stdlib-tier item. Undecided std design stays in
  [issue #833](https://github.com/HerringtonDarkholme/hd-lang/issues/833).

## Writing

- Follow [spec/STYLE.md](../../spec/STYLE.md) for any spec text: one rule per
  sentence, rule IDs, error examples, Why callouts. Never rename an existing
  heading or anchor in `spec/`, `docs/future-work/`, or `website/guide/`: the
  website, the anchor checker, and fixtures link to them. The one
  exception is a Spec Tiers move task, which deletes a moved heading and
  fixes every link to it.
- Design records use the same prose targets: paragraphs of at most four
  sentences, sentences of at most 25 words, enumerations in tables.
- Open every report with a status line that says nothing in it is accepted
  behavior, and name the exact records, decisions, and spec sections under
  review, with links.
- One idea per owner question. Each question states the effect first, then
  one to three candidate answers, then a short hd example.
- Compare with other languages when it helps the owner decide, and cite a
  source (official docs, RFCs, or the library's source) for each claim.

## Examples

- Keep examples short: the smallest code that shows the point.
- Write hd examples in `text` fences and check each one with the compiler's
  parser: `scripts/hd-conformance.sh parse FILE` from the repo root (it
  builds the parser when missing). A line that uses syntax no chapter specifies ends in
  `# hypothetical syntax`. Record every result in a Parse Log.
- The compiler lags the spec in places. When it rejects syntax that a
  chapter specifies, cite the chapter and say so in the Parse Log.
- Code in other languages goes in its own fence (`rust`, `swift`, ...) and
  is not parsed.
- Parsing checks syntax only. Say so; do not claim a block type-checks.

To parse every `text` block of a Markdown file, from the repo root:

```sh
node --experimental-strip-types --input-type=module -e '
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
const dir = mkdtempSync(join(tmpdir(), "hd-blocks-"));
const blocks = [...readFileSync(process.argv[1], "utf8").matchAll(/^```text\n([\s\S]*?)^```/gm)];
blocks.forEach((m, i) => {
  const file = join(dir, `block-${i + 1}.hd`);
  writeFileSync(file, m[1]);
  let result = "parse";
  try {
    execFileSync("scripts/hd-conformance.sh", ["parse", file], { stdio: "pipe" });
  } catch (error) {
    result = `${error.stdout}${error.stderr}`.trim().replaceAll(`${file}:`, "line ").replaceAll("\n", "; ");
  }
  console.log(`block ${i + 1}: ${result}`);
});
' docs/future-work/REPORT.md
```

## Tests And Fixtures

- Fixtures, fuzzers, and test tooling stay implementation-neutral. The spec
  owns the fixture format ([spec/conformance](../../spec/conformance/README.md)).
  Never shape a fixture to the compiler's behavior.

## Git And Safety

- Stage specific paths only. Never `git add -A`, `git add .`, or
  `git commit -a`. Never stage `.claude/worktrees/`.
- Never run a bare `git stash`, never force-push, and never rewrite pushed
  history.
- Commit only when the task or the skill asks for it. End the message with
  the `Co-Authored-By:` line the session gives you.
- Before a push that touches Markdown, run `bash spec/check.sh` and
  `pnpm run website:build`.
- Run only the checks a change needs. A spec-, docs-, or fixture-only
  change needs `bash spec/check.sh` and `pnpm run website:build`, not the
  full `pnpm run check`. A change to `crates/` or `lib/std/` adds
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test`. A change to `crates/`, `lib/std/`, `host/` or `website/`
  also runs `pnpm run test:web-compiler` (it has a size gate), and one to
  `host/` or `website/` runs `pnpm run test:ui`. A change to an example
  in a chapter or a doc also runs `HD_CONFORMANCE_ONLY=<file> cargo test -p
  hd_driver --test conformance -- --exact
  examples::spec_and_doc_examples_check_as_printed`.
- Nobody runs the full `pnpm run check` locally; CI runs the full suite.
  The orchestrator merges directly in the main checkout and runs the checks
  the change needs.
- Never wait for a check with an `until` or `while … sleep` loop. Run it in
  the foreground with a timeout, or in the background and wait for its
  completion notification. Never wait on CI; the orchestrator checks CI at
  its regular summaries and fixes a red run first.
- Background agents run one at a time. Don't start a full check while
  another agent's full check runs, unless the owner asks for parallel runs.
- Before any check, run `git fetch origin` and rebase on `origin/main`. A
  stale `origin/main` gets the push rejected anyway.

## Writing hd Code

When the task is writing hd programs, follow AGENTS.md "Writing hd Code:
Model Choice And A Feedback Log". Examples: `lib/std`, examples,
playground code, and hd test files. It does not apply to spec text,
fixtures, or the compiler. Log every syntax, type, API or semantic mistake in
`docs/audit/hd-writing-log.md`.

