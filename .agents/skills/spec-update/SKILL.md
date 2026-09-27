---
name: spec-update
description: Apply owner decisions that are decided but not yet applied, end to end - spec text in the spec/STYLE.md format with rule IDs, README diagnostics and Revision Notes, conformance fixtures and indexes, a rule inventory diff, the toy prototype in src/ or a KNOWN_FAILURES.tsv entry, audit cleanup, guide and website examples, the full check suite, and a fast-forward push. Use when the owner says a decision is final and should go into the specification. Anything ambiguous goes back to the owner as a question.
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
- A decision is listed under "Decided, Not Yet Applied".
- Not for a decision still being discussed: ask the owner, or use
  `brainstorm` or `stress-test`.

## Inputs

1. The decision IDs, for example `EQ-1` or "FN_TYPE Q2".
2. Where each is recorded. Look in:
   - `audit/types/QUESTIONS.md`, section "Decided, Not Yet Applied";
   - the **Owner Decisions** section of a `future-work/*.md` record;
   - `spec/README.md` Revision Notes, the record of what is already
     applied. A decision listed there is done; do not apply it twice.

## Method

1. **Read the decision and its scope.** Quote the decision text. List every
   spec section, diagnostic, fixture, and example it touches (`grep` the
   spec, `spec/conformance/`, `guide/`, `website/`, `playground/`, `src/`,
   and `test/`). If the text leaves a rule open, stop and ask the owner.
   Never fill a gap with a default.
2. **Work in a worktree** on a new branch from `origin/main`, and keep the
   main checkout untouched.
3. **Write the spec text** per `spec/STYLE.md`:
   - one rule per sentence, each with a rule ID that uses the chapter
     prefix;
   - a rule whose meaning changes gets a new ID, and the old ID is retired
     under Retired Rule IDs with the date and replacement;
   - never rename a heading or anchor;
   - an error example for each new diagnostic, and a Why callout for the
     rationale.
4. **Update `spec/README.md`.** Add any new diagnostic code to the
   Diagnostics table and remove one the decision withdraws. Add one
   Revision Notes entry per decision: "Name (owner decision, YYYY-MM-DD):"
   and what changed for existing source.
5. **Update conformance.** Add or change fixtures under
   `spec/conformance/` in the format its README defines. Keep
   `cases.tsv` and `examples.tsv` consistent: one `cases.tsv` row per
   fixture, one `examples.tsv` row per `text` fence. Fixtures stay
   implementation-neutral. The spec owns their format; never shape one to
   the prototype.
6. **Diff the rule inventory** of each changed chapter against `main`:

   ```sh
   node --experimental-strip-types spec/tools/rule-inventory.ts --diff --all \
       main:spec/<chapter>.md spec/<chapter>.md --out /tmp/<chapter>-diff.md
   ```

   Nothing may be lost except what the decision removes. Explain every
   lost code, example, or rule ID in the commit message.
7. **Update the prototype.** Implement the decision in `src/` with tests.
   When it is too large for this change, add the new fixtures to
   `test/portable/KNOWN_FAILURES.tsv` (`path`, `reason`, and the decision
   or finding ID), as the existing rows do. Never change the spec to match
   the prototype.
8. **Clean up the audit.** Move the decision out of "Decided, Not Yet
   Applied" in `audit/types/QUESTIONS.md`. Delete a finding or its evidence
   only when it no longer reproduces and a `grep` of the repo shows no other
   reference. Recount `audit/evidence/w9/failures-by-id.tsv` from
   `KNOWN_FAILURES.tsv`, and refresh the audit index files that cite it.
9. **Update everything that shows the behavior.** Examples and prose in
   `guide/`, the website, the playground's examples, and `future-work/`
   design records (mark the decision applied, with a spec link).
10. **Run every check** and fix what fails:

    ```sh
    bash spec/check.sh
    HD_TEST_JOBS=4 HD_SPEC_JOBS=4 npm run check
    npm run website:build
    npm run website:e2e
    npm run playground:build
    npm run playground:e2e
    ```

11. **Integrate.** Make logical commits (spec and fixtures, prototype,
    audit cleanup, docs), each message naming the decision and the rule
    anchors. Rebase on `origin/main`, rerun the checks if anything moved,
    and push as a fast-forward. Never force-push.

## Output

- Commits on `main` with the spec, README, fixtures, indexes, prototype or
  `KNOWN_FAILURES.tsv`, audit, and docs changes.
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
- Never rename headings or anchors, and never reuse a retired rule ID.

## Done When

- The decision is in the spec with rule IDs, in the README Revision Notes,
  and out of "Decided, Not Yet Applied".
- Fixtures cover each new rule and diagnostic, and both indexes agree.
- The prototype implements it, or `KNOWN_FAILURES.tsv` records the gap and
  `failures-by-id.tsv` is recounted.
- All checks in step 10 pass, and the commits are pushed as a
  fast-forward.
