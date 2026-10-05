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

- test/requirement-key-validation.test.ts :: ordinary and generic unknown requirement keys are unknown traits -> typing/invalid/requirement-key-unknown-generic-trait.hd; the ordinary key is a duplicate of typing/invalid/requirement-key-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: requirement keys reject every source of dynamic unsafety -> typing/invalid/requirement-key-supertrait-not-dynamically-safe.hd, typing/invalid/requirement-key-generic-method.hd; the associated function is a duplicate of typing/invalid/requirement-key-not-dynamically-safe.hd [deleted]
- test/requirement-key-validation.test.ts :: Inspectable subtraits are rejected at nested and provider key positions -> typing/invalid/inspectable-requirement-function-type.hd, typing/invalid/inspectable-requirement-provider-scope.hd [deleted]
- test/requirement-key-validation.test.ts :: an unrelated user trait named Inspectable remains an ordinary key -> typing/valid/requirement-key-user-trait-named-inspectable.hd [deleted]
- test/requirement-key-validation.test.ts :: reference-only and row method generics preserve dynamic safety -> typing/valid/requirement-key-dynamically-safe-generics.hd [deleted]
- test/requirement-key-validation.test.ts :: requirement keys bind all reachable associated types -> typing/valid/requirement-key-supertrait-binding.hd; the unbound key is a duplicate of typing/invalid/requirement-key-unbound.hd, and the bound key of typing/valid/requirement-key-binding.hd [deleted]
- test/requirement-key-validation.test.ts :: requirement keys diagnose arity and associated binding structure -> typing/invalid/requirement-key-missing-generic-argument.hd, typing/invalid/requirement-key-duplicate-binding.hd, typing/invalid/requirement-key-binding-unknown-type.hd; the unknown binding name is a duplicate of typing/invalid/requirement-key-unknown-binding.hd [deleted]
- test/requirement-key-validation.test.ts :: rows nested inside associated binding values are validated -> typing/invalid/requirement-key-binding-row-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: unknown key argument types are rejected -> typing/invalid/requirement-key-unknown-argument-type.hd [deleted]
- test/requirement-key-validation.test.ts :: trait-value key arguments have one canonical provider identity -> typing/valid/requirement-key-trait-value-argument.hd [deleted]
- test/requirement-key-validation.test.ts :: callable type rows are validated through nested nominal types -> typing/invalid/function-type-row-unknown-trait-nested.hd, typing/invalid/local-annotation-row-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: data surfaces validate rows after every trait body is known -> typing/invalid/data-field-row-not-dynamically-safe.hd [deleted]
- test/requirement-key-validation.test.ts :: row-kinded arguments validate their concrete keys -> typing/invalid/fn-row-argument-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: explicit closure rows use the same requirement-key validation -> typing/invalid/closure-row-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: provider bindings validate bare keys before their values -> typing/invalid/provider-scope-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: context types validate their key rows -> typing/invalid/context-row-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: trait method rows are validated even without an implementation -> typing/invalid/trait-method-row-unknown-trait.hd [deleted]
- test/requirement-key-validation.test.ts :: marked row parameters remain symbolic rather than becoming trait keys -> duplicate of typing/valid/dynamic-safety-row-parameter.hd [deleted]
- test/requirement-key-validation.test.ts :: aliases are expanded before requirement keys are validated -> typing/invalid/alias-unknown-target.hd, typing/invalid/row-alias-unknown-key.hd (Q5, ALIAS-MISSING); the TS test asserts a result the decision makes wrong: it expects `unknown-trait` at the use `$ MissingAlias`, and the spec gives `unknown-type` at the alias declaration

## test/dynamic-safety-nested.test.ts

- test/dynamic-safety-nested.test.ts :: local annotations validate direct and nested trait values -> typing/invalid/local-annotation-not-dynamically-safe.hd, typing/invalid/nested-local-annotation-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: signature types validate every nested value position -> typing/invalid/list-parameter-not-dynamically-safe.hd, typing/invalid/tuple-option-parameter-not-dynamically-safe.hd, typing/invalid/callback-parameter-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: data and enum fields validate after every trait body is defined -> typing/invalid/data-field-not-dynamically-safe.hd, typing/invalid/enum-payload-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: trait method types are validated once after trait definition -> typing/invalid/trait-method-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: associated binding values are nested value types -> typing/invalid/associated-binding-value-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: function bounds and requirement keys validate their nested value types -> typing/invalid/bound-argument-not-dynamically-safe.hd, typing/invalid/bound-binding-not-dynamically-safe.hd, typing/invalid/requirement-key-argument-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: declaration bounds validate after every trait body is defined -> typing/invalid/data-bound-not-dynamically-safe.hd, typing/invalid/enum-bound-not-dynamically-safe.hd, typing/invalid/trait-parameter-bound-not-dynamically-safe.hd, typing/invalid/method-bound-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: an implementation bound is validated even when the impl has no methods -> typing/invalid/impl-bound-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: lowered alias and newtype bounds retain their written value types -> typing/invalid/alias-bound-not-dynamically-safe.hd, typing/invalid/newtype-bound-not-dynamically-safe.hd [deleted]
- test/dynamic-safety-nested.test.ts :: nested dynamically safe traits remain valid -> typing/valid/nested-dynamically-safe-trait-values.hd [deleted]

## test/dynamic-safety-row-kind.test.ts

- test/dynamic-safety-row-kind.test.ts :: a type parameter after a callable row is not mistaken for a row parameter -> typing/invalid/trait-value-row-then-type-parameter.hd [deleted]
- test/dynamic-safety-row-kind.test.ts :: dynamic safety is independent of tuple element order -> typing/invalid/trait-value-type-parameter-then-row.hd [deleted]
- test/dynamic-safety-row-kind.test.ts :: a declared method row parameter remains dynamically safe -> duplicate of typing/valid/dynamic-safety-row-parameter.hd [deleted]

## test/value-category-supertraits.test.ts

- test/value-category-supertraits.test.ts :: Error rejects an AnyVal newtype target -> duplicate of typing/invalid/error-anyval-newtype.hd [deleted]
- test/value-category-supertraits.test.ts :: category supertraits are enforced without knowing the trait by name -> typing/invalid/anyref-subtrait-value-newtype.hd, typing/valid/anyref-subtrait-reference-newtypes.hd [deleted]
- test/value-category-supertraits.test.ts :: transitively implied AnyRef bounds preserve dynamic safety -> typing/valid/dynamic-safety-transitive-anyref.hd; the `Error` bound is a duplicate of typing/valid/dynamic-safety-implied-anyref.hd [deleted]
- test/value-category-supertraits.test.ts :: an implied AnyVal method parameter remains dynamically unsafe -> typing/invalid/dynamic-safety-implied-anyval.hd [deleted]
- test/value-category-supertraits.test.ts :: generic newtypes prove category supertraits through their impl bounds -> typing/invalid/anyref-subtrait-unbounded-generic-newtype.hd, typing/valid/anyref-subtrait-reference-newtypes.hd [deleted]
- test/value-category-supertraits.test.ts :: top-level generic signatures retain implied reference categories -> typing/valid/identity-implied-anyref-bound.hd [deleted]

## test/standard-ownership.test.ts

The fixtures use the language-tier `std.inspect.TypeId` and the prelude
`ConsoleError` in place of the stdlib-tier `std.time.Duration`.

- test/standard-ownership.test.ts :: a user cannot implement a standard trait for a standard data type -> typing/invalid/orphan-impl-standard-data.hd [deleted]
- test/standard-ownership.test.ts :: a user cannot implement a standard trait for a standard enum -> typing/invalid/orphan-impl-standard-enum.hd [deleted]
- test/standard-ownership.test.ts :: a user owns its data type when implementing a standard trait -> duplicate of typing/valid/prelude-surface.hd [deleted]
- test/standard-ownership.test.ts :: a user-owned trait may be implemented for a standard type -> typing/valid/user-trait-for-standard-type.hd [deleted]

## test/variance.test.ts

- test/variance.test.ts :: distinct method binders preserve runtime arguments, dictionaries and captured values -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd, typing/invalid/local-value-reuses-type-parameter.hd (Q1 and Q2, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects the program to compile and run, and `fn echo[T]` inside `impl[T]` and `let T = value` are each `duplicate-binding`
- test/variance.test.ts :: readonly public method inputs participate in nominal variance -> duplicate of typing/invalid/covariant-private-method-parameter.hd [deleted]
- test/variance.test.ts :: readonly public method results participate in nominal variance -> typing/invalid/contravariant-method-result.hd [deleted]
- test/variance.test.ts :: nested function parameter polarity reverses rather than becoming invariant -> typing/valid/covariant-callback-parameter.hd, typing/invalid/covariant-producer-callback-parameter.hd [deleted]
- test/variance.test.ts :: mutable method signature positions are invariant -> typing/invalid/covariant-mut-method-parameter.hd [deleted]
- test/variance.test.ts :: invariant method signature containers cannot hide variance violations -> typing/invalid/covariant-optional-method-parameter.hd [deleted]
- test/variance.test.ts :: callable requirement rows are invariant in data and enum surfaces -> typing/invalid/covariant-data-requirement-row.hd, typing/invalid/contravariant-enum-requirement-row.hd [deleted]
- test/variance.test.ts :: callable requirement rows stay invariant through method polarity -> typing/invalid/covariant-method-requirement-row.hd, typing/invalid/contravariant-method-requirement-row.hd [deleted]
- test/variance.test.ts :: callable requirements unrelated to the nominal parameter remain valid -> typing/valid/covariant-unrelated-requirement-row.hd [deleted]
- test/variance.test.ts :: method generic binders do not capture implementation parameters in Self -> typing/invalid/method-type-parameter-reuses-impl-parameter.hd (Q1, SHADOW-TPARAM); the TS test asserts a result the decision makes wrong: it expects no error for `fn echo[T]` and `fn copy[T]` inside `impl[T]`, and only `invalid-variance` for `fn consume[T]`, and each binder is `duplicate-binding`
- test/variance.test.ts :: nested implementation targets compose their declared variance signs -> typing/valid/variance-contravariant-target-parameter.hd, typing/invalid/variance-contravariant-target-result.hd (Q3, VARIANCE-NONBARE) [deleted]
- test/variance.test.ts :: an invariant implementation target does not impose signed method constraints -> typing/valid/variance-invariant-target.hd (Q3, VARIANCE-NONBARE) [deleted]
- test/variance.test.ts :: opposing target signs constrain a shared parameter to equality -> typing/valid/variance-opposing-target-signs.hd (Q3, VARIANCE-NONBARE) [deleted]
- test/variance.test.ts :: trait arguments in method signatures retain their invariant polarity -> typing/invalid/covariant-trait-argument-parameter.hd [deleted]
- test/variance.test.ts :: a separate trait implementation does not change nominal variance -> typing/valid/variance-separate-trait-impl.hd [deleted]
- test/variance.test.ts :: enum inherent methods are included in the readonly public surface -> typing/invalid/covariant-enum-method-parameter.hd [deleted]
- test/variance.test.ts :: suspending inherent methods have the same variance obligations -> typing/invalid/covariant-suspending-method-parameter.hd [deleted]
- test/variance.test.ts :: associated construction and mutable receiver signatures are not readonly instance views -> not migrated (partial: the `mut self` method is typing/invalid/covariant-mut-self-method-parameter.hd, Q4, VARIANCE-MUT-SELF, and the TS test asserts a result the decision makes wrong, since it expects no error for `pub fn set(mut self, value: U)` on `Box[+T]`; the associated function `pub fn new(value: U) -> Self` is typing/valid/variance-receiverless-function.hd, Q9, VARIANCE-ASSOC-FN)
- test/variance.test.ts :: a public method cannot infer its result past the variance check -> duplicate of typing/invalid/public-method-missing-result-type.hd [deleted]
- test/variance.test.ts :: private readonly inherent methods participate in declared variance -> duplicate of typing/invalid/covariant-private-method-parameter.hd and typing/invalid/contravariant-method-result.hd [deleted]
- test/variance.test.ts :: inferred private inherent results cannot bypass variance checking -> typing/invalid/contravariant-inferred-private-result.hd, typing/valid/covariant-inferred-private-result.hd [deleted]
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

- test/callable-storage.test.ts :: generic lists invoke stored callables -> runtime/valid/generic-storage-invokes-callables.hd [deleted]
- test/callable-storage.test.ts :: generic callable-list results retain their concrete callable ABI -> runtime/valid/generic-storage-invokes-callables.hd [deleted]
- test/callable-storage.test.ts :: generic lists invoke callables with erased inputs -> runtime/valid/generic-storage-invokes-callables.hd [deleted]
- test/callable-storage.test.ts :: nested generic storage uses the same callable representation -> runtime/valid/generic-storage-invokes-callables.hd [deleted]
- test/callable-storage.test.ts :: generic tuples invoke stored callables -> runtime/valid/generic-storage-invokes-callables.hd [deleted]
- test/callable-storage.test.ts :: generic maps invoke stored callables -> runtime/valid/generic-storage-invokes-callables.hd [deleted]
- test/callable-storage.test.ts :: callable storage preserves mutable list identity -> runtime/valid/generic-storage-callable-list-identity.hd [deleted]
- test/callable-storage.test.ts :: generic provider rows survive callable storage -> runtime/valid/row-parameter-callable-in-list.hd [deleted]
- test/callable-storage.test.ts :: generic lists invoke suspending callables after pending -> runtime/valid/pending-first-poll-generic-callable-list.hd [deleted]

## test/generic-callable-requirements.test.ts

- test/generic-callable-requirements.test.ts :: a generic field substitutes its callable requirement key through storage -> runtime/valid/generic-requirement-key-substitution.hd [deleted]
- test/generic-callable-requirements.test.ts :: callable rows infer generic data arguments from requirement keys -> runtime/valid/generic-inference-from-requirement-key.hd [deleted]
- test/generic-callable-requirements.test.ts :: generic calls infer type arguments from callable requirement keys -> runtime/valid/generic-inference-from-requirement-key.hd [deleted]
- test/generic-callable-requirements.test.ts :: generic callable results restore concrete requirement keys -> runtime/valid/generic-requirement-key-substitution.hd [deleted]
- test/generic-callable-requirements.test.ts :: suspending generic callable results retain their requirement substitutions -> runtime/valid/generic-requirement-key-substitution.hd [deleted]
- test/generic-callable-requirements.test.ts :: stored suspensions carry each call site's provider permutation -> runtime/valid/requirement-row-order-stored-suspension.hd [deleted]
- test/generic-callable-requirements.test.ts :: dynamic trait suspensions carry concrete provider-key substitutions -> runtime/valid/requirement-row-order-trait-value.hd [deleted]
- test/generic-callable-requirements.test.ts :: dynamic trait calls adapt generic callable parameters and results -> runtime/valid/requirement-row-order-trait-value.hd [deleted]
- test/generic-callable-requirements.test.ts :: provider adaptation follows binder identity across reordered rows -> runtime/valid/requirement-row-order-data-field.hd [deleted]
- test/generic-callable-requirements.test.ts :: two generic keys may collapse to one concrete row during adaptation -> runtime/valid/requirement-row-duplicate-after-substitution.hd [deleted]
- test/generic-callable-requirements.test.ts :: unordered requirement keys do not guess an ambiguous binder mapping -> typing/invalid/ambiguous-requirement-key-solution.hd, typing/valid/ambiguous-requirement-key-annotated.hd (Q6, AMBIGUOUS-TYPE); the TS test asserts a result the decision makes wrong: it expects `cannot-infer-type`, and the spec gives `ambiguous-type`
- test/generic-callable-requirements.test.ts :: enum payload extraction retains generic callable provider substitutions -> runtime/valid/generic-requirement-key-substitution.hd [deleted]

## test/inspectable-forwarding.test.ts

- test/inspectable-forwarding.test.ts :: forwarded inspection distinguishes dynamic and static type identity -> runtime/valid/inspectable-dynamic-vs-static-identity.hd [deleted]
- test/inspectable-forwarding.test.ts :: a dynamic trait is an exact generic downcast target -> runtime/valid/inspectable-generic-downcast-targets.hd [deleted]
- test/inspectable-forwarding.test.ts :: concrete generic downcasts still recover the same reference -> runtime/valid/inspectable-generic-downcast-targets.hd [deleted]
- test/inspectable-forwarding.test.ts :: an Error bound supplies transitive Inspectable evidence -> runtime/valid/error-bound-downcast-evidence.hd [deleted]
- test/inspectable-forwarding.test.ts :: nested static identity retains a mutable dynamic trait argument -> runtime/valid/typeid-nested-mut-trait-argument.hd; it compares `TypeId` values instead of the printed name, because the spec prints a fixture-declared trait by its absolute qualified name [deleted]

## test/standard-module-values.test.ts

- test/standard-module-values.test.ts :: a standard module member is a function value -> duplicate of runtime/valid/module-qualified-function-value.hd [deleted]
- test/standard-module-values.test.ts :: a renamed standard module resolves member values -> runtime/valid/module-alias-function-value.hd [deleted]
- test/standard-module-values.test.ts :: an expected function type instantiates a generic module member -> runtime/valid/module-qualified-generic-function-value.hd [deleted]
- test/standard-module-values.test.ts :: explicit type arguments instantiate a generic module member value -> runtime/valid/module-qualified-generic-function-value.hd [deleted]
- test/standard-module-values.test.ts :: a parenthesized module member remains an ordinary function value -> duplicate of runtime/valid/module-qualified-function-value-parenthesized.hd [deleted]
- test/standard-module-values.test.ts :: module selection finds a function bound through the prelude -> runtime/valid/module-qualified-prelude-function.hd [deleted]
- test/standard-module-values.test.ts :: module selection finds the same directly imported declaration -> runtime/valid/module-qualified-beside-direct-use.hd [deleted]
- test/standard-module-values.test.ts :: module selection finds the same declaration imported under an alias -> runtime/valid/module-qualified-beside-direct-use.hd [deleted]
- test/standard-module-values.test.ts :: a local value shadows a standard module namespace -> runtime/valid/local-shadows-module-namespace.hd [deleted]
- test/standard-module-values.test.ts :: a module namespace does not expose a private standard function -> typing/invalid/module-path-private-std-function.hd (Q7, PRIVATE-STD) [deleted]

## test/local-implementation-extent.test.ts

- test/local-implementation-extent.test.ts :: a local trait implementation is unavailable before its declaration -> typing/invalid/local-impl-before-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: a child-suite implementation does not leak into its parent suite -> typing/invalid/local-impl-child-suite-not-parent.hd [deleted]
- test/local-implementation-extent.test.ts :: a local trait implementation works after its declaration -> runtime/valid/local-impl-visible-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: a local inherent implementation has the same lexical extent -> typing/invalid/local-inherent-impl-before-declaration.hd, runtime/valid/local-impl-visible-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: trait-value conformance follows local implementation extent -> typing/invalid/local-impl-trait-value-before-declaration.hd, runtime/valid/local-impl-known-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: closures inherit implementations visible where the closure is written -> typing/invalid/local-impl-closure-before-declaration.hd, runtime/valid/local-impl-visible-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: local implementations remain global for overlap checking -> typing/invalid/local-impl-sibling-overlap.hd [deleted]
- test/local-implementation-extent.test.ts :: an implementation method can use its own local implementation -> runtime/valid/local-impl-visible-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: a local supertrait implementation must already be visible -> typing/invalid/local-supertrait-impl-after-child.hd, runtime/valid/local-impl-known-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: local declaration defaults keep their declaration-point implementation scope -> typing/invalid/local-impl-after-field-default.hd, runtime/valid/local-impl-visible-after-declaration.hd [deleted]
- test/local-implementation-extent.test.ts :: local trait defaults keep their declaration-point implementation scope -> typing/invalid/local-impl-after-trait-default.hd, runtime/valid/local-impl-visible-after-declaration.hd [deleted]

## test/least-common-type.test.ts

- test/least-common-type.test.ts :: optional injection uses one LCT across literals, if, match, and inferred results -> duplicate of runtime/valid/lct-optional-injection.hd [deleted]
- test/least-common-type.test.ts :: declared and built-in variance contribute at every shared LCT site -> typing/valid/lct-variance-every-site.hd [deleted]
- test/least-common-type.test.ts :: LCT remains order-independent when variance arguments need a structural join -> typing/valid/lct-structural-join-order.hd [deleted]
- test/least-common-type.test.ts :: wide declarations solve variance positions without a candidate product -> typing/valid/lct-wide-variance-join.hd; the 5-second time limit is an implementation goal, and the suite's own limit is 10 seconds [deleted]
- test/least-common-type.test.ts :: LCT rejects two optional layers and distinguishes a forbidden combined step -> duplicate of typing/invalid/lct-optional-two-layers.hd and typing/invalid/least-type-weakening-variance.hd [deleted]

## test/contextual-data-fields.test.ts

- test/contextual-data-fields.test.ts :: known mutable field types contextualize empty collection literals -> typing/valid/empty-literal-field-context.hd [deleted]
- test/contextual-data-fields.test.ts :: explicit data arguments contextualize empty mutable fields -> typing/valid/empty-literal-field-context.hd [deleted]
- test/contextual-data-fields.test.ts :: mutable generic data context infers nested data arguments -> typing/valid/nested-data-argument-from-field-type.hd [deleted]
- test/contextual-data-fields.test.ts :: inference-only field context reaches empty literals through control flow -> typing/valid/empty-literal-field-context.hd [deleted]
- test/contextual-data-fields.test.ts :: an unresolved field type does not guess an empty literal's element type -> typing/invalid/empty-literal-generic-field-unsolved.hd [deleted]
- test/contextual-data-fields.test.ts :: a readonly value can still initialize a direct mutable field -> typing/valid/readonly-value-direct-mut-field.hd [deleted]
- test/contextual-data-fields.test.ts :: an inference hint does not require nested data to be mutable -> typing/valid/readonly-part-nested-data-literal.hd [deleted]

## test/typed-derivation.test.ts

- test/typed-derivation.test.ts :: a derived walk passes each member with its facts -> duplicate of runtime/valid/typed-derivation-walk.hd [deleted]
- test/typed-derivation.test.ts :: a derivation block's member lines edit only its own derivation -> duplicate of runtime/valid/typed-derivation-walk.hd [deleted]
- test/typed-derivation.test.ts :: enum, generic, and embedded targets walk their members -> runtime/valid/typed-derivation-embedded-generic-walk.hd; the enum target is a duplicate of runtime/valid/typed-derivation-walk.hd [deleted]
- test/typed-derivation.test.ts :: a derived build fills members from their defaults -> runtime/valid/typed-derivation-build-defaults.hd [deleted]
- test/typed-derivation.test.ts :: @derive(Eq) compares data and enum members -> runtime/valid/derived-equality-members.hd; typing/valid/derived-equality.hd checks the same program at the type phase only [deleted]
- test/typed-derivation.test.ts :: typed derivation reports its diagnostics at the opt-in -> duplicates of typing/invalid/derive-member-not-derivable.hd, typing/invalid/derive-error-trait.hd, typing/invalid/unknown-annotation-member.hd, typing/invalid/omitted-member-without-default.hd, and typing/invalid/derive-before-function.hd; `@derive(Missing)` is typing/invalid/derive-unknown-trait.hd (Q8, DERIVE-MISSING), and the TS test asserts a result the decision makes wrong, since it expects `underivable-trait` and the spec gives `unknown-trait`
- test/typed-derivation.test.ts :: @derive(Debug) on a newtype needs its base type's Debug and applies it -> runtime/valid/derived-debug-newtype.hd; the missing base trait is a duplicate of typing/invalid/derive-newtype-base-missing-trait.hd [deleted]

## test/test-runner-requirements.test.ts

- test/test-runner-requirements.test.ts :: a plain test body receives the TestRunner capability -> duplicate of typing/valid/test-body-uses-test-runner.hd; the checker's requirement list that it also inspects is an implementation detail [deleted]
- test/test-runner-requirements.test.ts :: a timed test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd [deleted]
- test/test-runner-requirements.test.ts :: a table test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd [deleted]
- test/test-runner-requirements.test.ts :: a property test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd [deleted]
- test/test-runner-requirements.test.ts :: a property with a generator test body receives the TestRunner capability -> typing/valid/test-runner-every-registration-form.hd [deleted]
- test/test-runner-requirements.test.ts :: a unit test body does not receive PropertyRunner -> duplicate of typing/invalid/test-body-uses-property-runner.hd [deleted]

## test/compiler-suspension.test.ts and test/suspension.test.ts (pending-first-poll)

Each fixture runs under `# fixture-runtime-scenario: pending-first-poll`. Its suspension points are `Console.write_line!` host calls, because the scenario makes only host calls pending. The poll-order and WAT assertions of the TypeScript tests are implementation details and stay there.

- test/compiler-suspension.test.ts :: CFG suspension lowering preserves nested argument order -> runtime/valid/pending-first-poll-argument-order.hd; the per-function poll trace is not migrated
- test/compiler-suspension.test.ts :: CFG suspension lowering branches and short-circuits around child frames -> runtime/valid/pending-first-poll-branches.hd; the poll trace is not migrated
- test/compiler-suspension.test.ts :: CFG suspension lowering preserves loops, continue, break values, and cleanup -> runtime/valid/pending-first-poll-loops.hd [deleted]
- test/compiler-suspension.test.ts :: CFG suspension lowering preserves match bindings and suspending guards -> runtime/valid/pending-first-poll-match-guards.hd [deleted]
- test/compiler-suspension.test.ts :: CFG suspension lowering propagates Result failures after child completion -> runtime/valid/pending-first-poll-result-propagation.hd [deleted]
- test/compiler-suspension.test.ts :: CFG suspension lowering nests dynamic trait suspensions -> runtime/valid/pending-first-poll-dynamic-method.hd [deleted]
- test/suspension.test.ts :: a nested return after a direct drive completes the frame and unwinds defer once -> runtime/valid/pending-first-poll-nested-return-defer.hd [deleted]
- test/suspension.test.ts :: nested returns after a direct drive use the language result type, not the poll type -> runtime/valid/pending-first-poll-return-wide-result.hd [deleted]
- test/suspension.test.ts :: a nested void return after a direct drive completes with poll-ready -> runtime/valid/pending-first-poll-void-return.hd [deleted]
- test/suspension.test.ts :: a void return operand runs once before suspension cleanup -> runtime/valid/pending-first-poll-void-return-operand.hd [deleted]
- test/suspension.test.ts :: separate Result propagation after a direct drive completes and unwinds the frame -> runtime/valid/pending-first-poll-result-defer.hd [deleted]
- test/suspension.test.ts :: non-suspending comprehensions propagate through the surrounding suspension frame -> runtime/valid/pending-first-poll-comprehension-propagation.hd [deleted]
- test/suspension.test.ts :: cleanup cannot propagate out of an ordinary or suspending function -> not migrated (no pending host call; a diagnostic test)
- test/suspension.test.ts :: a closure declared in cleanup can propagate within its own function -> not migrated (no pending host call)

## test/cli.test.ts and test/cli-commands.test.ts (CLI cases)

Paths `cli/NAME` are CLI cases under `spec/conformance/cli/`, indexed by `cli-cases.tsv`.

- test/cli.test.ts :: the hd executable passes its arguments and sets the exit status -> cli/exit-program-status (the ExitCode(3) status), cli/exit-usage-error (a rejected command line); the unknown-command text and the escaped internal error are not migrated
- test/cli.test.ts :: hd test fails a test whose result is .Err -> cli/exit-test-failure; the message text is not migrated
- test/cli.test.ts :: hd run and hd test judge suspending results by Termination -> cli/exit-program-status (statuses 0, 3, and 1), cli/exit-test-failure; the message text is not migrated
- test/cli-commands.test.ts :: hd test on a package tests each module, and with no path the current package -> cli/exit-test-failure, cli/exit-package-file; the printed counts and the loose-directory case are not migrated
- test/cli-commands.test.ts :: hd run, check, and build on a package file link the package -> cli/exit-package-file for the check; the run and build forms are not migrated (cli.run.file makes `hd run FILE` an error)
- test/cli-commands.test.ts :: hd run resolves super uses, and reports a package error in its own file -> cli/json-file-location for the file of the error; the run part is not migrated

## test/std/*.hd (stdlib-tier tests, run by test/std.test.ts)

The hd files stay: `test/std.test.ts` still runs each through `hd test`. A
later task deletes them.

- test/std/annotation.hd :: finds each attached value by its type -> runtime/valid/facts-of-literal-generic-none.hd
- test/std/annotation.hd :: reads a generic function's values -> runtime/valid/facts-of-literal-generic-none.hd
- test/std/annotation.hd :: a function without decorators holds none -> runtime/valid/facts-of-literal-generic-none.hd
- test/std/collections.hd :: map and filter -> runtime/valid/list-and-optional-map.hd for `map`; `filter` is a duplicate of runtime/valid/list-access-building.hd
- test/std/collections.hd :: first, last, and reversed -> duplicate of runtime/valid/list-access-building.hd
- test/std/collections.hd :: sorted_by is stable -> duplicate of runtime/valid/list-access-building.hd
- test/std/collections.hd :: chunks and zip -> duplicate of runtime/valid/list-chunks.hd and runtime/valid/list-access-building.hd
- test/std/collections.hd :: chunks rejects a zero size -> duplicate of runtime/panic/list-chunks-zero.hd
- test/std/collections.hd :: Set writes one debug_list entry per element, in insertion order -> not migrated: the test asserts the compact text `[2, 1]` of `debug(set)`, and `std-format.debug.render` says `debug` text is multi-line (Q10)
- test/std/collections.hd :: windows rejects a zero size -> duplicate of runtime/panic/list-windows-size.hd
- test/std/cmp-iter.hd :: min, max, and clamp -> duplicate of runtime/valid/cmp-min-max.hd and runtime/valid/cmp-clamp.hd
- test/std/cmp-iter.hd :: min and max give the first of equal values -> runtime/valid/cmp-min-max-distinguishable-tie.hd
- test/std/cmp-iter.hd :: clamp rejects an empty range -> duplicate of runtime/panic/cmp-clamp-reversed.hd
- test/std/cmp-iter.hd :: Reverse inverts the order -> duplicate of runtime/valid/cmp-reverse.hd
- test/std/cmp-iter.hd :: adapters -> duplicate of runtime/valid/iterator-adapters-run.hd
- test/std/cmp-iter.hd :: take reads only what it yields -> duplicate of runtime/valid/iterator-adapters-run.hd (`std-iter.adapter.take.limit`)
- test/std/cmp-iter.hd :: composites compare through std.cmp -> duplicate of runtime/valid/structural-ordering.hd, runtime/valid/structural-equality.hd, and runtime/valid/nan-ordering-composites.hd; `min` of two lists is in runtime/valid/cmp-min-max-distinguishable-tie.hd
- test/std/cmp-iter.hd :: repeated comparisons start each time from the first element -> not migrated: a regression guard of the prototype, with no rule beyond ordinary comparison
- test/std/handle-fact.hd :: reads a typed fact through a handle whose F has no Inspectable bound -> runtime/valid/handle-fact-exact-type.hd
- test/std/console.hd :: BufferConsole records each line -> duplicate of runtime/valid/buffer-console.hd
- test/std/console.hd :: println records inside a test body -> duplicate of runtime/valid/println-in-test-body.hd
- test/std/console.hd :: a new BufferConsole is empty -> duplicate of runtime/valid/buffer-console.hd
- test/std/derive.hd :: derived ordering is lexicographic in field order -> runtime/valid/derived-ordering-run.hd
- test/std/derive.hd :: derived enum ordering follows variant order, then payloads -> runtime/valid/derived-ordering-run.hd
- test/std/derive.hd :: derived partial ordering of floats -> runtime/valid/derived-ordering-run.hd
- test/std/derive.hd :: derived Hash hashes the fields in order -> duplicate of runtime/valid/hash-bytes-derived.hd
- test/std/derive.hd :: == uses a generic derived Eq -> runtime/valid/derived-equality-generic.hd
- test/std/derive.hd :: a newtype derives from its base type -> duplicate of runtime/valid/derived-newtype.hd
- test/std/derive.hd :: a type with Eq and Hash keys a map -> runtime/valid/map-key-types.hd
- test/std/error.hd :: an absent optional source ends the chain -> runtime/valid/error-chain-derived-causes.hd
- test/std/error.hd :: a transparent variant skips its member -> runtime/valid/error-chain-derived-causes.hd
- test/std/error.hd :: an erased error reports its causes -> runtime/valid/error-chain-derived-causes.hd
- test/std/error.hd :: an empty report displays its message alone -> duplicate of runtime/valid/error-report.hd
- test/std/hash.hd :: a string hashes its UTF-8 length as a u64, then its UTF-8 bytes -> duplicate of runtime/valid/hash-bytes-sequences.hd
- test/std/hash.hd :: integers hash their little-endian bytes at their own width -> duplicate of runtime/valid/hash-bytes-scalars.hd
- test/std/hash.hd :: a type parameter bounded by Eq and Hash keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: a char keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: an i64 keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: a derived Eq and Hash type keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: a type parameter keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: collect builds a map, the later value wins in the first position -> duplicate of runtime/valid/collect-targets-run.hd
- test/std/map.hd :: collect builds a map over a type-parameter key -> not migrated: no rule beyond `std-iter.collect.map`, which runtime/valid/collect-targets-run.hd covers for concrete keys
- test/std/map.hd :: a map entry meets a Display bound -> duplicate of runtime/valid/display-tuples.hd (an entry is a tuple)
- test/std/map.hd :: debug writes a map's entries in order -> not migrated: the test asserts the compact text `{"a": 1}`, and `std-format.debug.render` says `debug` text is multi-line (Q10)
- test/std/num.hd :: checked arithmetic reports overflow as None -> duplicate of runtime/valid/num-checked-wrapping.hd
- test/std/num.hd :: wrapping and saturating arithmetic -> duplicate of runtime/valid/num-checked-wrapping.hd, runtime/valid/num-saturating.hd, and runtime/valid/num-abs-diff.hd
- test/std/num.hd :: bit counts -> duplicate of runtime/valid/num-bit-counts.hd
- test/std/num.hd :: i64 methods -> duplicate of runtime/valid/num-every-width.hd
- test/std/num.hd :: f64 classification -> duplicate of runtime/valid/num-is-nan.hd and runtime/valid/num-is-finite.hd
- test/std/num.hd :: parse_i32 and parse_i64 -> duplicate of runtime/valid/num-parse-integers.hd
- test/std/num.hd :: every integer width -> duplicate of runtime/valid/num-every-width.hd
- test/std/providers.hd :: ManualClock starts where it is told and sleep! advances it -> duplicate of runtime/valid/manual-clock.hd
- test/std/providers.hd :: ManualClock covers a Clock row -> duplicate of runtime/valid/manual-clock.hd and runtime/valid/clock-helpers.hd
- test/std/providers.hd :: SeededRandom repeats its draws for a seed -> duplicate of runtime/valid/seeded-random.hd
- test/std/providers.hd :: SeededRandom covers a Random row -> duplicate of runtime/valid/seeded-random.hd and runtime/valid/rng-from-random.hd
- test/std/property.hd :: addition commutes -> runtime/valid/property-generators-scalars.hd
- test/std/property.hd :: draws stay in range -> runtime/valid/property-generators-scalars.hd
- test/std/property.hd :: lists stay short -> runtime/valid/property-generators-collections.hd
- test/std/property.hd :: assume discards odd values -> runtime/valid/property-generators-scalars.hd; also a duplicate of runtime/valid/property-assume-discards.hd
- test/std/property.hd :: a string is at most 16 chars -> not migrated: the spec gives `Arbitrary` for `string` no length limit, and `Choices` has no size (`std-testing.choices.no-size`) (Q11)
- test/std/property.hd :: a map holds at most max entries -> runtime/valid/property-generators-collections.hd
- test/std/property.hd :: int takes its type from the context -> runtime/valid/property-generators-scalars.hd
- test/std/property.hd :: the draw budget ends recursion -> runtime/valid/property-generators-collections.hd; also a duplicate of runtime/valid/property-draw-budget.hd
- test/std/property.hd :: an f64 may be any value -> runtime/valid/property-generators-scalars.hd
- test/std/option-result.hd :: option map and unwrap_or -> runtime/valid/list-and-optional-map.hd for `map`; `unwrap_or` is a duplicate of runtime/valid/option-and-then.hd
- test/std/option-result.hd :: option ok_or, is_some, is_none, expect -> duplicate of runtime/valid/option-tests-conversions.hd
- test/std/option-result.hd :: option expect panics on None -> duplicate of runtime/panic/option-expect-none.hd
- test/std/option-result.hd :: result map and map_err -> duplicate of runtime/valid/result-map.hd and runtime/valid/result-and-then.hd
- test/std/option-result.hd :: result ok, err, is_ok, is_err, unwrap_or, expect -> duplicate of runtime/valid/result-tests-conversions.hd and runtime/valid/result-and-then.hd
- test/std/sized-numeric.hd :: narrow signed arithmetic stays in range -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: i8 overflow panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: i8 MIN / -1 panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: unsigned 32-bit values compare and display as unsigned -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: u32 subtraction below zero panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: u64 keeps its full range -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: u64 overflow panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: numeric casts convert in range -> runtime/valid/numeric-casts-in-range.hd
- test/std/sized-numeric.hd :: an out-of-range integer cast wraps -> duplicate of runtime/valid/narrowing-cast-wraps.hd
- test/std/sized-numeric.hd :: an out-of-range float cast saturates -> duplicate of runtime/valid/float-cast-saturates.hd
- test/std/sized-numeric.hd :: f32 arithmetic keeps f32 width -> runtime/valid/numeric-casts-in-range.hd
- test/std/sized-numeric.hd :: narrow shifts keep the width -> runtime/valid/sized-integer-arithmetic.hd
- test/std/time-process.hd :: Duration counts whole milliseconds -> duplicate of runtime/valid/duration-api.hd
- test/std/time-process.hd :: Duration arithmetic and Display -> duplicate of runtime/valid/duration-arithmetic.hd and runtime/valid/duration-display.hd
- test/std/time-process.hd :: Termination reports exit codes -> duplicate of runtime/valid/termination-report.hd
- test/std/time-process.hd :: ScriptedProcess answers by program -> duplicate of runtime/valid/scripted-process.hd
- test/std/text.hd :: is_empty, ends_with, and contains -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: find returns a byte offset -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: trim_start, trim_end, and upper -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: strip_prefix and strip_suffix -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: lines and repeat -> duplicate of runtime/valid/string-lines.hd and runtime/valid/string-repeat.hd
- test/std/text.hd :: join and StringBuilder -> duplicate of runtime/valid/text-join-builder.hd
- test/std/text.hd :: to_utf8 and string::from_utf8 -> duplicate of runtime/valid/utf8-valid-text.hd, runtime/valid/utf8-invalid-bytes.hd, runtime/valid/utf8-truncated.hd, and runtime/valid/utf8-overlong.hd
- test/std/text-prefix.hd :: interpolate joins pieces and values in order -> duplicate of runtime/valid/text-prefix-helpers.hd
- test/std/text-prefix.hd :: process_escapes replaces each escape -> duplicate of runtime/valid/text-prefix-helpers.hd
- test/std/text-prefix.hd :: a prefix can process escapes in its pieces -> duplicate of runtime/valid/text-prefix-helpers.hd
- test/std/tuple-text.hd :: writes a one-element tuple with a comma -> the Display half is a duplicate of runtime/valid/display-tuples.hd; the Debug half is not migrated: it asserts the compact text `(1,)` of `debug((1,))` (Q10)
- test/std/tuple-text.hd :: writes the empty tuple and wider tuples without one -> not migrated: it asserts the compact `debug` text of `()`, `(1, 2)`, and `Some(1)` (Q10)

## test/std.test.ts (non-hd rows)

- test/std.test.ts :: an imported std name takes its local alias; the rest stay hidden -> typing/invalid/use-alias-original-name-unbound.hd (with `std.testing.assert`, a language-tier name, in place of `std.cmp.min`) [deleted]

## test/package-relative.test.ts

- test/package-relative.test.ts :: ordinary source files resolve self children and super siblings from their own module -> duplicate of runtime/valid/relative-self-top-level.hd
- test/package-relative.test.ts :: nested files resolve child, sibling and repeated-parent uses -> duplicate of runtime/valid/relative-self-current.hd for `self` and `super`; typing/valid/relative-repeated-super.hd for `super.super`
- test/package-relative.test.ts :: directory modules resolve self from their directory identity -> duplicate of typing/valid/relative-self-mod-file.hd [deleted]
- test/package-relative.test.ts :: src/main.hd resolves self from its program root -> duplicate of typing/valid/relative-self-main.hd [deleted]
- test/package-relative.test.ts :: src/main.hd cannot move above its program root -> duplicate of typing/invalid/root-file-super.hd [deleted]
- test/package-relative.test.ts :: src/lib.hd resolves self from its program root -> duplicate of typing/valid/relative-self-lib.hd [deleted]
- test/package-relative.test.ts :: src/lib.hd cannot move above its program root -> typing/invalid/root-file-lib-super.hd [deleted]
- test/package-relative.test.ts :: tests/checkout.hd resolves self from its program root -> duplicate of runtime/valid/integration-shared-use.hd [deleted]
- test/package-relative.test.ts :: tests/checkout.hd cannot move above its program root -> duplicate of typing/invalid/integration-super-above-test-root.hd [deleted]
- test/package-relative.test.ts :: shared test modules resolve from their own module and cannot escape the test root -> typing/valid/relative-shared-test-module.hd, typing/invalid/relative-shared-test-above-test-root.hd [deleted]
- test/package-relative.test.ts :: ordinary source files cannot move above the package root -> not migrated: `module.relative.above-root` names no error code, and a fixture needs one (Q12)

## test/package.test.ts

- test/package.test.ts :: a use of another module's public declarations links and runs -> duplicate of runtime/valid/init-group-order.hd
- test/package.test.ts :: relative uses, re-exports, and initialization order follow the use graph -> typing/valid/relative-repeated-super.hd for `super.super`; duplicate of typing/valid/pub-use-chain.hd and runtime/valid/init-group-order.hd
- test/package.test.ts :: package use errors point at the use declaration of their file -> typing/invalid/unknown-package-name.hd for `unknown-import`; duplicate of typing/invalid/unknown-pkg-module.hd, typing/invalid/private-package-name.hd, typing/invalid/unknown-dep-module.hd, and typing/invalid/root-file-super.hd; the namespace and `as` uses that now link duplicate runtime/valid/relative-self-current.hd and typing/invalid/module-path-missing-member.hd
- test/package.test.ts :: std use errors in a package point at the use declaration of their file -> duplicate of typing/invalid/unknown-std-module.hd, typing/invalid/unknown-std-name.hd, and typing/invalid/private-std-function.hd (where the error is reported is not specified) [deleted]
- test/package.test.ts :: files of one folder may use each other in a loop -> duplicate of typing/valid/folder-loop-within-folder.hd [deleted]
- test/package.test.ts :: folders that depend on each other in a loop are rejected -> duplicate of typing/invalid/folder-cycle-facade.hd, typing/valid/folder-cycle-leaf-folder.hd, and typing/invalid/folder-cycle-nested.hd; the message text is not migrated
- test/package.test.ts :: uses in test code make no folder edge -> duplicate of runtime/valid/folder-graph-test-edges.hd [deleted]
- test/package.test.ts :: a pub use chain must end at a declaration -> duplicate of typing/invalid/pub-use-loop.hd and typing/invalid/use-through-pub-use-loop.hd [deleted]
- test/package.test.ts :: shared names and bad paths are rejected -> not migrated: `package-name-collision` and `duplicate-module-path` name no rule of the spec (a module's private names are its own), a used name that collides with a declaration has no code (`names.use.no-collision`), and `unclosed-delimiter` appears only in the README table (Q13)
- test/package.test.ts :: single-declaration uses and the package root module resolve -> typing/valid/use-path-only-declaration.hd for the path-only use of one declaration; the `src/mod.hd` root is not migrated, since `module.path.no-root-mod` makes it an error, and typing/valid/lib-root-pkg.hd covers `pkg.{X}` from `src/lib.hd`

## Mixed-file rows (batch 3)

- test/index-width.test.ts :: a u64 list index is bounds-checked before it can wrap to u32 -> runtime/panic/list-index-u64-beyond-u32.hd, runtime/panic/list-set-u64-beyond-u32.hd [deleted]
- test/index-width.test.ts :: a compound list index keeps its context and is evaluated once -> runtime/valid/compound-assign-index-once.hd [deleted]
- test/list-spread-diagnostic.test.ts :: postfix list spreads remain valid -> duplicate of runtime/valid/list-suffix-spread.hd [deleted]
- test/list-spread-diagnostic.test.ts :: prefix copies in data expressions remain valid -> duplicate of parse/valid/copy-update-spread.hd [deleted]
- test/unused-locals.test.ts :: unused-local warnings name exactly the unread bindings -> duplicate of typing/warnings/unused-local-binding.hd (a fixture holds one marker, so the second unread name is not asserted) [deleted]
- test/usize-alias.test.ts :: usize expands in a transparent alias target -> typing/valid/usize-alias-positions.hd [deleted]
- test/usize-alias.test.ts :: usize still expands within trait and implementation heads -> typing/valid/usize-alias-positions.hd [deleted]
- test/usize-alias.test.ts :: usize expands in a newtype base -> typing/valid/usize-alias-positions.hd [deleted]
- test/usize-alias.test.ts :: usize remains a protected prelude name -> typing/invalid/prelude-shadow-usize-alias.hd; the type-parameter and local forms are covered by typing/invalid/prelude-shadow-result-generic.hd and typing/invalid/prelude-shadow-hash-local.hd [deleted]
- test/usize-alias.test.ts :: subtracting one from an empty list length has u32 overflow semantics -> duplicate of runtime/panic/usize-len-underflow.hd [deleted]
- test/contextual-numeric-literals.test.ts :: context preserves range, unsigned-negation, and no-widening errors -> typing/invalid/literal-operand-range-wide.hd, typing/invalid/literal-operand-negation-unsigned.hd, typing/invalid/mixed-width-operands-no-widening.hd [deleted]
- test/contextual-numeric-literals.test.ts :: literal suffix calls keep their parameter type -> duplicate of typing/valid/literal-suffix-generic-num.hd and typing/invalid/literal-suffix-parameter-type.hd (`types.literal.suffixed.in`) [deleted]
- test/index-context.test.ts :: built-in indices preserve every explicitly typed unsigned width -> typing/valid/index-unsigned-widths.hd [deleted]
- test/index-context.test.ts :: built-in indices distinguish negative literals from signed values -> typing/invalid/compound-index-negative-literal.hd; duplicate of typing/invalid/list-index-negative-literal.hd, typing/invalid/list-slice-negative-literal.hd, typing/invalid/list-index-signed.hd, typing/invalid/string-index-signed.hd, and typing/invalid/list-slice-signed-bound.hd [deleted]
- test/index-context.test.ts :: bound inference ignores a blanket implementation whose own bound is unavailable -> typing/invalid/bound-inference-blanket-impl-unmet.hd [deleted]
- test/index-context.test.ts :: bound inference rejects a unique implementation with incompatible trait arguments -> typing/invalid/bound-inference-incompatible-argument.hd [deleted]
- test/inherent-implementation-bounds.test.ts :: a bounded standard inherent Map implementation keeps its key bounds -> not migrated: it marks a user implementation as `standard`, which user code cannot do (a user `impl Map[K, V]` is `orphan-impl`)
- test/inherent-implementation-bounds.test.ts :: an unbounded standard inherent Map implementation still fails its key bound -> not migrated: same reason

## Batch 4 rows (test/suspension.test.ts, test/captured-cells.test.ts)

- test/suspension.test.ts :: cleanup cannot propagate out of an ordinary or suspending function -> typing/invalid/defer-propagate-query.hd, typing/invalid/defer-propagate-query-suspending.hd [deleted]
- test/suspension.test.ts :: a closure declared in cleanup can propagate within its own function -> runtime/valid/defer-closure-propagation.hd [deleted]
- test/captured-cells.test.ts :: nested mutable captures share storage after an activation ends and across suspension -> runtime/valid/nested-mutable-captures.hd (the custom pending-poll half stays; a fixture cannot name that schedule) [deleted]
- test/captured-cells.test.ts :: loop activations retain independent mutable cells while sibling closures share each cell -> runtime/valid/loop-iteration-cells.hd (same; complements runtime/valid/loop-iteration-binding-capture.hd) [deleted]
- test/captured-cells.test.ts :: shared capture lookup uses explicit closure index rather than array position -> not migrated: asserts unit-only HIR cell rewriting
- test/captured-cells.test.ts :: cell conversion is idempotent and keeps distinct activations' local identities separate -> not migrated: asserts unit-only HIR cell rewriting
- test/captured-cells.test.ts :: typed rewriting reaches dictionary bounds and match tests while leaving metadata intact -> not migrated: asserts unit-only HIR cell rewriting

## Batch 4 rows (test/call-speculation.test.ts)

- test/call-speculation.test.ts :: contextually typed closure selects one candidate and commits once (both orders) -> runtime/valid/candidate-closure-selection.hd, runtime/valid/candidate-closure-selection-reversed.hd [deleted]
- test/call-speculation.test.ts :: nested binding and branch scopes stay isolated while selecting a candidate (both orders) -> runtime/valid/candidate-scope-isolation.hd, runtime/valid/candidate-scope-isolation-reversed.hd [deleted]
- test/call-speculation.test.ts :: complex argument shapes do not turn two distinct traits into overloads -> typing/invalid/distinct-traits-not-overloads.hd [deleted]
- test/call-speculation.test.ts :: associated calls check closures and named arguments per candidate -> runtime/valid/associated-closure-named-args.hd [deleted]
- test/call-speculation.test.ts :: a suspending argument is checked per candidate without ambiguity -> runtime/valid/suspending-argument-candidate.hd (direct-drive half; the custom pending schedule stays) [deleted]
- test/call-speculation.test.ts :: provider scopes and captures from a failed trial do not leak -> runtime/valid/failed-trial-no-leak.hd [deleted]
- test/call-speculation.test.ts :: generic method substitutions stay local to the winning candidate -> runtime/valid/generic-method-candidate-local.hd [deleted]
- test/call-speculation.test.ts :: explicit method type arguments select the candidate per trial (both forms) -> runtime/valid/explicit-type-args-method.hd, runtime/valid/explicit-type-args-associated.hd [deleted]
- test/call-speculation.test.ts :: associated tuple spread arguments select the fitting candidate -> runtime/valid/tuple-spread-candidate.hd [deleted]
- test/call-speculation.test.ts :: candidate selection includes the result type -> runtime/valid/result-type-candidate.hd [deleted]
- test/call-speculation.test.ts :: numeric candidates use the ordinary literal-default tie break, and a bare literal fits none -> runtime/valid/numeric-candidate-tie-break.hd, typing/invalid/numeric-candidate-no-fit.hd [deleted]
- test/call-speculation.test.ts :: failed fits report every available instantiation -> typing/invalid/no-fit-candidate.hd (codes only; the message text is not portable) [deleted]
- test/call-speculation.test.ts :: an unused local still warns after rejected trials rewind -> typing/warnings/unused-after-rejected-trials.hd [deleted]
- test/call-speculation.test.ts :: lazy result inference feeds candidate selection -> runtime/valid/lazy-result-candidate.hd [deleted]
- test/call-speculation.test.ts :: the remaining eight tests (registry traversal, journal unwind, aliases, savepoints, Map order, graph reachability, nested trials, cyclic graphs) -> not migrated: assert checker-internal speculation journals and snapshots

## Batch 4 rows (test/cli.test.ts)

- test/cli.test.ts :: all 16 tests (argv/exit status, CLI commands end to end, unknown commands, hd test verdicts, Termination judging, Console routing, assert_equal rendering, snapshots, timeouts, shrinking, properties) -> not migrated: drive the hd executable and its hosted test runner; the portable suite has no CLI tier (cli: 0 of 0 selected)

## Batch 4 rows (test/compiler-suspension.test.ts)

- test/compiler-suspension.test.ts :: a module may declare its own all! without importing std.task.all -> typing/valid/user-defined-all.hd [deleted]
- test/compiler-suspension.test.ts :: all!/race! poll order, cancellation, and trace sequences -> not migrated: assert trace event orders and manual __hd_poll/__hd_cancel driving
- test/compiler-suspension.test.ts :: race! with an empty list literal / over a list empty at run time -> not migrated: the fixtures are KNOWN_FAILURES rows (tag RACE-EMPTY); the TS tests pin the current behavior
- test/compiler-suspension.test.ts :: host record/replay, encoding, validation, console routing -> not migrated: assert the host bridge, not language behavior
- test/compiler-suspension.test.ts :: the CONF-behavior tests (frames, dictionaries, child pending, cancellation unwinding, CFG lowering, block_on, module bindings) -> behavior already covered by their conformance fixtures; the frame/trace assertions stay [kept]

## Batch 4 rows (test/compiler.test.ts)

- test/compiler.test.ts :: shared enum data is a per-variant constant computed once -> runtime/valid/shared-enum-data-computed-once.hd [deleted]
- test/compiler.test.ts :: a function result that is a function type with a row keeps that row -> runtime/valid/closure-result-keeps-requirement-row.hd [deleted]
- test/compiler.test.ts :: a trait default method instantiates the trait's type parameters -> runtime/valid/trait-default-method-instantiates-params.hd [deleted]
- test/compiler.test.ts :: cannot-infer-type message text -> not migrated: asserts exact diagnostic messages; the codes are already covered by typing/invalid/cannot-infer-type.hd and its siblings
- test/compiler.test.ts :: optional and Result context errors have stable diagnostics -> not migrated: asserts codes on existing fixtures; the TS test stays as the message/code pin
- test/compiler.test.ts :: the CONF-behavior tests (power, varargs, defaults, named args, enums, strings, defer, panic, patterns, closures, function types) -> behavior already covered by their conformance fixtures; the WAT/HIR/trace assertions stay [kept]
- test/compiler.test.ts :: checker HIR shape, Wasm GC smoke, panic-tag consistency, panic-detail import flag -> not migrated: assert compiler internals

## Batch 4 rows (test/compiler-types.test.ts)

- test/compiler-types.test.ts :: owning a trait argument's outer constructor permits a foreign trait impl (TQ-2) -> typing/valid/outer-constructor-permits-impl.hd plus the existing typing/invalid/orphan-impl-nested-trait-argument.hd [deleted]
- test/compiler-types.test.ts :: built-in comparison dictionaries carry their supertrait dictionaries (EQ-1) -> runtime/valid/ord-supertrait-dispatch.hd and typing/invalid/float-misses-ord-bound.hd; the HIR supertrait assertions stay [kept]
- test/compiler-types.test.ts :: a type parameter calls an associated function through its bound (TQ-9) -> not migrated: asserts HIR trait-call/dictionary shape; behavior already covered by runtime/valid/associated-function-calls.hd
- test/compiler-types.test.ts :: a generic inherent implementation lowers to a generic function (TQ-19) -> runtime/valid/generic-inherent-box.hd; the HIR genericParameters assertion stays [kept]
- test/compiler-types.test.ts :: a marker implementation's bounds are proven (TQ-20) -> typing/invalid/marker-bound-unproven.hd [deleted]
- test/compiler-types.test.ts :: i64 literals, explicit widening, checked arithmetic, and narrowing (F-253) -> runtime/valid/i64-widening-checked.hd and typing/invalid/implicit-narrowing-i64.hd; the fix-it edit assertions stay [kept]
- test/compiler-types.test.ts :: println drives write_line! on a program-defined Console (MHP-1) -> runtime/valid/buffered-println-program-console.hd [deleted]
- test/compiler-types.test.ts :: println drives a write_line! pending on a host operation until it finishes (MHP-1) -> not migrated: needs a custom hostSuspensionPending schedule
- test/compiler-types.test.ts :: Debug is checked, and derived builders render debug text (T33, T53) -> runtime/valid/debug-derive-data.hd and typing/invalid/debug-missing-derive.hd [deleted]
- test/compiler-types.test.ts :: @derive(Debug) picks Rust's builder per data type and variant (T54) -> runtime/valid/debug-derive-variants.hd [deleted]
- test/compiler-types.test.ts :: u8 checked arithmetic and ExitCode entry results (T8) -> runtime/valid/u8-checked-add.hd and runtime/panic/u8-add-overflow.hd; the ExitCode entry halves stay [kept]
- test/compiler-types.test.ts :: the other 37 tests (reify, erasure, provider packs, row forwarding, dictionaries, GC layouts, HIR permissions) -> not migrated: assert Wasm/HIR lowering shapes

## Batch 4 rows (test/types.test.ts)

- test/types.test.ts :: optional invariance rejects weakening the payload but accepts weakening the outer view -> typing/invalid/optional-payload-weakening.hd, typing/invalid/optional-generic-payload-weakening.hd, typing/invalid/optional-payload-mutable-outer.hd, typing/valid/optional-outer-weakening.hd [deleted]
- test/types.test.ts :: mutation through generic aliases, nested optionals, propagation and matches survives Wasm erasure -> runtime/valid/optional-alias-mutation.hd [deleted]
- test/types.test.ts :: optional functions and tuples retain mutable optional arguments in emitted closure signatures -> runtime/valid/optional-closure-arguments.hd [deleted]
- test/types.test.ts :: mut User? supplies mutable payload access through inferred let mut and matching -> runtime/valid/optional-mutable-match.hd [deleted]
- test/types.test.ts :: mutable optional payload annotations grant only their declared payload access -> typing/warnings/let-mut-optional-payload.hd, typing/invalid/optional-readonly-payload-store.hd [deleted]
- test/types.test.ts :: mutable primitive and tuple payloads reject while mutable optional constructors remain valid -> typing/invalid/mut-on-primitive-optional.hd, typing/invalid/mut-on-primitive-generic-optional.hd, typing/invalid/mut-on-tuple-optional.hd, typing/invalid/mut-on-tuple-generic-optional.hd, typing/valid/mut-outer-views-valid.hd [deleted]
- test/types.test.ts :: inference advice preserves optional permission boundaries in source annotations -> not migrated: asserts internal canonical type strings through unit-only helpers
- test/types.test.ts :: optional payload permission and optional outer permission have distinct canonical types -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: nesting preserves the scope of each mut prefix and optional constructor -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: function optionality remains distinct from result optionality -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: type substitution and generic matching preserve permission constructor boundaries -> not migrated: asserts unit-only substitution helpers
- test/types.test.ts :: deep permission erasure rebuilds canonical option and function types -> not migrated: asserts a unit-only erasure helper
- test/types.test.ts :: display rendering hides compiler-internal names but keeps user spellings -> not migrated: asserts the unit-only display renderer
- test/types.test.ts :: source type rendering round-trips mutable constructors rather than changing prefix scope -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: optional type identity includes payload mut and not outer mut -> not migrated: asserts the unit-only inspect key splitter
- test/types.test.ts :: the REPL renders invariant optional mutable payloads without erasing their generic permissions -> not migrated: REPL session behavior, no portable fixture form
- test/types.test.ts :: the REPL preserves mutable outer optionals nested inside another optional -> not migrated: REPL session behavior, no portable fixture form
- test/types.test.ts :: typed build uses the shared optional constructor for declared mutable members -> not migrated: asserts another fixture file passes, not language behavior

## Owner Questions

Q1 to Q8 are answered (owner, 2026-10-04); each answer is in the spec,
and each line above names the fixture that covers it.

| Question                                                                 | Answer                                                                                                | Spec                                                                                                                 |
| ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Q1. May a method's own type parameter reuse its `impl`'s parameter name? | No: `fn echo[T]` inside `impl[T] Box[T]` is `duplicate-binding` (SHADOW-TPARAM)                       | [`names.type-param.no-redeclare.method`](../spec/lang/03-names-and-scopes.md#r-names.type-param.no-redeclare.method) |
| Q2. May a local name in a body reuse a type parameter's name?            | No: a local `data T` or value `T` inside `fn work[T]` is `duplicate-binding` (SHADOW-TPARAM)          | [`names.type-param.no-redeclare.body`](../spec/lang/03-names-and-scopes.md#r-names.type-param.no-redeclare.body)     |
| Q3. How is a method on a non-bare target checked against variance?       | By each `impl` parameter's variance in the target, composed as Kotlin and Scala do (VARIANCE-NONBARE) | [Variance On A Non-Bare Target](../spec/lang/04-type-system.md#variance-on-a-non-bare-target)                        |
| Q4. Do `mut self` methods count toward declared variance?                | Yes: `pub fn set(mut self, value: U)` on `Box[+T]` is `invalid-variance` (VARIANCE-MUT-SELF)          | [`types.variance.surface.mut-self`](../spec/lang/04-type-system.md#r-types.variance.surface.mut-self)                |
| Q5. Is an alias whose target names nothing an error at the alias?        | Yes, used or not: `unknown-type`, or `unknown-trait` for a row key (ALIAS-MISSING)                    | [`types.alias.target-unknown`](../spec/lang/04-type-system.md#r-types.alias.target-unknown)                          |
| Q6. Which code does an ambiguous generic solution report?                | The new code `ambiguous-type`; `cannot-infer-type` stays for no solution (AMBIGUOUS-TYPE)             | [`types.infer.ambiguous.code`](../spec/lang/04-type-system.md#r-types.infer.ambiguous.code)                          |
| Q7. Which code does a module path to a private std function report?      | `private-import`, as for any module (PRIVATE-STD)                                                     | [`expr.name.qualified.private`](../spec/lang/05-expressions.md#r-expr.name.qualified.private)                        |
| Q8. Which code does `@derive(Missing)` report?                           | `unknown-trait`; `underivable-trait` stays for a real trait with no template (DERIVE-MISSING)         | [`annot.derive.unknown`](../spec/lang/14-annotations.md#r-annot.derive.unknown)                                      |

- **Q9, answered 2026-10-04.** An inherent associated function with no
  receiver does not count toward declared variance, so `pub fn new(value: U)`
  on `Box[+T]` is accepted
  ([`types.variance.surface.no-receiver`](../spec/lang/04-type-system.md#r-types.variance.surface.no-receiver),
  VARIANCE-ASSOC-FN).

- **Q10.** What text does `debug(value)` give for a composite? Several std
  tests assert compact text: `[2, 1]` for a set, `{"a": 1}` for a map,
  `(1,)` for a tuple, `Some(1)` for an optional.
  [`std-format.debug.render`](../spec/std/format.md#r-std-format.debug.render)
  says the text is multi-line and consistently indented, and
  [`std-format.debug.layout`](../spec/std/format.md#r-std-format.debug.layout)
  leaves compact or pretty layout to the writer. No fixture can assert the
  text until the spec fixes it.
- **Q11.** Does the default `Arbitrary` for `string` have a length limit? The
  test asserts at most 16 chars.
  [`std-testing.choices.no-size`](../spec/std/testing.md#r-std-testing.choices.no-size)
  says `Choices` has no size, and no rule gives the default generator a limit.
- **Q12.** Which code does a `super` above the package root report in a file
  that is not a root file, as `use super.super.x` in `src/a.hd`? The test
  expects `unknown-module`.
  [`module.relative.above-root`](../spec/lang/10-modules.md#r-module.relative.above-root)
  says only "compile-time error".
- **Q13.** The package-link test asserts four codes that no rule gives. A
  private name of one module that another module also declares reports
  `package-name-collision`. A `use` of a name that a module also declares
  reports `duplicate-module-name`
  ([`names.use.no-collision`](../spec/lang/03-names-and-scopes.md#r-names.use.no-collision)
  gives no code). Two paths such as `src/main.hd` and `src/Main.hd` report
  `duplicate-module-path`. An unclosed `(` reports `unclosed-delimiter`,
  which only the README table lists. Which of these belong in the spec?
