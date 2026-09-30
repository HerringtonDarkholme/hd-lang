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
| [Annotations](14-annotations.md) | shapes, decorators, member metadata, typed derivation, error derivation |

These chapters are the language tier. The stdlib tier, std APIs that
ordinary hd can implement, is specified in the
[Standard Library](std/README.md) chapters.

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
| Error | `alias-cycle`, `ambiguous-associated-type`, `ambiguous-method`, `ambiguous-promoted-member`, `ambiguous-row-pattern`, `argument-order`, `bang-call-outside-suspension`, `bare-parameter-impl-target`, `bare-variant-pattern`, `binding-not-yet-visible`, `break-value-context`, `closure-parameter-needs-annotation`, `comparison-chaining`, `copy-into-ordinary-field`, `cyclic-test-dependency`, `decorator-not-annotator`, `decorator-not-top-level`, `decorator-target-kind`, `default-order`, `derive-field-missing-trait`, `direct-variant-use`, `discarded-must-use-value`, `doc-comment-without-target`, `duplicate-argument`, `duplicate-associated-binding`, `duplicate-data-pattern-field`, `duplicate-embedded-field`, `duplicate-fact`, `duplicate-field`, `duplicate-inherent-member`, `duplicate-module-name`, `duplicate-pipe-placeholder`, `duplicate-test-name`, `duplicate-tests-block`, `duplicate-trait-member`, `embedded-copy-required`, `embedded-non-data`, `embedding-too-deep`, `float-literal-range`, `folder-cycle`, `gadt-derivation`, `generic-kind-mismatch`, `generic-member-call`, `generic-requirement-key-collision`, `identity-needs-reference-bound`, `identity-requires-references`, `implicit-narrowing`, `impossible-gadt-pattern`, `incompatible-identity-operands`, `inspectable-requirement`, `integer-literal-range`, `invalid-assignment-target`, `invalid-delegation`, `invalid-error-marker`, `invalid-escape`, `invalid-impl-target`, `invalid-literal-suffix`, `invalid-map-key`, `invalid-member-line`, `invalid-result-propagation`, `invalid-string-prefix`, `invalid-test-statement`, `invalid-variance`, `let-mut-readonly-type`, `local-impl-nonlocal-pair`, `marker-template`, `member-not-derivable`, `misplaced-derivation`, `misplaced-test-case`, `misplaced-tests-block`, `missing-contextual-enum-type`, `missing-derived-bound`, `missing-eq`, `missing-let`, `missing-partial-ord`, `missing-required-field`, `missing-requirement`, `missing-result-type`, `missing-return-value`, `missing-supertrait-implementation`, `missing-trait-method`, `mixed-derived-law`, `mixed-numeric-types`, `mixed-signedness`, `multi-binding-needs-parentheses`, `multi-line-pipe-step`, `multiple-positional-value-packs`, `mut-on-primitive`, `mutable-embedded-field`, `mutable-field-modifier`, `mutable-impl-target`, `mutable-receiver-required`, `mutable-upgrade`, `newtype-derivation-self`, `no-common-type`, `no-least-common-type`, `non-literal-test-argument`, `non-reassignable-binding`, `non-reassignable-parameter-binding`, `nonexhaustive-match`, `nonfinal-positional-spread`, `nonfinal-positional-value-pack`, `nonfinal-vararg`, `nonhost-entry-requirement`, `nonlocal-impl`, `nonnumeric-unary-plus`, `not-suspending`, `old-bound-operator`, `old-export-declaration`, `old-import-declaration`, `old-row-separator`, `old-struct-declaration`, `omitted-member-without-default`, `orphan-impl`, `overlapping-impl`, `pack-length-mismatch`, `pack-map-mapper-mismatch`, `package-cycle`, `partial-generic-arguments`, `pattern-arity`, `pattern-order`, `pipe-placeholder-in-closure`, `pipe-step-needs-placeholder`, `placeholder-outside-pipe`, `positional-spread-needs-vararg`, `possibly-uninitialized-binding`, `prelude-name-shadow`, `private-member`, `private-type-leak`, `public-test-item`, `re-export-loop`, `readonly-argument-to-mutable-parameter`, `readonly-edge`, `readonly-root`, `recursive-closure-needs-result-type`, `recursive-function-needs-result-type`, `requirement-in-default`, `reserved-semicolon`, `return-outside-function`, `row-parameter-in-context`, `sealed-trait-implementation`, `structure-outside-template`, `supertrait-cycle`, `suspending-pipe-step`, `suspension-forbidden-context`, `tab-whitespace`, `test-only-use`, `too-many-embedded-fields`, `top-level-read-before-initialization`, `trailing-block-position`, `trait-method-signature`, `trait-method-visibility`, `trait-not-dynamically-safe`, `trait-resolution-depth`, `trait-value-impl-target`, `type-used-as-value`, `underivable-trait`, `unexpected-bom`, `unknown-annotation-member`, `unknown-associated-type`, `unknown-named-argument`, `unknown-panic-category`, `unknown-shape-target`, `unreachable-match-arm`, `unresolved-generic-placeholder`, `unsatisfied-trait-bound`, `unsaturated-enum-constructor`, `unsigned-negation`, `unsupported-equality`, `unsupported-function-identity`, `variance-representation-change`, `variant-result-owner` |
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
| `argument-count` | A call supplies more or fewer arguments than the callee accepts, or a type-argument list has more positional arguments than its declaration has generic parameters. |
| `not-callable` | A call's callee is not a function, closure, or constructor. |
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
| **exhausted iterator** | An iterator whose `next` has returned `.None`. What a later `next` returns is unspecified. See [`flow.for.iterator-exhausted`](06-control-flow.md#r-flow.for.iterator-exhausted). |
| **fact** | An ordinary value attached to a type, member, or variant for derivations to read. See [Facts](14-annotations.md#facts). |
| **field lookup** | The steps that resolve `x.name` to one field from a module. See [Field Lookup](03-names-and-scopes.md#field-lookup). |
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
| **iterator adapters** | A stdlib term, in the [Standard Library glossary](std/README.md#glossary). |
| **known implementation** | An implementation in the program's dependency graph whose target matches a type; a local one counts only where its methods are available. See [`names.member.known-impl`](03-names-and-scopes.md#r-names.member.known-impl). |
| **law partners** | Comparison and hash traits whose laws relate them, such as `Hash` and `Eq`. See [Law Partners](09-traits.md#law-partners). |
| **lexical provider** | A provider a closure fixes where it is written, by capturing the value of `$.use`. See [Lexical And Dynamic Providers](11-requirements-and-suspension.md#lexical-and-dynamic-providers). |
| **literal suffix** | A name written directly after a numeric literal's digits, which names a suffix function. See [Literal Suffixes](01-lexical-structure.md#literal-suffixes). |
| **member line** | A line of a derivation block that edits one member's facts or omits it, or a line of a trait-less derivation block that edits its metadata. See [Member Lines](14-annotations.md#member-lines). |
| **member metadata** | The ordered list of values attached to a data field, an enum variant, or a parameter. See [Terminology](14-annotations.md#terminology). |
| **method lookup** | The steps that resolve `x.name(args)` to an own inherent method or a candidate. See [Method Lookup](03-names-and-scopes.md#method-lookup). |
| **method reference** | A method or associated function named as a function value, written `Owner::name` or `value::name` without arguments. See [Method References](07-functions.md#method-references). |
| **minimal version selection** | Choosing, for each host path and compatibility line, the largest minimum that any reached manifest states. See [Version Selection](10-modules.md#version-selection). |
| **mutable edge** | A direct field declared `field: mut U`; a readonly container removes its `mut`. See [`types.path.field.mutable-edge`](04-type-system.md#r-types.path.field.mutable-edge). |
| **mutable edges** | What a data type has when it declares a direct `field: mut U`, or embeds a type that has mutable edges. See [`data.edge.definition`](08-data-and-enums.md#r-data.edge.definition). |
| **mutable requirement trait** | A trait that declares or inherits a `mut self` method; its providers always have mutable access. See [`req.mut.trait`](11-requirements-and-suspension.md#r-req.mut.trait). |
| **non-reassignable** | A binding whose name cannot be rebound. See [`types.view.non-reassignable`](04-type-system.md#r-types.view.non-reassignable). |
| **one-key row slot** | A place where one bare key may stand for a row: `$.Context[...]` or a row-kinded type argument written without `$`. See [`req.row.alias.one-key-slot`](11-requirements-and-suspension.md#r-req.row.alias.one-key-slot). |
| **operator trait** | A `std.ops` trait, such as `Add[Rhs = Self]`, whose implementation gives a type one operator. See [Operator Traits](05-expressions.md#operator-traits). |
| **part** | The value an embedded field holds: the outer value's own copy of a value of the embedded type. See [Parts And Copies](08-data-and-enums.md#parts-and-copies). |
| **path requirement** | A manifest value `{ path = "DIR" }` through which a workspace member depends on another member. See [`module.workspace.path-requirement`](10-modules.md#r-module.workspace.path-requirement). |
| **pipe expression** | `value |> step`, which passes a value to a step. See [Pipe Expressions](05-expressions.md#pipe-expressions). |
| **place expression** | An expression that identifies a storage location, which may be read or, when permissions allow, assigned. See [`expr.category.place`](05-expressions.md#r-expr.category.place). |
| **prefix function** | A function marked `@str_prefix`, which a prefixed string calls. See [`expr.prefix.marker`](05-expressions.md#r-expr.prefix.marker). |
| **prefixed string** | An identifier followed directly by `"` or `"""`, as in `sql"..."`. See [`lex.prefix.form`](01-lexical-structure.md#r-lex.prefix.form). |
| **prelude** | The implicit scope of public standard-library names that every module has. See [Prelude](10-modules.md#prelude). |
| **program instance** | One instantiated Wasm module graph with its module storage, provider bindings, and execution state. See [`module.init.program-instance`](10-modules.md#r-module.init.program-instance). |
| **promoted candidate** | Among the promoted inherent methods that take part, the one with the called name at the smallest depth. See [`names.method-lookup.promoted-candidate`](03-names-and-scopes.md#r-names.method-lookup.promoted-candidate). |
| **promoted member** | A `pub` field or `pub` inherent method of a part's type, reached from the outer type through the part's path. See [`names.promote.member`](03-names-and-scopes.md#r-names.promote.member). |
| **pseudo-version** | A version that names one untagged commit by a base version, its time, and its hash. See [`module.version.pseudo`](10-modules.md#r-module.version.pseudo). |
| **readonly edge** | A field declared `field: U` with a composite `U`, which gives readonly access through any container. See [`types.path.field.readonly-edge`](04-type-system.md#r-types.path.field.readonly-edge). |
| **readonly view** | The `T` access to a composite value; it does not imply deep immutability. See [`types.view.term`](04-type-system.md#r-types.view.term). |
| **requirement row** | The normalized unordered set of requirement keys on a callable signature. See [`req.row.definition`](11-requirements-and-suspension.md#r-req.row.definition). |
| **requirement-free** | A default expression that uses no provider and does not suspend. See [`fn.default.requirement-free`](07-functions.md#r-fn.default.requirement-free). |
| **row alias** | A transparent alias that names a set of requirement keys. See [Row Aliases](11-requirements-and-suspension.md#row-aliases). |
| **row parameter** | A generic parameter whose values are requirement rows. See [`req.row.parameter`](11-requirements-and-suspension.md#r-req.row.parameter). |
| **rule ID** | A stable dotted name for one normative rule. See [Rule IDs](STYLE.md#rule-ids). |
| **runtime identity** | Two types share it when they are the same declaration applied to type arguments with the same runtime identity. See [`trait.identity.definition`](09-traits.md#r-trait.identity.definition). |
| **runtime profile** | A named compile-time set of host capability traits, their boundary adapters, and runtime choices such as panic exit statuses. See [`module.profile.definition`](10-modules.md#r-module.profile.definition). |
| **scalar boundary** | A byte offset of a string, from `0` to its length, that does not fall inside a scalar value's encoding. See [`types.string.boundary`](04-type-system.md#r-types.string.boundary). |
| **script** | An entry module with no `main`, whose top-level executable statements are the entry behavior. See [`module.init.script`](10-modules.md#r-module.init.script). |
| **sealed trait** | A standard trait whose implementations only the compiler and the standard library supply. See [Sealed Traits](09-traits.md#sealed-traits). |
| **self reference** | A member's or variant's `self_ref`: whether its type needs the type being derived (`.Required`), only refers to it (`.Optional`), or neither (`.Absent`), computed from its type alone. See [Self References](14-annotations.md#self-references). |
| **shape** | In generic code, the machine representation a value occupies; see [Shapes and Generic Code](04-type-system.md#shapes-and-generic-code). In annotations, a compiler-provided runtime value that describes a declaration's or type's structure; see [Terminology](14-annotations.md#terminology). |
| **specialized data shape type** | The type of `shape[D]()` for a data type `D`: the members of `DataShape`, plus a `fields` record with one member per direct field. See [Shape Intrinsics](14-annotations.md#shape-intrinsics). |
| **specialized enum shape type** | The type of `shape[E]()` for an enum `E`, which adds a `variants` record with one member per variant. See [Shape Intrinsics](14-annotations.md#shape-intrinsics). |
| **substitution step** | A pipe step that contains `_`. See [`expr.pipe.step-kinds`](05-expressions.md#r-expr.pipe.step-kinds). |
| **suffix function** | A function marked `@num_suffix`, which a suffixed literal calls. See [`expr.suffix.marker`](05-expressions.md#r-expr.suffix.marker). |
| **suffixed literal** | A numeric literal with a literal suffix, such as `250ms`, which calls the suffix function, as `ms(250)`. See [Literal Suffixes](05-expressions.md#literal-suffixes). |
| **take part** | The members of a type that lookup considers: its own fields and inherent methods, whatever their visibility, and its promoted members. See [`names.take-part.definition`](03-names-and-scopes.md#r-names.take-part.definition). |
| **template** | A trait's one derived implementation, written `impl[T] Trait for T by Structure:` in the trait's module. See [Templates](14-annotations.md#templates). |
| **test case** | One test, registered by a call of the prelude function `it`, or one row of `it_each`, in test position. See [Test Cases](10-modules.md#test-cases). |
| **test code** | A package's `tests:` blocks, test modules, and integration test modules, compiled only by a test build. See [`module.test.code`](10-modules.md#r-module.test.code). |
| **test dependency** | A dependency that the manifest declares for test builds only. See [`module.test.dependency`](10-modules.md#r-module.test.dependency). |
| **test module** | A module whose file name ends in `_test.hd`. See [Test Modules](10-modules.md#test-modules). |
| **test position** | The top level of a `tests:` block, a test module, or an integration test module, where test-case calls go. See [`module.testing.test-position`](10-modules.md#r-module.testing.test-position). |
| **trait candidates** | The trait methods of the receiver's type with the called name whose trait is available at the call. See [`names.method-lookup.trait-candidates`](03-names-and-scopes.md#r-names.method-lookup.trait-candidates). |
| **trait methods** | The methods of every trait that a known implementation implements for a type. See [`names.member.trait-methods`](03-names-and-scopes.md#r-names.member.trait-methods). |
| **trait-less derivation block** | An `impl X by Structure:` without a trait, whose member lines write shared metadata of `X` for every derivation and its shape. See [Trait-Less Derivation Blocks](14-annotations.md#trait-less-derivation-blocks). |
| **type pack** | A generic parameter ending in `...`, which stands for a list of types. See [Pack Parameters](12-variadic-generics.md#pack-parameters). |
| **type-argument default** | A type written with `=` after a generic parameter's bound, used when a use site leaves the parameter unsolved or a written type omits it. See [Type-Argument Defaults](04-type-system.md#type-argument-defaults). |
| **typed derivation** | Implementing a trait for a data type or enum from its members through the trait's template. See [Typed Derivation](14-annotations.md#typed-derivation). |
| **unbound method reference** | `Owner::name` without an argument clause, where `Owner` names a type, a trait, or a type parameter. See [`fn.ref.unbound`](07-functions.md#r-fn.ref.unbound). |
| **value expression** | An expression that produces a value. See [`expr.category.value`](05-expressions.md#r-expr.category.value). |
| **visible** | A field or inherent method is visible from a module that declares it, and from every module when it is `pub`. See [`names.visible.field-method`](03-names-and-scopes.md#r-names.visible.field-method). |
| **workspace** | A set of packages that one committed workspace manifest lists, selected as one graph. See [Workspaces](10-modules.md#workspaces). |

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
  Typed Derivation,
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
  Typed Derivation,
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
- Typed derivation M24 (owner decision in
  Typed Derivation,
  2026-09-27): a `Source` implementation may strengthen only the bound on
  `member[F]`. A strengthened bound on `missing[F]`, previously valid, is
  now `trait-method-signature`, so the undecided generic-`missing` question
  is closed. Member and parameter metadata are `List[Any]` values evaluated
  once at compile time, as facts are. A fact or metadata expression that
  reaches `std.task.block_on`, previously unspecified, is
  `suspension-forbidden-context`. `@derive(X)` needs no
  `use std.structure.Structure`; only `by Structure` does. An ordinary
  decorator before a function stays `decorator-not-annotator`, now with a
  rule ID. `Clone`'s module and the derived-function cache API are listed
  as undecided, to be chosen with the standard library.
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
- Literal suffix revisions (Literal Suffixes L12, L13, L15, L16, and L17,
  owner decisions 2026-09-28): only decimal and floating-point literals take
  a suffix. `0xff'B`, previously a suffixed literal, now begins an
  unterminated character literal, `unterminated-string`. A reserved word
  directly after digits, as in `5else`, previously a suffixed literal whose
  suffix was unknown, is now `invalid-token`. A suffixed literal has no
  compile-time rule of its own: in a fact or shared enum data it follows
  the rules for any call there. `timeout=` takes any `Duration` value, such
  as a call; `timeout="5s"` is now `type-mismatch` instead of
  `non-literal-test-argument`, and the value is evaluated when the test
  case runs. `Duration` is a whole number of milliseconds in an `i64`, and
  `std.time` no longer declares `ns` or `us`.
- Typed derivation M25 (owner decision in
  Typed Derivation,
  2026-09-28): a data type's one variant has an empty `VariantInfo.facts`.
  `@derive(...)` before a function, which the grammar admits, is
  `decorator-not-annotator`. Two declaration facts of one concrete type on a
  member, variant, or parameter, previously an error with no code, are
  `duplicate-fact`. A type-level fact of a primitive or `std` type, such as
  `@"internal"`, previously `unused-derivation-fact`, no longer warns.
- Typed derivation M26 (owner decision in
  Typed Derivation,
  2026-09-28): the `annotate Target:` block is removed, and `annotate` is no
  longer a reserved word. `annotate User: name = [max_len(80)]`, previously
  valid, is now a `syntax-error`; write `impl User by Structure:` with the
  same member line, after `use std.structure.Structure`. `annotate` is now
  an ordinary identifier. A trait-less derivation block's facts apply to
  every derivation and to shapes: `+=` appends after the decorator values
  and `=` replaces them. Parameter metadata, previously also written in
  `annotate get_user`, is written only with `@` on the parameter. The block
  must be in the target's module and not in a local scope; a method, an
  associated type, or an omit line `f = pass` in it, and a newtype target,
  are errors. `impl C by E` without a trait, where `E` is not `Structure`,
  is `invalid-delegation`.
- Typed derivation M27 (owner decision in
  Typed Derivation,
  2026-09-28): `Part = pass` on an embedded part stays
  `omitted-member-without-default`, now shown as an example. Two decorators
  before one declaration whose type-level facts have one concrete type, as
  two `@style(...)` lines, previously not covered, are `duplicate-fact` on
  the later decorator.
- Typed derivation M28 (owner decision in
  Typed Derivation,
  2026-09-28): a trait-less derivation block's header declares only its
  target's own type parameters, without bounds, as
  `impl[T] Box[T] by Structure:`; `impl Box[i32] by Structure:` or a bound,
  previously without a meaning, is `misplaced-derivation`. A second
  trait-less block for one type, previously undecided, is
  `overlapping-impl`. A type-level fact written in a trait-less block's
  `Self` line now gets `unused-derivation-fact` on that line. A member
  line's right side may be any expression of a list type, so
  `name = shared_list`, previously `invalid-member-line`, is valid.
- Typed derivation M29 (owner decision in
  Typed Derivation,
  2026-09-28): a trait-less block's header may rename the type's
  parameters, so `impl[U] Box[U] by Structure:` for `data Box[T]`,
  previously unclear, is valid. A member line whose right side is not a
  list, such as `name = 5`, is `invalid-member-line` rather than a type
  mismatch. A `Self` line in a per-trait derivation block, previously
  unchecked, now warns `unused-derivation-fact` when the fact's package
  does not supply that block's trait. `duplicate-fact` on declaration facts
  stays on the later value.
- `println` drives `write_line!` (mutable host providers MHP-1, owner
  decision 2026-09-28): a `println(value)` call calls
  `write_line!(value.to_string())` on the covering `Console` provider and
  drives it to completion inside itself, so a program-defined provider,
  such as a recording console, now receives the line. `println` stays
  non-suspending, and no caller changes. A `write_line!` call that stays
  pending on a host operation, or returns `.Err(ConsoleError)`, previously
  unspecified, now makes `println` panic; the panic category is not yet
  specified.
- Literal suffixes L18 (owner decision in
  Literal Suffixes,
  2026-09-28): `Duration`'s public API is `Duration::milliseconds`,
  `Duration::seconds`, and `as_milliseconds`. A standard suffix whose
  `Duration` overflows `i64` milliseconds, previously unspecified, panics
  at run time with `integer-overflow`; a warning for it is optional. No
  existing source changes meaning.
- Testing T53 (owner decision in
  Testing, 2026-09-28):
  `DebugWriter` has builders like Rust's `Formatter`: `debug_struct`,
  `debug_tuple`, `debug_list`, `debug_map`, and `write`, with the builder
  types `DebugStruct`, `DebugTuple`, `DebugList`, and `DebugMap`.
  `@derive(Debug)` generates builder calls, and the writer chooses the
  layout. `std.testing` declares `Choices` and `Arbitrary`, and gives
  `it_prop` and `it_prop_with` their signatures. `snapshot_file` keeps its
  file at `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`, and
  a missing file, previously unspecified, fails the test case outside an
  update run. The conformance format gains `# fixture-test-layout:`. No
  existing source changes meaning.
- `println` has `block_on`'s rules (mutable host providers, MHP-1
  follow-ups, owner decision 2026-09-28): `println` is an ordinary `std`
  prelude function, and its panics are ordinary `std` panics with no
  category of their own. It follows every rule of `block_on`. A `println`
  call inside `main!` or a test body, previously valid, now panics with
  `suspension-nested-driver`; write with `$.use(Console).write_line!`
  there. A `println` call in a `defer` suite or a default expression,
  directly or transitively, previously valid, is now
  `suspension-forbidden-context`.
- `println` is `block_on` of its write (mutable host providers, MHP-1
  second round, owner decision 2026-09-28): a `write_line!` call that is
  pending on a host write, which previously made `println` panic, is now
  driven until it finishes, as `block_on` does. The panic on
  `.Err(ConsoleError)` is `explicit-panic`, as any `panic` call in `std`
  is. A `println` call at the top level of a script stays valid.
- Testing T54 (owner decision in
  Testing, 2026-09-28): a `snapshot` or `snapshot_file` mismatch, and a missing
  snapshot file outside an update run, previously a test failure with no
  panic category, now panic with `assertion-failed`. `@derive(Debug)`
  follows Rust's mapping: a data type or a variant with named payload
  fields uses `debug_struct`, a variant with positional payload fields
  uses `debug_tuple`, and a variant without a payload writes its name.
  The builder type names `DebugStruct`, `DebugTuple`, `DebugList`, and
  `DebugMap` are confirmed. No existing source changes meaning otherwise.
- Decorators D1-D9 (owner decisions in
  Decorators, 2026-09-28):
  a decorator is a plain value on any item (a function, data type, enum,
  trait, implementation, or newtype) or member (a field, variant, value
  parameter, or method). `@tool("search")` before a function, previously
  `decorator-not-annotator`, is valid, and `shape_of(f).metadata[M]()`
  reads it. A decorator before a trait, an implementation, a method, or a
  method's value parameter, and a decorator other than `@derive` before a
  newtype, previously `syntax-error`, are valid. `@derive(...)` stays an
  intrinsic: before a function, trait, implementation, or method it is
  `decorator-not-annotator`. In a decorator, a bare name of a function with
  no parameters is now called, so `@hidden` attaches `hidden()`, not the
  function value. `std.annotation` declares `Target`, `Annotate`, and
  `annotate`: a value whose type carries `@annotate(.Fn)` on any target
  other than a function is `decorator-not-annotator`. Two values of one
  concrete type before one function, trait, implementation, newtype, or
  method are `duplicate-fact`. Modules take no decorators.
- Literal suffixes L11 (owner decision in
  Literal Suffixes,
  2026-09-28, with the names of Decorators D9): a suffix is a function
  marked `@num_suffix`, and `250ms` is the call `ms(250)`.
  `std.ops.LiteralSuffix` and the newtype carriers are removed:
  `use std.ops.LiteralSuffix` and `impl LiteralSuffix[i32, Pixels] for px`,
  previously a suffix declaration, no longer compile; write
  `@num_suffix fn px(count: i32) -> Pixels` after
  `use std.ops.num_suffix`. `std.ops` declares `NumSuffix` and
  `num_suffix`. A suffix that names nothing, previously `unknown-type`, is
  `unknown-name`. A suffix that names a function without `@num_suffix`, or
  whose function has type parameters, providers, suspension, or anything
  but one numeric parameter, is the new error `invalid-literal-suffix`, at
  the literal; a parameter the literal cannot fill is the ordinary call
  error. `std.time`'s `ms`, `s`, `min`, and `h` keep their meaning.
- Bound and row operators (owner decision in
  [Open Issues](../future-work/OPEN_ISSUES.md#bound-and-row-operators),
  2026-09-28): several trait bounds are joined with `&` in every bound
  position, as in `T < Eq & Hash`, `trait Ord < Eq & PartialOrd`, and
  `impl[T < Eq & Hash] ...`. The former `T < Eq + Hash` is the new error
  `old-bound-operator`. Requirement rows join keys with `+` again, and every
  position writes a row the same way:
  `fn load(id: UserId) -> User $ Db + Cache:`,
  `fn(UserId) -> User $ Db + Cache`, `Job[$ Logger + Clock]`, and
  `$.Context[$ Metrics + Cache]`. The comma lists of 2026-09-27, `$ Db, Cache`
  and `$(Db, Cache)`, are the new error `old-row-separator`. A parenthesized
  nonempty row such as `$(Db + Cache)` and a `-` between keys are
  `syntax-error`. `$()` is still the empty row. `old-row-operator` was
  withdrawn.
- Bound and row operators, item 4 (owner decision in
  [Open Issues](../future-work/OPEN_ISSUES.md#bound-and-row-operators),
  2026-09-28): a callback whose row lacks the key a provider-installing
  function discharges now matches the extension pattern. Passing
  `fn tick() -> void $ Clock` to `cb: fn() -> void $ R + Logger`, previously
  a `type-mismatch`, is valid, and `R` is `Clock`. Rows still have no
  subtraction.
- Literal suffixes L19 (owner decision in
  Literal Suffixes,
  2026-09-28): a name written directly before a string's quote is a string
  prefix, and `sql"a $x"` is the call `sql(t)` of a function marked
  `@str_prefix`, where `t` is a `std.ops.Template` holding the raw text
  pieces and the interpolated values. The built-in raw string is removed:
  `r"..."` now calls the standard prefix `std.ops.r`, so it needs
  `use std.ops.r`, and without one it is `unknown-name`. `r"$name"` and
  `r"${x}"`, previously literal text, now interpolate the values' `Display`
  text; `r"$true"` is `syntax-error`. A prefixed string in a pattern is
  `syntax-error`. A name before a quote that names nothing, as in
  `b"..."`, is `unknown-name`. A prefix that names a function without
  `@str_prefix`, or whose function has type parameters, providers,
  suspension, or anything but one `Template[T]` parameter, is the new error
  `invalid-string-prefix`. `std.ops` declares `StrPrefix`, `str_prefix`,
  `Template`, and `r`.
- Requirement reuse RU2 (owner decision in
  Requirement Reuse,
  2026-09-28): `type AppRow = Db + Cache` declares a row alias, and
  `$ AppRow + Clock` stands for its keys wherever a row follows `$`. Such a
  declaration was previously a `syntax-error`. A row alias used as a value
  type, a bound, or a single key is `generic-kind-mismatch`. An alias that
  expands to itself, row or not, is the new error `alias-cycle`; an
  ordinary alias cycle was previously not diagnosed. Diagnostics print
  aliased rows as written and list the expanded keys on a mismatch.
- Requirement reuse RU3 (same record, 2026-09-28): data types, enums, and
  traits take no row parameters. `data Job[R]` with a field
  `run: fn() -> void $ R`, previously valid, is now
  `generic-kind-mismatch`, and so is `Job[$ Logger + Clock]`. A row
  parameter in `$.Context[...]` is the new error `row-parameter-in-context`.
- Requirement reuse RU4 (same record, 2026-09-28): a row pattern has at most
  one unknown row parameter. `f: fn() -> void $ R1 + R2`, previously
  accepted with its rows left unsolved, is the new error
  `ambiguous-row-pattern` unless another parameter fixes one of them.
- Requirement reuse RU5 (same record, 2026-09-28): a function value whose row
  is a subset of an expected function type's row fits that type everywhere.
  Passing `fn health() -> string $ Clock` where
  `fn() -> string $ Db + Clock` is expected, previously a `type-mismatch`,
  is valid. A key outside the wider row is still `type-mismatch`.
- Requirement reuse RU10 (same record, follow-up decision, 2026-09-28): a
  bare row alias in a one-key row slot stands for its row. `$.Context[AppRow]`,
  previously `generic-kind-mismatch`, is valid and means
  `$.Context[$ AppRow]`. The same holds for the row argument of `Fn` and
  `SuspendFn` and for a row alias's row parameter. It also holds for an
  explicit type argument, as in `provide_log[AppRow](job)`. A row alias used
  as a value type, a bound, a type-kinded argument, or a key in `$.use` or a
  `$.with` binding is still `generic-kind-mismatch`.
- Requirement reuse RU12 (same record, follow-up decision, 2026-09-28): a
  list or map literal with no expected type gives its function values the
  union of their rows. `handlers := [health, orders]`, previously
  `no-common-type` when the rows differ, is valid, and calling through
  `handlers` requires every key of the union. The inferred list does not
  convert to a list with a wider row.
- Requirement reuse RU13 (same record, follow-up decision, 2026-09-28): a
  caller may fix a row parameter to a row that lists a key the callee
  installs. The callback's lookups then use the callee's nearer provider, as
  nested `$.with` scopes do. No source changes validity; the rule states
  behavior that was previously unspecified.
- Narrowing integer casts wrap (owner decision in
  Open Issues,
  item 1, 2026-09-28): an integer-to-integer cast keeps the low bits of the
  value, as Go conversions and Rust `as` do, so `u8(x)` with `x = 300` gives
  44 where it previously panicked. A cast whose argument is an integer
  literal checks the literal against the target type, so `u8(300)` is
  `integer-literal-range` at compile time. A float-to-integer cast out of
  range still panics.
- Property test discards (same record, item 2, 2026-09-28): a case that
  `assume` discards no longer counts toward `cases`. A property test whose
  discards exceed 10 times `cases` fails, where it previously passed.
- Type names as values (same record, item 3, 2026-09-28): a type, trait,
  or alias name used where a value is required, as in `let x = User`, is
  `type-used-as-value`, now defined by a numbered rule.
- Standard-library scope (same record, item 4, 2026-09-28): the examples of
  `trait.derive.newtype.self-error` and `annot.bound.more` use `Map`
  instead of `Set`, and `trait.derive.hash.seeded` and
  `req.determinism.hash-seeded` describe runtime-provided hash seeds
  without naming a standard `Hasher` type. No behavior changes.
- Derivation codes (same record, item 5, 2026-09-28): a field that an
  intrinsic derivation compares, orders, or hashes without implementing the
  trait is the new error `derive-field-missing-trait`, whose message names
  the trait and the field. It replaces `field-not-eq` and `field-not-hash`,
  which are withdrawn. A use of a derived implementation whose added bound
  fails is `missing-derived-bound`, now defined by a numbered rule.
- Property test inputs (same record, item 6, 2026-09-28): `it_prop` and
  `it_prop_with` require `T < Debug`. A property whose input type does not
  implement `Debug`, previously accepted, is `unsatisfied-trait-bound`. A
  failure prints the shrunk input.
- Property test regressions (same record, item 7, 2026-09-28): the runner
  saves a failing property's shrunk choice stream in
  `__regressions__/<module>/<test-slug>`, one decimal number per line, and
  replays it before new cases on the next run.
- Mixed variants under `@derive(Debug)` (same record, item 8, 2026-09-28):
  a variant with both positional and named payload fields uses the struct
  builder, naming positional fields `_0`, `_1`, and so on:
  `Mixed { _0: 1, label: "x" }`.
- Literal suffixes L20 (owner decision in
  Literal Suffixes,
  2026-09-28): a suffixed literal and a prefixed string are plain call
  sugar. A suffix or prefix function may now be generic or need providers:
  its call follows the ordinary generic and row rules, so `12px` where
  `px` needs `Console` is valid inside a function that has `Console`, and
  `missing-requirement` where it does not. Both were
  `invalid-literal-suffix` or `invalid-string-prefix` before. The raw-text
  prefix `r` moves from `std.ops` to `std.text`: `use std.ops.r` becomes
  `use std.text.r`, and the old import is now `unknown-name`.
- Literal suffixes L21 (same record, 2026-09-28): the compiler checks a
  marked function's shape at its definition. A function marked
  `@str_prefix` must take exactly one required parameter of type
  `Template[T]`, and one marked `@num_suffix` exactly one required
  primitive numeric parameter; neither may suspend. A marked function that
  breaks this is `type-mismatch` on its `fn` line, where it was previously
  accepted and each use failed. Further parameters with defaults are now
  allowed. `return"x"` stays two tokens.
- Dependency cycles DC1 (owner decision in
  Dependency Cycles,
  2026-09-29): a public inherent method must declare its result type, as a
  public function does. `pub fn total(self): self.count` inside `impl Cart:`
  is now `missing-result-type`. Without a `$` clause its row is empty, where
  it was previously inferred from the body.
- Dependency cycles DC2 to DC6 and DC9 (same record, 2026-09-29): files of
  one folder may use each other in a loop, which was previously an error
  for every loop. The folder graph must be acyclic, with no exemption for a
  parent and child folder. A root facade that uses `shop/` beside a root
  `error.hd` that `shop/` uses, previously valid, is now the new error
  `folder-cycle`; moving `error.hd` to `error/mod.hd` fixes it. Uses in
  test code make no edge. Package dependency cycles are invalid.
- Dependency cycles DC7 (same record, 2026-09-29): modules that use each
  other initialize as one group. Their top-level statements run in
  dependency order, then by module identity and source position, as Go
  orders one package's variables. A group whose statements can never
  become ready is `top-level-read-before-initialization`.
- Dependency cycles DC8 (same record, 2026-09-29): a `pub use` chain must
  end at a declaration. Two `pub use` lines that forward a name to each
  other stay invalid; a facade and a child file that use each other become
  valid.
- Dependency cycles DC10 (same record, 2026-09-29): a package interface
  records each fact by its value, so it may depend on a body that a fact
  calls. No source changes validity.
- Dependency cycles DC11 (same record, 2026-09-29): the two invalid
  dependency shapes get codes. A `pub use` loop is `re-export-loop`, and a
  package dependency cycle is `package-cycle`. No source changes validity.
  The conformance format gains the `# fixture-package-tree:` header, which
  places a fixture in a package of several files.
- Small follow-ups (owner decision in
  Open Issues,
  2026-09-29): in a prefixed string, a `$` is text unless `{` or a
  character that can start an identifier follows, so `r"costs $5"` keeps
  `$5` as text where the rule left it undefined. A suffix function's one
  parameter may have a default, and a non-public function's inferred
  result takes the union row; both were already valid.
- Literal suffixes L22 (owner decision in
  Literal Suffixes,
  2026-09-29): a function marked `@num_suffix` or `@str_prefix` declares
  exactly one parameter. `@num_suffix fn kb(count: i64, unit: i64 = 1024)`,
  valid since L21, is now `type-mismatch` at the definition. Which line of
  the definition the error names is left to the implementation. The helpers
  `interpolate`, `process_escapes`, and `EscapeError` move from `std.ops`
  to `std.text`; no rule names them, so no rule changes.
- Float-to-integer casts saturate (owner decision in
  Open Issues,
  follow-ups, 2026-09-29): a cast out of range clamps to the target's
  minimum or maximum, and NaN gives 0, as Rust `as` does. `i8(x)` with
  `x = 300.0` gives 127 where it previously panicked. No numeric cast
  panics.
- Derived newtypes (same record, follow-ups, 2026-09-29): `@derive` on a
  newtype whose base type lacks the trait is `derive-field-missing-trait` at
  the base type, as for a derived field. `@derive(Eq) type Wrapped(Opaque)`
  with no `Eq` for `Opaque` was previously `missing-partial-eq` in the
  prototype, which the specification did not name.
- Requirement reuse RU15 (owner decision in
  Requirement Reuse,
  2026-09-29): the union row applies at every least-common-type site, not
  only in list and map literals. `if admin: orders else: health`, where the
  two functions need `Db` and `Clock`, was `no-common-type` and is now a
  `fn() -> string $ Clock + Db`. The same holds for `match` arms and for
  the results of a closure or non-public function whose result type is
  inferred. A value that holds function values, such as a list, still does
  not widen.
- Decorators D10 (owner decision in
  Decorators, 2026-09-29):
  `std.annotation.Target` gains `Newtype`, so a fact type can opt newtypes
  in with `@annotate(.Newtype)`. A value before a newtype was always an
  error when its type was limited; it is now valid when the limit lists
  `.Newtype`. A value attached to a kind its `@annotate` does not list is
  the new error `decorator-target-kind`, previously
  `decorator-not-annotator`; this covers `@num_suffix` and `@str_prefix`
  before anything but a function. A misplaced `@derive` stays
  `decorator-not-annotator`, and a suffix naming an unmarked function stays
  `invalid-literal-suffix`.
- Operator traits OP1 (owner decision in
  Operator Traits,
  2026-09-29): `std.ops` declares Rust-shaped operator traits, such as
  `Add[Rhs]` with an associated `Out`. A binary operator with a
  non-primitive operand calls the left operand's implementation, so
  `a + b` on a `data` type with `impl Add[Money] for Money`, previously
  `type-mismatch`, is valid. An operand type without one stays
  `type-mismatch`.
- Operator traits OP2 (same record, 2026-09-29): the standard library
  implements the operator traits for the primitive number types with
  intrinsic bodies, so `fn sum[T < Add[T, Out = T]]` accepts `i32`. Two
  primitive operands keep the built-in rules and search no trait, so no
  existing numeric expression changes.
- Operator traits OP3 (same record, 2026-09-29): the twelve operators
  `+ - * / %`, unary `-`, `& | ^ ~`, `<<` and `>>` have traits. `**`,
  unary `+`, the logical operators, and the comparisons have none.
- Operator traits OP4 (same record, 2026-09-29): a literal on the left of
  a non-primitive operand takes its default type and is never typed from
  an implementation, so `3 * price` is `type-mismatch` unless `i32`
  implements `Mul` for the price type.
- Operator traits OP5 (same record, 2026-09-29): compound assignment is
  new. `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, and `>>=` become
  tokens, so `a-=b`, previously `a`, `-`, `=`, `b` and a syntax error,
  lexes as one assignment. On primitives `x += 1` is built in; on other
  types it calls an `AddAssign`-family `mut self` method, which changes a
  `data` value in place for every alias.
- Operator traits OP6 (same record, 2026-09-29): a newtype gets operators
  only from hand-written implementations. No source changes validity.
- Operator traits OP7 (same record, 2026-09-29): a user type implementing
  `std.ops.Index` or `IndexSet` supports `r[k]` or `r[k] = v`, which were
  `type-mismatch` before. `List` and `Map` keep built-in indexing.
- Operator traits OP8 (same record, 2026-09-29): a supertrait list may bind
  an associated type, as in `trait Summable < Add[Self, Out = Self]`,
  which was `syntax-error`. An implementation whose supertrait binds
  another type is `missing-supertrait-implementation`.
- Operator traits OP9 (same record, 2026-09-29): `std.num` declares the
  sealed traits `Num`, `Integer`, and `Float` for the primitive number
  types, with `zero`, `one`, and `from_i64` on `Num`. Because `Num` has
  `%`, floating `%` is now valid and truncates as C `fmod` does, where
  `2.0 % 1.0` was `type-mismatch`. A function marked `@num_suffix` may be
  generic over `N < Num`, which was `type-mismatch` at the definition.
- Operator traits OP10 (same record, 2026-09-29): compound assignment
  depends on the place's kind. On an `AnyVal` type, such as a newtype over
  `i64` or a type parameter bounded by `Num`, `a += b` now means
  `a = a + b`; it needs `Add` and a reassignable place, where it was
  `type-mismatch` without an `AddAssign` implementation. On any other type
  it still calls the assign method, never falls back, and its error offers
  `a = a + b` as a fix-it. `Num::from_i64` is now checked:
  `u8::from_i64(300)`, which wrapped to 44, panics with
  `integer-overflow`. `Num` gains the supertraits `PartialOrd` and
  `Display`, so `T < Num` code may compare and interpolate.
- Operator traits OP11 (same record, 2026-09-29): the trait of `~` is
  renamed from `BitNot` to `Not`, with the method `not`, so
  `use std.ops.BitNot` no longer resolves. `string` implements `Add`,
  so `T < Add[T, Out = T]`, which rejected `string` with
  `unsatisfied-trait-bound`, now accepts it; `a + b` on two strings stays
  built in.
- Operator traits OP12 (same record, 2026-09-29): a newtype construction
  over a data type carries its argument's permission, so `Order(d)` is
  `mut Order` exactly when `d` has mutable access; storing `Order(seen)`
  with a readonly `seen` into a `mut Order` binding is now
  `mutable-upgrade`. The `a = a + b` fix-it of a rejected compound
  assignment is no longer required.
- Operator traits OP13 (same record, 2026-09-29): the ten assign traits,
  such as `AddAssign`, are removed, so `use std.ops.AddAssign` no longer
  resolves. `a op= b` means `a = a op b` for every type. On a data type
  that implements only `AddAssign`, `a += b`, previously valid, is now
  `type-mismatch`; with `Add` it is valid on a reassignable place, where
  it was `type-mismatch`. A parameter place, which the assign call
  allowed, is now `non-reassignable-parameter-binding`, and a field of a
  readonly root is `readonly-root`. Another reference to the old value no
  longer sees the change. An index place reads, then stores (stress test
  decision 11); on a `Map` the read is `V?`, so `counts[w] += 1` on a
  `Map[string, i32]` stays `type-mismatch`.
- Dependency cycles DC12 (owner decision in
  Dependency Cycles,
  2026-09-29): a plain `use` whose name leads into a `pub use` loop is
  `re-export-loop`, where no code was specified.
- Stress test decisions 5-11 (owner decisions in
  Stress Test 2026-09-29,
  2026-09-29): a decorator before an `impl ... by Structure:` block, which
  was silently ignored, now warns `unused-derivation-fact`. A newtype
  with a row parameter, as in `type Job[R](fn() -> void $ R)`, which had
  no specified meaning, is `generic-kind-mismatch`. A function-typed left
  operand still selects an operator implementation by its own type, now
  stated. Decision 11 is applied with Operator traits OP13.
- Iterator adapters (STDLIB questions 14 and 18, owner decision,
  2026-09-29): the prelude `Iterator[T]` gains the default methods
  `filter`, `take`, `enumerate`, and `collect`, as Rust's `Iterator` has
  them; there is no separate extension trait. `filter`, `take`, and
  `enumerate` are lazy, and a negative `take` count panics. A call such as
  `counter.take(2)` on an iterator type that also implements another
  available trait with a `take` method is now `ambiguous-method`; an
  inherent `take` still wins.
- Local mutability (owner decision in
  Open Issues,
  2026-09-29): `let mut name = value`, previously a `syntax-error`, is valid
  and infers `mut T`; it is `mutable-upgrade` when `value` is readonly. A
  plain `let` now infers the readonly view even of a fresh value, so
  `let user = User { ... }` followed by `user.name = ...`, previously
  valid, is `readonly-root`; write `let mut user`. In a multi-name `let`,
  each name may take its own `mut`. `let mut name: T = ...` with a
  readonly `T` is the new error `let-mut-readonly-type`. `mut name := ...`
  and `for mut item in ...` stay `syntax-error`.
- Pipe operator (owner decisions PL3-PL13 in
  Pipe Operator, as
  amended by Chaining Study
  CS2, 2026-09-29): `|>` is a new token, and `a |> b`, previously a
  `syntax-error`, is a pipe expression between `|` and comparison. A step
  is a substitution step with exactly one `_` or a bare name or path. A
  line starting with `|>` now continues the previous line, so
  `lex.continue.no-operator` is replaced by
  `lex.continue.no-other-operator`. `_` in expression position, previously
  a `syntax-error`, now parses and is `placeholder-outside-pipe` outside a
  step. New codes: `duplicate-pipe-placeholder`,
  `pipe-placeholder-in-closure`, `pipe-step-needs-placeholder`,
  `suspending-pipe-step`, `multi-line-pipe-step`, and
  `placeholder-outside-pipe`.
- Iterators (owner decisions CS7 and CS8 in
  Chaining Study,
  2026-09-29): `Iterator[T]` is a concrete prelude `data` type holding a
  `step: fn() -> T?` closure, not a trait. `impl Iterator[T] for X`, which
  made `X` an iterator, is now an error, since `Iterator` is not a trait.
  The adapters are ordinary methods, and `map[U]` and `fold[A, R]` join
  them. `for` and comprehensions accept only `Iterable[T]`, which
  `Iterator[T]` now implements, so an iterator satisfies an `Iterable[T]`
  bound where it was `unsatisfied-trait-bound`. A type that implemented
  both traits, iterated through `Iterable`, no longer arises.
- Method references (owner decisions MR1-MR4 in
  Method References,
  2026-09-29): `Type::name`, `Trait::name`, and `T::name` without a call,
  previously `deferred-method-value`, are unbound method references with
  the receiver first. `value::name`, also `deferred-method-value`, is a
  bound reference that captures the receiver when it is made, and
  `user::domain()` is now an ordinary call. The code
  `deferred-method-value` is withdrawn. `::` never names a field, so
  `User::email` for a field is `unknown-method`. A method reference is a
  bare pipe step.
- Collect (owner decisions CO1-CO4 in
  Collecting Iterators,
  2026-09-29): `collect` is `collect[C < FromIterator[T]]`, with `C` taken
  from the expected type, an explicit list, or `List[T]` when nothing
  fixes it, so existing `collect()` calls keep their `List[T]`. A `Map`
  target keeps the last value of an equal key; a `Result` or optional
  target stops at the first failure and leaves the rest of the iterator
  unread. `?` inside a comprehension, which no rule covered, is valid: it
  propagates, and the comprehension stops.
- Dependencies (owner decisions DEP1-DEP7 in
  Dependencies,
  2026-09-29, with the still-valid
  [Packages](../future-work/PACKAGES.md#owner-decisions) decisions 1, 4, 8,
  10, 12, and 13): no source changes, and `use dep.NAME` stays. There is
  no registry. An `hd.toml` dependency is a host path and a minimum
  version, `billing = "github.com/acme/billing@1.2.0"` where the example
  had `billing = "1.2.0"`. Versions are version control tags, so
  `[package] version` is gone. Selection is minimal version selection, and
  `hd.sum` holds the hashes; there is no lockfile. `module.manifest.tooling`
  and `module.tooling.package` are retired.
- Pipe steps (owner decisions PL14-PL16 in
  Pipe Operator,
  2026-09-29): a `_` belongs to the innermost pipe step that contains it,
  so `x |> f(_, y |> g(_))`, which no rule covered, is valid. A
  leading-dot line before the first `|>` stays part of the chain, and
  `x |> user.greet` is `user.greet(x)`, as the first apply pass read them;
  both are now stated.
- Iterator construction and exhaustion (owner decisions CS9-CS11 in
  Chaining Study,
  2026-09-29): `step` is private, so `source.step` outside `std` is
  `private-member`, and code builds an iterator with the new associated
  function `Iterator::from_fn(step)`. What `next` returns after it has
  returned `.None` is now unspecified, where it was `.None` again; code
  that calls `next` or `collect` on an exhausted iterator no longer has a
  portable result. `from_fn` does not fuse. `flow.for.iterator-progress`
  is retired.
- Method references (owner decisions MR6 and MR7 in
  Method References,
  2026-09-29): `value::name` for an associated function, which no rule
  covered, is `unknown-method`. `Identity::echo[i32]!(42)` stays a
  `syntax-error`.
- `FromIterator` (owner decision CO6 in
  Collecting Iterators,
  2026-09-29): `FromIterator` is not a prelude name, so an `impl` of it
  without `use std.iter.FromIterator` is `unknown-trait`. A `collect` call
  needs no import.
- Dependencies (owner decisions DEP8-DEP15 in
  Dependencies,
  2026-09-29): no source changes. A workspace member depends on another
  member through a path requirement, `{ path = "../billing" }`, and a
  tagged version whose manifest holds one is rejected. `github.com` is the
  one known host, so a requirement on any other host needs a `.git`
  segment. A manifest does not state its own host path. Two keys that name
  one host path and compatibility line are invalid. A tagged release may
  require a pseudo-version. `module.dep.requirement` and
  `module.repo.known-host` are retired.
- Type-argument defaults (owner decisions TD1-TD7 in
  Type-Argument Defaults
  and CO5 in Collecting Iterators,
  2026-09-29): a generic parameter of a function, method, data type, enum,
  trait, or `type` declaration may declare a default after its bound, as in
  `collect[C < FromIterator[T] = List[T]]`. An implementation header, an
  enum variant, and a type pack take none. A default fills only what a use
  site leaves unsolved, so an argument wins, and an omitted slot of a
  written type takes it. An explicit list may now omit trailing slots,
  which are inferred like `_` and then defaulted: `pair[string]("left", 1)`,
  previously `partial-generic-arguments`, is valid. That code now reports
  a written type that omits a slot without a default, such as
  `Map[string]`, and a trait value type that omits a `Self` default. An
  implementation method repeats a trait method's default. No diagnostic
  code is added. `collect`'s `List[T]` fallback is now its declared
  default, so existing calls keep their meaning; `flow.collect.default`
  and eleven other rule IDs are retired.
- Associated type bindings (owner decisions AT1 and AT2 in
  Open Issues,
  2026-09-29): a binding may name an associated type that a supertrait
  declares, so `I < NamedSupplier[Item = T]`, previously
  `unknown-associated-type`, is valid; a name that two supertraits declare
  separately is `ambiguous-method`. A trait value type may bind
  associated types, as `Supplier[Item = i32]`, and is dynamically safe
  when it binds every one; `Supplier` alone stays
  `trait-not-dynamically-safe`, and an associated function still makes a
  trait unsafe. Such a value satisfies `T < Supplier`, with `T::Item` the
  bound type. A binding on a type that is not a trait, such as
  `List[i32, Item = i32]`, was a `syntax-error` and is now
  `unknown-associated-type`. The trait of an `impl` header, a
  trait-qualified call, and a method reference still reject bindings with
  `syntax-error`. Eight rule IDs are retired.
- `let mut` follow-ups (owner decisions 1-4 of
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  2026-09-29): a multi-name `let` with `mut` puts its names in
  parentheses, as `let (mut log, db) = pair`. The former
  `let mut log, db = pair` is now a `syntax-error`, and so is a
  parenthesized list without `mut`. A primitive type written with `mut`,
  as `let n: mut i32`, and `let mut n = 0` report the new error
  `mut-on-primitive`; `let mut n = 0` was `mutable-upgrade`. `let mut`
  accepts an initializer of type `mut T?`. The redundant `mut` in
  `let mut a: mut T` gets the new warning `redundant-let-mut`.
- Operator follow-ups (owner decisions 5-7 of the same list,
  2026-09-29): `m[k] op= v` on a `Map` reads the entry as `V`, so
  `counts[w] += 1`, previously `type-mismatch`, is valid and panics with
  `index-out-of-bounds` when `w` is absent. Unwrapping a newtype over a
  reference type carries its permission, so `Draft(order)` with
  `order: mut Order` has type `mut Draft`. The ten compound-assignment
  tokens are confirmed, with no change.
- Decorator and wording follow-ups (owner decisions 8-10 of the same list,
  2026-09-29): no source changes. Every decorator before a derivation
  block warns, a standard value such as `@"internal"` included. A caller
  of an `AnyRef`-bounded method parameter converts only primitive and
  tuple values; an optional needs no conversion, as before.
  `grammar.stmt.let-mut`, `expr.assign.compound.map-read`, and
  `types.trait.safe.convert` are retired.
- Typed derivation M30 (owner decisions in
  [Typed Derivation](../future-work/TYPED_DERIVATION.md#owner-decisions-m30-2026-09-29),
  2026-09-29): no source changes. A fact type has no compile-time check
  hook, and derivation between two types is out of scope. On a name
  clash, `Structure::walk(self, w)` calls the generated `walk`. Template
  constants, typed shared constants, and composing templates are
  deferred, and `h.default()` may allocate. The M26 readings of trait-less
  blocks are confirmed. Undecided Parts keeps only the points that wait on
  other areas.
- Chaining Study CS12 and Dependencies DEP16-DEP18 (owner decisions in
  Chaining Study and
  Dependencies,
  2026-09-29): no source changes. CS12 and DEP16 confirm rules already
  applied. A fetched package that requires a workspace member's host path
  is a separate package from the local member, so a build may hold both.
  A dependency requirement whose tag does not exist is invalid, and the
  toolchain never falls back to an untagged commit or a nearby version;
  the error's code is named later with the other manifest diagnostics.
- Type-argument defaults TD8-TD10 (owner decisions in
  Type-Argument Defaults,
  2026-09-29): a type-argument list with more positional arguments than
  its declaration has generic parameters is `argument-count`, in a call
  and in a written type; it named no code before. The binary `std.ops`
  operator traits declare `Rhs = Self`, so `impl Add for Money` means
  `impl Add[Money] for Money`, and `T < Add[Out = T]` means
  `T < Add[T, Out = T]`. Source that writes the argument keeps its
  meaning. `fn.generic.explicit.too-long` and `expr.op.trait.shape` are
  retired.
- Associated type bindings AT3-AT5 (owner decisions in
  Open Issues,
  2026-09-29): a binding name that two supertraits declare separately,
  previously `ambiguous-method`, is the new error
  `ambiguous-associated-type`. A requirement key may bind associated
  types, as in `fn load() -> User $ Store[Item = User]`, previously a
  `syntax-error`. Keys compare by trait instantiation and bindings, so
  `Store[Item = User]` and `Store[Item = Post]` are two keys. A provider
  for a bound key must bind each associated type to the stated type, or
  the entry is `type-mismatch`, and `$.use` of a bound key returns that
  bound trait value type. Row aliases may list bound keys. Four rule IDs
  are retired.
- `let` apply-pass answers (owner decisions Let 1-5 and Map 6 in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  2026-09-29): a multi-name `let` always puts its names in parentheses.
  `let (a, b) = pair`, previously a `syntax-error`, is valid, and its
  names are reassignable with readonly types. `let a, b = pair`,
  previously valid, is now a `syntax-error` whose fix-it adds the
  parentheses, as is `let mut log, db = pair`. A parenthesized list still
  needs two names. `a, b := pair` is unchanged. The other answers confirm
  applied readings: `mut-on-primitive` wherever a primitive type is
  written with `mut`, the `redundant-let-mut` warning, and
  `index-out-of-bounds` for a missing key in `m[k] op= v`.
  `grammar.stmt.let-mut-list.bare` and
  `grammar.stmt.let-mut-list.needs-mut` are retired.
- Associated type bindings AT6-AT7 (owner decisions in
  Open Issues,
  2026-09-29): an ambiguous projection, such as `I::Item` when two bounds
  on `I` both declare `Item`, is `ambiguous-associated-type`; it was an
  error with no code. A requirement key must bind every associated type of
  its trait, so `$ Store`, previously accepted when `Store` declares
  `Item`, is `trait-not-dynamically-safe`. `trait.assoc.ambiguous` and
  `trait.binding.ambiguous` are retired.
- Type-Argument Defaults TD11 and Dependencies DEP19 (owner decisions in
  Type-Argument Defaults
  and Dependencies,
  2026-09-29): TD11 confirms `argument-count` for a written type with too
  many arguments and changes nothing. A pseudo-version whose hash names no
  commit, or whose time does not match its commit, is invalid with no
  fallback, as a missing tag is; its code is named later with the other
  manifest diagnostics.
- Strings STR1-STR6 (owner decisions in
  Strings, 2026-09-29): a
  `string` is an immutable sequence of bytes that is always valid UTF-8,
  as in Go. `s.len()`, previously a scalar count, is the byte count, so
  `"héllo".len()` is 6, not 5. `s[i]`, previously
  `unsupported-string-indexing`, reads a byte as `u8`; that code is
  withdrawn. `string` does not implement `Iterable`, as before, and a
  rule now says so: `for c in s` is `unsatisfied-trait-bound`. The new
  built-in methods `chars`, `char_indices`, and `bytes` iterate
  explicitly, and `slice(start, end)` takes byte offsets. Every position a string method takes or returns is a
  byte offset. Equality and ordering compare bytes, which keeps the old
  order. Strings cross the host boundary as their bytes. Retired:
  `types.string.scalars`, `types.string.compare`, `types.string.len`,
  `types.string.no-indexing`, `types.string.traversal`,
  `types.string.host-utf8`, `expr.index.trait.read`,
  `expr.index.trait.builtin`, `expr.ord.std.text`, `module.method.i32`, and
  `module.string.scalar`.
- Error derivation (Error Conversion decision 10, owner decision in
  Error Conversion,
  2026-09-27): `@error` is a compiler intrinsic that derives `Display`,
  `Error`, and `From` for an enum or data type, as Rust's `thiserror` does.
  Chapter 14 named it only in prose before. An `@error` line always means
  the intrinsic, even where a binding named `error` is in scope, and inside
  an error type `@from` and `@source` are markers, not decorator values. A hand-written `Display`,
  `Error`, or matching `From` beside `@error`, and two `@from` members of
  one type, are `overlapping-impl`. `Error`'s `cause` method is now
  specified. `trait.error.api` is retired. No code is added; the codes for
  the other invalid forms are undecided.
- `let` batch 7 (owner decision Let 7 in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  2026-09-29): a parenthesized `let` list may be a same-line suite body,
  so `if ok: let (a, b) = pair`, previously a `syntax-error`, is valid.
  Its names are never read, so each gets the existing
  `unused-local-binding` warning. `if ok: a, b := pair` and
  `if ok: let a, b = pair` stay `syntax-error`, and a `for` over several
  names still needs an indented body. `grammar.inline.multi-name-let-for`
  is retired.
- Strings STR7-STR10 (owner decisions in
  Strings, 2026-09-29): a
  `slice` offset off a scalar boundary or past the end, and a `start`
  greater than `end`, panic as `index-out-of-bounds`; the boundary panic
  named no category before, and the reversed case had no rule. `List[T]`
  implements `Index[i32]` and `IndexSet[i32, T]`, `Map[K, V]` implements
  `Index[K]` and `IndexSet[K, V]`, and `string` implements `Index[i32]`
  with `Out = u8`. Generic code bounded by these traits, previously
  rejected with `unsatisfied-trait-bound` for these types, now accepts
  them. Through the bound, a `Map` read returns `V` and panics on a missing
  key. Built-in indexing is unchanged, and `s[i] = v` stays
  `invalid-assignment-target`. `module.string.slice.boundary` and
  `expr.index.trait.builtin-string` are retired.
- Spec follow-ups (owner decisions of 2026-09-27 in
  Spec Follow-Ups, applied 2026-09-29):
  confusable and mixed-script identifiers are warnings, not errors. Two
  available traits with same-named default methods make a dot call
  `ambiguous-method`. An expected type never weakens the
  elements of a list already built: returning
  `[for p in ps => Word { ... }].iter()` as `mut Iterator[Word]` is
  `type-mismatch`. One declared result type in a recursive cycle is
  enough. A lone `"` inside `"""..."""` is allowed. `all!` re-polls only
  unfinished children. `break` and `continue` are `never` expressions.
  Many rules now name the code their fixtures already expect, which
  changes no program's validity. The glossary lists every bold defined
  term. Retired: `trait.dyn.methods`, `trait.conflict.ambiguous`,
  `lex.ident.confusable`, `lex.ident.mixed-script`,
  `req.model.no-reinterpretation`, `req.schedule.all-order`,
  `fn.recursion.named`, `fn.decl.omitted-not-recursive`,
  `types.infer.explicit.results`, and `types.infer.body-result`.
- Error derivation batch 9 (Error Conversion decisions 21-27, owner
  decisions in
  Error Conversion,
  2026-09-29): the invalid `@error` forms now have codes. A misplaced
  `@error`, `@from`, or `@source`, such as `@error` before a function, a
  bare `@error` before a data type, or `@error("...")` before an enum, is
  `decorator-target-kind`. A second cause member, and `@from` on a bare
  type parameter, are the new error `invalid-error-marker`. A cause or
  transparent member that is not an `Error` is `unsatisfied-trait-bound`,
  and `$self` in a message is `unknown-name`. `$_0` in a message names the
  variant's unnamed payload member; unnamed shared enum data is not in
  scope. The generated `Error` bounds a carried-only type parameter by
  `Inspectable` and a transparent member's type parameter by `Error`;
  these bounds were undecided before.
  Writing `@error` needs no `use std.error.Error`, so a module that
  imports it only for `@error` may drop the import. No rule ID is retired.
- Spec follow-ups batch 10 (SF1-SF3, owner decisions in
  Spec Follow-Ups,
  2026-09-29): an inherent implementation outside the package that owns its
  target, such as `impl i32:` in an application, is `orphan-impl`. A raw tab
  inside a string or character literal, already invalid, is
  `tab-whitespace`; write `\t` instead. The code `missing-partial-eq` is
  renamed `missing-eq`, because `PartialEq` no longer exists: `==` or
  `assert_equal` on a type without `Eq` now reports `missing-eq`, and
  `missing-partial-eq` is retired. No rule ID is retired.
- Error derivation batch 11 (Error Conversion decisions 28-30, owner
  decisions in
  Error Conversion,
  2026-09-29): an `@error` line whose arguments are neither one message
  nor `transparent`, such as `@error(opaque)`, `@error(42)`, or
  `@error("closed", "shut")`, is `invalid-error-marker`; it was invalid
  without a code. A type parameter that only a message interpolates gets
  `P < Display & Inspectable` on the generated `Error`, a bound that was
  undecided before. The two readings of batch 9 stay as applied. No rule
  ID is retired.
- Property-test API batch 12 (Testing PT1-PT9, owner decisions in
  [Testing](../future-work/TESTING.md#owner-decisions), 2026-09-29):
  `Choices.int` and `Choices.float` are generic, as
  `int[N < Integer](lo: N, hi: N) -> N` and
  `float[F < Float](lo: F, hi: F) -> F`, so a draw takes its type from its
  bounds or its context. `value := c.int(0, 100)` is now an `i32`, and
  code that needs an `i64` writes `let value: i64 = c.int(0, 100)`.
  `float` draws only finite values. `Choices.string`'s parameter is
  renamed `max_chars` and counts chars, and `Choices.map` is new.
  `Choices` has no size, and a per-case draw budget, with no API, makes
  every draw return its simplest value once it is spent. `it_prop` and
  `it_prop_with` take `examples: List[T] = []`, run first on every run.
  A property body cannot discard a case. The default `f32` and `f64`
  generators include NaN, the infinities, `-0.0`, and subnormals.
  `@derive(Arbitrary)` is specified, tuned by one member fact,
  `arbitrary.with(gen)`; a generator of the wrong type panics on the
  first case. Retired: `module.testing.choices.int`,
  `module.testing.choices.float`, and `module.testing.choices.string`,
  replaced by `module.testing.choices.int-generic`, `.float-generic`, and
  `.string-chars`.
- Short binding lists (owner decision Q1 in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 13, 2026-09-30): a multi-name `:=` binding always puts its names
  in parentheses, as a multi-name `let` does. `(a, b) := pair`,
  previously a `syntax-error`, is valid, and `a, b := pair`, previously
  valid, is now a `syntax-error` whose fix-it adds the parentheses. A line
  that starts with `(` never continues the line before as a call, so
  `f` on one line and `(a, b) := pair` on the next are two statements.
  The grouped expression `(a, b := pair)` is unchanged, and
  `if ok: (a, b) := pair` is a `syntax-error`. No rule ID is retired.
- Error derivation batch 13 (Error Conversion decision 31, owner decision
  in Error Conversion,
  2026-09-30): an `@from` or `@source` line with arguments inside an error
  type, such as `@from(yaml)`, is `invalid-error-marker`; it was invalid
  without a code. No rule ID is retired.
- Spec follow-ups batch 13 (SF4, owner decision in
  Spec Follow-Ups,
  2026-09-30): an inherent implementation whose target is a tuple or a
  transparent alias is the new error `invalid-impl-target`, and one whose
  target is a trait value type is `trait-value-impl-target`. Both were
  invalid without a code. No rule ID is retired.
- Property-test batch 13 (Testing Q5-Q10, owner decisions in
  [Testing](../future-work/TESTING.md#owner-decisions), 2026-09-30):
  `arbitrary.with` is generic,
  `with[T < Inspectable](gen: fn(mut Choices) -> T) -> Generator`, so a
  generator whose values are not inspectable is `unsatisfied-trait-bound`,
  and so is a tuned member whose type is not inspectable. The derived
  code downcasts the first drawn value; a failed downcast panics with
  `explicit-panic`, naming the member and both types. A derived enum whose
  every variant is recursive panics on the first case with
  `explicit-panic`; `List`, `Map`, and optional members do not make a
  variant recursive. Once the draw budget is spent, `T?` draws `.None`,
  `Result[T, E]` draws `.Ok` of `T`'s simplest value, and a tuple draws
  each element's simplest value. Retired:
  `module.testing.arbitrary.with.mismatch`, replaced by
  `module.testing.arbitrary.with.downcast-failure`.
- Provider scope batch 14 (PS1-PS3, owner decision in
  [Open Issues](../future-work/OPEN_ISSUES.md#provider-scope-overlap),
  2026-09-30): a closure or local `fn` no longer captures providers from an
  enclosing `$.with` block. Each key its body uses goes into its declared
  or inferred row and is resolved at each call, so a callee's `$.with` may
  supply it. A closure written in a `$.with` block that uses a key and then
  escapes the block, or is passed to an empty-row parameter such as a
  `filter` callback, is now `type-mismatch`. Lexical scope is explicit:
  capture the value, as in `clock := $.use(Clock)` and then
  `fn(): clock.now()`. Direct calls inside a `$.with` block, and providers
  bound when a suspension is constructed, are unchanged. The overlap
  `req.with.nearest.forced` is intended behavior. Retired:
  `req.row.omitted.closure`, replaced by `req.row.omitted.closure-row` and
  `req.row.omitted.outer-scope`; `fn.capture.providers` and
  `fn.capture.providers.bound`, replaced by `fn.capture.no-providers` and
  `fn.capture.provider-value`.
- Same-line `:=` lists (owner decision Q1a in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): `if ok: (a, b) := pair`, previously a
  `syntax-error`, is valid, as `if ok: let (a, b) = pair` is.
  `unused-local-binding` reports the names it binds. Retired:
  `grammar.inline.multi-name-binding`, replaced by
  `grammar.inline.bind-list`.
- Grouped bindings dropped (owner decision Q1b in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): the grouped expression `(a, b := value)`,
  previously valid, is now a `syntax-error` whose fix-it writes
  `(a, b) := value`. A nested multi-name binding is written
  `((a, b) := value)`, and `[(a, b) := pair]` is
  `multi-binding-needs-parentheses`. Retired:
  `grammar.expr.multi-binding.nested` and
  `grammar.expr.multi-binding.not-tuple`, replaced by
  `grammar.expr.multi-binding.wrapped`,
  `grammar.expr.multi-binding.no-grouped`, and
  `grammar.expr.multi-binding.no-grouped.fix`.
- Redundant `mut` in a `let` list (owner decision LM-a in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): in `let (mut a, b): (mut User, User) = pair`,
  the `mut` before `a` now warns `redundant-let-mut`, as
  `let mut a: mut User` does. In both forms the fix-it removes the `mut`
  before the name and keeps the annotation. No rule ID is retired.
- `mut self` on a primitive (owner decision LM-b in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): a `mut self` receiver in an impl whose `Self` is
  primitive is valid; it is not `mut-on-primitive`. No rule ID is
  retired.
- Monomorphic closures (owner decision CLO1 in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): a closure declares no type parameters, and
  `fn[T](x: T): x` is a `syntax-error`. The grammar already rejected it;
  the rule is now stated. No rule ID is retired.
- `?` operand hint (owner decision Q-? in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): the operand of `x?` gets an expected type as an
  inference hint, never a coercion: `Result[T, E]` with the enclosing
  function's error type, or `T?`. So `let ports: List[i32] = it.collect()?`
  builds a `Result` or optional target instead of the default `List`. No
  rule ID is retired.
- `Map` trait implementations (owner decision Q-map in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 15, 2026-09-30): the standard library writes `Iterable` and
  `FromIterator` for `Map[K, V]` with `K < Eq & Hash`, with no std-only
  exception. Source is unaffected. No rule ID is retired.
- Collision check in closures (owner decision PS3a in
  [Open Issues](../future-work/OPEN_ISSUES.md#provider-scope-overlap),
  batch 15, 2026-09-30): inside a closure, a `$.with` compares its keys
  with the closure's declared or inferred row and the `$.with` blocks
  inside the closure only. A key bound by a `$.with` around the closure no
  longer collides, and a key of the closure's declared row now can. No
  rule ID is retired.
- Trailing block on the same line (owner decision TB1 in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 16, 2026-09-30): editorial.
  `grammar.call.trailing-block.next-line` now names its code,
  `syntax-error`, and examples show that `if close: trailing(): pass` and
  `if close: trailing:` with an indented body are both errors. Source is
  unaffected. No rule ID is retired.
- `self` in a primitive `mut self` method (owner decision LM-c in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 16, 2026-09-30): in an impl whose `Self` is primitive, the `mut`
  of a `mut self` receiver is dropped, so `self` has the plain type
  `Self`. In an impl for `i32`, `self + 1` is valid, and a call such as
  `start.next()` needs no mutable access. No rule ID is retired.
- Self references (owner decision SR1 in
  [Typed Derivation](../future-work/TYPED_DERIVATION.md#owner-decision-sr1-2026-09-30),
  batch 17, 2026-09-30): `std.structure` declares `enum SelfRef` with
  `Absent`, `Optional`, and `Required`, and `Member` and `VariantInfo`
  gain a compiler-computed `self_ref` field. A `Member` or `VariantInfo`
  built by hand, if any, now needs it. Derived `Arbitrary` is an ordinary
  `std.testing` template that reads `self_ref`. A derived data type with a
  `.Required` member, such as `data Ring: next: Ring`, now panics on the
  first case as an enum with no finite value does. No rule ID is retired.
- Generic inference from several arguments (owner decision INF-mut in
  [Follow-Ups Decided 2026-09-29 (Evening)](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  batch 17, 2026-09-30): when one type parameter is solved from several
  arguments, only permission weakening joins their types. `max(small,
  large)` with an `i32` and an `i64` is now a `type-mismatch`; write
  `max(i64(small), large)`. `cmp(user, label)` with a `User` and a
  `Display` is `no-common-type`; write `cmp[Display](user, label)`. This
  applies to `assert_equal` too. No rule ID is retired.
- Property tests move to the stdlib tier (owner decisions ST2, ST3, and ST6
  in [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 4, 2026-09-30): stdlib tier. The Property Tests and Draw Budget
  sections of Modules move to [Testing](std/testing.md), with their text
  unchanged. Source is unaffected. `module.testing.choices.*` becomes
  `std-testing.choices.*`, `module.testing.arbitrary`, `.arbitrary.std`, and
  `.arbitrary.float` become `std-testing.arbitrary`, `.arbitrary.std`, and
  `.arbitrary.float`, `module.testing.prop.*` becomes `std-testing.prop.*`,
  and `module.testing.budget` and `module.testing.budget.*` become
  `std-testing.budget` and `std-testing.budget.*`. The old IDs are
  retired. The signatures of `it_prop` and `it_prop_with` stay in
  [Table Tests](10-modules.md#table-tests) with their registration rules.
- Test timeouts, table-test rows, and snapshot files move to the stdlib
  tier (owner decisions ST2, ST3, and ST6 in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 6, 2026-09-30): stdlib tier. Rules move from Modules' Test Cases,
  Table Tests, and Snapshots to the new sections
  [Test Timeout](std/testing.md#test-timeout),
  [Table-Test Rows](std/testing.md#table-test-rows), and
  [Snapshot Files](std/testing.md#snapshot-files) of Testing, with their
  text unchanged. Source is unaffected. `module.testing.option.timeout-any-duration`,
  `.option.timeout-at-run`, and `.option.timeout-import` become
  `std-testing.option.timeout-any-duration`, `.option.timeout-at-run`, and
  `.option.timeout-import`. `module.testing.it-each`, `.it-each.name`, and
  `.it-each.rows-at-run` become `std-testing.it-each`, `.it-each.name`, and
  `.it-each.rows-at-run`. `module.testing.snapshot`, `.snapshot.import`, and
  `.snapshot.mismatch` become `std-testing.snapshot`, `.snapshot.import`,
  and `.snapshot.mismatch`, and `module.testing.snapshot-file` and
  `module.testing.snapshot-file.*` become `std-testing.snapshot-file` and
  `std-testing.snapshot-file.*`. The old IDs are retired. The language tier
  keeps what the compiler checks: the `timeout` parameter's type, the
  registration and name-clash rules of `it_each`, and the `snapshot`
  signature with its literal `expect`
  ([`module.testing.snapshot.literal`](10-modules.md#r-module.testing.snapshot.literal)).
  [`module.testing.it.options-strings`](10-modules.md#r-module.testing.it.options-strings)
  now names the three options itself, since the option table no longer
  lists `timeout`; its meaning is unchanged.
- Derived `Arbitrary` moves to the stdlib tier (owner decisions ST2, ST3,
  and ST7 in [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions),
  migration step 5, 2026-09-30): stdlib tier. Modules' Derived Arbitrary
  section moves to [Derived Arbitrary](std/testing.md#derived-arbitrary)
  in Testing, and its heading is deleted. Source is unaffected.
  `module.testing.arbitrary.derive` and `module.testing.arbitrary.derive.*`
  become `std-testing.arbitrary.derive` and `std-testing.arbitrary.derive.*`.
  `module.testing.arbitrary.with` and `module.testing.arbitrary.with.*`
  become `std-testing.arbitrary.with` and `std-testing.arbitrary.with.*`,
  except `.with.inspectable`, which AT-with retires below. The old IDs are
  retired. [`module.testing.it-prop`](10-modules.md#r-module.testing.it-prop)
  keeps the registration of a property test case; its sentence on how the
  runner generates and shrinks inputs becomes
  [`std-testing.it-prop`](std/testing.md#r-std-testing.it-prop).
- Derived `Arbitrary` member bounds (owner decision AT-with in
  [Testing](../future-work/TESTING.md#owner-decisions), batch 20,
  2026-09-30): stdlib tier. The derived template requires every member to
  implement `Arbitrary` and to be inspectable, whether or not
  `arbitrary.with` tunes it. A tuned member whose type has no `Arbitrary`,
  previously valid, is now `unsatisfied-trait-bound`, and so is a member of
  a function type. The error is reported at `@derive(Arbitrary)` and names
  the member; a tuned member that is not inspectable was reported on its
  `@arbitrary.with` line. Such a type writes its `impl Arbitrary` by hand.
  A generator of the wrong type still panics with `explicit-panic`.
  Retired: `module.testing.arbitrary.with.inspectable`, replaced by
  `std-testing.arbitrary.derive.member-bounds`, `.derive.not-derivable`,
  and `.derive.manual`.
- Self references, restated (owner decision SIMPLE in
  [Typed Derivation](../future-work/TYPED_DERIVATION.md#owner-decision-simple-2026-09-30),
  batch 20, 2026-09-30): language tier. `self_ref` follows from a member's
  type alone, with no simplest value. A member whose type is another enum,
  every variant of which needs the enclosing type, was `.Optional` and is
  now `.Required`. So a derived `Arbitrary` for such a type panics on the
  first case instead of never ending. The enclosing type counts with any
  type arguments, as `Nest[List[T]]` inside `enum Nest[T]`. Retired:
  `annot.self-ref.needs`, `.needs.forms`, and `.needs.containers`, replaced
  by `annot.self-ref.enclosing.arguments`, `.needs.self`, `.needs.compound`,
  `.needs.result`, `.needs.enum`, `.needs.stop`, and `.needs.only`.
- Structure names (owner decision ST8, revised, in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions),
  2026-09-30): language tier. `Structure` gains the receiverless,
  compiler-supplied `fn name() -> string`: the target's declared name, with
  no module path and no type arguments, a compile-time constant callable
  only inside a template. A newtype has its own name; a transparent alias
  has its base type's. In the stdlib tier, the no-finite panic of derived
  `Arbitrary` reads `"${T::name()} has no finite value"`
  ([`std-testing.arbitrary.derive.no-finite.message`](std/testing.md#r-std-testing.arbitrary.derive.no-finite.message)).
  Source is unaffected. No rule ID is retired.
- Iterator adapters and collect targets move to the stdlib tier (owner
  decisions ST2 and ST3 in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 7, 2026-09-30): stdlib tier. Control Flow's Iterator Adapters and
  Collect Targets sections move to [Iterators](std/iter.md), with their
  text unchanged, and their headings are deleted. `map` on a list or an
  optional leaves Modules' Built-In Methods for
  [List And Optional Map](std/iter.md#list-and-optional-map). Source is
  unaffected. `flow.adapter.*` becomes `std-iter.adapter.*`,
  `flow.collect.*` becomes `std-iter.collect.*`, `module.method.map`
  becomes `std-iter.method.map`, and `module.prelude.from-iterator`
  becomes `std-iter.prelude.from-iterator`. The old IDs are retired.
  `Iterator[T]`, `from_fn`, `next`, and `Iterable` stay in
  [Iteration Protocols](06-control-flow.md#iteration-protocols): `for`
  depends on them. No language rule names an adapter or `collect`.
- String methods and the `r` prefix move to the stdlib tier (owner
  decisions ST2 and ST3 in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 8, 2026-09-30): stdlib tier. The rules for `lower`, `trim`, `split`,
  `replace`, and `starts_with` leave Modules' String Methods, and their
  signatures leave the Built-In Methods table, for
  [String Methods](std/text.md#string-methods) in Text. The `r` rules
  leave Expressions' Prefixed Strings, and `module.prelude.text-r` leaves
  Standard Names Outside The Prelude, for
  [Raw Text Prefix](std/text.md#raw-text-prefix). Text is unchanged, and
  source is unaffected. `module.string.lower`, `.trim`, `.split`,
  `.split.empty-separator`, `.split.absent`, `.replace`, `.replace.empty`,
  and `.starts-with` become `std-text.string.lower` and so on,
  `expr.prefix.std.*` becomes `std-text.prefix.std.*`, and
  `module.prelude.text-r` becomes `std-text.prelude.text-r`. The old IDs
  are retired. `len`, `s[i]`, `bytes`, `slice`, `chars`, and
  `char_indices` stay in [String Methods](10-modules.md#string-methods):
  [`flow.for.string-explicit`](06-control-flow.md#r-flow.for.string-explicit)
  names `chars` and `char_indices`, so the tier test keeps them.
- Debug text and builders move to the stdlib tier (owner decisions ST2
  and ST3 in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 9, 2026-09-30): stdlib tier. `trait.debug.render` and Traits'
  Debug Builders section, with the derived builder mapping, move to
  [Format](std/format.md), with their text unchanged, and the Debug
  Builders heading is deleted. Source is unaffected.
  `trait.debug.render` becomes `std-format.debug.render`,
  `trait.debug.builder.*` becomes `std-format.debug.builder.*`,
  `trait.debug.layout` becomes `std-format.debug.layout`, and
  `trait.debug.derive-builders*` becomes
  `std-format.debug.derive-builders*`. The old IDs are retired. The other
  `trait.debug.*` rules stay in [Debug Trait](09-traits.md#debug-trait):
  `Debug` and `debug` are prelude names, and `Debug`'s method names
  `DebugWriter`.
- `Duration` and its suffixes move to the stdlib tier (owner decisions
  ST2 and ST3 in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 9, 2026-09-30): stdlib tier. The `std.time` suffix rules leave
  Expressions' Literal Suffixes, and `module.prelude.time-suffixes`
  leaves Standard Names Outside The Prelude, for [Time](std/time.md),
  with their text unchanged. Source is unaffected. `expr.suffix.std.*`
  becomes `std-time.suffix.std.*`, and `module.prelude.time-suffixes`
  becomes `std-time.prelude.time-suffixes`. The old IDs are retired. The
  suffix mechanism, `@num_suffix`, stays language tier, and the language
  chapters' suffix examples declare their own suffix functions. The test
  signatures keep `timeout: Duration?`; a Note in
  [Test Cases](10-modules.md#test-cases) says `Duration` is the
  stdlib-tier `std.time.Duration`, named there only.
- Batch 21 follow-ups to batch 20 (owner decisions AT-any, ST8-newtype,
  and ST8-clash in [Open Issues](../future-work/OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening),
  2026-09-30): both tiers, Notes only. Source is unaffected, and no rule
  ID changes. AT-any confirms that `arbitrary.with` returns an opaque
  `Generator`, not a raw `Any`
  ([Derived Arbitrary](std/testing.md#derived-arbitrary)). ST8-newtype
  accepts that a newtype, which gets no `Structure`, panics with its base
  type's name ([The Structure Trait](14-annotations.md#the-structure-trait)).
  ST8-clash extends the Note on generated names in
  [Templates](14-annotations.md#templates) to `facts` and `name`: a clash
  with the derived trait's own receiverless member is resolved by
  qualifying the call.
- Language-chapter examples and the raw-string rule leave the stdlib tier
  (owner decisions ST2 and ST3 in
  [Spec Tiers](../future-work/SPEC_TIERS.md#owner-decisions), migration
  step 11, 2026-09-30): language tier. `lex.raw-string.none-text`, which
  named `std.text.r` as the prefix of `r"..."`, is retired for
  [`lex.raw-string.prefix`](01-lexical-structure.md#r-lex.raw-string.prefix):
  `r` resolves as any prefix name does, and a See also points to
  [Raw Text Prefix](std/text.md#raw-text-prefix). Examples in Lexical
  Structure, Expressions, Functions, and Traits that called `trim`,
  `lower`, `replace`, `split`, `filter`, `map`, or `collect` now use
  `slice`, `len`, interpolation, or a local helper. Source is unaffected.
  The stdlib terms move to a [Glossary](std/README.md#glossary) of their
  own.
