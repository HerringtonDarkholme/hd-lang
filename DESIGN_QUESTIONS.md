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
4. Which features must compile to JavaScript first?
5. Which features must compile to WebAssembly first?

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
8. Dynamic dispatch uses plain trait names, such as `value: Display`; there is no `dyn` or `any` marker.
9. Unambiguous promoted methods from embedded structs can satisfy trait requirements for the outer struct.
10. Constructor-style casting is enough for newtype unwrapping; no `.value` or pattern matching access is needed for now.
11. Variance is skipped for v1; generic types are invariant unless a concrete need appears later.
12. If multiple embedded structs promote conflicting methods, the outer type does not satisfy the trait automatically.
13. When promoted methods conflict during trait checking, diagnostics show the missing trait requirement, list the ambiguous promoted methods, and suggest an explicit impl or qualified embedded-method call.
13. Integer literals always default to `i32` when there is no expected type.
14. Integer literals are range checked when there is an expected numeric type.
15. Varargs must be the final positional parameter.
16. Spread syntax, such as `items...`, is positional only.
17. A named vararg accepts a list, such as `values=items`.
18. Variadic generics are minimal v1: type packs can appear in function types, vararg parameters, and spread calls only. No pack mapping, filtering, splitting, or arithmetic.
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
30. Comprehensions use clause-first syntax with `=>`, such as `[for user in users if (label := user.name.trim().lower()) != "" => label]`.
31. Multiple `for` clauses are allowed in comprehensions and run left to right.
32. Comprehensions do not have a separate `let` clause; local names use `:=` binding expressions.
33. In comprehensions, later clauses can use earlier names, `if` filters at the point where it appears, and later clauses are not visible to earlier clauses.
34. If a map comprehension produces the same key more than once, the later value wins.
35. Comprehensions cannot contain suspension points.
36. `break value` is only valid in value-producing loops with `else`; statement-only loops use plain `break`.
37. `match` arms use `pattern => expression`, and `_ => expression` is the fallback arm spelling.
38. `else if` is one direct conditional-chain syntax form, not a nested `else` block containing a separate `if`.
39. Struct fields do not have field-level mutability markers.
40. Struct mutation is controlled by `let mut` bindings, `mut` function parameters, and `mut self` receivers.
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
62. Serializable closures can only capture serializable values, and cannot capture live handles or capabilities unless a runtime feature explicitly supports that capture.
63. Context access uses `$` operations with requirement keys as arguments. `$.use(Database, Cache)` retrieves providers by requirement key in argument order.
64. Provider scopes use named requirement bindings, such as `$.with(Database=mock_db, Cache=memory_cache):`.
65. Reusable contexts use row-indexed provider-map types, such as `$.Context[Metrics + Cache]`, with builders like `$.context(Metrics=metrics, Cache=cache)` and lexical reuse through spread syntax, such as `$.with(Database=db, ...prod_context()):`.
66. Context rows are unordered and unique by requirement key. Duplicate providers cannot coexist; construction/spread collapses duplicates by lexical order, with later bindings winning, and missing providers are compile-time errors.

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

## Testing And Data Generation

1. Should every schema automatically define a valid data generator?
2. Should generated data be deterministic by default?
3. Should generated data be explainable, with a trace of which schema rule produced each value?
4. Should property tests be expected for all public functions, or only where explicitly written?

## Effects And Dependencies

1. Should effects mainly describe external dependencies, capabilities, suspension points, contracts, or all side-effectful behavior excluding normal errors?
2. Should user-defined effects be lightweight enough to use for small mocks?
3. How should package/app boundaries declare default providers for production and tests?
4. Should effects be part of public API compatibility?
5. Should an AI agent be able to replace real dependencies with mock handlers automatically?
6. Should effect polymorphism use explicit row variables, parameter-linked effects, or inferred propagation?
7. Should handled effects be removed explicitly from effect variables, such as `e - Logger`, or inferred from the handler body?
8. Is full row polymorphism needed, or is a smaller effect-variable model with union and removal enough?
9. What are the exact provider shadowing and lookup rules for `$.with(...)`, context spread, and `$.use(...)` context syntax?
10. Should all functions whose own body contains suspension points require both bang-suffixed declarations and bang-call syntax?
11. Should `$.use(...)` provider lookup be cached per lexical scope, per function invocation, or resolved at every use site?

## Sandbox, Capabilities, And Resumption

1. Should capabilities be the same thing as effects, or should capabilities be a lower-level runtime permission model underneath effects?
2. Which capabilities should exist initially: filesystem, network, database, clock, randomness, subprocess, secrets, auth context, logging, tracing, metrics, or cloud resources?
3. Should capabilities be declared in function signatures, annotations, handlers, package manifests, or inferred by tooling?
4. What state should interactive resumption preserve: variables, effect handlers, imports, logs, previous outputs, random seeds, or all execution steps?
5. Should interactive resumption replay prior cells/steps like notebooks, restore snapshots, or use explicit checkpoints?
6. For durable workflows, where should checkpoints be declared: standard-library calls, annotations, effect handlers, or inferred around external effects?
7. How should resumed execution prove that its capabilities and handlers are compatible with the original run?

## Serializable Closures And Incremental Computation

1. Should serializable closures be inferred by the compiler, explicitly annotated, or represented as a distinct function type?
2. What values are legal to capture: primitives, structs, branded values, handles, effect handlers, capabilities, or runtime resources?
3. How should tooling report non-serializable captures?
4. Should closure code identity be based on source hash, compiler artifact hash, stable symbol ID, or runtime registration?
5. Should cache keys include code identity, arguments, captured values, effect inputs, or all of these?
6. Should incremental dependencies be inferred from effects/data reads, declared through annotations, or both?
7. How should cache invalidation and recomputation be explained to AI agents and human reviewers?

## Adoption And Runtime

1. How should the first JavaScript compilation target work: readable JS, TypeScript, Node-first runtime, browser-first runtime, or bundler output?
2. How should the first WebAssembly target work: component model, WASI, browser WASM, or embedded runtime?
3. Should garbage collection be simple and predictable, or should runtime performance be a major design constraint?
4. Should the standard library be small and test-focused, or broad enough for real application development?
5. What should the smallest useful prototype demonstrate?

## Later Prototype Scoping

These questions are useful later, but are less relevant while the high-level language model is still being designed:

1. What is the smallest useful agent-tool prototype?
2. What is the smallest useful engineering/infra prototype?
3. Which compiler target should be implemented first within JavaScript and WebAssembly?
