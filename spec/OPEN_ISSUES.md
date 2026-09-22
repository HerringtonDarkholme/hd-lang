# Specification Completion Register

This register tracks decisions that still prevent the draft from becoming a
complete implementable specification. It distinguishes required core decisions
from deliberately unsupported features and provisional designs.

## Core Decisions Required

None currently. New core issues must be added here when an implementation
attempt exposes underspecified behavior.

## Core Features Explicitly Deferred

These are not completion blockers. The v1 specification should reject or omit
them rather than leave implementation-defined behavior.

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

## Provisional Chapters

The following mechanisms are intentionally isolated under `provisional/` and
may be implemented experimentally without claiming stable-core conformance:

- requirement rows, provider contexts, and one-shot suspension;
- variadic generic packs and pattern expansion;
- GADT result refinement;
- typed annotations, associated types used by annotators, shape APIs, and
  recursive annotation materialization.

Each provisional chapter owns its detailed open issues. Stabilizing one requires
moving its final grammar and semantics into the numbered core chapters and
adding conformance fixtures.

## Resolution Process

Resolve core decisions one at a time. After accepting a decision:

1. update the owning chapter and consolidated EBNF;
2. remove the item from this register;
3. add valid and invalid conformance fixtures;
4. run `spec/check.sh`.
