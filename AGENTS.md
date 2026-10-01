# Agent Instructions

hd is a language design project. The owner decides every design question
through Q&A; agents research, stress-test, write design records, and ask.

For design work, use the skills in [.agents/skills](.agents/skills):
`stress-test`, `complexity-reducer`, `brainstorm`, and `spec-update` (to
apply a decided owner decision). Each follows
[.agents/skills/shared-rules.md](.agents/skills/shared-rules.md), which
also holds the git and safety rules for this repo. See
[.agents/README.md](.agents/README.md) for which harness reads what.

## Design Cost Order

When a design needs a new mechanism, prefer the cheapest kind of change.
The list below runs from least favored to most favored:

1. a new syntax change (a keyword, grammar form, or token);
2. a semantic rule exception (a special case in an existing rule);
3. a compiler intrinsic addition (something the compiler must know by name);
4. a core library addition (ordinary std code that uses existing rules).

Brainstorms and stress tests rank their options by this order. For each
option, list which kinds of change it needs, and prefer the one whose
costliest change is highest on this list (closest to 4). Break ties by
counting changes. Owner direction, 2026-09-28.

## Spec Scope For The Standard Library

The specification has two tiers, one style, and one conformance suite:

| Tier | Holds | Where |
| --- | --- | --- |
| language | syntax, static and dynamic semantics, intrinsics, and anything the compiler knows by name | the numbered chapters `spec/01-*.md` to `spec/14-*.md` |
| stdlib | decided std APIs that `lib/std` can implement in plain hd over the language tier | [spec/std/](spec/std/README.md), one file per module |

The language tier names a std item only when the compiler must know it: a
lang item, an intrinsic, a prelude name, or the conformance harness (`it`,
`assert`, `assert_equal`, `println`). Any other std API that the owner
decides lives in `spec/std/`. Undecided std design lives in
[future-work/STDLIB.md](future-work/STDLIB.md).

**The tier test.** Could `lib/std` implement the item in ordinary hd, over
language-tier items only, with no compiler knowledge of its name, and keep
the same observable behavior? Yes puts it in the stdlib tier. A diagnostic
code about it, a language rule that names it, or a position rule that lists
it each mean no.

| Kind of rule | Where it goes |
| --- | --- |
| syntax, typing, evaluation, a lang item, an intrinsic | the numbered chapter for its topic |
| a prelude name and its signature | [Modules, Prelude](spec/10-modules.md#prelude) |
| a test-position rule the compiler checks, or a literal `snapshot` argument | [Standard Testing](spec/10-modules.md#standard-testing) |
| a std API that passes the tier test, such as an iterator adapter or `trim` | `spec/std/<module>.md`, rule IDs `std-<module>.*` |
| every diagnostic code, and every panic category | the language tier: README Diagnostics, and Control Flow |
| a Revision Notes entry, naming its tier | the one log in [spec/README.md](spec/README.md#revision-notes) |
| a fixture | `spec/conformance/`; its tier is the tier of its `specification` column |
| undecided std design, such as `Set` or a default hasher | future-work/STDLIB.md only |

A stdlib chapter may cite any language rule. A language chapter links to
`spec/std/` only from a Note or See also, never from a numbered rule. A
language-tier fixture or example uses only language-tier std items.
Undecided std items appear in no spec example.

Owner direction, 2026-09-28; tiers from
[Spec Tiers](future-work/SPEC_TIERS.md#owner-decisions), 2026-09-30.
Until a migration task moves a section, its rules and fixtures stay where
they are, even when the tier test says stdlib.

## Spec Text Tools

Batch agents take rule counts from `npm run spec -- counts`, not from a
hand-written script. `npm run spec -- audit` reports STYLE.md warnings, and
`npm run spec -- refs ID` lists every citation of a rule. See
[spec/tools/README.md](spec/tools/README.md).
After a spec pass, a batch agent runs `npm run spec -- rewrite <base>` and
quotes its `Summary:` line in its report.

## Writing hd Code: Model Choice And A Feedback Log

When a task is **writing hd programs**, pick the model by what the code is
for:
- **Real implementation runs on Sonnet.** Code that must be correct uses
  at least Sonnet: the standard library in `lib/std`, examples, playground
  examples, sample apps, and hd test files.
- **Haiku is a probe.** A Haiku agent writes hd to show how a weak model
  copes with the language, and above all whether the compiler's error
  messages lead it to the fix. Its code is evidence, not a deliverable.

Evidence: in the property-test writing trials of 2026-09-29, checked only
with the reference parser, Sonnet wrote `not x`, `const`, a top-level
`NAME := ...`, and `(dt, key) := case`, which was a `syntax-error` then.
Haiku wrote `mut` before a parameter name, `{ ... }` blocks in match arms,
and bool-returning property bodies. Both kinds of mistake are worth
logging.

Every such agent, on either model, logs each mistake it made in
[audit/hd-writing-log.md](audit/hd-writing-log.md): one row per syntax
error, type error, API misuse, or semantic misunderstanding. Record:
- what it wrote;
- what the compiler said, word for word;
- whether that message led it to the fix;
- the fix;
- the model, in the Model column.

The log is used to audit the compiler's diagnostics and the docs.

This rule does **not** apply to:
- specification text and conformance fixtures;
- the compiler prototype itself (`src/`).

Those tasks use the normal model. Owner direction, 2026-09-29, revised
the same day to split implementation from probing (batch 13, Q11).
