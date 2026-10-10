# Missing-Errors Triage (M8, 2026-10-10)

hd commit `b67d71a8`. Scope: invalid conformance fixtures that fail —
today **130 silent** (`fail:no-diagnostic`) **+ 134 wrong-code invalid =
264**, plus 25 valid-but-failing cases in the same buckets. (The gap
audit's 282 = 130 + 152 moved as buckets shifted.) Method: the M4
[triage](triage-2026-10-10.md) groups (minimal program + rules each)
mapped to the crate, module and function where the check belongs, with a
size (S <1 day, M several days, L week+) and a soundness verdict.
Soundness = yes only if invalid code can reach codegen and miscompile or
run past a must-trap; wrong-code rejections of already-invalid code are
UX. Small (1–2 case) buckets are tabled in §6, ungrouped.

Task-list naming: only #204 is safely mappable from the audit text
(no-common-type joins); other `#203–#214, #224, #232, #273–#275` numbers
cannot be resolved from the tree (no such issues exist upstream), so
groups carry descriptive names plus the M4 group IDs for merging.

## Soundness-relevant first (per the gap audit)

| Group (M4 ID) | N | Where the check belongs | Size | Why sound |
| --- | --- | --- | --- | --- |
| Embedding checks absent (A-embed) | 14 | `hd_check::conflicts::promotion_conflicts` (extend; graph cycle/depth walk) + `hd_resolve::lower::Lower::fields` + `hd_check::update`/body assignment | M | dup/cycle breaks layout invariants codegen trusts (codes exist in `hd_diag`, zero emits) |
| Duplicate declarations accepted (A-dup) | 5 | `hd_resolve::lower::Lower::item` (mirror existing `duplicate_members`) + literal-dup in `hd_check::body/expr` | S | dup tag/field index silently picks one; backend trusts uniqueness |
| Trait/impl duplication (C-redeclare) | 8 | `hd_resolve::lower::{duplicate_members, supers}` (assoc kinds, supertrait closure, dup check) + `hd_driver` ModuleFinish extra members + `hd_resolve::header::Universe::overlaps` | M | silent resolution/impl-choice change → wrong method runs |
| Sealed-trait enforcement (C-sealed) | 3 | `hd_resolve::lower::Lower::impl_item` + `hd_check::conform` (name-set gate; neighbor sealed check at lower.rs:2483) | S | user impl of compiler-supplied trait breaks closed-world solver assumption |
| Implied bounds (C-bounds) | 6 | `hd_check::header::ItemCheck::check_ty` (extend goal sites; header.rs:485) | S | unchecked bound → missing impl at monomorphization (trap or miscompile) |
| Resolution/ownership (C-resolve) | 9 | `hd_resolve::lower::{misplaced_impl, ownership}` + `hd_types::solver` (fuel/ambiguity) + `hd_check::{header, trial, local, fnref}` | M | orphan/nonlocal/ambiguous let codegen assume forbidden resolutions |
| Type-system singles (E, partial) | 10 | scattered `hd_check::{literals, body, header::check_ty, trial}` + `hd_resolve::lower` | M (batch of S) | variance/map-key/bound/`never` break backend-trusted invariants; literal-range/tuple-rest no |
| Requirement checks (G-req) | 6 | `hd_check::rows` + `hd_resolve::lower::Lower::row` + `hd_check::call` (default-value case missing at call.rs:1419) | M | forbidden-context/inspectable-key let runtime assume a context it lacks |
| Overloading + dup bindings (I-names) | 2 | `hd_resolve::lower::{build_folder, scope_of}` (`or_insert` keeps first silently) + `hd_check::local` | S | second decl dead, calls hit the first → wrong function runs |
| `block_on` in `defer` never panics (RE-2) | 1 | `hd_wasm::emit.rs::Em::scope` (wrap suite execution in `forbid(1/-1)`; checker already accepts indirect) | S | exits 0 past a must-trap (`flow.defer.block-on.indirect`) |
| Representation variance hole (T3, caveat) | 3 | `hd_check::access::perm_fits` + `body::coerce` over `hd_resolve::variance::walk` | M | fixtures reject today, but if such a coercion were ever accepted silently it would miscompile |
| Huge-literal float fallback (T8, caveat) | 1 | `hd_check::body::check_literal_ranges` + `hd_syntax::lexer::number` | S | review the fallback for silent-miscompile risk |

## Wrong-code groups (reject, wrong or duplicate code; UX only)

| Group (M4 ID) | N | Where | Size |
| --- | --- | --- | --- |
| Failed joins → `no-common-type` (T1) | 11 | `hd_check::body::Ck::coerce/expect` + `rows::join_site/join_target`, applied at ~6 LCT sites | M |
| One `identity-requires-references` per `is` (ID-1) | 9 | `hd_check::expr::binary_values` KwIs loop (expr.rs:638-646) | S |
| Parse fallout suppression (SE-1) | 9 | `hd_syntax::parser` recovery (`parser/mod.rs`, `stmts.rs`, `exprs.rs`) | M |
| Row/type-kind → `generic-kind-mismatch` (U1/U2/UT2) | 6 | `hd_resolve::lower` kind checks + `hd_check::ty` | M |
| Paired result/operand collapse (T5) | 5 | `hd_check::body::coerce/expect` × `expr::binary_values` | S |
| Contextual `.None` (C1+T6) | 6 | `hd_check::call::variant_value` (sole `MissingContextualEnumType` emit) + `body::coerce` | S |
| Arity-vs-name ordering (A2/A3) | 4 | `hd_check::call::check_args` + `variant_by_name` + `init::shared_args` | S |
| Signedness pairs → `mixed-signedness` (T2) | 4 | `hd_check::expr::binary_values` + `range_expr` (code exists, zero emits) | S |
| Supertrait check ordering (MS1-3) | 3 | `hd_check::header::supertraits` vs `hd_resolve::lower` derive/sealed sites | S |
| Bare-variant cascades | 3 | `hd_check::pat::pattern` (+ poison bare arm) | S |
| Representation variance code (T3) | 3 | as soundness row above | M |
| Later-param defaults (N2) | 2 | fn-param default scope (`lower.rs`, `body::BodyKind::Default`) | S |
| Non-`Num` `sum` bound (M4) | 2 | `hd_check::call::resolve_method` (keep hidden candidate, report bound) | M |
| Derived-bound at use (T4) | 2 | `hd_check::body::check_value_bindings/require_ref_as` (code exists, zero emits) | S |
| `it` outside test position (N6) | 2 | name resolution special-case → `misplaced-test-case` | S |
| Field-vs-method codes (D1/D5) | 2 | `hd_check::expr::field_expr` branches | S |
| `requirement-in-default` code (MR-1) | 2 | `hd_check::rows::require_key` (code exists, zero emits) | S |
| Suspend-as-trait + cascades (UT1/UT3) | 2 | trait-path resolution + `Known::sealed`; error-type suppression | M |
| Placeholder ordering (T7) | 1 | `hd_check::call::check_args` vs `expr::placeholder_value` | S |
| Huge-literal range (T8) | 1 | `check_literal_ranges` before float fallback | S |
| `fact::[D]()` inference (C2) | 1 | `hd_check::facts/trial` + call path (blocked on Collect Structure, C-C1) | M |
| Pattern-field code (N5) | 1 | `hd_check::pat` payload-field lookup | S |
| `test-only-use` code (MR-2) | 1 | `hd_resolve::lower` TestOnly path vs `rows::require_key` | S |
| `1..=` location (SE-2) | 1 | parser position (or fixture marker — audit decision) | S |

Zero-emit codes found (new branch, not retarget):
`MixedSignedness`, `MissingDerivedBound`, `NoCommonType`,
`RequirementInDefault`, plus the ordering-only suppressions.

## Silent non-soundness groups (accept, but no miscompile path)

Typed facts (13, `hd_check::facts`, M — metadata only, Collect stalls);
annotation blocks (16, `hd_resolve::lower` + `hd_structure`, M);
private-type leaks (6, new `header::signature_privacy`, M — encapsulation
only); entry-point checks (5, `hd_driver` entry validation, S — loud
startup failure); module/test-layout (9, scattered project/resolve/check,
M); expression checks (9, M — defined runtime behavior);
flow unreachables (5, S — lints); fn well-formedness (2, S);
missing warns (2, S). One fix covers several: `header::check_ty`
(C-bounds + E bound/map-key/default + D-entry Termination goals);
`Lower::{item, duplicate_members, fields, supers}` (A-dup + C-redeclare +
A-embed local parts); `conflicts::promotion_conflicts` (A-embed graph);
`{ownership, misplaced_impl}` (C-resolve ownership).

## Valid-but-failing + runtime text (completeness, not soundness)

Provider destructuring ×2, `fn!` weakening, optional wrap, intrinsic-method
inference + one-fit literal (share `call::trait_call_args` — fix together,
S–M), TestRunner property bodies, `Suspend.cancel` M (checker + seed +
emit; unblocks MR-3), closure self-reference S, `0O` octal S (lexer
already accepts; checker `split_suffix` doesn't), raw identifiers S
(one `ident_text` helper, ~15 sites), `let`-pattern `mut` S, defaulted
type-arg fill S, trait-arg-owned operators S, view panic messages S
(std `panic_with` messages), stack-overflow mapping S (Node host mirror;
wasmtime side done).

## Small buckets (1–2 cases, ungrouped)

`fail:bang-call-outside-suspension` (2), `fail:invalid-token` (2),
`fail:missing-required-field` (2), `fail:tab-whitespace` (2), plus 20
singles (M4 §8). Spot pattern: nearly all are second-error cascades on
already-rejected code — same suppression family as the S cascade rows
above (T5/A2/bare-variant/UT3). Proposed handling: one cascade-suppression
task (S, `hd_check::body::coerce/expect` + `call::check_args` +
`pat::pattern`) sweeps most; the remainder are single S fixes each.

## Largest tasks by programs affected

| Task | N | Size | Sound |
| --- | --- | --- | --- |
| Silent-check batches (embed, dup, redeclare, bounds, resolve, entry, layout, typesys, expr, req) | ~100 | M batches of S | mostly yes |
| Typed-fact + annotation-block validation | 29 | M | no |
| Body surface (`!::[T]`, mixed payloads, `...=`, inference-pending) — Collect/Body, separate lane | ~29 | M | n/a (unsupported, not wrong errors) |
| Cascade suppression (one-error rule) | ~25 | S | no |
| Joins → `no-common-type` (+ #204) | 11 | M | no |
| `Suspend.cancel` end to end | 10 | M | no |
| Identity `is` dedup | 9 | S | no |
| Parse fallout | 9 | M | no |
| Row/type-kind mismatch | 6 | M | no |
| Contextual `.None` | 6 | S | no |
