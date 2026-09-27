# Grammar Audit Findings

Scope: [Lexical Structure](../../spec/01-lexical-structure.md),
[Grammar](../../spec/02-grammar.md), the grammar fragments in chapters 11 to
14, and the [reference parser](../../spec/reference-parser/). Roadmap item:
[Grammar Audit](../../future-work/ROADMAP.md#1-grammar-audit).

Every owner question (Q1 to Q17) and follow-up (B1 to B9) is decided and
applied; the record is the spec's Revision Notes GQ1 to GQ17 in
`spec/README.md`. The prototype's remaining gaps on those decisions are in
[`../README.md`](../README.md) (GQ4, GQ11, GQ14).

Severity:

- **High**: two readings with different meaning and no rule that picks one,
  or a tool that accepts or rejects code against explicit spec text.
- **Medium**: a rule exists but readers will misread the code, or two parts
  of the spec disagree.
- **Low**: consistency or ergonomics.

## Method

All tools are under [`tools/`](tools/). They import only Node built-ins and
`spec/`.

- [`tools/ambiguity.ts`](tools/ambiguity.ts) (wrapper: `tools/amb`) reads the
  `ebnf` fences of chapter 02, turns them into BNF, and runs an Earley parser
  that **counts derivations** instead of only accepting. For every input with
  more than one parse it prints each locally ambiguous node and its competing
  trees. Source text goes through the reference lexer, so layout tokens are
  the spec's.
  - `amb fixtures`: every accept fixture in `spec/conformance`.
  - `amb generate N SEED`: random derivations from the grammar, checked at
    the token level.
  - `amb firstfollow`: LL(1) FIRST/FOLLOW conflicts on `:`, `[`, `$`, `!`,
    `?`, `<`, `else`, `for`, `if`, `...`, and the layout tokens.
  - `amb cases FILE`: hand-written probes, each run through both the
    reference parser and the derivation counter. Only
    [`probes/reference.cases`](probes/reference.cases) remains; the probes
    for resolved findings were removed.
- Rerun the probes with
  `audit/grammar/tools/amb cases audit/grammar/probes/reference.cases`.

The audit found no unintended ambiguity in the fixtures: every ambiguity is
one the spec hands to name resolution. `:` has no LL(1) conflict at the
token level; the risk around `:` is entirely in layout.

## Resolved

Removed from this file: GR-01 to GR-09, GR-11 to GR-20, GR-22, GR-23, and
the keyword-set review. Each was fixed in the reference parser or the spec
text, or settled by one of Q1 to Q17 and K1 to K3. GR-10 items a to e are
fixed; item e was re-checked on 2026-09-26 (`f(x = 1, fn (y): y)` is now
rejected). Item g (`t.1_0`, probe R11) is gone: owner decision TUP-1 spells
tuple selection `t._1`, so both the spec and the reference parser reject
`t.1_0` as a `syntax-error`.

## Open Findings

### GR-10: Reference-parser heuristics disagree with the spec

Severity: Medium. Status: item f open, re-checked on 2026-09-27 with
`amb cases audit/grammar/probes/reference.cases` (probe R12).

| # | Input | Reference | Spec |
| - | ----- | --------- | ---- |
| f | dedent to an unused column inside a bracketed suite (R12) | `syntax-error` | `invalid-dedent` |

Also open: a multi-line parameter list with a default value
(`fn f(\n    x: i32 = 1,\n) -> void:`) reports `missing-let`, from the
line-based check in `spec/reference-parser/contextual.ts`.

### GR-21: `a?.b` and `x is T` look like Kotlin and Swift but mean something else

Severity: Low; no question. `a?.b` is propagation then member access, which
returns from the enclosing function on `.None`, not optional chaining. `x is
T`-style reading is wrong too: `is` is identity comparison, as in Python.
Teaching material should call this out.
