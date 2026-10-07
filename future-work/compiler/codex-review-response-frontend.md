# New Compiler Design: Codex Review Response, Frontend Lane

Status: Response to the Codex review of 8bb6860d, frontend lane rows, 2026-10-07.

Part of the [compiler design](README.md). The backend lane's rows are in
[codex-review-response.md](codex-review-response.md). The review itself
is `codex_review.md`, which is not committed.

**How each row was checked.** Each finding was read against the current
frontend files (HEAD 85f5c683) and the spec, looking for a reason the
claim is false. The quoted sentence is the one at issue, as it stood
before this pass. Verdicts:

- `accepted-fixed`: true, and fixed in a frontend file;
- `partly`: true in part; the rest is explained or left to the backend;
- `needs-backend`: true, and the fix lands in a backend file;
- `answered in trait-solver.md`: answered in
  [trait-solver.md §16.5](trait-solver.md#165-codex-review) before this
  pass.

No finding was rejected.

## Rows

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| 3 | answered in trait-solver.md | [trait-solver.md §3.2](trait-solver.md#32-owner-modules), §5.3; [resolution-and-interfaces.md §4.12.1](resolution-and-interfaces.md#4121-owner-module-impl-tables-mine) | Owner lookup could not enumerate impls owned through an open trait argument. A per-run candidate directory, filtered by the dependency closure (owner, 2026-10-07), now covers it. |
| 4 | partly | [type-checking.md §1.7](type-checking.md#17-body-tasks-and-the-exactly-once-rule), §2.4 step 3; [resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction) | Quoted: "Any other becomes a getter that computes the value on first use and stores it in a global" (codegen.md §12.3), and "A missing argument with a default is filled by a `Default` instruction". True: [`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval) evaluates per call. The checker now emits a default call with the earlier arguments, after the explicit ones; the interface keeps only default presence and fact types. The default instruction and the compile-time fact evaluator are backend work. |
| 6 | accepted-fixed | [type-checking.md §9.1](type-checking.md#91-who-can-omit), §16 question 1; [resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction), §4.10.1; [syntax.md §4.5](syntax.md#45-header-extraction-and-the-api-text-hash); [trait-solver.md §3.10](trait-solver.md#310-derives-delegation-and-error) | Quoted: "So an omitted result never reaches a folder interface" (type-checking.md §9.1), next to the owner's "A derive template may call private helpers" (goals.md). True: a helper with an omitted result or row put a body into the interface. Proposed: a template-named helper follows the public signature rules, and its private type closure is hidden too. This is new language text, so it is open question 1. |
| 7 | accepted-fixed | [type-checking.md §5.1](type-checking.md#51-representation), [§5.5](type-checking.md#55-private-rows-and-the-m3-fixpoint), §17 items 1, 2, 6 | Quoted: "Start from the union of its `Uses` keys", and "M3 now writes one provider per key of the solved row". True on both counts. Now: one set per row variable and a monotone worklist, a substitution in each pending row, `PendingCall` records with the lexical providers, an appended provider range, and an M3 row sweep over `HAS_ROWVAR` types. The pool and patch changes are backend work. |
| 8 | answered in trait-solver.md | [trait-solver.md §7.1](trait-solver.md#71-memo-keys-and-eligibility), §7.3; [type-checking.md §1.6](type-checking.md#16-the-trait-solver-interface) | Memo keys now hold the environment, visible local impls and availability; depth is a stored height; exhaustion is never cached. Applied in type-checking.md §1.6 (change 2). |
| T1 | answered in trait-solver.md | [trait-solver.md §2.1](trait-solver.md#21-trait-references), §4.3; [type-checking.md §1.4](type-checking.md#14-the-type-accessor-api), §1.6 | Projections name the associated item and the instantiated trait reference; `Project` answers `Normalized`. Applied in type-checking.md (changes 1, 4). |
| T2 | accepted-fixed | [type-checking.md §3.5](type-checking.md#35-the-trail-and-the-one-rollback-contract), §2.7, §3.2, §13 | Quoted: "Nothing else in a body is mutable, and no checker state is ever cloned." False as stated: watch heads, obligation states, blame, literal-class data, scope maps and the error count were outside both classes, and `ob_next_watch` could not hold one obligation on several lists. Now every field is in one of four classes; watch lists are an edge table; `resolved` writes below the checkpoint are trailed; the verifier hashes contents and indices. |
| T3 | accepted-fixed | [type-checking.md §6.1](type-checking.md#61-arm-local-equalities), rules TC-10 and TC-11 | Quoted: "Step 7 needs a **scoped pop**: it undoes only `ArmEq` entries above the mark and keeps `Bind` entries." True: a binding made through an equality loses its reason, and a union adds no `Bind` entry. Now equalities are consulted, never written into types, and an outer variable may be bound only to a type free of the arm's refined parameters and existentials. Arms no longer roll back. Section 6.1 gives the soundness argument. |
| T4 | answered in trait-solver.md | [type-checking.md §2.5](type-checking.md#25-methods-and-operators) | Verified against [`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.fits.expected). Applied: each trial runs `coerce(result, want)` before fits are counted (change 9). |
| T5 | accepted-fixed | [type-checking.md §3.6](type-checking.md#36-literal-widths), §2.3, §2.6, §2.7 | Quoted: "At the end of each statement, every class still open takes its default". True: [`types.literal.local.form.closure-return.statements`](../../spec/lang/04-type-system.md#r-types.literal.local.form.closure-return.statements) joins return and break values across statements. Now a literal class held by an open join defaults at the join. Extending this to private functions with omitted results is reading 1 of type-checking.md §16.1. |
| T6 | answered in trait-solver.md | [trait-solver.md §7.6](trait-solver.md#76-bounded-is-not-linear), §5.2; [type-checking.md §2.5](type-checking.md#25-methods-and-operators), §2.7 | The solver and coherence parts were answered there. Applied here: the head prefilter, the per-site trial memo with its taint rule, and once-per-round waking (changes 7, 8, 11). |
| T7 | accepted-fixed | [resolution-and-interfaces.md §4.10.1](resolution-and-interfaces.md#4101-header-validation-stages); [trait-solver.md §3.7](trait-solver.md#37-supertraits), §5.1 | Quoted: "Run the header checks that need nothing else", followed by a list without impl member completeness, variance or bound well-formedness. True. Now every header rule is in stage A (headers), B (`HeaderCheck(F)`, the solver over frozen tables) or C (bodies), and stages A and B run for dependencies too. The `HeaderCheck` cache entry and task are backend work. |
| A5 | answered in trait-solver.md | [trait-solver.md §3.11](trait-solver.md#311-the-synthetic-impl-inventory) | Every synthetic impl family has a descriptor, an invalidation source, an overlap rule and an evidence form. Tuple templates join every tuple head in coherence. |
| P2 | needs-backend | none in frontend files; [type-checking.md §1.7](type-checking.md#17-body-tasks-and-the-exactly-once-rule) states the input | Quoted: "scripted one-function edit in a generated 10k-line package, then `hd check`; p50/p95 over 20 edits" with "p50 ≤ 50 ms" (goals.md, `edit-latency`). True that a private body edit rechecks and serializes the whole module, so the target depends on module size. Module size, serialization and publish time are cache and goals concerns. The frontend's input: an M2 body result depends only on its body and the module's frozen inputs, so per-body reuse is possible later. |
| P6 | partly | [type-checking.md §9.1](type-checking.md#91-who-can-omit), §1.7 | Quoted: "The second row is the `recheck-precision` target: one module." (design-overview.md §1.4). The review's exceptions were real. Two are gone on the frontend side: a helper's body edit no longer changes any interface (finding 6), and a check reads only a fact's type. What counts toward the target, and init order work, are design-overview.md and goals.md matters. |

## Other Notes From The Backend Lane, Applied

| Note | Where |
| --- | --- |
| A7: suggestions search only inputs the cache key covers | [type-checking.md §10.5](type-checking.md#105-fix-its-for-common-mistakes) |
| I2: when the checker reserves a TIR slot | [type-checking.md §1.5](type-checking.md#15-what-the-checker-needs-from-the-tir-builder), four cases |
| A3: interface and coherence spans relative to a declaration | [resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction), §4.11.1, §4.12.3 |
| Data structures: `Rigid` in `TyView`, one local table, no `local_pool` in the checker's checkpoint, `Mut`, constants as `Ref`s | [type-checking.md §1.4](type-checking.md#14-the-type-accessor-api), §1.5, §3.5, §8.3, §13. The checker never needs a constant's TIR span: literal diagnostics come from its own literal table. |

## Trait-Solver Changes Applied

[trait-solver.md §16.4](trait-solver.md#164-changes-needed-in-type-checkingmd-and-the-other-design-files)
changes 1 to 11 are applied in type-checking.md (§1.4, §1.6, §2.5, §2.7,
§6.2, §11.1), and changes 12 to 14 in resolution-and-interfaces.md
(§4.10 step 5, §4.12.1, §4.12.3).

## Changes For The Backend Lane

From trait-solver.md §16.4, not made here:

15. **data-structures.md §3.4 and §3.9.2:** the projection type is
    `Assoc { assoc: DefId, tref }`, with the trait's arguments, not
    `{ base, trait_, name }`.
16. **checking-and-tir.md, catalog:** the `TraitMethod` choice gains
    `TraitValue` and `Builtin`; `CallDyn` gains one evidence operand per
    method-level bound; `NewVariant`'s evidence choices are `Evidence`
    values.
17. **checking-and-tir.md §4.13.9:** a derive instance needs no
    coinductive assumption. Supertraits, supertrait bindings, delegation
    parts and newtype bases are checked by the folder's `HeaderCheck(F)`
    task (resolution-and-interfaces.md §4.10.1); scheduler.md gains that
    task, and cache.md its entry.
18. **codegen.md §13.2 step 4:** select by head only, with no depth limit
    and no fuel; a failure is an internal error (rule TS-6).
19. **codegen.md §13.5:** vtables follow the trait record's shape, with
    supertrait vtables by pointer; a slot for a method with method-level
    parameters is the erased instance with one vtable parameter per bound.
20. **codegen.md §13.6:** tuple `Eq`, `Ord`, `Hash` and `Debug` are
    tuple-template instances, instantiated per tuple type like any impl.
21. **cache.md, the `check` key,** gains `arg_impls_closure_hash`, a
    Merkle hash over the `arg_impls` section hashes of the module's
    dependency closure. The solver memo is never persisted.

From this pass, also listed in
[type-checking.md §17](type-checking.md#17-changes-needed-in-compilerdesignmd):

22. **Defaults (finding 4).** Replace `Default` with a default call: the
    default body's `DefId`, the callee's type arguments and the earlier
    argument values, emitted per call. Replace the first-use getter of
    codegen.md §12.3. Facts, metadata and shared enum data need a
    compile-time evaluator with its own budget and its own diagnostic.
23. **Rows (finding 7).** A `Row` whose pending part names a
    module-scoped `RowVar` may live outside the body-local pool until
    `ModuleFinish` ends. M3 may rewrite `HAS_ROWVAR` types in the `ty`
    column and in records, and patches each pending call's provider word
    to point at an appended range.
24. **GADT arms (T3).** A static `Refine` coercion kind; no scoped pop in
    checking-and-tir.md §4.13.6.
25. **Open literals (I7).** `konst` accepts a literal whose width is still
    an inference variable; the width comes from the `Solution`.
26. **Header checks (T7).** A `HeaderCheck(F)` task and cache entry,
    keyed by F's interface key and its dependencies' deep and `arg_impls`
    hashes; codegen waits for every such task.

## New Open Questions

One, in [type-checking.md §16](type-checking.md#16-open-questions-for-the-owner):

1. **Signatures of hidden template helpers.** Recommendation: a private
   item that a template body names follows the public signature rules: a
   written result type, an omitted `$` clause meaning the empty row, and
   no top-level bindings. It uses existing codes and keeps interfaces
   syntax-only.

Three readings to confirm are in
[type-checking.md §16.1](type-checking.md#161-readings-of-the-spec-to-confirm):
literal joins in private functions with omitted results, the GADT rule
for outer variables, and `is` on results.
