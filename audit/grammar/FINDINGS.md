# Grammar Audit Findings

Scope: [Lexical Structure](../../spec/lang/01-lexical-structure.md),
[Grammar](../../spec/lang/02-grammar.md), the grammar fragments in chapters 11 to
14, and the parser of the prototype compiler. It was a
[roadmap](../../future-work/ROADMAP.md) item before the roadmap was cut to one page.

Every owner question (Q1 to Q17) and follow-up (B1 to B9) is decided and
applied; the commit messages are the record. The prototype's remaining gap on those decisions, GQ4, is
in [`../README.md`](../README.md).

Severity:

- **High**: two readings with different meaning and no rule that picks one,
  or a tool that accepts or rejects code against explicit spec text.
- **Medium**: a rule exists but readers will misread the code, or two parts
  of the spec disagree.
- **Low**: consistency or ergonomics.

## Method

All tools are under [`tools/`](tools/). They import only Node built-ins,
`tools/`, and `spec/`, and they reach the compiler only through its command
line.

- [`tools/ambiguity.ts`](tools/ambiguity.ts) (wrapper: `tools/amb`) reads the
  `ebnf` fences of chapter 02, turns them into BNF, and runs an Earley parser
  that **counts derivations** instead of only accepting. For every input with
  more than one parse it prints each locally ambiguous node and its competing
  trees. Source text goes through the tool's own layout lexer
  ([`tools/lexer.ts`](tools/lexer.ts)), so layout tokens are the spec's.
  - `amb fixtures`: every accept fixture in `spec/conformance`.
  - `amb generate N SEED`: random derivations from the grammar, checked at
    the token level.
  - `amb firstfollow`: LL(1) FIRST/FOLLOW conflicts on `:`, `[`, `$`, `!`,
    `?`, `<`, `else`, `for`, `if`, `...`, and the layout tokens.
  - `amb cases FILE`: hand-written probes, each run through both the
    compiler's `parse` and the derivation counter. Only
    [`probes/reference.cases`](probes/reference.cases) remains; the probes
    for resolved findings were removed.
- Rerun the probes with
  `audit/grammar/tools/amb cases audit/grammar/probes/reference.cases`.

The audit found no unintended ambiguity in the fixtures: every ambiguity is
one the spec hands to name resolution. `:` has no LL(1) conflict at the
token level; the risk around `:` is entirely in layout.

## Resolved

Removed from this file: GR-01 to GR-09, GR-11 to GR-20, GR-22, GR-23, and
the keyword-set review. Each was fixed in the spec text or the tools, or
settled by one of Q1 to Q17 and K1 to K3. GR-10 tracked the heuristics of
the spec's reference parser, and closed when that parser was removed; its
one open item, probe R12, is GR-24 below.

## Open Findings

### GR-21: `a?.b` and `x is T` look like Kotlin and Swift but mean something else

Severity: Low; no question. `a?.b` is propagation then member access, which
returns from the enclosing function on `.None`, not optional chaining. `x is
T`-style reading is wrong too: `is` is identity comparison, as in Python.
Teaching material should call this out.

### GR-24: A dedent to an unused column inside brackets is a `syntax-error`

Severity: Medium. Status: open, carried over from GR-10 item f, and
re-checked on 2026-10-02 with
`amb cases audit/grammar/probes/reference.cases` (probe R12).

| Input | Compiler | Spec |
| ----- | -------- | ---- |
| dedent to an unused column inside a bracketed suite (R12) | `syntax-error` | `invalid-dedent` |
