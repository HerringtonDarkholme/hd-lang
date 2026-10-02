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
| [Variadic Generics](12-variadic-generics.md) | none: hd has no packs; where arity-generic code lives instead |
| [GADTs](13-gadts.md) | variant result refinement and match typing |
| [Annotations](14-annotations.md) | decorators, function facts, member metadata, typed derivation, error derivation |

These chapters are the language tier. The stdlib tier, std APIs that
ordinary hd can implement, is specified in the
[Standard Library](std/README.md) chapters.

Deferred language design and the runtime, library, ABI, product, and tooling
backlog are tracked in [Open Issues](../future-work/OPEN_ISSUES.md).

Runtime and standard-library behavior that is not language semantics is
sketched in the archived
`RUNTIME_AND_LIBRARY.md`. The language tour remains
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
| Error | `alias-cycle`, `ambiguous-associated-type`, `ambiguous-method`, `ambiguous-promoted-member`, `ambiguous-row-pattern`, `argument-order`, `bang-call-outside-suspension`, `bare-parameter-impl-target`, `bare-variant-pattern`, `binding-not-yet-visible`, `break-value-context`, `closure-parameter-needs-annotation`, `comparison-chaining`, `copy-into-ordinary-field`, `cyclic-test-dependency`, `decorator-not-annotator`, `decorator-not-top-level`, `decorator-target-kind`, `default-order`, `derive-field-missing-trait`, `direct-variant-use`, `discarded-must-use-value`, `doc-comment-without-target`, `duplicate-argument`, `duplicate-associated-binding`, `duplicate-data-pattern-field`, `duplicate-embedded-field`, `duplicate-fact`, `duplicate-field`, `duplicate-inherent-member`, `duplicate-module-name`, `duplicate-pipe-placeholder`, `duplicate-test-name`, `duplicate-tests-block`, `duplicate-trait-member`, `embedded-copy-required`, `embedded-non-data`, `embedding-cycle`, `embedding-too-deep`, `float-literal-range`, `folder-cycle`, `gadt-derivation`, `generic-kind-mismatch`, `generic-member-call`, `generic-requirement-key-collision`, `identity-needs-reference-bound`, `identity-requires-references`, `implicit-narrowing`, `impossible-gadt-pattern`, `incompatible-identity-operands`, `inspectable-requirement`, `integer-literal-range`, `invalid-assignment-target`, `invalid-delegation`, `invalid-error-marker`, `invalid-escape`, `invalid-facts-of-target`, `invalid-impl-target`, `invalid-literal-suffix`, `invalid-map-key`, `invalid-member-line`, `invalid-result-propagation`, `invalid-string-prefix`, `invalid-test-statement`, `invalid-variance`, `let-else-falls-through`, `let-mut-readonly-type`, `local-impl-nonlocal-pair`, `marker-template`, `member-not-derivable`, `misplaced-derivation`, `misplaced-test-case`, `misplaced-tests-block`, `missing-contextual-enum-type`, `missing-derived-bound`, `missing-eq`, `missing-let`, `missing-partial-ord`, `missing-required-field`, `missing-requirement`, `missing-result-type`, `missing-return-value`, `missing-supertrait-implementation`, `missing-trait-method`, `mixed-derived-law`, `mixed-numeric-types`, `mixed-signedness`, `multi-line-pipe-step`, `mut-on-primitive`, `mut-on-tuple`, `mutable-embedded-field`, `mutable-field-modifier`, `mutable-impl-target`, `mutable-receiver-required`, `mutable-upgrade`, `newtype-derivation-self`, `no-common-type`, `no-least-common-type`, `non-literal-test-argument`, `non-reassignable-binding`, `non-reassignable-parameter-binding`, `nonexhaustive-match`, `nonfinal-positional-spread`, `nonfinal-vararg`, `nonhost-entry-requirement`, `nonlocal-impl`, `nonnumeric-unary-plus`, `not-suspending`, `old-bound-operator`, `old-export-declaration`, `old-import-declaration`, `old-row-separator`, `old-struct-declaration`, `omitted-member-without-default`, `orphan-impl`, `overlapping-impl`, `package-cycle`, `partial-generic-arguments`, `pattern-arity`, `pattern-order`, `pipe-placeholder-in-closure`, `pipe-step-needs-placeholder`, `placeholder-outside-pipe`, `positional-spread-needs-vararg`, `possibly-uninitialized-binding`, `prelude-name-shadow`, `private-member`, `private-type-leak`, `public-test-item`, `re-export-loop`, `readonly-argument-to-mutable-parameter`, `readonly-edge`, `readonly-root`, `recursive-closure-needs-result-type`, `recursive-function-needs-result-type`, `refutable-let-pattern`, `requirement-in-default`, `reserved-semicolon`, `return-outside-function`, `row-parameter-in-context`, `sealed-trait-implementation`, `structure-outside-template`, `supertrait-cycle`, `suspending-pipe-step`, `suspension-forbidden-context`, `tab-whitespace`, `test-only-use`, `too-many-embedded-fields`, `top-level-read-before-initialization`, `trailing-block-position`, `trait-method-signature`, `trait-method-visibility`, `trait-not-dynamically-safe`, `trait-resolution-depth`, `trait-value-impl-target`, `type-used-as-value`, `underivable-trait`, `unexpected-bom`, `unknown-annotation-member`, `unknown-associated-type`, `unknown-named-argument`, `unknown-panic-category`, `unreachable-match-arm`, `unresolved-generic-placeholder`, `unsatisfied-trait-bound`, `unsaturated-enum-constructor`, `unsigned-negation`, `unsupported-equality`, `unsupported-function-identity`, `variance-representation-change`, `variant-result-owner` |
| Error | `defer-control-flow`, `defer-outside-cleanup-scope`, `suspending-defer` |
| Error (general) | `argument-count`, `break-outside-loop`, `duplicate-binding`, `duplicate-type`, `duplicate-variant`, `invalid-dedent`, `invalid-token`, `not-callable`, `syntax-error`, `type-mismatch`, `unclosed-delimiter`, `unexpected-indentation`, `unknown-data-field`, `unknown-method`, `unknown-name`, `unknown-trait`, `unknown-type`, `unknown-variant`, `unmatched-delimiter`, `unterminated-string` |
| Warning | `confusable-identifier`, `derivation-line-drift`, `mixed-script-identifier`, `redundant-let-mut`, `unreachable-code`, `unused-derivation-fact`, `unused-local-binding`, `variant-binding-name-mismatch` |
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
| `argument-count` | A call supplies more or fewer arguments than the callee accepts, a `race!` call supplies no task, or a type-argument list has more positional arguments than its declaration has generic parameters. |
| `not-callable` | A call's callee is not a function, closure, constructor, or callable value. |
| `break-outside-loop` | `break` or `continue` appears outside a loop body. |

A diagnostic is reported on the line where the smallest construct that breaks
the rule begins. When a rule concerns one token of a larger construct, such as
a duplicate field name, the smallest construct is that token's own element.
Conformance fixtures place their marker on that line.

Note (normative): `unsatisfied-trait-bound` covers several trait
requirements: a generic bound (including `T < AnyVal`, `T < AnyRef`, and
`T < mut Trait` given readonly access), `Display` for string interpolation,
`Iterable` for a `for` loop, and `std.process.Termination`
for an entry point's or a test body's result. Because one code covers these
origins, its message must name the type, the missing trait, and where the
requirement comes from (the bound, the interpolation, the loop, the entry
point, or the test).

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

This glossary lists the terms that the numbered chapters define in bold.
Each entry links to the rule, or the section, that defines the term.
The stdlib chapters' terms are in the
[Standard Library glossary](std/README.md#glossary).

| Term | Definition |
| --- | --- |
| **active** | A driver context while its executor is evaluating or polling it on the current program-instance call stack. See [`req.bang.active`](11-requirements-and-suspension.md#r-req.bang.active). |
| **available** | A trait method is available in a module when its trait is available to dot-call lookup there. See [`names.visible.trait`](03-names-and-scopes.md#r-names.visible.trait). |
| **bare step** | A pipe step that is a name or path without `_`. See [`expr.pipe.step-kinds`](05-expressions.md#r-expr.pipe.step-kinds). |
| **bound method reference** | `value::name`, where `value` names a value, as a function value. See [`fn.ref.bound`](07-functions.md#r-fn.ref.bound). |
| **bound requirement key** | A requirement key that binds associated types, such as `Store[Item = User]`; its provider value has that trait value type. See [Bound Requirement Keys](11-requirements-and-suspension.md#bound-requirement-keys). |
| **call place** | A call `v()` whose callee's type implements `Update`, so `v() = x` and `v() op= x` store through it. See [Callable Values](05-expressions.md#callable-values). |
| **callable value** | A value whose type implements `Apply`, read by calling it with no arguments, as in `count()`. See [Callable Values](05-expressions.md#callable-values). |
| **coherence slot** | One `(trait, concrete target)` pair over the resolved package graph. See [Terminology](14-annotations.md#terminology). |
| **compatibility line** | The versions of a package that must stay compatible: one major number, or `0.MINOR` below 1.0. See [`module.version.line`](10-modules.md#r-module.version.line). |
| **compound assignment** | A statement `place op= value`, such as `total += x`, that combines an operator with a store. See [Compound Assignment](05-expressions.md#compound-assignment). |
| **conflict** | Two or more members with one name at the smallest depth where that name occurs, including one member reached through two paths. See [`names.conflict.definition`](03-names-and-scopes.md#r-names.conflict.definition). |
| **copy-update literal** | A data literal with one leading spread, which builds a new value from an existing one. See [Copy-Update Literals](08-data-and-enums.md#copy-update-literals). |
| **data type** | A nominal product type with reference semantics. See [`data.kind.data`](08-data-and-enums.md#r-data.kind.data). |
| **dependency requirement** | A manifest entry `PATH@VERSION` that maps a dependency key to a host path and a minimum version. See [Dependency Requirements](10-modules.md#dependency-requirements). |
| **depth** | The number of embedded fields on a part's path. See [`names.part.depth`](03-names-and-scopes.md#r-names.part.depth). |
| **derivation block** | An `impl Trait for X by Structure:` that applies a trait's template to one type, with optional member lines. See [Derivation Blocks](14-annotations.md#derivation-blocks). |
| **driver context** | Where a bang call is valid: a suspending function or closure body, or the host executor driving `main!`. See [`req.bang.driver-contexts`](11-requirements-and-suspension.md#r-req.bang.driver-contexts). |
| **dynamic provider** | A provider a closure gets at each call, because its row keeps the key. See [Lexical And Dynamic Providers](11-requirements-and-suspension.md#lexical-and-dynamic-providers). |
| **embedded field** | A bare type-name member of a data declaration, which embeds another data type. See [Data Embedding](08-data-and-enums.md#data-embedding). |
| **entry module** | The selected root module of an executable package. See [`module.init.entry-module`](10-modules.md#r-module.init.entry-module). |
| **enum** | A nominal sum type. See [`data.kind.enum`](08-data-and-enums.md#r-data.kind.enum). |
| **error derivation** | Implementing `Display`, `Error`, and `From` for an error type from its `@error` lines. See [Error Derivation](14-annotations.md#error-derivation). |
| **error type** | An enum with a bare `@error` line, or a data type with an `@error("...")` or `@error(transparent)` line. See [`annot.error.type`](14-annotations.md#r-annot.error.type). |
| **executable entry point** | A public top-level function named `main` or `main!` with no parameters. See [`module.entry.definition`](10-modules.md#r-module.entry.definition). |
| **exhausted** | An iterator whose `next` has returned `.None`. See [`flow.for.iterator-exhausted`](06-control-flow.md#r-flow.for.iterator-exhausted). |
| **exhausted iterator** | An iterator whose `next` has returned `.None`. What a later `next` returns is unspecified. See [`flow.for.iterator-exhausted`](06-control-flow.md#r-flow.for.iterator-exhausted). |
| **fact** | An ordinary value attached to a type, member, or variant for derivations to read. See [Facts](14-annotations.md#facts). |
| **field lookup** | The steps that resolve `x.name` to one field from a module. See [Field Lookup](03-names-and-scopes.md#field-lookup). |
| **fixed elements** | The elements of a tuple type other than its rest element. See [`types.tuple.rest.form`](04-type-system.md#r-types.tuple.rest.form). |
| **fits** | A candidate implementation fits a call when the call's arguments check against its method's parameter types. See [`trait.resolve.fits`](09-traits.md#r-trait.resolve.fits). |
| **folder** | The directory that holds a source file; `mod.hd` included, and nested directories are separate folders. See [`module.folder.directory`](10-modules.md#r-module.folder.directory). |
| **folder graph** | A package's folders, with an edge where a file in one folder uses a module in another. It must be acyclic. See [`module.cycle.folder-edge`](10-modules.md#r-module.cycle.folder-edge). |
| **generic field** | A field whose declared type is a generic parameter; reading it yields the substituted type unchanged. See [`types.path.field.generic`](04-type-system.md#r-types.path.field.generic). |
| **handle** | A compiler-generated constant naming one member (`Field[S, F]`) or variant (`Variant[S]`) of a derivation's target. See [Handles](14-annotations.md#handles). |
| **hides** | A member hides every member with the same name at a greater depth, in the same namespace. See [`names.hide.depth`](03-names-and-scopes.md#r-names.hide.depth). |
| **inherent associated function** | A member of an inherent implementation without a `self` parameter, called through the type, as in `User::guest()`. See [Inherent Members](09-traits.md#inherent-members). |
| **inherent method** | A member of an inherent implementation whose first parameter is `self` or `mut self`, called with dot syntax. See [Inherent Members](09-traits.md#inherent-members). |
| **initialization group** | A strongly connected component of the use graph: one module, or modules that use each other in a loop, initialized together. See [`module.init.group`](10-modules.md#r-module.init.group). |
| **inspectable types** | The types for which the compiler supplies `Inspectable`: primitives, module-level declarations, collections and tuples of inspectable types, and matching dynamic values. See [Inspectable Types](09-traits.md#inspectable-types). |
| **integration test module** | A module under the package's test root, which sees the package as a dependent does. See [`module.test.integration`](10-modules.md#r-module.test.integration). |
| **irrefutable** | A pattern that alone covers its initializer's type, so a `let` with it needs no `else`. Any other pattern is refutable. See [`flow.let.irrefutable`](06-control-flow.md#r-flow.let.irrefutable). |
| **iterable** | A value whose type implements `Iterable[T]`, such as a `List` or a `Map`. An `Iterator` is not iterable, though `for` takes a mutable one directly. See [`flow.for.accepts`](06-control-flow.md#r-flow.for.accepts). |
| **iterator** | A value of the prelude type `Iterator[T]`. It stores one traversal's progress and is single-pass: a second traversal calls `iter()` on the source again. See [`flow.for.iterator-type`](06-control-flow.md#r-flow.for.iterator-type). |
| **iterator adapters** | A stdlib term, in the [Standard Library glossary](std/README.md#glossary). |
| **known implementation** | An implementation in the program's dependency graph whose target matches a type; a local one counts only where its methods are available. See [`names.member.known-impl`](03-names-and-scopes.md#r-names.member.known-impl). |
| **law partners** | Comparison and hash traits whose laws relate them, such as `Hash` and `Eq`. See [Law Partners](09-traits.md#law-partners). |
| **let-else** | A `let` statement with a refutable pattern and an `else` block, which runs when the pattern does not match and must diverge. See [Let-Else Statements](02-grammar.md#let-else-statements). |
| **lexical provider** | A provider a closure fixes where it is written, by capturing the value of `$.use`. See [Lexical And Dynamic Providers](11-requirements-and-suspension.md#lexical-and-dynamic-providers). |
| **literal function** | A function marked `@num_suffix` or `@str_prefix`, which a suffixed literal or prefixed string calls. See [Literal Suffixes](05-expressions.md#literal-suffixes). |
| **literal suffix** | A name written directly after a numeric literal's digits, which names a suffix function. See [Literal Suffixes](01-lexical-structure.md#literal-suffixes). |
| **local type names** | The name category of data types, enums, traits, aliases, and newtypes declared inside an executable suite. See [`names.category.local-type`](03-names-and-scopes.md#r-names.category.local-type). |
| **member line** | A line of a derivation block that edits one member's facts or omits it, or a line of a trait-less derivation block that edits its metadata. See [Member Lines](14-annotations.md#member-lines). |
| **member metadata** | The ordered list of values attached to a data field, an enum variant, or a parameter. See [Terminology](14-annotations.md#terminology). |
| **member names** | The name category of data fields, embedded fields, methods, enum variants, and tuple fields, within the namespace of their owning type. See [`names.category.member`](03-names-and-scopes.md#r-names.category.member). |
| **method lookup** | The steps that resolve `x.name(args)` to an own inherent method or a candidate. See [Method Lookup](03-names-and-scopes.md#method-lookup). |
| **method reference** | A method or associated function named as a function value, written `Owner::name` or `value::name` without arguments. See [Method References](07-functions.md#method-references). |
| **minimal version selection** | Choosing, for each host path and compatibility line, the largest minimum that any reached manifest states. See [Version Selection](10-modules.md#version-selection). |
| **module names** | The name category of top-level types, traits, functions, and names introduced by use declarations. See [`names.category.module`](03-names-and-scopes.md#r-names.category.module). |
| **mutable edge** | A direct field declared `field: mut U`; a readonly container removes its `mut`. See [`types.path.field.mutable-edge`](04-type-system.md#r-types.path.field.mutable-edge). |
| **mutable edges** | What a data type has when it, or a type it embeds at any depth, declares a direct `field: mut U`. See [Mutable Edges](08-data-and-enums.md#mutable-edges). |
| **mutable requirement trait** | A trait that declares or inherits a `mut self` method; its providers always have mutable access. See [`req.mut.trait`](11-requirements-and-suspension.md#r-req.mut.trait). |
| **non-reassignable** | A binding whose name cannot be rebound. See [`types.view.non-reassignable`](04-type-system.md#r-types.view.non-reassignable). |
| **one-key row slot** | A place where one bare key may stand for a row: `$.Context[...]` or a row-kinded type argument written without `$`. See [`req.row.alias.one-key-slot`](11-requirements-and-suspension.md#r-req.row.alias.one-key-slot). |
| **operator trait** | A `std.ops` trait, such as `Add[Rhs = Self]`, whose implementation gives a type one operator. See [Operator Traits](05-expressions.md#operator-traits). |
| **part** | The value an embedded field holds: the outer value's own copy of a value of the embedded type. See [Parts And Copies](08-data-and-enums.md#parts-and-copies). |
| **path requirement** | A manifest value `{ path = "DIR" }` through which a workspace member depends on another member. See [`module.workspace.path-requirement`](10-modules.md#r-module.workspace.path-requirement). |
| **pipe expression** | `value \|> step`, which passes a value to a step. See [Pipe Expressions](05-expressions.md#pipe-expressions). |
| **place expression** | An expression that identifies a storage location, which may be read or, when permissions allow, assigned. See [`expr.category.place`](05-expressions.md#r-expr.category.place). |
| **positional spread** | An argument `x...` that passes the value `x` in place of separate arguments: a tuple fills the callee's remaining inputs, and a vararg takes a value of its own type. See [Positional Spreads](05-expressions.md#positional-spreads). |
| **prefix function** | A literal function marked `@str_prefix`, which a prefixed string calls. See [`expr.literal-fn.marker`](05-expressions.md#r-expr.literal-fn.marker). |
| **prefixed string** | An identifier followed directly by `"` or `"""`, as in `sql"..."`. See [`lex.prefix.form`](01-lexical-structure.md#r-lex.prefix.form). |
| **prelude** | The implicit scope of public standard-library names that every module has. See [Prelude](10-modules.md#prelude). |
| **primitive types** | `bool`, the integer types `i8` to `i64` and `u8` to `u64`, `f32`, `f64`, `char`, and `string`. See [Primitive Types](04-type-system.md#primitive-types). |
| **program instance** | One instantiated Wasm module graph with its module storage, provider bindings, and execution state. See [`module.init.program-instance`](10-modules.md#r-module.init.program-instance). |
| **promoted candidate** | Among the promoted inherent methods that take part, the one with the called name at the smallest depth. See [`names.method-lookup.promoted-candidate`](03-names-and-scopes.md#r-names.method-lookup.promoted-candidate). |
| **promoted member** | A `pub` field or `pub` inherent method of a part's type, reached from the outer type through the part's path. See [`names.promote.member`](03-names-and-scopes.md#r-names.promote.member). |
| **pseudo-version** | A version that names one untagged commit by a base version, its time, and its hash. See [`module.version.pseudo`](10-modules.md#r-module.version.pseudo). |
| **readonly edge** | A field declared `field: U` with a composite `U`, which gives readonly access through any container. See [`types.path.field.readonly-edge`](04-type-system.md#r-types.path.field.readonly-edge). |
| **readonly view** | The `T` access to a composite value; it does not imply deep immutability. See [`types.view.term`](04-type-system.md#r-types.view.term). |
| **refutable** | A pattern that may fail to match its initializer, such as `.Some(v)`; a `let` with one needs an `else` block. See [`flow.let.irrefutable`](06-control-flow.md#r-flow.let.irrefutable). |
| **requirement row** | The normalized unordered set of requirement keys on a callable signature. See [`req.row.definition`](11-requirements-and-suspension.md#r-req.row.definition). |
| **requirement-free** | A default expression that uses no provider and does not suspend. See [`fn.default.requirement-free`](07-functions.md#r-fn.default.requirement-free). |
| **rest element** | A last tuple element `List[T]...`, which stands for any number of trailing `T` values. See [Rest Elements](04-type-system.md#rest-elements). |
| **rest member** | The one `List[T]` member that a rest tuple's `Structure` has for its rest element. See [`annot.tuple.rest`](14-annotations.md#r-annot.tuple.rest). |
| **row alias** | A transparent alias that names a set of requirement keys. See [Row Aliases](11-requirements-and-suspension.md#row-aliases). |
| **row parameter** | A generic parameter whose values are requirement rows. See [`req.row.parameter`](11-requirements-and-suspension.md#r-req.row.parameter). |
| **rule ID** | A stable dotted name for one normative rule. See [Rule IDs](STYLE.md#rule-ids). |
| **runtime identity** | Two types share it when they are the same declaration applied to type arguments with the same runtime identity. See [`trait.identity.definition`](09-traits.md#r-trait.identity.definition). |
| **runtime profile** | A named compile-time set of host capability traits, their boundary adapters, and runtime choices such as panic exit statuses. See [`module.profile.definition`](10-modules.md#r-module.profile.definition). |
| **scalar boundary** | A byte offset of a string, from `0` to its length, that does not fall inside a scalar value's encoding. See [`types.string.boundary`](04-type-system.md#r-types.string.boundary). |
| **script** | An entry module with no `main`, whose top-level executable statements are the entry behavior. See [`module.init.script`](10-modules.md#r-module.init.script). |
| **sealed trait** | A standard trait whose implementations only the compiler and the standard library supply. See [Sealed Traits](09-traits.md#sealed-traits). |
| **self reference** | A member's or variant's `self_ref`: whether its type needs the type being derived (`.Required`), only refers to it (`.Optional`), or neither (`.Absent`), computed from its type alone. See [Self References](14-annotations.md#self-references). |
| **shape** | In generic code, the machine representation a value occupies. See [Shapes and Generic Code](04-type-system.md#shapes-and-generic-code). |
| **spread pattern** | A last tuple-pattern element, a name or `_` followed by `...`, that matches a rest element's list, as in `let (a, xs...) = t`. See [Spread Patterns](06-control-flow.md#spread-patterns). |
| **substitution step** | A pipe step that contains `_`. See [`expr.pipe.step-kinds`](05-expressions.md#r-expr.pipe.step-kinds). |
| **suffix function** | A literal function marked `@num_suffix`, which a suffixed literal calls. See [`expr.literal-fn.marker`](05-expressions.md#r-expr.literal-fn.marker). |
| **suffixed literal** | A numeric literal with a literal suffix, such as `250ms`, which calls the suffix function, as `ms(250)`. See [Literal Suffixes](05-expressions.md#literal-suffixes). |
| **take part** | The members of a type that lookup considers: its own fields and inherent methods, whatever their visibility, and its promoted members. See [`names.take-part.definition`](03-names-and-scopes.md#r-names.take-part.definition). |
| **template** | A trait's one derived implementation, written `impl[T] Trait for T by Structure:` in the trait's module. See [Templates](14-annotations.md#templates). |
| **tuple template** | A trait's derivation for every tuple type, written `impl[T < Tuple] Trait for T by Structure:` in the trait's module. See [Tuple Templates](14-annotations.md#tuple-templates). |
| **test case** | One test, registered by a call of the prelude function `it`, or one row of `it_each`, in test position. See [Test Cases](10-modules.md#test-cases). |
| **test code** | A package's `tests:` blocks, test modules, and integration test modules, compiled only by a test build. See [`module.test.code`](10-modules.md#r-module.test.code). |
| **test dependency** | A dependency that the manifest declares for test builds only. See [`module.test.dependency`](10-modules.md#r-module.test.dependency). |
| **test module** | A module whose file name ends in `_test.hd`. See [Test Modules](10-modules.md#test-modules). |
| **test position** | The top level of a `tests:` block, a test module, or an integration test module, where test-case calls go. See [`module.testing.test-position`](10-modules.md#r-module.testing.test-position). |
| **test registration function** | `it`, or a registration function that `std.testing` declares in the stdlib tier; only a direct call of one may stand in test position. See [`module.testing.position-statements`](10-modules.md#r-module.testing.position-statements). |
| **trait candidates** | The trait methods of the receiver's type with the called name whose trait is available at the call. See [`names.method-lookup.trait-candidates`](03-names-and-scopes.md#r-names.method-lookup.trait-candidates). |
| **trait methods** | The methods of every trait that a known implementation implements for a type. See [`names.member.trait-methods`](03-names-and-scopes.md#r-names.member.trait-methods). |
| **trait-less derivation block** | An `impl X by Structure:` without a trait, whose member lines write shared metadata of `X` for every derivation. See [Trait-Less Derivation Blocks](14-annotations.md#trait-less-derivation-blocks). |
| **type forms** | The kinds of type that hd-lang has, such as primitive types, tuples, optional types, and function types. See [Type Forms](04-type-system.md#type-forms). |
| **type-argument default** | A type written with `=` after a generic parameter's bound, used when a use site leaves the parameter unsolved or a written type omits it. See [Type-Argument Defaults](04-type-system.md#type-argument-defaults). |
| **type-argument marker** | The `::` before an explicit type-argument list in an expression, as in `first::[string](names)`. See [`grammar.expr.type-arguments.marker`](02-grammar.md#r-grammar.expr.type-arguments.marker). |
| **typed derivation** | Implementing a trait for a data type or enum from its members through the trait's template. See [Typed Derivation](14-annotations.md#typed-derivation). |
| **typed fact type** | A fact type whose `@annotate` decorator writes a type argument, as in `@annotate::[T](.Field)`, so a value on a target checks against the fact type at the target's type. See [`annot.typed-fact.declare`](14-annotations.md#r-annot.typed-fact.declare). |
| **unbound method reference** | `Owner::name` without an argument clause, where `Owner` names a type, a trait, or a type parameter. See [`fn.ref.unbound`](07-functions.md#r-fn.ref.unbound). |
| **untyped fact type** | A fact type whose `annotate` type argument is the default `Any`, as in `@annotate(.Field)`, so its values stay unchecked. See [`annot.typed-fact.untyped`](14-annotations.md#r-annot.typed-fact.untyped). |
| **value expression** | An expression that produces a value. See [`expr.category.value`](05-expressions.md#r-expr.category.value). |
| **value names** | The name category of top-level executable bindings, parameters, local bindings, local named functions, loop bindings, pattern bindings, and captured values. See [`names.category.value`](03-names-and-scopes.md#r-names.category.value). |
| **vararg** | A final parameter written `name...: T`, which collects the call's remaining positional arguments into `T`. See [`fn.vararg.form`](07-functions.md#r-fn.vararg.form). |
| **visible** | A field or inherent method is visible from a module that declares it, and from every module when it is `pub`. See [`names.visible.field-method`](03-names-and-scopes.md#r-names.visible.field-method). |
| **workspace** | A set of packages that one committed workspace manifest lists, selected as one graph. See [Workspaces](10-modules.md#workspaces). |
