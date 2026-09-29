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

## Writing hd Code: Cheapest Model And A Feedback Log

When a task is **writing hd programs**, use the least capable and cheapest
model available (for example Haiku). Examples of such tasks:
- the standard library in `lib/std`;
- examples, playground examples, and hd test files;
- sample apps.

The point is to learn how hard hd is to use, and how well the compiler's
feedback helps.

Every such agent logs each mistake it made in
[audit/hd-writing-log.md](audit/hd-writing-log.md): one row per syntax
error, type error, API misuse, or semantic misunderstanding. Record:
- what it wrote;
- what the compiler said, word for word;
- whether that message led it to the fix;
- the fix.

The log is used to audit the compiler's diagnostics and the docs.

This rule does **not** apply to:
- specification text and conformance fixtures;
- the compiler prototype itself (`src/`).

Those tasks use the normal model. Owner direction, 2026-09-29.
