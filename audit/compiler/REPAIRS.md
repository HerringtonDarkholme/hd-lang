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
Each attempt runs under a rollback transaction over the checker's reachable state, preserving existing object identities.
Locals, captures, diagnostics, enumerable argument caches, and lazy signature inference participate in the same transaction.
Nested attempts restore their own entry state, whether they succeed or fail.

The selected candidate is checked again outside the transaction; trial HIR is never committed.
Associated calls use the same instantiation fitting, expected-result check, and numeric literal default tie-break.
Distinct traits remain ambiguous instead of acquiring argument-based overload resolution.

The transaction contract covers ordinary properties, arrays, maps, and sets.
Opaque weak collections fail closed; adding external mutable caches or private slots requires extending the contract.
The repair trades snapshot cost for a simple rollback guarantee; it makes no performance claim.

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

### Repair Status Table

| Finding | Status | Repair and limits |
| --- | --- | --- |
| A02 | Partially fixed | Public readonly inherent instance signatures now participate in nominal variance verification. Type encoding, coercion and least-common-type findings remain open. Private-surface interpretation remains deferred. |
| A03 | Reported control-flow defect fixed | All child-driving bodies use the suspension CFG. The linear backend and comprehension bypass are removed. This does not close A06's entry/waker gap or prove all lowering correct. |
| A04 | Reported placeholder capture fixed | Generated expression and type placeholders cannot capture legal user identifiers. Broader generated helper-name hygiene remains unreviewed. |
| A05 | Reported candidate-checking defects fixed | Arbitrary argument expressions and associated candidates are checked under complete reachable-state rollback, then the winner is committed once. Broader resolution conformance remains open. |

## Reproduced Failures And Repairs

### A02: Readonly Method Variance

Before the repair, a covariant data type's public readonly method could consume its type parameter without any diagnostic.
The repaired checker reports `invalid-variance` at the offending signature type.
It reuses the field polarity traversal for method inputs, results, and implementation targets.
Renamed implementation parameters, nested target constructors, and method-generic shadowing retain their distinct roles, including within `Self`.
Public methods require explicit result types before this pass, so result inference cannot bypass it.

Associated construction and mutable receivers do not expose a readonly instance view and are excluded.
Separate trait implementations retain their existing independent signature checks.
This bounded repair does not resolve the audit's private-method specification question.

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
