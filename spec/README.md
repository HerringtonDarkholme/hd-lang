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
| Error | `ambiguous-method`, `ambiguous-promoted-member`, `annotation-build-signature`, `annotation-resolution-reentry`, `annotation-top-level-read`, `argument-order`, `bang-call-outside-suspension`, `bare-parameter-impl-target`, `bare-variant-pattern`, `binding-not-yet-visible`, `break-value-context`, `closure-parameter-needs-annotation`, `comparison-chaining`, `copy-into-ordinary-field`, `decorator-not-annotator`, `decorator-not-top-level`, `default-order`, `deferred-method-value`, `direct-variant-use`, `discarded-must-use-value`, `doc-comment-without-target`, `duplicate-annotation-impl`, `duplicate-argument`, `duplicate-associated-binding`, `duplicate-data-pattern-field`, `duplicate-embedded-field`, `duplicate-field`, `duplicate-inherent-member`, `duplicate-module-name`, `duplicate-trait-member`, `embedded-copy-required`, `embedding-too-deep`, `entry-error-not-display`, `field-not-eq`, `field-not-hash`, `float-literal-range`, `function-impl-target`, `generic-kind-mismatch`, `generic-requirement-key-collision`, `identity-needs-reference-bound`, `identity-requires-references`, `implicit-narrowing`, `impossible-gadt-pattern`, `incompatible-identity-operands`, `inspectable-requirement`, `integer-literal-range`, `invalid-assignment-target`, `invalid-delegation`, `invalid-escape`, `invalid-field-metadata`, `invalid-map-key`, `invalid-parameter-metadata`, `invalid-result-propagation`, `invalid-variance`, `local-impl-nonlocal-pair`, `missing-child-annotation`, `missing-contextual-enum-type`, `missing-derived-bound`, `missing-let`, `missing-partial-eq`, `missing-partial-ord`, `missing-required-field`, `missing-requirement`, `missing-result-type`, `missing-return-value`, `missing-supertrait-implementation`, `missing-trait-method`, `mixed-numeric-types`, `mixed-signedness`, `multi-binding-needs-parentheses`, `multiple-positional-value-packs`, `mutable-capture-requires-mut-fn`, `mutable-embedded-field`, `mutable-field-modifier`, `mutable-impl-target`, `mutable-receiver-required`, `mutable-upgrade`, `no-common-type`, `no-least-common-type`, `non-reassignable-binding`, `non-reassignable-parameter-binding`, `nonexhaustive-match`, `nonfinal-positional-spread`, `nonfinal-positional-value-pack`, `nonfinal-vararg`, `nonhost-entry-requirement`, `nonnumeric-unary-plus`, `not-suspending`, `old-export-declaration`, `old-import-declaration`, `old-struct-declaration`, `orphan-annotation-in-library`, `orphan-impl`, `overlapping-annotation-impl`, `overlapping-impl`, `pack-length-mismatch`, `pack-map-mapper-mismatch`, `partial-generic-arguments`, `pattern-arity`, `pattern-order`, `positional-spread-needs-vararg`, `possibly-uninitialized-binding`, `prelude-name-shadow`, `private-member`, `private-type-leak`, `readonly-argument-to-mutable-parameter`, `readonly-edge`, `readonly-root`, `recursive-closure-needs-result-type`, `recursive-function-needs-result-type`, `requirement-in-default`, `reserved-semicolon`, `return-outside-function`, `sealed-trait-implementation`, `supertrait-cycle`, `suspension-forbidden-context`, `tab-whitespace`, `too-many-embedded-fields`, `top-level-read-before-initialization`, `trailing-block-position`, `trait-method-signature`, `trait-method-visibility`, `trait-not-dynamically-safe`, `trait-value-impl-target`, `type-used-as-value`, `unexpected-bom`, `unknown-annotation-member`, `unknown-associated-type`, `unknown-named-argument`, `unknown-shape-target`, `unreachable-match-arm`, `unrepresentable-type-shape`, `unresolved-generic-placeholder`, `unsatisfied-trait-bound`, `unsaturated-enum-constructor`, `unsigned-negation`, `unsupported-equality`, `unsupported-string-indexing`, `variance-representation-change`, `variant-result-owner` |
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
and `Iterable` or `Iterator` for a `for` loop. Because one code covers these
origins, its message must name the type, the missing trait, and where the
requirement comes from (the bound, the interpolation, or the loop).

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
