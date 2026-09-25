# hd-lang MVP Compiler

This directory contains the executable Wasm GC MVP described in
[MVP_IMPLEMENTATION_PLAN.md](MVP_IMPLEMENTATION_PLAN.md). The implementation is
deliberately incremental: accepted programs compile to validated Wasm GC, and
features outside the current slice receive stable diagnostics.

## Run It

The repository pins Node 24.19.0 and npm dependencies through
`package-lock.json`.

```sh
npm install
npm run toolchain:gate
npm run hd -- check examples/core.hd
npm run hd -- build --wat examples/core.hd
npm run hd -- run examples/core.hd
npm run hd -- trace examples/suspension.hd
npm run hd -- record examples/suspension.hd
npm run hd -- replay examples/suspension.hd
npm run check
```

The package also exposes `bin/hd.js` as the `hd` executable when installed or
linked through npm.

## Implemented Surface

- indentation-sensitive lexing with source spans and structured diagnostics;
- documentation-comment attachment on AST declarations and members, with
  orphan diagnostics;
- named arguments for statically resolved functions and methods, including
  suspending and dynamic trait dispatch, plus enum payload constructors, with
  source-order evaluation;
- pure function-parameter defaults evaluated per call after all explicit
  arguments, including earlier-parameter references, erased generics, and
  suspending function construction;
- homogeneous `T...` parameters lowered as `list[T]`, with positional values,
  positional list spread, and named-list supply across ordinary, generic,
  suspending, static-trait, and dynamic-trait calls;
- first-class homogeneous-vararg function types and indirect calls using the
  same `list[T]` ABI;
- a recursive-descent declaration/statement parser and Pratt expression parser;
- named functions, forward calls, typed parameters, typed results, and locals;
- top-level `pub` visibility metadata for functions and nominal types, with
  private-signature leak checks and an MVP `Console` host-capability profile
  for public `main` entry points;
- named `test` blocks retained in the AST and checked as active driver bodies,
  including the same explicit-discard and must-use rules as functions, and
  emitted as internal `__hd_test_N` Wasm driver exports for the harness;
- complete explicit generic call arguments with per-slot `_` inference, plus
  indented zero-argument trailing callback blocks;
- stored function fields remain callable through readonly data views and
  preserve their declared result permission;
- named local functions lowered through typed closure bindings, including
  enclosing captures and recursion;
- `i32`, `f64`, `bool`, Unicode-scalar `char`, and UTF-8 `string` values;
- heterogeneous tuple literals, tuple types, simultaneous tuple destructuring,
  and statically typed numeric selection, stored in erased Wasm GC arrays;
- checked `i32` arithmetic and exponentiation, IEEE `f64` power, UTF-8 string
  concatenation, scalar and string comparisons, Wasm GC reference identity,
  boolean short-circuiting, and explicit panics;
- interpreted `$name` and `${expression}` string segments with left-to-right
  built-in display conversion for `string`, `i32`, `bool`, and `char`;
- `println` with the same built-in display surface, statically requiring a
  lexical `Console` provider and streaming UTF-8 from Wasm GC strings through
  the narrow host byte callback;
- value-producing `if`, statement `if`, `while`, value-producing `while ...
  else`, `break`, `break value`, and `continue`;
- list and insertion-ordered map `for` iteration with tuple destructuring and
  value-producing `for ... else`;
- lexical branch and loop scopes;
- data declarations, literals, and field reads backed by Wasm GC structs,
  including pure per-construction field defaults evaluated after explicit
  initializers and shallow copy-update with source-first evaluation;
- mutable data permissions with one-way `mut T` to `T` weakening, readonly
  aliases over shared identity, permission-aware direct and generic fields,
  mutable-path checking, and field assignment through Wasm GC `struct.set`;
- tagged enums, constructors, exhaustive matching, and payload bindings backed
  by Wasm GC structs, including shared constructor fields, pure ordered
  defaults, per-variant factories, named or numeric shared-field access, and
  canonical fieldless-variant identities;
- expected-type contextual enum constructors such as `.Ready(42)`;
- exhaustive boolean matching, guarded patterns, and literal matching for
  integers, floats, characters, and strings;
- contextual enum patterns and recursive nominal data patterns with field
  bindings and literal field constraints, plus named enum-payload bindings
  resolved independently of source order and literal, nested-data, or
  nested-enum payload constraints;
- erased optional and `Result` values, contextual constructors, exhaustive
  matching, recursive nominal payload patterns, must-use checking, and postfix
  propagation;
- named function values plus typed nested and recursive closures, expected-type
  parameter/result inference, result inference for nonrecursive closures, and
  GC environments for direct and transitive captures, including lexical
  provider overrides that escape their `$.with` scope;
- concrete requirement rows with hidden `externref` provider threading and
  transitive call paths from `hd explain-requirements`;
- normalized concrete row union/subtraction plus statically resolved `$.use`
  (including ordered multi-provider tuple lookup) and lexical `$.with`
  provider overrides;
- requirement-bearing closure types with invocation-time provider threading and
  least-row inference for requirements not satisfied by lexical providers;
- generic requirement-row parameters with least-row inference, symbolic and
  concrete row union, repeated-row consistency, symbolic subtraction, keyed
  Wasm GC provider packs, lexical restoration of removed providers, and
  absent-subtraction warnings;
- erased generic marker traits as provider keys, with call-site substitution
  and pre-erasure collision checking;
- erased generic functions with call-site type inference, Wasm GC boxing for
  primitive values, inference through optional and `Result` types, and
  higher-order callable adapters for erased type and requirement-row ABIs;
- erased generic suspending functions whose GC frames retain boxed values,
  trait dictionaries, and providers across polls;
- simple traits and explicit implementations with signature validation,
  concrete method lookup, ambiguity diagnostics, static dispatch, and dynamic
  Wasm GC trait values carrying erased receivers and typed method references,
  including `mut self` enforcement through static, dynamic, default, and
  generic-bound dispatch;
- inherent `impl Type:` methods lowered to direct typed functions, including
  named arguments, mutable receivers, suspending calls, and duplicate-member
  diagnostics;
- embedded data fields with direct method promotion and bodyless explicit
  trait opt-in for one compatible readonly promoted method; mutable promoted
  requirements are rejected at the embedded readonly edge;
- default trait methods with target-specific lowering, dynamic method-table
  entries, and explicit override precedence;
- suspending trait methods with typed dynamic GC-frame wrappers, trait-bound
  dispatch, stored driving, and cancellation forwarding;
- erased generic parameters with independent GC dictionaries for multiple
  trait bounds, dictionary forwarding, method dispatch on values produced
  inside generic bodies, and concrete call-site recovery for returned `T`
  values;
- generic data declarations with inferred construction arguments, precise
  instantiated member types, and uniform `anyref` field erasure in one Wasm GC
  layout per declaration;
- generic enums with inferred and contextual construction, recursive
  instantiations, precise pattern bindings, and uniform `anyref` payload
  erasure in one Wasm GC layout per declaration;
- trait values as lexical providers, including dispatch after generic
  requirement-row packing and subtraction;
- reusable `$.Context[...]` values backed by GC structs, `$.context` creation,
  and left-to-right context spreading into contexts and lexical scopes;
- stackless suspension frames with `fn!`, construction-time provider capture,
  direct and stored bang driving, explicit `mut Suspend[T]` bindings,
  child-pending propagation, local spilling, synchronous cancellation, and
  one-shot, competing-driver, and reentrant poll/cancel state traps;
- module-level single and grouped `use` syntax, with executable
  `std.task.block_on` support, a per-instance active-driver guard, and nested
  driver traps;
- imported `std.resource.ResourceError[E]` as the canonical generic
  `Operation(E) | Disposed` enum, using the same erased Wasm GC representation
  as source-declared generic enums;
- executable `std.testing.assert_equal` for supported scalar, string, tuple,
  and recursively nested list values, with `missing-partial-eq` at unsupported
  types;
- suspension CFG lowering for bang calls nested in expressions, call
  arguments, short-circuiting, branches, loops, match guards, propagation, and
  provider scopes, with scoped cleanup and cancellation;
- a frame-level poll ABI that returns readiness separately from the stored
  result, plus host-visible construction, poll, ready, cancellation, and
  invalid-state trace events;
- deterministic host pending fixtures with poll counts and GC-frame resumption;
- started-frame cancellation that cancels the active child before registered
  top-level cleanup, plus scalar development start/poll/cancel exports;
- suspension poll record/replay with function-name-based site identities that
  survive unrelated declaration insertion, source-derived function code
  identity, argument/result and provider configuration checks, and CLI sidecar
  commands;
- strings backed by Wasm GC byte arrays, with scalar-counting `string.len()`;
- non-suspending `defer` on normal completion, return, break, and continue;
- homogeneous `list[T]` literals, indexing, `len()`, and mutable `append()` over
  a growable Wasm GC vector with erased backing storage, plus indexed
  replacement through `mut list[T]`;
- insertion-ordered `map[K, V]` literals with duplicate replacement, optional
  indexed or `get()` lookup, `len()`, growable indexed insertion and
  `remove()` through `mut map[K, V]`, and erased Wasm GC key/value storage;
- typed HIR, readable WAT output, Binaryen validation, and V8 execution; and
- a curated frontend conformance gate tied to `spec/conformance/cases.tsv`,
  including stable rejection diagnostics, Wasm runtime panic cases, complete
  prelude-name shadow protection, and non-fatal unreachable-code, unused-local,
  and variant-binding-name-mismatch warnings.

The active boundary is intentionally narrower than the language specification.
Task combinator intrinsics, associated and generic trait members, provider-call
replay, and annotations remain in later MVP slices. The compiler
rejects syntax it recognizes from those slices rather than assigning placeholder
semantics; unresolved `all!` and `race!` calls report
`unsupported-task-combinator`.
Interpolation and `println` report `missing-display` for other displayed types until the
canonical prelude `Display` dictionary and floating-point formatting are part
of the executable standard-library trait slice.

## Layout

- `lexer.ts`, `parser.ts`, and `ast.ts` implement the source frontend.
- `checker.ts` resolves names and produces the typed nodes in `hir.ts`.
- `emitter.ts` lowers HIR to readable WAT.
- `suspension.ts` lowers suspending HIR into explicit resumable control flow.
- `wasm.ts` parses, validates, and emits Wasm with pinned Binaryen.
- `compiler.ts` exposes the in-process compiler API.
- `requirements.ts` computes transitive provider explanations.
- `cli.ts` implements the current command-line interface.
- `toolchain-gate.ts` proves the required Wasm GC operations independently of
  the language frontend.
- `../test/cli.test.ts` exercises the packaged CLI surface end to end.
