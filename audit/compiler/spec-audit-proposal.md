# Spec Audit Proposals (M7, 2026-10-10)

One entry per row of `audit/compiler/diagnostic-notes.md` (85 rows found,
not ~35; the file grew), grouped by spec chapter. Each entry names the
rules and fixtures involved, the options, and a recommendation with its
reason. Nothing here changes the spec or fixtures; the owner picks.
"Today" behavior is quoted from the notes row.

## Lexical structure and grammar (01, 02)

### CRLF inside multiline strings
- **Notes row:** `runtime/valid/crlf-line-endings.hd` (2026-10-09)
- **Rules / fixtures:** `lex.multiline.line-feed` (U+000A alone);
  fixture `runtime/valid/crlf-line-endings.hd` (expects `"first\nsecond"`)
- **Options:** (a) Fix lexer/lowering to normalize CRLF. (b) Change the
  rule to keep `\r\n`. (c) Change the fixture to `"first\r\nsecond"`.
- **Recommendation:** (a) — the rule already wants U+000A alone; this is
  a lowering bug, route to implementation.

### `trailing-block-position` message is just the code
- **Notes row:** `trailing-block-position` (and `syntax-error`) (2026-10-08)
- **Rules / fixtures:** `grammar.call.trailing-block.not-header`;
  `parse/invalid/trailing-block-in-brackets.hd`,
  `two-trailing-blocks.hd`, `colon-line-after-if-suite.hd`
- **Options:** (a) Keep code-only. (b) Add message guidance (`:` in
  parens starts a trailing block; use `name=value`). (c) A dedicated
  hint diagnostic.
- **Recommendation:** (b) — message-only, no grammar change, fixes a
  predictable `:`/`=` confusion.

### `range-inclusive-no-end.hd` marker off by one line
- **Notes row:** Fixture `parse/invalid/range-inclusive-no-end.hd`
  (2026-10-10, M4 triage)
- **Rules / fixtures:** `grammar.expr.range.inclusive-needs-end`;
  fixture marker L4, error at L5:1
- **Options:** (a) Move the marker to the error line. (b) Locate the
  error at the `..=` token. (c) Position-tolerance note for parse errors.
- **Recommendation:** (b) if the parser can locate `..=`, else (a) — but
  one ruling should cover all parse fixtures, not just this one.

### Frozen-spec change for `trailing-block-ineligible`
- **Notes row:** Spec change, against the freeze (60d16179) (2026-10-09)
- **Rules / fixtures:** `grammar.call.trailing-block.eligible`; README
  code list; `typing/invalid/trailing-block-ineligible.hd`, `cases.tsv`,
  `hd_diag` codes
- **Options:** (a) Ratify (owner already directed leaving it in place;
  compiler adds the diagnostic). (b) Revert + drop fixture.
  (c) Keep spec, mark fixture aspirational.
- **Recommendation:** (a) — only the compiler side remains.

### Positional after spread at a vararg
- **Notes row:** `typing/invalid/nonfinal-spread.hd` against
  `positional-spread-duplicates-vararg.hd` (2026-10-09)
- **Rules / fixtures:** `expr.call.spread.position`
  (`nonfinal-positional-spread`) vs `expr.call.spread.vararg-duplicate`
  (`duplicate-argument`); both fixtures
- **Options:** (a) `vararg-duplicate` priority (today).
  (b) `position` priority. (c) Both errors.
- **Recommendation:** (a) — the vararg is genuinely supplied twice and
  the spec already carves this shape out.

### `positional-after-spread.hd` declares nothing (fixture)
- **Notes row:** `typing/invalid/positional-after-spread.hd` (2026-10-09)
- **Rules / fixtures:** `expr.call.spread.position`; the fixture body is
  `sum(values..., 3)` with no declarations (already logged; verified in
  M4 that declared names reach the expected error)
- **Options:** (a) Fix the fixture with declarations. (b) Keep failing.
- **Recommendation:** (a) — one-line fixture fix, no spec change.

## Names, scopes and methods (03)

### `ambiguous-method` hint shows generic form
- **Notes row:** `ambiguous-method` hint (2026-10-08, was #75)
- **Rules / fixtures:** `names.method-lookup.ambiguous` (Note suggests
  `Trait::name(x, ...)`); today `Convert::[..]::convert(..)`
- **Options:** (a) Accept the generic form. (b) Require as-written
  arguments (`Convert::[i32]::convert(c)`). (c) Drop the Note.
- **Recommendation:** (b) — strictly more actionable, message-only.

### Tied promoted candidates
- **Notes row:** Promoted methods
  (`names.method-lookup.promoted-candidate`) (2026-10-09)
- **Rules / fixtures:** `names.method-lookup.promoted-candidate`;
  `runtime/valid/depth-two-promotion.hd`; planned `#42`
- **Options:** (a) `ambiguous-method` on equal-depth ties. (b) Deterministic
  tie-break. (c) Reject such hierarchies at declaration.
- **Recommendation:** (a) — matches planned behavior, parallels ordinary
  ambiguity.

### Test/task paths shown by file
- **Notes row:** Module paths of test and task code in messages (2026-10-08)
- **Rules / fixtures:** resolver internals (`shop.$tests.common`) vs
  `hd_project::user_text` boundary; `use super.up` reads "no module named
  `tests`"; `TypeId.of` test/task names untested
- **Options:** (a) File-form at the boundary; quote use paths as written.
  (b) Expose internal paths. (c) Map resolver messages and `TypeId` names.
- **Recommendation:** (a) now, (c) for `TypeId` later — `TypeId.of` is
  unsupported, so defer it.

## Type system (04)

### Union rows print without element order
- **Notes row:** Union rows in messages
  (`req.row.union.literal.diagnostics`) (2026-10-09)
- **Rules / fixtures:** `req.row.union.literal.diagnostics(.expanded)`;
  rows stored as sorted key sets (#48)
- **Options:** (a) Record element order at the join. (b) Specify
  content-order printing of sorted storage. (c) Drop the requirement.
- **Recommendation:** (b) short-term, (a) long-term — (b) describes today.

### Unsolved parameter as expected type
- **Notes row:** A join site whose expected type is an unsolved generic
  parameter (2026-10-09)
- **Rules / fixtures:** `types.lct.sites` ("when no expected type is
  available"); `keep(if admin: orders else: health)` (#48)
- **Options:** (a) Unsolved parameter = no expected type (join, then
  solve; today). (b) It fixes the parameter from the first branch.
  (c) Inference error.
- **Recommendation:** (a) — (b) would under-supply providers.

### `dyn` conversion without the impl
- **Notes row:** A value converted to a trait value whose type lacks the
  impl (2026-10-09, #30)
- **Rules / fixtures:** `trait.dyn.convert` (names no code);
  fixtures expect `type-mismatch` ("usize does not implement Gauge")
- **Options:** (a) Fix `type-mismatch` with "does not implement".
  (b) `unsatisfied-trait-bound`. (c) New `missing-implementation` code.
- **Recommendation:** (a) — fixtures already expect it; the message
  carries the fact.

### Polymorphic recursion via reference-layout parameter
- **Notes row:** Polymorphic recursion through a reference-layout
  parameter (2026-10-09, O25)
- **Rules / fixtures:** `types.generic.polymorphic-recursion` ("always
  an error"); `wrap([value], n - 1)` builds (REF-keyed, finite),
  tuple-pinned `T` stops
- **Options:** (a) Error only when the instance set is infinite.
  (b) Reject the REF-keyed case too. (c) Exempt move-only unbounded
  parameters.
- **Recommendation:** (a) — matches the finite build.

### `pub` on a local declaration
- **Notes row:** `pub` on a local declaration (2026-10-09, #172)
- **Rules / fixtures:** `names.local.no-pub`, `fn.local.private`
  (forbid, name no code); no fixture; compiler lifts silently
- **Options:** (a) Name a code + fixture. (b) Specify silent ignore.
  (c) Warning.
- **Recommendation:** (a) — silent acceptance contradicts two rules.

### Short-binding store plus literal fallback (fixture)
- **Notes row:**
  `typing/invalid/callable-value-short-binding-store.hd` (#173d)
  (2026-10-09)
- **Rules / fixtures:** `expr.call.apply.readonly` (`readonly-root`),
  `types.literal.local.default` (`usize` fallback); `view := live(0)`
  yields both errors
- **Options:** (a) Fix the fixture (`live(+0)`/annotation) to isolate
  `readonly-root`. (b) Suppress the second error. (c) Change fallback.
- **Recommendation:** (a) — both errors follow from independent rules.

### `f32` literal double rounding
- **Notes row:** An `f32` float literal (2026-10-09, #181a)
- **Rules / fixtures:** `types.literal.*`; checker stores `f64` bits
  (decimal→`f64`→`f32`); differs within 2^-53 of an `f32` midpoint
- **Options:** (a) Document double rounding (today). (b) Require single
  decimal→`f32`. (c) Forbid `f32`-context literals.
- **Recommendation:** (b) long-term, (a) now — (b) is the only
  correctly-rounded behavior.

### Printable `TypeId` name of `void`
- **Notes row:** Printable `TypeId` name of `void` (2026-10-09, #235)
- **Rules / fixtures:** `trait.typeid.stable` (one name);
  `trait.typeid.name.prelude` (`void`) vs `trait.typeid.name.tuple`
  (`()`); no fixture prints it; today `()`
- **Options:** (a) Prelude spelling `void`. (b) Canonical `()`.
  (c) Context-dependent.
- **Recommendation:** (b) — matches the mismatch messages; clarify both
  rules to the canonical form.

### Messages naming `void`
- **Notes row:** Messages that name `void` (2026-10-09, #235)
- **Rules / fixtures:** `expected i32, found ()` where source wrote
  `-> void`; `void` and `()` are one type
- **Options:** (a) Print as written. (b) Always canonical `()`.
  (c) Always `void`.
- **Recommendation:** (b) — one type, one spelling; (a) needs
  source-form tracking for little gain.

### `literal-var-method-missing.hd` second error (fixture)
- **Notes row:** `typing/invalid/literal-var-method-missing.hd`
  (2026-10-09, #30)
- **Rules / fixtures:** `types.literal.local.statement` (`usize` after
  the statement); `let count: u32 = i` adds `type-mismatch` after the
  intended `unknown-method`
- **Options:** (a) Fix the fixture to isolate one error. (b) Keep failing.
- **Recommendation:** (a) — already logged; fixture written for
  cross-statement width flow.

## Expressions (05)

### Copy-update field-read timing
- **Notes row:** Copy-update `D { ...src, f: e }` (2026-10-09, #122)
- **Rules / fixtures:** `expr.update.spread-first` (evaluation order,
  not read timing); mutation through another reference is observable
- **Options:** (a) Construction-time reads (today). (b) Spread-time
  snapshot. (c) Implementation-defined.
- **Recommendation:** (a) or (b), but decide — audit should force the
  choice; lean (b), matching "spread first" intuition.

### Calling `Fn[Args, O, $ R]` with separate arguments
- **Notes row:** A call through `f: Fn[Args, O, $ R]` (2026-10-09, #197)
- **Rules / fixtures:** `expr.call.spread.inputs`; arity unknowable in
  the body; today `type-mismatch` ("one spread")
- **Options:** (a) Name `type-mismatch`. (b) Repack against tuple arity.
  (c) Leave unnamed.
- **Recommendation:** (a) — the message names the one legal form.

### Tuple index past the end
- **Notes row:** `typing/invalid/tuple-index-out-of-range.hd` (2026-10-09,
  #187)
- **Rules / fixtures:** `value._1` on `(i32,)`; no rule gives tuple
  selection `unknown-method`; checker reports `unknown-data-field`
- **Options:** (a) Keep checker + amend fixture. (b) Add a rule naming
  `unknown-method`. (c) Dedicated `tuple-index-out-of-range` code.
- **Recommendation:** (a) — every other missing member is
  `unknown-data-field`.

### Empty tuple against rest-only tuple type
- **Notes row:** `expr.tuple.rest.collect` on `()` (2026-10-09, #173c)
- **Rules / fixtures:** `expr.tuple.rest.collect`; `()` accepted as empty
  rest against `(List[i32]...,)`
- **Options:** (a) Accept (today). (b) Reject except against `()`.
  (c) Leave unspecified.
- **Recommendation:** (a) — "fill fixed, collect the rest" read naturally.

### `**` on a type-parameter base
- **Notes row:** `**` on a base of a type parameter (2026-10-09)
- **Rules / fixtures:** `expr.power.*` names int/float bases only; today
  `Integer`/`Float` bounds resolve, `Num`-only/unbounded is
  `type-mismatch`
- **Options:** (a) Resolve by bound (today). (b) Reject all parameter
  bases. (c) Defer to instantiation.
- **Recommendation:** (a) — the exponent rule is otherwise unchoosable.

### Mixed numeric types in `**`
- **Notes row:** `typing/invalid/power-mixed-numeric-types.hd`
  (2026-10-09)
- **Rules / fixtures:** `2 ** 2.0` in `-> f64`; rejected `**` is an error
  value, so one `type-mismatch` on the exponent
- **Options:** (a) Keep single exponent error. (b) Report both faults.
  (c) Multi-error cases.
- **Recommendation:** (a) — satisfies the one-error case.

### `x is x` reported twice
- **Notes row:** `identity-requires-references` on `x is x` (2026-10-08)
- **Rules / fixtures:** `expr.is.*` ("using `is` … is an error",
  singular); 9 fixtures
- **Options:** (a) Keep per-operand. (b) Single error per `is`
  expression. (c) Per operand only when kinds differ.
- **Recommendation:** (b) — one mistake, one error; clarification + fix.

## Control flow (06)

### Map iteration under mutation
- **Notes row:** Map iteration while the map changes (2026-10-09, #124)
- **Rules / fixtures:** `flow.for.invalidate.panic` (net count change
  panics); snapshot + tombstones with a count-restoring compaction edge
- **Options:** (a) Keep panic rule + snapshot detail. (b) Specify
  snapshot semantics incl. compaction. (c) Fail-fast on any mutation.
- **Recommendation:** (a) — the panic covers every observable case but
  the narrow memory-safe window; document the edge as allowed.

### Loop joins lack row union
- **Notes row:** Loop value joins (2026-10-09, R20)
- **Rules / fixtures:** `types.lct.uses`, `types.lct.row-union-every-site`
  (no loop joins); `break`-carried function values
- **Options:** (a) Add loop joins to the site table. (b) Exclude them.
  (c) Stay silent.
- **Recommendation:** (a) — every other join site gets the union.

### Entry-run panics need `CODE:` (conformance runner)
- **Notes row:** `flow.panic.report` and the entry-run panic fixtures
  (2026-10-09, #191)
- **Rules / fixtures:** `flow.panic.report` (message optional);
  `collections.hd` sites pass `""`, slicing carries messages
- **Options:** (a) Message every std panic site. (b) Runner accepts a
  category-only line (as test mode does). (c) Mandatory message in rule.
- **Recommendation:** (b) short-term, (a) ongoing.

## Traits (09)

### `@derive(Eq, Ord)` without `PartialOrd`
- **Notes row:** `@derive(Eq, Ord)` (no `PartialOrd`) (2026-10-08)
- **Rules / fixtures:** `trait.derive.related.replaces` (settled,
  517696e9); fixture expects single `mixed-derived-law`; today both
  errors (was #126)
- **Options:** (a) Amend `replaces` to partial. (b) Keep rule+fixture,
  fix compiler. (c) Intentional cascade, clarify.
- **Recommendation:** (b) — owner settled spec+fixture; audit requires
  the single error.

### Field missing several derived traits
- **Notes row:** Field missing several derived traits (2026-10-08)
- **Rules / fixtures:** `trait.derive.field-missing-trait` (singular
  "names the trait and the field")
- **Options:** (a) First-trait-only + clarify fix-one-at-a-time.
  (b) One error per missing trait. (c) Silent on count + new fixture.
- **Recommendation:** (a) — matches wording; clarification only.

### Walker failure reported at block header
- **Notes row:** Derivation block (2026-10-08)
- **Rules / fixtures:** `trait.derive.field-missing-trait` ("at the
  field"); blocks have no field positions
- **Options:** (a) Header position + rule exception. (b) Field positions
  in blocks. (c) Both.
- **Recommendation:** (a) — clarify rather than invent positions.

### `unknown-trait` cascades into junk Body error
- **Notes row:** `unknown-trait` on an impl head (2026-10-08, was #94)
- **Rules / fixtures:** resolution fails on the head, then `unsupported:
  stage Body: body of … has no header` with empty path
- **Options:** (a) Keep. (b) Poison the body (no Body error).
  (c) Keep Body error, fix the empty path.
- **Recommendation:** (b), (c) at minimum — empty paths are never OK.

### `dyn Sub` inherits supertrait bindings
- **Notes row:** `dyn Sub` where `trait Sub < Super[Item = T]`
  (2026-10-09, #193)
- **Rules / fixtures:** `trait.dyn.binding.complete/.identity`,
  `trait.binding.super.projection`;
  `runtime/valid/trait-value-binding-identity.hd`
- **Options:** (a) Supertrait bindings count for completeness+identity
  (today). (b) Neither. (c) Completeness only.
- **Recommendation:** (a) — symmetry with bounds treatment.

### Projection with no reachable assoc type is silent
- **Notes row:** Projection `T::Element` under `T < Supplier`
  (2026-10-09, #193)
- **Rules / fixtures:** `trait.assoc.projection`,
  `trait.assoc.ambiguous-type`; `unknown-associated-type` is
  bindings-only; no fixture
- **Options:** (a) Error at the projection. (b) Silent + use-site
  reports. (c) `ambiguous-associated-type`.
- **Recommendation:** (a) — declaration-site mistake; silences cascades.

### Written binding vs own bound's supertrait binding
- **Notes row:** `T < Prices[Item = string]` under
  `trait Prices < Supplier[Item = i32]` (2026-10-10, #239)
- **Rules / fixtures:** `trait.binding.once.elaborated`,
  `trait.binding.super.merge/.conflict`;
  `typing/invalid/supertrait-binding-conflict.hd`
- **Options:** (a) Silent in one bound (today). (b) Always
  `duplicate-associated-binding`. (c) Written wins silently.
- **Recommendation:** (b) — same-bound contradiction is the same
  meaning as cross-bound.

### `dyn` completeness for `Self`-fixed assoc types
- **Notes row:** `dyn Summable` where `trait Summable < Add[Out = Self]`
  (2026-10-10, #239)
- **Rules / fixtures:** `trait.dyn.binding.complete`,
  `trait.binding.super.meaning`; `dyn` cannot name `Self`
- **Options:** (a) Counts as bound, not carried (today). (b) Incomplete
  (`not-dynamically-safe`). (c) Carry opaquely.
- **Recommendation:** (a) — (b) bans a common pattern; matches widening.

### Poison value after `unknown-method`
- **Notes row:** `typing/invalid/error-find-concrete-receiver.hd`
  (2026-10-09, #137)
- **Rules / fixtures:** `failure.find::[DiskError]()` → `unknown-method`,
  then `never` value adds `nonexhaustive-match` + `unknown-variant` +
  `unknown-name` (five errors)
- **Options:** (a) Poison silent value. (b) Keep all five. (c) Fixture
  expects the cascade.
- **Recommendation:** (a) — follow-ons reason about a value never
  produced.

### Misplaced `@error(transparent)`
- **Notes row:** `@error(transparent)` before a variant without exactly
  one payload member (2026-10-09)
- **Rules / fixtures:** `annot.error.form.variant-transparent`
  (exactly one payload), `annot.error.form.misplaced`; silent today
  (name display, `.None` cause)
- **Options:** (a) Add a check (e.g. `decorator-target-kind`).
  (b) Keep silent. (c) Generalize `transparent`.
- **Recommendation:** (a) — `misplaced` covers it in spirit.

## Modules (10) and testing

### Beside-dir fix-it is text only
- **Notes row:** `duplicate-module-name` beside directory (2026-10-08)
- **Rules / fixtures:** `cli.task.beside-dir`,
  `module.test.integration.beside-dir` (both promise a move fix-it);
  `DiagBuf` has fix fields but no fill API
- **Options:** (a) Soften rules to text. (b) Add the fix API + attach.
  (c) Split per rule.
- **Recommendation:** (b) — one infrastructure fix satisfies these plus
  `unused-derivation-fact`.

### Library use of a dev dependency
- **Notes row:** Library code that uses a dev dependency (2026-10-08)
- **Rules / fixtures:** `module.test.non-test-use.dev-dependency`
  (`test-only-use`), `cli.dep.dev-use.remove-first`; today
  `unknown-module`; notes row says Fixed (O14)
- **Options:** (a) Keep `unknown-module`. (b) Implement specified codes
  + message. (c) Better `unknown-module` message.
- **Recommendation:** (b) — spec text and fixtures already exist.

### `nonlocal-impl.hd` reads a private field (fixture)
- **Notes row:** Fixture `typing/invalid/nonlocal-impl.hd` (2026-10-09)
- **Rules / fixtures:** `nonlocal-impl` intended; `names.field-lookup.private`
  adds correct second `private-member`; case needs one error
- **Options:** (a) `pub name` in the tree. (b) Multi-error cases.
  (c) Suppress inside invalid impls.
- **Recommendation:** (a) — smallest fix preserving the case.

### Property bodies need `TestRunner`
- **Notes row:** Property body rows (2026-10-09, #177, #225)
- **Rules / fixtures:** `std-testing.runner.body-property`
  (`it_prop` bodies accept `TestRunner`);
  `typing/valid/test-runner-every-registration-form.hd`; signatures give
  `prop: fn!(T) -> R` no row
- **Options:** (a) Row-holding property closures. (b) Amend fixture.
  (c) Exempt property bodies.
- **Recommendation:** (a) — signatures are the outlier.

### Snapshot `--update` semantics
- **Notes row:** `snapshot` with empty/stale `expect` (2026-10-09, #177)
- **Rules / fixtures:** `module.testing.snapshot.literal` Why (update
  fills in place); compiler compares only, always
- **Options:** (a) Specify fill-in-place. (b) Compare-only + fix Why.
  (c) Separate entry point.
- **Recommendation:** (b) short-term, (a) as a feature.

### `dbg` in release and dependencies
- **Notes row:** `module.dbg.release`, `cli.dbg.*`, `module.dbg.dependency*`
  (2026-10-09, #180)
- **Rules / fixtures:** `dbg-in-release`/`dbg-in-dependency` wait on the
  build profile (`Goal` carries none, codegen.md §13.17) and fetched
  packages; `cli/dbg-release` step 2 fails
- **Options:** (a) Thread profile through `Goal` + model fetched packages.
  (b) Drop the rules. (c) Mark fixture aspirational.
- **Recommendation:** (a) — rules decided; only plumbing missing.

### REPL value display via `dbg_text`
- **Notes row:** `cli.repl.value`, `module.dbg.body.value` (2026-10-09, #180)
- **Rules / fixtures:** `dbg_text` intrinsic stub panics
  `explicit-panic: intrinsic`; no REPL yet
- **Options:** (a) Implement `dbg_text` + display path. (b) Specify the
  stub panic. (c) Remove `cli.repl.value` meanwhile.
- **Recommendation:** (a) — the panic is a placeholder, not a semantic.

### `dbg` printer for types lacking `Debug`
- **Notes row:** `module.dbg.signature` with `lib/std/format.hd`
  (2026-10-09, #180)
- **Rules / fixtures:** hidden `dbg_show[T]` + four std helpers, no spec
  change; std declarations stay ordinary hd
- **Options:** (a) Ratify as implementation detail. (b) Specify in
  language tier. (c) Require `Debug` bounds.
- **Recommendation:** (a) — nothing observable changes.

### `dbg` of generic values
- **Notes row:** `module.dbg.value.generic` (2026-10-09, #180)
- **Rules / fixtures:** implementation-defined for unbounded types;
  compiler prints per-instance concrete types
- **Options:** (a) Specify per-instance printing. (b) Keep
  implementation-defined. (c) `Debug`-bound printing only.
- **Recommendation:** (a) — free precision from collection.

### `dbg` body order with spread
- **Notes row:** `module.dbg.body.print` with spread/`values=`
  (2026-10-09, #180)
- **Rules / fixtures:** positional prints-as-evaluated; spread/`values=`
  evaluates into the tuple first; unknown-tuple fallback prints no
  location
- **Options:** (a) Specify both behaviors. (b) Force strict order.
  (c) Leave spread unspecified.
- **Recommendation:** (a) — documents the real evaluation structure.

### `folder-cycle-leaf-folder.hd` returns `dyn Error` (fixture)
- **Notes row:** Fixture `typing/valid/folder-cycle-leaf-folder.hd`
  (2026-10-10, M4 triage)
- **Rules / fixtures:** `module.cycle.acyclic` (only moves the file);
  `types.trait.value` (`dyn` + trait only); tree declares `enum Error`
- **Options:** (a) Fixture returns `Error`. (b) Accept `dyn` over enums.
  (c) New `dyn`-over-enum rule.
- **Recommendation:** (a) — compiler right; one-line fixture fix.

### Private member from an integration view
- **Notes row:** `private-member` from an integration-view module
  (2026-10-09)
- **Rules / fixtures:** `module.test.doc.view` + visibility; `slug.text`
  reads private field outside `pkg.slug`; hd_check lane;
  `runtime/valid/doc-test-compile-fail.hd`
- **Options:** (a) Enforce on the hd_check lane. (b) Exempt
  integration/doc views. (c) View keeps defining-module visibility.
- **Recommendation:** (a) — the rule as written demands it.

## Requirements and suspension (11)

### Row parameters print as `owner#index`
- **Notes row:** Row parameters in messages (2026-10-08, was #51)
- **Rules / fixtures:** `req.row.union.literal.diagnostics`,
  `req.row.alias.diagnostics` (cover literal/alias rows only)
- **Options:** (a) Clarify scope + keep internal form. (b) Declared
  names in messages. (c) Both.
- **Recommendation:** (b) long-term, (a) for this audit.

### Requirement keys with inherited bindings stay distinct
- **Notes row:** `$ PriceFeed` vs `$ PriceFeed[Item = i32]` (2026-10-10,
  #239)
- **Rules / fixtures:** `req.key.binding.identity/.complete/.rows`,
  `trait.dyn.binding.identity`;
  `typing/valid/requirement-key-trait-value-argument.hd`
- **Options:** (a) Completeness counts it, keys stay two (today).
  (b) Merge keys like `dyn` identity. (c) Require written bindings.
- **Recommendation:** (a) — identity and completeness ask different
  questions; merging collapses distinct rows.

### Indirect `block_on` in a default is a panic (fixture)
- **Notes row:** Fixture `typing/invalid/suspension-forbidden-context.hd`
  (2026-10-10, #205b)
- **Rules / fixtures:** `req.drive.block-on.direct/.syntax` (own source
  only) vs `.indirect` (panic); `wait_now` wrapper
- **Options:** (a) Fixture becomes a panic fixture. (b) See through one
  helper. (c) New transitive error.
- **Recommendation:** (a) — transitive analysis contradicts `.syntax`.

### `println` in non-entry init: which error?
- **Notes row:** `println` in non-entry module init (2026-10-10, #205b)
- **Rules / fixtures:** `module.console.println-block-on` ("every rule of
  `block_on`") vs `.direct` (three contexts only);
  `non-entry-top-level-println.hd` expects `missing-requirement` alone
- **Options:** (a) `missing-requirement` only (today). (b) Both errors +
  fixture update. (c) Forbidden-context only.
- **Recommendation:** (b) — but `.direct` must then name non-entry init.

### Alias expansion in row diagnostics
- **Notes row:** `req.row.alias.diagnostics.expanded` (2026-10-09, #175)
- **Rules / fixtures:** message-appended expansion for the function's own
  row (today: "`$ WebRow + Auth` is `$ Auth + Cache + Clock + Db`");
  closures' written rows and union element order unfinished
- **Options:** (a) Accept current scope. (b) Require header-label form.
  (c) Extend to closures + union order.
- **Recommendation:** (a) short-term, (c) full fix.

## Annotations and `@error` (14)

### `unused-derivation-fact` fix-it is text only
- **Notes row:** `unused-derivation-fact` fix-it (2026-10-08)
- **Rules / fixtures:** `annot.fact.unused-block-decorator.fix` (moves
  value into block as `Self += [...]`)
- **Options:** (a) Downgrade to message-suggests. (b) Add `DiagBuf` fix
  API + fixture. (c) Drop `.fix`.
- **Recommendation:** (b) — one fix covers beside-dir rows too.

### `@error` rejects prefixed-string messages
- **Notes row:** `@error` message argument (2026-10-08)
- **Rules / fixtures:** `annot.error.form.argument`,
  `annot.error.message.string` ("ordinary interpolated string")
- **Options:** (a) Clarify plain-literal-only. (b) Accept `str_prefix`
  forms. (c) Accept string-typed prefixes (new rule).
- **Recommendation:** (a) — cheapest, matches compiler.

### `facts_of` result vs std declaration
- **Notes row:** `facts_of(f)` and the decorators it reads (2026-10-10,
  #231)
- **Rules / fixtures:** `annot.facts-of.result` (returns `Facts`);
  std declares `-> Facts`; valueless-decorator list unspecified;
  `runtime/valid/facts-of-read.hd`
- **Options:** (a) List valueless decorators + schedule
  `typed-fact.infer` in bodies. (b) Return `dyn Any`. (c) Callers
  downcast.
- **Recommendation:** (a) — rule already promises `Facts`.

### `facts_of(f).find` on `-> dyn Any`
- **Notes row:** `annot.facts-of.result`
  (`facts_of(f).find::[Route]()`, #178) (2026-10-09)
- **Rules / fixtures:** rule says `Facts`; std said `dyn Any`;
  `runtime/valid/facts-of-read.hd`
- **Options:** (a) Lower intrinsic to `Facts` (fix std+compiler).
  (b) Rule to `dyn Any`. (c) `find` on `Any`.
- **Recommendation:** (a) — rule explicit.

### `Walker.rest` default with bounded `member`
- **Notes row:** `Walker.rest` default with bounded `member` (2026-10-09,
  #173c, #182)
- **Rules / fixtures:** `annot.walk.rest.default`; walkers without `rest`
  expected valid; std walkers hand-write `rest` meanwhile
- **Options:** (a) Check default once against the trait (accept; Rust).
  (b) Check per implementation (reject; frozen TS). (c) Bound the
  default's `T`.
- **Recommendation:** (a) — fixtures expect validity.

## Standard library

### `join` needs one primitive (frozen)
- **Notes row:** `spec/std/README.md` primitives, `join` (2026-10-08, #112)
- **Rules / fixtures:** owner chose A1 `concat_all`; spec frozen; O(n log
  n) halves today
- **Options:** (a) Defer to unfreeze batch. (b) Add now despite freeze.
  (c) Specify `join` without the primitive.
- **Recommendation:** (a) — record deferred A1 only.

### Shortest float display ties
- **Notes row:** `types.display.float` (2026-10-09, #181a)
- **Rules / fixtures:** "shortest round-trip digits" unspecified on ties;
  `2^-25` halfway; host writes even digit per `std-num.to-fixed.ties`
- **Options:** (a) Even-last-digit (unless it mis-reads below a power of
  two). (b) Round-up (Rust). (c) Unspecified.
- **Recommendation:** (a) — composes with the `to_fixed` tie rule.

## CLI (incl. JSON, cache, REPL)

### Usage vs not-implemented errors in JSON
- **Notes row:** Command-line and not-implemented errors in `--format
  json` (2026-10-08)
- **Rules / fixtures:** Exit Status Note (usage errors codeless);
  `cli.json.diagnostic.*`; not-implemented manifest sections open
- **Options:** (a) `"code":null` for both. (b) Usage null,
  not-implemented `unsupported` with position. (c) Code usage errors.
- **Recommendation:** (b) — unimplemented sections are program facts at
  a line, not CLI misuse.

### Unrunnable test case: ignored
- **Notes row:** `hd test --format json`, a case that cannot run yet
  (2026-10-08)
- **Rules / fixtures:** `cli.json.test.result` (three outcomes); text
  `UNSUPPORTED` counted apart; run passes either way
- **Options:** (a) Keep ignored/UNSUPPORTED. (b) Fourth outcome.
  (c) Fail the run.
- **Recommendation:** (a) — confirm counting in summary rules only.

### `hd.toml` errors and the 0.0.0 toolchain
- **Notes row:** `hd.toml` diagnostics (2026-10-08)
- **Rules / fixtures:** `cli.manifest.*`; line + column 1; version 0.0.0
  placeholder
- **Options:** (a) Keep line-level + label placeholder. (b) Exact spans.
- **Recommendation:** (a) — reasonable; no rule change.

### `[capabilities]` errors have no code
- **Notes row:** `[capabilities]` key and value errors (2026-10-08)
- **Rules / fixtures:** `cli.cap.table.keys`, `cli.cap.value.unscoped`
  (errors, no codes); `--cap` errors are command-line errors
- **Options:** (a) Keep codeless. (b) Assign code(s). (c) Code keys only.
- **Recommendation:** (b) — manifest semantic errors with positions
  should be filterable.

### `cli.mode.member.unlisted`: no code/position/fix-its
- **Notes row:** `cli.mode.member.unlisted` (2026-10-08)
- **Rules / fixtures:** rule + `.unlisted.fix` (two fix-its, no code)
- **Options:** (a) Keep all absent. (b) Position + fix-its, still codeless.
  (c) Full code + position + fix-its.
- **Recommendation:** (b) — rule already specifies the attachments.

### `unlisted-entry` fix-it position
- **Notes row:** `unlisted-entry` fix-it (2026-10-08)
- **Rules / fixtures:** `cli.exe.main-unlisted` (no line named); end of
  `hd.toml` append
- **Options:** (a) Keep end-of-file, positionless intentional. (b) After
  last `[[executable]]`. (c) Inline position.
- **Recommendation:** (a) — sane default, deliberately unspecified.

### `cli/ambiguous-import` expects unnamed fields (fixture)
- **Notes row:** Fixture `cli/ambiguous-import` (2026-10-08)
- **Rules / fixtures:** `cli.json.diagnostic.fields` (six) + `.fixes`;
  fixture wants `notes`/`related`/`fix`/`rule`/`rules` + exact quoting
- **Options:** (a) Fix fixture to specified schema. (b) Add the fields
  to rules. (c) Aspirational failing.
- **Recommendation:** (a) — compiler already emits the contract.

### Workspace roots prefix member directories
- **Notes row:** Workspace mode diagnostic paths (2026-10-08)
- **Rules / fixtures:** `cli.json.diagnostic.file` ("relative to the
  package root … in a package"); today `lib/src/lib.hd`
- **Options:** (a) Keep prefix + extend rule sentence. (b) Per-member
  relative (ambiguous). (c) Absolute.
- **Recommendation:** (a) — disambiguation requires it.

### `cli/command-help` vs the Commands table (fixture)
- **Notes row:** Fixture `cli/command-help` (2026-10-09)
- **Rules / fixtures:** `cli.command.help` ("every command of the
  table"); table has `fmt`/`fix`/`cache gc`, no
  `explain`/`def`/`repl`/`debug`; compiler follows the table
- **Options:** (a) Fix fixture to the table. (b) Change the table.
  (c) Keep failing.
- **Recommendation:** (a) — new commands are a separate decision.

### `hd new --pages` note is stale
- **Notes row:** `hd new --pages` (2026-10-09)
- **Rules / fixtures:** "Planned" note vs `cli.new.pages.*` + fixtures +
  implementation (agree)
- **Options:** (a) Retire the note. (b) Restore note, remove feature.
  (c) Mark historical.
- **Recommendation:** (a) — the note is the stale artifact.

### Shared default compiled cache
- **Notes row:** The default compiled cache (2026-10-09, O20)
- **Rules / fixtures:** `toolchain_key` (compiler version + target + std
  hash, not the `hd` build); dev builds share entries (seen `ex[init]`
  failure)
- **Options:** (a) Hash the `hd` build into the key. (b) Document
  fresh-`HD_CACHE` for dev. (c) Per-checkout namespace.
- **Recommendation:** (a) — cross-build reads are unsound.

### Denied capability from `hd test`
- **Notes row:** `denied-capability` from `hd test` (2026-10-09)
- **Rules / fixtures:** `cli.cap.total.test` ("in the same way");
  message names the program, no position
- **Options:** (a) Keep as-is. (b) Also set `file`. (c) `hd.toml`
  position.
- **Recommendation:** (a) or (b) — smallest compatible enrichment.

### `instantiation-too-deep` truncation
- **Notes row:** `instantiation-too-deep` message (2026-10-09, O25, #218)
- **Rules / fixtures:** codegen §13.4 ("types printed in full");
  120-byte cut (`hd_mono::SHOWN_TYPE_MAX`) on 2^33-leaf trees
- **Options:** (a) Amend rule to allow the cut. (b) Print whole.
  (c) Summarize last instance.
- **Recommendation:** (a) — full print is unbounded.

### `test-outcomes` text step conflict (fixture)
- **Notes row:** CLI fixture `cli/test-outcomes` (2026-10-09, #183)
- **Rules / fixtures:** `cli.test.report.*` (`FAIL`/`PANIC` blocks,
  `test result:`, empty stderr) vs `test-outcomes` per-module form; JSON
  steps pass
- **Options:** (a) Amend `test-outcomes` text expectations. (b) Change
  runner output. (c) Split fixtures by mode.
- **Recommendation:** (a) — runner follows the specified form.

### REPL value display via `dbg_text`
- **Notes row:** `cli.repl.value`, `module.dbg.body.value` (2026-10-09)
- **Options:** See Modules entry above.
- **Recommendation:** Implement `dbg_text` (placeholder panic today).

## Correctly-rounded float `**`
- **Notes row:** Float `**` (2026-10-09, #173a)
- **Rules / fixtures:** `expr.power.float.pow` (correct rounding);
  double-double + half-even midpoints; ~26k + specials tested, no
  mismatch; `f32` double-rounding caveat
- **Options:** (a) Keep requirement + cite corpus. (b) Weaken pending
  proof. (c) Name the algorithm.
- **Recommendation:** (a) — midpoint policy principled; only caveat noted.

## Drift watch (re-measure, not audit decisions)

- **Notes row:** Drift: `script-test-init-requirement` (M4 triage)
  (2026-10-10). Emits the expected code today — confirm green and close
  (`module.init.tests.requirement-free`).
- **Notes row:** Drift: `enum-default-requires-provider` (M4 triage)
  (2026-10-10). Emits `missing-requirement`; fixture keys
  `requirement-in-default` — compiler side moves
  (`data.shared.module-init.requirement-free`).
- **Notes row:** Drift: `missing-requirement-cold-construction.hd` (M4
  triage) (2026-10-10). First code right plus `cancel` cascades —
  resolve whether `Suspend.cancel` resolves as written before touching
  the fixture.
- **Notes row:** Fixture `runtime/valid/resource-disposed-result.hd`
  (2026-10-09). Signature violates `module.boundary.in/.error` as
  written — fixture fix (`pub message` / derive) when the freeze lifts.

## Coverage

85 notes rows → entries above (the REPL `dbg_text` row cross-references
the Modules `dbg` entry). Verified programmatically: every row's
distinctive terms appear in this file. Three worker batches drafted by
chapter; four rows the batches missed (void `TypeId`, void messages, two
fixture rows) were drafted directly. Three batch overlaps (copy-update,
map iteration, CRLF drafted twice) were deduped.
"Unchecked" rule lookups flagged by drafters are marked in place.
