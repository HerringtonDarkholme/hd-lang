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
| [Annotations](14-annotations.md) | shapes, decorators, member metadata, typed derivation |

Deferred language design and the runtime, library, ABI, product, and tooling
backlog are tracked in [Open Issues](../future-work/OPEN_ISSUES.md).

Runtime and standard-library behavior that is not language semantics remains in
[`RUNTIME_AND_LIBRARY.md`](../future-work/RUNTIME_AND_LIBRARY.md). The language tour remains
the readable introduction; this directory is the formalization target.

Chapters are written to the [Specification Style](STYLE.md) guide.

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

### Normative Vocabulary

Chapters written in the [specification style](STYLE.md) use these words and
phrases with these meanings:

| Word or phrase | Meaning |
| --- | --- |
| **must**, **must not** | An absolute requirement on a program or an implementation. |
| **should**, **should not** | A recommendation. There may be valid reasons to depart from it, and doing so does not by itself make a program invalid. |
| **may** | A permission: the program or implementation is allowed, but not required, to do it. |
| **is an error**, **is a `code` error**, "Error: `code`." | The implementation diagnoses the program, as [Diagnostics](#diagnostics) defines. When a code is named, the diagnostic carries that code. |
| **is invalid**, **is rejected** | The implementation diagnoses the program. The rule names no specific code. |
| **Why**, **Note** | A callout that explains the rules around it. It adds no requirement. |

A rule may carry a stable **rule ID**, written `r[data.field.unique]` before
the rule. A link cites it by the anchor `r-` followed by the ID, as
[Rule IDs](STYLE.md#rule-ids) describes.

### Diagnostics

Unless a rule explicitly calls for a warning, **diagnose** means reject the
program with an error. Diagnostic prose and source spans may improve, but the
machine-readable code is stable. The table below is normative; `cases.tsv`
maps each exercised code to its fixture.

| Severity | Stable diagnostic codes |
| --- | --- |
| Error | `ambiguous-method`, `ambiguous-promoted-member`, `argument-order`, `bang-call-outside-suspension`, `bare-parameter-impl-target`, `bare-variant-pattern`, `binding-not-yet-visible`, `break-value-context`, `closure-parameter-needs-annotation`, `comparison-chaining`, `copy-into-ordinary-field`, `cyclic-test-dependency`, `decorator-not-annotator`, `decorator-not-top-level`, `default-order`, `deferred-method-value`, `direct-variant-use`, `discarded-must-use-value`, `doc-comment-without-target`, `duplicate-argument`, `duplicate-associated-binding`, `duplicate-data-pattern-field`, `duplicate-embedded-field`, `duplicate-fact`, `duplicate-field`, `duplicate-inherent-member`, `duplicate-module-name`, `duplicate-test-name`, `duplicate-tests-block`, `duplicate-trait-member`, `embedded-copy-required`, `embedded-non-data`, `embedding-too-deep`, `field-not-eq`, `field-not-hash`, `float-literal-range`, `gadt-derivation`, `generic-kind-mismatch`, `generic-member-call`, `generic-requirement-key-collision`, `identity-needs-reference-bound`, `identity-requires-references`, `implicit-narrowing`, `impossible-gadt-pattern`, `incompatible-identity-operands`, `inspectable-requirement`, `integer-literal-range`, `invalid-assignment-target`, `invalid-delegation`, `invalid-escape`, `invalid-map-key`, `invalid-member-line`, `invalid-result-propagation`, `invalid-test-statement`, `invalid-variance`, `local-impl-nonlocal-pair`, `marker-template`, `member-not-derivable`, `misplaced-derivation`, `misplaced-test-case`, `misplaced-tests-block`, `missing-contextual-enum-type`, `missing-derived-bound`, `missing-let`, `missing-partial-eq`, `missing-partial-ord`, `missing-required-field`, `missing-requirement`, `missing-result-type`, `missing-return-value`, `missing-supertrait-implementation`, `missing-trait-method`, `mixed-derived-law`, `mixed-numeric-types`, `mixed-signedness`, `multi-binding-needs-parentheses`, `multiple-positional-value-packs`, `mutable-embedded-field`, `mutable-field-modifier`, `mutable-impl-target`, `mutable-receiver-required`, `mutable-upgrade`, `newtype-derivation-self`, `no-common-type`, `no-least-common-type`, `non-literal-test-argument`, `non-reassignable-binding`, `non-reassignable-parameter-binding`, `nonexhaustive-match`, `nonfinal-positional-spread`, `nonfinal-positional-value-pack`, `nonfinal-vararg`, `nonhost-entry-requirement`, `nonlocal-impl`, `nonnumeric-unary-plus`, `not-suspending`, `old-export-declaration`, `old-import-declaration`, `old-row-operator`, `old-struct-declaration`, `omitted-member-without-default`, `orphan-impl`, `overlapping-impl`, `pack-length-mismatch`, `pack-map-mapper-mismatch`, `partial-generic-arguments`, `pattern-arity`, `pattern-order`, `positional-spread-needs-vararg`, `possibly-uninitialized-binding`, `prelude-name-shadow`, `private-member`, `private-type-leak`, `public-test-item`, `readonly-argument-to-mutable-parameter`, `readonly-edge`, `readonly-root`, `recursive-closure-needs-result-type`, `recursive-function-needs-result-type`, `requirement-in-default`, `reserved-semicolon`, `return-outside-function`, `sealed-trait-implementation`, `structure-outside-template`, `supertrait-cycle`, `suspension-forbidden-context`, `tab-whitespace`, `test-only-use`, `too-many-embedded-fields`, `top-level-read-before-initialization`, `trailing-block-position`, `trait-method-signature`, `trait-method-visibility`, `trait-not-dynamically-safe`, `trait-resolution-depth`, `trait-value-impl-target`, `type-used-as-value`, `underivable-trait`, `unexpected-bom`, `unknown-annotation-member`, `unknown-associated-type`, `unknown-named-argument`, `unknown-panic-category`, `unknown-shape-target`, `unreachable-match-arm`, `unresolved-generic-placeholder`, `unsatisfied-trait-bound`, `unsaturated-enum-constructor`, `unsigned-negation`, `unsupported-equality`, `unsupported-function-identity`, `unsupported-string-indexing`, `variance-representation-change`, `variant-result-owner` |
| Error | `defer-control-flow`, `defer-outside-cleanup-scope`, `suspending-defer` |
| Error (general) | `argument-count`, `break-outside-loop`, `duplicate-binding`, `duplicate-type`, `duplicate-variant`, `invalid-dedent`, `invalid-token`, `not-callable`, `syntax-error`, `type-mismatch`, `unclosed-delimiter`, `unexpected-indentation`, `unknown-data-field`, `unknown-method`, `unknown-name`, `unknown-trait`, `unknown-type`, `unknown-variant`, `unmatched-delimiter`, `unterminated-string` |
| Warning | `confusable-identifier`, `derivation-line-drift`, `mixed-script-identifier`, `unreachable-code`, `unused-derivation-fact`, `unused-local-binding`, `variant-binding-name-mismatch` |
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
| `unexpected-indentation` | A line is indented where layout does not open a suite, or the first body line of a nested suite inside brackets is not deeper than its header's line and the logical line containing the header. |
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
requirements: a generic bound (including `T < AnyVal`, `T < AnyRef`, and
`T < mut Trait` given readonly access), `Display` for string interpolation,
`Iterable` or `Iterator` for a `for` loop, `std.process.Termination` for
an entry point's or a test body's result, and `std.ops.LiteralSuffix` for a
suffixed literal. Because one code covers these origins, its message must
name the type, the missing trait, and where the requirement comes from (the
bound, the interpolation, the loop, the entry point, the test, or the
literal).

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

## Glossary

This glossary is a stub. It grows as chapters are restyled, and each entry
links to the rule that defines the term.

| Term | Definition |
| --- | --- |
| **copy-update literal** | A data literal with one leading spread, which builds a new value from an existing one. See [Copy-Update Literals](08-data-and-enums.md#copy-update-literals). |
| **data type** | A nominal product type with reference semantics. See [`data.kind.data`](08-data-and-enums.md#r-data.kind.data). |
| **derivation block** | An `impl Trait for X by Structure:` that applies a trait's template to one type, with optional member lines. See [Derivation Blocks](14-annotations.md#derivation-blocks). |
| **embedded field** | A bare type-name member of a data declaration, which embeds another data type. See [Data Embedding](08-data-and-enums.md#data-embedding). |
| **enum** | A nominal sum type. See [`data.kind.enum`](08-data-and-enums.md#r-data.kind.enum). |
| **fact** | An ordinary value attached to a type, member, or variant for derivations to read. See [Facts](14-annotations.md#facts). |
| **integration test module** | A module under the package's test root, which sees the package as a dependent does. See [`module.test.integration`](10-modules.md#r-module.test.integration). |
| **handle** | A compiler-generated constant naming one member (`Field[S, F]`) or variant (`Variant[S]`) of a derivation's target. See [Handles](14-annotations.md#handles). |
| **literal suffix** | A name written directly after a numeric literal's digits, which names a type implementing `std.ops.LiteralSuffix`. See [Literal Suffixes](01-lexical-structure.md#literal-suffixes). |
| **member line** | A line of a derivation block that edits one member's facts or omits it. See [Member Lines](14-annotations.md#member-lines). |
| **mutable edges** | What a data type has when it declares a direct `field: mut U`, or embeds a type that has mutable edges. See [`data.edge.definition`](08-data-and-enums.md#r-data.edge.definition). |
| **mutable requirement trait** | A trait that declares or inherits a `mut self` method; its providers always have mutable access. See [`req.mut.trait`](11-requirements-and-suspension.md#r-req.mut.trait). |
| **part** | The value an embedded field holds: the outer value's own copy of a value of the embedded type. See [Parts And Copies](08-data-and-enums.md#parts-and-copies). |
| **rule ID** | A stable dotted name for one normative rule. See [Rule IDs](STYLE.md#rule-ids). |
| **suffixed literal** | A numeric literal with a literal suffix, such as `250ms`, which calls the suffix's `from_literal`. See [Literal Suffixes](05-expressions.md#literal-suffixes). |
| **template** | A trait's one derived implementation, written `impl[T] Trait for T by Structure:` in the trait's module. See [Templates](14-annotations.md#templates). |
| **test case** | One test, registered by a call of the prelude function `it`, or one row of `it_each`, in test position. See [Test Cases](10-modules.md#test-cases). |
| **test code** | A package's `tests:` blocks, test modules, and integration test modules, compiled only by a test build. See [`module.test.code`](10-modules.md#r-module.test.code). |
| **test dependency** | A dependency that the manifest declares for test builds only. See [`module.test.dependency`](10-modules.md#r-module.test.dependency). |
| **test position** | The top level of a `tests:` block, a test module, or an integration test module, where test-case calls go. See [`module.testing.test-position`](10-modules.md#r-module.testing.test-position). |
| **test module** | A module whose file name ends in `_test.hd`. See [Test Modules](10-modules.md#test-modules). |
| **typed derivation** | Implementing a trait for a data type or enum from its members through the trait's template. See [Typed Derivation](14-annotations.md#typed-derivation). |

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
- GQ7 (refinement): a leading-dot line is now a `syntax-error` when a
  same-line suite is still open at the end of the logical line it would
  continue. After `f := fn(x): x`, a deeper `.len()` line previously joined
  the closure body.
- GQ3: the first body line of a suite nested inside delimiters must be
  indented farther than the first physical line of the logical line that
  contains its header, not only farther than the header's own line. A body
  level with or left of the enclosing statement is now a `syntax-error`.
- GQ4: `pack.map(` and `pack.map_list(` always form the pack operation. A
  local or parameter named `pack` no longer makes `pack.map(...)` a method
  call; `pack.map(5)` on such a value is now a `syntax-error`.
- GQ6: a backtick raw identifier such as `` `type` `` writes any reserved word
  as an identifier, usable as a field, member, named-argument label,
  parameter, or binding. Backticks were previously an `invalid-token`.
- GQ8: inside delimiters, a line whose first token is `(`, `[`, `{`, or `!`
  starts a new operand instead of a call, index, data-literal, or suspension
  call suffix on the previous line's last operand. A list written as `first`
  and then `[1]` on the next line, previously `first[1]`, is now a
  `syntax-error` for the missing comma.
- GQ9: a same-line `if` directly inside another same-line suite is now a
  `syntax-error`, including a same-line function, closure, `defer`, or loop
  body such as `fn f() -> i32: if c: 1 else: 2` or `defer: if flag: pass`.
  Parentheses nest one: `if a: (if b: 1 else: 2) else: 3`.
- GQ10: a trailing block call is valid as the right-hand side of `=`,
  `_ :=`, `return`, and `break`, as it already was after `:=` and `let`.
  Chained bindings with a multi-name pattern after the first, such as
  `a, b := c, d := fn() -> (i32, i32): (1, 2)`, are now a `syntax-error`.
- GQ11: `[` directly after `annotate` always opens generic parameters. A
  facet expression beginning with a list literal, previously accepted, is now
  a `syntax-error` unless parenthesized.
- GQ12: a bang call with explicit type arguments writes them after the `!`,
  matching the declaration: `all![i32, string](a, b)`,
  `identity.echo![i32](42)`, and `Identity::echo![i32](42)`. A method's
  explicit list must now be followed by an ordinary call, so the former
  `identity.echo[i32]!(42)` is no longer valid.
- GQ13: list literals accept suffix spreads, as in `[0, xs...]`; each spread
  inserts a list's elements in place.
- GQ14: annotation bodies accept `pass` alone on an indented line, as data
  bodies already did.
- GQ15: `"$self"` interpolates the receiver; it was previously a
  `syntax-error`. A `$` before any other reserved word, as in `"$true"`,
  stays a `syntax-error`.
- GQ17: `reified`, `super`, `as`, and `use` are no longer reserved words.
  Each keeps its meaning in its fixed position (generic parameters, use roots,
  use aliases, and use declarations or `$.use(...)`) and is an ordinary
  identifier elsewhere, so `resource.use(f)`, `fn use() -> void`, and a
  parameter named `as` are valid.
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
- O1 to O3, TQ-27 (rest): `T?` is exact sugar for the prelude enum
  `enum Option[T]: Some(value: T); None`, and `T??` is
  `Option[Option[T]]`. `nil` is no longer a keyword or literal (30 reserved
  words remain); it is an ordinary identifier, so old code using it reports
  `unknown-name`. The absent value is written `.None` or `Option.None`, and
  `.Some(value)` or `Option.Some(value)` builds a present one; `Option` is a
  new prelude name. A plain `T` still converts to `T?`, one layer only, so a
  `T` where `T??` is expected is now `type-mismatch` and needs `.Some(...)`.
  The optional patterns `value?` and `nil` were removed; optionals are
  matched with `.Some(pattern)` and `.None`, and a bare `None` pattern is
  `bare-variant-pattern`. `nil-to-nonoptional` was removed: `.None` without an
  expected optional type is `missing-contextual-enum-type`.
  `optional-pattern-requires-optional` was removed: `.Some(value)` against a
  non-enum type is `missing-contextual-enum-type`. `Option[T]` is an ordinary
  implementation target, so `impl Tr for string?` targets `Option[string]`
  and does not overlap `impl Tr for i32?`.
- P6: method values stay deferred, and their spellings are reserved.
  `Type::name` or `Trait::name` without an argument clause (the unbound
  method function) and `x::name` where `x` is a value (the bound method
  value), called or not, are `deferred-method-value` errors. Previously the
  first was a plain syntax error and a called `x::name(...)` failed as an
  unknown trait or type. `x.callback(args)` with only a function-typed field
  `callback` stays `unknown-method`, and its message should suggest
  `(x.callback)(args)`.
- TQ-4 follow-ups: when several instantiations of one generic trait fit a
  dot call, and exactly one fits with every integer literal argument at
  `i32` and every floating-point literal argument at `f64`, that one is
  selected; the call was `ambiguous-method`. The `type-mismatch` for a call
  that fits no instantiation lists the available instantiations.
- Trait value types as targets: `impl Marker for Display` or
  `impl Marker for Any` is now `trait-value-impl-target`.
- P2 (revises E2): member lookup skips fields and inherent methods that are
  not visible from the calling module. A use in another module whose own
  member is private, previously `private-member`, now reaches a visible
  promoted member; `private-member` is reported only when nothing visible is
  found. An own trait method whose trait is not available still stops the
  search with `trait-not-in-scope`.
- VE1 to VE4: embedding is value embedding. Filling an embedded field, in a
  literal, a copy-update, or a store, stores a copy of the value: its
  ordinary fields are copied shallowly and its embedded parts recursively, so
  two outer values never share a part, and a later change to the source is
  no longer seen through the outer value. Access through an embedded field
  follows its container: through a `mut` outer value, the embedded field, its
  promoted fields, and its promoted `mut self` methods have `mut` access, so
  a promoted `mut self` call on a `mut` receiver, previously
  `mutable-receiver-required`, is valid, and so is assigning a promoted
  field. An embedded field is no longer a readonly edge: a mutation through
  one reached from a readonly value is `readonly-root`. `let alias =
  post.Timestamps` on a `mut Post` now binds a `mut` alias. The copy of a
  readonly value whose type has direct `mut U` fields, at any embedded depth,
  is readonly, so using its literal as `mut T` is `mutable-upgrade`. An
  embedded field is now an invariant position for variance, so
  `data Holder[+T]: Box[T]` is `invalid-variance`.
- VE-S: every copy into an embedded part is written with `...`. A data
  literal fills an embedded field as `Timestamps: ...stamps`, even for a
  fresh literal, and an embedded field is assigned with the new token `...=`,
  as in `post.Timestamps ...= stamps`. The former `Timestamps: stamps` and
  `post.Timestamps = stamps` are now `embedded-copy-required`; `...` or `...=`
  on any other field is `copy-into-ordinary-field`. Copy-update keeps its
  leading spread. Source text in which `...` is immediately followed by `=`
  now lexes as `...=`.
- Second embedding review, point 3: a prefix `...` means copy only, and a
  suffix `...` always spreads. A provider-context spread is now written with
  a suffix, as in `$.with(Tag=x, ctx...)` and `$.context(base..., Clock=c)`;
  the former prefix form `$.with(...ctx)` is a `syntax-error`.
- TQ-31 revised: method lookup still never selects an embedded type's trait
  method, but a method name that an embedded type has only through a trait
  now stops the search at that depth. `page.to_string()`, where `Page`
  embeds `Label`, `Label` implements `Display` and embeds `Base`, and `Base`
  has an inherent `to_string`, previously selected `Base.to_string` and is
  now `embedded-trait-method-not-promoted`, suggesting
  `page.Label.to_string()`; without `Base` it was `unknown-method`. An
  inherent method beside such a blocking type at the same depth, previously
  selected, is now `ambiguous-promoted-member`.
- Embedded fields are always public: the `pub` marker on an embedded field
  was removed from the grammar, so `pub Base` in a data body is now a
  `syntax-error`, and an unmarked embedded field, previously module-private,
  is visible wherever its outer type is. Embedding a module-private data type
  in a public data type is now `private-type-leak`. A promoted member no
  longer needs every embedded field on its path to be visible, only itself.
- Private embedded members: a field or inherent method of an embedded type
  that is not visible from the calling module is now ignored entirely. A use
  that found only such a member, previously `private-member`, is now
  `unknown-data-field` or `unknown-method`; `private-member` is reported
  only for an invisible own member of the receiver's type.
- Rust-style trait lookup: a trait method is a method-call candidate only
  where its trait is available, wherever the implementation is declared. A
  trait method of the receiver's type whose trait is not available no longer
  stops the search: a call that was `trait-not-in-scope` now selects a
  promoted method of that name when there is one, and is `unknown-method`
  otherwise, with a message suggesting the use declaration. The code
  `trait-not-in-scope` was removed. An available trait method of the
  receiver's type no longer hides promoted methods: a call that selected it
  while an embedded type had a method of that name, as an inherent method or
  through an available trait, is now `ambiguous-method`. An embedded type
  blocks a name only through an available trait. Breadth-first promotion
  and shortest-path selection are unchanged.
- Embedding depth semantics and ignored part traits: a type's promoted
  members are its parts' fields and inherent methods at every depth, and for
  each name the shallowest member hides deeper ones, so every embedded type
  decides its own names. Two members with one name at the same smallest
  depth, including a type embedded twice at one depth, are now
  `ambiguous-promoted-member` at the outer type's data declaration, reported
  on the later of the two embedded fields involved, instead of at a use; a
  data type whose uses were previously ambiguous is now rejected even when
  nothing uses the name. In a public type the check also covers the view
  from other modules, where only public members take part. An embedded
  type's trait methods no longer stop the search: `page.to_string()`, where
  `Page` embeds `Label`, `Label` implements `Display` and embeds `Base`, and
  `Base` has an inherent `to_string`, previously
  `embedded-trait-method-not-promoted`, now calls `Base.to_string`; without
  `Base` it is `unknown-method`, and beside a promoted inherent method of
  another embedded type, previously `ambiguous-promoted-member`, it selects
  that method. An available trait method of the receiver beside an embedded
  type that has the name only through a trait, previously
  `ambiguous-method`, is now selected. The code
  `embedded-trait-method-not-promoted` was removed.
- Trait delegation: `impl Trait for C by E` implements `Trait` for `C` by
  forwarding every method with a receiver, including defaulted ones, to the
  embedded field `E`, whose type must implement the trait; the body may
  replace individual methods, associated types take the part's bindings, and
  associated functions are written in the body. `by` is a new contextual
  word after an implementation's target type, so `impl Trait for C by E`,
  previously a `syntax-error`, is accepted. The new code
  `invalid-delegation` covers a name that is not an embedded field, a part
  that does not implement the trait, and an associated type binding in a
  delegating body.
- Embedding limits: a data type may declare at most three embedded fields,
  and embedding chains may be at most three levels deep. A fourth embedded
  field, previously valid, is now `too-many-embedded-fields`; a part at
  depth 4, previously valid, is now `embedding-too-deep` at every data type
  that reaches it, including through generic data types and recursive
  embedding.
- Single view of a type's members: only `pub` fields and `pub` inherent
  methods of an embedded type are promoted, even when it is declared in the
  same module as the outer type. A use that reached a private member of a
  part in its own module, previously valid, is now `unknown-data-field` or
  `unknown-method`; the member is reached through the explicit path, as in
  `x.Part.secret`. Only a `pub` own member hides promoted members: a private
  own member with the name of a promoted member in its namespace, which
  previously won inside its module and was skipped elsewhere, is now
  `ambiguous-promoted-member`, reported on the private member. Every name
  resolves to the same member for every caller, so a type is checked once,
  and the separate check of a public type's view from other modules is
  gone. In the `Label`/`Base` example of Member Resolution,
  `page.Label.to_string()` is `ambiguous-method` between `Label`'s `Display`
  method and `Base`'s promoted `to_string`, as the rules already required;
  the example previously said it called `Label`'s `Display` method.
- VE-S in same-line suites: the inline statement form now accepts the copy
  assignment, as in `if fresh: post.Timestamps ...= stamps`. The grammar
  allowed `...=` only in a full assignment statement, while a plain `=` on
  an embedded field is `embedded-copy-required`, so a same-line suite could
  not store into an embedded field at all; that source, previously a
  `syntax-error`, is now accepted.
- Option follow-up A2: `.None` no longer gets a least-common-type special
  case. `[1, .None]` or `if c: 1 else: .None` without an expected type,
  previously typed `List[i32?]` or `i32?`, is now
  `missing-contextual-enum-type`, like any contextual variant; an expected
  type such as `List[i32?]` supplies the type.
- Option follow-up A3: optionals follow the ordinary enum rules. An optional
  value now erases to `Any`, which was previously a type error that required
  `Any?`. `is` now accepts optionals,
  previously `identity-requires-references`: `.None` is canonical, and each
  `.Some(value)` construction, including an implicit wrap, has its own
  identity. Optionals now implement `Reference`, and the implementation
  model lists them with the reference values, so a present value is a tagged
  record rather than its bare payload.
- Grammar follow-up B7: a nested suite body inside brackets that is not
  deeper than its header's line and the logical line containing the header,
  previously `syntax-error`, is now `unexpected-indentation`.
- Grammar follow-up B8: an unbackticked `reified` at the start of a generic
  parameter is always the modifier. `[reified]`, which previously declared a
  parameter named `reified`, is now a `syntax-error`; write `` [`reified`] ``.
- Prototype-fix follow-up C1: the codes for malformed operators and literals
  are pinned. `&`, `|`, or `^` on a non-integer and arithmetic on a
  non-numeric type are `type-mismatch`; a misplaced separator in a numeric
  literal, including after an exponent marker or sign as in `1e_5`, is
  `invalid-token`; `0b1z` and a bare `0x` are `syntax-error`.
- Prototype-fix follow-up C2: `2 ** -1` is stated to be a compile-time
  `type-mismatch`, because the negated literal is a signed exponent. The
  chapter previously called `2 ** -3` valid.
- Prototype-fix follow-up C3: an implementation method's generic parameters
  must match the trait method's by position, with the same markers and the
  same bounds in the same order; names may differ. The chapter previously
  did not say whether bounds were part of the exact signature; a method that
  adds, drops, reorders, or changes a bound is now `trait-method-signature`,
  reported at the implementation method.
- Collection naming: the built-in collection types are `List[T]` and
  `Map[K, V]`, capitalized like every other nominal type. Only primitive
  types such as `i32`, `bool`, and `string` keep lowercase names. The former
  prelude names `list` and `map` are now ordinary names, so `list[i32]` is an
  `unknown-type` error unless a declaration in scope supplies `list`. Literals
  are unchanged: `[1, 2]` has type `List[i32]` and `{"k": 1}` has type
  `Map[string, i32]`.
- Standard-library decision 14: a runtime profile may bind a host provider
  with `mut` access for a trait it marks mutable. An entry-point row may then
  contain `mut K` for that trait, as in `pub fn main() -> void $ mut Console`
  under a profile that marks `Console` mutable; such a row was previously a
  `mutable-upgrade` error under every profile. A `mut K` entry for a trait
  the profile does not mark mutable remains `mutable-upgrade`, and a
  registered boundary's row follows the access its registration contract
  binds.
- Error conversion (Error Conversion decisions 1 to 11): postfix `?` on a
  `Result` defines "accepts" as one step. The error propagates when it is
  assignable to the enclosing error type by one assignability rule;
  otherwise `?` calls that type's `From[E]` implementation once. The two
  steps never combine and conversions never chain. A `?` whose error was
  previously rejected for not matching exactly may now be accepted: an
  `FsError` that implements a dynamically safe trait `Tr` propagates into
  `Result[U, Tr]`, and an `FsError` propagates into `Result[U, SyncError]`
  when `SyncError` implements `From[FsError]`. Anything else stays
  `invalid-result-propagation`. The standard library declares the
  conversion trait `From[T]` in `std.convert` and the error trait `Error`
  (a `Display` subtrait whose members all have defaults) in `std.error`.
  Neither is a prelude name, so a module may still declare its own `From`
  or `Error`. A `from` with a requirement clause or `!` is
  `trait-method-signature`. The erased `Error` never crosses a registered
  boundary.
- Error conversion, decision 4: a dynamic trait value type satisfies a
  generic bound on its own trait and on its supertraits. A call such as
  `show(value)` with `value: Display` and `fn show[T < Display]`, previously
  `unsatisfied-trait-bound`, is valid, and `Result[void, Error]` is a valid
  entry-point result.
- Error conversion, decision 7: a variant constructor with exactly one
  payload field, written without an argument clause, is a function value of
  type `fn(P) -> Enum`. `ToolError.NotFound` with one payload, previously
  `unsaturated-enum-constructor`, is valid. A constructor with two or more
  payload fields stays `unsaturated-enum-constructor`. Postfix `?` has no
  mapping clause, and there are no anonymous error unions.
- Value-category traits (owner decision): the sealed prelude trait
  `Reference` is renamed `AnyRef`, and a new sealed prelude trait `AnyVal`
  is added. Both are compiler-implemented subtraits of `Any`, and every
  value type implements exactly one of them: `AnyVal` for primitives and
  tuples, `AnyRef` for the reference values. `T < Reference` is now
  `unknown-trait`; write `T < AnyRef`. `T < AnyVal` accepts only primitives
  and tuples. `AnyRef` replaces an existing prelude name. `AnyVal` is a
  prelude addition and a deliberate exception to Standard-library decision 7
  (the prelude does not grow), so a module that declares its own `AnyVal` or
  `AnyRef` is now `prelude-name-shadow`.
- Runtime type identity (Inspectable owner decisions 1 to 15): the standard
  module `std.inspect` declares the sealed trait `Inspectable`, with
  `fn runtime_type(self) -> TypeId`, the opaque `TypeId` with
  `TypeId::of[T < Inspectable]()`, the default methods
  `downcast[T < AnyRef + Inspectable](self) -> T?` and
  `downcast_mut[T < AnyRef + Inspectable](mut self) -> mut T?`, and the
  free function `downcast_val[T < Inspectable](value: Inspectable) -> T?`
  for value types (decision 15).
  The compiler supplies `Inspectable` for every inspectable type, and a value
  of such a type erases to `Inspectable` by assignability rule 6. Runtime
  identity is the declaration plus its type arguments, ignoring `mut` at
  every level. `std.error.Error` now extends `Inspectable`, so an
  `impl Error` for a type declared in a block suite, previously valid, is
  `missing-supertrait-implementation`. "Sealed" is defined: `Any`,
  `AnyVal`, `AnyRef`, `Suspend[T]`, `ShapeMetadata`, and `Inspectable` are
  sealed, and an `impl` of any of them outside the standard library, including
  `impl Any for X`, is `sealed-trait-implementation`, as is a member that
  redeclares a sealed supertrait's member (previously
  `duplicate-trait-member`). A trait that extends `Inspectable` as a
  requirement key is the new `inspectable-requirement`.
- Option variants (owner decision, confirming O2): `Some` and `None` are not
  prelude names. A bare `None` or `Some(value)` expression is `unknown-name`
  unless a declaration in scope supplies the name. An unqualified variant
  pattern with a payload list, such as `Some(value)`, is now
  `bare-variant-pattern`, matching the bare `None` pattern; it was previously
  unspecified.
- Result variants (owner decision, extending O2 to `Result`): `Result` is the
  ordinary prelude enum `enum Result[T, E]: Ok(value: T); Err(error: E)`, and
  `Ok` and `Err` are no longer prelude names. Results are built with
  `.Ok(value)`, `.Err(error)`, `Result.Ok(value)`, and `Result.Err(error)`
  (`.Ok()` for `Result[void, E]`) and matched with the same spellings. Old
  code is affected three ways: a bare `Ok(value)` or `Err(error)` expression
  is now `unknown-name`; a bare `Ok(pattern)` or `Err(pattern)` pattern is
  now `bare-variant-pattern`; and a module may now declare its own `Ok` or
  `Err`, previously `prelude-name-shadow`. `.Ok(value)` without an expected
  `Result` type is `missing-contextual-enum-type`.
- Runtime type identity, inner `mut` (Inspectable owner decision 16,
  revising decision 2): a `mut` inside a type argument is part of runtime
  identity, and only the outer `mut` of the erased view is ignored.
  `TypeId::of[List[mut User]]()` and `TypeId::of[List[User]]()`, previously
  equal, now differ, and so do their printable names, which now show the
  inner `mut`. An erased `List[User]` downcast to `List[mut User]`,
  previously `.Some`, is now `.None`. `TypeId::of[mut User]()` still equals
  `TypeId::of[User]()`.
- Promotion wording (owner decision, 2026-09-27): chapter 08 now says in
  every place that only `pub` fields and `pub` inherent methods of an
  embedded part are promoted. This restates the "Single view of a type's
  members" entry, so no source changes meaning.
- Readonly copy stores (owner decision, 2026-09-27): a store of a readonly
  copy is `mutable-upgrade` only where the store's target requires a mutable
  part: a `mut` field, parameter, or binding. A store into a readonly target
  is valid, and the stored value is readonly.
- Duplicate field declarations (owner decision, 2026-09-27): two fields with
  one name in one data type are `duplicate-field`, the code a data literal
  uses for a repeated field. The rule previously named no code.
- Embedded non-data types (owner decision, 2026-09-27): an embedded field
  must name a data type, a generic data type, or a transparent alias that
  resolves to one. Embedding an enum, a newtype, a trait value type, `Any`, a
  builtin or collection type, a function type, or a type parameter is now
  the new code `embedded-non-data`; chapter 08 previously named no code.
- Requirement rows as comma lists (owner decision, 2026-09-27): a row lists
  separate injected values, so several keys are written as a comma list. A
  declaration or closure header may list them bare, as in
  `fn load(id: UserId) -> User $ Db, Cache:`. Inside a type they are
  parenthesized, as in `fn(UserId) -> User $(Db, Cache)` and
  `$.Context[$(Metrics, Cache)]`. A single key may stay bare, and `$()` is
  still the empty row. The former `$ Db + Cache` and every `+` or `-` inside
  a row are now the new code `old-row-operator`; `+` keeps only its bound
  meaning, as in `T < A + B`. Row subtraction was removed. A callee removes a
  key by extension instead: a parameter `cb: fn() -> void $(R, Logger)` with
  the callee row `$ R` replaces `cb: fn() -> void $ R` with the callee row
  `$ (R - Logger)`. A callback
  whose row lacks the removed key, previously accepted with the
  `requirement-subtract-absent` warning, is now a `type-mismatch`, and that
  warning was withdrawn.
- Numeric member access (owner decision TUP-1, 2026-09-27): tuple elements
  are selected as `pair._0`, `pair._1`, and so on, and an enum's unnamed
  shared constructor parameters as `StatusCode.NotFound._0`. The member name
  is an ordinary identifier, so `t._0._1` needs no special lexing, and
  `"${pair._0}"` interpolates an element. The former spelling `pair.0`, and
  `t.0.1` with its floating-point token `0.1`, are now `syntax-error`; no new
  code was added. A leading-dot line such as `._0` now continues the
  previous line, while `.0` still does not.
- One equality trait (owner decision EQ-1, 2026-09-27): `std.cmp` has one
  equality trait, `Eq`, which declares `fn eq(self, other: Self) -> bool`;
  `PartialEq` was removed, so `T < PartialEq`, `impl PartialEq for X`, and
  `@derive(PartialEq)` are now `unknown-trait`, and `PartialEq` is no longer
  a prelude name. `PartialOrd` now extends `Eq`, and `Ord` extends only
  `PartialOrd`. Floating-point types now implement `Eq` with IEEE 754
  semantics, a documented exception to the reflexive law, so `f64`
  satisfies `T < Eq`, previously `unsatisfied-trait-bound`; they still
  implement neither `Hash` nor `Ord`, so they are still not map keys.
  `assert_equal` now requires `T < Eq`. The diagnostic code
  `missing-partial-eq` keeps its spelling and now reports a missing `Eq`.
- Law partners (owner decision TQ-12, 2026-09-26, revised by EQ-1): `Hash`
  and `PartialOrd` have `Eq` as law partner, and `Ord` has `Eq` and
  `PartialOrd`. Deriving `Hash`, `PartialOrd`, or `Ord` now requires each
  partner in the same `@derive` list, and a derived implementation beside a
  hand-written law partner is an error. `@derive(Hash)` alone, or beside a
  hand-written `Eq`, and `@derive(Eq)` beside a hand-written `Hash`, all
  previously valid, are now the new code `mixed-derived-law`.
- Derived newtypes (owner decision TQ-11, 2026-09-26): a newtype declaration
  may carry `@derive(...)`, and each derived implementation applies the base
  type's implementation to the wrapped value. `@derive(Eq, Hash)` before
  `type Mile(i32)`, previously a `syntax-error`, is valid. Other decorators
  on a newtype remain a `syntax-error`.
- Default method bodies (owner decision TY-13, 2026-09-26): a trait's
  default method body sees only the members of the trait and its
  supertraits. A default body that calls an inherent method of an
  implementing type is now `unknown-method`, and one that reads a field of
  an implementing type is rejected.
- Trait parameter variance (owner decision TQ-16, 2026-09-26): trait
  generic parameters are invariant, and a variance marker on one, as in
  `trait Source[+T]`, is now `invalid-variance`.
- Inner `mut` in implementation heads (owner decision TQ-30, 2026-09-26):
  `impl Store[User] for Shelf` and `impl Store[mut User] for Shelf` implement
  distinct instantiations and do not overlap; only a target's outer `mut`
  stays `mutable-impl-target`.
- Implementation modules (owner decision TQ-17, 2026-09-26): an inherent
  implementation must be in the module that declares its target type, and a
  trait implementation in a module that declares the trait, the target's
  constructor, or an owned trait argument's constructor. An implementation
  elsewhere in the owning package, previously valid, is now the new code
  `nonlocal-impl`.
- Inherent member names (owner decision TQ-19, 2026-09-26): inherent members
  with one name clash only when their implementations' targets unify.
  `impl Box[i32]` and `impl Box[string]` may now each declare `show`,
  previously `duplicate-inherent-member`; `impl[T] Box[T]` beside
  `impl Box[i32]` with one name stays `duplicate-inherent-member`.
- Associated function calls (owner decision TQ-9, 2026-09-26): `Type::f`
  looks for an inherent member first, then members of available traits that
  `Type` implements, and two trait candidates are `ambiguous-method`;
  `T::f` under a bound resolves through the bound, never a runtime type
  object; and `Trait::f(...)` whose `Self` inference does not determine is
  invalid. Existing source that compiled keeps its meaning.
- Dynamic safety (owner decision TQ-10, 2026-09-26, in part): a trait whose
  method, or a supertrait's method, declares a row parameter, a `reified`
  parameter, or a type or value pack is not dynamically safe; using it as a
  value type, previously accepted, is now `trait-not-dynamically-safe`.
- Least common type (owner decision TQ-15, 2026-09-26): inference never
  widens a dynamic trait value to a supertrait, so `[shown, tagged]` with two
  child traits of `Named` is `no-common-type` unless an expected type such as
  `List[Named]` is given.
- Bound depth (owner decision TQ-20, 2026-09-27): a bound proof that needs a
  bound nested more than 64 deep is the new code `trait-resolution-depth`.
  The limit is fixed by the specification.
- Complete type shapes (owner decision TQ-23, 2026-09-26): `TypeShape` gains
  `Mut(inner)`, `Trait(decl, args)`, `Any`, and `Suspend(result)`, and
  `Newtype(base)` became `Newtype(decl, base)`. A shape of a type that uses
  mutable access, a trait value, `Any`, or `Suspend[T]`, previously
  `unrepresentable-type-shape`, is valid, and that code was removed.
- Enum semantics (owner decisions 1 to 4, 2026-09-27): enums stay `AnyRef`
  with today's identity rule, and an enum value never changes once built.
  Shared constructor data is now a per-variant constant: each variant's
  `->` expression and omitted defaults are evaluated once at compile time,
  stored once per variant, and never in an enum value. Assigning a shared
  field, previously valid through a `mut` enum root, is now
  `invalid-assignment-target`. A `->` expression that names a payload
  parameter is now `unknown-name`. Two constructions of a payload-free
  variant whose enum declares shared data, previously distinct under `is`,
  are now the same canonical value. Payload-free enums get no automatic
  `Eq` or `Hash`: the chapter 04 statement that the standard library
  implements `Hash` for them was withdrawn, so such an enum needs
  `@derive(Eq, Hash)` to be a map key. The non-normative Implementation
  Model now describes one enum representation: `i31ref` tags for
  payload-free variants and one GC struct subtype per payload variant.
  Chapter 13's GADT example `IntBox(n: i64) -> Box[i64](n)`, which built
  shared data from the payload, is now written with constants.
- Dynamic safety revised (owner decision TQ-10, revised 2026-09-27): a
  dynamically safe trait's methods may be suspending, as `Console`'s
  `write_line!` already was, and may declare row parameters, whose providers
  pass as one bundle. A trait value type whose trait has a row-parameter
  method, `trait-not-dynamically-safe` since the TQ-10 entry above, is valid
  again. Only `reified` parameters and packs remain excluded. Chapter 09
  now states the defining one-copy rule (`trait.dyn.safe.one-copy`), and
  the other dynamic-safety rules are its consequences.
- Value categories of `void`, `never`, and newtypes (owner decisions VC-1
  to VC-3, 2026-09-27): `void` implements `AnyVal`, so `T < AnyVal` now
  accepts it; `never` implements neither `AnyVal` nor `AnyRef`, because it
  has no values; and a newtype has its base type's category, so
  `type Mile(i32)` satisfies `T < AnyVal` and `type Owner(User)` satisfies
  `T < AnyRef`, both previously unspecified. `downcast_val` keeps its
  `T < Inspectable` bound.
- Replay determinism (durable replay decisions 8, 12, and 13, 2026-09-26):
  the standard `Hasher` is seeded from the code identity and runtime
  profile, so its hash values, previously not stable across processes, are
  now stable within one code identity and runtime profile. Weak references
  and finalizers, previously deferred, may exist only inside the standard
  runtime, where user code cannot observe them. A runtime profile now
  includes the host's stack and memory limits.
- Standard-library inherent methods (standard library decision 8,
  2026-09-26): the standard library, which owns the built-in types, may now
  declare inherent implementations for primitives, built-in collection type
  constructors, and `Option`, in any of its modules. Their `pub` methods
  need no `use`. User code is unaffected: an inherent implementation for a
  type another package owns stays invalid.
- Function type constructors (FN_TYPE owner decision 1, 2026-09-27): every
  function type is an application of `Fn[(Is...), O, R]` or
  `SuspendFn[(Is...), O, R]`, whose arguments are the inputs as one tuple,
  the output, and the requirement row (`$()` when there is none). Existing
  function types keep their meaning. A spelled form whose inputs are not a
  tuple, such as `Fn[i32, i32, $()]`, is `generic-kind-mismatch`.
- `mut fn` removed (FN_TYPE owner decision 2, 2026-09-27): a closure may
  assign captured `let` storage and obtain mutable access from a captured
  `mut T` binding. Such a plain closure, previously
  `mutable-capture-requires-mut-fn`, is valid, and that code was removed.
  `mut fn` types and `mut fn` closure literals are now `syntax-error`, `mut`
  on a grouped or spelled function type is invalid, and calling a function
  value never needs mutable access.
- Exact function type sugar (FN_TYPE owner decision 3, 2026-09-27):
  `fn(A) -> O $ R` is the same type as `Fn[(A,), O, R]`, and
  `fn!(A) -> O $ R` as `SuspendFn[(A,), O, R]`. Either spelling is valid
  anywhere, diagnostics print the sugar, and only the spelled names need an
  import.
- Vararg marker (FN_TYPE owner decision 4, 2026-09-27): a vararg element is
  `Rest[T]` in constructor form, so `fn(string, i32...) -> i32` is
  `Fn[(string, Rest[i32]), i32, $()]`. `Rest[T]` is valid only as the final
  element of a function type's inputs; a non-final one is
  `nonfinal-vararg`, as a non-final `T...` already was.
- Declared function variance (FN_TYPE owner decision 5, 2026-09-27):
  function types are contravariant in each input element, covariant in the
  output, and invariant in the row. `fn() -> mut User` where
  `fn() -> User` is expected, and `fn(User) -> T` where `fn(mut User) -> T`
  is expected, previously `type-mismatch`, are now accepted. A conversion
  that is not representation-preserving, such as `fn() -> i32` to
  `fn() -> Display`, is `variance-representation-change`, previously
  `type-mismatch`. Chapter 04's function-invariance and container-view
  rules were withdrawn.
- Function types as implementation targets (FN_TYPE owner decision 6,
  2026-09-27): `impl Marker for fn(i32) -> i32`, previously
  `function-impl-target`, is valid in the package that declares `Marker`,
  and that code was removed. The standard library owns `Fn` and
  `SuspendFn`, so implementing a standard trait for a function type is
  `orphan-impl`. A row argument in an implementation head is a row
  parameter or a concrete row. This supersedes the function-type sentence
  of TQ-27 (partial) above.
- Function values stay out of `Inspectable` (FN_TYPE owner decision 7,
  2026-09-27): no behavior changes. The exclusion list now names `Fn` and
  `SuspendFn` too, since they are standard-library declarations.
- `std.function` (FN_TYPE owner decision 8, 2026-09-27): `Fn`, `SuspendFn`,
  and `Rest` are declared in the specified standard module
  `std.function`, which a conformance fixture may use. They are not prelude
  names, so a user declaration named `Fn` stays valid.
- Function identity (FN_TYPE owner decision 9, 2026-09-27): function values
  are `AnyRef`, but the identity of each one is unspecified, so an
  implementation may share or allocate them. `expr.is.closure`, which gave
  each closure evaluation one identity, was withdrawn. A direct `is` on a
  function-typed operand, previously valid, is the new code
  `unsupported-function-identity`. Generic code over `T < AnyRef` still
  compiles, with an unspecified result.
- Entry-point error chains (error conversion decision 13, 2026-09-27): when
  `main`'s error type implements `std.error.Error`, the host prints the
  message and then each cause as `caused by: ...`, instead of only
  `Display.to_string`. Other error types print as before.
- `?` in test blocks (error conversion decisions 14 and 16, 2026-09-27): a
  `test` block is a propagation target, as if it returned
  `Result[void, Error]`. A `?` there, previously
  `invalid-result-propagation`, is valid for an error that converts to the
  erased `Error`, and for any other `E < Display`, which is wrapped in a
  standard-library message error. A propagated `.Err` fails the test.
- Exit status (error conversion decision 17, 2026-09-27): `std.process`
  declares `ExitStatus`. When `main`'s error type implements it, a failing
  entry point exits with `error.status()` instead of 1.
- Erased errors at boundaries (error conversion decision 18, 2026-09-27): no
  rule changed. Chapter 10 notes that an error type holding an erased
  `Error` converts to an `ErrorReport` before crossing a boundary.
- Generic function values as arguments (error conversion decision 19,
  2026-09-27): a generic function or generic variant constructor passed as a
  call argument takes its type arguments from the call, so
  `result.map_err(TaskError.Failed)` is valid where it was
  `unresolved-generic-placeholder`. A parameter left unsolved is still that
  error. Declarations still write their own generic parameters and
  signatures.
- No non-exhaustive enums (error conversion decision 20, 2026-09-27): no
  rule changed. Chapter 08 lists a non-exhaustive enum form as unsupported.
- Typed derivation (owner decisions M1-M22 in
  [Typed Derivation](../future-work/TYPED_DERIVATION.md#owner-decisions),
  2026-09-27): `@derive` now also accepts any trait with a derivation
  template, `impl[T] Trait for T by Structure:` in the trait's module, and a
  derivation block `impl Trait for X by Structure:` applies one template with
  member lines (`f += [...]`, `f = [...]`, `f = pass`, `Self += [...]`).
  `std.structure` declares `Structure`, the `Field` and `Variant` handles,
  and the `Walker`, `Describer`, and `Source` traits. `@derive` of any other
  trait, including `Error`, is `underivable-trait`. A decorator before a data
  or enum declaration whose value is not a facet, previously an error, now
  attaches a type-level fact. `by Structure` is never a delegation. `+=` is
  a new token, so `a+=b`, previously `+` and `=`, lexes as one token and is
  still a `syntax-error` outside a member line. Enum payload parameters
  accept parameter decorators. New codes: `underivable-trait`,
  `misplaced-derivation`, `structure-outside-template`, `marker-template`,
  `invalid-member-line`, `duplicate-fact`, `omitted-member-without-default`,
  `member-not-derivable`, `generic-member-call`, `newtype-derivation-self`,
  and `gadt-derivation`; warnings `derivation-line-drift` and
  `unused-derivation-fact`; panic category `structure-variant-mismatch`.
  An implementation of `Walker`, `Describer`, or `Source` may strengthen its
  `member` bound, the one exception to `trait.impl.generics.fixed-bounds`.
- No orphan exception (owner decision, 2026-09-27): the root application
  package may no longer declare an orphan annotation. Such an annotation,
  previously valid in the root application and `orphan-annotation-in-library`
  in a library, is now `orphan-impl` in every package, and the code
  `orphan-annotation-in-library` was removed. A foreign target is annotated or
  derived through a local mirror type or a newtype.
- Requirement access from the trait (owner decision, 2026-09-27): a
  requirement trait that declares or inherits a `mut self` method is a
  mutable requirement trait, and its provider always has mutable access;
  every other provider is readonly. `$.use(K)` yields `mut K` for such a
  trait, and a `$.with` or `$.context` binding for it needs a `mut T`
  value, otherwise `mutable-upgrade`. The `mut` spellings `$ mut K`,
  `$.use(mut K)`, and `mut K=expression`, previously valid, are now
  `syntax-error`, and runtime profiles and registration contracts no longer
  mark traits mutable. This supersedes standard-library decision 14
  above. Calling a `mut self` method on `$.use(K)`, previously
  `mutable-receiver-required` without `mut`, is valid for a mutable
  requirement trait.
- Typed derivation M23 (owner decision in
  [Typed Derivation](../future-work/TYPED_DERIVATION.md#owner-decisions),
  2026-09-27): `by Structure` now needs `use std.structure.Structure`, like
  any other name. A derivation template or block without it, previously
  valid, is `unknown-trait`, and the `use` itself is not
  `structure-outside-template`. `by Structure` is still never a delegation.
  A call to `missing` through a generic source, previously
  `generic-member-call`, is valid; the code now covers `member` only. A
  derivation block on a newtype, previously unspecified, is
  `misplaced-derivation`: a newtype derives only through its base with
  `@derive`. `Clone` is a standard-library trait, listed in
  [STDLIB](../future-work/STDLIB.md#clone).
- Exit status code type (error entry-point follow-ups question 1,
  2026-09-27): `ExitStatus.status()` returns `std.process.StatusCode`, a
  wrapper of a `u8` that is never 0, built with
  `StatusCode::new(code: u8) -> StatusCode?`. An implementation that returned
  `i32` is now `trait-method-signature`.
- Exit status of erased errors (error entry-point follow-ups question 2,
  2026-09-27): no behavior changed. Chapter 10 now states that the rule reads
  the static error type, so `main() -> Result[void, Error]` exits with 1.
- Mutable `Console` (mutable host providers, owner decision 2026-09-27):
  `Console.write_line!` takes `mut self`, so `Console` is a mutable
  requirement trait and `$.use(Console)` yields `mut Console`. `println` and
  its `$ Console` row are unchanged. A user implementation of `Console` that
  declared `write_line!(self, ...)` is now `trait-method-signature`, and
  `console := $.use(Console)` followed by `console.write_line!(...)` is now
  `mutable-receiver-required`; bind with `let console: mut Console`.
- Facet protocol removed (Typed Derivation decision 10, 2026-09-27):
  `Annotation`, `Annotate`, `TypeAnnotator`, `DataAnnotator`,
  `EnumAnnotator`, `FuncAnnotator`, `FieldMetadata`, `VariantMetadata`,
  `ParamMetadata`, and `AnnotationRef` are no longer declared or in the
  prelude. `annotate Facet for Target`, previously valid, is a
  `syntax-error`, and `Facet::annotation(Target)` and
  `Facet::annotation_ref(Target)` are ordinary qualified calls with no
  special meaning; `annotate Target:` member blocks remain. A facet is an
  ordinary trait with an associated function derived through a template, as
  in `User::validator()`. Member and parameter metadata are typed
  `List[Any]`, so a value attached to a member of the wrong type, previously
  `invalid-field-metadata` or `invalid-parameter-metadata`, is valid. A
  decorator before a data or enum declaration always attaches a fact; before
  a function, every ordinary decorator is `decorator-not-annotator`. The
  codes `annotation-build-signature`, `annotation-resolution-reentry` (also
  a panic category), `annotation-top-level-read`,
  `duplicate-annotation-impl`, `overlapping-annotation-impl`,
  `missing-child-annotation`, `invalid-field-metadata`, and
  `invalid-parameter-metadata`, and the panic category
  `annotation-reference-unresolved` were removed.
- Exit codes (Testing T8, owner decision 2026-09-27): `std.process` declares
  `type ExitCode(u8)`, where 0 means success, and
  `trait Termination: fn report(self) -> ExitCode`, implemented by `void`,
  `ExitCode`, and `Result[T, E]` with `T < Termination` and `E < Display`.
  An `.Err` prints as before and exits with 1. The `ExitStatus` trait, the
  `StatusCode` type, and the static-error-type rule are removed, so code
  that implemented `ExitStatus` or named `StatusCode` is now `unknown-trait`
  or `unknown-type`; `main` returns `ExitCode` or `Result[ExitCode, E]` to
  pick a code. This supersedes error conversion decision 17 and error
  entry-point follow-ups questions 1 and 2.
- Entry results as a bound (Testing T5, owner decision 2026-09-27): an entry
  point's result type must implement `Termination`, as for an ordinary
  bound. A result that does not, previously `entry-error-not-display`, is
  now `unsatisfied-trait-bound`, and the code `entry-error-not-display` was
  removed. `main` may now also return `ExitCode` or `Result[ExitCode, E]`.
- Test body results (Testing T4, owner decision 2026-09-27): a `test` block's
  result type is inferred from its body like a closure's and must implement
  `Termination`; the test fails when that result reports a code other than
  0. `?` in a test block no longer converts to the erased `Error` and no
  longer wraps other `E < Display` errors, so a block that uses `?` now ends
  in a `Result[void, E]` value instead of a `void` statement. A result that
  does not implement `Termination`, such as an optional, is now
  `unsatisfied-trait-bound` instead of `invalid-result-propagation`. A
  test body's final `Result` or optional expression, previously
  `discarded-must-use-value`, is now the test's result. This supersedes
  error conversion decision 16.
- Tests blocks and `it` (Testing T2, T3, T7, T10, T11, T13, T16, T17, T22,
  and T23, owner decisions 2026-09-27): the `test "name":` item is removed,
  and `test` is an ordinary identifier again. A file holds at most one
  top-level `tests:` block, whose items are module items visible only inside
  it; `tests` is a reserved word. A test case is the call
  `it("name", ...):` of the prelude intrinsic `it`, with the literal options
  `ignore`, `expect_panic`, and `timeout`. Every top-level statement of a
  `tests:` block or a `_test.hd` test module must be such a call. A former
  `test "name":` block becomes `it("name"):` inside `tests:`, and code that
  named something `tests` or `it` must rename it. New codes:
  `duplicate-tests-block`, `invalid-test-statement`, `misplaced-test-case`,
  `non-literal-test-argument`, `duplicate-test-name`, `test-only-use`, and
  `cyclic-test-dependency`. The test-block `use` of Packages decision 5
  never reached this specification.
- Suspending trailing blocks (Testing T14, owner decision 2026-09-27): a
  trailing block passed for a parameter of type `fn!(...)` is a suspending
  closure, for every callee. A bang call in such a block, previously
  `bang-call-outside-suspension`, is now valid. The test block is no longer
  a separate driver context.
- Test body results (Testing T15, owner decision 2026-09-27): a trailing
  block given to `it` has result `Result[void, Error]` when it uses `?`, and
  `void` otherwise, instead of an inferred result. `?` on an error type that
  implements `Error` now converts into the erased `Error`. A body that
  propagated a `string` or other non-`Error` error, previously valid, is now
  `invalid-result-propagation`, and a body without `?` that ended in a
  `Result` value must end in a `void` statement. An explicit closure body
  keeps its own result, bounded by `Termination`. This supersedes the
  inferred result of Testing T4.
- Test outcomes (Testing T18, T19, T20, T21, and T28, owner decisions
  2026-09-27): `Termination.report` only computes the exit code, and the
  host or test runner prints an `.Err`. A failed assertion is always an
  `assertion-failed` panic, which ends the test case. A test case in a
  `tests:` block or a test module gets no host providers, so a requirement
  not supplied by `$.with` is `missing-requirement`. An integration test
  case's row is bound from the test run's one runtime profile, and one whose
  row that profile cannot bind is reported as skipped rather than rejected.
  Each test case still runs in its own fresh program instance, driven like
  `main!`.
- Table tests (Testing T31, owner decision 2026-09-27): `std.testing`
  declares `it_each(name, rows, body)`, which registers one test case per
  row, named `name[i]`. It is imported, not a prelude name, and a call of it
  is another statement a `tests:` block or a test module admits.
- Property tests (Testing T36, owner decision 2026-09-27): calls of
  `std.testing.it_prop` and `it_prop_with`, which register property test
  cases, are also admitted at the top level of a `tests:` block or a test
  module. Both are imported, not prelude names.
- Final function parameters after defaults (Testing T40, owner decision
  2026-09-27): a final parameter whose type is a function type may follow
  defaulted parameters, as in Kotlin and Swift; a trailing block or a named
  argument supplies it. `fn retry(times: i32, backoff: i32 = 100, body:
  fn() -> void)`, previously `default-order`, is now valid. `it` is
  therefore an ordinary `std.testing` function with defaulted options
  before `body`, not a compiler intrinsic.
- `Debug` (Testing T33, T39, and T48, owner decisions 2026-09-27):
  `std.format` declares the trait `Debug`, with `fn debug(self, out: mut
  DebugWriter) -> void`, and the function `debug(value) -> string`; both
  `Debug` and `debug` are prelude names, so a module that declared or bound
  either must rename it. `@derive(Debug)` derives it through its template.
  `assert_equal` now requires `T < Eq + Debug`, so comparing a type without
  `Debug`, previously valid, is `unsatisfied-trait-bound`.
- Test-case functions (Testing T41, T43, T47, and T50, owner decisions
  2026-09-27 and 2026-09-28): `it_each`, `it_prop`, and `it_prop_with` take
  a literal name and `it`'s options, and are called only in test position.
  `it_each` takes its options before `body`, so its body closure is now
  passed as `body=` rather than by position. Its rows are evaluated when
  the test runs, and another test case named `name[i]` is
  `duplicate-test-name`. A row or property body's result follows the test
  body rule. Any use of the four test-case functions other than a direct
  call in test position, including a use as a value, is
  `misplaced-test-case`. An `expect_panic` value that names no panic
  category, previously accepted, is the new error `unknown-panic-category`.
- Test code placement (Testing T44, T45, and T46, owner decisions
  2026-09-27): `pub` on an item inside a `tests:` block is the new error
  `public-test-item`. A test module or an integration test module that
  contains a `tests:` block is the new error `misplaced-tests-block`.
  Under `tests/`, `pkg` names the library's modules with their public
  declarations only, and the new use root `tests` names the other
  integration test modules, as in `use tests.common`.
- Snapshots (Testing T49, owner decision 2026-09-27): `std.testing` declares
  `snapshot(text, expect="")` and `snapshot_file(text)`. A non-literal
  `expect` is `non-literal-test-argument`.
- Literal suffixes (Literal Suffixes L1-L9, owner decisions 2026-09-28): a
  numeric literal may end in a suffix, as in `250ms`, `1.5kb`, or `0xff'B`,
  which calls `from_literal` of the suffix type's `std.ops.LiteralSuffix`
  implementation and has that implementation's `Out` type. The suffix is a
  module name, brought in by `use`; `std.time` declares `ns`, `us`, `ms`,
  `s`, `min`, and `h` for `Duration`, and the prelude declares none. A
  number directly followed by a letter, as in `5s`, previously a
  `syntax-error`, is now a suffixed literal; one followed by a reserved
  word, as in `5else`, previously two tokens, is now one suffixed literal
  whose suffix is unknown.
- Test timeouts (Testing, the note after T52, owner decision 2026-09-28):
  the `timeout` option of `it`, `it_each`, `it_prop`, and `it_prop_with` is
  a `Duration?`, written as a suffixed literal such as `timeout=5s` with
  `use std.time.s`. `timeout="5s"`, previously valid, is now
  `non-literal-test-argument`.
- The `tests` root outside `tests/` (Testing T52, owner decision
  2026-09-28): a use of the `tests` root outside an integration test module,
  including in a test module, is `test-only-use`. `DebugWriter` stays an
  imported name, not a prelude name.
