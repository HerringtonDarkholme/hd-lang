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

Open questions for the owner:

- [Typed Derivation: Open Points](TYPED_DERIVATION.md) lists the four
  points that M1-M30 leave to other areas.
- [Nominal Function Types: Per-Declaration Data For Tools](FN_TYPE.md)
  keeps questions 9 and 10, how tool adapters get per-declaration data.
- [Testing Redesign: Open Points](TESTING.md) summarizes the applied
  property-test decisions PT1-PT9 and Q5-Q10, and the two apply-pass
  readings the owner confirmed. The generator parameter style and two
  deferred fixtures wait for the owner.
- [Special Cases: Inventory And Simplification](SPECIAL_CASES.md) lists
  hd's compiler intrinsics, rule exceptions, special syntax, magic names,
  and single-construct diagnostics, and ranks nine cuts as owner questions.
- [Syntax And Semantics Cost Review](SYNTAX_SEMANTICS_COST.md) weighs
  each language-tier syntax form and rule exception by its rule count
  against its use in real hd code. It ranks six cuts, including C4-C7,
  and marks seven owner decisions, led by embedding, literal sugar, and
  packs, as possible reopens.
- [Reopen: Packs And Literal Sugar](REOPEN_PACKS_LITERALS.md) follows the
  owner's reopening of packs and literal sugar on 2026-09-30. It shows how
  `all!` and arity-generic impls work with no packs, through tuple-kinded
  `Fn` inputs and one call intrinsic. It also trims literal suffixes and
  prefixes from 89 rules to 38, and asks nine questions.
- [Spec Tiers](SPEC_TIERS.md) splits the spec into a language tier and a
  stdlib tier under `spec/std/`, with an inventory, a conformance split,
  and a migration plan. The owner decided ST1-ST8 on 2026-09-30; step 1,
  the `spec/std/` scaffold, is done.
- [Call Indexing](CALL_INDEXING.md) studies the owner's idea of indexing
  with call syntax, `list(0)` and `list(0) = v`, so `[]` after an operand
  means only type arguments. It counts 178 index sites, surveys how
  trait-based languages separate fields from methods, and compares the two
  remaining options, B and C1. It also shows how B can keep a live
  variable, `var()`, while arrays keep `xs[0]`. The owner chose B+F as
  D1 to D4 on 2026-09-30, and all four are applied: the `::[` type
  arguments, and the callable values, `Apply` and `Update`. Its batch 25
  follow-ups, BF and BFF, are applied too.

Deferred:

- [Iterator Performance Study](ITERATOR_PERF.md) compares closure, nested,
  and flat-stage iterator designs. The closure design stands (CS8); the
  flat-stage design and a rerun of the stage 2 benchmarks wait for a
  specializing compiler.

Research and direction outside the specification:

- [Runtime and Library Design](RUNTIME_AND_LIBRARY.md) describes the
  standard-library, tooling, and runtime direction, including the decided
  [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules), the test runner, and
  package tooling.
- [Standard Library Design](STDLIB.md) drafts hd's module tree, effect
  traits with deterministic providers, and the owner's decisions on them.
  The standard library stays outside the specification by design.
- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package
  managers and drafts the `hd.toml` schema, the checked compatibility
  rule, and the agent-first CLI. The registry-free model of DEP1-DEP19 is
  in [Package Manifest](../spec/10-modules.md#package-manifest).
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.
- [Wasm GC MVP Implementation Plan](../src/MVP_IMPLEMENTATION_PLAN.md) records the
  chosen fast-iteration compiler plan and its deliberately limited slices.
