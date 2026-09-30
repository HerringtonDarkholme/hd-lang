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
  glossary pass, and unnamed diagnostic codes, all applied on 2026-09-29,
  batch 10 (SF1-SF3) included.

Decided, not yet applied:

- [Dependencies Through Version Control](DEPENDENCIES.md) surveys
  registry-free dependency management and records owner decisions
  DEP1-DEP7 (Go modules in hd spelling), applied in
  [Package Manifest](../spec/10-modules.md#package-manifest).
- [Strings: Go's Byte Model](STRINGS.md) records owner decisions
  STR1-STR6: a string is immutable UTF-8 bytes, `len` and `s[i]` count
  bytes, and iteration is explicit through `chars`, `char_indices`, and
  `bytes`.
- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package
  managers and drafts the `hd.toml` schema, versioning, resolution, and
  lockfile under owner decisions 1-14, some now overturned by DEP1.

Open questions for the owner:

- [Error Conversion: The `@error` Intrinsic](ERROR_CONVERSION.md) records
  decision 10, error derivation as one compiler intrinsic, applied in
  [Error Derivation](../spec/14-annotations.md#error-derivation), and the
  owner's answers to its apply-pass points (decisions 21-30); one later
  point waits for the owner.
- [Pipe Operator And Iterator `map`/`fold`](PIPE_OPERATOR.md) surveys pipes,
  UFCS, and extension methods in 17 languages. Its decisions PL3-PL13 are
  applied; three pipe readings wait for the owner.
- [Chaining Study: Pipe, Function Shorthand, And Iterator Adapters](CHAINING_STUDY.md)
  measures six combinations on one real-code corpus. CS2, CS7, and CS8 are
  applied; three iterator readings wait for the owner.

- [Typed Derivation: Open Points](TYPED_DERIVATION.md) lists the four
  points that M1-M30 leave to other areas.
- [Nominal Function Types: Per-Declaration Data For Tools](FN_TYPE.md)
  keeps questions 9 and 10, how tool adapters get per-declaration data.
- [Literal Suffixes: Open Points](LITERAL_SUFFIXES.md) keeps three
  prefixed-string readings.
- [Operator Traits: Open Points](OPERATOR_TRAITS.md) records its apply
  passes; no reading waits for the owner.
- [Testing Redesign: Open Points](TESTING.md) records the property-test API
  decisions PT1-PT9, applied; four points from applying them, the generator
  parameter style, and two deferred fixtures wait for the owner.
- [Stress Test 2026-09-29: Open Points](STRESS_2026_09_29.md) records its
  answers; no reading waits for the owner.
- [Method And Field References](METHOD_REFERENCES.md) designs CS5: it
  surveys references in eight languages and compares five options. Its
  decisions MR1-MR4 are applied; one reading waits for the owner.
- [Collecting Iterators Into Collections](COLLECT.md) compares five ways
  for a data `Iterator[T]` to end in a `Map`, a `Set`, or an all-or-nothing
  `Result`. Its decisions CO1-CO6 are applied.
- [Type-Argument Defaults](TYPE_ARG_DEFAULTS.md) designs general
  defaults, such as `C < FromIterator[T] = List[T]`, that replace the
  retired `flow.collect.default`. It surveys C++, Rust, TypeScript, Swift,
  and C#. Its decisions TD1-TD11 are applied or confirmed; nothing waits
  for the owner.
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
