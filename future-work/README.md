# Future Work

These documents cover deferred or exploratory work outside the accepted
language specification:

- [Roadmap](ROADMAP.md) orders the remaining work into grammar, type
  checking, runtime, standard library, packages, prototype, audit cleanup, and
  agent tooling areas.
- [Durable Replay: Core Or Library](DURABLE_REPLAY.md) records which parts of
  durable replay the compiler and runtime provide and which a library builds;
  all fifteen owner decisions are applied.
- [Spec Follow-Ups](SPEC_FOLLOWUPS.md) logs decided editorial fixes, the
  glossary pass, unnamed diagnostic codes, and open restyle questions.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work.
- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package managers
  and drafts the `hd.toml` schema, versioning, resolution, and lockfile for
  roadmap area 5, revised to the owner's decisions 1-14.
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [Testing Redesign](TESTING.md) records the owner's test redesign
  decisions T1-T12 (a `tests:` block per file, `test("name"):` as a
  library intrinsic, a `Termination` trait and `ExitCode` shared with `main`); not yet
  in the specification.
- [Typed Derivation: Survey And Design Options](TYPED_DERIVATION.md)
  surveys derivation in other languages and records how libraries derive
  typed trait implementations, schemas, and tool adapters (roadmap area 2).
  Owner decisions M1-M23 fully decide the design, and they are applied in
  [Typed Derivation](../spec/14-annotations.md#typed-derivation); the
  prototype is pending. The few remaining open points are listed.
- [Typed Derivation: Stress Test Of The M1-M11 Design](DERIVATION_STRESS_TEST.md)
  tests the current derivation design against 21 use cases and ranks the problems found, with questions for the owner.
- [Typed Derivation: Stress Test Round 2 (M1-M15)](DERIVATION_STRESS_TEST_2.md)
  retests the design after typed member handles (M14) and the error
  intrinsic (M15, now spelled `@error`), maps round 1's problems to their
  status, and ranks the remaining and new ones, with questions for the
  owner.
- [Typed Derivation: Stress Test Round 3 (M1-M19)](DERIVATION_STRESS_TEST_3.md)
  retests the value-driven walk (M19) on 14 library cases, proposes a
  `Source` protocol for input-driven `build`, and ranks what still breaks.
  The owner answered round 3 with decisions M20 and M21.
- [Nominal Function Types](FN_TYPE.md) makes function types standard
  generic constructors such as `Fn[(Is...), O, R]`, so they can be
  implementation targets; decisions 1 to 9 are applied to the
  specification, and per-declaration data for tool adapters (decision 10)
  stays parked with typed derivation.
- [Runtime and Library Design](RUNTIME_AND_LIBRARY.md) describes the broader
  standard-library, tooling, and runtime direction.
- [Standard Library Design](STDLIB.md) surveys other standard libraries and
  drafts hd's module tree, effect traits with deterministic providers, and
  questions for the owner.
- [Error Conversion](ERROR_CONVERSION.md) surveys error composition in other
  languages and compares designs for using `?` across domain error types.
- [Error Design Stress Test (V1, V2a, V2b)](ERROR_STRESS_TEST.md) translates
  real crates' error types (ripgrep, cargo, serde_json, reqwest,
  `std::io::Error`, ast-grep, naga, globset) into the recorded
  error-derivation design (now the `@error` intrinsic) and two message-free
  variants, compares them, and ranks the problems found, with questions for
  the owner.
- [Runtime Type Identity](INSPECTABLE.md) records the design of the
  `Inspectable` trait, `TypeId`, and `downcast`, and the owner decisions now
  in the specification.
- [Enum Semantics: Value Category And Identity](ENUM_SEMANTICS.md) asks
  whether enums, `Option`, and `Result` belong with values (`AnyVal`) or
  references (`AnyRef`) and surveys other languages. The owner kept every
  enum `AnyRef` with identity, made enum values immutable, and turned shared
  constructor data into per-variant constants; the decisions are applied.
- [Wasm GC MVP Implementation Plan](../src/MVP_IMPLEMENTATION_PLAN.md) records the
  chosen fast-iteration compiler plan and its deliberately limited slices.

Accepted language behavior belongs in the [formal specification](../spec/README.md).
