# hd-lang MVP Compiler

This directory contains the executable Wasm GC MVP described in
[MVP_IMPLEMENTATION_PLAN.md](MVP_IMPLEMENTATION_PLAN.md). The implementation is
deliberately incremental: accepted programs compile to validated Wasm GC, and
features outside the current slice receive stable diagnostics.

Small pipeline stages remain direct modules such as `lexer.ts`, `ast.ts`,
`hir.ts`, and `wasm.ts`. Larger stages use same-named folders (`parser/`,
`checker/`, and `emitter/`). Each folder exposes its public surface only from
`index.ts`; consumers do not import its internal files.

## Run It

The repository pins Node 24.19.0 and npm dependencies through
`package-lock.json`.

```sh
npm install
npm run toolchain:gate
npm run lint
npm run format:check
npm run test:portable
npm test
npm run hd -- parse spec/conformance/parse/valid/layout.hd
npm run hd -- check examples/core.hd
npm run hd -- test spec/conformance/runtime/valid/defer-order.hd
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
- requirement-free function-parameter defaults evaluated per call after all explicit
  arguments, including earlier-parameter references, erased generics, and
  suspending function construction;
- homogeneous `T...` parameters lowered as `list[T]`, with positional values,
  positional list spread, and named-list supply across ordinary, generic,
  suspending, static-trait, and dynamic-trait calls;
- first-class homogeneous-vararg function types and indirect calls using the
  same `list[T]` ABI;
- a recursive-descent declaration/statement parser and Pratt expression parser;
- named functions, forward calls, typed parameters, typed results, and locals;
- source-ordered module bindings backed by typed Wasm globals, including
  function reads and reassignment of top-level `let` bindings, binding-point
  visibility, and transitive initialization checks through referenced
  functions and closures; a Wasm start function runs module initialization
  exactly once before either a script entry or a declared `main`;
- top-level `pub` visibility metadata for functions and nominal types, with
  private-signature leak checks and an MVP `Console` host-capability profile
  for public `main` entry points; named runtime profiles can admit explicit
  user trait capabilities;
- named `test` blocks retained in the AST and checked as active driver bodies,
  including the same explicit-discard and must-use rules as functions, and
  emitted as internal `__hd_test_N` Wasm driver exports for the harness;
- complete explicit generic call arguments with per-slot `_` inference for
  functions and inherent methods, plus indented zero-argument trailing
  callback blocks;
- stored function fields remain callable through readonly data views and
  preserve their declared result permission;
- first-class `fn!` values for named suspending functions and capturing
  suspending closures, including ordinary construction, direct bang calls,
  provider forwarding, and one-way weakening to `fn(...) -> mut Suspend[T]`;
- the sealed prelude `Waker` trait is available as a dynamic value at the
  suspension boundary, including retention through ordinary functions;
- named local functions lowered through typed closure bindings, including
  enclosing captures, recursion, suspension, and requirement forwarding;
- `i32`, `f64`, `bool`, Unicode-scalar `char`, and UTF-8 `string` values;
- heterogeneous tuple literals, tuple types, simultaneous tuple destructuring,
  and statically typed numeric selection, stored in erased Wasm GC arrays;
- checked `i32` arithmetic and exponentiation, IEEE `f64` power, UTF-8 string
  concatenation, scalar and string comparisons, Wasm GC reference identity,
  boolean short-circuiting, and explicit panics;
- interpreted `$name` and `${expression}` string segments with left-to-right
  canonical `Display` dispatch for concrete implementations, generic bounds,
  dynamic trait values, and the standard `string`, `i32`, `f64`, `bool`, and
  `char` implementations;
- `println` with the same display surface, statically requiring a
  lexical `Console` provider and streaming UTF-8 from Wasm GC strings through
  the narrow host byte callback;
- suspending host capability methods with scalar and UTF-8 string arguments
  and results, using opaque per-call tokens and a byte-stream bridge that keeps
  Wasm GC references inside Wasm;
- JSON-safe host-provider replay values with tagged integers, exact IEEE-754
  `f64` bits, and exact hex-encoded UTF-8 bytes;
- value-producing `if`, statement `if`, `while`, value-producing `while ...
else`, `break`, `break value`, and `continue`;
- list and insertion-ordered map `for` iteration with tuple destructuring and
  value-producing `for ... else`;
- eager list and map comprehensions with ordered nested clauses, conditional
  filters, lexical clause bindings, duplicate map-key replacement, and a
  compile-time ban on suspension calls;
- right-associative single and tuple binding expressions with enclosing-scope
  visibility, readonly inferred bindings, and flow-sensitive initialization
  across short-circuit conditions;
- lexical branch and loop scopes;
- data declarations, literals, and field reads backed by Wasm GC structs,
  including requirement-free per-construction field defaults evaluated after explicit
  initializers and shallow copy-update with source-first evaluation;
- mutable data permissions with one-way `mut T` to `T` weakening, readonly
  aliases over shared identity, permission-aware direct and generic fields,
  mutable-path checking, and field assignment through Wasm GC `struct.set`;
- tagged enums, constructors, exhaustive matching, and payload bindings backed
  by Wasm GC structs, including shared constructor fields,
  requirement-free ordered defaults, per-variant factories, named or numeric shared-field access, and
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
  erased method-level generics with bounds, explicit or inferred type
  arguments, named arguments, mutable receivers, suspending calls, and
  duplicate-member diagnostics;
- receiverless associated functions called through `Type::function`, including
  `Self` substitution, method-level generics, and suspending calls;
- blanket trait implementations over generic targets, with unified target and
  trait-argument inference; their adapters materialize static, dynamic, and
  bound dictionaries for ordinary and suspending methods; bounded blanket
  dictionaries capture nested dictionaries, including when forwarded from a
  caller or retained by a parent supertrait;
- embedded data fields with direct field and method promotion, including
  generic substitution through the embedded edge, plus bodyless explicit trait
  opt-in for one compatible readonly promoted method; mutable promoted
  requirements are rejected at the embedded readonly edge;
- default trait methods with target-specific lowering, dynamic method-table
  entries, and explicit override precedence;
- generic supertraits substitute parent arguments through inherited calls and
  checked trait-value widening; child dictionaries retain blanket or concrete
  parent implementations;
- suspending trait methods with typed dynamic GC-frame wrappers, trait-bound
  dispatch, stored driving, and cancellation forwarding;
- erased generic parameters with independent GC dictionaries for multiple
  trait bounds, dictionary forwarding, method dispatch on values produced
  inside generic bodies, and concrete call-site recovery for returned `T`
  values;
- concrete and bounded generic `PartialEq` and `PartialOrd` dispatch, with
  structural equality for tuples, lists, optionals, `Result`, and maps and
  lexicographic tuple/list plus nil-first optional ordering, recursively using
  explicit implementations and erased bound dictionaries for nested values;
  primitives and those built-in composites also satisfy `PartialEq` and
  `PartialOrd` bounds, and primitives satisfy `Display` bounds and become
  `Display` trait values, through generated standard-library dictionaries;
- generic data declarations with inferred or complete explicit construction
  arguments, precise instantiated member types, and uniform `anyref` field
  erasure in one Wasm GC layout per declaration;
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
- uniform Wasm GC `Suspend[T]` wrappers with concrete-frame poll, cancel, and
  boxed-result references, preserving identity through data fields, optionals,
  generic function parameters, and aliases;
- module-level single and grouped `use` syntax, with executable
  `std.task.block_on` support, a per-instance active-driver guard, and nested
  driver traps;
- imported `std.resource.ResourceError[E]` as the canonical generic
  `Operation(E) | Disposed` enum, using the same erased Wasm GC representation
  as source-declared generic enums;
- executable `std.testing.assert` with source-order argument evaluation, plus
  `assert_equal` for supported scalar, string, tuple, list, optional, `Result`,
  and order-independent map values and for explicit nominal or bounded generic
  `PartialEq` implementations, with mandatory reasons and
  `missing-partial-eq` at unsupported types;
- suspension CFG lowering for bang calls nested in expressions, call
  arguments, short-circuiting, branches, loops, match guards, propagation, and
  provider scopes, with scoped cleanup and cancellation;
- a frame-level poll ABI that returns readiness separately from the stored
  result, plus host-visible construction, poll, ready, cancellation, and
  invalid-state trace events;
- deterministic host pending fixtures with poll counts and GC-frame resumption,
  including a portable CLI scenario that cancels a root while a named nested
  frame is pending and checks its source-defined cleanup result;
- a `pending-gate` runtime profile that wraps an opaque host provider as a Wasm
  GC trait value, polls its suspending method, forwards cancellation, and
  verifies the original provider-backed cleanup fixture;
- scalar host-provider method arguments and results for `i32`, `f64`, `bool`,
  and `char`, with an opaque per-invocation token and provider poll replay that
  restores recorded readiness and scalar results without calling the live host;
- JSON-safe tagged scalar replay values, with exact IEEE-754 bit strings for
  `f64` values such as negative zero, infinities, and NaN;
- started-frame cancellation that cancels the active child before registered
  top-level cleanup, plus scalar development start/poll/cancel exports;
- suspension poll record/replay with function-name-based site identities that
  survive unrelated declaration insertion, source-derived function code
  identity, argument/result and provider configuration checks, and CLI sidecar
  commands;
- strings backed by Wasm GC byte arrays, with scalar-counting `string.len()`;
- Unicode `string.trim()` and default-case `string.lower()` through a bytewise
  host bridge that reconstructs the result as a Wasm GC byte array;
- `string.split()` implemented in WAT, retaining boundary empty pieces and
  splitting an empty separator into Unicode scalar strings;
- non-suspending `defer` on normal completion, return, break, and continue;
- homogeneous `list[T]` literals, indexing, `len()`, and mutable `append()` over
  a growable Wasm GC vector with erased backing storage, plus indexed
  replacement through `mut list[T]`;
- insertion-ordered `map[K, V]` literals with duplicate replacement, optional
  indexed or `get()` lookup, `len()`, growable indexed insertion and
  `remove()` through `mut map[K, V]`, and erased Wasm GC key/value storage;
- built-in list and map `iter()` values as mutable Wasm GC cursors whose
  `next()` yields `T?`; explicit and `for`-loop iteration share exhaustion,
  partly consumed cursor, replacement, and structural invalidation behavior;
- explicit `Iterator[T]` implementations participate in ordinary `for` loops
  and comprehensions through their mutable `next()` method;
- typed HIR, readable WAT output, Binaryen validation, and V8 execution; and
- an implementation-neutral conformance gate tied to
  `spec/conformance/cases.tsv`, invoked through the public CLI by a concurrent
  TypeScript runner, including stable rejection diagnostics, Wasm runtime panic cases,
  complete prelude-name shadow protection, and non-fatal unreachable-code,
  unused-local, and variant-binding-name-mismatch warnings.

Selected runtime failures cross the development host boundary with stable
codes, including explicit panic, assertions, integer overflow and division,
invalid shifts, list bounds, iterator invalidation, and suspension driver/state
failures. Portable panic fixtures verify the declared code rather than
accepting an arbitrary Wasm trap.

The active boundary is intentionally narrower than the language specification.
Task combinator intrinsics, strings and structural values in the host-provider
ABI, and annotations remain in later MVP slices. The compiler rejects syntax it
recognizes from those slices rather than assigning placeholder semantics;
unresolved `all!` and `race!` calls report `unsupported-task-combinator`.
Interpolation and `println` report `missing-display` when the displayed type
does not implement the canonical prelude trait.

## Layout

- `lexer.ts` and `ast.ts` define the small source-frontend stages.
- `parser/` builds the AST and exposes its public API from `parser/index.ts`.
- `checker/` resolves names and produces the typed nodes in `hir.ts`.
- `emitter/` lowers HIR to readable WAT and exposes only `emitter/index.ts`.
- `suspension.ts` lowers suspending HIR into explicit resumable control flow.
- `wasm.ts` parses, validates, and emits Wasm with pinned Binaryen.
- `compiler.ts` exposes the in-process compiler API.
- `requirements.ts` computes transitive provider explanations.
- `cli.ts` implements the current command-line interface.
- `toolchain-gate.ts` proves the required Wasm GC operations independently of
  the language frontend.
- `../test/portable/cases.tsv` selects portable `.hd` conformance fixtures;
  `../test/run-portable.ts` runs them through `hd parse`, `hd check`, and
  `hd test` without importing compiler internals.
- `../test/cli.test.ts` exercises the packaged CLI surface end to end.
