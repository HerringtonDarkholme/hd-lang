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
| Error | `ambiguous-method`, `ambiguous-promoted-member`, `annotation-build-signature`, `annotation-resolution-reentry`, `annotation-top-level-read`, `argument-order`, `bang-call-outside-suspension`, `bare-parameter-impl-target`, `bare-variant-pattern`, `binding-not-yet-visible`, `break-value-context`, `closure-parameter-needs-annotation`, `comparison-chaining`, `decorator-not-annotator`, `decorator-not-top-level`, `default-order`, `direct-variant-use`, `discarded-must-use-value`, `doc-comment-without-target`, `duplicate-annotation-impl`, `duplicate-argument`, `duplicate-associated-binding`, `duplicate-data-pattern-field`, `duplicate-embedded-field`, `duplicate-field`, `duplicate-inherent-member`, `duplicate-module-name`, `duplicate-trait-member`, `entry-error-not-display`, `field-not-eq`, `field-not-hash`, `float-literal-range`, `function-impl-target`, `generic-kind-mismatch`, `generic-requirement-key-collision`, `identity-needs-reference-bound`, `identity-requires-references`, `implicit-narrowing`, `impossible-gadt-pattern`, `incompatible-identity-operands`, `integer-literal-range`, `invalid-assignment-target`, `invalid-escape`, `invalid-field-metadata`, `invalid-map-key`, `invalid-parameter-metadata`, `invalid-result-propagation`, `invalid-variance`, `local-impl-nonlocal-pair`, `missing-child-annotation`, `missing-contextual-enum-type`, `missing-derived-bound`, `missing-let`, `missing-partial-eq`, `missing-partial-ord`, `missing-required-field`, `missing-requirement`, `missing-result-type`, `missing-return-value`, `missing-supertrait-implementation`, `missing-trait-method`, `mixed-numeric-types`, `mixed-signedness`, `multi-binding-needs-parentheses`, `multiple-positional-value-packs`, `mutable-capture-requires-mut-fn`, `mutable-embedded-field`, `mutable-field-modifier`, `mutable-impl-target`, `mutable-receiver-required`, `mutable-upgrade`, `nil-to-nonoptional`, `no-common-type`, `no-least-common-type`, `non-reassignable-binding`, `non-reassignable-parameter-binding`, `nonexhaustive-match`, `nonfinal-positional-spread`, `nonfinal-positional-value-pack`, `nonfinal-vararg`, `nonhost-entry-requirement`, `nonnumeric-unary-plus`, `not-suspending`, `old-export-declaration`, `old-import-declaration`, `old-struct-declaration`, `optional-pattern-requires-optional`, `orphan-annotation-in-library`, `orphan-impl`, `overlapping-annotation-impl`, `overlapping-impl`, `pack-length-mismatch`, `pack-map-mapper-mismatch`, `partial-generic-arguments`, `pattern-arity`, `pattern-order`, `positional-spread-needs-vararg`, `possibly-uninitialized-binding`, `prelude-name-shadow`, `private-member`, `private-type-leak`, `readonly-argument-to-mutable-parameter`, `readonly-edge`, `readonly-root`, `recursive-closure-needs-result-type`, `recursive-function-needs-result-type`, `requirement-in-default`, `reserved-semicolon`, `return-outside-function`, `sealed-trait-implementation`, `supertrait-cycle`, `suspension-forbidden-context`, `tab-whitespace`, `top-level-read-before-initialization`, `trailing-block-position`, `trait-method-signature`, `trait-method-visibility`, `trait-not-dynamically-safe`, `trait-not-in-scope`, `type-used-as-value`, `unexpected-bom`, `unknown-annotation-member`, `unknown-associated-type`, `unknown-named-argument`, `unknown-shape-target`, `unreachable-match-arm`, `unrepresentable-type-shape`, `unresolved-generic-placeholder`, `unsatisfied-trait-bound`, `unsaturated-enum-constructor`, `unsigned-negation`, `unsupported-equality`, `unsupported-string-indexing`, `variance-representation-change`, `variant-result-owner` |
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
given readonly access), `Display` for string interpolation, and `Iterable` or
`Iterator` for a `for` loop. Because one code covers these origins, its
message must name the type, the missing trait, and where the requirement comes
from (the bound, the interpolation, or the loop).

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

## Revision Notes

These notes record language changes that alter the meaning or validity of
existing source. Each entry names the decision that made the change.

- K1: the `shape` keyword and the `shape(Target)` form were removed. The
  prelude intrinsics `shape[T]()` and `shape_of(f)` replace them; field and
  variant shapes are selected through typed `fields` and `variants` members,
  and the ordered lists became `field_list` and `variant_list`. `shape` is now
  an ordinary prelude name.
- K2: the `where` bound clause was removed; every bound is written inline in
  the generic parameter list. A bound may bind associated types, as in
  `I < Supplier[Item = T]`. `where` is now an ordinary identifier.
- K3: the logical operators `and`, `or`, and `not` became `&&`, `||`, and
  prefix `!` at the same precedence levels. The three words are now ordinary
  identifiers.
- GQ1: a requirement clause directly before a declaration's or closure's `:`,
  or a bodyless trait method's line end, belongs to that declaration. In
  `fn make() -> fn() -> i32 $ Console:`, `make` now requires `Console` and
  returns a plain `fn() -> i32`; the returned type needs parentheses to carry
  a row, as in `fn make() -> (fn() -> i32 $ Log) $ Console:`. A result such as
  `fn() -> i32 $ Log $ Console` is now a `syntax-error`. Inside types the row
  still attaches to the innermost ungrouped function type.
- GQ2: after an indented closure body inside brackets, the next line must
  start with `,` or a closing delimiter and be indented no farther than the
  line holding the closure header. A closing delimiter at the end of a body
  line, as in `apply(fn(a):` followed by `a + 1)`, and a header resuming
  directly after a closure body are now `syntax-error`.
- GQ5: data patterns label fields with `:`, as data literals do:
  `Point { x: 0, y }` and `Point { x: px }`. The former
  `Point { x = 0, y }` is now a `syntax-error`. Labels in parentheses (named
  arguments, variant payloads, and their patterns) keep `=`.
- GQ7: a line that starts with `.` and an identifier, indented farther than
  the logical line it follows, continues that line unless the line ends in
  `:` or `=>`. Leading-dot method chains, previously a syntax error, are
  valid.
- TQ-1: an implementation target must start with a type constructor; a bare
  type parameter target is `bare-parameter-impl-target`. Two implementations
  overlap when they share the trait, their trait arguments unify, and their
  targets share the type constructor; bounds are ignored, so
  `impl Tr for Box[i32]` and `impl Tr for Box[string]` now overlap. The
  prelude `impl[T, I < mut Iterator[T]] Iterable[T] for I` was removed: `for`
  loops and comprehensions accept `Iterator[T]` directly, and an iterator no
  longer satisfies an `Iterable[T]` bound.
- TQ-2: a trait implementation may also appear in the package that owns the
  outer type constructor of one of the trait's arguments, so a facet's package
  may write `annotate Facet for string`.
- TQ-3: an inherent method of the receiver's type wins over every trait
  method at a dot call; several trait candidates are `ambiguous-method`.
- TQ-5: an implementation target written with an outer `mut`, as in
  `impl Marker for mut Counter`, is now `mutable-impl-target`. It was
  previously legal alone and `overlapping-impl` beside
  `impl Marker for Counter`.
- TQ-6: a child trait may not declare a member with the name of a
  supertrait member (`duplicate-trait-member`).
- E1 to E5: member lookup on a nominal type `S` searches `S`'s own members
  (fields, inherent methods, and trait methods) first, and embedded fields
  only when `S` has no member with that name, whatever its kind or
  visibility. An outer trait method therefore now hides an embedded type's
  inherent method, and a name present only through a trait that is not
  available is `trait-not-in-scope` instead of reaching an embedded member.
  Promotion searches every depth, shortest path first; a same-depth clash is
  `ambiguous-promoted-member`, and a member that is present but not visible
  is `private-member`. An embedded type's trait methods are never promoted.
  A promoted method no longer fills a trait method: a bodyless implementation
  that relied on one is now `missing-trait-method`, and
  `promoted-mutable-requirement` was removed.
- M1: fields and methods share one member namespace per type. A field and an
  inherent method with the same name are `duplicate-inherent-member`; a field
  and a usable trait method with the same name make a call `x.name(args)`
  `ambiguous-method`; and `x.callback(args)` calls a function-typed field.
- M2 (replaces M1): fields and methods are separate namespaces, chosen by
  syntax. `x.name` looks up fields only and `x.name(args)` methods only. A
  field and an inherent method with the same name are now legal (previously
  `duplicate-inherent-member`); a field and a trait method with the same
  name no longer make a call `ambiguous-method`. A function-typed field is
  called as `(x.callback)(args)`; `x.callback(args)` is now a method call
  and `unknown-method` when there is no such method. An outer field no longer
  hides an embedded type's inherent method from a call, and an embedded
  type's field never answers a call.
- TQ-4: when a type implements one generic trait at several instantiations,
  a dot call selects the one instantiation whose method fits the arguments
  and expected type; it was always `ambiguous-method`.
- TQ-27 (partial): tuple types are valid implementation targets, with one
  standard-library constructor per arity. A function type as a target is
  `function-impl-target`.
- TQ-28: two implementations overlap only when their trait arguments and
  full targets unify, so `impl Tr for Box[i32]` and `impl Tr for Box[string]`
  no longer overlap; `impl[T] Tr for Box[T]` still overlaps both.
- TQ-29: a `for` loop or comprehension over a type that implements both
  `Iterable[T]` and `Iterator[T]` uses `Iterable[T]`.
- P2 (revises E2): member lookup skips fields and inherent methods that are
  not visible from the calling module. A use in another module whose own
  member is private, previously `private-member`, now reaches a visible
  promoted member; `private-member` is reported only when nothing visible is
  found. An own trait method whose trait is not available still stops the
  search with `trait-not-in-scope`.
