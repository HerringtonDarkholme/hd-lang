# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../spec/README.md), which is authoritative.

Once everything a design record decided is applied or superseded, the
record moves to [archive/](archive/) with an "Archived" note at its top.
Archived records are history: they keep the reasoning behind decisions,
and `pnpm run spec refs --dead` treats every citation in them as history.

## Active Records

Planning and backlog:

- [Roadmap](ROADMAP.md) orders the remaining work into grammar, type
  checking, runtime, standard library, packages, prototype, audit cleanup, and
  agent tooling areas.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work. It also lists the
  applied decision batches, each linked to the Revision Notes.

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
  and single-construct diagnostics, and ranks nine cuts. Batches 24, 26,
  and 31b answered seven of its questions; Q2 and Q3, which merge
  diagnostic codes, wait for the code revamp.

Prototype plan:

- [Compiler/Library Audit](COMPILER_LIBRARY_AUDIT.md) sorts what the
  TypeScript prototype implements by hand into true intrinsics, library
  code movable to `lib/std` now or after a small hook, and language
  semantics. It plans eleven one-hour chunks that delete about 1,900
  lines of TS. The owner answered its four questions in batch 36.

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

## Archived Records

Every decision in these records is applied or superseded.

- [Syntax And Semantics Cost Review](archive/SYNTAX_SEMANTICS_COST.md)
  weighed each language-tier syntax form by its rule count against its use
  in real hd code. Batches 24 and 26 answered its cuts, and its reopen
  questions led to the two records below.
- [Reopen: Packs And Literal Sugar](archive/REOPEN_PACKS_LITERALS.md)
  showed how `all!` and arity-generic impls work with no packs, and
  trimmed literal suffixes and prefixes. Batch 31 applied it: packs are
  gone.
- [Simplify Embedding](archive/SIMPLIFY_EMBEDDING.md) trimmed embedding,
  promotion, and `by` delegation from 219 rules to 61, keeping embedding.
  Batch 32 applied it.
- [One Compile-Time Intrinsic](archive/COMPTIME_UNIFICATION.md) asked
  whether one comptime or macro intrinsic could replace derivation,
  delegation, field facts, tuple traits, and `all!`. Batch 36 took
  generalized templates and typed member facts and rejected the rest.
  Its deferred code-generation idea is listed in
  [Open Issues](OPEN_ISSUES.md#ideas-noted-for-later).
- [Spec Tiers](archive/SPEC_TIERS.md) split the spec into a language tier
  and a stdlib tier under `spec/std/`. Decisions ST1-ST8 and batch 29 are
  applied, and the migration is complete.
- [Call Indexing](archive/CALL_INDEXING.md) studied indexing with call
  syntax. The owner chose B+F (D1-D4): `::[` type arguments, and callable
  values through `Apply` and `Update`. All of it is applied, with the
  batch 25 follow-ups BF and BFF.
