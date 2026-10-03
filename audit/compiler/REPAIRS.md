# Compiler Audit Repairs

Status: Verified implementation repairs for existing rules, not new language decisions. The broader audit remains incomplete.

Implementation base: `76f29a5fb14780ac69cd606d880c7cc30d5b3a33`.
The repairs were rebased onto `900f8fcf` before integration checks, preserving the upstream range, shift-count, and testing-prelude changes.
The historical audit baseline and coverage ledgers remain unchanged.
Changes are restricted to `src/` and this audit's repair tracking; no specification or conformance fixture is edited.

## Repair Status

### Independent Repair: Captured Cells

Captured-cell conversion now resolves closures by their explicit HIR indices, rather than array positions.
Repeated conversion leaves existing cells intact instead of nesting cell storage.
A typed, exhaustive HIR traversal replaces reflective object rewriting and preserves unrelated metadata.
Local identity remains object identity; equal-looking locals from separate activations never share storage accidentally.

Five source regressions cover sparse closure indices, idempotence, dictionary and match traversal, nested escaped captures, and per-iteration storage.
The runtime cases run with immediate and artificially pending child execution.
They verify existing behavior remains intact, not that those compositions were previously broken.
This closes these specific A07 defects, not the full host ABI, replay, or capture audit.

Regression tests: [captured-cells.test.ts](../../src/checker/captured-cells.test.ts).

### Independent Repair: Candidate Transactions

Candidate checking now accepts arbitrary argument expressions instead of allowing only a syntax whitelist.
Each attempt runs under a rollback transaction over mutable checker state, preserving existing object identities.
Locals, captures, diagnostics, argument caches, and lazy signature inference participate in the same transaction.
Nested attempts restore their own entry state, whether they succeed or fail.

The selected candidate is checked again outside the transaction; trial HIR is never committed.
Associated calls use the same instantiation fitting, expected-result check, and numeric literal default tie-break.
Distinct traits remain ambiguous instead of acquiring argument-based overload resolution.

The transaction contract covers ordinary properties, arrays, maps, and sets.
Opaque weak collections fail closed; adding external mutable caches or private slots requires extending the contract.
Immutable program inputs do not participate; mutable inference registries use sparse write journals.

#### Transaction Cost Regression

The first transaction implementation, `d8f5f0c9`, traversed lazy inference's program context through the signature map.
On slicing, each candidate snapshot reached about 35,000 objects, including immutable declarations, type registries, and implementation tables.
Additional integer methods increased candidate counts and multiplied that unrelated work.

Checker contexts now declare immutable inputs and snapshot only mutable body state and caches.
Lazy signature replacements and pending or failed inference membership use nested, touched-entry rollback journals.
Existing closure, global, and diagnostic containers are captured without traversing their immutable contents.
Destructive signature-map operations preserve iteration order through a lazy order snapshot; normal inference replacements never enumerate the registry.

Source regressions forbid immutable registry traversal and signature enumeration during ordinary trials, using a 10,000-entry registry.
They also cover nested savepoints, delete/reinsert ordering, symbol-keyed caches, accessor avoidance, and rollback after snapshot setup fails.
The original candidate correctness regressions remain intact.
No timeout, standard library, specification, or conformance fixture changed.

Fresh-process local checks compare `17b96635` against this repair; each case reports the median of three runs.
Whole-conformance timings use `node --experimental-strip-types test/run-portable.ts --suite conformance --jobs 8` on both revisions.

| Measurement | Before | After |
| --- | ---: | ---: |
| `typing/valid/slice-types.hd` check | 2.697 s | 0.250 s |
| `runtime/valid/slicing-run.hd` check | 2.377 s | 0.276 s |
| All 1,893 conformance cases, eight workers | 35.390 s | 29.589 s |

Validation: full `pnpm run check`, all 75 source tests, website build, and fuzz smoke passed.
The source suite includes 24 candidate transaction regressions.

Regression tests: [call-speculation.test.ts](../../src/checker/call-speculation.test.ts).

Candidate edge cases for later conformance fixtures:

| Input shape | Required observation |
| --- | --- |
| Reverse two instantiations while an unannotated closure fits only one | The same candidate and result are selected, without duplicate captures or locals |
| A tuple spread or named argument is checked against several associated candidates | Failed attempts do not affect the winner's argument order or generated locals |
| A failed attempt first calls a function with an omitted result type | Later candidates and final checking see its correctly inferred result, not stale partial inference |
| An argument introduces a provider scope, captured value, or suspended child | Trial effects disappear; the selected call retains exactly its own effects |
| Two unrelated traits offer a method with the same name but different parameter types | Argument fitting does not resolve the ambiguity |
| Two associated instantiations accept the arguments, but only one result fits the expected type | Select the result-compatible instantiation |

### Independent Repair: Optional Constructor Boundaries

Optional construction now preserves the permission of its payload independently of the optional's outer view.
On the pre-repair `main`, all three spellings below parsed to the same internal type, `mut:User?`.
The owner clarified that `mut User?` contains a mutable `User`, just like `Option[mut User]`.
Explicit `mut Option[User]` or `mut (User?)` applies permission to the outer optional instead.
Nested constructors, generic substitution, generated builders, runtime type identity, and source rendering must preserve that distinction.

Source generation uses the shared optional constructor instead of concatenating a question mark onto a mutable type spelling.
The REPL renders canonical constructors back into unambiguous source syntax.
This is a representation repair; it does not close the separate variance, conversion, or least-common-type findings.

Edge cases for later fixtures include nested optional payload permissions, optional function versus optional result types, mutable generic aliases, and mutable fields in derived builders.
Inference advice now renders these boundaries without suggesting an annotation with different permission scope.
Whether `let mut` also admits an outer readonly optional whose payload is another readonly optional of a mutable value remains an audit question.
This repair recognizes the specified direct mutable payload, without granting arbitrary nested access.

Regression tests: [types.test.ts](../../src/types.test.ts).

### Independent Repair: Private Readonly Method Variance

The readonly nominal surface now includes private inherent methods, as [types.variance.surface](../../spec/lang/04-type-system.md#r-types.variance.surface) requires.
Written signature positions are checked before body lowering; omitted private results are checked after inference completes.
The late check resolves prepared methods through their source AST identity rather than reconstructing generated function names.
This closes visibility and inferred-result bypasses without changing the existing associated-constructor or mutable-receiver exclusions.

Inference exposed another bypass: an impl's `T` and a method's shadowing `T` previously shared one internal identity.
An inferred result containing a value obtained through `Self` could therefore lose its enclosing impl occurrence during variance checking.
Colliding method binders now receive inaccessible identities before `Self` expansion and local declaration hoisting.
Typed, exhaustive AST traversal preserves bounds, defaults, nested scopes, source spans, and local nominal declaration-point shadowing.

Qualified references retain their source owner and carry a separate type-binder candidate.
The checker consults that candidate only after existing value-owner lookup, preserving local values and captured values with the same spelling.
Diagnostics restore issued binder spellings, including Unicode names.
Unchanged programs and implementations retain the fast path; the slicing timeout repair remains intact.

| Later fixture candidate | Required observation |
| --- | --- |
| Private readonly method consumes a covariant impl parameter | Reject `invalid-variance`, just as for a public method |
| Private readonly method infers a positive result containing a contravariant impl parameter | Reject after inference rather than inspecting the omitted-result placeholder |
| Shadowed versus renamed method binder returns an invariant wrapper or mutable list containing `self.consume` | Reject identically; method naming cannot erase the impl occurrence |
| Method-owned generic result, including a captured callback | Accept independently of the enclosing nominal parameter |
| Local nominal declaration shadows a method binder, including inside a nested closure | Preserve declaration-point scope without leaking it to the enclosing suite |
| Type-bound `T::zero()` and local-value `T::to_string()` share a binder spelling | Resolve the type and value paths independently, including captures and match bindings |

Validation: full `pnpm run check`, all 87 source tests, website build, and fuzz smoke passed after rebasing onto `875c63c8`.
The 27 variance regressions include generated Wasm with distinct receiver and method type arguments, numeric dictionaries, and captured value owners.

Regression tests: [variance.test.ts](../../src/checker/variance.test.ts).

### Further A02 Edge Case: Callable Requirement Rows

Status: Reproduced, not fixed by the private-method repair.
The callable variance traversal checks inputs and results, but currently omits generic occurrences in requirement rows.
[types.variance.function](../../spec/lang/04-type-system.md#r-types.variance.function) requires the callable's requirement row to be invariant.
The same issue occurs inside a private method's callable parameter; it predates the private-method repair.

| Later fixture candidate | Required observation |
| --- | --- |
| Covariant `Box[T]` stores a callable with requirement `Cap[T]` | Reject `invalid-variance`; the callable row contains an invariant occurrence of `T` |
| Readonly inherent method receives that callable through a signed impl parameter | Apply the same row traversal regardless of method visibility |
| A method's standalone requirement clause mentions a signed impl parameter | Audit its specified polarity separately; do not infer a new rule from callable row invariance |

### Repair Status Table


| Finding | Status | Repair and limits |
| --- | --- | --- |
| A02 | Partially fixed | Public and private readonly inherent signatures, including inferred private results, participate in variance verification. Optional constructor boundaries preserve payload permission. Shared coercion, least-common-type, and callable requirement-row findings remain open. |
| A03 | Reported control-flow defect fixed | All child-driving bodies use the suspension CFG. The linear backend and comprehension bypass are removed. This does not close A06's entry/waker gap or prove all lowering correct. |
| A04 | Reported placeholder capture fixed | Generated expression and type placeholders cannot capture legal user identifiers. Broader generated helper-name hygiene remains unreviewed. |
| A05 | Reported candidate-checking defects fixed | Arbitrary argument expressions and associated candidates use mutable-state rollback and sparse inference journals, then the winner is committed once. Broader resolution conformance remains open. |

## Reproduced Failures And Repairs

### A02: Readonly Method Variance

Before the repair, a covariant data type's public readonly method could consume its type parameter without any diagnostic.
The repaired checker reports `invalid-variance` at the offending signature type.
It reuses the field polarity traversal for method inputs, results, and implementation targets.
Renamed implementation parameters, nested target constructors, and method-generic shadowing retain their distinct roles, including within `Self`.
Public methods require explicit result types before this pass, so result inference cannot bypass it.

Associated construction and mutable receivers do not expose a readonly instance view and are excluded.
Separate trait implementations retain their existing independent signature checks.
The later private-method repair closes the visibility and inferred-result gap under the current explicit private-surface rule.
Callable requirement-row variance remains a separate reproduced defect.

Regression tests: [variance.test.ts](../../src/checker/variance.test.ts).

### A03: Suspension Exits

Before the repair, a nested return after a direct child drive produced `0` instead of the expected `43`.
Separate Result propagation attempted to return an enum reference from an i32 readiness poll, producing invalid Wasm.
Non-driving comprehensions containing propagation had the same poll-return escape.

Child-driving bodies now have one CFG owner for returns, propagation, cleanup, completion, and cancellation.
Bodies without child drives retain an ordinary function body behind a synchronous poll wrapper.
CFG comprehension lowering no longer depends on whether a comprehension itself contains a bang call.
Void return operands are evaluated before cleanup, rather than discarded because they require no result storage.
Sequential suspension sites reserve their indices before lowering their continuations, preserving execution-order numbering.

Cleanup cannot itself propagate with `?`, as the existing cleanup restriction requires.
The checker reports `defer-control-flow`; a nested closure may still propagate within its own function.
No public driver or wake-delivery protocol is changed.

Regression tests: [suspension.test.ts](../../src/emitter/suspension.test.ts).

### A04: Generated Placeholder Hygiene

Before the repair, a member named `HDTYPE0X` became the target type's name.
An unrelated expression named `hdexpr0` became an inserted integer expression.
End-to-end derivation also corrupted placeholder-shaped fields and variants, reporting `unknown-data-field` and `unknown-variant`.
Out-of-range placeholder-shaped names became `undefined` in those diagnostics.

The source builder now issues out-of-language handles and allocates collision-free parser identifiers after renaming.
Only issued expression tokens are substituted; type tokens are replaced only in AST type positions.
Inserted expression nodes and their source spans remain intact, and replacements do not cascade through user type spellings.
Derived data and enum equality now compile and execute with those legal field and variant names.

This repair does not establish hygiene for other generated helper and local names.

Regression tests: [generated-source.test.ts](../../src/checker/generated-source.test.ts).

## Validation

| Check | Result |
| --- | --- |
| `TERM=xterm-256color pnpm run check` | Passed: both TypeScript projects, lint, formatting, 1,871 selected conformance cases, 16 portable fixtures, 375 existing Node tests, and specification gates |
| `node --test --experimental-strip-types 'src/**/*.test.ts'` | Passed: 29 new regressions, zero failures |
| `pnpm run website:build` | Passed: 44 pages and playground |
| `git diff --check` | Passed |

The initial integrated test run inherited `TERM=dumb` and failed the terminal Backspace test.
Node's readline treats that key literally in this environment; the focused terminal tests pass with `TERM=xterm-256color`.
The complete check then passed with that terminal setting; no REPL code or existing test was changed.
The initial lint run also required extracting propagation checking from a method that exceeded the repository's line limit after adding the cleanup guard.
Eight pre-existing dead-citation warnings remain; there are no failing citations.

The colocated `src/` regression tests require an explicit Node test command because the existing package test glob covers `test/`, not `src/`.
The original checkout and specification files are not edited by this repair commit.
The audit's historical rule/file ledgers remain snapshots, not claims of full semantic verification.
