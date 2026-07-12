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

1. How should inline validation annotations and external `annotate <Facet> for <Target>` blocks attach to fields, aliases, and function parameters?
2. For each effect, what should the compiler/tooling generate: handler requirements, mock handlers, dependency graphs, audit reports, or observability metadata?
3. For each function, what should tooling show first: signature, effects, contracts, examples, tests, dependencies, or observability behavior?
4. For each tool function, what should be generated: JSON Schema, OpenAPI, MCP definitions, TypeScript types, docs, examples, or runtime registration?
5. For each property test failure, what structured output should be produced for AI repair?
6. For each registered resource or workflow, what should be derived from code versus explicitly annotated?

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

1. What should `Result[T, E]` ergonomics look like for compiler-verifiable error handling: constructors, matching, diagnostics, and interaction with `?` propagation?
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
3. Should dependency handlers be selected explicitly at call sites, by lexical scope, by test configuration, or by runtime environment?
4. Should effects be part of public API compatibility?
5. Should an AI agent be able to replace real dependencies with mock handlers automatically?
6. Should effect polymorphism use explicit row variables, parameter-linked effects, or inferred propagation?
7. Should handled effects be removed explicitly from effect variables, such as `e - log`, or inferred from the handler body?
8. Is full row polymorphism needed, or is a smaller effect-variable model with union and removal enough?
9. What are the exact handler selection rules for dependencies resolved with `use(Database)`?
10. Should all functions whose own body contains suspension points require both bang-suffixed declarations and bang-call syntax?
11. Should `use(Database)` be cached per lexical scope, per function invocation, or resolved at every call?

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
