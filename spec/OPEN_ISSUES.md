# Specification Completion Register

This register tracks decisions that still prevent the draft from becoming a
complete implementable specification. It distinguishes language decisions from
deliberately unsupported features and runtime or library work.

## Core Decisions Required

No unresolved core decision is currently listed here.

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

- **Retain exclusion:** direct uses of enum variants; contextual `.Variant`
  syntax is available when the enum type is known.
- **Accepted:** top-level `:=` and `let` bindings remain accessible to later
  function bodies in the same module; no distinct stored declaration is needed.
- **Retain exclusion:** function and method overloading.
- **Retain exclusion:** source-level `module` or `package` declarations in the
  path-inferred module system.
- **Retain exclusion:** wildcard uses.
- **Retain exclusion:** package-private visibility; declarations are
  module-private by default or `pub`.
- **Accepted:** enum variants inherit enum visibility; data fields and
  inherent methods are module-private unless individually marked `pub`.
  Trait methods share their trait's visibility and cannot mark `pub` separately.
- **Retain exclusion:** nested `test` blocks.

### Expressions, Patterns, And Control Flow

- **Retain exclusion:** truthiness conversions; conditions require `bool`.
- **Retain exclusion:** user-defined arithmetic and bitwise operator
  overloading; comparison traits are the explicit exception.
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
- **Accepted:** `PartialEq`/`Eq` and `PartialOrd`/`Ord` back comparison
  operators. User data and enums require explicit implementation or derivation;
  `is` compares composite reference identity without invoking equality.
- **Accepted:** `@derive(Trait, ...)` on data and enums is a compiler intrinsic
  that generates ordinary, checked trait implementations. It is not an
  `Annotation` or `DataAnnotator`.
- **Accepted:** derived structural equality includes every declared data field
  (including embedded fields) and every enum payload field; distinct variants
  are unequal. There is no implicit field exclusion or cycle detection;
  recursively comparing cyclic graphs may exhaust the stack.
- **Accepted:** `@derive(PartialOrd, Ord)` compares data fields lexicographically
  in declaration order and orders enum variants by declaration order, then
  compares shared data and payload fields. Unordered fields propagate through
  `PartialOrd`.
- **Accepted:** `@derive(Hash)` for data and enums hashes all declared fields
  and the enum variant identity. User-defined map keys still need explicit
  `Eq + Hash` conformance, whether handwritten or derived; agreement of custom
  implementations is not enforced.
- **Accepted:** `T` implicitly constructs a present `T?`, and `nil` constructs
  absence. In patterns, `value?` matches and binds the present value; no
  built-in `Some(...)` constructor or pattern is used for `T?`.
- **Retain exclusion:** suspension calls inside comprehensions.
- **Retain exclusion:** a special comprehension `let` clause; `:=` remains the
  binding form inside expressions.
- **Retain exclusion:** catchable runtime panic or language-level unwinding.

### Functions And Closures

- **Accepted:** a directly bound local closure may refer to its own binding in
  its body when its return type is explicit. Nonrecursive closures may infer
  their return type; closure parameter types may be contextual.
- **Retain exclusion:** partial explicit generic argument lists. A call either
  infers every generic argument or supplies the complete list explicitly.
- **Accepted:** `_` may occupy a slot in a complete explicit generic argument
  list for a named function. Each placeholder is inferred; unresolved
  placeholders are errors. `_` is not an ordinary type argument.
- **Accepted:** generic methods accept complete explicit generic argument lists
  with the same `_` placeholder rules as named module functions.
- **Retain exclusion:** shorthand-argument or arrow closure syntax.
- **Retain exclusion:** non-local returns from closures.
- **Retain exclusion:** multiple trailing callback blocks or trailing callbacks
  with parameters.
- **Retain exclusion:** ownership-taking receivers and reference sigils.
- **Retain exclusion:** reassignable parameter bindings. Parameter names are
  immutable bindings, independently of whether their types grant mutable
  access. Copy the value into a `let` local when rebinding is needed.

### Data Types, Enums, Representation, And Resources

- **Accepted:** ordinary named data fields may have pure defaults, evaluated
  once per construction when omitted; embedded fields remain required.
- **Accepted:** shared enum constructor parameters may have pure defaults with
  function-default ordering and evaluation rules; variant payload parameters
  remain required.
- **Retain exclusion:** inline enum variant field blocks; variants use
  call-style constructor parameters.
- **Accepted:** generic data types may be embedded with type arguments; the
  final type name remains the field name, and duplicate names are rejected.
- **Accepted:** data members have no standalone `mut` modifier. A named field
  may have a `mut T` type, but embedded fields cannot have a mutable type or
  mutable-member modifier; use a named field for a mutable edge.
- **Accepted:** a `mut T` root may reassign any field; mutation through a child
  or a `mut self` call additionally requires a `mut`-typed direct field. A
  readonly data view reads a direct `mut U` field as `U` and may be constructed
  with `U` in that field. Constructing a `mut` outer view requires `mut U`.
  A generic `field: P` retains substituted `P`, including `mut U`.
- **Accepted:** copy-update reads each copied field through the spread source's
  view. A readonly spread can fill a direct `mut U` field in a readonly copy;
  a mutable copy requires a `mut U` replacement. Generic fields retain their
  substituted type.
- **Accepted:** `mut T` permits assignment to visible fields, but validation
  and cross-field invariants are not automatically enforced by field writes.
  Keep invariant-bearing fields private and expose controlled methods for
  their writes; independently held mutable aliases to stored children remain
  allowed and are not governed by the parent's setter.
- **Accepted:** readonly is an access-path permission, not object immutability
  or stable/snapshot reads. Ordinary callable values may return mutable aliases
  under their declared result types. Structural extraction sites apply the
  permission rule defined for that form; direct data fields and generic fields
  intentionally differ.
- **Retain exclusion:** stable object layout, field offsets, variant tags,
  object addresses, and representation identity as core-language semantics.
- **Deferred:** weak references and user-visible finalizers. Neither is
  supported now; revisit them separately from resource cleanup.
- **Deferred:** resource ownership, deterministic cleanup, alias escape, and
  use-after-disposal checking. The core language does not currently prevent a
  resource from escaping before a deferred cleanup invalidates it.

### Traits And Types

- **Deferred:** runtime type tests for dynamic trait values and downcasting
  from trait values to concrete or other trait types. Erased values remain
  opaque for now; `is` continues to mean reference identity.
- **Retain exclusion:** implicit structural trait conformance.
- **Retain exclusion:** trait implementation specialization.
- **Deferred:** negative trait implementations. There is no current use case
  requiring them; explicit conformance and the existing overlap rules remain.
- **Deferred:** direct composition of permission weakening with generic
  variance, such as `mut Cell[Cat]` to `Cell[Animal]`.
- **Accepted:** a direct generic data field declared `field: P` reads as its
  substituted type. For `Box[mut User]`, `value: P` reads as `mut User` even
  through a readonly `Box` and an erased generic getter may return `P`.
- **Accepted:** indexing or iterating `list[T]` yields `T`, including `mut U`
  when `T = mut U`, even through a readonly list. Replacing an element still
  requires a mutable list root. Covariant weakening to `list[U]` remains an
  explicit type conversion and removes mutable element access through that
  converted view.
- **Accepted:** lookup or iteration on `map[K, V]` preserves `V`, including
  `mut U` when `V = mut U`, even through a readonly map; replacing entries
  requires a mutable map. Lookup returns `V?`, and present-value unwrapping
  preserves that generic `V`.
- **Accepted:** generic contents retain their declared type through readonly
  wrappers. Optional and `Result` unwrapping preserve `T`, including `mut U`;
  tuple extraction preserves each declared element type, and generic enum
  payloads preserve their substituted type. Direct non-generic mutable fields
  or payloads still follow the enclosing view's permission.
- **Accepted:** `map[K, V]` requires `K: Eq + Hash` and excludes `mut T` key
  types. User data and enums may implement both traits. The language does not
  require or verify equal hashes for keys that `Eq` considers equal. If a key
  changes through another mutable alias while stored, the map does not
  reindex it; lookup or removal can miss an entry still visible in iteration.
- **Retain exclusion:** implicit signed/unsigned and integer/floating-point
  conversions.
- **Retain exclusion:** upgrading const access `T` to mutable access `mut T`.

### Variadic Generics

- **Accepted:** `pack.map` maps a heterogeneous tuple through a named generic
  function to another tuple; `pack.map_list` collects homogeneous mapper
  results. These operations support the per-child state and readiness steps
  of a library `all!` driver without first-class polymorphic function values.
- **Deferred:** pack filtering, indexing, splitting and concatenation, length
  arithmetic, and runtime iteration. None is needed by `all!`; revisit only
  when a concrete use case requires one.

Pattern expansion and tuple mapping are the accepted pack transformations.
Other pack operations are outside the current design.

### Annotations

- **Accepted:** prefix `@Facet` on module-level data, enum, and function
  declarations expands to `annotate Facet for Target: pass`, then to
  `impl Annotate[Facet] for Target`.
- **Accepted:** prefix `@value` on a named or embedded data field or enum
  variant expands to its member's `annotate Target` metadata; order and
  duplicate checks match explicit member metadata lists. Embedded-field
  metadata is checked against `FieldMetadata[EmbeddedType]`, attaches only to
  that field's own shape, and does not propagate to promoted members.
- **Accepted:** typed parameter decorators on module-level named functions.
  They attach `ParamMetadata[T]` values to `ParamShape` before `map_param` and
  lower through `annotate Function` parameter metadata.
- **Accepted:** configured declaration-facet values. A decorator expression's
  static annotation type determines coherence; its ordinary value is evaluated
  once and reused for mapping and `build`.
- **Retain exclusion:** decorators and `annotate` blocks on local declarations.
  Annotation coherence and memoization remain package-global.
- **Accepted:** `@derive(PartialEq, Eq)` uses the compiler-intrinsic path, not
  annotation lowering. No equality is derived implicitly.
- **Accepted:** generic-family `annotate` declarations use generic binders and
  optional `where` clauses, lowering to ordinary generic `Annotate` impls with
  the same coherence and overlap rules. No unconstrained wildcard target is
  introduced.
- **Retain exclusion:** a separate whole-derivation replacement hook inside
  `annotate`. Use direct `impl Annotate[Facet] for Target` to bypass child
  resolution, member mapping, and aggregate `build` entirely.
- **Retain exclusion:** exact function-parameter `ParamTarget` result
  overrides inside `annotate Facet for Function`; parameter metadata remains
  available through `annotate Function` and prefix decorators.
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
