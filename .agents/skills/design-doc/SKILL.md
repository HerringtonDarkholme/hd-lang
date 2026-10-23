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

## First: Exploration Or Design?

When the domain is new to the owner (for example host embedding, durable
execution, or REPL checkpoints), do not start with a design. First write a
solution exploration with the `brainstorm` skill. It explains the domain
from scratch, surveys how existing systems solve it, and compares the
options on the use cases. Write this design doc only after the owner has
picked a direction. A single recommended design is hard to review without
the map of the alternatives.

## The Issue Comes First

The issue's Deliverable, constraints and Done check are the
specification of the document. Map each item to a section with the same
name, in the form the issue asks for. A shape the issue defines must
appear in that form; a workaround that changes it is reported as a gap,
with a minimal repro, not presented as the shape. Then apply the bar
and the domain checklist below.

## The Bar

A design doc is not done until it has each of these. Leave a part out only
when it truly has nothing to say, and then say why in one line.

1. **Problem, users and use cases.** Who uses it, and three to six concrete
   use cases, each followed end to end (for a tool: the user's goal, the
   commands, the output they read, the decision they make from it). Keep
   them fixed for every option. Then what hd does today, with spec links
   and a probe on the current build.
2. **State of the art, in depth.** Before designing, list what the leading
   tools in the domain handle and why, not only their surface syntax. For
   example, a benchmark harness must answer warm-up, iteration
   calibration, statistics (confidence or credible intervals, outliers),
   noise control, baselines and comparison of an old and a new
   implementation. Every item on that list is either covered by the design
   or excluded with a reason. For each important tool, give its design
   (how it works inside), its interface (what the user writes and sees),
   its coverage (what it handles) and its limitations (what it refuses or
   gets wrong, and why). A table of tools with one line each is not
   enough.
3. **Semantics it touches.** Go below the surface. For every hd concept the
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
4. **Architecture.** Where it lives (crate, module, pipeline stage), which
   existing facts it reuses (TIR, checker facts, caches) and which new ones
   it needs. Include:
   - the data structures, with field lists;
   - the data flow, as a short diagram or numbered steps;
   - pseudo-code for the core algorithm, 20 to 60 lines, in Rust-like or
     hd-like form;
   - caching and incremental behaviour;
   - how it is tested.
5. **Extension points.** How users extend it (custom lints, user policies,
   user derives, plugins), or why they cannot yet and what would allow it
   later. State the security and stability cost of each extension
   mechanism.
6. **Options.** At least two genuinely different mechanisms, plus the
   smallest design that could work. Compare them on the fixed programs. A
   comparison table summarizes the prose; it never replaces the argument.
7. **Cost, bottom-up.** One row per component: the code it replaces (exact
   `wc -l` code lines), the code it adds, piece by piece, and a reference
   implementation's measured size. Then compile time, runtime, emitted Wasm
   size, and the web compiler's gzip size (the 2 MB cap is hard). Add a
   stop rule: a commit that runs past 1.5 times its share stops for review.
   A top-down guess is not a budget.
8. **Stress test.** Run the `stress-test` skill's method on the design
   itself: real code from known libraries and tools, adversarial inputs,
   and the edge cases from step 3. Record what breaks and how the design
   answers it.
9. **Prototypes.** Every example parses with today's compiler, and every
   runnable prototype runs (`hd FILE`). Where a needed feature is missing,
   give the exact gap with a minimal repro and the diagnostic, not prose.
10. **Phasing.** Implementation steps, each independently green and about
   one hour of agent work, with the test that proves each one.
11. **Recommendation.** One option, what it gives up, and the next best.
    Then name exactly what needs the owner's approval.

## Domain Checklists

The bar above applies to every design. Each domain adds its own concerns.
Name the domain (or domains) in the status line, and give every item of
its checklist a section or a one-line reason why it does not apply.

### Language feature (spec/lang, the checker)

- **Grammar:** the exact syntax, its parse in every position, ambiguities
  with existing forms, and the formatter's output.
- **Static semantics:** typing rules, inference (use sites only, never a
  declaration's signature), generics and bounds, traits and coherence,
  requirement rows, suspension and bang calls.
- **Dynamic semantics:** evaluation order, panics and their order, and
  value or reference behaviour under copies, moves and element access
  (decision E).
- **An interaction matrix** with every existing feature: patterns and
  exhaustiveness, derives and `Structure`, closures and captures, modules
  and visibility, serialization, the ownership passes, and the inliner.
  Give a program for each interaction that could go wrong.
- **Soundness:** an argument that the rules cannot be broken, and what an
  accepted program must never do.
- **Diagnostics:** the error codes, messages and fix-its a misuse gets.
- **Agent writing and human reading:** how a coding agent gets it right
  first time, and how a reader understands it with no context.
- **Spec and tests:** the rules it adds or retires, by tier, and the
  conformance cases.
- **The no-feature alternative:** a library-only or existing-mechanism
  option, compared honestly.

### Standard library API (spec/std, lib/std)

- **Layer:** hd code or intrinsic, and why. Intrinsics need a spec-named
  reason.
- **The stdlib design bar** (#350): batteries included, the 80% ergonomic
  path plus the 20% atoms, strongly typed by default, no naive algorithms,
  builders with one terminal, and no one-combination shortcuts.
- **Complete signatures,** with error types, requirement rows
  (capabilities), bang suspension, receivers (`&`, `&mut`, value) and
  generic bounds.
- **Naming and shape** consistent with the rest of std.
- **Performance:** algorithmic complexity, allocations per call, and the
  cost on a hot path.
- **Security:** capability leaks, forged values, types created from data.
- **Tests:** unit and property tests, plus a stress test that translates
  real library code (Rust, Go, Effect) into the API.
- **Prior art:** each library's API design and its known problems.

### CLI and tooling (spec/cli, hd_cli)

- **Workflows end to end:** what the user types, sees and decides, for
  local use and for CI.
- **Command and flag naming:** a read command is not named like a write
  command, and flags stay consistent with the other commands (selection,
  `--format`, `--jobs`).
- **Output:** the text form, the JSON schema, exit statuses, and how
  errors read.
- **Configuration:** `hd.toml` keys, precedence over CLI flags, and
  validation.
- **Scope:** files, packages and workspaces.
- **Incremental behaviour and speed** on a large package.
- **Platforms:** macOS, Linux, Windows; Node and wasmtime.
- **Security:** grants, network, and writes outside the package.
- **Tests:** CLI conformance cases.

### Compiler internals (crates/hd_*)

- **Pipeline position:** inputs and outputs (TIR, checker facts), and the
  invariants it relies on and must keep.
- **Data structures** with their memory layout (the repo's data-oriented
  rules), and the algorithm with its worst-case complexity. Name any
  superlinear step.
- **Invariants and the verifier:** what is checked in checked and debug
  builds.
- **Caching and incremental keys,** determinism (byte-identical output)
  and parallelism.
- **Budgets:** compile time per stage, peak memory, emitted Wasm size per
  program, web compiler gzip (2 MB hard cap), and Rust lines bottom-up
  against the code replaced.
- **Interaction with other passes,** such as the ownership passes and the
  inliner.
- **Tests:** stage snapshots, conformance, checked mode, plus a switch and
  rollback plan when it replaces an existing path.

### Runtime and host (hd_wasm_linear runtime, hosts, std.rt)

- **Memory model:** object layout, reference counting and cycle collection
  effects, allocations per operation.
- **Host ABI and wire format,** with parity between Node and wasmtime.
- **Concurrency:** suspension, cancellation, deadlines, and wake-ups.
- **Failure:** panics, traps, and resource cleanup.
- **Platforms:** Windows, macOS, Linux; browser versus CLI hosts.
- **Footprint:** hello's size and what every program pays.
- **Performance:** runtime benchmarks against a saved baseline, run
  interleaved.
- **Security:** the sandbox, grants, and what a guest can reach.
- **Debugging and observability:** traces, line tables, logs.

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

1. Title and a status line: "design proposal, YYYY-MM-DD; domain: …; not
   accepted".
2. Contents.
3. Problem, users, use cases and today.
4. State of the art.
5. Semantics.
6. Options, then the recommended design's architecture (data structures,
   data flow, pseudo-code, caching, testing).
7. Extension points.
8. Cost.
9. Stress test.
10. Phasing.
11. Recommendation and approvals.
12. Sources and a parse log.

## Review Before Handing In

- Does every Deliverable item of the issue have its own section, in the
  form the issue asks for?
- Could an implementer start tomorrow without asking a design question?
- Does every item of the domain checklist have a section, or a reason why
  it does not apply?
- Does every rule the design depends on have a spec link?
- Is every number measured or derived bottom-up, with its method stated?
- Did the stress test find something? If not, it was not adversarial
  enough.
- Is every item of the state-of-the-art list covered or excluded with a
  reason?
- Does each use case work end to end in the recommended design?
