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
npm run hd -- repl
npm run hd -- check --format json examples/core.hd
npm run hd -- explain unknown-data-field
npm run hd -- doc main examples/core.hd
npm run check
```

`hd repl` starts an interactive session. Each input is a declaration, a
statement, or an expression; expressions print their value and type. A line
ending in `:` starts a block, which an empty line ends. The session is kept as
one program (`:source` shows it): declarations at the top level and statements
in a synthesized `pub fn main() -> void $ Console`. Every input recompiles and
reruns that program, skipping console output already shown, so declarations
cannot see REPL bindings and suspending calls are not available. `:type EXPR`,
`:reset`, `:help`, and `:quit` are the commands. The session and its
commands live in `repl.ts` and its input rules in `repl-input.ts`; both run
in a browser too. `repl-terminal.ts` adds the terminal front end. The
website's REPL panel runs the same session in the playground's compiler
worker ([`../playground/README.md`](../playground/README.md)).
In a terminal the REPL colors the line being typed, printed values, and
`:source` output (`src/highlight.ts`), and colors errors and warnings. Set
`NO_COLOR` or `TERM=dumb` to turn coloring off; piped input is never colored.

The package also exposes `bin/hd.js` as the `hd` executable when installed or
linked through npm.

## Agent Queries

hd is written mostly by coding agents, so the CLI answers questions about a
program by name and in JSON, not by file position
([roadmap area 8](../future-work/ROADMAP.md#8-tooling-for-agents)).
Every command below also has the default `--format text`, which is the only
format a person needs.

### Machine-Readable Diagnostics

`--format json` is accepted by every command that compiles a file: `parse`,
`check`, `test`, `run`, `trace`, `record`, `replay`, `build`, `dump-hir`, and
`explain-requirements`. It changes only the diagnostic stream: each
diagnostic the text format would print goes to stderr as one JSON object per
line (JSON Lines), in the same order. Stdout keeps what the command prints,
such as the `ok` line or program output, and exit codes do not change.

```sh
hd check --format json app.hd 2> diagnostics.jsonl
```

```json
{
  "kind": "diagnostic",
  "code": "old-struct-declaration",
  "severity": "error",
  "message": "'struct' was replaced by 'data'",
  "file": "app.hd",
  "span": {
    "start": { "line": 1, "column": 1, "offset": 0 },
    "end": { "line": 1, "column": 7, "offset": 6 }
  },
  "notes": [],
  "related": [],
  "fix": {
    "message": "replace 'struct' with 'data'",
    "edits": [
      {
        "span": {
          "start": { "line": 1, "column": 1, "offset": 0 },
          "end": { "line": 1, "column": 7, "offset": 6 }
        },
        "replacement": "data"
      }
    ]
  },
  "rule": "data.decl.no-struct",
  "rules": [
    { "id": "data.decl.no-struct", "anchor": "spec/08-data-and-enums.md#r-data.decl.no-struct" }
  ]
}
```

(Each record is printed on one line; it is spread out here to read.)

| Field | Meaning |
| --- | --- |
| `kind` | `diagnostic` for compiler diagnostics, `runtime-panic` for a panic while running, `entry-error` when a `Result`-returning `main` returns `Err`. |
| `code` | The stable code from [spec/README.md](../spec/README.md#diagnostics) or the panic category; `null` only for `entry-error`. |
| `severity` | `error` or `warning`. |
| `message`, `notes` | The prose the text format prints. |
| `file` | The path as given on the command line. |
| `span` | Primary location. Lines and columns are 1-based, columns count UTF-16 code units, `offset` is the 0-based UTF-16 offset, and `end` is exclusive. `null` for runtime records. |
| `related` | Secondary locations as `{message, file, span}`. |
| `fix` | `{message, edits}` when the prototype knows the one correct edit, else `null`. An edit replaces its `span` with `replacement`; an empty span inserts and an empty replacement deletes. |
| `rule` | The rule ID naming this code, when exactly one rule does; else `null`. |
| `rules` | Every rule whose text names this code, as `{id, anchor}`. |

Fixes are suggested only where the diagnostic's own message names the
replacement: `old-struct-declaration`, `old-import-declaration`,
`old-export-declaration`, `unexpected-bom`, and `missing-let`
(`diagnostic-report.ts`). A producer may also attach a `fix` or `related`
spans to a `Diagnostic` directly.

Rule IDs are not kept in a table. `spec-index.ts` reads the specification
each time a command needs it and finds each rule ID marker (`r[data.field.unique]`
opening a list item, paragraph, quote, or table cell, as
[Rule IDs](../spec/STYLE.md#rule-ids) defines it). A rule names a code
with "Error: `code`." or "Warning: `code`." or "is a `code` error". So as more
chapters gain rule IDs, `rule` and `rules` fill in without code changes.
`HD_SPEC_DIR` points the index at another specification directory; the
tests use it.

### `hd explain CODE`

`hd explain` prints what the specification says about a diagnostic code or
runtime panic category: its severity row, its normative meaning when the
[general-code table](../spec/README.md#diagnostics) has one, the rules that
name it, the chapter sections that mention it, and the conformance fixtures
that exercise it. It exits 1 for a code the specification never names.

```text
$ hd explain unknown-data-field
unknown-data-field: error (general)
  A data literal, pattern, or field access names a field the data type does not declare, ...
  (spec/README.md#diagnostics)

mentioned in:
  spec/03-names-and-scopes.md#member-resolution  line 457
  ...

fixtures:
  spec/conformance/typing/invalid/data-literal-unknown-field.hd  type reject:unknown-data-field
  ...
```

`--format json` prints one object with `code`, `known`, `category`,
`meaning`, `meaningSource`, `rules` (`id`, `anchor`, `file`, `line`, `text`),
`mentions` (`anchor`, `heading`, `file`, `line`, `rule`), and `fixtures`
(`path`, `phase`, `expectation`, `specification`).

### `hd def NAME [PATH]` And `hd doc NAME [PATH]`

These resolve a symbol by name in a project. `PATH` is one `.hd` file or a
package directory with a `src/` tree, as in
[the package linker](../playground/README.md#packages-and-modules); it
defaults to the current directory.

- In a package, `pkg.user.User` names `User` in `src/user.hd` or
  `src/user/mod.hd`, and `pkg.User` names an item of `src/mod.hd`, including
  one it re-exports with `pub use`. The `pkg.` root may be left out; a name
  with no module path is searched in every module. In a single file the name
  is just the item path.
- After the item come member segments: `User.email` (field), `User.greet`
  (method from any `impl`), `Show.show` (trait method), `Show.Output`
  (associated type), `Status.Banned` (variant), and `Status.Banned.reason`
  (payload field). `Type::function` is accepted for `Type.function`.

`hd def` prints each match's location, kind, qualified name, and signature.
`hd doc` adds the doc comment, the fields, variants, associated types, and
methods (with the trait each method implements), and the traits a type
implements or the implementations of a trait. For a single file that
type-checks, results, requirement rows, and binding types the source omits
are filled in from the checker and marked inferred.

```text
$ hd def pkg.user.User project
project/src/user/mod.hd:2:1: data pkg.user.User
  pub data User
```

Lookups read parsed modules only, so they answer while a program still has
type errors. A module that does not parse reports its diagnostics (in the
chosen format) and is skipped. When nothing matches, the command exits 1 and
lists the qualified names that end in the query's last segment.

`--format json` prints `{"query", "symbols", "suggestions"}`. Each symbol has
`name`, `kind` (`function`, `data`, `enum`, `trait`, `binding`, `field`,
`variant`, `method`, `associated-function`, or `associated-type`), `module`,
`file`, `span`, `public`, `signature`, and `doc`, plus as they apply `owner`,
`trait`, `type`, `embedded`, `parameters` (`name`, `type`, `variadic`,
`default`), `result`, `requirements`, `suspending`, `omitted`, `hasDefault`,
`members`, `implementations` (`trait`, `target`, `file`, `span`),
`supertraits`, and `via` (the re-exported name the query matched).
Promoted members of embedded fields and blanket implementations are not
listed yet.

`hd explain-requirements --format json` prints the same facts as its text
form: `{"functions": [{"functionName", "declared", "paths": [{"key", "path"}]}]}`.

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
- homogeneous `T...` parameters lowered as `List[T]`, with positional values,
  positional list spread, and named-list supply across ordinary, generic,
  suspending, static-trait, and dynamic-trait calls;
- first-class homogeneous-vararg function types and indirect calls using the
  same `List[T]` ABI;
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
- tuple patterns, nested in any pattern, and exhaustiveness by
  pattern-matrix usefulness over bool, optionals, `Result`, enums, tuples,
  and data;
- contextual enum patterns and recursive nominal data patterns with field
  bindings and literal field constraints, plus named enum-payload bindings
  resolved independently of source order and literal, nested-data, or
  nested-enum payload constraints;
- erased optional and `Result` values, contextual constructors, exhaustive
  matching, recursive nominal payload patterns, must-use checking, and postfix
  propagation; `Option[T]` is `T?`, with `.None`, `.Some(value)`, and their
  `Option.`-qualified forms as constructors and patterns over the erased
  carrier;
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
- embedded data fields (value embedding): promotion of `pub` fields and inherent methods
  (depth at most 3), with generic substitution through each embedded field; an
  embedded field follows its container's access, so through `mut C` a
  promoted field may be assigned and a promoted `mut self` method called;
  every fill copies (`Label: ...value`, copy-update, `place ...= value`), with
  one generated `$hd.copy_d<N>` per embedded data type that copies ordinary
  fields shallowly and parts recursively; a copy of a readonly value whose
  type has mutable edges is readonly (`mutable-upgrade` where `mut` is
  needed); trait conformance through a part is explicit delegation,
  `impl Trait for C by E`;
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
  lexicographic tuple/list plus `.None`-first optional ordering, recursively using
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
- in-process package linking (`package.ts`, used by the browser playground,
  not the CLI): `pkg`, `self`, and `super` uses between the modules of one
  package resolve to public declarations and `pub use` re-exports, and the
  modules reachable from the entry are joined into one program in
  initialization order. Linked modules share one top-level namespace, and
  namespace or renaming uses of package declarations are not supported
  (`../playground/README.md#packages-and-modules`);
- imported `std.resource.ResourceError[E]` as the canonical generic
  `Operation(E) | Disposed` enum, using the same erased Wasm GC representation
  as source-declared generic enums;
- imported `std.convert.From[T]` and `std.error.Error` as trait declarations
  in the compiled module; `?` on a `Result` converts the error by one
  assignability rule or one `From` call, `Type::from(x)` selects the `From`
  instantiation by argument type, and a single-payload variant constructor
  is a function value (a dynamic trait value does not yet satisfy a bound on
  its own trait);
- runtime type identity: importing a `std.inspect` name or `std.error.Error`
  declares the sealed `Inspectable` (`std.error.Error` extends it) and
  `TypeId`, a data type holding the canonical printable name (an inner
  `mut` kept, the outer `mut` dropped); every
  inspectable type erases to `Inspectable` or `mut Inspectable` through a
  generated dictionary whose `runtime_type` builds that name, splicing in the
  names carried by `T < Inspectable` dictionaries; `downcast`, `downcast_mut`,
  `downcast_val`, and `TypeId::of` are checker intrinsics that compare names
  and unwrap the stored payload; `impl Inspectable`, a redeclared or
  implemented `runtime_type`/`downcast`/`downcast_mut`, and an Inspectable
  requirement key in a function's requirement clause are rejected. Not
  covered: `Hash` for `TypeId`
  (no `Hash` trait, F-255), `Any` as a type argument (F-255), a type
  parameter bounded only by a subtrait of `Inspectable`, Inspectable keys in
  closure types and provider scopes, qualified printable names (the
  prototype has one module), and opaqueness (`TypeId { key: ... }` is
  constructible). A type parameter instantiated with `mut U` looks up
  implementations for `U`, and its Inspectable dictionary adds the inner
  `mut` when a composite key is built from it;
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
- White_Space `string.trim()` and default-case `string.lower()` through a
  bytewise host bridge that reconstructs the result as a Wasm GC byte array;
  every host-boundary decoder keeps a leading U+FEFF;
- `string.split()` and `string.replace()` implemented in WAT, retaining
  boundary empty pieces, splitting an empty separator into Unicode scalar
  strings, and inserting an empty `old`'s replacement at scalar boundaries;
- non-suspending `defer` on normal completion, return, break, and continue;
- homogeneous `List[T]` literals, indexing, `len()`, and mutable `append()` over
  a growable Wasm GC vector with erased backing storage, plus indexed
  replacement through `mut List[T]`;
- insertion-ordered `Map[K, V]` literals with duplicate replacement, optional
  indexed or `get()` lookup, `len()`, growable indexed insertion and
  `remove()` through `mut Map[K, V]`, and erased Wasm GC key/value storage;
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
Interpolation and `println` report `unsatisfied-trait-bound` when the displayed type
does not implement the canonical prelude trait.

## Layout

- `lexer.ts` and `ast.ts` define the small source-frontend stages.
- `parser/` builds the AST and exposes its public API from `parser/index.ts`.
- `checker/` resolves names and produces the typed nodes in `hir.ts`.
- `emitter/` lowers HIR to readable WAT and exposes only `emitter/index.ts`.
- `suspension.ts` lowers suspending HIR into explicit resumable control flow.
- `wasm.ts` parses, validates, and emits Wasm with pinned Binaryen.
- `compiler.ts` exposes the in-process compiler API.
- `package.ts` links the modules of a multi-file package into one program.
- `requirements.ts` computes transitive provider explanations.
- `cli.ts` implements the current command-line interface; `cli-queries.ts`
  implements `explain`, `def`, and `doc`.
- `diagnostic-report.ts` writes diagnostics as text or JSON Lines and derives
  suggested fixes.
- `spec-index.ts` indexes rule IDs, diagnostic codes, and fixtures from the
  specification sources.
- `symbols.ts` resolves name-addressed symbol lookups over parsed modules.
- `toolchain-gate.ts` proves the required Wasm GC operations independently of
  the language frontend.
- `../test/portable/cases.tsv` selects portable `.hd` conformance fixtures;
  `../test/run-portable.ts` runs them through `hd parse`, `hd check`, and
  `hd test` without importing compiler internals.
- `../test/cli.test.ts` exercises the packaged CLI surface end to end, and
  `../test/agent-tooling.test.ts` the JSON diagnostics, `explain`, `def`, and
  `doc`.
