# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../spec/README.md), which is authoritative. A
design record is removed once everything it decided is in the
specification; git history keeps it.

Planning and backlog:

- [Roadmap](ROADMAP.md) orders the remaining work into grammar, type
  checking, runtime, standard library, packages, prototype, audit cleanup, and
  agent tooling areas.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work.
- [Spec Follow-Ups](SPEC_FOLLOWUPS.md) logs decided editorial fixes, the
  glossary pass, and unnamed diagnostic codes, to apply in one pass.

Decided, not yet applied:

- [Error Conversion: The `@error` Intrinsic](ERROR_CONVERSION.md) holds
  decision 10, error derivation as one compiler intrinsic. The rest of the
  error design is in the specification.
- [Pipe Operator And Iterator `map`/`fold`](PIPE_OPERATOR.md) surveys pipes,
  UFCS, and extension methods in 17 languages and records owner decisions
  PL1-PL10.
- [Chaining Study: Pipe, Function Shorthand, And Iterator Adapters](CHAINING_STUDY.md)
  measures six combinations on one real-code corpus and records owner
  decisions CS1-CS3, which amend PL8; CS5 and CS6 are still open.
- [Dependencies Through Version Control](DEPENDENCIES.md) surveys
  registry-free dependency management and records owner decisions
  DEP1-DEP7 (Go modules in hd spelling).
- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package
  managers and drafts the `hd.toml` schema, versioning, resolution, and
  lockfile under owner decisions 1-14, some now overturned by DEP1.

Open questions for the owner:

- [Typed Derivation: Open Points](TYPED_DERIVATION.md) lists what M1-M29
  leave undecided and the M26 readings awaiting confirmation.
- [Nominal Function Types: Per-Declaration Data For Tools](FN_TYPE.md)
  keeps questions 9 and 10, how tool adapters get per-declaration data.
- [Literal Suffixes: Open Points](LITERAL_SUFFIXES.md) keeps three
  prefixed-string readings.
- [Operator Traits: Open Points](OPERATOR_TRAITS.md) keeps three readings
  from the apply passes.
- [Testing Redesign: Open Points](TESTING.md) keeps the property-test API
  beyond T53 and two deferred fixtures.
- [Stress Test 2026-09-29: Open Points](STRESS_2026_09_29.md) keeps two
  decorator readings.
- [Method And Field References](METHOD_REFERENCES.md) designs CS5: it
  surveys references in eight languages and compares five options, from
  closures only to Kotlin-style `::` and Swift-style key paths.
- [Collecting Iterators Into Collections](COLLECT.md) compares five ways
  for a data `Iterator[T]` to end in a `Map`, a `Set`, or an all-or-nothing
  `Result`, with the duplicate-key policy and the spec-or-STDLIB split.
- [Iterator Performance Study](ITERATOR_PERF.md) is stage 1 of a
  performance study: it compares closure, nested, and flat-stage iterator
  designs by calls, allocation, and fusion potential, and specifies the
  stage 2 benchmarks.

Research and direction outside the specification:

- [Runtime and Library Design](RUNTIME_AND_LIBRARY.md) describes the
  standard-library, tooling, and runtime direction, including the decided
  [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules) and the test runner.
- [Standard Library Design](STDLIB.md) drafts hd's module tree, effect
  traits with deterministic providers, and questions for the owner. The
  standard library stays outside the specification by design.
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [Wasm GC MVP Implementation Plan](../src/MVP_IMPLEMENTATION_PLAN.md) records the
  chosen fast-iteration compiler plan and its deliberately limited slices.
