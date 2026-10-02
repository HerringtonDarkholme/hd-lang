# Shared Rules For hd Agent Skills

Every skill in this folder follows these rules. Read this file before
starting, and reread it before committing.

## Who Decides

1. The owner decides every design question. An agent researches,
   stress-tests, compares, and asks. It never picks an option on the owner's
   behalf, and never records a decision the owner did not state. Only the
   `spec-update` skill edits a spec chapter, numbered or in `spec/std/`, or
   the prototype in `src/`, and only to apply a decision the owner made.
   The other skills write reports and questions.
2. A recommendation is allowed only where the skill says so, and it is
   labeled **Recommendation** and kept apart from the facts.
3. Fix-style work adds no features. When a fix would need a design choice,
   the choice becomes an owner question (in the report, or in
   [future-work/OPEN_ISSUES.md](../../future-work/OPEN_ISSUES.md)), never a
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
  ([Roadmap](../../future-work/ROADMAP.md#direction)).
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
  [future-work/OPEN_ISSUES.md](../../future-work/OPEN_ISSUES.md).

## Writing

- Follow [spec/STYLE.md](../../spec/STYLE.md) for any spec text: one rule per
  sentence, rule IDs, error examples, Why callouts. Never rename an existing
  heading or anchor in `spec/`, `future-work/`, or `guide/`: the
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
- Write hd examples in `text` fences and check each one with the compiler:
  `hd debug parse FILE`, which is
  `node --experimental-strip-types bin/hd.js debug parse FILE` from the repo
  root. A line that uses syntax no chapter specifies ends in
  `# hypothetical syntax`. Record every result in a Parse Log.
- The compiler is a toy and lags the spec. When it rejects syntax that a
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
    execFileSync("node", ["--experimental-strip-types", "bin/hd.js", "debug", "parse", file], { stdio: "pipe" });
  } catch (error) {
    result = `${error.stdout}${error.stderr}`.trim().replaceAll(`${file}:`, "line ").replaceAll("\n", "; ");
  }
  console.log(`block ${i + 1}: ${result}`);
});
' future-work/REPORT.md
```

## Tests And Fixtures

- Fixtures, fuzzers, and test tooling stay implementation-neutral. The spec
  owns the fixture format ([spec/conformance](../../spec/conformance/README.md)).
  The prototype in `src/` is a toy; never shape a fixture to its behavior.

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
  change needs `bash spec/check.sh`, `node --experimental-strip-types
  test/run-portable.ts --changed`, and `pnpm run website:build`, not the full
  `pnpm run check`. The full check is for changes to `src/`, `lib/std/`,
  `test/`, or `bin/`, and runs once before the push.
- Never read, print, or copy credential files: `~/.npmrc`, `~/.netrc`,
  `~/.git-credentials`, `~/.ssh/*`, `.env` files, `~/.config/gh/*`, or any
  token store.

## Writing hd Code

When the task is writing hd programs, follow AGENTS.md "Writing hd Code:
Model Choice And A Feedback Log". Examples: `lib/std`, examples,
playground code, and hd test files. It does not apply to spec text,
fixtures, or `src/`. Log every syntax, type, API or semantic mistake in
`audit/hd-writing-log.md`.

