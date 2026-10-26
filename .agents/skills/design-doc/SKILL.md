---
name: design-doc
description: Write an hd design for the owner to approve - a stdlib API as a pull request of checked hd signatures plus usage examples, or a language, compiler, CLI or runtime design as a short doc ordered for a human reader (context, illustrative problems, proposed solution; prior art and stress test at the back). Use for any "design document", "design note" or "design PR" deliverable in docs/future-work/. For a pure option survey of an open language question, use `brainstorm`.
---

# Design Doc

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

Owner, 2026-10-25: "don't make the design doc fucking long, the
organization should make human read easy", and "for stdlib stuff, submit
signature and example usages". A design is read in five minutes. Depth
goes to the back, never to the front.

## Which Form

- **Standard library API** (anything in lib/std or spec/std): a pull
  request of hd code, no prose doc. See "Stdlib: Signatures And Examples".
- **Everything else** (language rule, compiler internals, CLI, runtime,
  host): a short doc. See "Other Designs: A Short Doc".
- **A domain new to the owner:** run `brainstorm` first; design only after
  the owner picks a direction.

The issue's Deliverable and Done check still define what must be covered.
A shape the issue or the owner fixed appears in that shape; a gap is shown
with a minimal repro, not worked around.

## Stdlib: Signatures And Examples

Deliver one hd package as a pull request (never pushed to main):

```text
docs/future-work/<topic>/
  hd.toml
  src/<topic>.hd     every public type and function; bodies are panic("todo")
  src/examples.hd    realistic usage of every part
```

- `hd check` and `hd fmt --check` pass in the package. Say how to run them.
- **Every public item is used by an example.** If no example needs it,
  delete it. Every example is a realistic task (a client call, a server
  handler, a config load), not a call list.
- **Diff against today's std.** Anything the new API drops or changes in
  behaviour is listed in the PR, never silent.
- **Check the basics against prior art** (for HTTP: Fetch, Go net/http,
  reqwest; for collections: Rust std, Go): name each common case they
  cover and show it in an example or say why it is out.
- Signatures follow the repo's rules: requirement rows, bang functions,
  receivers (`&self`, `&mut self`, `mut self`, value), error types with
  `@error`, derives, and the stdlib bar (#350): typed by default, the 80%
  ergonomic path plus atoms, no one-combination shortcuts.

The PR description is short, in this order:
1. **Context**, two or three lines, and the issue number.
2. **Illustrative problems**: two to four, each a few lines of today's code
   and what goes wrong.
3. **What the PR proposes**: a few bullets pointing at the files.
4. **Open questions** for the owner, numbered, one line each.

## Other Designs: A Short Doc

File `docs/future-work/<TOPIC>.md`, linked from
`docs/future-work/README.md`. Main body: about 150 lines at most. Order:

1. **Title and status line**: "proposal, YYYY-MM-DD; domain: …; not
   accepted".
2. **Context**: what exists today and why it matters, five lines at most,
   with spec links.
3. **Illustrative problems**: two to four concrete programs or commands,
   each with what happens today and why that is wrong. These stay fixed for
   the rest of the doc.
4. **Proposed solution**: shown on those same problems first (code before
   prose), then how it works: the rules, the data structures and the
   algorithm, briefly. Pseudo-code only where prose would be longer.
5. **Cost**, bottom-up: a short table, one row per component (code
   removed, code added, compile time, runtime, Wasm size, web compiler gzip
   against the 2 MB cap).
6. **Open questions** for the owner, numbered, one line each.

Then, at the back, as appendices a reader may skip:

- **A. Alternatives considered**: each in a few lines, and why not.
- **B. Prior art**: how the leading tools handle it, with sources.
- **C. Stress test**: real code and adversarial cases run against the
  design, and what broke.
- **D. Details**: the domain checklist items below, one short paragraph
  or one line each, and the implementation phases (each about one hour,
  each with its test).

### Domain checklists (appendix D, one line each where possible)

- **Language feature:** grammar and formatting, typing and inference
  (use sites only), interaction with patterns, derives, closures, modules,
  ownership and the inliner, soundness, diagnostics, spec rules and
  conformance, the no-feature alternative.
- **Compiler internals:** pipeline position and invariants, data layout,
  worst-case complexity, caching and determinism, budgets, tests and the
  rollback plan.
- **CLI and tooling:** the workflow end to end, naming consistent with other
  commands, text and JSON output, exit statuses, `hd.toml` keys, platforms,
  security, CLI conformance cases.
- **Runtime and host:** object layout and counting, host ABI parity (Node,
  wasmtime), suspension and cancellation, failure and cleanup, footprint,
  benchmarks, sandbox and grants.

## Style

- Code first, prose second. A reader should see the proposal in the first
  screen.
- Short sentences, plain words. No question lists in the body, no
  decision tables standing in for argument, no "Owner decisions" section.
- Every measured number names its method; every claim about another tool
  has a source.
- Prefer existing mechanisms. Mark any new language rule, intrinsic or
  syntax as needing approval, with its cost.

## Review Before Handing In

- Could the owner understand the proposal from the first screen?
- Stdlib: is every public item used by an example, and does the package
  check and format?
- Doc: is the main body within about 150 lines, with prior art and the
  stress test at the back?
- Does every illustrative problem work in the proposal?
- Is anything from today's behaviour dropped without being listed?
