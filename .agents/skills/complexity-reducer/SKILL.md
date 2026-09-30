---
name: complexity-reducer
description: Look for things to remove or merge in an hd design record or spec area - special cases, redundant markers or rules, concepts that serve one use case, mechanisms an existing one could absorb - and write a ranked list of cuts with before/after examples, costs, soundness checks, and rule accounting, as questions for the owner. Use when the owner asks to simplify an area, or after a design grows new markers, forms, or exceptions. Never applies a cut itself.
---

# Complexity Reducer

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

## Purpose

Make hd smaller without making it weaker. Each proposed cut removes or
merges something, names the existing rule that absorbs it, shows that no
bad program becomes valid, and accounts for every rule it touches. The
owner decides which cuts to take.

The owner's past cuts show the kind of result wanted:

| Cut | What absorbed it |
| --- | --- |
| `mut fn` and the readonly-capture rule | closures mutate their captures freely, as in Swift, Kotlin, and Go |
| `-` in requirement rows | extension of an existing row |
| error derivation through the general machinery | one intrinsic, `@derive(Error)` |
| bodiless marker templates | none: a template must have a body |
| inferred `From` and cause | explicit `@from` and `@source`, as in thiserror |

## When To Use

- The owner asks to simplify a chapter, a design record, or a feature.
- A design gained new markers, spellings, or exceptions in a recent round.
- Not for evaluating a decision set against real code: use `stress-test`.
- Not for choosing among designs for an open question: use `brainstorm`.

## Inputs

1. The area: a spec chapter or section in either tier (for example
   `spec/11-requirements-and-suspension.md` or `spec/std/testing.md`), a
   design record, or a feature
   named across several files.
2. Optional: the owner's goal (fewer keywords, fewer diagnostics, one form
   per idea) and anything declared off limits.

## Method

1. **Inventory.** List what the area defines: forms and spellings,
   markers and annotations, rules with their IDs, diagnostic codes, and
   concepts. For a chapter, start from
   `node --experimental-strip-types spec/tools/rule-inventory.ts spec/<chapter>.md`,
   where a stdlib chapter's `<chapter>` is `std/<module>`.
   Note which fixtures in `spec/conformance/` and which guide pages use each
   item.
2. **Find candidates.** Look for:
   - a special case of a general rule, stated separately;
   - two markers, spellings, or rules that say the same thing;
   - a concept that only one use case needs;
   - a mechanism that an existing one could express (a trait, an ordinary
     function, an existing annotation, an existing row or bound form);
   - a language-tier rule for a std item that passes the tier test in
     AGENTS.md, which the cut would move to the stdlib tier;
   - a rule that exists only to patch another rule's corner case;
   - inference whose result a short explicit form would state better.
3. **Shape each cut.** Give before and after hd examples, each a few lines
   and parsed. Name the existing rules that absorb the cut, by rule ID or
   anchor. Write what the cut costs: what becomes longer, harder, or
   impossible, and which programs change meaning.
4. **Check soundness.** For each cut, name what the removed rule protected
   against (a bad program, an ambiguity, a runtime failure). Show the
   absorbing rule still rejects it, with an error example. If it does not,
   say the cut opens a hole and state the hole; do not hide it.
5. **Account for every rule.** Table every rule ID, diagnostic code,
   grammar production, and fixture the cut touches, each marked *deleted*,
   *merged into X*, *reworded*, or *unchanged*. Nothing may vanish without a
   row. A deleted diagnostic must say what the same program reports instead.
6. **Compare.** Where another language made the same cut, or kept the
   feature for a reason, say which and cite a source.
7. **Rank.** Rank cuts by complexity removed (forms, rules, diagnostics,
   concepts) against cost, and group cuts that depend on each other. Each
   cut ends with one owner question.

## Output

File: `future-work/<AREA>_SIMPLIFICATION.md`, linked from
`future-work/README.md`. Sections, in order:

1. Title and a status line ("simplification review, YYYY-MM-DD; changes
   no decision, spec text, or prototype code"), with links to the area.
2. Summary table: rank, cut, what absorbs it, items removed, cost, and
   soundness (*holds*, *holds with condition*, or *opens a hole*).
3. Inventory of the area.
4. One subsection per cut: Before, After, Absorbed by, Cost, Soundness,
   Rule accounting table, Other languages, and Question for the owner.
5. Cuts considered and rejected, with one line each on why.
6. Parse Log.

## Hard Rules

- Never apply a cut: do not edit the spec, a design record, fixtures, or
  `src/`.
- Every cut names an absorbing rule that already exists. A cut that needs a
  new mechanism is a design option, and goes to `brainstorm` or to an owner
  question instead.
- No rule, diagnostic, or fixture disappears without a row in the
  accounting table.
- A cut that changes the meaning of a valid program says so first.
- Do not propose cuts that contradict recorded owner principles in the
  shared rules, such as merging derivation blocks or dropping `defer`.

## Done When

- Every cut has before and after examples that parse, an absorbing rule, a
  cost, a soundness verdict, and a complete accounting table.
- The summary ranks all cuts, and each cut ends with one question.
- `future-work/README.md` links the report, and `bash spec/check.sh` and
  `npm run website:build` pass.
- The final message to the caller gives the report path and the top cuts
  with their soundness verdicts.
