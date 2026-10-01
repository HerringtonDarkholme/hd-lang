---
name: stress-test
description: Stress-test an hd design record (a set of owner decisions) by translating real code from well-known libraries into it, parsing every example with the spec's reference parser, and ranking what breaks as questions for the owner. Use when a design record in future-work/ has new or revised decisions and should be tried against realistic code before it reaches the spec. Writes a report under future-work/; never decides or edits decisions, spec, or prototype.
---

# Stress Test

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

## Purpose

Find where a recorded design fails on real code, before it reaches the
spec. The output is evidence and questions for the owner, never a decision.

## When To Use

- A design record in `future-work/` gained or changed owner decisions.
- The owner asks whether a decision set holds up, or asks for a retest
  after fixes. A round N report maps round N-1's problems to their status.
- Not for a question with no decision yet: use the `brainstorm` skill.
- Not for finding things to remove: use the `complexity-reducer` skill.
- Not for applying a decision to the spec: use the `spec-update` skill.

## Inputs

1. The design record and the exact decisions under test, for example
   "`future-work/archive/FN_TYPE.md`, questions 9 and 10". Ask the caller if
   the scope is unclear.
2. Optional: variants to compare (V1, V2a, ...), libraries to translate, or
   an earlier stress test whose problem IDs to reuse.

## Method

1. **Fix the surface.** Read the record, its owner decisions, and every spec
   section it depends on, in both tiers: the numbered chapters and
   `spec/std/` (see the shared rules, Spec Tiers). Write a short "Surface Being Tested" section that
   lists exactly what the report assumes. Where the record leaves an API
   open, state the assumption the examples use.
2. **Pick use cases.** Choose 8-20 cases that cover the design's claims.
   Prefer the public API of real, well-known libraries: Rust crates such as
   serde, thiserror, anyhow, clap, reqwest, cargo, or ripgrep, or their Go,
   Swift, Kotlin, or Python equivalents. Name the library each case
   approximates. Add operation cases (propagation, printing, tests,
   boundaries) that reuse the declared types.
3. **Translate.** For each case give the original shape in its own language
   fence, the hd translation, what the compiler would generate, how the
   relevant operations behave, and what breaks. Stay as close to the
   original's behavior as hd allows, and note every difference a user would
   notice.
4. **Parse.** Check every `text` block with the reference parser (the
   command is in the shared rules). Fix syntax mistakes that are yours.
   Mark syntax no chapter specifies with `# hypothetical syntax`, and record
   each result either way.
5. **Judge.** Give each case a verdict: *works*, *friction* (works with a
   workaround or a visible behavior difference), or *breaks* (the design
   cannot express it, or an operation gives a different answer). When
   counting lines or markers, state the counting rules first.
6. **Compare.** Where the design borrows from another language, show the
   same case there (for example thiserror, Swift `Error`, Kotlin sealed
   classes), with sources.
7. **Rank problems.** Group failures into problems. Rank by how many cases
   hit a problem and how silent the failure is. Each problem gets an ID
   (reuse an earlier ID for the same problem), an **Effect** paragraph that
   names the cases, and **Candidates**: one to three fixes, each phrased as
   a question for the owner. Do not reopen alternatives the record rejected.
8. **Write and link.** Write the report, add a one-to-three-line entry to
   `future-work/README.md`, and run the checks in the shared rules.

## Output

File: `future-work/<TOPIC>_STRESS_TEST.md`, or `<TOPIC>_STRESS_TEST_<N>.md`
for round N. Sections, in order:

1. Title and a status line: "design review, YYYY-MM-DD; changes no
   decision, design record, spec text, or prototype code", with links to
   the decisions and spec sections under test.
2. Contents.
3. Surface Being Tested, and the variants if any.
4. Method: case selection, counting rules, verdict definitions, and limits
   (parsing is not type-checking).
5. Summary: a verdict table (case, library, verdict per variant, design
   element at fault) and one paragraph of conclusions.
6. Cases, one subsection each.
7. Comparison with other languages, as a table when there are variants.
8. Problems, Ranked: a table (rank, ID, problem, severity, cases), then one
   subsection per problem with Effect and Candidates.
9. Parse Log: every `text` block and its result, plus any reference-parser
   findings.

Earlier stress tests, such as the chaining study and the error and
derivation rounds, are worked examples of this format in git history: a
report is removed once its decisions are in the specification.

## Hard Rules

- Never edit a design record's decisions, a spec chapter, a fixture, or
  `src/`. Findings about them go in the report.
- Never mark a candidate fix as chosen. At most say which candidates the
  evidence favors, labeled as such.
- Every hd block is parsed and logged. No unparsed example ships.
- Library shapes are approximations. Say so, and do not paste large
  copyrighted source.

## Done When

- Every case has a translation, a verdict, and its blocks in the Parse Log.
- Every problem has an effect, the cases it hits, and one to three
  candidate questions.
- `future-work/README.md` links the report, and `bash spec/check.sh` and
  `pnpm run website:build` pass.
- The final message to the caller gives the report path, the verdict
  summary, and the top three problems.
