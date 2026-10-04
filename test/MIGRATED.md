# Migrated TypeScript Tests

These TypeScript tests now have implementation-neutral conformance fixtures
under `spec/conformance/`. Each line maps one test to its fixtures, or to the
existing fixture that already covered it. A test marked "not migrated" stays
in TypeScript, for the reason given. Delete a test only when its line names
fixtures or duplicates and no "not migrated" part. Remove a line once its
test is deleted, and delete this file when it is empty.

Paths are relative to `spec/conformance/`. Owner questions are listed at
the end.

## test/requirement-key-validation.test.ts

- test/requirement-key-validation.test.ts :: ordinary and generic unknown requirement keys are unknown traits -> typing/invalid/requirement-key-unknown-generic-trait.hd; the ordinary key is a duplicate of typing/invalid/requirement-key-unknown-trait.hd
- test/requirement-key-validation.test.ts :: requirement keys reject every source of dynamic unsafety -> typing/invalid/requirement-key-supertrait-not-dynamically-safe.hd, typing/invalid/requirement-key-generic-method.hd; the associated function is a duplicate of typing/invalid/requirement-key-not-dynamically-safe.hd
- test/requirement-key-validation.test.ts :: Inspectable subtraits are rejected at nested and provider key positions -> typing/invalid/inspectable-requirement-function-type.hd, typing/invalid/inspectable-requirement-provider-scope.hd
- test/requirement-key-validation.test.ts :: an unrelated user trait named Inspectable remains an ordinary key -> typing/valid/requirement-key-user-trait-named-inspectable.hd
- test/requirement-key-validation.test.ts :: reference-only and row method generics preserve dynamic safety -> typing/valid/requirement-key-dynamically-safe-generics.hd
- test/requirement-key-validation.test.ts :: requirement keys bind all reachable associated types -> typing/valid/requirement-key-supertrait-binding.hd; the unbound key is a duplicate of typing/invalid/requirement-key-unbound.hd, and the bound key of typing/valid/requirement-key-binding.hd
- test/requirement-key-validation.test.ts :: requirement keys diagnose arity and associated binding structure -> typing/invalid/requirement-key-missing-generic-argument.hd, typing/invalid/requirement-key-duplicate-binding.hd, typing/invalid/requirement-key-binding-unknown-type.hd; the unknown binding name is a duplicate of typing/invalid/requirement-key-unknown-binding.hd
- test/requirement-key-validation.test.ts :: rows nested inside associated binding values are validated -> typing/invalid/requirement-key-binding-row-unknown-trait.hd
- test/requirement-key-validation.test.ts :: unknown key argument types are rejected -> typing/invalid/requirement-key-unknown-argument-type.hd
- test/requirement-key-validation.test.ts :: trait-value key arguments have one canonical provider identity -> typing/valid/requirement-key-trait-value-argument.hd
- test/requirement-key-validation.test.ts :: callable type rows are validated through nested nominal types -> typing/invalid/function-type-row-unknown-trait-nested.hd, typing/invalid/local-annotation-row-unknown-trait.hd
- test/requirement-key-validation.test.ts :: data surfaces validate rows after every trait body is known -> typing/invalid/data-field-row-not-dynamically-safe.hd
- test/requirement-key-validation.test.ts :: row-kinded arguments validate their concrete keys -> typing/invalid/fn-row-argument-unknown-trait.hd
- test/requirement-key-validation.test.ts :: explicit closure rows use the same requirement-key validation -> typing/invalid/closure-row-unknown-trait.hd
- test/requirement-key-validation.test.ts :: provider bindings validate bare keys before their values -> typing/invalid/provider-scope-unknown-trait.hd
- test/requirement-key-validation.test.ts :: context types validate their key rows -> typing/invalid/context-row-unknown-trait.hd
- test/requirement-key-validation.test.ts :: trait method rows are validated even without an implementation -> typing/invalid/trait-method-row-unknown-trait.hd
- test/requirement-key-validation.test.ts :: marked row parameters remain symbolic rather than becoming trait keys -> duplicate of typing/valid/dynamic-safety-row-parameter.hd
- test/requirement-key-validation.test.ts :: aliases are expanded before requirement keys are validated -> typing/invalid/alias-unknown-target.hd, typing/invalid/row-alias-unknown-key.hd (Q5, ALIAS-MISSING); the TS test asserts a result the decision makes wrong: it expects `unknown-trait` at the use `$ MissingAlias`, and the spec gives `unknown-type` at the alias declaration

## test/dynamic-safety-nested.test.ts

- test/dynamic-safety-nested.test.ts :: local annotations validate direct and nested trait values -> typing/invalid/local-annotation-not-dynamically-safe.hd, typing/invalid/nested-local-annotation-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: signature types validate every nested value position -> typing/invalid/list-parameter-not-dynamically-safe.hd, typing/invalid/tuple-option-parameter-not-dynamically-safe.hd, typing/invalid/callback-parameter-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: data and enum fields validate after every trait body is defined -> typing/invalid/data-field-not-dynamically-safe.hd, typing/invalid/enum-payload-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: trait method types are validated once after trait definition -> typing/invalid/trait-method-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: associated binding values are nested value types -> typing/invalid/associated-binding-value-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: function bounds and requirement keys validate their nested value types -> typing/invalid/bound-argument-not-dynamically-safe.hd, typing/invalid/bound-binding-not-dynamically-safe.hd, typing/invalid/requirement-key-argument-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: declaration bounds validate after every trait body is defined -> typing/invalid/data-bound-not-dynamically-safe.hd, typing/invalid/enum-bound-not-dynamically-safe.hd, typing/invalid/trait-parameter-bound-not-dynamically-safe.hd, typing/invalid/method-bound-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: an implementation bound is validated even when the impl has no methods -> typing/invalid/impl-bound-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: lowered alias and newtype bounds retain their written value types -> typing/invalid/alias-bound-not-dynamically-safe.hd, typing/invalid/newtype-bound-not-dynamically-safe.hd
- test/dynamic-safety-nested.test.ts :: nested dynamically safe traits remain valid -> typing/valid/nested-dynamically-safe-trait-values.hd

## test/dynamic-safety-row-kind.test.ts

- test/dynamic-safety-row-kind.test.ts :: a type parameter after a callable row is not mistaken for a row parameter -> typing/invalid/trait-value-row-then-type-parameter.hd
- test/dynamic-safety-row-kind.test.ts :: dynamic safety is independent of tuple element order -> typing/invalid/trait-value-type-parameter-then-row.hd
- test/dynamic-safety-row-kind.test.ts :: a declared method row parameter remains dynamically safe -> duplicate of typing/valid/dynamic-safety-row-parameter.hd

## test/value-category-supertraits.test.ts

- test/value-category-supertraits.test.ts :: Error rejects an AnyVal newtype target -> duplicate of typing/invalid/error-anyval-newtype.hd
- test/value-category-supertraits.test.ts :: category supertraits are enforced without knowing the trait by name -> typing/invalid/anyref-subtrait-value-newtype.hd, typing/valid/anyref-subtrait-reference-newtypes.hd
- test/value-category-supertraits.test.ts :: transitively implied AnyRef bounds preserve dynamic safety -> typing/valid/dynamic-safety-transitive-anyref.hd; the `Error` bound is a duplicate of typing/valid/dynamic-safety-implied-anyref.hd
- test/value-category-supertraits.test.ts :: an implied AnyVal method parameter remains dynamically unsafe -> typing/invalid/dynamic-safety-implied-anyval.hd
- test/value-category-supertraits.test.ts :: generic newtypes prove category supertraits through their impl bounds -> typing/invalid/anyref-subtrait-unbounded-generic-newtype.hd, typing/valid/anyref-subtrait-reference-newtypes.hd
- test/value-category-supertraits.test.ts :: top-level generic signatures retain implied reference categories -> typing/valid/identity-implied-anyref-bound.hd

## test/standard-ownership.test.ts

The fixtures use the language-tier `std.inspect.TypeId` and the prelude
`ConsoleError` in place of the stdlib-tier `std.time.Duration`.

- test/standard-ownership.test.ts :: a user cannot implement a standard trait for a standard data type -> typing/invalid/orphan-impl-standard-data.hd
- test/standard-ownership.test.ts :: a user cannot implement a standard trait for a standard enum -> typing/invalid/orphan-impl-standard-enum.hd
- test/standard-ownership.test.ts :: a user owns its data type when implementing a standard trait -> duplicate of typing/valid/prelude-surface.hd
- test/standard-ownership.test.ts :: a user-owned trait may be implemented for a standard type -> typing/valid/user-trait-for-standard-type.hd

## test/variance.test.ts

- test/variance.test.ts :: distinct method binders preserve runtime arguments, dictionaries and captured values -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd, typing/invalid/local-value-reuses-type-parameter.hd (Q1 and Q2, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects the program to compile and run, and `fn echo[T]` inside `impl[T]` and `let T = value` are each `duplicate-binding`
- test/variance.test.ts :: readonly public method inputs participate in nominal variance -> duplicate of typing/invalid/covariant-private-method-parameter.hd
- test/variance.test.ts :: readonly public method results participate in nominal variance -> typing/invalid/contravariant-method-result.hd
- test/variance.test.ts :: nested function parameter polarity reverses rather than becoming invariant -> typing/valid/covariant-callback-parameter.hd, typing/invalid/covariant-producer-callback-parameter.hd
- test/variance.test.ts :: mutable method signature positions are invariant -> typing/invalid/covariant-mut-method-parameter.hd
- test/variance.test.ts :: invariant method signature containers cannot hide variance violations -> typing/invalid/covariant-optional-method-parameter.hd
- test/variance.test.ts :: callable requirement rows are invariant in data and enum surfaces -> typing/invalid/covariant-data-requirement-row.hd, typing/invalid/contravariant-enum-requirement-row.hd
- test/variance.test.ts :: callable requirement rows stay invariant through method polarity -> typing/invalid/covariant-method-requirement-row.hd, typing/invalid/contravariant-method-requirement-row.hd
- test/variance.test.ts :: callable requirements unrelated to the nominal parameter remain valid -> typing/valid/covariant-unrelated-requirement-row.hd
- test/variance.test.ts :: method generic binders do not capture implementation parameters in Self -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd (Q1, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects no error for `fn echo[T]` and `fn copy[T]` inside `impl[T]`, and only `invalid-variance` for `fn consume[T]`, and each binder is `duplicate-binding`
- test/variance.test.ts :: nested implementation targets compose their declared variance signs -> typing/valid/variance-contravariant-target-parameter.hd, typing/invalid/variance-contravariant-target-result.hd (Q3, VARIANCE-NONBARE)
- test/variance.test.ts :: an invariant implementation target does not impose signed method constraints -> typing/valid/variance-invariant-target.hd (Q3, VARIANCE-NONBARE)
- test/variance.test.ts :: opposing target signs constrain a shared parameter to equality -> typing/valid/variance-opposing-target-signs.hd (Q3, VARIANCE-NONBARE)
- test/variance.test.ts :: trait arguments in method signatures retain their invariant polarity -> typing/invalid/covariant-trait-argument-parameter.hd
- test/variance.test.ts :: a separate trait implementation does not change nominal variance -> typing/valid/variance-separate-trait-impl.hd
- test/variance.test.ts :: enum inherent methods are included in the readonly public surface -> typing/invalid/covariant-enum-method-parameter.hd
- test/variance.test.ts :: suspending inherent methods have the same variance obligations -> typing/invalid/covariant-suspending-method-parameter.hd
- test/variance.test.ts :: associated construction and mutable receiver signatures are not readonly instance views -> not migrated (partial: the `mut self` method is typing/invalid/covariant-mut-self-method-parameter.hd, Q4, VARIANCE-MUT-SELF, and the TS test asserts a result the decision makes wrong, since it expects no error for `pub fn set(mut self, value: U)` on `Box[+T]`; the associated function `pub fn new(value: U) -> Self` waits for owner question Q9)
- test/variance.test.ts :: a public method cannot infer its result past the variance check -> duplicate of typing/invalid/public-method-missing-result-type.hd
- test/variance.test.ts :: private readonly inherent methods participate in declared variance -> duplicate of typing/invalid/covariant-private-method-parameter.hd and typing/invalid/contravariant-method-result.hd
- test/variance.test.ts :: inferred private inherent results cannot bypass variance checking -> typing/invalid/contravariant-inferred-private-result.hd, typing/valid/covariant-inferred-private-result.hd
- test/variance.test.ts :: shadowed method binders cannot erase Self-derived invariant inferred results -> typing/invalid/contravariant-inferred-invariant-result.hd covers the binder `V`; the binder `T` is typing/invalid/method-type-parameter-reuses-impl-parameter.hd (Q1, SHADOW-TPARAM), and the TS test asserts a result the decision makes wrong, since it expects only `invalid-variance` for `fn hidden[T]` inside `impl[T]`
- test/variance.test.ts :: method-owned inferred and callable results remain independent of the receiver binder -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd (Q1, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects no error, and each method binder `T` inside `impl[T]` is `duplicate-binding`
- test/variance.test.ts :: method binder normalization preserves defaults, bounds and nested declaration scope -> not migrated (implementation detail: inspects the checker's binder renaming)
- test/variance.test.ts :: structural binder renaming retains type heads and labels while visiting rows and projections -> not migrated (implementation detail: inspects the checker's binder renaming)
- test/variance.test.ts :: a local nominal shadows a method binder from its declaration point without leaking out -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd, typing/invalid/local-data-reuses-type-parameter.hd (Q1 and Q2, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects only `unused-local-binding`, and `fn work[T]` inside `impl[T]` and the local `data T` are each `duplicate-binding`
- test/variance.test.ts :: qualified type owners rename binders but preserve unrelated owners -> not migrated (implementation detail: inspects the checker's binder renaming)
- test/variance.test.ts :: internal method identities retain Unicode binder spellings in diagnostics -> not migrated (implementation detail: diagnostic message text)
- test/variance.test.ts :: qualified owner resolution distinguishes type binders from lexical values -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd, typing/invalid/local-value-reuses-type-parameter.hd (Q1 and Q2, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects no error, and each method binder `T` inside `impl[T]`, each `let T = value`, and the pattern binding `T` are `duplicate-binding`
- test/variance.test.ts :: nested closure type scopes do not change later method annotations -> not migrated (implementation detail: inspects the checker's binder renaming)

## test/callable-storage.test.ts

- test/callable-storage.test.ts :: generic lists invoke stored callables -> runtime/valid/generic-storage-invokes-callables.hd
- test/callable-storage.test.ts :: generic callable-list results retain their concrete callable ABI -> runtime/valid/generic-storage-invokes-callables.hd
- test/callable-storage.test.ts :: generic lists invoke callables with erased inputs -> runtime/valid/generic-storage-invokes-callables.hd
- test/callable-storage.test.ts :: nested generic storage uses the same callable representation -> runtime/valid/generic-storage-invokes-callables.hd
- test/callable-storage.test.ts :: generic tuples invoke stored callables -> runtime/valid/generic-storage-invokes-callables.hd
- test/callable-storage.test.ts :: generic maps invoke stored callables -> runtime/valid/generic-storage-invokes-callables.hd
- test/callable-storage.test.ts :: callable storage preserves mutable list identity -> runtime/valid/generic-storage-callable-list-identity.hd
- test/callable-storage.test.ts :: generic provider rows survive callable storage -> runtime/valid/row-parameter-callable-in-list.hd
- test/callable-storage.test.ts :: generic lists invoke suspending callables after pending -> not migrated (needs pending-first-poll scenario)

## test/generic-callable-requirements.test.ts

- test/generic-callable-requirements.test.ts :: a generic field substitutes its callable requirement key through storage -> runtime/valid/generic-requirement-key-substitution.hd
- test/generic-callable-requirements.test.ts :: callable rows infer generic data arguments from requirement keys -> runtime/valid/generic-inference-from-requirement-key.hd
- test/generic-callable-requirements.test.ts :: generic calls infer type arguments from callable requirement keys -> runtime/valid/generic-inference-from-requirement-key.hd
- test/generic-callable-requirements.test.ts :: generic callable results restore concrete requirement keys -> runtime/valid/generic-requirement-key-substitution.hd
- test/generic-callable-requirements.test.ts :: suspending generic callable results retain their requirement substitutions -> runtime/valid/generic-requirement-key-substitution.hd
- test/generic-callable-requirements.test.ts :: stored suspensions carry each call site's provider permutation -> runtime/valid/requirement-row-order-stored-suspension.hd
- test/generic-callable-requirements.test.ts :: dynamic trait suspensions carry concrete provider-key substitutions -> runtime/valid/requirement-row-order-trait-value.hd
- test/generic-callable-requirements.test.ts :: dynamic trait calls adapt generic callable parameters and results -> runtime/valid/requirement-row-order-trait-value.hd
- test/generic-callable-requirements.test.ts :: provider adaptation follows binder identity across reordered rows -> runtime/valid/requirement-row-order-data-field.hd
- test/generic-callable-requirements.test.ts :: two generic keys may collapse to one concrete row during adaptation -> runtime/valid/requirement-row-duplicate-after-substitution.hd
- test/generic-callable-requirements.test.ts :: unordered requirement keys do not guess an ambiguous binder mapping -> typing/invalid/ambiguous-requirement-key-solution.hd, typing/valid/ambiguous-requirement-key-annotated.hd (Q6, AMBIGUOUS-TYPE); the TS test asserts a result the decision makes wrong: it expects `cannot-infer-type`, and the spec gives `ambiguous-type`
- test/generic-callable-requirements.test.ts :: enum payload extraction retains generic callable provider substitutions -> runtime/valid/generic-requirement-key-substitution.hd

## test/inspectable-forwarding.test.ts

- test/inspectable-forwarding.test.ts :: forwarded inspection distinguishes dynamic and static type identity -> runtime/valid/inspectable-dynamic-vs-static-identity.hd
- test/inspectable-forwarding.test.ts :: a dynamic trait is an exact generic downcast target -> runtime/valid/inspectable-generic-downcast-targets.hd
- test/inspectable-forwarding.test.ts :: concrete generic downcasts still recover the same reference -> runtime/valid/inspectable-generic-downcast-targets.hd
- test/inspectable-forwarding.test.ts :: an Error bound supplies transitive Inspectable evidence -> runtime/valid/error-bound-downcast-evidence.hd
- test/inspectable-forwarding.test.ts :: nested static identity retains a mutable dynamic trait argument -> runtime/valid/typeid-nested-mut-trait-argument.hd; it compares `TypeId` values instead of the printed name, because the spec prints a fixture-declared trait by its absolute qualified name

## test/standard-module-values.test.ts

- test/standard-module-values.test.ts :: a standard module member is a function value -> duplicate of runtime/valid/module-qualified-function-value.hd
- test/standard-module-values.test.ts :: a renamed standard module resolves member values -> runtime/valid/module-alias-function-value.hd
- test/standard-module-values.test.ts :: an expected function type instantiates a generic module member -> runtime/valid/module-qualified-generic-function-value.hd
- test/standard-module-values.test.ts :: explicit type arguments instantiate a generic module member value -> runtime/valid/module-qualified-generic-function-value.hd
- test/standard-module-values.test.ts :: a parenthesized module member remains an ordinary function value -> duplicate of runtime/valid/module-qualified-function-value-parenthesized.hd
- test/standard-module-values.test.ts :: module selection finds a function bound through the prelude -> runtime/valid/module-qualified-prelude-function.hd
- test/standard-module-values.test.ts :: module selection finds the same directly imported declaration -> runtime/valid/module-qualified-beside-direct-use.hd
- test/standard-module-values.test.ts :: module selection finds the same declaration imported under an alias -> runtime/valid/module-qualified-beside-direct-use.hd
- test/standard-module-values.test.ts :: a local value shadows a standard module namespace -> runtime/valid/local-shadows-module-namespace.hd
- test/standard-module-values.test.ts :: a module namespace does not expose a private standard function -> typing/invalid/module-path-private-std-function.hd (Q7, PRIVATE-STD); the TS test asserts a result the decision makes wrong: it expects `unknown-name`, and the spec gives `private-import`

## test/local-implementation-extent.test.ts

- test/local-implementation-extent.test.ts :: a local trait implementation is unavailable before its declaration -> typing/invalid/local-impl-before-declaration.hd
- test/local-implementation-extent.test.ts :: a child-suite implementation does not leak into its parent suite -> typing/invalid/local-impl-child-suite-not-parent.hd
- test/local-implementation-extent.test.ts :: a local trait implementation works after its declaration -> runtime/valid/local-impl-visible-after-declaration.hd
- test/local-implementation-extent.test.ts :: a local inherent implementation has the same lexical extent -> typing/invalid/local-inherent-impl-before-declaration.hd, runtime/valid/local-impl-visible-after-declaration.hd
- test/local-implementation-extent.test.ts :: trait-value conformance follows local implementation extent -> typing/invalid/local-impl-trait-value-before-declaration.hd, runtime/valid/local-impl-known-after-declaration.hd
- test/local-implementation-extent.test.ts :: closures inherit implementations visible where the closure is written -> typing/invalid/local-impl-closure-before-declaration.hd, runtime/valid/local-impl-visible-after-declaration.hd
- test/local-implementation-extent.test.ts :: local implementations remain global for overlap checking -> typing/invalid/local-impl-sibling-overlap.hd
- test/local-implementation-extent.test.ts :: an implementation method can use its own local implementation -> runtime/valid/local-impl-visible-after-declaration.hd
- test/local-implementation-extent.test.ts :: a local supertrait implementation must already be visible -> typing/invalid/local-supertrait-impl-after-child.hd, runtime/valid/local-impl-known-after-declaration.hd
- test/local-implementation-extent.test.ts :: local declaration defaults keep their declaration-point implementation scope -> typing/invalid/local-impl-after-field-default.hd, runtime/valid/local-impl-visible-after-declaration.hd
- test/local-implementation-extent.test.ts :: local trait defaults keep their declaration-point implementation scope -> typing/invalid/local-impl-after-trait-default.hd, runtime/valid/local-impl-visible-after-declaration.hd

## test/least-common-type.test.ts

- test/least-common-type.test.ts :: optional injection uses one LCT across literals, if, match, and inferred results -> duplicate of runtime/valid/lct-optional-injection.hd
- test/least-common-type.test.ts :: declared and built-in variance contribute at every shared LCT site -> typing/valid/lct-variance-every-site.hd
- test/least-common-type.test.ts :: LCT remains order-independent when variance arguments need a structural join -> typing/valid/lct-structural-join-order.hd
- test/least-common-type.test.ts :: wide declarations solve variance positions without a candidate product -> typing/valid/lct-wide-variance-join.hd; the 5-second time limit is an implementation goal, and the suite's own limit is 10 seconds
- test/least-common-type.test.ts :: LCT rejects two optional layers and distinguishes a forbidden combined step -> duplicate of typing/invalid/lct-optional-two-layers.hd and typing/invalid/least-type-weakening-variance.hd

## test/contextual-data-fields.test.ts

- test/contextual-data-fields.test.ts :: known mutable field types contextualize empty collection literals -> typing/valid/empty-literal-field-context.hd
- test/contextual-data-fields.test.ts :: explicit data arguments contextualize empty mutable fields -> typing/valid/empty-literal-field-context.hd
- test/contextual-data-fields.test.ts :: mutable generic data context infers nested data arguments -> typing/valid/nested-data-argument-from-field-type.hd
- test/contextual-data-fields.test.ts :: inference-only field context reaches empty literals through control flow -> typing/valid/empty-literal-field-context.hd
- test/contextual-data-fields.test.ts :: an unresolved field type does not guess an empty literal's element type -> typing/invalid/empty-literal-generic-field-unsolved.hd
- test/contextual-data-fields.test.ts :: a readonly value can still initialize a direct mutable field -> typing/valid/readonly-value-direct-mut-field.hd
- test/contextual-data-fields.test.ts :: an inference hint does not require nested data to be mutable -> typing/valid/readonly-part-nested-data-literal.hd

## test/typed-derivation.test.ts

- test/typed-derivation.test.ts :: a derived walk passes each member with its facts -> duplicate of runtime/valid/typed-derivation-walk.hd
- test/typed-derivation.test.ts :: a derivation block's member lines edit only its own derivation -> duplicate of runtime/valid/typed-derivation-walk.hd
- test/typed-derivation.test.ts :: enum, generic, and embedded targets walk their members -> runtime/valid/typed-derivation-embedded-generic-walk.hd; the enum target is a duplicate of runtime/valid/typed-derivation-walk.hd
- test/typed-derivation.test.ts :: a derived build fills members from their defaults -> runtime/valid/typed-derivation-build-defaults.hd
- test/typed-derivation.test.ts :: @derive(Eq) compares data and enum members -> runtime/valid/derived-equality-members.hd; typing/valid/derived-equality.hd checks the same program at the type phase only
- test/typed-derivation.test.ts :: typed derivation reports its diagnostics at the opt-in -> duplicates of typing/invalid/derive-member-not-derivable.hd, typing/invalid/derive-error-trait.hd, typing/invalid/unknown-annotation-member.hd, typing/invalid/omitted-member-without-default.hd, and typing/invalid/derive-before-function.hd; `@derive(Missing)` is typing/invalid/derive-unknown-trait.hd (Q8, DERIVE-MISSING), and the TS test asserts a result the decision makes wrong, since it expects `underivable-trait` and the spec gives `unknown-trait`
- test/typed-derivation.test.ts :: @derive(Debug) on a newtype needs its base type's Debug and applies it -> runtime/valid/derived-debug-newtype.hd; the missing base trait is a duplicate of typing/invalid/derive-newtype-base-missing-trait.hd

## test/test-runner-requirements.test.ts

- test/test-runner-requirements.test.ts :: a plain test body receives the TestRunner capability -> duplicate of typing/valid/test-body-uses-test-runner.hd; the checker's requirement list that it also inspects is an implementation detail
- test/test-runner-requirements.test.ts :: a timed test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd
- test/test-runner-requirements.test.ts :: a table test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd
- test/test-runner-requirements.test.ts :: a property test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd
- test/test-runner-requirements.test.ts :: a property with a generator test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd
- test/test-runner-requirements.test.ts :: a unit test body does not receive PropertyRunner -> duplicate of typing/invalid/test-body-uses-property-runner.hd

## Owner Questions

Q1 to Q8 are answered (owner, 2026-10-04); each answer is in the spec,
and each line above names the fixture that covers it.

| Question | Answer | Spec |
| --- | --- | --- |
| Q1. May a method's own type parameter reuse its `impl`'s parameter name? | No: `fn echo[T]` inside `impl[T] Box[T]` is `duplicate-binding` (SHADOW-TPARAM) | [`names.type-param.no-redeclare.method`](../spec/lang/03-names-and-scopes.md#r-names.type-param.no-redeclare.method) |
| Q2. May a local name in a body reuse a type parameter's name? | No: a local `data T` or value `T` inside `fn work[T]` is `duplicate-binding` (SHADOW-TPARAM) | [`names.type-param.no-redeclare.body`](../spec/lang/03-names-and-scopes.md#r-names.type-param.no-redeclare.body) |
| Q3. How is a method on a non-bare target checked against variance? | By each `impl` parameter's variance in the target, composed as Kotlin and Scala do (VARIANCE-NONBARE) | [Variance On A Non-Bare Target](../spec/lang/04-type-system.md#variance-on-a-non-bare-target) |
| Q4. Do `mut self` methods count toward declared variance? | Yes: `pub fn set(mut self, value: U)` on `Box[+T]` is `invalid-variance` (VARIANCE-MUT-SELF) | [`types.variance.surface.mut-self`](../spec/lang/04-type-system.md#r-types.variance.surface.mut-self) |
| Q5. Is an alias whose target names nothing an error at the alias? | Yes, used or not: `unknown-type`, or `unknown-trait` for a row key (ALIAS-MISSING) | [`types.alias.target-unknown`](../spec/lang/04-type-system.md#r-types.alias.target-unknown) |
| Q6. Which code does an ambiguous generic solution report? | The new code `ambiguous-type`; `cannot-infer-type` stays for no solution (AMBIGUOUS-TYPE) | [`types.infer.ambiguous.code`](../spec/lang/04-type-system.md#r-types.infer.ambiguous.code) |
| Q7. Which code does a module path to a private std function report? | `private-import`, as for any module (PRIVATE-STD) | [`expr.name.qualified.private`](../spec/lang/05-expressions.md#r-expr.name.qualified.private) |
| Q8. Which code does `@derive(Missing)` report? | `unknown-trait`; `underivable-trait` stays for a real trait with no template (DERIVE-MISSING) | [`annot.derive.unknown`](../spec/lang/14-annotations.md#r-annot.derive.unknown) |

- **Q9.** Does an inherent associated function count toward declared
  variance? The test accepts `pub fn new(value: U) -> Self` on `Box[+T]`.
  [`types.variance.surface`](../spec/lang/04-type-system.md#r-types.variance.surface)
  names every inherent method, and
  [`grammar.trait.method-kind`](../spec/lang/02-grammar.md#r-grammar.trait.method-kind)
  calls a receiverless member an associated function, not a method.
