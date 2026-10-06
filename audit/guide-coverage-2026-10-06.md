# Guide Coverage Report

Date: 2026-10-06. Method: every `##` section of each spec chapter (plus
named constructs new users ask for), grepped across `guide/`. `none` =
no mention; `brief` = named in passing or one fragment; `full` = a
subsection or more with runnable code. Spot-checked, not exhaustive.

## The ten gaps a new user hits first

1. **JSON and serde** (`spec/std/json.md`, `spec/std/serde.md`): no
   mention of `Serialize`/`Deserialize`, `encode`/`decode`, or reading a
   config file. A config loader is every user's second program.
2. **Doc tests**: zero mentions. A `##` fenced `hd` block in a doc
   comment runs under `hd test`; nothing tells the user.
3. **`race!` and timeouts** (`spec/std/task.md`): `all!` is named,
   `race!` never is. No timeout pattern anywhere.
4. **File I/O** (`spec/std/fs.md`, `spec/std/path.md`): brief.
   `read_text`/`write_text` and `Path` appear; `FsRead`/`FsWrite`
   providers, `MemoryFs`, and temp dirs do not.
5. **Regex** (`spec/std/regex.md`): none. No log-scanning story.
6. **Testing workflow** (`spec/std/testing.md`): `it`, `assert`,
   `it_each`, `it_prop`, `Arbitrary`, `ManualClock` are covered;
   `snapshot`/`snapshot_file`, `temp_dir`, `hd_run`, `SeededRandom`,
   and `MapArgs`/`MapEnv` are none.
7. **Publishing a package** (`spec/lang/10-modules.md` Versions,
   Integrity, Toolchain): `hd.toml`, `use`, and workspaces are covered;
   versions, `hd.sum`, and the toolchain pin are none.
8. **Collections depth** (`spec/std/collections.md`,
   `spec/std/iter.md`): `List`/`Map` basics are full; `Set`,
   `sorted`/`group_by`, `ListView`, `Deque`, `Heap` are none.
9. **Time** (`spec/std/time.md`): `Duration` literals and `sleep` are
   brief; `Timestamp`/`parse_rfc3339`/`to_rfc3339` and `Instant` are
   brief-to-none.
10. **String building** (`spec/std/text.md`): `trim`/`split` are full;
    `join`, `StringBuilder`, `repeat`, and `chars`/`bytes` are brief.

## spec/lang/01-lexical-structure.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Processing Model | [spec/lang/01-lexical-structure.md#processing-model](spec/lang/01-lexical-structure.md#processing-model) | none |
| Physical And Logical Lines | [spec/lang/01-lexical-structure.md#physical-and-logical-lines](spec/lang/01-lexical-structure.md#physical-and-logical-lines) | brief |
| Whitespace And Indentation | [spec/lang/01-lexical-structure.md#whitespace-and-indentation](spec/lang/01-lexical-structure.md#whitespace-and-indentation) | brief |
| Comments | [spec/lang/01-lexical-structure.md#comments](spec/lang/01-lexical-structure.md#comments) | full |
| Identifiers | [spec/lang/01-lexical-structure.md#identifiers](spec/lang/01-lexical-structure.md#identifiers) | brief |
| Keywords And Reserved Words | [spec/lang/01-lexical-structure.md#keywords-and-reserved-words](spec/lang/01-lexical-structure.md#keywords-and-reserved-words) | none |
| Literals | [spec/lang/01-lexical-structure.md#literals](spec/lang/01-lexical-structure.md#literals) | full |
| Operators And Delimiters | [spec/lang/01-lexical-structure.md#operators-and-delimiters](spec/lang/01-lexical-structure.md#operators-and-delimiters) | brief |
| Lexical Token Grammar | [spec/lang/01-lexical-structure.md#lexical-token-grammar](spec/lang/01-lexical-structure.md#lexical-token-grammar) | none |
| Unsupported Lexical Extensions | [spec/lang/01-lexical-structure.md#unsupported-lexical-extensions](spec/lang/01-lexical-structure.md#unsupported-lexical-extensions) | none |

## spec/lang/02-grammar.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Source Files And Suites | [spec/lang/02-grammar.md#source-files-and-suites](spec/lang/02-grammar.md#source-files-and-suites) | brief |
| Test Blocks | [spec/lang/02-grammar.md#test-blocks](spec/lang/02-grammar.md#test-blocks) | full |
| Statements | [spec/lang/02-grammar.md#statements](spec/lang/02-grammar.md#statements) | full |
| Declarations | [spec/lang/02-grammar.md#declarations](spec/lang/02-grammar.md#declarations) | full |
| Generic Parameters And Bounds | [spec/lang/02-grammar.md#generic-parameters-and-bounds](spec/lang/02-grammar.md#generic-parameters-and-bounds) | full |
| Types | [spec/lang/02-grammar.md#types](spec/lang/02-grammar.md#types) | full |
| Use Declarations | [spec/lang/02-grammar.md#use-declarations](spec/lang/02-grammar.md#use-declarations) | full |
| Expressions | [spec/lang/02-grammar.md#expressions](spec/lang/02-grammar.md#expressions) | full |
| Control-Flow Expressions | [spec/lang/02-grammar.md#control-flow-expressions](spec/lang/02-grammar.md#control-flow-expressions) | full |
| Patterns | [spec/lang/02-grammar.md#patterns](spec/lang/02-grammar.md#patterns) | full |
| Comprehensions | [spec/lang/02-grammar.md#comprehensions](spec/lang/02-grammar.md#comprehensions) | full |
| Requirements And Provider Contexts | [spec/lang/02-grammar.md#requirements-and-provider-contexts](spec/lang/02-grammar.md#requirements-and-provider-contexts) | full |
| Annotations | [spec/lang/02-grammar.md#annotations](spec/lang/02-grammar.md#annotations) | full |

## spec/lang/03-names-and-scopes.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Name Categories | [spec/lang/03-names-and-scopes.md#name-categories](spec/lang/03-names-and-scopes.md#name-categories) | brief |
| Lexical Scopes | [spec/lang/03-names-and-scopes.md#lexical-scopes](spec/lang/03-names-and-scopes.md#lexical-scopes) | full |
| Module Scope | [spec/lang/03-names-and-scopes.md#module-scope](spec/lang/03-names-and-scopes.md#module-scope) | full |
| Use Declarations | [spec/lang/03-names-and-scopes.md#use-declarations](spec/lang/03-names-and-scopes.md#use-declarations) | full |
| Local Bindings | [spec/lang/03-names-and-scopes.md#local-bindings](spec/lang/03-names-and-scopes.md#local-bindings) | full |
| Binding Expressions | [spec/lang/03-names-and-scopes.md#binding-expressions](spec/lang/03-names-and-scopes.md#binding-expressions) | full |
| Function And Closure Scopes | [spec/lang/03-names-and-scopes.md#function-and-closure-scopes](spec/lang/03-names-and-scopes.md#function-and-closure-scopes) | full |
| Control-Flow Binding Scopes | [spec/lang/03-names-and-scopes.md#control-flow-binding-scopes](spec/lang/03-names-and-scopes.md#control-flow-binding-scopes) | brief |
| Member Resolution | [spec/lang/03-names-and-scopes.md#member-resolution](spec/lang/03-names-and-scopes.md#member-resolution) | full |
| Unsupported Scope Extensions | [spec/lang/03-names-and-scopes.md#unsupported-scope-extensions](spec/lang/03-names-and-scopes.md#unsupported-scope-extensions) | none |

## spec/lang/04-type-system.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Type Forms | [spec/lang/04-type-system.md#type-forms](spec/lang/04-type-system.md#type-forms) | full |
| Primitive Types | [spec/lang/04-type-system.md#primitive-types](spec/lang/04-type-system.md#primitive-types) | full |
| Literal Types | [spec/lang/04-type-system.md#literal-types](spec/lang/04-type-system.md#literal-types) | full |
| Nominal And Structural Types | [spec/lang/04-type-system.md#nominal-and-structural-types](spec/lang/04-type-system.md#nominal-and-structural-types) | brief |
| Transparent Aliases And Newtypes | [spec/lang/04-type-system.md#transparent-aliases-and-newtypes](spec/lang/04-type-system.md#transparent-aliases-and-newtypes) | brief |
| Optional Types | [spec/lang/04-type-system.md#optional-types](spec/lang/04-type-system.md#optional-types) | full |
| Result Types | [spec/lang/04-type-system.md#result-types](spec/lang/04-type-system.md#result-types) | full |
| Numeric Conversions | [spec/lang/04-type-system.md#numeric-conversions](spec/lang/04-type-system.md#numeric-conversions) | full |
| Assignability And Coercion | [spec/lang/04-type-system.md#assignability-and-coercion](spec/lang/04-type-system.md#assignability-and-coercion) | brief |
| Composite Values And Access Permission | [spec/lang/04-type-system.md#composite-values-and-access-permission](spec/lang/04-type-system.md#composite-values-and-access-permission) | full |
| Generics | [spec/lang/04-type-system.md#generics](spec/lang/04-type-system.md#generics) | full |
| Variance | [spec/lang/04-type-system.md#variance](spec/lang/04-type-system.md#variance) | full |
| Trait Values And `Any` | [spec/lang/04-type-system.md#trait-values-and-any](spec/lang/04-type-system.md#trait-values-and-any) | brief |
| Map Key Types | [spec/lang/04-type-system.md#map-key-types](spec/lang/04-type-system.md#map-key-types) | none |
| Least Common Type | [spec/lang/04-type-system.md#least-common-type](spec/lang/04-type-system.md#least-common-type) | none |
| Type Inference Boundaries | [spec/lang/04-type-system.md#type-inference-boundaries](spec/lang/04-type-system.md#type-inference-boundaries) | brief |
| Implementation Model (Non-Normative) | [spec/lang/04-type-system.md#implementation-model-non-normative](spec/lang/04-type-system.md#implementation-model-non-normative) | none |
| Unsupported Type-System Extensions | [spec/lang/04-type-system.md#unsupported-type-system-extensions](spec/lang/04-type-system.md#unsupported-type-system-extensions) | none |

## spec/lang/05-expressions.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Evaluation Order | [spec/lang/05-expressions.md#evaluation-order](spec/lang/05-expressions.md#evaluation-order) | brief |
| Expression Categories | [spec/lang/05-expressions.md#expression-categories](spec/lang/05-expressions.md#expression-categories) | brief |
| Primary Expressions | [spec/lang/05-expressions.md#primary-expressions](spec/lang/05-expressions.md#primary-expressions) | full |
| Postfix Expressions | [spec/lang/05-expressions.md#postfix-expressions](spec/lang/05-expressions.md#postfix-expressions) | full |
| Unary And Binary Operators | [spec/lang/05-expressions.md#unary-and-binary-operators](spec/lang/05-expressions.md#unary-and-binary-operators) | full |
| Pipe Expressions | [spec/lang/05-expressions.md#pipe-expressions](spec/lang/05-expressions.md#pipe-expressions) | full |
| Range Expressions | [spec/lang/05-expressions.md#range-expressions](spec/lang/05-expressions.md#range-expressions) | full |
| Binding Expressions | [spec/lang/05-expressions.md#binding-expressions](spec/lang/05-expressions.md#binding-expressions) | full |
| Comprehensions | [spec/lang/05-expressions.md#comprehensions](spec/lang/05-expressions.md#comprehensions) | full |
| Closures And Control Expressions | [spec/lang/05-expressions.md#closures-and-control-expressions](spec/lang/05-expressions.md#closures-and-control-expressions) | full |
| Unsupported Expression Extensions | [spec/lang/05-expressions.md#unsupported-expression-extensions](spec/lang/05-expressions.md#unsupported-expression-extensions) | none |

## spec/lang/06-control-flow.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Blocks And Completion | [spec/lang/06-control-flow.md#blocks-and-completion](spec/lang/06-control-flow.md#blocks-and-completion) | full |
| Conditional Expressions | [spec/lang/06-control-flow.md#conditional-expressions](spec/lang/06-control-flow.md#conditional-expressions) | full |
| For Loops | [spec/lang/06-control-flow.md#for-loops](spec/lang/06-control-flow.md#for-loops) | full |
| While Loops | [spec/lang/06-control-flow.md#while-loops](spec/lang/06-control-flow.md#while-loops) | full |
| Break, Continue, And Loop Else | [spec/lang/06-control-flow.md#break-continue-and-loop-else](spec/lang/06-control-flow.md#break-continue-and-loop-else) | full |
| Match Expressions | [spec/lang/06-control-flow.md#match-expressions](spec/lang/06-control-flow.md#match-expressions) | full |
| Let Patterns | [spec/lang/06-control-flow.md#let-patterns](spec/lang/06-control-flow.md#let-patterns) | full |
| Return | [spec/lang/06-control-flow.md#return](spec/lang/06-control-flow.md#return) | full |
| Deferred Cleanup | [spec/lang/06-control-flow.md#deferred-cleanup](spec/lang/06-control-flow.md#deferred-cleanup) | full |
| Unreachable Code | [spec/lang/06-control-flow.md#unreachable-code](spec/lang/06-control-flow.md#unreachable-code) | none |
| Runtime Panics | [spec/lang/06-control-flow.md#runtime-panics](spec/lang/06-control-flow.md#runtime-panics) | full |

## spec/lang/07-functions.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Declarations | [spec/lang/07-functions.md#declarations](spec/lang/07-functions.md#declarations) | full |
| Parameters | [spec/lang/07-functions.md#parameters](spec/lang/07-functions.md#parameters) | full |
| Function Types And Values | [spec/lang/07-functions.md#function-types-and-values](spec/lang/07-functions.md#function-types-and-values) | full |
| Closures | [spec/lang/07-functions.md#closures](spec/lang/07-functions.md#closures) | full |
| Multiple Inline Closures | [spec/lang/07-functions.md#multiple-inline-closures](spec/lang/07-functions.md#multiple-inline-closures) | brief |
| Trailing Callback Blocks | [spec/lang/07-functions.md#trailing-callback-blocks](spec/lang/07-functions.md#trailing-callback-blocks) | full |
| Generic Functions | [spec/lang/07-functions.md#generic-functions](spec/lang/07-functions.md#generic-functions) | full |
| Methods And Receivers | [spec/lang/07-functions.md#methods-and-receivers](spec/lang/07-functions.md#methods-and-receivers) | full |
| Recursion | [spec/lang/07-functions.md#recursion](spec/lang/07-functions.md#recursion) | brief |
| Program Entry Functions | [spec/lang/07-functions.md#program-entry-functions](spec/lang/07-functions.md#program-entry-functions) | full |
| Unsupported Function Extensions | [spec/lang/07-functions.md#unsupported-function-extensions](spec/lang/07-functions.md#unsupported-function-extensions) | none |

## spec/lang/08-data-and-enums.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Data Declarations | [spec/lang/08-data-and-enums.md#data-declarations](spec/lang/08-data-and-enums.md#data-declarations) | full |
| Construction And Access | [spec/lang/08-data-and-enums.md#construction-and-access](spec/lang/08-data-and-enums.md#construction-and-access) | full |
| Data Embedding | [spec/lang/08-data-and-enums.md#data-embedding](spec/lang/08-data-and-enums.md#data-embedding) | full |
| Enum Declarations | [spec/lang/08-data-and-enums.md#enum-declarations](spec/lang/08-data-and-enums.md#enum-declarations) | full |
| Shared Enum Constructor Data | [spec/lang/08-data-and-enums.md#shared-enum-constructor-data](spec/lang/08-data-and-enums.md#shared-enum-constructor-data) | brief |
| Generic And Recursive Enums | [spec/lang/08-data-and-enums.md#generic-and-recursive-enums](spec/lang/08-data-and-enums.md#generic-and-recursive-enums) | brief |
| Matching Enums | [spec/lang/08-data-and-enums.md#matching-enums](spec/lang/08-data-and-enums.md#matching-enums) | full |
| Option And Result | [spec/lang/08-data-and-enums.md#option-and-result](spec/lang/08-data-and-enums.md#option-and-result) | full |
| Representation And Garbage Collection | [spec/lang/08-data-and-enums.md#representation-and-garbage-collection](spec/lang/08-data-and-enums.md#representation-and-garbage-collection) | none |
| Generalized Algebraic Data Types | [spec/lang/08-data-and-enums.md#generalized-algebraic-data-types](spec/lang/08-data-and-enums.md#generalized-algebraic-data-types) | brief |
| Typed Derivation Of Data And Enums | [spec/lang/08-data-and-enums.md#typed-derivation-of-data-and-enums](spec/lang/08-data-and-enums.md#typed-derivation-of-data-and-enums) | full |
| Unsupported Aggregate Extensions | [spec/lang/08-data-and-enums.md#unsupported-aggregate-extensions](spec/lang/08-data-and-enums.md#unsupported-aggregate-extensions) | none |

## spec/lang/09-traits.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Trait Declarations | [spec/lang/09-traits.md#trait-declarations](spec/lang/09-traits.md#trait-declarations) | full |
| Trait Implementations | [spec/lang/09-traits.md#trait-implementations](spec/lang/09-traits.md#trait-implementations) | full |
| Inherent Implementations | [spec/lang/09-traits.md#inherent-implementations](spec/lang/09-traits.md#inherent-implementations) | full |
| Method Resolution | [spec/lang/09-traits.md#method-resolution](spec/lang/09-traits.md#method-resolution) | full |
| Generic Bounds And Static Dispatch | [spec/lang/09-traits.md#generic-bounds-and-static-dispatch](spec/lang/09-traits.md#generic-bounds-and-static-dispatch) | full |
| Dynamic Trait Values | [spec/lang/09-traits.md#dynamic-trait-values](spec/lang/09-traits.md#dynamic-trait-values) | brief |
| `Any` | [spec/lang/09-traits.md#any](spec/lang/09-traits.md#any) | brief |
| Sealed Traits | [spec/lang/09-traits.md#sealed-traits](spec/lang/09-traits.md#sealed-traits) | brief |
| Runtime Type Identity | [spec/lang/09-traits.md#runtime-type-identity](spec/lang/09-traits.md#runtime-type-identity) | full |
| Embedding And Trait Satisfaction | [spec/lang/09-traits.md#embedding-and-trait-satisfaction](spec/lang/09-traits.md#embedding-and-trait-satisfaction) | brief |
| Trait Delegation | [spec/lang/09-traits.md#trait-delegation](spec/lang/09-traits.md#trait-delegation) | brief |
| Default-Method Conflicts | [spec/lang/09-traits.md#default-method-conflicts](spec/lang/09-traits.md#default-method-conflicts) | none |
| Unsupported Trait Extensions | [spec/lang/09-traits.md#unsupported-trait-extensions](spec/lang/09-traits.md#unsupported-trait-extensions) | none |

## spec/lang/10-modules.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Package Manifest | [spec/lang/10-modules.md#package-manifest](spec/lang/10-modules.md#package-manifest) | full |
| Path-Inferred Modules | [spec/lang/10-modules.md#path-inferred-modules](spec/lang/10-modules.md#path-inferred-modules) | full |
| Use Roots | [spec/lang/10-modules.md#use-roots](spec/lang/10-modules.md#use-roots) | full |
| Use Forms | [spec/lang/10-modules.md#use-forms](spec/lang/10-modules.md#use-forms) | full |
| Dependency Cycles | [spec/lang/10-modules.md#dependency-cycles](spec/lang/10-modules.md#dependency-cycles) | none |
| Prelude | [spec/lang/10-modules.md#prelude](spec/lang/10-modules.md#prelude) | full |
| Standard Testing | [spec/lang/10-modules.md#standard-testing](spec/lang/10-modules.md#standard-testing) | full |
| Module Initialization | [spec/lang/10-modules.md#module-initialization](spec/lang/10-modules.md#module-initialization) | brief |
| Public Uses And Visibility | [spec/lang/10-modules.md#public-uses-and-visibility](spec/lang/10-modules.md#public-uses-and-visibility) | full |
| Name Resolution Across Packages | [spec/lang/10-modules.md#name-resolution-across-packages](spec/lang/10-modules.md#name-resolution-across-packages) | brief |
| Executable Entry Point | [spec/lang/10-modules.md#executable-entry-point](spec/lang/10-modules.md#executable-entry-point) | full |
| Wasm Boundary | [spec/lang/10-modules.md#wasm-boundary](spec/lang/10-modules.md#wasm-boundary) | brief |
| Tooling, ABI, And Unsupported Extensions | [spec/lang/10-modules.md#tooling-abi-and-unsupported-extensions](spec/lang/10-modules.md#tooling-abi-and-unsupported-extensions) | none |
| Versions, Version Selection, Integrity, Toolchain | [spec/lang/10-modules.md#versions](spec/lang/10-modules.md#versions) | none |

## spec/lang/11-requirements-and-suspension.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Decomposed Model | [spec/lang/11-requirements-and-suspension.md#decomposed-model](spec/lang/11-requirements-and-suspension.md#decomposed-model) | full |
| Requirement Rows | [spec/lang/11-requirements-and-suspension.md#requirement-rows](spec/lang/11-requirements-and-suspension.md#requirement-rows) | full |
| Provider Access | [spec/lang/11-requirements-and-suspension.md#provider-access](spec/lang/11-requirements-and-suspension.md#provider-access) | full |
| Provider Scopes | [spec/lang/11-requirements-and-suspension.md#provider-scopes](spec/lang/11-requirements-and-suspension.md#provider-scopes) | full |
| Mutable Providers | [spec/lang/11-requirements-and-suspension.md#mutable-providers](spec/lang/11-requirements-and-suspension.md#mutable-providers) | brief |
| Suspending Functions | [spec/lang/11-requirements-and-suspension.md#suspending-functions](spec/lang/11-requirements-and-suspension.md#suspending-functions) | full |
| `Suspend[T]` Protocol | [spec/lang/11-requirements-and-suspension.md#suspendt-protocol](spec/lang/11-requirements-and-suspension.md#suspendt-protocol) | brief |
| Compilation Strategy | [spec/lang/11-requirements-and-suspension.md#compilation-strategy](spec/lang/11-requirements-and-suspension.md#compilation-strategy) | none |
| Construction-Time Requirement Binding | [spec/lang/11-requirements-and-suspension.md#construction-time-requirement-binding](spec/lang/11-requirements-and-suspension.md#construction-time-requirement-binding) | none |
| Cancellation | [spec/lang/11-requirements-and-suspension.md#cancellation](spec/lang/11-requirements-and-suspension.md#cancellation) | brief |
| Requirement Polymorphism | [spec/lang/11-requirements-and-suspension.md#requirement-polymorphism](spec/lang/11-requirements-and-suspension.md#requirement-polymorphism) | brief |
| Runtime Boundary | [spec/lang/11-requirements-and-suspension.md#runtime-boundary](spec/lang/11-requirements-and-suspension.md#runtime-boundary) | none |

## spec/lang/12-variadic-generics.md

The chapter is a stub (no `##` sections yet).

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Tuple rest elements (`List[T]...`) | [spec/lang/07-functions.md#varargs](spec/lang/07-functions.md#varargs) | brief |

## spec/lang/13-gadts.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Variant Result Types | [spec/lang/13-gadts.md#variant-result-types](spec/lang/13-gadts.md#variant-result-types) | brief |
| Shared Constructor Data | [spec/lang/13-gadts.md#shared-constructor-data](spec/lang/13-gadts.md#shared-constructor-data) | brief |
| Construction | [spec/lang/13-gadts.md#construction](spec/lang/13-gadts.md#construction) | brief |
| Pattern Refinement | [spec/lang/13-gadts.md#pattern-refinement](spec/lang/13-gadts.md#pattern-refinement) | brief |
| Payload Pattern Conventions | [spec/lang/13-gadts.md#payload-pattern-conventions](spec/lang/13-gadts.md#payload-pattern-conventions) | none |
| Type-Checking Requirements | [spec/lang/13-gadts.md#type-checking-requirements](spec/lang/13-gadts.md#type-checking-requirements) | none |
| Runtime Representation | [spec/lang/13-gadts.md#runtime-representation](spec/lang/13-gadts.md#runtime-representation) | none |
| Refinement Algorithm | [spec/lang/13-gadts.md#refinement-algorithm](spec/lang/13-gadts.md#refinement-algorithm) | none |

## spec/lang/14-annotations.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Terminology | [spec/lang/14-annotations.md#terminology](spec/lang/14-annotations.md#terminology) | brief |
| Two `annotate` Forms | [spec/lang/14-annotations.md#two-annotate-forms](spec/lang/14-annotations.md#two-annotate-forms) | full |
| Grammar | [spec/lang/14-annotations.md#grammar](spec/lang/14-annotations.md#grammar) | none |
| Typed Derivation | [spec/lang/14-annotations.md#typed-derivation](spec/lang/14-annotations.md#typed-derivation) | full |
| Serialization | [spec/lang/14-annotations.md#serialization](spec/lang/14-annotations.md#serialization) | none |
| Error Derivation | [spec/lang/14-annotations.md#error-derivation](spec/lang/14-annotations.md#error-derivation) | full |

## spec/std/cli.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Argument Forms | [spec/std/cli.md#argument-forms](spec/std/cli.md#argument-forms) | none |

## spec/std/cmp.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Comparison (`Eq`, `Ord`, `clamp`) | [spec/std/cmp.md](spec/std/cmp.md) | brief |

## spec/std/collections.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Lists (`sorted`, `group_by`, `view`, `chunks`) | [spec/std/collections.md](spec/std/collections.md) | brief |
| `Set` | [spec/std/collections.md#set](spec/std/collections.md#set) | none |
| `Deque`, `Heap` | [spec/std/collections.md](spec/std/collections.md) | none |

## spec/std/console.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Console`, `println`, `dbg` | [spec/std/console.md](spec/std/console.md) | full |
| `ConsoleInput`, `read_line` | [spec/std/console.md](spec/std/console.md) | brief |

## spec/std/digest.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| SHA-256 (`sha256`, `sha256_hex`) | [spec/std/digest.md](spec/std/digest.md) | none |

## spec/std/encoding.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Hex, base64, `DecodeError` | [spec/std/encoding.md](spec/std/encoding.md) | none |

## spec/std/error.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Error`, `report_of`, `caused by` chain | [spec/std/error.md](spec/std/error.md) | brief |

## spec/std/format.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Debug`, `debug`, `dbg` | [spec/std/format.md](spec/std/format.md) | full |

## spec/std/fs.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `FsRead`/`FsWrite`, `read_text`/`write_text`, `MemoryFs` | [spec/std/fs.md](spec/std/fs.md) | brief |

## spec/std/hash.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Hash`, `Hasher` | [spec/std/hash.md](spec/std/hash.md) | none |

## spec/std/host.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Args`, `Env` | [spec/std/host.md](spec/std/host.md) | brief |

## spec/std/iter.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Adapters (`map`, `filter`, `fold`, `collect`), `FromIterator` | [spec/std/iter.md](spec/std/iter.md) | brief |

## spec/std/json.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Json`, `Number`, `parse`, compact/pretty text | [spec/std/json.md](spec/std/json.md) | none |
| Typed JSON (`to_json`, `from_json`, `encode`, `decode`) | [spec/std/json.md#typed-json](spec/std/json.md#typed-json) | none |

## spec/std/num.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Integer parsing, `to_fixed`, suffixes | [spec/std/num.md](spec/std/num.md) | brief |

## spec/std/ops.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Operator traits, `Default` | [spec/std/ops.md](spec/std/ops.md) | brief |

## spec/std/option.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Option` helpers (`and_then`, `unwrap_or`, `expect`) | [spec/std/option.md](spec/std/option.md) | brief |

## spec/std/path.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Path`, `join`, `parent`, `extension` | [spec/std/path.md](spec/std/path.md) | brief |

## spec/std/process.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Process`, `run!` | [spec/std/process.md](spec/std/process.md) | brief |

## spec/std/random.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Random`, `SeededRandom`, `Rng` | [spec/std/random.md](spec/std/random.md) | none |

## spec/std/regex.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| Patterns, `find`, `captures`, `replace`, `split` | [spec/std/regex.md](spec/std/regex.md) | none |

## spec/std/result.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Result` helpers | [spec/std/result.md](spec/std/result.md) | brief |

## spec/std/serde.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Serialize`/`Deserialize`, data model | [spec/std/serde.md](spec/std/serde.md) | none |

## spec/std/task.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `all!`, `race!`, `block_on`, `sleep` | [spec/std/task.md](spec/std/task.md) | brief |

## spec/std/testing.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `it`, `assert`, `it_each`, `it_prop`, `Arbitrary` | [spec/std/testing.md](spec/std/testing.md) | full |
| Snapshots, `temp_dir`, `hd_run`, fakes | [spec/std/testing.md](spec/std/testing.md) | none |

## spec/std/text.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `trim`, `split`, raw strings, interpolation | [spec/std/text.md](spec/std/text.md) | full |
| `join`, `StringBuilder`, `repeat`, `chars`/`bytes` | [spec/std/text.md](spec/std/text.md) | brief |

## spec/std/time.md

| Feature | Spec link | Guide mention |
| --- | --- | --- |
| `Duration`, suffixes, `sleep`, `ManualClock` | [spec/std/time.md](spec/std/time.md) | brief |
| `Timestamp`, RFC 3339, `Instant`, dates | [spec/std/time.md](spec/std/time.md) | none |
