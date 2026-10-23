---
name: design-doc
description: Write a design document for an hd feature, tool or compiler component (for example `hd lint`, the inliner, host embedding) that the owner can approve and an implementer can build from. Covers the semantics the feature touches, the architecture with data structures and pseudo-code, extension points, a bottom-up cost budget, stress tests and a recommendation. Use for a deliverable named "design document" or "design note" in docs/future-work/. For a pure option survey of an open language question, use `brainstorm`.
---

# Design Doc

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

## Purpose

A design doc says what to build, how it works inside the compiler or tool,
what it costs, and where it breaks, with enough detail that an implementer
needs no further design decisions. A survey of other tools plus a command
line and a config block is not a design: it is the first section of one.

## The Bar

A design doc is not done until it has each of these. Leave a part out only
when it truly has nothing to say, and then say why in one line.

1. **Problem and users.** Who uses it, the three to six concrete programs it
   must serve (kept fixed for every option), and what hd does today, with
   spec links and a probe on the current build.
2. **Semantics it touches.** Go below the surface. For every hd concept the
   feature reads or changes, state the exact rule it relies on, with a spec
   link:
   - types (generics, associated types, trait values, references, rows);
   - names and scopes (bindings, shadowing, closures and captures, modules,
     visibility);
   - control flow (branches, loops, `break`, `return`, `?`, `defer`,
     `using`, suspension, cancellation, panics);
   - ownership and values (copies, moves, element references, `mut`).

   For each, say how the feature handles it, and give a counterexample
   program where a naive version is wrong.
3. **Architecture.** Where it lives (crate, module, pipeline stage), which
   existing facts it reuses (TIR, checker facts, caches) and which new ones
   it needs. Include:
   - the data structures, with field lists;
   - the data flow, as a short diagram or numbered steps;
   - pseudo-code for the core algorithm, 20 to 60 lines, in Rust-like or
     hd-like form;
   - caching and incremental behaviour;
   - how it is tested.
4. **Extension points.** How users extend it (custom lints, user policies,
   user derives, plugins), or why they cannot yet and what would allow it
   later. State the security and stability cost of each extension
   mechanism.
5. **Options.** At least two genuinely different mechanisms, plus the
   smallest design that could work. Compare them on the fixed programs. A
   comparison table summarizes the prose; it never replaces the argument.
6. **Cost, bottom-up.** One row per component: the code it replaces (exact
   `wc -l` code lines), the code it adds, piece by piece, and a reference
   implementation's measured size. Then compile time, runtime, emitted Wasm
   size, and the web compiler's gzip size (the 2 MB cap is hard). Add a
   stop rule: a commit that runs past 1.5 times its share stops for review.
   A top-down guess is not a budget.
7. **Stress test.** Run the `stress-test` skill's method on the design
   itself: real code from known libraries and tools, adversarial inputs,
   and the edge cases from step 2. Record what breaks and how the design
   answers it.
8. **Prototypes.** Every example parses with today's compiler, and every
   runnable prototype runs (`hd FILE`). Where a needed feature is missing,
   give the exact gap with a minimal repro and the diagnostic, not prose.
9. **Phasing.** Implementation steps, each independently green and about
   one hour of agent work, with the test that proves each one.
10. **Recommendation.** One option, what it gives up, and the next best.
    Then name exactly what needs the owner's approval.

## Style

- Write a readable proposal in prose, following the repo's design-doc
  style. No question lists, no decision tables standing in for argument,
  no "Owner decisions" section; fold decisions into the body text.
- Keep facts, measurements and recommendations apart, and cite a source
  for every claim about another tool.
- Prefer existing mechanisms. Mark any new language rule, intrinsic or
  syntax as needing approval, with its cost.

## Output

File: `docs/future-work/<TOPIC>.md`, linked from
`docs/future-work/README.md`. Sections, in order:

1. Title and a status line: "design proposal, YYYY-MM-DD; not accepted".
2. Contents.
3. Problem, users and today.
4. Semantics.
5. Options, then the recommended design's architecture (data structures,
   data flow, pseudo-code, caching, testing).
6. Extension points.
7. Cost.
8. Stress test.
9. Phasing.
10. Recommendation and approvals.
11. Sources and a parse log.

## Review Before Handing In

- Could an implementer start tomorrow without asking a design question?
- Does every rule the design depends on have a spec link?
- Is every number measured or derived bottom-up, with its method stated?
- Did the stress test find something? If not, it was not adversarial
  enough.
