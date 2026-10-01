# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../spec/README.md), which is authoritative.

Once everything a design record decided is applied, superseded, or moved
to [Open Issues](OPEN_ISSUES.md), the record moves to [archive/](archive/)
with an "Archived" note at its top. Archived records are history: they
keep the reasoning behind decisions, and `pnpm run spec refs --dead`
treats every citation in them as history.

## Active Records

Planning and backlog:

- [Roadmap](ROADMAP.md) is one page: the order of the remaining work, and
  what is parked or on hold.
- [Open Issues](OPEN_ISSUES.md) is the single backlog for unresolved language,
  runtime, library, ABI, product, and tooling work, including undecided
  standard-library design. It also lists the applied decision batches, each
  linked to the Revision Notes.

Open questions for the owner:

- [Shape Review](SHAPE_REVIEW.md) traces where `shape`, `shape_of`, and
  the shape types came from, finds no owner decision that approved them,
  and compares removing them, keeping only a function-fact read, and
  keeping them, against `Structure` templates.

Prototype plan:

- [Compiler/Library Audit](COMPILER_LIBRARY_AUDIT.md) sorts what the
  TypeScript prototype implements by hand into true intrinsics, library
  code movable to `lib/std` now or after a small hook, and language
  semantics. It plans eleven one-hour chunks that delete about 1,900
  lines of TS. The owner answered its four questions in batch 36.

Research and direction outside the specification:

- [Packages: Survey And Manifest Draft](PACKAGES.md) surveys package
  managers and drafts the `hd.toml` schema, the checked compatibility
  rule, and the agent-first CLI. The registry-free model of DEP1-DEP19 is
  in [Package Manifest](../spec/10-modules.md#package-manifest).
- [Ownership, Escape, And Compile-Time Concurrency Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md)
  surveys possible foundations for future lifetime and resource-safety work.

## Archived Records

Each archived record's note says where its open points went.

Archived 2026-10-01 with their open points moved to
[Open Issues](OPEN_ISSUES.md):

- [Typed Derivation: Open Points](archive/TYPED_DERIVATION.md) recorded
  M30, SR1, and SIMPLE. Its four open points are in
  [Open Issues](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
- [Nominal Function Types](archive/FN_TYPE.md) applied decisions 1-9.
  Questions 9 and 10 are under
  [parked tool adapters](OPEN_ISSUES.md#parked-tool-adapters).
- [Testing Redesign: Open Points](archive/TESTING.md) summarized PT1-PT9,
  Q5-Q10, and the AT decisions. The generator parameter style and two
  deferred fixtures are in
  [Testing Open Points](OPEN_ISSUES.md#testing-open-points).
- [Special Cases: Inventory And Simplification](archive/SPECIAL_CASES.md)
  listed hd's intrinsics, rule exceptions, special syntax, and magic names,
  and ranked nine cuts. Q2 and Q3 wait for
  [the code revamp](OPEN_ISSUES.md#codes-waiting-for-the-code-revamp).
- [Iterator Performance Study](archive/ITERATOR_PERF.md) compared closure,
  nested, and flat-stage iterators. The closure design stands (CS8); the
  flat-stage design waits for a specializing compiler, task #86.
- [Standard Library Design](archive/STDLIB.md) drafted the module tree and
  effect traits. Decided std APIs are in [spec/std/](../spec/std/README.md).
- [Runtime and Library Design](archive/RUNTIME_AND_LIBRARY.md) described
  the runtime, test runner, and package tooling direction, including the
  decided [Replay Rules](archive/RUNTIME_AND_LIBRARY.md#replay-rules).
- [Wasm GC MVP Implementation Plan](archive/MVP_IMPLEMENTATION_PLAN.md),
  moved from `src/`, recorded the fast-iteration compiler plan and its
  slices.

Archived with every decision applied or superseded:

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
