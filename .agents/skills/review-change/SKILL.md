---
name: review-change
description: Review a compiler, std or spec change before it merges - refuse a new depth, count or variable cap that names no language limit, refuse a feature fix that skips a form of its feature, and send back a fix whose design is not the general algorithm instead of merging it with a follow-up ticket; the spec rules the change touches are the judge. Use when gating a lane or chat commit, reading a merge gate's SECOND REVIEW, or reviewing any change before push.
---

# Review A Change

A change merges when the spec says it is right, not when its own test
passes. A narrow fix that turns one test green and leaves the rule broken
for the next form is how the codebase got its backlog: on 2026-11-15 one
night's lanes found 30 bugs, most in forms no test had ever tried, and
three were caps added by fixes merged that week.

## Refuse These

Each of these is sent back. Never merge it with a follow-up ticket.

1. **A new cap with no language limit.** A depth, count, size, step or
   variable cap (`MAX_*`, `*_LIMIT`, `OPEN_VARS = 4`, "at most 8", a fuel
   or round bound) is refused unless it names the spec limit it enforces
   (`grammar.limit.*`, `hd_base::limits::TYPE_DEPTH`, ...) and that limit
   has its diagnostic. A cap that only makes a test or a scale line pass
   is a shortcut. The fix is the linear algorithm.
2. **A feature fix that skips a form of its feature.** The change's tests
   cover every form the spec rule names, and every neighbour of the fixed
   case: each type kind (struct, enum, newtype, tuple, union, option,
   generic instance), each container (list, map, set, box), each call path
   (direct, method, generic, function value, closure, inlined in
   release), each build (dev, checked, release) and each host the code
   runs on. A form left out names why it cannot be affected.
3. **A fix whose design is not the general algorithm.** A scan per lookup,
   a walk per use, a rebuilt map per call, a recursion on input structure,
   a special case for one type, or a second copy of a walk that already
   exists. Send it back with the design that would hold: the index, memo
   or single pass a state-of-the-art compiler uses (rustc, Zig, Swift).
4. **A design that names no reference.** A new pass or data structure
   names the production implementation it follows (Binaryen, regalloc2,
   Cranelift, rustc, LLVM, V8, Zig, Swift) and where it differs, in the
   commit's design paragraph. Without one, send it back to read one
   first.

## The Spec Is The Judge

- List the spec rules the change touches. Each one needs a conformance
  case for each form it names, passing in dev, checked and optimized. A
  rule the change implements with no case is not done.
- A change that adds or rewords a rule adds its cases in the same commit.
- `pnpm -s spec coverage` must not drop.
- When the spec says nothing about the case, the change does not decide
  it. Stop, and ask the owner a question that cites the rules around it.

## Read

- The design paragraph of each commit message, against the code: the
  stated cost must be what the loops do.
- Every new constant, `cap`, `limit` and `max` in the diff.
- Every `Known:`, "remaining", "still" and "not yet" line in a commit
  message that ends `Fixes #N`. A named gap is either fixed in the same
  change or the commit does not say `Fixes`.
- The gate's SECOND REVIEW pick, NEW and WORSE baselines, against each
  case's expected note.
