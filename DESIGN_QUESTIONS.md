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

1. What does "compiler-verifiable error handling" mean for this language: checked effects, typed results, required handlers, or all of them?
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
4. Which mistakes should be allowed to surface at runtime as effects?
5. Should the language prefer refusing ambiguous code, or accepting it with explicit runtime checks?

## Testing And Data Generation

1. Should every schema automatically define a valid data generator?
2. Should generated data be deterministic by default?
3. Should generated data be explainable, with a trace of which schema rule produced each value?
4. Should property tests be expected for all public functions, or only where explicitly written?

## Effects And Dependencies

1. Should effects mainly describe external dependencies, error paths, contracts, or all side-effectful behavior?
2. Should user-defined effects be lightweight enough to use for small mocks?
3. Should dependency handlers be selected explicitly at call sites, by lexical scope, by test configuration, or by runtime environment?
4. Should effects be part of public API compatibility?
5. Should an AI agent be able to replace real dependencies with mock handlers automatically?
6. Should effect polymorphism use explicit row variables, parameter-linked effects, or inferred propagation?
7. Should handled effects be removed explicitly from effect variables, such as `e - log`, or inferred from the handler body?
8. Is full row polymorphism needed, or is a smaller effect-variable model with union and removal enough?

## Sandbox, Capabilities, And Resumption

1. Should capabilities be the same thing as effects, or should capabilities be a lower-level runtime permission model underneath effects?
2. Which capabilities should exist initially: filesystem, network, database, clock, randomness, subprocess, secrets, auth context, logging, tracing, metrics, or cloud resources?
3. Should capabilities be declared in function signatures, annotations, handlers, package manifests, or inferred by tooling?
4. What state should interactive resumption preserve: variables, effect handlers, imports, logs, previous outputs, random seeds, or all execution steps?
5. Should interactive resumption replay prior cells/steps like notebooks, restore snapshots, or use explicit checkpoints?
6. For durable workflows, where should checkpoints be declared: standard-library calls, annotations, effect handlers, or inferred around external effects?
7. How should resumed execution prove that its capabilities and handlers are compatible with the original run?

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
