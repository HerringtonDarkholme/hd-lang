# Compiler Audit Repairs

Status: Verified implementation repairs for existing rules, not new language decisions. The broader audit remains incomplete.

Implementation base: `76f29a5fb14780ac69cd606d880c7cc30d5b3a33`.
The repairs were rebased onto `900f8fcf` before integration checks, preserving the upstream range, shift-count, and testing-prelude changes.
The historical audit baseline and coverage ledgers remain unchanged.
Changes are restricted to `src/` and this audit's repair tracking; no specification or conformance fixture is edited.

## Repair Status

| Finding | Status | Repair and limits |
| --- | --- | --- |
| A02 | Partially fixed | Public readonly inherent instance signatures now participate in nominal variance verification. Type encoding, coercion and least-common-type findings remain open. Private-surface interpretation remains deferred. |
| A03 | Reported control-flow defect fixed | All child-driving bodies use the suspension CFG. The linear backend and comprehension bypass are removed. This does not close A06's entry/waker gap or prove all lowering correct. |
| A04 | Reported placeholder capture fixed | Generated expression and type placeholders cannot capture legal user identifiers. Broader generated helper-name hygiene remains unreviewed. |

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
