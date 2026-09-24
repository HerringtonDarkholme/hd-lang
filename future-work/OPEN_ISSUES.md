# Open Issues

The core language currently has no unresolved decision that blocks an
implementation. This is the single backlog for deliberately deferred language
design and unresolved runtime, library, ABI, product, or tooling work. Settled
behavior and explicit exclusions belong in the specification rather than this
list.

## Deferred Language Design

### Expressions And Control Flow

- **First-class bound-method values.** Direct `receiver.method()` calls are
  defined, but bare `receiver.method` as a value is not. A future design must
  settle the reference syntax, `self` versus `mut self` capture and lifetime,
  and how suspension and requirements appear in the resulting function type.

### Resources And Lifetime

- **Weak references and user-visible finalizers.** Revisit these separately
  from deterministic resource cleanup.
- **Resource ownership and deterministic cleanup.** The language does not yet
  prevent a resource from escaping before deferred cleanup invalidates it. A
  future design must cover ownership, alias escape, cleanup failure, suspension,
  and use-after-disposal checking. See the
  [ownership and escape research](OWNERSHIP_AND_ESCAPE_RESEARCH.md) for design
  background.

### Traits And Types

- **Runtime type tests and downcasting.** Dynamic trait values remain opaque;
  `is` continues to mean reference identity. A future design may permit tests
  or casts from a trait value to a concrete or other trait type.
- **Negative trait implementations.** No current use case requires them;
  explicit conformance and the existing overlap rules remain sufficient.
- **Permission weakening with generic variance.** Direct conversions such as
  `mut Cell[Cat]` to `Cell[Animal]` remain unspecified.

### Variadic Generics

- **Additional pack operations.** Filtering, indexing, splitting,
  concatenation, length arithmetic, and runtime iteration remain deferred.
  None is required by the accepted `all!` design; revisit them only for a
  concrete use case.

## Runtime, Library, And ABI Work

The canonical background is in
[Runtime and Library Design](RUNTIME_AND_LIBRARY.md). Remaining work belongs to
the standard library, package tooling, or runtime rather than core syntax:

- checked panic reporting and host representation for overflow, invalid
  shifts, failed casts, bounds errors, and related defects;
- the complete `hd.toml` schema, lockfile, and dependency resolver;
- the Wasm component ABI and registration adapters;
- property-testing strategies, derivation, shrinking, replay, correlated and
  stateful inputs, and failure artifacts in `std.testing`;
- standard capability-trait granularity and host-provider configuration;
- workflow version compatibility and stable event and suspension identity;
- observability instruments, privacy and redaction, sampling, exporter
  configuration, and custom spans;
- serializable-closure capture, identity, compatibility, security, migration,
  and execution guarantees;
- the `std.incremental` API, dependency identities, cache and storage policy,
  graph lifetime, collection granularity, and inspection integration.

## Product And Tooling Work

These questions should be answered through concrete libraries and prototypes,
not additional core syntax:

- which tool and RPC artifacts to generate first, such as JSON Schema,
  OpenAPI, MCP, typed client definitions, or documentation;
- the explicit registry and deployment APIs for tool, RPC, workflow, and
  host-callable Wasm adapters;
- how authentication, tenant, and access-control metadata compose with
  ordinary function requirements;
- which compiler reports best support human review and AI repair loops;
- how annotation facets model validation, database schemas, UI, retention,
  observability metadata, and infrastructure resources;
- how generated infrastructure is reviewed, synchronized, versioned, and
  connected to controlled operational telemetry without exposing raw
  production data.

## Resolution Process

After resolving an item:

1. update the owning specification or design document;
2. remove the item from this file;
3. add valid and invalid conformance fixtures where applicable;
4. run `spec/check.sh`.
