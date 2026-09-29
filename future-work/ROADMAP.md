# Roadmap

This plan groups the remaining work into eight areas. Unresolved design
questions stay in [Open Issues](OPEN_ISSUES.md); this document says in what
order they are worked on and where each one belongs. Access control, tenancy,
and per-tool requirement reports are application-level concerns and are not
part of this plan.

## 1. Grammar Audit

Scope: [Lexical Structure](../spec/01-lexical-structure.md) and
[Grammar](../spec/02-grammar.md).

Goal: find ambiguous or poorly designed grammar before more is built on it.
The trait supertrait `:` was missed by earlier reviews; more such cases are
expected.

- Mechanical search:
  - convert the grammar for a GLR or Earley parser and report inputs with
    more than one parse;
  - check FIRST/FOLLOW conflicts wherever `:`, `[`, `$`, `!`, `?`, or
    indentation can start different constructs;
  - fuzz the reference parser against the specification text to find
    disagreements.
- Known risky spots:
  - the overloaded `:` (supertraits, type annotations, suite openers, named
    arguments, map literals);
  - `[` for generic arguments versus indexing;
  - `?` as the optional type versus propagation;
  - the `$` requirement row versus string interpolation;
  - trailing blocks and the multiple-closure form;
  - reserved versus contextual keywords;
  - line continuation inside indentation.
- Comparison: how Python, Kotlin, Swift, and MoonBit resolve the same
  collisions.
- Scope reduction was closed on 2026-09-27: every feature stays, including
  loop-`else` values, binding expressions, and trailing-block forms.
- Output: an ambiguity list with a proposed fix for the owner to decide, and
  conformance fixtures for each case.

## 2. Type-Checking Rules

Scope: [Type System](../spec/04-type-system.md), [Traits](../spec/09-traits.md),
[Variadic Generics](../spec/12-variadic-generics.md), and
[GADTs](../spec/13-gadts.md).

Goal: state trait behavior as normative rules rather than prose.

- Trait satisfaction:
  - impl selection, coherence, the orphan rule, and overlap;
  - supertrait obligations;
  - how bounds are discharged, including inline bounds and dictionaries;
  - when an embedded field satisfies a trait;
  - default-method conflicts;
  - dynamic safety.
- Trait derivation: what `@derive` produces, when it is legal, and how derived
  implementations interact with written ones and with generics.
- Method resolution: lookup order across inherent methods, traits, embedding,
  and `mut` access.
- Research:
  - Rust: the trait solver, coherence and orphan rules, auto traits, object
    safety, and `#[derive]`;
  - MoonBit: `derive(Show, Eq, Hash, ToJson, ...)` and `impl Trait for Type`;
  - Swift: protocol conformance and synthesized conformances;
  - Kotlin: interface delegation.
- Done: runtime type identity is specified in
  [Runtime Type Identity](../spec/09-traits.md#runtime-type-identity), and
  complete runtime shape coverage in
  [Common Shape Representation](../spec/14-annotations.md#common-shape-representation).
  Typed derivation (owner decisions M1-M29) is applied in
  [Typed Derivation](../spec/14-annotations.md#typed-derivation). Annotation
  locality was superseded: derivation blocks live in the type's module.
  GADTs, declared variance, and variadic packs stay (scope reduction was
  closed on 2026-09-27).
- Moved here:
  - the open parts of
    [Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets),
    listed in [Typed Derivation: Open Points](TYPED_DERIVATION.md), and
    the `@error` intrinsic, decided but not yet applied
    ([Error Conversion](ERROR_CONVERSION.md));
  - the propagation rules and dependent-return provenance from
    [Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy);
  - [Confirmed Deferred Type Features](OPEN_ISSUES.md#confirmed-deferred-type-features).
- Output: a normative rules section per topic, a findings list, and questions
  for the owner.

## 3. Runtime: Durable Replay

Scope: [Requirements and Suspension](../spec/11-requirements-and-suspension.md)
and [Runtime and Library Design](RUNTIME_AND_LIBRARY.md).

Goal: decide, with evidence, whether durable replay is a core language feature
or a library feature. Decided: a runtime feature with a small specification
and compiler contract, with the rest in libraries. All seventeen durable
replay decisions are applied to
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules) and the specification
([Determinism](../spec/11-requirements-and-suspension.md#determinism)).

- Question: what must the compiler provide, and what can a library build?
  - Compiler candidates: stable suspension-site IDs, code identity, provider
    call interception, and determinism checks.
  - Library candidates: event-log storage, retry policy, and workflow APIs.
- Research: Temporal and Restate (library plus server), Golem (Wasm-level
  durability), Azure Durable Functions, Unison (content-addressed code), and
  Flix and Koka (effect handlers).
- Test: can a library get replay right without compiler support? What it
  cannot do is core; the rest is library.
- Moved here:
  - [Observability Hooks](OPEN_ISSUES.md#observability-hooks), which use a
    hook separate from replay (Durable Replay decision 14);
  - [Serializable Closures And Incremental Computation](OPEN_ISSUES.md#serializable-closures-and-incremental-computation);
  - weak references and finalizers;
  - asynchronous and fallible cleanup;
  - the `std.fingerprint` algorithm;
  - runtime-profile panic codes;
  - the Wasm component ABI.

## 4. Standard Library

Goal: make hd useful, with requirements as the feature that sets its library
apart. Design notes are in [Standard Library Design](STDLIB.md), and move
into the specification once accepted.

- Survey: module layout and naming in Python, Rust, Kotlin, and Swift, with
  MoonBit and Go as small-language comparisons.
- Requirement-aware design:
  - every effect is a requirement trait: `Clock`, `Random`, `Fs`, `Net`,
    `Env`, `Console`, `Process`;
  - each has a deterministic test provider;
  - there are no ambient globals;
  - pure modules need no requirement.
- Layers:
  - core: numbers, strings and text, optionals and `Result`, errors,
    collections, iterators, comparison, and hashing;
  - effect traits and their providers;
  - `std.task`: combinators, structured-scope `Task[T]` (STDLIB decision
    11), timeout, race, and retry;
  - serialization and JSON, which depend on derivation from area 2;
  - `std.testing`: property testing, shrinking, and providers;
  - time and fingerprint. `Secret[T]` was removed from the design for now
    ([STDLIB decision 12](STDLIB.md#owner-decisions)).
- Error conversion: how `?` combines errors from several domains is
  specified in [Propagation](../spec/05-expressions.md#propagation); the
  error-chain helpers are library API in [STDLIB](STDLIB.md#stderror).
- Moved here: the host capability catalog, provider configuration format,
  task combinators, property testing, the library half of derivation,
  generated artifacts, and exporter configuration.

## 5. Packages

Scope: [Modules](../spec/10-modules.md#package-manifest), which defines the
language-level package and `use` model and leaves the rest to this area.

Goal: specify how hd code is split into packages, versioned, and depended on,
so that libraries, including the standard library, can be shared and builds
repeat exactly.

- Manifest: the complete `hd.toml` schema, including source roots, entry
  points and executable-main selection, test and development dependencies,
  and target or runtime profile.
- Versions: version syntax and constraint semantics, and what counts as a
  breaking change. Package interface files make a checked compatibility rule
  possible: compare the public interface of two versions and require a major
  version for incompatible changes.
- Resolution: the dependency resolver, whether two versions of one package may
  coexist, and how the orphan and coherence rules from area 2 apply across
  packages.
- Lockfile: its format, content hashes, and reproducible builds.
- Standard library: how `std` is versioned and tied to the compiler version.
- Distribution: there is no registry (owner, 2026-09-28). Dependencies
  come from version control hosts, as decided in
  [Dependencies](DEPENDENCIES.md#owner-decisions) (DEP1-DEP7, not yet
  applied), which overturns [Packages](PACKAGES.md#owner-decisions)
  decisions 2, 3, and 11.
- Agent use: every package operation is a non-interactive command with
  machine-readable output, and package metadata is queryable through the
  program database in area 8.
- Research: Cargo, uv and `pyproject.toml`, Go modules (minimal version
  selection), Swift Package Manager, Gradle for Kotlin, and MoonBit's `mooncakes`.
- Moved here: the `hd.toml` schema, executable-main selection, lockfile,
  version constraints, and dependency resolver from
  [Runtime, Library, ABI, And Tooling Work](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work).

## 6. Prototype Catch-Up

Needs no owner decisions; tracked in [the audit folder](../audit/README.md).

- The applied decisions the prototype does not follow yet, listed in the
  audit README with the conformance cases each one keeps failing.
- The open prototype findings in `audit/findings/`, grouped by ID in
  `audit/evidence/w9/failures-by-id.tsv`.

## 7. Audit Cleanup

Goal: keep [the audit folder](../audit/README.md) limited to what is still
open.

- Re-run each remaining finding in `audit/findings/` against the current
  prototype and specification; delete the finding when it no longer
  reproduces or its decision is applied.
- Remove evidence, probes, and scripts that only back deleted findings.
- Renumber `audit/REPORT.md`, update `audit/evidence/findings-table.md`, and
  recount `audit/evidence/w9/failures-by-id.tsv` from
  `test/portable/KNOWN_FAILURES.tsv`.
- Repeat after each area above lands, so solved items do not accumulate.

## 8. Tooling For Agents

hd is designed primarily for coding agents to write and for humans to read.
Tooling follows José Valim's
[Evolving programming languages in the AI era](https://x.com/josevalim/status/2103133294317445290):
agents need queryable program facts and runtime state, not IDE interfaces.
A language server is therefore low priority.

- Program database: expose symbols, definitions, references, call graphs,
  types, trait implementations, requirement rows, and suspension points
  through a query interface (SQLite, Datalog, or a small DSL) addressed by
  name rather than file position. Agents can then compose questions such as
  "every public function that eventually requires `Fs`" or "every path where
  this value can be `.None`", and the same queries can serve as lint rules.
- Name-addressed CLI queries: documentation and definition lookup by symbol,
  extending the existing `explain-requirements` and `trace` commands.
- Runtime observability over debuggers: queryable traces of suspension,
  provider calls, and replay events, sharing the interception point decided
  in area 3.
- Machine-readable diagnostics with stable codes and suggested fixes.
- Explicitness over inference where it adds guarantees: agents do not mind
  writing annotations, so area 2 should prefer rules that check stated types
  over rules that only infer them. Locality rules that forbid action at a
  distance, such as derivation blocks living in the type's module, serve
  the same goal.
- The REPL stays: it is how humans try the language, so it keeps up with the
  specification.
- Lower priority: a formatter and a language server for human reading.

Owner decisions (2026-09-27) on the program database: it is a compiler
feature, not specification text. It is queried with SQL, SQLite preferred
as an implementation detail; recursive questions use `WITH RECURSIVE`. It
is a snapshot (`hd index` / `hd check` write it; `hd query` rebuilds it when
stale; programs that do not type-check still get their parse-level facts,
marked partial), not a live service. The fact schema, the `--format json`
diagnostic records, and the symbol-name format (`pkg.user.User`,
`User.email`) are tooling contracts versioned with the toolchain and
documented in `src/README.md`; diagnostic codes stay normative. The checker
tags each diagnostic with the exact rule ID that fired where it knows it,
added incrementally, falling back to the code's rule list.

## Order

1. Areas 1 and 2 run in parallel. Findings go under `audit/`; questions go to
   [Open Issues](OPEN_ISSUES.md) for the owner.
2. Area 3 runs alongside them. It is mostly research, and its outcome shapes
   `std.task` and the replay APIs.
3. Area 4 starts its survey now; its design starts after areas 2 and 3 settle
   traits, derivation, and requirements.
4. Area 5 starts with its survey and manifest schema now; version and
   resolution rules follow area 2's coherence rules.
5. Area 6 continues in the background.
6. Area 7 runs first, and again after each other area lands.
