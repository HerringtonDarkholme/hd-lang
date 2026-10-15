---
name: write-issue
description: Write a GitHub issue for hd-lang that a fresh agent (Muse, a subagent) can act on without asking - one item, the rule, what happens today, where to look, and a done check - concise, with no history. Use when filing, splitting or rewriting an issue, or before assigning one to Muse.
---

# Write An Issue

An issue is a work order for a reader with no context: a fresh agent or
the owner months later. It must say what to change, why, where, and how to
prove it is done. It must not tell the story of how the item was found.

## One Item Per Issue

- One fix, one feature or one decision per issue. Two items are two issues.
- A list of loosely related items ("smaller checks", "leftovers") is never
  an issue. Split it before filing or assigning it.
- When splitting an issue, close the old one with one line listing the new
  numbers, and say which items were dropped and why (for example "checked
  at <commit>: already works").

## Probe Before Writing

- Reproduce the problem with the current build and quote the result.
  Do not file an item from an old audit or note without re-checking it:
  many old items are already fixed.
- If it already works, do not file it. If only a test is missing, say
  "coverage, not a bug".

## The Body

Use these labelled parts, in this order. Leave out a part that has nothing
to say. Aim for 6 to 15 lines.

1. **Rule.** The spec rule ID(s) and file:line, with one sentence of what
   the rule requires and the diagnostic code. For a design or engineering
   item with no rule, one or two sentences of the problem and why it
   matters.
2. **Today.** The smallest input and what happens now: exact command or
   program, actual output or code. Name the commit you probed at.
3. **Where.** The likely crate and file (`crates/hd_check/src/pat.rs`), or
   the doc section that holds the design. A pointer, not a plan.
4. **Do.** Only when the fix has a choice the owner already made, or a
   constraint ("do not edit spec/", "reuse the existing fix-it model").
5. **Done check.** The test that fails before and passes after: which kind
   (conformance fixture, CLI case, driver test), where it goes, and what it
   asserts.

For a design issue, replace Today/Where/Done with the open questions, the
options already known, and what "decided" looks like.

## Leave Out

- History: "migrated from task T123", "found in audit X", "split out of
  the M4 plan", old commit trails. Git and the issue timeline keep them.
- Links to deleted files. Point at the live doc or issue instead.
- Restating the spec at length. Cite the rule; quote at most one sentence.
- Plans the implementer should make themselves.
- Words that only the author knows (internal task names, agent lane names).

## Title And Labels

- Title: the defect or goal in plain words, under ~70 characters:
  "`pub` on a local function is accepted", not "G2 from coverage audit".
- Labels: one priority (P1 to P4) and one area (checker, cli, runtime, std,
  wasmtime, perf, design, documentation); add `muse` only when the item is
  fully specified (rule, today, done check) and needs no owner decision;
  `owner-decision` when it waits on the owner. Never invent labels.

## Example

```text
Title: Duplicate string match arms are not reported unreachable

**Rule.** `flow.match.unreachable` (spec/lang/06-control-flow.md): an arm
that earlier arms already cover is an error.

**Today** (at 66cf2167). `"a" => 1`, `"a" => 2`, `_ => 3` is accepted.
Integer, char and bool duplicates are already caught.

**Where.** crates/hd_check/src/pat.rs (coverage).

**Done check.** An invalid fixture in spec/conformance/typing/invalid/,
indexed in cases.tsv, citing the rule; fails before, passes after.
```
