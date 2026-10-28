---
name: design-doc
description: Write an hd design for the owner to approve, as one pull request - a short design doc ordered for a human reader (context, illustrative problems, proposed solution; prior art and stress test at the back), plus, for a stdlib API, a checked package of hd signatures and usage examples. No experiment or prototype folders. Use for any "design document", "design note" or "design PR" deliverable in docs/future-work/. For a pure option survey of an open language question, use `brainstorm`.
---

# Design Doc

First read [../shared-rules.md](../shared-rules.md)
(`.agents/skills/shared-rules.md`). Those rules apply to every step below
and win over anything here.

Owner, 2026-10-25: "don't make the design doc fucking long, the
organization should make human read easy", and "for stdlib stuff, submit
signature and example usages". Owner, 2026-10-26: "keep design doc, keep
signature and example, remove experiments". A design is read in five
minutes. Depth goes to the back, never to the front.

## The Handoff: One Pull Request

Every design is one pull request (never pushed to main) holding:

- **The design doc**, `docs/future-work/<TOPIC>.md`: short and readable,
  in the order under "The Design Doc". Always present.
- **For a standard library API** (lib/std, spec/std): a checked hd package,
  `docs/future-work/<topic>/`, with signatures and usage examples. See
  "Stdlib: Signatures And Examples". The doc points at it and quotes only
  the few lines a reader needs.
- **Nothing else.** No experiment, prototype, model, catalog or "before"
  folders, no measurement scripts, no evidence dumps. Measurements and
  probes are summarized in the doc in a line or a table; their files stay
  in scratch.

For a domain new to the owner, run `brainstorm` first and design only
after the owner picks a direction.

The issue's Deliverable and Done check still define what must be covered.
A shape the issue or the owner fixed appears in that shape; a gap is shown
with a minimal repro, not worked around.

## Stdlib: Signatures And Examples

The package, next to the doc in the same pull request:

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
  behaviour is listed in the doc, never silent.
- **Check the basics against prior art** (for HTTP: Fetch, Go net/http,
  reqwest; for collections: Rust std, Go): name each common case they
  cover and show it in an example or say why it is out. Match their
  behavior by default; each difference needs an anti case and its
  trade-off in the doc ([shared rules](../shared-rules.md)).
- Signatures follow the repo's rules: requirement rows, bang functions,
  receivers (`&self`, `&mut self`, `mut self`, value), error types with
  `@error`, derives, and the stdlib bar (#350): typed by default, the 80%
  ergonomic path plus atoms, no one-combination shortcuts.

The PR description is three lines: what it proposes, the issue number,
and how to run the checks. Everything else is in the doc.

## The Design Doc

File `docs/future-work/<TOPIC>.md`, linked from
`docs/future-work/README.md`. Owner, 2026-10-26: "it is not just short, it
should move most important stuff at the top, study existing stuff down,
stress test should not be skipped, also prior art should also have small
code snippet to reflect their design and usage, in appendix".

Most important first. The main body is about 150 lines at most:

1. **Title and status line**: "proposal, YYYY-MM-DD; domain: …; not
   accepted".
2. **The proposal**: what changes, in three to five lines, then the one
   code block a reader most needs to see (the new API in use, or the new
   rule on an example). A reader who stops here knows the decision asked.
3. **Open questions** for the owner, numbered, one line each, each with
   the recommended answer.
4. **Problems it solves**: two to four concrete programs or commands, each
   with what goes wrong today and how the proposal changes it, side by
   side.
5. **How it works**: the rules, data structures and algorithm, briefly;
   pseudo-code only where prose would be longer. For a stdlib API, a short
   table of the public types and what each is for, pointing at the package
   for the full signatures.
6. **Cost**, bottom-up: a short table, one row per component (code
   removed, code added, compile time, runtime, Wasm size, web compiler gzip
   against the 2 MB cap), plus any compiler gap the design hits, with its
   diagnostic.

Then the appendices, in this order. A reader may skip them; the writer may
not:

- **A. Stress test (required, never skipped)**: real code from well-known
  libraries or apps rewritten in the proposal, plus adversarial and edge
  cases; each with what happened and what the design does about it. A
  stress test that found nothing was not adversarial enough.
- **B. Prior art, with code**: for each leading tool, a small snippet of
  how it is written and used (five to fifteen lines), then one or two lines
  on its design choice and what hd takes or rejects from it, with a source
  link.
- **C. Alternatives considered**: each in a few lines, and why not.
- **D. Today in hd**: the study of what exists now (current std, spec rules
  with links, probes on the current build). This is background and goes
  here, not at the top.
- **E. Details**: the domain checklist items below, one line each where
  possible, and the implementation phases (each about one hour, each with
  its test).

### Domain checklists (appendix E, one line each where possible)

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

- Could the owner understand the proposal and the decision asked from the
  first screen?
- Stdlib: is every public item used by an example, and does the package
  check and format?
- Does the doc open with the proposal, its key code and the open
  questions, with today's state at the back?
- Is the main body within about 150 lines?
- Is there a real stress test, and does every prior-art entry have a code
  snippet?
- Does the pull request hold only the doc, the package (for stdlib) and
  the index line, with no experiment or prototype folders?
- Does every illustrative problem work in the proposal?
- Is anything from today's behaviour dropped without being listed?
