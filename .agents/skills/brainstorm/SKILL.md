---
name: brainstorm
description: Generate and compare design options for an open hd language question - survey how other languages and tools solve it (Rust, Go, Swift, Kotlin, Haskell, Scala 3, Zig, Koka, Unison, MoonBit, TypeScript, and others) with sources, propose three to five distinct options including one radical simplification, show each in short parsed hd code, compare them in a table, and end with a labeled recommendation and questions for the owner. Use when a design question is open and the owner wants options, not a verdict.
---

# Brainstorm

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

## Purpose

Give the owner a small set of genuinely different answers to one open
question, with the evidence to choose between them. The owner decides; the
recommendation is advice, labeled as such.

## When To Use

- A question is open in a GitHub issue, a design record, or
  the owner's message, and no decision exists yet.
- A stress test or simplification review raised a problem whose candidate
  fixes need a fuller comparison.
- Not for testing decisions already made: use `stress-test`.

## Inputs

1. The question, in one sentence, and where it came from.
2. Constraints: the owner decisions and principles that bind it, and
   anything the owner already rejected. Find them in the design records
   before starting; ask the caller if none are given.
3. Optional: languages or libraries the owner wants covered.

## Method

1. **Teach the domain first** when the owner may not know it: what the
   problem is in plain words, the vocabulary, the hard parts, and why naive
   answers fail. Do this before any option, with a worked example. Assume
   the reader is a strong engineer new to this field.
2. **State the problem.** Restate the question, what hd has today (with
   spec links to either tier, the numbered chapters or `spec/std/`), and the use cases an answer must serve. Pick three to six
   concrete use cases and keep them fixed for every option.
3. **Check it is core.** If the question depends on an unsettled core
   decision, say so and ask that question first instead.
4. **Survey.** Show how 6-12 languages or tools solve it: pick those with a
   real position, such as Rust, Go, Swift, Kotlin, Haskell, Scala 3, Zig,
   Koka, Unison, MoonBit, TypeScript, OCaml, or Java. Use one table row
   each, cite a source per claim, and end with two or three takeaways.
5. **Generate options.** Propose three to five options that differ in
   mechanism, not in spelling. Include at least one radical simplification:
   the smallest design that could work, even if it drops a use case. Name
   each option by its idea, not a letter alone.
6. **Show each option.** Give a short hd example of the same use case for
   every option, parsed with the compiler. State the rules the
   option adds or removes and the tier of each, by the tier test in
   AGENTS.md "Spec Scope For The Standard Library", its soundness conditions, and its interaction
   with existing features (traits, requirement rows, suspension, derivation,
   RC representation).
7. **Compare.** Build one table: options as columns, and as rows the use
   cases, the rules added or removed, soundness, agent-writability,
   human readability, implementation cost, and evolution.
8. **Recommend.** Under a heading **Recommendation**, pick one option or
   an ordering and say why in a short list. Say what the recommendation
   gives up, and name the next best option.
9. **Ask.** End with numbered questions for the owner, one idea each. Each
   question lists its options, the recommended one, and a short example.

## Output

File: `docs/future-work/<TOPIC>.md` (a survey and design options record, like
the deleted FN_TYPE record), linked from
`docs/future-work/README.md`. Sections, in order:

1. Title and a status line ("design exploration, YYYY-MM-DD; nothing here
   is decided or in the specification").
2. Contents.
3. The Domain, for a reader new to it.
4. Problem, and What hd Has Today.
5. Survey, with takeaways.
6. One section per option.
7. Comparison table.
8. Recommendation.
9. Questions For The Owner.
10. Sources, and a Parse Log.

When the owner later answers, a separate task records the answers in an
**Owner Decisions** section at the top. This skill does not write that
section.

## Hard Rules

- Options must be distinct mechanisms. Two spellings of one idea are one
  option.
- At least one option is a radical simplification.
- The recommendation is labeled and separate. Every other section states
  facts and trade-offs only.
- Respect recorded owner decisions and principles. An option that reopens a
  rejected alternative says what new evidence justifies it.
- Every claim about another language has a source.

## Done When

- The survey, every option, the comparison table, the recommendation, and
  the questions are present, and every hd block parses or is marked
  `# hypothetical syntax`.
- `docs/future-work/README.md` links the record, and `bash spec/check.sh` and
  `pnpm run website:build` pass.
- The final message to the caller gives the file path, the options in one
  line each, and the recommendation.
