# Design Questions

This file is the navigation index for unresolved hd-lang design work. It is not
a second specification and does not repeat settled answers. Stable-core status
is tracked in the [Specification Completion Register](spec/OPEN_ISSUES.md), and
accepted syntax and semantics live in the [formal specification](spec/README.md).

## Provisional Language Features

The detailed questions are maintained beside the designs they affect:

- [Requirements and Suspension](spec/provisional/requirements-and-suspension.md#open-issues): driver APIs, cancellation details, provider selection, requirement-row polymorphism, and cleanup interaction.
- [Variadic Generics](spec/provisional/variadic-generics.md#open-issues): exact expansion grammar, explicit pack arguments, inference, and expansion count.
- [GADTs](spec/provisional/gadts.md#open-issues): formal refinement/unification, explicit constructor arguments, existentials, variance, and reflection.
- [Annotations](spec/provisional/annotations.md#open-issues): final shape APIs, generic targets, materialization, missing-child policy, generic implementations, whole derivation replacement, optional decorator sugar, manual laziness, and zero-sized facet values.

Moving one of these features into stable core requires resolving its chapter's
questions, integrating its grammar into `spec/02-grammar.md`, and adding parser,
typing, and runtime conformance cases.

## Runtime And Library Backlog

The canonical runtime discussion is in
[Runtime and Library Design](RUNTIME_AND_LIBRARY.md). Remaining work includes:

1. Property-testing strategies, derivation, shrinking, replay, correlated and stateful inputs, and failure artifacts in `std.testing`.
2. Standard capability-trait granularity and host provider configuration.
3. Workflow version compatibility, event/suspension identity, and component ABI details.
4. Observability instruments, privacy/redaction, sampling, exporter configuration, and custom spans.
5. Resource ownership or lifetime enforcement, cleanup syntax, failure behavior, alias escape, and suspension interaction.
6. Serializable-closure capture, identity, compatibility, security, migration, and execution guarantees.
7. The `std.incremental` API, dependency identities, cache/storage policy, graph lifetime, collection granularity, and inspection integration.

## Product And Tooling Backlog

These questions should be answered through concrete libraries and prototypes,
not additional core syntax:

1. Which tool/RPC artifacts to generate first: JSON Schema, OpenAPI, MCP, typed client definitions, or documentation.
2. The explicit registry and deployment APIs for tool, RPC, workflow, and host-callable Wasm adapters.
3. How auth, tenant, and access-control metadata compose with ordinary function requirements.
4. Which compiler reports best support human review and AI repair loops.
5. How annotation facets model validation, database schemas, UI, retention, observability metadata, and infrastructure resources.
6. How generated infrastructure is reviewed, synchronized, versioned, and connected to controlled operational telemetry without exposing raw production data.

Function contracts/invariants, ownership transfer, property-test syntax, and a
dedicated concurrency control-flow construct remain deliberately outside the
MVP. Reopening one requires a motivating program that cannot be expressed
cleanly through the stable core plus a library.
