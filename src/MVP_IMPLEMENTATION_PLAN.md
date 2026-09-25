# Wasm GC MVP Implementation Plan

**Status:** chosen MVP implementation direction, 2026-09-24

**Implementation status:** the S-1 toolchain gate is complete. The executable
S0 gate now checks every currently supported manifest parser case and a
checked-in structural AST snapshot. Documentation comments retain their joined
text on AST declarations and members, and unattached comments receive the
specified diagnostic. The parser also preserves named-argument, vararg, and
positional-spread markers and enforces their ordering rules. Named arguments
execute for statically resolved functions and methods, including suspending and
dynamic trait dispatch, and for enum payload constructors, while preserving
source evaluation order. Pure function-parameter defaults lower through hidden
helpers and run after explicit arguments in declaration order, including for
generic and suspending functions. Homogeneous
varargs use the existing `list[T]` ABI and support positional elements, one
positional list spread, or a named list across ordinary, generic, suspending,
static-trait, and dynamic-trait calls. First-class `fn(T...) -> U` values retain
the same list ABI and calling convention. Data-field defaults are checked
transitively for purity and evaluated once per construction, after explicit
initializers and in declaration order. Data copy-update evaluates one leading
source once, then explicit replacements in source order, and constructs a fresh
Wasm GC struct without invoking defaults for copied fields. The executable S1 core covers functions
and named function values, nested and recursive
closures with type inference and transitive captures, named local functions
sharing the closure representation, explicit generic call lists with `_`
inference, contextually typed trailing callback blocks, and stored callable
fields whose result permissions survive readonly outer access, GC data and enums,
including fieldless data as empty GC structs and shared enum constructor data
lowered through hidden per-variant factories with pure ordered defaults,
contextual enum construction, guarded literal and recursive data
patterns, named enum-payload pattern bindings, literal, nested-data, and
nested-enum payload constraints, value-producing while loops, checked integer
and floating power, characters, UTF-8 string concatenation and ordering,
interpreted-string segments with intrinsic `string`, `i32`, `bool`, and `char`
display conversion in source order. The same temporary display surface powers
`println`, which statically resolves a lexical `Console` provider and streams
UTF-8 bytes from its Wasm GC string to a narrow host callback,
numeric access to unnamed shared enum fields, and reference identity through
Wasm GC `ref.eq`, including sealed `Reference` generic bounds and canonical
fieldless enum variants. Mutable data access retains `mut T` in typed HIR,
weakens it only toward `T`, checks readonly roots and stored edges, applies the
direct-versus-generic field rule during construction and access, and lowers
field writes through mutable Wasm GC struct fields. Heterogeneous tuple literals and types retain exact
element types in HIR and use erased Wasm GC array storage; simultaneous tuple
bindings stage the initializer once before introducing their names;
list and map `for` loops evaluate their iterable once, preserve map insertion
order, support tuple entry bindings, and share the existing loop-else and
suspension-CFG semantics; mutable list and map roots support checked indexed
replacement while readonly roots retain their nested generic element
permissions;
optional/result propagation, panic, and non-suspending cleanup. S1 conformance
breadth remains in progress. The checker preserves typed HIR when it emits
non-fatal unreachable-code, unused-local, and variant-binding-name-mismatch
warnings. It protects the complete prelude namespace from declaration and
binding shadowing. Top-level `pub` functions and nominal types retain visibility
in the AST; public signatures reject private type and requirement leaks, and
public `main` rows are checked against the MVP host profile, currently
`Console`. Named test blocks lower to hidden active-driver functions for type
checking and harness-only Wasm exports, so explicit discard and must-use
diagnostics apply uniformly and runtime fixtures execute the same generated
code as entry functions. The
curated gate asserts stable diagnostics for the syntax
and type errors it currently covers and executes supported manifest panic cases
through Wasm GC.
The remaining interpolation boundary is the canonical prelude `Display`
dictionary: unsupported embedded types receive `missing-display` until that
standard trait and floating-point formatting are executable.
S2 has concrete-row normalization, hidden
provider threading, lookup, lexical overrides, reusable GC-backed contexts,
ordered multi-provider tuple lookup, left-to-right context spreads,
provider-aware nested calls, lexical provider
capture by escaping closures, requirement-bearing closure types with concrete
least-row inference, grouped and empty concrete rows, and transitive
requirement-path explanations. S2 now also has generic row inference, least-row
union, symbolic subtraction, keyed GC provider packs, lexical restoration of a
subtracted key, composition of multiple symbolic rows with concrete keys,
repeated-row mismatch checking, and the non-fatal
`requirement-subtract-absent` warning. Erased generic marker traits now provide
canonical instantiated keys, substitute through generic calls, accept distinct
concrete keys, and reject key expressions that can collide under substitution.
Trait-backed provider examples with methods remain coupled to S4. The current surface and commands are tracked in
[README.md](README.md).
S3 now has cold GC-frame construction, construction-time provider capture,
direct and stored bang driving, one-shot state checks, synchronous
cancellation, and invalid-state traps. Module `use` declarations retain their
imported names, and `std.task.block_on` drives an explicitly mutable stored
suspension from non-suspending code. A per-instance driver guard rejects
nested and competing drivers, and polling state rejects reentrant poll or
cancel calls through the host boundary. Linear sequential bang calls lower to
explicit child-frame continuation states: child `Pending` propagates to the
parent, live locals spill into the parent frame, and later polls resume the
child before continuing. General suspension CFG lowering preserves
left-to-right evaluation through nested call arguments, short-circuiting,
branches, loops, pattern guards, propagation, provider scopes, and scoped
cleanup. Started cancellation cancels the active child and runs registered
`defer` suites. A deterministic host fixture controls pending polls, and the
trace ABI exposes state and cleanup transitions. The `all!` and `race!`
intrinsics remain blocked on their unresolved standard signatures and receive
the dedicated `unsupported-task-combinator` diagnostic.

The first imported standard testing intrinsic, `assert_equal`, executes
structural equality for the MVP scalar, string, tuple, and list surface and
retains the `PartialEq` boundary for unsupported nominal values.

S4 has begun with inferred erased generic functions, `anyref` ABI lowering,
primitive Wasm GC boxing, and substitution through optional and `Result`
types. Generic suspending functions use the same erased ABI and retain boxed
values and trait dictionaries in their GC frames across polls. Higher-order
generic callable adapters now bridge concrete closure ABIs to erased type and
requirement-row parameters. The initial trait slice parses
simple required-method traits and explicit implementations, checks missing,
mismatched, duplicate, and ambiguous methods, performs concrete static
dispatch, and lowers dynamic trait values to GC objects containing an erased
receiver and typed method references. Trait values also work as lexical
providers and inside generic requirement packs. A first erased dictionary path
supports independent dictionaries for multiple trait bounds, method dispatch
on bounded values loaded inside the generic body, dictionary forwarding, and
unwrapping a returned `T` to its concrete call-site representation. Default
methods lower once per implementation target and may call other trait methods.
Suspending trait methods use typed Wasm GC wrappers around erased concrete
frames, including static, dynamic, trait-bound, stored, default-method, and
cancellation paths. Mutable trait receivers retain `mut self` in typed HIR and
require mutable paths through static, dynamic, default-method, and bounded
generic calls. Imported `std.resource.ResourceError[E]` lowers as a canonical
generic `Operation(E) | Disposed` Wasm GC enum. Inherent `impl Type:` methods
share the direct function ABI, including mutable and suspending receivers;
bare embedded data fields promote unambiguous inherent methods, and bodyless
explicit trait implementations may forward one compatible readonly promoted
method while rejecting mutable promotion through the readonly edge;
associated functions and generic methods remain deferred. Associated trait
members remain. Generic data declarations now
use one erased GC layout,
infer their type arguments at construction, preserve instantiated types in HIR,
and box or unbox exact generic fields at storage boundaries. Generic enums use
the same rule for payloads and recursive fields. Homogeneous `list[T]` values
use a growable GC vector with typed literals, indexing, `len()`, mutable
`append()`, and erased element storage in HIR. `map[K, V]` uses a GC object
with insertion-ordered erased arrays,
growable insertion, duplicate replacement, optional indexed or `get()` lookup,
mutable `remove()`, and built-in scalar or string keys.

S5 has begun with record/replay at the deterministic suspension poll boundary.
Each event carries a function-name-based site identity that survives unrelated
declaration insertion, a source-derived function code identity, runtime
provider key, encoded poll argument and result, and provider-configuration
identity. Replay bypasses live pending decisions, rejects changed executed
functions and incompatible configurations, and is available from the Node API and
`hd record`/`hd replay` sidecar commands. Explicit source labels and
provider-method
arguments and results will join the same log when suspending host-provider
dispatch lands.

Every development command listed below is covered through the packaged `hd`
entrypoint, including WAT and binary builds, HIR and requirement inspection,
tracing, and an isolated record/replay round trip.

This document records the implementation plan for a fast semantic MVP. It is
not part of the language specification. The MVP exists to make critical
semantic choices with executable evidence while compiling to Wasm GC from the
first end-to-end slice.

## Goals

The MVP must:

1. compile accepted hd-lang programs to genuine Wasm GC modules;
2. keep the edit, compile, validate, and run loop below one second for small
   programs where practical;
3. expose typed IR, generated WAT, normalized requirement rows, and suspension
   traces for inspection;
4. make competing semantic choices cheap to test and remove; and
5. reject unimplemented language features with structured diagnostics instead
   of guessing their behavior.

Full conformance, optimization, packaging, and a production host ABI are not
MVP goals.

## Toolchain

| Piece | Initial choice | Reason |
| --- | --- | --- |
| Compiler | TypeScript on Node | Keeps compilation, validation, execution, and host providers in one process. |
| Wasm emission | Generate WAT and parse it with pinned `binaryen.js` | WAT is readable and diffable; Binaryen validates and emits the binary. |
| Development runtime | Node's V8 | Runs Wasm GC and makes fixture providers ordinary JavaScript imports. |
| Parser | Hand-written recursive descent with Pratt expression parsing | Produces a useful AST directly and keeps diagnostics under compiler control. |
| Syntax oracle | `spec/reference_parser.py` | Detects disagreement with the normative chapter-02 grammar and lexical rules. |

The implementation must pin the Node and Binaryen versions. `wasm-tools` and
Wasmtime are compatibility checks and later runtime candidates, not required
subprocesses in the inner development loop.

## S-1: Toolchain Gate

Before building the front end, generate a small WAT module that uses:

- a GC struct and GC array;
- nullable references and a reference cast;
- an `externref` import;
- a mutable GC global that retains a frame; and
- exported allocation and field-access functions.

Parse and validate the text with Binaryen, emit a binary, validate it with
Node, instantiate it, and verify retained references under repeated
allocation. This gate must pass on every supported development platform.

GC values remain internal to the Wasm module. The initial JavaScript boundary
uses scalars, `externref`, and encoded buffers; it does not depend on JavaScript
constructors for Wasm GC structs or arrays.

## Representation Shortcuts

- Primitive values use their corresponding Wasm scalar types.
- Each data declaration becomes a GC struct.
- Enums initially use tagged GC structs; a subtype representation may be
  evaluated later when it serves a semantic or performance question.
- Strings use UTF-8 in a GC byte array with a small runtime library.
- Ordinary generics use the specification's uniform `anyref` erasure and
  dictionary passing. Primitive values are boxed in erased positions.
- Development suspension frames store every local. Liveness analysis is
  deferred.
- A panic emits `unreachable`; the runner discards the poisoned instance.
- Development builds run no optimizer. Binaryen optimization is reserved for
  explicit release and size experiments.
- Wasm stack switching is not used. Suspending functions lower to explicit,
  stackless state machines.

## Implementation Slices

### S0: Front End

Implement the chapter-01 lexer and layout model, the subset parser, source
spans, an AST, and structured parse diagnostics. Check supported syntax against
the reference parser.

Done when the selected parse fixtures agree with `cases.tsv` and AST snapshots
are stable.

### S1: Core End To End

Implement name binding, a checker for the core subset, typed IR, and Wasm GC
lowering for:

- scalar expressions and control flow;
- functions and closures;
- data, enums, and `match`;
- `T?`, `Result`, and `?`;
- panics; and
- non-suspending `defer`.

Done when curated core runtime cases compile to Wasm GC and execute in Node.
The full `runtime/valid` directory is not an S1 requirement because some cases
depend on later slices.

### S2: Requirements

Lower each concrete requirement row to hidden provider inputs. Implement
`$.use`, `$.with`, `$.context`, concrete row normalization, row variables,
union, and subtraction. Initially, providers may be host-supplied `externref`
values; general user-defined dynamic trait values are deferred.

Add an explanation view that prints the transitive requirements and their call
paths.

Done when the chapter-11 provider examples run and the selected missing-row,
subtraction, and key-collision cases produce their specified diagnostics.

### S3: Suspension

Lower every `fn!` to a constructor, GC frame struct, state discriminant,
`poll`, and `cancel`. A frame stores its arguments, construction-time provider
bindings, locals, active child, and registered cleanup state.

Implement cold construction, bang calls, stored suspension driving, one-shot
and exclusive-driving checks, synchronous cancellation, and `defer` cleanup in
that order. The `std.task` combinators, starting with `all!` and `race!`, are
compiler intrinsics; lower them as generated frames rather than library code. Host fixtures provide deterministic pending and ready operations.

Done when provider capture occurs at construction and cancellation runs
registered `defer` suites from the innermost unfinished frame outward.

### S4: Traits And Erased Generics

Implement erased generic calls, trait dictionaries, dynamic trait values, and
the comparison traits needed by ordinary collections. Build `list[T]` and
`map[K, V]` only to the extent required by chosen examples.

Done when the selected trait fixtures and representative erased collection
programs run through Wasm GC.

### S5: Semantic And Replay Experiments

Instrument suspending provider calls with an event identity, provider key,
operation, encoded arguments and result, and provider-configuration identity.
Support record and replay in the Node runner.

Temporary flags such as `--sem row-subtraction=<variant>` may compare unresolved
rules. Each flag must correspond to explicit fixtures. After a decision, remove
the alternatives, update the owning specification and `OPEN_ISSUES.md`, and
retain accepted and rejected conformance cases.

Prioritize experiments for row subtraction, suspension abandonment,
cancellation, replay site identity, and provider compatibility during replay.

### S6: Annotations

Generate shape constructors at compile time and materialize registry entries
lazily. Start this slice only when annotation semantics become the active
design question.

## Diagnostics And Tests

Diagnostics are implemented with the slice that introduces their behavior;
they are not postponed to a separate compiler phase. Every diagnostic has a
stable code and a fixture.

The MVP uses a curated case index drawn from `spec/conformance/cases.tsv`.
Coverage expands with each slice. One command runs the curated cases, relevant
example blocks, and `spec/check.sh`. Full conformance becomes a milestone only
after the semantic MVP has answered its critical design questions.

Useful development commands should include:

```text
hd run FILE
hd check FILE
hd build --wat FILE
hd dump-hir FILE
hd explain-requirements FILE
hd trace FILE
```

## Deferred Work

The MVP defers GADTs, packs, declared variance, multiple packages, dependency
resolution, the LSP, formatting, production optimization, the final WASI and
Component Model boundary, and complete annotation support. A deferred feature
enters the MVP only when it is necessary to settle an active semantic question.
GADTs, packs, and declared variance remain in the language and are planned
soon after the MVP.
