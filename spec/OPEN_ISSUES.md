# Specification Completion Register

This register tracks decisions that still prevent the draft from becoming a
complete implementable specification. It distinguishes language decisions from
deliberately unsupported features and runtime or library work.

## Core Decisions Required

- Select the static `MissingAnnotationPolicy` representation and granularity for
  aggregate annotation derivation. Until selected, derivation with a missing
  child annotation is rejected as unsupported.

## Unsupported Or Backlog Language Features

These are not completion blockers. Implementations must reject or omit them
rather than leave implementation-defined behavior.

The dispositions below are triage suggestions, not accepted language
decisions:

- **Support candidate** means the feature appears useful and reasonably aligned
  with the language.
- **Discuss** means the feature has a legitimate use case but needs focused
  design work.
- **Deferred** means the behavior is deliberately undecided and is not a
  permanent exclusion.
- **Retain exclusion** means the existing restriction remains consistent with
  accepted design goals.

### Lexical Surface

- **Retain exclusion:** primitive byte/bytes types and byte literals.

### Declarations, Scope, Modules, And Visibility

- **Retain exclusion:** direct imports of enum variants; contextual `.Variant`
  syntax is available when the enum type is known.
- **Accepted:** top-level `:=` and `let` bindings remain accessible to later
  function bodies in the same module; no distinct stored declaration is needed.
- **Retain exclusion:** function and method overloading.
- **Retain exclusion:** source-level `module` or `package` declarations in the
  path-inferred module system.
- **Retain exclusion:** wildcard imports.
- **Retain exclusion:** package-private visibility; declarations are
  module-private by default or `pub`.
- **Accepted:** enum variants inherit enum visibility; data fields and
  inherent methods are module-private unless individually marked `pub`.
  Trait methods share their trait's visibility and cannot mark `pub` separately.
- **Retain exclusion:** nested `test` blocks.

### Expressions, Patterns, And Control Flow

- **Retain exclusion:** truthiness conversions; conditions require `bool`.
- **Retain exclusion:** user-defined operator overloading.
- **Retain exclusion:** comparison chaining; combine comparisons with `and`.
- **Accepted:** `bool` match guards; guarded arms do not establish
  exhaustiveness.
- **Accepted:** data destructuring patterns with shorthand field binding,
  `field=pattern`, and implicit omission of unlisted fields.
- **Deferred:** first-class bound-method values. Direct `receiver.method()`
  calls are defined, but bare `receiver.method` as a value is not. Revisit the
  reference syntax, `self` versus `mut self` receiver capture and lifetime,
  and how suspension and requirements appear in the resulting function type.
- **Retain exclusion:** automatic conversion of heterogeneous collection
  literals to collections of `Any`.
- **Discuss:** user-defined ordering for aggregates and collections.
- **Discuss:** a dedicated present-value optional pattern.
- **Retain exclusion:** suspension calls inside comprehensions.
- **Retain exclusion:** a special comprehension `let` clause; `:=` remains the
  binding form inside expressions.
- **Retain exclusion:** catchable runtime panic or language-level unwinding.

### Functions And Closures

- **Discuss:** recursive local closure bindings.
- **Discuss:** partial explicit generic argument lists.
- **Discuss:** generic argument placeholders.
- **Support candidate:** explicit generic arguments on methods.
- **Retain exclusion:** shorthand-argument or arrow closure syntax.
- **Retain exclusion:** non-local returns from closures.
- **Retain exclusion:** multiple trailing callback blocks or trailing callbacks
  with parameters.
- **Retain exclusion:** ownership-taking receivers and reference sigils.
- **Discuss:** reassignable parameter bindings.

### Data Types, Enums, Representation, And Resources

- **Support candidate:** defaults for data fields.
- **Discuss:** defaults for shared enum constructor data.
- **Retain exclusion:** inline enum variant field blocks; variants use
  call-style constructor parameters.
- **Discuss:** generic embedded-field shorthand.
- **Discuss:** explicitly mutable embedded-field shorthand.
- **Retain exclusion:** stable object layout, field offsets, variant tags,
  object addresses, and representation identity as core-language semantics.
- **Discuss:** weak references and user-visible finalizers.
- **Discuss:** resource ownership, deterministic cleanup, alias escape, and
  use-after-disposal checking.

### Traits And Types

- **Discuss:** runtime type tests for dynamic trait values.
- **Discuss:** dynamic trait-value downcasting.
- **Retain exclusion:** implicit structural trait conformance.
- **Retain exclusion:** trait implementation specialization.
- **Discuss:** negative trait implementations.
- **Discuss:** direct composition of permission weakening with generic
  variance, such as `mut Cell[Cat]` to `Cell[Animal]`.
- **Discuss:** user-defined stable equality and hashing for map keys.
- **Retain exclusion:** implicit signed/unsigned and integer/floating-point
  conversions.
- **Retain exclusion:** upgrading const access `T` to mutable access `mut T`.

### Variadic Generics

- **Discuss:** general pack mapping.
- **Discuss:** pack filtering.
- **Discuss:** pack indexing.
- **Discuss:** pack splitting and concatenation.
- **Discuss:** pack length arithmetic.
- **Discuss:** iteration over a pack as a runtime sequence.

Pattern expansion remains the only accepted pack transformation unless one of
these operations is selected explicitly.

### Annotations

- **Discuss:** decorator syntax as exact sugar for member metadata.
- **Discuss:** wildcard or generic-family annotation derivation syntax beyond
  ordinary generic `impl Annotate[A] for Target` declarations.
- **Discuss:** replacing an entire derivation rather than overriding aggregate
  `build` after member mapping.
- **Retain exclusion:** function-parameter metadata assignment overrides.
- **Retain exclusion:** metadata overrides of promoted fields.
- **Retain exclusion:** implicit default annotations and downstream replacement
  of an authoritative library annotation.

## Library Or ABI Decisions

These require specifications, but they belong to the standard library, package
tooling, or runtime rather than core syntax.

- Checked panic reporting and host representation for overflow, invalid shifts,
  failed casts, bounds errors, and related defects.
- The complete `hd.toml` schema, lockfile, and dependency resolver.
- Wasm component ABI and registration adapters.

## Resolution Process

Resolve core decisions one at a time. After accepting a decision:

1. update the owning chapter and consolidated EBNF;
2. remove the item from this register;
3. add valid and invalid conformance fixtures;
4. run `spec/check.sh`.
