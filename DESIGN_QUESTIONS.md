# Design Questions

## High-Level Intention

Decisions:

1. The primary workflow is humans reviewing AI-generated code.
2. The language should feel like a normal general-purpose programming language with strong AI and infrastructure features.

Deferred:

1. Whether to prefer explicit code, concise code, or explicit-by-default with shorthand later.

Remaining questions:

1. Which language features should be visible directly in source syntax, and which should be generated or shown by tooling?
2. Which compiler/tooling reports should a human reviewer see first when reviewing AI-generated code?

## Main Use Scenarios

1. Which features are required for the agent tool-call scenario versus the engineering/infra scenario?
2. Which features should be available in single-file interactive scripts?
3. Which features require project/package-level tooling?
4. Which features must the initial WebAssembly compiler support?

## Language And Tooling Fit

Annotation decisions:

1. `annotate Facet for Target` is structural syntax that generates the same conformance as `impl Annotate[Facet] for Target`, while additionally supporting field or variant overrides.
2. Only one annotation block is allowed for a given facet/target pair in a package.
3. Annotation blocks are global within a package.
4. Libraries do not provide implicit default annotation blocks for downstream applications; if a library explicitly provides an annotation, downstream packages cannot override it.
5. Applications can define package-local annotations for imported targets only when the imported target has no library-provided annotation for that facet.
6. Annotation field/variant assignments can only target existing target members; they cannot create new members.
7. Function parameter assignment overrides are deferred in v1.
8. Function parameter customization uses parameter annotations/docs or a whole-function `build` override.
9. The whole-generation hook is named `build`.
10. Struct annotation `build` receives `Dict[string, FieldTarget]` keyed by field name.
11. Field metadata applicability is checked through the open `FieldMetadata[T]` trait. Different concrete metadata values become a homogeneous `list[FieldMetadata[T]]` through ordinary Go-style dynamic trait values.
12. `annotate Target` attaches member metadata to declaration shapes. For a field of type `T`, each assigned value must implement `FieldMetadata[T]`; variant and parameter metadata use their corresponding metadata traits.
13. Metadata entries are ordinary values. Reusable groups are ordinary homogeneous lists of dynamic metadata trait values, such as `list[FieldMetadata[string]]`.
14. Multiple instances of the same annotation/metadata type after composition are banned.
15. Annotation values are runtime metadata values evaluated in a restricted metadata phase, not compile-time-only constants and not normal application side effects.
16. Metadata evaluation is pure, deterministic, sandboxed, and dependency-free in v1: no `$` context use, no suspending `!` calls, no IO/network/database/time/random work, and no escaping mutation.
17. Metadata evaluation is strictly bottom-up: type metadata before field metadata, field metadata before struct metadata, parameter metadata before function metadata, payload-field metadata before variant metadata, and variant metadata before enum metadata. Parent annotators can inspect child annotation values and child metadata, but cannot retroactively change child attachment results.
18. The compiler type-checks annotations and lowers/desugars them into metadata-phase construction and `attach` calls. Desugared examples in the docs are illustrative only; exact generated helpers and storage are not normative.
19. There is no decorator syntax. Field and variant metadata are declared in `annotate Target` blocks and lowered to metadata attached to the corresponding declaration shapes.
20. Annotations attach to declarations/shapes, not to type expressions. There is no annotated type syntax in the current design.
21. Generic field mapping resolves the requested facet for the field's type. An explicit target annotation such as `annotate Validation for Email` becomes the reusable default wherever `Email` appears.
22. Facet resolution prefers an exact concrete facet/target annotation, then attached struct/enum derivation. Behavior when neither exists remains open.
23. Recursive annotation derivation is detected automatically using active `(annotation, concrete target)` entries. Re-entering an active entry produces a deferred `AnnotationRef[Info]`; completed and non-recursive targets produce ready references.
24. Completed annotation artifacts are memoized per package by `(facet, concrete target)`.
25. The common `Annotation` trait associates an annotation kind with its uniform `Info`; `Annotate[A]` records that a concrete target provides information for annotation `A`. Neither trait has a recursion-specific operation.
26. `AnnotationRef[T]` is the universal ready/deferred representation. Facet targets such as `Validator` do not need facet-specific recursion cases such as `Validator.Ref`.
27. Local metadata protocols correspond directly to member shapes: `FieldMetadata[T]`, `VariantMetadata`, and `ParamMetadata[T]`. Aggregate derivation remains in `StructAnnotator`, `EnumAnnotator`, and `FuncAnnotator`.
28. `annotate Validation for User: pass` requests default derivation through `StructAnnotator` and generates `impl Annotate[Validation] for User`; `pass` means that no structural result overrides are supplied.
29. Primitive and nominal targets use exact blocks such as `annotate Validation for i32` and `annotate Validation for string`; there is no `TypeDeriver` protocol or wildcard `annotate Facet for type` fallback.
30. Annotation facets are open across exact target types. New concrete cases may be added downstream unless that exact facet/target pair is already provided authoritatively by a library.
31. Manual `lazy` remains an optional field-metadata value candidate inside `annotate Target`, but automatic recursion detection is the preferred default. It must not change field storage, access, or type semantics.
32. Annotation values compose strictly bottom-up: exact type annotations produce type metadata; field metadata combines type metadata with field annotations; variant metadata combines payload-field metadata with variant annotations; and struct/enum metadata combines child metadata with annotations on the enclosing declaration. The compiler passes already-derived type metadata into `map_field` explicitly.
33. Annotator applicability and declaration visibility are separate concepts. `FuncAnnotator` does not require a target function to be `pub`, and `annotate Tool for function` does not change that function's language-level visibility.
34. Annotation semantics reduce to ordinary traits, implementations, values, dynamic trait values, and declaration shapes. `StructAnnotator` and `EnumAnnotator` remain the aggregate derivation protocols. `annotate` is the only annotation-oriented surface syntax.
35. `annotate Facet for Target` and a direct `impl Annotate[Facet] for Target` occupy the same coherence slot and cannot coexist. The direct impl constructs the complete information value; `annotate` derives unmentioned members and applies structural overrides.

Remaining questions:

1. What common runtime shape representation should the compiler expose for structs, enums, functions, fields, variants, and parameters?
2. How should generic derivation consume that representation for facets such as validation, JSON, UI, tools, database schema, retention, observability, and workflows?
3. What override paths besides one package-global `annotate Facet for Target` block should exist later, such as reusable override profiles or full derivation rewrites?
4. How should derived metadata/artifacts be explicitly exported for runtime use?
5. If an annotation facet needs stricter field-type-specific correctness for its override values, should that be enforced through smart constructors, generated diagnostics, or a later advanced type feature?
6. What syntax should materialize annotation-derived runtime values, currently sketched as `DatabaseSchema::annotation(User)`?
7. What is the final syntax for generic constraints on `impl`, including generic implementations of `FieldMetadata[T]` and `Annotate[A]`?
8. What syntax should define reusable generic annotation targets such as every `list[T]`? The current validation example deliberately uses the exact target `list[Entry]`.
9. Should type information and completed struct/enum information share `Annotation::Info`, or should the protocol expose separate associated types for these stages?
10. Should a statically visible `MissingAnnotationPolicy` select compile-time rejection or omission when a child type has no annotation for the requested facet? Its default, granularity, and exact syntax remain undecided; runtime `map_field` cannot make compilation fail.
11. Should arbitrary traits be allowed to opt into structural awareness of their implementation targets, beyond the compiler lowering already provided for `annotate`? This remains exploratory and is not needed by the annotation model.
12. If manual `lazy` metadata is retained, which aggregate annotators consume it and how is that scope expressed?
13. Should a future version add `@expr` as locality sugar for member metadata? If added, it must lower exactly to the corresponding `annotate Target` entry and introduce no new semantics.
14. For each effect, what should the compiler/tooling generate: handler requirements, mock handlers, dependency graphs, audit reports, or observability metadata?
15. For each function, what should tooling show first: signature, effects, contracts, examples, tests, dependencies, or observability behavior?
16. For each tool function, what should be generated: JSON Schema, OpenAPI, MCP definitions, TypeScript types, docs, examples, or runtime registration? Tool discovery and external exposure must be designed separately from declaration visibility and `FuncAnnotator` applicability.
17. For each property test failure, what structured output should be produced for AI repair?
18. For each registered resource or workflow, what should be derived from code versus explicitly annotated?

## Agent Tooling Scenario

1. Which annotations should be compile-time only versus runtime-visible?
2. Should tool descriptions be plain strings, structured documentation, or schema-like metadata?
3. Should tool examples be compiled, typechecked, and run as tests?
4. Should tool access control be expressed through contracts, effects, schema annotations, or a dedicated `access` section?
5. Should context, auth, and tenant data be ordinary parameters, implicit effects, or a special context type?
6. Should references between RPC tools be explicit declarations, inferred from calls, or generated into a graph?
7. Should imported data types be versioned for tool compatibility?
8. What external specs should the compiler generate first for existing tooling: JSON Schema, OpenAPI, MCP tool definitions, TypeScript types, or something else?

## Engineering And Infrastructure Scenario

1. What should `Result[T, E]` ergonomics look like for compiler-verifiable error handling beyond `Ok(value)` / `Err(error)` construction: matching, diagnostics, and interaction with `?` propagation?
2. Which infrastructure concepts should be first-class: databases, queues, cron jobs, object storage, secrets, deployments, regions, feature flags, or workflows?
3. Should observability be automatic for every function, or explicit through effects and annotations?
4. Should logs, traces, and metrics be queryable from language tooling?
5. Should durable workflow steps be ordinary functions with effects, or a separate workflow construct?
6. What resource definitions can be derived from code inspection?
7. How should derived resource definitions be synced to cloud infrastructure?
8. What observability data can AI agents inspect without raw production data access?
9. How should declarative data-retention and cascade requirements be expressed through the general annotation model?

## AI-Centered Workflow

1. What should an AI agent be able to infer from source code without reading implementation details?
2. Should the language make generated code intentionally repetitive and explicit, or compact and abstract?
3. Should the compiler produce feedback designed for AI repair loops?
4. Should failed type checks, contract checks, schema checks, and property tests produce structured machine-readable diagnostics?
5. Should the language include conventions for AI-generated mocks, fixtures, and dependency handlers?

## Correctness Model

1. What is the main correctness story: static types, effects, schemas, contracts, property tests, generated mocks, or all of them together?
2. Which mistakes should be caught before running code?
3. Which mistakes should be caught through generated tests?
4. Which mistakes should be allowed to surface at runtime as `Result` errors or contract failures?
5. Should the language prefer refusing ambiguous code, or accepting it with explicit runtime checks?

## Type System

Decisions:

1. Type aliases are transparent by default.
2. Nominal single-field newtypes are needed; candidate spelling is `type Mile(i32)`.
3. Lower-precision integers can widen into higher-precision integer types implicitly.
4. Higher-to-lower precision numeric conversion requires an explicit cast.
5. hd-lang has no inheritance-based subtype hierarchy.
6. Generic bounds use static dispatch, while plain trait-name value types use Go-style dynamic dispatch.
7. Constructor-style syntax is used for newtype construction and casts, such as `Mile(10)` and `i32(miles)`.
8. Dynamic dispatch uses plain trait names, such as `value: Display`; there is no `dyn` marker. `Any` is the built-in universal empty trait, analogous to Go's `any`, and every non-optional value type satisfies it automatically.
9. Unambiguous promoted methods from embedded structs can satisfy trait requirements for the outer struct.
10. Constructor-style casting is enough for newtype unwrapping; no `.value` or pattern matching access is needed for now.
11. Generic type declarations use explicit `+T` covariance, `-T` contravariance, and unmarked invariant `T`. The compiler verifies the declared polarity against the read-only surface. Variance conversions apply only to read-only outer views; mutable generic views are invariant. The built-in list element parameter is covariant.
12. If multiple embedded structs promote conflicting methods, the outer type does not satisfy the trait automatically.
13. When promoted methods conflict during trait checking, diagnostics show the missing trait requirement, list the ambiguous promoted methods, and suggest an explicit impl or qualified embedded-method call.
13. Integer literals always default to `i32` when there is no expected type.
14. Integer literals are range checked when there is an expected numeric type.
15. Varargs must be the final positional parameter.
16. Spread syntax, such as `items...`, is positional only.
17. A named vararg accepts a list, such as `values=items`.
18. Variadic generics are minimal v1: ordered type and value packs support compile-time pattern expansion in function types, vararg parameters, tuple types, call arguments, and type or expression patterns. A repeated pattern such as `Suspend[Ts]...` is instantiated once per pack element, `(Ts...)` forms a heterogeneous tuple type, and multiple packs in one pattern expand in lockstep with equal lengths. No general pack mapping, filtering, indexing, splitting, or arithmetic.
19. Tuple type and literal spelling uses `(T, U)` and `(value, other)`.
20. One-element tuples require a trailing comma, such as `(T,)` and `(value,)`.
21. Empty tuple exists as `()`.
22. Tuple fields use Rust-style numeric access, such as `point.0`.
23. Tuple destructuring supports both `x, y := point` and `let x, y = point`.
24. Named tuples are omitted in v1; use structs when field names matter.
25. Primitive numeric types use explicit widths; v1 has no `int`, `uint`, or `float` aliases.
26. `decimal` is a standard-library type, not a primitive.
27. `void` remains the no-useful-value return type spelling.
28. Integer literal range diagnostics include the invalid literal, target type range, and a suggested wider type.
29. Numeric narrowing diagnostics suggest explicit constructor-style casts, such as `i16(value)`.

Variance backlog:

- Decide whether permission weakening and variance may compose implicitly. Example: given `struct Cell[+T]` and `Cat <: Animal`, should `mut Cell[Cat]` coerce directly to the read-only view `Cell[Animal]`? The proposed safety argument is that the mutable alias remains restricted to storing `Cat`, while the widened alias cannot mutate the cell. Do not treat this conversion as accepted yet.
- Apply the same decision consistently to built-in and user-defined generic types, including the analogous `mut list[Cat]` to `list[Animal]` conversion.

30. Comprehensions use clause-first syntax with `=>`, such as `[for user in users if (label := user.name.trim().lower()) != "" => label]`.
31. Multiple `for` clauses are allowed in comprehensions and run left to right.
32. Comprehensions do not have a separate `let` clause; local names use `:=` binding expressions.
33. In comprehensions, later clauses can use earlier names, `if` filters at the point where it appears, and later clauses are not visible to earlier clauses.
34. If a map comprehension produces the same key more than once, the later value wins.
35. Comprehensions cannot contain suspension points.
36. `break value` is only valid in value-producing loops with `else`; statement-only loops use plain `break`.
37. `match` arms use `pattern => expression`, and `_ => expression` is the fallback arm spelling.
38. `else if` is one direct conditional-chain syntax form, not a nested `else` block containing a separate `if`.
39. Composite struct fields can carry mutable reference permission with `field: mut T`; ordinary `field: T` is a const edge.
40. Mutation requires a mutable root and `mut` on every composite reference edge crossed by the access path. Local roots use `let value: mut T`, parameters use `value: mut T`, and receivers use `mut self` as shorthand for `self: mut Self`.
Settled mutability details: `mut T` is a type modifier expressing reference permission, not ownership or exclusivity. `T` cannot be upgraded to `mut T`; `mut T` may be viewed as `T`. Permission composes through locals, fields, `list`/`map` arguments, function types, and returns. Primitive parameters pass by value. Local declarations use `let value: mut T`; callable parameters use `value: mut T`; receivers retain `mut self` as shorthand.

Generic mutability decision: an ordinary generic `T` denotes the complete type and may be instantiated with `User` or `mut User`. Consequently, an unconstrained generic declaration cannot contain `mut T`; it stores `T` directly and receives mutable permission through an instantiation such as `Box[mut User]`. This prevents `mut mut User` from arising through ordinary substitution.

Generic mutability constraint decision: `T: mut Any` requires `T` to be a mutable root type, while `T: mut Trait` additionally requires the underlying type to implement `Trait`. This lets generic code prove that `mut self` methods are callable. `T` remains the complete access-qualified type inferred from the argument. Const-root viewpoint adaptation remains necessary so a const container root cannot leak mutable access through an element whose stored type is `mut T`.

Universal trait decision: `Any` is the universal empty trait and is implemented automatically by every non-optional value type. `Any` is non-null; `Any?` admits `nil` through the ordinary optional-type rule. An optional `T?` can erase to `Any?`, not to `Any`. `Any` and other trait names may be used as Go-style dynamic trait values, and `mut Any` or `mut Trait` preserves mutable access to an erased composite value.

Alternative retained for comparison: shallow const local bindings could be re-aliased through mutable bindings or containers, parameters used implicit C++-style const/non-const references with `mut` before the parameter name, and fields had no `mut T` qualifier. This was simpler but did not represent nested mutation authority uniformly.

41. Struct copy-update uses spread syntax in a typed literal, such as `User { ...user, display_name: "Ada" }`.
42. Enum payload variant declarations use compact `Variant(field: type)` syntax in v1. Large payloads should use a separate struct.
43. Enum variant patterns in `match` arms must be qualified with the enum name, such as `JobStatus.Queued`, even when the matched value's type is known.
44. Enum payload patterns use call-style parentheses, such as `Expr.IntLit(value) => value`, not brace destructuring.
45. Enums support generic algebraic data types, recursive enum definitions, and GADT-style variants with explicit result types, such as `IntLit(value: i64) -> Expr[i64]`.
46. Pattern matching on a GADT-style variant refines the enum type parameter inside that arm, so `Expr.IntLit` can refine `Expr[T]` to `Expr[i64]` for arm-local type checking.
47. A GADT-style variant can refine the enum type while passing data to an enum-level constructor, such as `IntBox(n: i64) -> Box[i64](n)`.
48. `Result` values are constructed with capitalized helper constructors: `Ok(value)` and `Err(error)`.
49. Enums can declare constructor parameters for data shared by every variant, including unnamed parameters such as `enum StatusCode(i32):`.
50. Variants can call the enum constructor in their result expression, such as `NotFound -> StatusCode(404)`.
51. Enum constructor definitions and calls follow the same parameter conventions as functions: unnamed positional parameters/arguments first, then named parameters/arguments.
52. Enum payload patterns follow the same positional/named convention as calls: positional patterns first, then named patterns. Only `field=pattern` counts as a named pattern.
53. In enum payload patterns, bare identifiers bind new names, and positional binding names do not need to match payload field names, such as `Expr.Add(l, r)` for fields named `left` and `right`.
54. In enum payload patterns, literals match exact values, such as `Expr.Scale(value, factor=2)`.
55. Function parameter defaults may use pure expressions and pure function calls, but cannot require effects, dependencies, or suspension.
56. Generic arguments are inferred at call sites when unambiguous, and callers can provide the full generic argument list explicitly. v1 has no partial explicit generic arguments or placeholder generic arguments.
57. There is no separate short closure syntax in v1; same-line `fn(...) -> ...:` closure bodies are allowed for single-expression closures, such as `fn(x: i32) -> i32: x + 1`.
58. Shorthand argument closures using `$0`, `$1`, etc. are deferred; v1 requires named parameters in the closure parameter list.
59. Closures that mutate captured locals have mutable function type `mut fn(...) -> ...`, and calling one requires the closure value to be mutable.
60. Plain `fn(...) -> T` closures cannot mutate captured locals.
61. Closures that capture dependencies or capabilities carry those requirements in their function type.
62. Serializable-closure semantics are deferred. Capture eligibility, snapshots versus identity preservation, mutable state, live handles, capabilities, code identity, compatibility, and execution guarantees must be decided before selecting syntax or an API.
63. Context access uses `$` operations with requirement keys as arguments. `$.use(Database, Cache)` retrieves providers by requirement key in argument order.
64. Provider scopes use named requirement bindings, such as `$.with(Database=mock_db, Cache=memory_cache):`.
65. Reusable contexts use row-indexed provider-map types, such as `$.Context[Metrics + Cache]`, with builders like `$.context(Metrics=metrics, Cache=cache)` and lexical reuse through spread syntax, such as `$.with(Database=db, ...prod_context()):`.
66. Context rows are unordered and unique by requirement key. Duplicate providers cannot coexist; construction/spread collapses duplicates by lexical order, with later bindings winning, and missing providers are compile-time errors.
67. Trailing-block call syntax is restricted to a final zero-argument function parameter. Ordinary arguments remain inside parentheses, as in `when(a, b):`; calls without ordinary arguments omit empty `()`, as in `transaction:`. The block omits `fn()` and an explicit return type, which are inferred from the final parameter. Parameterized callbacks use explicit closure syntax, and multiple trailing blocks are not supported.
68. `return` inside a trailing block exits the generated callback, not the enclosing function. Trailing blocks do not support non-local return.
69. Inline closures use contextual typing: parameter and return types may be omitted when the expected function type determines them. Standalone or ambiguous closures require explicit signatures. Multiple multiline callbacks can be passed inline by parenthesizing each closure expression; the comma after each closing parenthesis separates call arguments.
70. Iteration uses separate `Iterable` and `Iterator` roles. An ordinary iterable source carries no per-traversal progress and creates an independent iterator on every `iter()` call. Each iterator owns mutable cursor/progress state for one traversal and advances through `next(mut self)`. Every iterator is itself iterable: its `iter()` returns the same mutable cursor without cloning or resetting it, so traversal resumes at its current position. "Traversal-state-free" applies to ordinary iterable sources, not to this self-iterable iterator implementation. Exact associated-type syntax and source mutation during traversal remain open.

## Modules, Packages, And Imports

Decisions:

1. Modules are path-inferred from files under the package source root.
2. There is no required `module` or `package` declaration in source files.
3. Directories define submodule namespaces only when they contain a `mod.hd` file.
4. `mod.hd` is required for every directory module and acts as the public index file.
5. Visibility is explicit with `pub`; declarations without `pub` are module-private.
6. Current-package imports use the `pkg` root, such as `import pkg.user.types.{User, UserId}`.
7. Module namespace imports use the same roots, such as `import pkg.user.types`.
8. Re-exports use `export`, mainly from `mod.hd`.
9. Relative imports use `self` and `super`, such as `import self.types.{UserId}` and `import super.shared.{Email}`.
10. Packages use `hd.toml`.
11. The default source root is `src`.
12. Standard-library imports use the `std` root.
13. External dependency imports use `dep.<name>`, where `<name>` is declared in `hd.toml`.
14. Import and re-export cycles are rejected in v1.
15. v1 has only module-private declarations and `pub`; there is no package-private visibility modifier.
16. `pub fn main` is the conventional default entry point for an executable package. It has no source-level parameters; arguments, environment, I/O, and other host services arrive through `$` context requirements.
17. Entry points follow ordinary suspension naming, so the spelling is `main!` only when the function can suspend. The default entry point may return `void` or `Result[void, E]`.
18. `pub` does not imply a Wasm export. Other public functions become host-visible only through explicit tool, workflow, or library registration that generates a boundary adapter.
19. Registered Wasm boundaries initially accept only recursively structural values: primitive scalars, `string`, tuples, `list[T]`, `map[K, V]`, structs, enums, `T?`, and `Result[T, E]`. Mutable types, trait values, closures, and live runtime handles cannot occur anywhere in an exported parameter or result. Context requirements are host bindings, not serialized parameters. Maps are unordered by default; insertion and iteration order are not part of their value or boundary semantics.

## Testing And Data Generation

Deferred. Property testing must be designed as a `std.testing` library surface rather than dedicated language syntax. The questions below are retained for that later library-design pass.

1. Should every schema automatically define a valid data generator?
2. Should generated data be deterministic by default?
3. Should generated data be explainable, with a trace of which schema rule produced each value?
4. Should property tests be expected for all public functions, or only where explicitly written?

## Effects And Dependencies

Settled direction: hd-lang pragmatically decomposes concerns commonly handled by algebraic effects into one-shot suspension, static requirement checking, and dependency injection. The mechanisms are related and intentionally not fully orthogonal. `fn!` and `Suspend[T]` provide one-shot suspension rather than general or multi-shot continuations; `$` rows verify dependency availability rather than primarily modeling host authority; contexts bind requirements to concrete providers. At explicit dependency and suspension boundaries, production, mock, sandbox, and replay providers or drivers can execute the same checked source differently. Ordinary code outside those boundaries is not reinterpreted.

1. Should user-defined effects be lightweight enough to use for small mocks?
2. How should package/app boundaries declare default providers for production and tests?
3. Which parts of this decomposed model are part of public API compatibility?
4. Should an AI agent be able to replace real dependencies with mock providers automatically?
5. Should requirement polymorphism use explicit row variables, parameter-linked requirements, or inferred propagation?
6. Should handled requirements be removed explicitly from row variables, such as `e - Logger`, or inferred from the provider scope?
7. Is full row polymorphism needed, or is a smaller requirement-variable model with union and removal enough?
8. What are the exact provider shadowing and lookup rules for `$.with(...)`, context spread, and `$.use(...)` context syntax?
9. Should all functions whose own body contains suspension points require both bang-suffixed declarations and bang-call syntax?
10. Should `$.use(...)` provider lookup be cached per lexical scope, per function invocation, or resolved at every use site?

Settled concurrency foundation: `fn name!(...) -> T` declares a cold suspended computation and lowers to a function constructing `Suspend[T]`. Calling `name(...)` evaluates its arguments and returns the cold value without entering the body. Calling `name!(...)` constructs and immediately drives it, and every nested bang call is a suspension point in the enclosing compiler-generated state machine. This resembles Kotlin's suspension lowering, but unlike Kotlin it exposes the cold suspended computation as an ordinary value.

Settled suspension protocol: `Suspend[T]` is a trait implemented directly by cold, single-execution state machines. It provides `poll(mut self, context: PollContext) -> Poll[T]` and synchronous `cancel(mut self) -> void`. `Poll[T]` reports `Pending` or `Ready(T)` for one poll; the suspension retains execution state. The polling context contains a waker. There is no separate public `Continuation[T]`. Driving is exclusive: competing drivers or reentrant polling cause a runtime panic, not a type error. Normal successive polls by the same driver are allowed while pending. Starting another execution or polling after completion or cancellation also causes a runtime panic; suspensions are neither reusable plans nor memoized results. The exact mechanism granting guarded mutable driver access remains open.

Dependencies are bound at construction. Constructing a suspension requires the declared `$` dependencies in the caller's context, either through its signature or a local provider scope, and captures the selected providers without entering the body. Driving it under another context does not rebind those providers. Durable recovery still reconstructs execution with rebound host providers; it does not serialize live provider handles.

Cancellation is synchronous: the proposed `cancel(mut self) -> void` protocol serializes with polling, terminates execution, cancels owned active child operations, and unregisters waits. Repeated cancellation is harmless and late wakes cannot restart execution. Cleanup cannot suspend. Source-level cleanup registration and resource lifetime guarantees remain in [Deferred Resource Cleanup And Scope Exit](#deferred-resource-cleanup-and-scope-exit); the cancellation protocol does not settle that backlog.

A separate `Task[T]` wrapper, including its public API, result caching, and relationship to drivers and scopes, is backlog work, not an accepted abstraction.

Compilation uses managed stackless state machines with saved live locals, nested child polling, and synchronous cancellation paths. See the [non-normative `fn!` lowering example](SYNTAX_NOTES.md#compiling-a-suspending-function) for construction-time provider capture and an illustrative generated `Suspend[i32]` implementation.

Settled `all!` result behavior: a child returning `Err` completes normally with a `Result` value. It does not stop `all!` or cancel siblings. `all!` waits for every child to complete and returns their values, including any `Err` values.

Settled `race!` result behavior: first completion wins, including `Err`. Remaining children are synchronously cancelled before `race!` returns the winning value. It does not wait for the first `Ok`.

Settled combinator cancellation: cancelling `all!` or `race!` synchronously cancels every unfinished child, preventing further execution. Providers must actively abort underlying external operations, including HTTP requests, rather than merely discard their results. Cancellation invokes the host/provider abort mechanism synchronously without waiting for remote acknowledgement; it cannot undo effects already performed remotely.

Concurrency questions to decide next:

1. How is a stored `Suspend[T]` driven directly, rather than through its original function call or a combinator?
2. What exactly starts child computations in `all!` and `race!`, and are starts ordered even when execution is concurrent?
3. How are structured execution scopes represented and enforced?
4. How do runtime defects and cancellation combine across multiple children? `Err` is an ordinary result and does not short-circuit `all!`.
5. Which cancellation propagation rules apply when a parent exits or a child panics? `race!` cancels remaining children when a child completes; tie-breaking between ready children remains open.

## Sandbox, Capabilities, And Resumption

Settled: capabilities are ordinary dependencies. They use normal traits, `$` requirement rows, and context providers; there is no separate capability language construct.

Settled: durable workflows initially use deterministic replay over an append-only event history. Suspending `!` calls are replay boundaries; recorded completions are reused, new commands suspend execution, and no `checkpoint` keyword or serialized machine stack is required in v1. Runs pin compatible code identity, providers are rebound on resumption, and external commands use idempotency keys.

Settled: interactive execution uses a live kernel for ordinary continuity plus a deterministic journal of atomically committed cell runs for recovery. Recovery restores a serializable snapshot and replays later runs in actual execution order. Rerunning an earlier or edited cell creates a new branch and makes prior descendants stale.

Settled: observability is automatic at semantic runtime boundaries, including entry points, registered tool/RPC calls, workflows, cells, suspending `!` operations, and runtime provider boundaries. Ordinary functions are not traced automatically. `Observability` is an explicit dependency of generated boundary adapters and of user functions that emit telemetry. Execution-local context propagates current spans and fields; generated scopes close spans from complete exits; logs automatically become current-span events; unhandled errors and runtime lifecycle failures produce automatic events.

Provisional v1 draft: `Observability` exposes `sample(candidate) -> bool` and non-suspending `emit(event) -> void`. The runtime owns span/context lifecycle and replay suppression; providers consume normalized span, log, metric, and runtime events. Console, recording, fan-out, and buffered OpenTelemetry implementations are expected. This interface may be optimized later.

1. Which capability traits should exist initially: filesystem, network, database, clock, randomness, subprocess, secrets, auth context, logging, tracing, metrics, or cloud resources?
2. What configuration syntax should bind concrete, scoped host providers to the requirement keys derived from an entry point?
3. Which top-level values are serializable into interactive namespace snapshots, and how are live-only values diagnosed and reconstructed?
4. How should resumed execution prove that its capabilities and providers are compatible with the original run?
5. What source edits are replay-compatible, and how should incompatible suspension-site or control-flow changes be diagnosed and migrated?
6. How should users inspect, switch, retain, or discard interactive history branches?

## Serializable Closures And Incremental Computation

Serializable closures are backlog work and should not be added to the language tour or MVP yet. Their semantics must be designed before considering syntax, annotations, inference, or wrapper APIs. The phrase "code reference plus captured environment" describes the goal but is not a sufficient semantic model.

Serializable-closure questions, in semantic order:

1. Does serialization snapshot captured values, preserve shared identity and aliasing, or reject captures for which those choices are observable?
2. Are mutable captures rejected, frozen at serialization time, copied into independent mutable state, or restored with shared identity?
3. How are nested object graphs, repeated references, cycles, optional values, trait values, and closures that capture other closures represented?
4. What identifies executable code, and what changes are compatible with an already stored closure across builds, deployments, and WebAssembly runtime versions?
5. Are erased generic instantiations sufficient, or must concrete reified type arguments become part of the stored closure identity?
6. Which requirements are captured and which are rebound at execution? In particular, how do capabilities, authorization context, provider configuration, and live resources behave?
7. Which invalid captures are compile-time errors, and which can only fail when a dynamic value is serialized?
8. What execution guarantees apply after movement or persistence: at-most-once, at-least-once, idempotent replay, cancellation, expiry, and result compatibility?
9. What security boundary validates stored closure data and code identity before execution in a sandbox?
10. How are schema evolution and migration handled for captured values and returned results?

Only after these questions have answers should the design choose among inference, a distinct function type, an annotation, or a library wrapper.

Current incremental-computation findings:

- Incremental queries, cross-run caching, and durable replay solve different problems and must have separate identities and correctness rules.
- The current direction is a native-feeling `std.incremental` API with runtime dependency tracking, not dedicated language syntax.
- Inputs and computation regions are explicit, while dependencies are recorded dynamically from the tracked values actually read.
- Pure incremental callbacks use existing function types: plain `fn`, not `mut fn` or `fn!`, with no `$` requirements, mutable parameters, or mutable captures.
- External reads need explicit stable version tokens. Neither suspension nor provider injection makes an external operation cacheable.
- v1 should investigate transactional updates, lazy recomputation, topological stabilization, equality cutoff, cycle diagnostics, and first-class graph inspection.

Incremental computation remains a large deferred surface. The findings above establish boundaries, not a complete API or runtime design.

1. What is the exact `std.incremental` API for inputs, computations, observation, updates, and stabilization?
2. Which stable identity and version-token protocols should tracked external inputs implement?
3. Which parts of code identity, arguments, captured values, type arguments, provider identity, target ABI, and dependency versions belong in persistent cache keys?
4. Should persistent and distributed caching be part of the initial library or a later storage implementation?
5. What is the initial tracking granularity for structs, lists, and maps?
6. How long do unobserved computation nodes and cached values remain alive?
7. How are accidental cycles reported, and should explicit fixed-point computation be deferred to a separate API?
8. How should cache invalidation and recomputation be explained to AI agents and human reviewers?

## Deferred Resource Cleanup And Scope Exit

Status: backlog only. Deterministic cleanup is not part of the MVP, and no `Drop`, `using`, `with`, `defer`, or `errdefer` syntax or semantics have been accepted.

The comparison must preserve the honest asymmetry:

- Protocol-owned RAII/`Drop` or `using` is more enforceable. Cleanup belongs to a type, can be discovered through constraints and tooling, and is difficult to forget when the language enforces the protocol.
- Statement-form `defer` is more directly expressive. It can close over arbitrary local state and register one-off scope-exit behavior without first wrapping that behavior in a resource or context-manager value.
- A protocol can emulate `defer` with a closure-backed scope guard, `ExitStack.callback`, or `DisposableStack.defer`, but that is effectively defer-as-a-library and carries additional ceremony.
- A `defer` statement can invoke a protocol method trivially, but it cannot ensure that callers remember to register cleanup.

Pragmatic `defer` cases that a resource/context protocol handles only through an adapter or additional state include:

1. Cleanup that observes local mutable state whose final value is not known at registration time, such as latency metrics containing the final retry count and cache-hit status.
2. Restoring a mutation to ambient state, such as a configuration flag, working directory, context stack, or temporary patch. The cleanup represents a diff rather than a standalone resource value.
3. Conditional or data-dependent registration after branching, retries, type inspection, or runtime-sized acquisition. Protocol ecosystems generally need an exit/disposable stack for this case.
4. Commit-or-rollback behavior selected from the final outcome, especially when commit itself may fail and must alter the returned error.
5. Staged construction that acquires several resources, releases earlier acquisitions on failure, but transfers all resources out on success. Block-bound context protocols need an explicit ownership-transfer or `pop_all` mechanism.
6. Multiple independent cleanups interleaved with ordinary logic rather than acquired together at one scope entry.

Most sound `defer` uses still represent a setup/teardown pair, but the setup may be a mutation or control-flow fact rather than construction of a resource object. Truly unpaired uses such as panic recovery or hidden return-value rewriting should not be treated as motivating hd-lang behavior without separate justification.

Candidate direction for later evaluation: make protocol-owned cleanup the foundation for actual resources, then add Zig-style lexical `defer` and possibly `errdefer` only as an ad-hoc escape hatch. If `defer` is adopted, prefer block scope in the style of Zig, Swift, or MoonBit rather than Go's function scope.

Open questions for the backlog:

1. Whether hd-lang's shared-reference model can provide useful RAII guarantees without ownership or affine types.
2. Whether cleanup protocols merely guarantee invocation or also prevent escape and use after disposal.
3. How `Result`, `?`, defects, cancellation, and multiple cleanup failures determine the final outcome.
4. Whether cleanup may suspend, how that suspension remains visible through `!`, and whether cancellation is masked during cleanup.
5. Whether a live process resource may cross a durable replay boundary or must be reacquired.
6. Whether `errdefer` should react to `Err` returned through `?`, runtime defects, cancellation, or some explicit subset.
7. How to prevent or diagnose resource escape and use after disposal. The current `T`/`mut T` distinction tracks mutation permission, not ownership, lifetime, or resource state, so an alias can be stored globally, returned, captured by a closure, or inserted into another composite before lexical cleanup runs. Candidate directions include resource-only affine ownership and explicit transfer, scoped region types, typestate combined with alias control, or a shared runtime handle that turns later operations into `Result` failures. No direction is selected.

## Adoption And Runtime

Settled: WebAssembly is the sole compilation target. hd-lang uses Wasm GC for managed language values and WASI for its host interface. All authority-bearing capability providers are injected by the host. Every host-backed standard-library service is exposed as a trait and obtained through the ordinary context system; pure standard-library operations remain context-free functions. The official hd runtime implements every standard capability, while each invocation receives only its granted providers.

1. Which WASI version and component ABI should the initial runtime use?
2. Which standard capability traits ship in v1?
3. Should the standard library be small and test-focused, or broad enough for real application development?
4. What should the smallest useful prototype demonstrate?

Capability granularity is deferred until the standard library is implemented. Decisions such as one broad `FileSystem` trait versus narrower `FileRead`, `FileWrite`, and `DirectoryList` traits should be made from concrete APIs and authority boundaries, not fixed independently in the language design.

## Later Prototype Scoping

These questions are useful later, but are less relevant while the high-level language model is still being designed:

1. What is the smallest useful agent-tool prototype?
2. What is the smallest useful engineering/infra prototype?
