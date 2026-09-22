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

- Nested named declarations and recursive local closure bindings.
- Direct imports of enum variants.
- General top-level stored declarations distinct from script bindings.
- Direct composition of permission weakening with generic variance.
- Struct field defaults.
- Generic or explicitly mutable embedded-field shorthand.
- Struct destructuring patterns and match guards.
- Trait-value downcasting and runtime type tests.
- Package-private and member-level visibility.
- Wildcard imports.
- Stable object layout, field offsets, and user-visible addresses.
- Resource ownership, deterministic cleanup, and escape checking.

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
