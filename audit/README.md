# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the grammar and type
audits that followed, are finished. Everything they found that has since been
fixed, applied to the specification, superseded, or dropped has been removed
from this folder. The spec's Revision Notes in
[`spec/README.md`](../spec/README.md#revision-notes) record the applied
decisions, and the repository history keeps the removed evidence.

## Conformance

On 2026-10-01 the conformance suite has 1,767 cases, and
`test/portable/cases.tsv` selects the 1,695 that the prototype passes. The
other 72 are listed in `test/portable/KNOWN_FAILURES.tsv`, each tagged with a
finding or with a decision below. Every case is in one of the two files. By
[tier](../spec/conformance/README.md#tiers):

| Tier | Cases | Selected | Known failures |
| --- | ---: | ---: | ---: |
| language | 1,693 | 1,628 | 65 |
| stdlib | 74 | 67 | 7 |

[`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) counts
them by tag:

| Tag | Cases | Why they fail |
| --- | ----- | ------------- |
| F-250 | 5 | GADT variant results and package roles give generic diagnostics |
| EMB-S | 4 | trait availability needs package roles |
| P2 | 5 | member visibility needs package roles |
| TQ-2 | 1 | package roles |
| M29 | 1 | the unused-fact warning needs a second package |
| F-259 | 1 | the `disposed-file` runtime profile does not exist |
| DC7 | 1 | group statements are not interleaved across modules |
| MHP-1 | 1 | no inferred script entry requirement row |
| INF-mut | 3 | batch 17: generic inference widens numbers, reports a trait-value conflict as `type-mismatch`, and `assert_equal` keeps its special case |
| BFF | 5 | D1's callable values, D2, and D4: the prototype does not know `Apply` or `Update`, so `v() = x` is `invalid-assignment-target` and `impl Apply` is `unknown-trait` |
| IT | 2 | batch 24, IT2: `lib/std/iter.hd` still implements `Iterable` for `Iterator`, so an iterator satisfies an `Iterable` bound |
| ST8-self | 1 | batch 25: the prototype resolves a receiverless template call only as `T::name()`, so `Structure::name()` is `unknown-type` |
| AT-gen | 2 | batch 26: derived `Arbitrary` gives a member's type parameter no `Inspectable` bound |
| ST8-own | 1 | batch 28: `Named::name()` in `Named`'s template is `associated-function-needs-target` |
| Q6 | 2 | batch 31: tuples have no `Hash`, so a tuple map key fails `Map`'s `K < Eq & Hash` bound (task #142) |
| Q6-others | 1 | batch 33: interpolation finds `Display` only for an exact target type, so `lib/std`'s generic tuple `Display` is unused |
| TR-traits | 3 | batch 34: rest tuples have no `Eq` or `PartialOrd`, and `lib/std` declares no rest tuple `Debug` or `Display` |
| O7 | 1 | batch 36: `arbitrary.with` still erases its generator, so a mismatched generator panics at run time |
| ANNOTATE-TYPED | 14 | batches 39 and 40: no typed facts: `annotate` takes no type argument, so `@annotate::[F]` is `argument-count`, `Field` has no `fact`, a bare generic decorator gets no expected type, and a `fn!` target is not `type-mismatch` |
| LITERAL-MARKERS | 8 | batch 39: the literal-function shape is still checked at the `fn` line, and a parameter outside `Num` is `type-mismatch` |
| O3b | 5 | batch 36: no tuple `Structure` or tuple templates, so tuple traits stop at 12 elements and a tuple template derives nothing |
| FX-let-mut | 1 | a fixture defect: `typing/valid/let-data-pattern.hd` writes `let mut` on an `i32`, which is `mut-on-primitive`; its `let` patterns pass without the `mut` |
| FX-shape | 1 | a fixture defect: `typing/valid/let-else-diverging-forms.hd` names a loop binding `shape`, a prelude intrinsic, so it is `prelude-name-shadow` |
| FX-push | 2 | a fixture defect: the `let` pattern `mut` fixtures call `List.push`, which neither `spec/std` nor `lib/std` declares; with `append` they pass |
| FX-map-index | 1 | a fixture defect: `runtime/valid/derived-newtype.hd` compares `m[k]` with `.Some(...)`, but `m[k]` now reads `V`; it should call `m.get(k)` |

## What Remains

| Path | What it holds | Why it stays |
| ---- | ------------- | ------------ |
| [`REPORT.md`](REPORT.md) | the architecture review and the ranked open findings | the review still describes the prototype |
| [`findings/`](findings/) | one file per open finding (33), indexed in [`evidence/findings-table.md`](evidence/findings-table.md) | still open: the tagged cases still fail, and the others were re-run or spot-checked on 2026-09-29 |
| [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) | `KNOWN_FAILURES.tsv` grouped by tag | the prototype's fix list |
| [`evidence/03-fuzz/findings/`](evidence/03-fuzz/findings/) | minimized fuzz fixtures for F-265 and F-310 | open findings; `spec/tools/fuzz/README.md` points here |
| [`evidence/04-runtime/`](evidence/04-runtime/) | replay, edit, and panic result tables | back F-155, F-161, and F-401; `roundtrip.tsv` stays as the history of F-404, closed with F-402 on 2026-09-30 because both described only the removed `hd replay` and `hd record` commands |
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
| MHP-1 | A `println` call at the top level of a script is valid (the second round). The prototype infers no script entry requirement row (`module.init.script-row`), so `println-top-level-script.hd` reports `missing-requirement`. |
| DC7 | The top-level statements of an initialization group run in dependency order, then by module identity and source position. The package linker joins a group's modules by identity and cannot interleave their statements, so a read that needs a later-joined module's binding is `top-level-read-before-initialization`, which `init-group-order.hd`, a package-tree fixture, shows. |
| M29 | A `Self` line in a per-trait derivation block warns `unused-derivation-fact` when the fact's package does not supply the block's trait. The fixture needs a second package, and the prototype CLI has no package roles. The other M29 rules, and M27 and M28, are implemented. |
| BFF | D1 (its callable-value part), D2, and D4: `v()` on a value whose type implements `std.ops.Apply` calls `apply`, and with `Update`, `v() = x` and `v() op= x` store through `update`. The prototype and `lib/std/ops.hd` declare neither trait, and every call target is `invalid-assignment-target`, so `callable-value-no-update.hd` passes without the traits. |
| IT | Batch 24, IT2: `Iterator[T]` does not implement `Iterable[T]`, so an `I < Iterable` bound rejects an iterator, readonly or mutable, and `for` takes a mutable iterator directly. `lib/std/iter.hd` still declares `impl[T] Iterable[T] for Iterator[T]`, so the prototype accepts both bound fixtures. Its `for` over a mutable iterator already works, through that impl. |
| ST8-self | Batch 25: inside a template, a `Structure::` call has the template's `T` as its `Self`, so `Structure::name()` and `Structure::facts()` are valid. The prototype resolves only `T::name()` and `T::facts()`, and reports `Structure::name()` as `unknown-type`. |

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
- Map implementations (batch 15, Q-map): `lib/std/iter.hd` writes
  `impl[K, V] Iterable[(K, V)] for Map[K, V]` without `K < Eq & Hash`,
  which a std-only exception in the checker allows. The spec's std writes
  the bound and has no such exception.
- Closures: a suspending closure in a generic function reaches no bound
  dictionary, so its body cannot call a method through the enclosing
  function's bounds. A non-suspending closure can.
- Strings: `slice` copies its bytes instead of sharing them, so it takes
  linear time (`module.string.slice`, `.slice.shared`), because the
  runtime `string` is a bare byte array.
- Derived `Arbitrary` is generated by the checker
  (`checker/derive-arbitrary.ts`), not through a `std.testing` template:
  the derivation pass runs before std is joined, and the simplest variant
  needs the member types, which `Structure` does not give a template.
  `Choices.int` draws through `i64`, so a `u64` above the largest `i64`
  is never drawn. The runner prints no panic message, so the messages
  that name a type with no finite value or a mismatched `arbitrary.with`
  member are not shown.
