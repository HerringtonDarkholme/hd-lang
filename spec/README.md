# hd-lang Specification

This directory contains the normative language specification.

The numbered chapters describe one evolving language specification. Features
are either specified, explicitly unsupported, or listed in
[Open Issues](../future-work/OPEN_ISSUES.md); hd-lang does not divide the language into
versioned subsets.

## Contents

| Chapter | Scope |
| --- | --- |
| [Lexical Structure](01-lexical-structure.md) | source text, tokens, indentation, literals |
| [Grammar](02-grammar.md) | consolidated EBNF |
| [Names and Scopes](03-names-and-scopes.md) | declarations, bindings, use declarations, member lookup |
| [Type System](04-type-system.md) | types, coercions, `mut`, generics, variance |
| [Expressions](05-expressions.md) | evaluation, operators, calls, literals, comprehensions |
| [Control Flow](06-control-flow.md) | blocks, conditionals, loops, matching, return |
| [Functions](07-functions.md) | parameters, closures, captures, trailing blocks |
| [Data Types and Enums](08-data-and-enums.md) | aggregate declaration, construction, embedding |
| [Traits](09-traits.md) | conformance, methods, static and dynamic dispatch |
| [Modules](10-modules.md) | packages, use declarations, visibility, entry points, Wasm boundary |
| [Requirements and Suspension](11-requirements-and-suspension.md) | requirement rows, providers, `fn!`, `Suspend[T]` |
| [Variadic Generics](12-variadic-generics.md) | type/value packs and pattern expansion |
| [GADTs](13-gadts.md) | variant result refinement and match typing |
| [Annotations](14-annotations.md) | shapes, metadata, derivation, overrides, recursion |

Deferred language design and the runtime, library, ABI, product, and tooling
backlog are tracked in [Open Issues](../future-work/OPEN_ISSUES.md).

Runtime and standard-library behavior that is not language semantics remains in
[`RUNTIME_AND_LIBRARY.md`](../future-work/RUNTIME_AND_LIBRARY.md). The language tour remains
the readable introduction; this directory is the formalization target.

Parser and type-checker cases live in
[Conformance Fixtures](conformance/README.md). Run the repository-local
specification checks with:

```sh
spec/check.sh
```

## Conformance Language

The key words **must**, **must not**, **should**, **should not**, and **may** are
normative. Text marked as a note or example is explanatory unless it explicitly
states otherwise.

### Diagnostics

Unless a rule explicitly calls for a warning, **diagnose** means reject the
program with an error. Diagnostic prose and source spans may improve, but the
machine-readable code is stable. The table below is normative; `cases.tsv`
maps each exercised code to its fixture.

| Severity | Stable diagnostic codes |
| --- | --- |
| Error | `ambiguous-method`, `annotation-build-signature`, `annotation-resolution-reentry`, `annotation-top-level-read`, `argument-order`, `bang-call-outside-suspension`, `bare-variant-pattern`, `binding-not-yet-visible`, `break-value-context`, `closure-parameter-needs-annotation`, `comparison-chaining`, `decorator-not-annotator`, `decorator-not-top-level`, `default-order`, `direct-variant-use`, `discarded-must-use-value`, `doc-comment-without-target`, `duplicate-annotation-impl`, `duplicate-argument`, `duplicate-data-pattern-field`, `duplicate-embedded-field`, `duplicate-field`, `duplicate-inherent-member`, `duplicate-module-name`, `duplicate-trait-member`, `entry-error-not-display`, `field-not-eq`, `field-not-hash`, `float-literal-range`, `generic-kind-mismatch`, `generic-requirement-key-collision`, `identity-needs-reference-bound`, `identity-requires-references`, `implicit-narrowing`, `impossible-gadt-pattern`, `incompatible-identity-operands`, `integer-literal-range`, `invalid-assignment-target`, `invalid-escape`, `invalid-field-metadata`, `invalid-map-key`, `invalid-parameter-metadata`, `invalid-result-propagation`, `invalid-variance`, `local-impl-nonlocal-pair`, `missing-child-annotation`, `missing-contextual-enum-type`, `missing-derived-bound`, `missing-let`, `missing-partial-eq`, `missing-partial-ord`, `missing-required-field`, `missing-requirement`, `missing-result-type`, `missing-return-value`, `missing-supertrait-implementation`, `missing-trait-method`, `mixed-numeric-types`, `mixed-signedness`, `multi-binding-needs-parentheses`, `multiple-positional-value-packs`, `mutable-capture-requires-mut-fn`, `mutable-embedded-field`, `mutable-field-modifier`, `mutable-receiver-required`, `mutable-upgrade`, `nil-to-nonoptional`, `no-common-type`, `no-least-common-type`, `non-reassignable-binding`, `non-reassignable-parameter-binding`, `nonexhaustive-match`, `nonfinal-positional-spread`, `nonfinal-positional-value-pack`, `nonfinal-vararg`, `nonhost-entry-requirement`, `nonnumeric-unary-plus`, `not-suspending`, `old-export-declaration`, `old-import-declaration`, `old-struct-declaration`, `optional-pattern-requires-optional`, `orphan-annotation-in-library`, `orphan-impl`, `overlapping-annotation-impl`, `overlapping-impl`, `pack-length-mismatch`, `pack-map-mapper-mismatch`, `partial-generic-arguments`, `pattern-arity`, `pattern-order`, `positional-spread-needs-vararg`, `possibly-uninitialized-binding`, `prelude-name-shadow`, `private-type-leak`, `promoted-mutable-requirement`, `readonly-argument-to-mutable-parameter`, `readonly-edge`, `readonly-root`, `recursive-closure-needs-result-type`, `recursive-function-needs-result-type`, `requirement-in-default`, `reserved-name`, `reserved-semicolon`, `return-outside-function`, `sealed-trait-implementation`, `supertrait-cycle`, `suspension-forbidden-context`, `tab-whitespace`, `top-level-read-before-initialization`, `trailing-block-position`, `trait-method-signature`, `trait-method-visibility`, `trait-not-dynamically-safe`, `type-used-as-value`, `unexpected-bom`, `unknown-annotation-member`, `unknown-named-argument`, `unknown-shape-target`, `unreachable-match-arm`, `unrepresentable-type-shape`, `unresolved-generic-placeholder`, `unsatisfied-trait-bound`, `unsaturated-enum-constructor`, `unsigned-negation`, `unsupported-equality`, `unsupported-string-indexing`, `variance-representation-change`, `variant-result-owner` |
| Error | `defer-control-flow`, `defer-outside-cleanup-scope`, `suspending-defer` |
| Error (general) | `argument-count`, `break-outside-loop`, `duplicate-binding`, `duplicate-type`, `duplicate-variant`, `invalid-dedent`, `invalid-token`, `not-callable`, `syntax-error`, `type-mismatch`, `unclosed-delimiter`, `unexpected-indentation`, `unknown-data-field`, `unknown-method`, `unknown-name`, `unknown-trait`, `unknown-type`, `unknown-variant`, `unmatched-delimiter`, `unterminated-string` |
| Warning | `confusable-identifier`, `mixed-script-identifier`, `requirement-subtract-absent`, `unreachable-code`, `unused-local-binding`, `variant-binding-name-mismatch` |
| Runtime panic | The complete closed category list is defined in [Control Flow](06-control-flow.md#runtime-panics). |
| Boundary failure | `boundary-cycle`, `boundary-decoder-panic` |

The general error codes cover failures that every front end detects but that
no single chapter owns. Each applies only when no more specific code in the
table describes the failure:

| Code | Meaning |
| --- | --- |
| `syntax-error` | Source does not match the grammar, and no more specific lexical or layout code applies. |
| `invalid-token` | A character sequence forms no token. |
| `unterminated-string` | A string or character literal reaches the end of its line or file without closing. |
| `unclosed-delimiter` | An opening `(`, `[`, or `{` has no matching close. |
| `unmatched-delimiter` | A closing `)`, `]`, or `}` has no matching open, or closes a different kind. |
| `unexpected-indentation` | A line is indented where layout does not open a suite. |
| `invalid-dedent` | A dedent returns to a column that no enclosing suite uses. |
| `unknown-name` | A value name resolves to no binding in scope. |
| `unknown-type` | A type name resolves to no type in scope. |
| `unknown-trait` | A trait name resolves to no trait in scope. |
| `unknown-method` | Member lookup finds no method or field with that name. |
| `unknown-data-field` | A data literal, pattern, or field access names a field the data type does not declare, or a variant construction or pattern names a payload field the enum variant does not declare. |
| `unknown-variant` | An enum variant name resolves to no variant of the expected enum. |
| `duplicate-binding` | One scope declares the same binding name twice where the language forbids it. |
| `duplicate-type` | One module declares the same type name twice. |
| `duplicate-variant` | One enum declares the same variant name twice. |
| `type-mismatch` | A value's type is not assignable to the type its context requires. |
| `argument-count` | A call supplies more or fewer arguments than the callee accepts. |
| `not-callable` | A call's callee is not a function, closure, or constructor. |
| `break-outside-loop` | `break` or `continue` appears outside a loop body. |

A diagnostic is reported on the line where the smallest construct that breaks
the rule begins. When a rule concerns one token of a larger construct, such as
a duplicate field name, the smallest construct is that token's own element.
Conformance fixtures place their marker on that line.

Note (normative): `unsatisfied-trait-bound` covers several trait
requirements: a generic bound (including `T < Reference` and `T < mut Trait`
given readonly access), `Display` for string interpolation, and `Iterable` for
a `for` loop. Because one code covers these origins, its message must name the
type, the missing trait, and where the requirement comes from (the bound, the
interpolation, or the loop).

Identifier-security warnings use Unicode confusable skeletons and a
moderately-restrictive mixed-script profile. They do not change identifier
identity and do not reject a program unless a package warning policy promotes
them.

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

Unresolved decisions are recorded in [Open Issues](../future-work/OPEN_ISSUES.md). The
draft is not complete until every issue required for parsing, type checking, or
execution has either been specified or explicitly classified as unsupported or
runtime and library work.
