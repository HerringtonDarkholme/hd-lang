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

### Independent Repair: Std Submodule Binding

Status: SOURCE-2 reproduced and fixed as one part of A01; package ownership remains open. SOURCE-4 and local implementation extent are repaired separately below.

Before this repair, `withStandardSubmodules` walked every AST object before checking and rewrote a matching receiver spelling to a hidden std function name. A parameter named `arbitrary` therefore could not shadow `use std.testing.arbitrary`: `arbitrary.with(42)` called the imported module function and failed against its generator signature instead of calling the parameter's inherent method.

The std submodule use now remains an ordinary import binding and continues to make the module reachable through the existing use graph. Direct member-call checking resolves that binding to a public hidden std function only after checking the receiver name for a local, capture, module binding, or declared function. Typed-fact discovery uses the same std member resolver, so `@arbitrary.with(generator)` keeps its early expected-type check without rewriting unrelated body expressions.

This removes the reflective source transformation and makes the existing lexical checker authoritative. It does not add first-class module values or repair package-module ownership under A01. The separate repeated-alias and local-implementation findings are repaired below.

Manual repair probes cover an unaliased parameter, an aliased parameter, a local binding, and a closure capture. Existing valid and invalid `arbitrary.with` conformance cases preserve their results. No conformance fixture changed.

Validation: full `TERM=xterm-256color pnpm run check`, website build, and fuzz smoke passed. The fuzz smoke produced no phase signatures; its existing non-gating contract signatures remain outside this repair.

Std submodule edge cases for later fixtures:

| Input shape | Required observation |
| --- | --- |
| A parameter, local, or captured value has the imported submodule alias and an inherent `with` method | Resolve the lexical value and call its method |
| The submodule is imported under an alias and remains unshadowed | Resolve its public function through the alias |
| A module binding with the alias is declared after the call | Report that binding as not yet visible; do not fall back to the imported module |
| A selected submodule member is private, missing, or not a function | Reject the selection without exposing the hidden std declaration; audit the final diagnostic separately |
| A typed fact selects a public submodule function through an alias | Discover its result type and preserve the typed-fact expected-type check |

### Independent Repair: Repeated Std Declaration Aliases

Status: SOURCE-4's reproduced alias-loss defect is fixed as one part of A01. The loader's broader source-text renaming and package declaration ownership remain open architecture work.

Before this repair, the std loader selected one local spelling for each qualified declaration. A later `use std.cmp.min as second` physically renamed the one joined function to `second`, so an earlier distinct binding `use std.cmp.min as first` remained in the import table but had no signature. The valid call `first(second(3, 2), 1)` therefore reported `unknown-name` for `first`.

The checker now computes every additional spelling before joining std and carries those bindings beside the joined program. Alias-aware declaration registries resolve each spelling to the same stored signature, data, enum, or trait object while iteration exposes that object exactly once. Written type aliases expand to the joined declaration spelling before semantic checking. Qualified calls and method references canonicalize a type alias only after lexical value lookup, so local values keep ordinary shadowing behavior. Type-argument defaults are filled through the same binding relation.

This is deliberately a binding adapter around one declaration identity, not duplicated declarations or generated forwarding functions. It preserves lazy result inference and transactional signature rollback because alias lookup resolves to the stored declaration name before inference. Compiler-provided std names and imported submodules retain their separate resolution paths.

Manual probes and source regression tests cover functions, function values, generic data, enums, newtypes, traits with defaulted parameters, associated calls, method references, reversed import order, an original spelling plus an alias, lexical shadowing, Wasm emission, and canonical-only registry iteration. No specification or conformance fixture changed.

Validation: full `TERM=xterm-256color pnpm run check`, all 110 source tests, 1,915 conformance cases, website build, and fuzz smoke passed. The fuzz smoke produced no phase signatures; its existing non-gating contract signatures remain outside this repair.

Repeated-alias edge cases for later conformance fixtures:

| Input shape | Required observation |
| --- | --- |
| Import one ordinary std function under two aliases and call both | Both calls resolve to the same declaration and emit it once |
| Reverse the two imports | Checking and runtime behavior are unchanged |
| Import the original spelling and a renamed spelling together | Both bindings work; neither is treated as a duplicate local name |
| Use two aliases of a generic data, enum, newtype, or trait with defaults | Written types canonicalize to one declaration identity with the same arguments and defaults |
| Select an inherent associated function or method reference through either type alias | Both selections resolve the same inherent implementation |
| Shadow one function alias with a parameter, local, or capture | Lexical value lookup wins for that spelling; the other import remains available |
| Use aliases in a call whose failed candidate trial infers an omitted result | Rollback journals the canonical signature once and leaves no alias-specific inference state |

Regression tests: [import-bindings.test.ts](../../src/checker/import-bindings.test.ts).

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

### Independent Repair: Callable Requirement-Row Variance

Callable requirement rows now participate in nominal variance validation as invariant positions, as [types.variance.function](../../spec/lang/04-type-system.md#r-types.variance.function) requires.
The traversal resolves declaration and implementation binders while inspecting each row entry, then reuses the existing nominal argument traversal.
It applies to data fields, enum payloads, and readonly inherent method inputs and results regardless of visibility.
Unrelated concrete requirements remain valid.

This repair does not change callable type normalization, substitution, erasure, or the runtime provider ABI.
It also does not assign a polarity to a method's standalone requirement clause; that remains a separate specification audit question.

| Later fixture candidate | Required observation |
| --- | --- |
| Covariant `Box[T]` stores a callable with requirement `Cap[T]` | Reject `invalid-variance`; the callable row contains an invariant occurrence of `T` |
| Readonly inherent method receives that callable through a signed impl parameter | Apply the same row traversal regardless of method visibility |
| A method's standalone requirement clause mentions a signed impl parameter | Audit its specified polarity separately; do not infer a new rule from callable row invariance |

Regression tests: [variance.test.ts](../../src/checker/variance.test.ts).

Validation: full `pnpm run check`, all 90 source tests, website build, and fuzz smoke passed after rebasing onto `93dc7125`.

### Further Callable Edge Case: Generic Provider-Key Erasure

Status: Reproduced and fixed as an independent checker/backend repair.
Before the repair, a stored generic callable such as `Job[T].callback: fn() -> i32 $ Repo[T]` rejected a concrete `Repo[User]` callback after `Job[User]` substitution.
Substituting only the checker type was also unsound: Wasm adapters still named `Repo[generic:T]`, so a program that reached the backend trapped when the caller supplied `Repo[User]`.

Callable traversal now includes requirement-key types during resolution, substitution, generic detection, and inference.
Inference treats a requirement row as an unordered set: it tries structurally compatible key matches, validates the complete instantiated row, and commits only one unambiguous binder solution.
The HIR carries an explicit binder-to-type map at every erased data, call, trait-call, pattern, and suspension boundary.
Wasm adapters instantiate keys from that map and look providers up by concrete key; they never infer binder identity from sorted positions.

An additional edge appeared during the repair: two suspending instantiations can normalize to the same concrete callable result type while requiring opposite provider-slot permutations.
Attaching the map to a checker local would lose that distinction after a branch join.
Direct and dynamic-trait suspension frames therefore carry a call-site result adapter with the runtime value, and stored suspensions apply that adapter when their result is read.

| Regression case | Required observation |
| --- | --- |
| Store `fn() -> i32 $ Repo[T]` in `Job[T]`, instantiate `Job[User]`, and call under a `Repo[User]` provider | Type-check and return normally; the stored callable and adapter agree on the concrete provider key |
| Use two generic parameters in requirements in a different order from their declaration | Preserve binder identity rather than matching keys by position or sorting |
| Collapse `Repo[A] + Repo[B]` with `A = B` | Deduplicate the concrete row without losing the provider |
| Infer `A` and `B` from the unordered row `Repo[User] + Repo[Post]` | Report `cannot-infer-type`; do not select one of two valid permutations |
| Join stored suspensions whose concrete result types are equal but whose binder permutations differ | Each frame retains and applies its own result adapter |
| Pass and return such a callable through a dynamic suspending trait method | Adapt both the trait ABI and the stored suspension result using the concrete keys |

Regression tests: [generic-callable-requirements.test.ts](../../src/checker/generic-callable-requirements.test.ts).

Validation: full `pnpm run check`, all 102 source tests, website build, and fuzz smoke passed after rebasing onto `f007c257`.

### Independent Repair: Least Common Type

Before this repair, list and map literals used a candidate generator specialized to outer permission weakening and readonly `List` covariance.
Value-producing `if`, `match`, and inferred function results used a separate equality-or-function-row path.
None of those paths implemented optional injection, user-declared variance, readonly `Map` value variance, or general function variance consistently.

One declaration-aware least-common-type engine now owns every specified inference site.
It drops `never`, widens only direct function values to their top-level row union, solves nominal and function bounds structurally, and admits permission weakening, declared readonly variance, and one optional injection.
Declared variance recurses through user types, built-in collections, and function inputs and results while rejecting representation-changing optional or trait conversions.
The explicit prohibition on combining an outer permission weakening with a variance step remains part of candidate reachability.
Each generic argument is solved independently as a least or greatest representation-preserving bound, rather than materializing their Cartesian product.

Match checking now collects every arm type before choosing a result, rather than folding arms in source order.
Its final arm expressions are value contexts while inference is pending, so a must-use optional arm is not diagnosed as discarded.
Function-result discovery likewise joins all final and explicit return types at once; the normal second checking pass then applies the selected coercions.

The implementation deliberately keeps least-common-type inference distinct from generic-argument inference, which the specification restricts to permission weakening.
It also does not claim that all expected-type coercion paths share one representation yet.
String-encoded semantic types and the broader A02 conversion architecture therefore remain open.

| Regression case | Required observation |
| --- | --- |
| Join `i32` and `i32?` in a list, `if`, `match`, and inferred result | Infer `i32?`, insert `.Some` exactly once, and execute normally |
| Join readonly `Producer[mut User]` and `Producer[User]` | Use the declaration's `+T` marker at every inference site |
| Join readonly `Map[string, mut User]` and `Map[string, User]` | Apply built-in covariance only to the value argument |
| Join `fn(User) -> mut User` and `fn(mut User) -> User` | Apply contravariant inputs and covariant results together |
| Reorder three match arms whose two covariant arguments widen independently | Produce the same structural result regardless of source order |
| Join two 24-parameter covariant types with alternating mutable arguments | Solve per position within the test deadline; do not enumerate `2^24` candidate types |
| Join `i32` and `i32??` | Reject `no-common-type`; optional inference adds only one layer |
| Join `mut List[mut User]` and `List[User]` | Reject `no-least-common-type`; do not combine outer weakening with element variance |
| Use different requirement rows inside two nested lists | Reject; row union applies only to the direct function values at an inference site |

Regression tests: [least-common-type.test.ts](../../src/checker/least-common-type.test.ts).

Validation after rebasing onto `eafc840e`: full `pnpm run check`, all 107 source tests, website build, and fuzz smoke passed.
The existing `runtime/valid/lct-optional-injection.hd` known-failure case also checks successfully when invoked directly.
No specification or conformance fixture changed.

### Independent Repair: Local Implementation Extent

Status: T5 reproduced and fixed as one part of A01. Package declaration ownership and initialization scheduling remain open.

Before this repair, local declaration hoisting removed every local `impl` statement and appended its declaration to the module-wide implementation table. A method call before the declaration, and a call in a parent suite after an implementation in one child suite, therefore both resolved successfully. The checker retained lexical identities for local types and traits but no declaration-point availability for their implementations.

Local implementations still enter one global registry so overlap, ownership, and uniqueness checks see the complete program. Each now has a stable internal identity, while its source statement becomes a compile-time marker in the containing suite. Function checking maintains a stack of active implementation identities: reaching a marker activates the implementation for the rest of that suite and its children, and leaving the suite restores the parent set. Method lookup and trait-conformance lookup use filtered views of the global trait and inherent implementation registries.

Synthetic functions preserve the implementation set at the source expression's declaration point. This covers local implementation methods, local-trait defaults, data and enum defaults, and enum helper bodies. Closures inherit the active set where the closure is written while retaining access to the complete registry for declarations inside their own body. Default trait bodies use their trait declaration's environment plus the implementation being defined; unrelated implementations introduced later at the implementing declaration do not leak backward into the default body.

Supertrait and delegation preparation use the same declaration-point visibility relation. Supertrait dictionaries resolve their stored global implementation indices by stable identity rather than indexing a filtered array. Candidate transactions continue to exclude both complete implementation registries as immutable input; their lexical activation stack remains mutable checker state.

| Later fixture candidate | Required observation |
| --- | --- |
| Call a trait or inherent method before its local implementation | Reject lookup; a later declaration does not apply backward |
| Put the implementation in one child suite and call from its parent or sibling | Reject outside the declaring suite, while accepting calls inside it and its children |
| Define closures before and after the implementation, including one that escapes its child suite | Each closure retains the implementation environment at its own definition point |
| Coerce a local nominal value to a local trait value before and after the implementation | Trait conformance follows the same extent as method lookup |
| Use a local implementation in a data/enum default or local-trait default body | Resolve only implementations visible where that declaration body was written |
| Declare a required local supertrait implementation after versus before a child-trait implementation | The later prerequisite does not satisfy the earlier declaration; the earlier one does |
| Put overlapping implementations in disjoint lexical suites | Still reject globally; lexical scope does not create a second coherent pair |
| Call another method from the local implementation's own method body | The implementation sees itself and earlier visible implementations |

Regression tests: [local-implementation-extent.test.ts](../../src/checker/local-implementation-extent.test.ts).

Validation on `dca2cace`: full `TERM=xterm-256color pnpm run check`, all 121 source tests, website build, and fuzz smoke passed. The fuzz smoke produced no phase signatures; its existing non-gating contract signatures remain outside this repair.
No specification or conformance fixture changed.

### Repair Status Table


| Finding | Status | Repair and limits |
| --- | --- | --- |
| A01 | Partially fixed | Std submodule calls honor lexical value bindings, repeated aliases share one checker identity, and local implementations preserve declaration-point suite extent while remaining global for coherence. Package declaration ownership and initialization scheduling remain open. |
| A02 | Partially fixed | Public and private readonly inherent signatures, optional constructor boundaries, callable-row variance and erasure, and every specified least-common-type site are repaired. Expected-type coercion remains distributed, and semantic types remain string-encoded. |
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
The callable requirement-row repair closes the omitted row traversal without changing callable erasure.

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
