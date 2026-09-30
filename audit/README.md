# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the grammar and type
audits that followed, are finished. Everything they found that has since been
fixed, applied to the specification, superseded, or dropped has been removed
from this folder. The spec's Revision Notes in
[`spec/README.md`](../spec/README.md#revision-notes) record the applied
decisions, and the repository history keeps the removed evidence.

## Conformance

On 2026-09-29 the prototype passes 1,399 of the 1,581 conformance cases, all
of them selected in `test/portable/cases.tsv`. The other 182 are listed in
`test/portable/KNOWN_FAILURES.tsv`, each tagged with a finding or with a
decision below, and all 182 still fail.
[`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) counts
them by tag:

| Tag | Cases | Why they fail |
| --- | ----- | ------------- |
| TDEF | 26 | no type-argument defaults, short explicit lists, `Rhs = Self` on the operator traits, or `argument-count` for a long list |
| PIPE | 16 | no `\|>` token or `_` placeholder |
| ATB | 19 | a binding names only the bound trait's own associated types; trait value types and requirement keys take no binding; no `ambiguous-associated-type`; an unbound requirement key is accepted |
| LMUT | 18 | no parenthesized `let (a, b)` list, in a same-line suite or not, bare `let a, b` still accepted, no `mut-on-primitive` or `redundant-let-mut` |
| F-250 | 15 | packs, GADT variant results, and package roles give generic diagnostics |
| MREF | 10 | `Type::name` without a call is still `deferred-method-value` |
| STR | 14 | `len` still counts scalars; `s[i]` is `unsupported-string-indexing`; no `chars`, `char_indices`, `bytes`, or `slice`; `List`, `Map`, and `string` implement no `Index` |
| ITER | 8 | `Iterator` is still a trait; no `from_fn`, `map`, `fold`, or `Iterable` for iterators |
| ERRD | 24 | no `@error` intrinsic or its batch 9 and 11 codes and bounds: `@error` resolves as an ordinary decorator, so `error`, `from`, and `source` are unknown names |
| SF | 6 | spec follow-ups batch 10: `impl i32:` is `unknown-type`, a raw tab in a string is accepted, and `==` without `Eq` still reports `missing-partial-eq` |
| PT | 5 | property-test batch 12: `int` and `float` are not generic, no `string(max_chars)`, `map`, `examples`, draw budget, `Arbitrary` template, or `arbitrary.with` |
| EMB-S | 4 | trait availability needs package roles |
| P2 | 4 | member visibility needs package roles |
| COLLECT | 4 | `collect` returns only `List[T]`, and there is no `FromIterator` |
| OPF | 3 | `m[k] op= v` on a `Map` reads `V?`, and a newtype unwraps to a readonly base |
| TQ-2 | 1 | package roles |
| M29 | 1 | the unused-fact warning needs a second package |
| F-259 | 1 | the `disposed-file` runtime profile does not exist |
| DC7 | 1 | group statements are not interleaved across modules |
| GQ4 | 1 | the prototype has no pack operations |
| MHP-1 | 1 | no inferred script entry requirement row |

## What Remains

| Path | What it holds | Why it stays |
| ---- | ------------- | ------------ |
| [`REPORT.md`](REPORT.md) | the architecture review and the ranked open findings | the review still describes the prototype |
| [`findings/`](findings/) | one file per open finding (35), indexed in [`evidence/findings-table.md`](evidence/findings-table.md) | still open: the tagged cases still fail, and the others were re-run or spot-checked on 2026-09-29 |
| [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) | `KNOWN_FAILURES.tsv` grouped by tag | the prototype's fix list |
| [`evidence/03-fuzz/findings/`](evidence/03-fuzz/findings/) | minimized fuzz fixtures for F-265 and F-310 | open findings; `spec/tools/fuzz/README.md` points here |
| [`evidence/04-runtime/`](evidence/04-runtime/) | replay, edit, and panic result tables | back F-155, F-161, F-401, and F-404 |
| [`evidence/05-object-model/`](evidence/05-object-model/SUMMARY.md), [`05-requirements/`](evidence/05-requirements/SUMMARY.md), [`06-compiler/`](evidence/06-compiler/SUMMARY.md) | representation, cost, and compiler-structure measurements | back the architecture review and F-501 to F-610 |
| [`probes/`](probes/), [`scripts/`](scripts/), [`bench/`](bench/) | the inputs and scripts that reproduce those runs | needed to re-run the open findings |
| [`grammar/FINDINGS.md`](grammar/FINDINGS.md) | GR-10 item f, GR-21, and the ambiguity tool in [`grammar/tools/`](grammar/tools/) | open reference-parser and teaching findings |
| [`types/QUESTIONS.md`](types/QUESTIONS.md) | TQ-14 and the parked TQ-24 to TQ-26 | live owner decisions |
| [`types/FINDINGS.md`](types/FINDINGS.md), [`PROPOSED_RULES.md`](types/PROPOSED_RULES.md), [`RESEARCH.md`](types/RESEARCH.md) | the open type-rule findings, the draft rule text for them, and the language comparison behind them | back the open type findings |
| [`hd-writing-log.md`](hd-writing-log.md) | mistakes agents make writing hd code | the diagnostics and docs audit (AGENTS.md) |

The scripts write fresh WAT and timings when run. Only the WAT that an open
finding cites is kept.

## Applied Decisions the Prototype Does Not Follow Yet

Every other applied decision is implemented in the prototype; the spec's
Revision Notes in `spec/README.md` are the record.

| #  | Decision |
| -- | -------- |
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. The check and `Iterable` are implemented; one fixture needs package roles (`--package-role`, `--dependency`), which the prototype CLI lacks. |
| EMB-S | Rust-style trait lookup: a trait method is a candidate only where its trait is available, wherever the impl is declared; an unavailable trait is invisible, so a promoted method of that name is selected and a call that finds nothing is `unknown-method` suggesting the import. `member-lookup.ts` and `program-embedding.ts` implement the rest, but the prototype checks one module without trait imports (a multi-file package is linked into one namespace), so every trait is available, and the fixtures need package roles. |
| P2 | Member lookup skips own fields and inherent methods not visible from the calling module; a private member of an embedded type is never promoted, and `private-member` is reported only for an invisible own member when nothing visible matches. `member-lookup.ts` follows the algorithm, but the prototype checks one module (a linked package shares one namespace), so every own member is visible, and the fixtures need package roles. |
| GQ4 | `pack.map(` and `pack.map_list(` always form the pack operation, even beside a local named `pack`. The prototype checks the operation's argument shape but has no pack operations, so a valid use still resolves as a method call. |
| MHP-1 | A `println` call at the top level of a script is valid (the second round). The prototype infers no script entry requirement row (`module.init.script-row`), so `println-top-level-script.hd` reports `missing-requirement`. |
| DC7 | The top-level statements of an initialization group run in dependency order, then by module identity and source position. The package linker joins a group's modules by identity and cannot interleave their statements, so a read that needs a later-joined module's binding is `top-level-read-before-initialization`, which `init-group-order.hd`, a package-tree fixture, shows. |
| M29 | A `Self` line in a per-trait derivation block warns `unused-derivation-fact` when the fact's package does not supply the block's trait. The fixture needs a second package, and the prototype CLI has no package roles. The other M29 rules, and M27 and M28, are implemented. |

## Prototype Gaps No Fixture Reaches

[`src/README.md`](../src/README.md) describes the implemented surface and
its limits. These gaps are recorded only here:

- Shape intrinsics (K1): `shape[T]()` and `metadata[T]()` for a type
  parameter `T` report `unsupported-reified-shape`, since the prototype
  passes no runtime type descriptor. `ShapeMetadata` has no
  implementations, so a `T < ShapeMetadata` bound is never met.
  `SourcePosition.file` is empty, and a `DeclarationId` is a hash of the
  qualified name.
- Testing: only functions of a `tests:` block are hidden from code outside
  it. Test dependencies are not implemented. A panic outside
  `expect_panic` stops the run (F-403).
- Testing T8: a test body's `Result` reports only its outer tag, not its
  `.Ok` value's `ExitCode`.
- Testing T40: a trailing block binds the final parameter only for calls
  that the checker plans, not for the built-in functions it special-cases.
