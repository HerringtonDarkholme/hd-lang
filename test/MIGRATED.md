# Migrated TypeScript Tests

These TypeScript tests now have implementation-neutral conformance fixtures
under `spec/conformance/`. Each line maps one test to its fixtures, or to the
existing fixture that already covered it. A test marked "not migrated" stays
in TypeScript, for the reason given. Delete a test only when its line names
fixtures or duplicates and no "not migrated" part. Remove a line once its
test is deleted, and delete this file when it is empty.

Paths are relative to `spec/conformance/`. Owner questions Q1 to Q5 are
listed at the end.

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
- test/requirement-key-validation.test.ts :: aliases are expanded before requirement keys are validated -> not migrated (owner question Q5)

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

- test/variance.test.ts :: distinct method binders preserve runtime arguments, dictionaries and captured values -> not migrated (owner question Q1; it also names a local value `T`, owner question Q2)
- test/variance.test.ts :: readonly public method inputs participate in nominal variance -> duplicate of typing/invalid/covariant-private-method-parameter.hd
- test/variance.test.ts :: readonly public method results participate in nominal variance -> typing/invalid/contravariant-method-result.hd
- test/variance.test.ts :: nested function parameter polarity reverses rather than becoming invariant -> typing/valid/covariant-callback-parameter.hd, typing/invalid/covariant-producer-callback-parameter.hd
- test/variance.test.ts :: mutable method signature positions are invariant -> typing/invalid/covariant-mut-method-parameter.hd
- test/variance.test.ts :: invariant method signature containers cannot hide variance violations -> typing/invalid/covariant-optional-method-parameter.hd
- test/variance.test.ts :: callable requirement rows are invariant in data and enum surfaces -> typing/invalid/covariant-data-requirement-row.hd, typing/invalid/contravariant-enum-requirement-row.hd
- test/variance.test.ts :: callable requirement rows stay invariant through method polarity -> typing/invalid/covariant-method-requirement-row.hd, typing/invalid/contravariant-method-requirement-row.hd
- test/variance.test.ts :: callable requirements unrelated to the nominal parameter remain valid -> typing/valid/covariant-unrelated-requirement-row.hd
- test/variance.test.ts :: method generic binders do not capture implementation parameters in Self -> not migrated (owner question Q1)
- test/variance.test.ts :: nested implementation targets compose their declared variance signs -> not migrated (owner question Q3)
- test/variance.test.ts :: an invariant implementation target does not impose signed method constraints -> not migrated (owner question Q3)
- test/variance.test.ts :: opposing target signs constrain a shared parameter to equality -> not migrated (owner question Q3)
- test/variance.test.ts :: trait arguments in method signatures retain their invariant polarity -> typing/invalid/covariant-trait-argument-parameter.hd
- test/variance.test.ts :: a separate trait implementation does not change nominal variance -> typing/valid/variance-separate-trait-impl.hd
- test/variance.test.ts :: enum inherent methods are included in the readonly public surface -> typing/invalid/covariant-enum-method-parameter.hd
- test/variance.test.ts :: suspending inherent methods have the same variance obligations -> typing/invalid/covariant-suspending-method-parameter.hd
- test/variance.test.ts :: associated construction and mutable receiver signatures are not readonly instance views -> not migrated (owner question Q4)
- test/variance.test.ts :: a public method cannot infer its result past the variance check -> duplicate of typing/invalid/public-method-missing-result-type.hd
- test/variance.test.ts :: private readonly inherent methods participate in declared variance -> duplicate of typing/invalid/covariant-private-method-parameter.hd and typing/invalid/contravariant-method-result.hd
- test/variance.test.ts :: inferred private inherent results cannot bypass variance checking -> typing/invalid/contravariant-inferred-private-result.hd, typing/valid/covariant-inferred-private-result.hd
- test/variance.test.ts :: shadowed method binders cannot erase Self-derived invariant inferred results -> not migrated (partial: typing/invalid/contravariant-inferred-invariant-result.hd covers the case without a shadowing binder; the shadowing cases wait for owner question Q1)
- test/variance.test.ts :: method-owned inferred and callable results remain independent of the receiver binder -> not migrated (owner question Q1)
- test/variance.test.ts :: method binder normalization preserves defaults, bounds and nested declaration scope -> not migrated (implementation detail: inspects the checker's binder renaming)
- test/variance.test.ts :: structural binder renaming retains type heads and labels while visiting rows and projections -> not migrated (implementation detail: inspects the checker's binder renaming)
- test/variance.test.ts :: a local nominal shadows a method binder from its declaration point without leaking out -> not migrated (owner questions Q1 and Q2)
- test/variance.test.ts :: qualified type owners rename binders but preserve unrelated owners -> not migrated (implementation detail: inspects the checker's binder renaming)
- test/variance.test.ts :: internal method identities retain Unicode binder spellings in diagnostics -> not migrated (implementation detail: diagnostic message text)
- test/variance.test.ts :: qualified owner resolution distinguishes type binders from lexical values -> not migrated (owner questions Q1 and Q2)
- test/variance.test.ts :: nested closure type scopes do not change later method annotations -> not migrated (implementation detail: inspects the checker's binder renaming)

## Owner Questions

- **Q1.** May a method's own type parameter reuse its implementation's
  parameter name, as `fn echo[T]` inside `impl[T] Box[T]`? The tests say
  yes, and the method's `T` is a separate parameter.
  [`names.type-param.shadow`](../spec/lang/03-names-and-scopes.md#r-names.type-param.shadow)
  allows shadowing only a module name.
- **Q2.** May a local name in a method body reuse a type parameter's name?
  The tests accept a local `data T`, and a local value `T` called as
  `T::to_string()`, inside `fn work[T]`. The spec does not say whether
  the type parameters share the body's outermost scope, or which `T` a
  `T::` owner picks.
- **Q3.** How is an inherent method checked against declared variance when
  its implementation's target is not the bare parameters, as in
  `impl[U] Box[Consumer[U]]`, `impl[U] Box[U?]`, or `impl[U] Pair[U, U]`?
  The tests multiply each method position by the sign of `U` in the target.
  An invariant position in the target imposes nothing.
- **Q4.** Do `mut self` methods count toward declared variance?
  [`types.variance.surface`](../spec/lang/04-type-system.md#r-types.variance.surface)
  includes every inherent method. The test accepts
  `pub fn set(mut self, value: U)` on `Box[+T]`.
- **Q5.** Is an alias whose target names nothing an error at the alias, as
  in `type MissingAlias = Missing`? The prototype accepts it there, and
  reports `unknown-trait` only where `$ MissingAlias` uses it.
