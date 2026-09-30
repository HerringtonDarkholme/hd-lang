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

The spec names a std item only when the language needs it:
- the compiler or runtime gives it support;
- it is in the prelude;
- or syntax refers to it, such as a literal form.

Any other std type or function, such as `Set` or a default hasher, lives only
in [future-work/STDLIB.md](future-work/STDLIB.md) and the std sources. Spec
examples must not depend on those. Owner direction, 2026-09-28.

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
