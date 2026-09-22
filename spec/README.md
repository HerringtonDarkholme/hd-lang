# hd-lang Specification

This directory contains the normative language specification.

Core chapters describe the stable language. Material under `provisional/` is
still under design and is not part of the stable core specification.

## Contents

| Chapter | Scope |
| --- | --- |
| [Lexical Structure](01-lexical-structure.md) | source text, tokens, indentation, literals |
| [Grammar](02-grammar.md) | consolidated EBNF |
| [Names and Scopes](03-names-and-scopes.md) | declarations, bindings, imports, member lookup |
| [Type System](04-type-system.md) | types, coercions, `mut`, generics, variance |
| [Expressions](05-expressions.md) | evaluation, operators, calls, literals, comprehensions |
| [Control Flow](06-control-flow.md) | blocks, conditionals, loops, matching, return |
| [Functions](07-functions.md) | parameters, closures, captures, trailing blocks |
| [Structs and Enums](08-structs-and-enums.md) | aggregate declaration, construction, embedding |
| [Traits](09-traits.md) | conformance, methods, static and dynamic dispatch |
| [Modules](10-modules.md) | packages, imports, visibility, entry points, Wasm boundary |

Provisional chapters:

| Chapter | Unstable surface |
| --- | --- |
| [Requirements and Suspension](provisional/requirements-and-suspension.md) | requirement rows, providers, `fn!`, `Suspend[T]` |
| [Variadic Generics](provisional/variadic-generics.md) | type/value packs and pattern expansion |
| [GADTs](provisional/gadts.md) | variant result refinement and match typing |
| [Annotations](provisional/annotations.md) | shapes, metadata, derivation, overrides, recursion |

The remaining decisions needed to finish the draft are tracked in the
[Specification Completion Register](OPEN_ISSUES.md).

Runtime and standard-library behavior that is not language semantics remains in
[`RUNTIME_AND_LIBRARY.md`](../RUNTIME_AND_LIBRARY.md). The language tour remains
the readable introduction; this directory is the formalization target.

Future parser and type-checker cases live in
[Conformance Fixtures](conformance/README.md). Run the repository-local
specification checks with:

```sh
spec/check.sh
```

## Conformance Language

The key words **must**, **must not**, **should**, **should not**, and **may** are
normative. Text marked as a note or example is explanatory unless it explicitly
states otherwise.

## Grammar Notation

The specification uses EBNF for lexical and syntactic grammar. In grammar
productions:

- `name = expression ;` defines a production.
- Quoted text denotes a literal token.
- `A, B` denotes concatenation.
- `A | B` denotes alternatives.
- `[A]` denotes an optional expression.
- `{A}` denotes zero or more repetitions.
- `(A)` groups expressions.
- `"a" ... "z"` denotes an inclusive character range.
- Uppercase names such as `NEWLINE`, `INDENT`, `DEDENT`, and `SUITE_END` denote
  tokens produced by lexical or layout processing.

The chapter [Grammar](02-grammar.md) contains the consolidated grammar. Other
chapters repeat the productions relevant to the feature they specify. If a
repeated production conflicts with the consolidated grammar, the conflict is a
specification defect rather than an intentional precedence rule.

## Specification Status

The files currently form a draft. A rule is normative only when its chapter
states it as a requirement. Explicitly open issues are not implementation
freedom to guess silently: an implementation must diagnose unsupported syntax
until the issue is resolved by a later specification revision.

Unresolved decisions are recorded in the completion register or in the owning
provisional chapter. The stable-core draft is not complete until every issue
required for parsing, type checking, or execution has either been specified or
deliberately moved out of the supported language version.
