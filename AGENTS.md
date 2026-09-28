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
