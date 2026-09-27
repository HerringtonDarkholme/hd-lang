# Future Work

These documents cover deferred or exploratory work outside the accepted
language specification:

- [Roadmap](ROADMAP.md) orders the remaining work into grammar, type
  checking, runtime, standard library, packages, prototype, audit cleanup, and
  agent tooling areas.
- [Durable Replay: Core Or Library](DURABLE_REPLAY.md) decides which parts of
  durable replay the compiler and runtime provide and which a library builds.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work.
- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package managers
  and drafts the `hd.toml` schema, versioning, resolution, and lockfile for
  roadmap area 5.
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [Typed Derivation: Survey And Design Options](TYPED_DERIVATION.md)
  surveys derivation in other languages and proposes how libraries derive
  typed trait implementations, schemas, and tool adapters (roadmap area 2).
- [Typed Derivation: Stress Test Of The M1-M11 Design](DERIVATION_STRESS_TEST.md)
  tests the current derivation design against 21 use cases and ranks the problems found, with questions for the owner.
- [Typed Derivation: Stress Test Round 2 (M1-M15)](DERIVATION_STRESS_TEST_2.md)
  retests the design after typed member handles (M14) and `@derive(Error)`
  (M15), maps round 1's problems to their status, and ranks the remaining
  and new ones, with questions for the owner.
- [Nominal Function Types](FN_TYPE.md) proposes making function types
  standard generic constructors such as `Fn[(Is...), O, R]`, so they can be
  implementation targets, and compares per-declaration data for tool
  adapters, with questions for the owner.
- [Runtime and Library Design](RUNTIME_AND_LIBRARY.md) describes the broader
  standard-library, tooling, and runtime direction.
- [Standard Library Design](STDLIB.md) surveys other standard libraries and
  drafts hd's module tree, effect traits with deterministic providers, and
  questions for the owner.
- [Error Conversion](ERROR_CONVERSION.md) surveys error composition in other
  languages and compares designs for using `?` across domain error types.
- [Runtime Type Identity](INSPECTABLE.md) records the design of the
  `Inspectable` trait, `TypeId`, and `downcast`, and the owner decisions now
  in the specification.
- [Enum Semantics: Value Category And Identity](ENUM_SEMANTICS.md) asks
  whether enums, `Option`, and `Result` belong with values (`AnyVal`) or
  references (`AnyRef`), surveys other languages, and recommends making
  every enum an identity-free value, with questions for the owner.
- [Wasm GC MVP Implementation Plan](../src/MVP_IMPLEMENTATION_PLAN.md) records the
  chosen fast-iteration compiler plan and its deliberately limited slices.

Accepted language behavior belongs in the [formal specification](../spec/README.md).
