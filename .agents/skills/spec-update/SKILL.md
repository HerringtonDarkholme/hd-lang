---
name: spec-update
description: Apply owner decisions that are decided but not yet applied, end to end - spec text in the spec/STYLE.md format with rule IDs, in the language tier or the stdlib tier (spec/std/), README diagnostics, conformance fixtures and indexes, a rule inventory diff, the toy prototype in src/ or a KNOWN_FAILURES.tsv entry, known-issues and open-issues cleanup, guide and website examples, the full check suite, and a fast-forward push. Use when the owner says a decision is final and should go into the specification. Anything ambiguous goes back to the owner as a question.
---

# Spec Update

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

## Purpose

Turn a recorded owner decision into accepted language behavior: the spec
says it, fixtures test it, the prototype does it or records the gap, and
nothing else in the repo contradicts it. This is the one skill that edits
the spec, and it adds exactly what the decision says, no more.

## When To Use

- The owner states that a decision is final and asks for it to be applied.
- A decision is recorded as decided but not yet applied.
- Not for a decision still being discussed: ask the owner, or use
  `brainstorm` or `stress-test`.

## Inputs

1. The decision IDs, for example `EQ-1` or "FN_TYPE Q2".
2. Where each is recorded. Look in:
   - the **Owner Decisions** section of a `future-work/*.md` record;
   - the commit messages, the record of what is already applied
     (`git log --grep '<decision ID>'`). A decision a commit applied is
     done; do not apply it twice. OPEN_ISSUES holds only open questions.

## Method

1. **Read the decision and its scope.** Quote the decision text. List every
   spec section, diagnostic, fixture, and example it touches (`grep` the
   spec, including `spec/std/`, `spec/conformance/`, `guide/`, `website/` (including
   `website/playground/`), `src/`, and `test/`). If the text leaves a rule
   open, stop and ask the owner.
   Never fill a gap with a default.
2. **Work in a worktree** on a new branch from `origin/main`, and keep the
   main checkout untouched.
3. **Choose the tier** of each new rule, by the tier test in AGENTS.md
   ("Spec Scope For The Standard Library") and
   [spec/std/README.md](../../../spec/std/README.md#the-tier-test):
   - **Language tier**, a numbered chapter: syntax, semantics, intrinsics,
     lang items, prelude names, test registration the compiler checks, the
     conformance harness, and anything a diagnostic code names.
   - **Stdlib tier**, `spec/std/<module>.md`: a std API that `lib/std` can
     write in ordinary hd over language-tier items only.
   - **CLI tier**, `spec/cli/command-line.md`: what an `hd` command does, the
     `hd.toml` executables, and package tasks.
   - A rule in a section that no migration task has moved yet goes next to
     its neighbors in the numbered chapter. Moving a section is a
     migration task of its own, not part of a decision.
   - A decision that splits across tiers gets rules in both. When the tier
     is unclear, ask the owner.
4. **Write the spec text** per `spec/STYLE.md`:
   - one rule per sentence, each with a rule ID that uses the chapter
     prefix: the numbered chapter's prefix, or `std-<module>` in
     `spec/std/<module>.md`;
   - no language-tier rule depends on `spec/std/`: a numbered chapter
     links there only from a Note or See also, never from a numbered rule,
     and a language-tier example uses no stdlib-tier item;
   - a stdlib chapter may cite any language rule;
   - a rule whose meaning changes gets a new ID, and the old ID is retired:
     delete it, and do not list it anywhere, not in `spec/STYLE.md` either;
     the commit message names it with its replacement;
   - never rename a heading or anchor; only a Spec Tiers move task deletes
     a moved heading, and it fixes every link to it in the same commit;
   - an error example for each new diagnostic, and a Why callout for the
     rationale.

   **A new stdlib chapter file** needs, in the same commit:
   - an entry in `STD_CHAPTERS` in `website/src/pages.ts`, such as
     `["testing", "Testing"]`;
   - a link from its row in the chapter table of `spec/std/README.md`;
   - for a module not yet in that table, a new row there, a prefix row in
     `spec/STYLE.md`, and a key in `CHAPTER_PREFIXES` in
     `spec/tools/spec-prose.ts`.
5. **Update `spec/README.md`.** Add any new diagnostic code to the
   Diagnostics table and remove one the decision withdraws. A diagnostic
   stays in this table even when its rule is stdlib-tier. Add no history:
   the commit message records what a pass applied, per decision with its
   tier and what changed for existing source; OPEN_ISSUES holds only open
   questions.
6. **Update conformance.** Add or change fixtures under
   `spec/conformance/` in the format its README defines. Keep
   `cases.tsv` and `examples.tsv` consistent: one `cases.tsv` row per
   fixture, one `examples.tsv` row per `text` fence. Fixtures stay
   implementation-neutral. The spec owns their format; never shape one to
   the prototype.
   - A fixture's tier is the tier of its `specification` column: a path
     under `std/` is stdlib, any other is language. There is no other
     tier marker.
   - Cite the section the fixture tests. A language-tier fixture uses only
     language-tier std items; a stdlib fixture may use both tiers.
   - A stdlib chapter's examples get `examples.tsv` rows keyed
     `std/<module>.md`.
7. **Diff the rule inventory** of each changed chapter against `main`:

   ```sh
   node --experimental-strip-types spec/tools/rule-inventory.ts --diff --all \
       main:spec/<chapter>.md spec/<chapter>.md --out /tmp/<chapter>-diff.md
   ```

   `<chapter>` is `lang/<NN-name>` for a language chapter, `std/<module>`
   for a stdlib chapter, and `cli/command-line` for the CLI chapter. A new
   chapter has no `main` version to diff; list its rule IDs in the commit
   message.

   Nothing may be lost except what the decision removes. Explain every
   lost code, example, or rule ID in the commit message. A retired ID shows
   up as lost. The diff fails when an added ID appears in the history of
   the numbered or stdlib chapters: that ID was retired before, so choose
   another.
8. **Update the prototype.** Implement the decision in `src/` with tests.
   When it is too large for this change, add the new fixtures to
   `test/portable/KNOWN_FAILURES.tsv` (`path`, `reason`, and the decision
   or finding ID), as the existing rows do. Never change the spec to match
   the prototype.
9. **Clean up the issue lists.** Remove the decision's question from
   `future-work/OPEN_ISSUES.md`. In `src/KNOWN_ISSUES.md`, delete a finding
   only when it no longer reproduces and a `grep` of the repo shows no other
   reference. Recount its known-failure tag table from
   `KNOWN_FAILURES.tsv`, and list an applied decision the prototype does not
   follow yet.
10. **Update everything that shows the behavior.** Examples and prose in
   `guide/`, the website, the playground's examples, and `future-work/`
   design records (mark the decision applied, with a spec link).
11. **Run the checks the change needs**, and fix what fails. Pick by what
    the diff touches:

    | The change touches | Run |
    | --- | --- |
    | only `spec/`, `guide/`, `future-work/`, fixtures, or indexes | `bash spec/check.sh`, `node --experimental-strip-types test/run-portable.ts --changed`, and `pnpm run website:build` |
    | also `src/`, `lib/std/`, `test/`, or `bin/` | `pnpm run check` and `pnpm run website:build` |
    | the website or playground code | also `pnpm run website:e2e` |

    `--changed` runs only the conformance cases whose fixture differs from
    `origin/main`, so a spec-only pass never reruns the whole suite. While
    iterating on prototype code, run one phase
    (`test/run-portable.ts --phase parse|type|runtime`); run the full
    `pnpm run check` once before the push, not after every edit or rebase
    of unrelated files.

12. **Integrate.** Make logical commits (spec and fixtures, prototype,
    issue-list cleanup, docs), each message naming the decision and the rule
    anchors. Rebase on `origin/main`, rerun the checks if the rebase touched the same files,
    and push as a fast-forward. Never force-push.

## Output

- Commits on `main` with the spec, README, fixtures, indexes, prototype or
  `KNOWN_FAILURES.tsv`, known and open issues, and docs changes.
- A final message to the caller: the decisions applied, the new and
  retired rule IDs, the diagnostics added or removed, the conformance
  counts (passing out of total), anything recorded as a known failure, the
  commit hashes, and every question raised for the owner.

## Hard Rules

- Apply exactly the decision. A related improvement becomes an owner
  question or an `OPEN_ISSUES.md` entry, not an edit.
- Ambiguity goes back to the owner, never a guess.
- No rule, diagnostic, or example disappears unless the decision removes
  it, and the inventory diff proves it.
- The spec leads and the prototype follows.
- No language-tier rule, example, or fixture depends on `spec/std/`.
- Never rename headings or anchors, and never reuse a retired rule ID.

## Done When

- The decision is in the spec, in the tier the tier test gives, with rule
  IDs; named in the commit message; and out of `OPEN_ISSUES.md`.
- Fixtures cover each new rule and diagnostic, and both indexes agree.
- The prototype implements it, or `KNOWN_FAILURES.tsv` records the gap and
  the tag table in `src/KNOWN_ISSUES.md` is recounted.
- All checks in step 11 pass, and the commits are pushed as a
  fast-forward.
